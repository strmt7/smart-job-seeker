//! Read-only inbox counts for an explicitly connected Gmail account.
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Mutex;
use waypoint_domain::Sha256Digest;
use waypoint_workspace::Workspace;
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
struct Credentials {
    auth_type: String,
    email: String,
    password: String,
}
enum Connection {
    AppPassword(Credentials),
    OAuth(crate::gmail_oauth::Credentials),
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

fn connection(bytes: &[u8]) -> Result<Connection, String> {
    if let Ok(credentials) = serde_json::from_slice::<Credentials>(bytes) {
        if credentials.auth_type == "app_password" {
            return Ok(Connection::AppPassword(credentials));
        }
        return Err("Unsupported Gmail credential type".into());
    }
    serde_json::from_slice::<crate::gmail_oauth::Credentials>(bytes)
        .map(Connection::OAuth)
        .map_err(|_| "Stored Gmail credentials are invalid".into())
}

fn label_counts(value: &Value) -> Result<(u64, u64), String> {
    if value["id"].as_str() != Some("INBOX") {
        return Err("Gmail returned another label".into());
    }
    let total = value["messagesTotal"]
        .as_u64()
        .ok_or("Gmail did not report message count")?;
    let unread = value["messagesUnread"]
        .as_u64()
        .ok_or("Gmail did not report unread count")?;
    if unread > total {
        return Err("Gmail returned inconsistent inbox counts".into());
    }
    Ok((total, unread))
}

// OAuth refresh adapted from rejection-rejector commit 48c672b29cf098ff5da10d324de3e87766a2f483;
// license and attribution are retained in docs/REJECTION_REJECTOR_LICENSE.md.
fn oauth_counts(credentials: &crate::gmail_oauth::Credentials) -> Result<(u64, u64), String> {
    let client =
        crate::gmail_net::client(30, false).map_err(|_| "Cannot initialize Gmail connection")?;
    oauth_counts_with(
        &client,
        credentials,
        crate::gmail_oauth::TOKEN_URL,
        "https://gmail.googleapis.com/gmail/v1/users/me/labels/INBOX",
    )
}

fn oauth_counts_with(
    client: &reqwest::blocking::Client,
    credentials: &crate::gmail_oauth::Credentials,
    token_url: &str,
    inbox_url: &str,
) -> Result<(u64, u64), String> {
    let response = client
        .post(token_url)
        .form(&[
            ("client_id", credentials.client_id.as_str()),
            ("client_secret", credentials.client_secret.as_str()),
            ("refresh_token", credentials.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .map_err(|_| "Gmail authorization refresh failed")?;
    let tokens: Zeroizing<crate::gmail_oauth::Tokens> = Zeroizing::new(
        crate::gmail_net::json(response, 32768)
            .map_err(|_| "Gmail authorization refresh failed")?,
    );
    if tokens.access_token.is_empty() {
        return Err("Gmail authorization returned no access token".into());
    }
    if tokens
        .refresh_token
        .as_deref()
        .is_some_and(|value| value != credentials.refresh_token)
    {
        return Err("Gmail rotated the refresh credential; reconnect the account".into());
    }
    let response = client
        .get(inbox_url)
        .bearer_auth(&tokens.access_token)
        .send()
        .map_err(|_| "Gmail inbox status request failed")?;
    let value: Value =
        crate::gmail_net::json(response, 32768).map_err(|_| "Gmail inbox status request failed")?;
    label_counts(&value)
}
struct Task {
    id: String,
    digest: String,
    result: Option<Result<(u64, u64), String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

fn reserve(tasks: &mut Option<Task>, task: Task) -> Result<(), String> {
    if tasks
        .as_ref()
        .is_some_and(|current| current.result.is_none())
    {
        return Err("An inbox status check is already running".into());
    }
    // A new explicit check supersedes a completed count that was never collected.
    *tasks = Some(task);
    Ok(())
}

pub fn start(id: String, ws: &Workspace) -> Result<(), String> {
    let bytes = Zeroizing::new(
        ws.gmail_credentials()
            .map_err(|_| "Cannot read Gmail credentials")?
            .ok_or("Connect Gmail first")?,
    );
    let credentials = connection(&bytes)?;
    let digest =
        Sha256Digest::of(std::str::from_utf8(&bytes).map_err(|_| "Invalid Gmail credentials")?)
            .as_str()
            .to_owned();
    let mut tasks = TASK.lock().map_err(|_| "Gmail inbox status unavailable")?;
    reserve(
        &mut tasks,
        Task {
            id: id.clone(),
            digest,
            result: None,
        },
    )?;
    drop(tasks);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| match &credentials {
            Connection::AppPassword(credentials) => {
                crate::gmail_password::inbox_counts(&credentials.email, &credentials.password)
            }
            Connection::OAuth(credentials) => oauth_counts(credentials),
        })
        .unwrap_or_else(|_| Err("Gmail inbox status check stopped unexpectedly".into()));
        if let Ok(mut tasks) = TASK.lock() {
            if let Some(task) = tasks.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str, ws: &Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Gmail inbox status unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Inbox status task not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    let expected = task.digest.clone();
    *tasks = None;
    let current = ws
        .gmail_credentials()
        .map_err(|_| "Cannot read Gmail credentials")?
        .map(Zeroizing::new);
    let unchanged = current
        .as_ref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .is_some_and(|value| Sha256Digest::of(value).as_str() == expected);
    if !unchanged {
        return Ok(json!({"status":"failed","error":"Gmail connection changed; check again"}));
    }
    match result {
        Ok((messages, unseen)) => {
            Ok(json!({"status":"complete","messages":messages,"unseen":unseen}))
        }
        Err(error) => Ok(json!({"status":"failed","error":error})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    fn task(id: &str, result: Option<Result<(u64, u64), String>>) -> Task {
        Task {
            id: id.into(),
            digest: "fixture".into(),
            result,
        }
    }
    #[test]
    fn new_check_can_replace_abandoned_result_but_not_running_work() {
        let mut tasks = Some(task("old", Some(Ok((3, 1)))));
        reserve(&mut tasks, task("new", None)).unwrap();
        assert_eq!(tasks.as_ref().unwrap().id, "new");
        assert!(reserve(&mut tasks, task("other", None)).is_err());
        assert_eq!(tasks.as_ref().unwrap().id, "new");
    }
    #[test]
    fn oauth_label_counts_require_complete_consistent_inbox_data() {
        assert_eq!(
            label_counts(&json!({"id":"INBOX","messagesTotal":10,"messagesUnread":3})).unwrap(),
            (10, 3)
        );
        assert!(
            label_counts(&json!({"id":"OTHER","messagesTotal":10,"messagesUnread":3})).is_err()
        );
        assert!(label_counts(&json!({"id":"INBOX","messagesTotal":2,"messagesUnread":3})).is_err());
        assert!(label_counts(&json!({"id":"INBOX","messagesTotal":10})).is_err());
        assert!(matches!(connection(br#"{"client_id":"fictional","client_secret":"fictional","refresh_token":"fictional","can_send":false}"#).unwrap(), Connection::OAuth(_)));
    }

    #[test]
    fn oauth_refresh_and_inbox_count_use_read_only_requests() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(key, value)| {
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                if index == 0 {
                    assert!(headers.starts_with("POST /token "));
                } else {
                    assert!(headers.starts_with("GET /inbox "));
                    assert!(headers
                        .to_ascii_lowercase()
                        .contains("authorization: bearer fictional-access"));
                }
                while request.len() < header_end + length {
                    let mut chunk = [0; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                }
                let body = if index == 0 {
                    r#"{"access_token":"fictional-access","expires_in":3600}"#
                } else {
                    r#"{"id":"INBOX","messagesTotal":7,"messagesUnread":2}"#
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let client = crate::gmail_net::client(5, true).unwrap();
        let creds = crate::gmail_oauth::Credentials {
            client_id: "fictional".into(),
            client_secret: "fictional".into(),
            refresh_token: "fictional".into(),
            can_send: false,
        };
        let counts = oauth_counts_with(
            &client,
            &creds,
            &format!("http://{addr}/token"),
            &format!("http://{addr}/inbox"),
        )
        .unwrap();
        assert_eq!(counts, (7, 2));
        worker.join().unwrap();
    }
}
