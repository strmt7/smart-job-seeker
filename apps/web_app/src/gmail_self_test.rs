//! A user-triggered, fixed self-addressed connectivity test, never an application.
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::Mutex;
use waypoint_workspace::Workspace;
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
struct Credentials {
    auth_type: String,
    email: String,
    password: String,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}
struct Task {
    id: String,
    result: Option<Result<(), String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

fn valid_address(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && domain.eq_ignore_ascii_case("gmail.com")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}

fn transmit(stream: &mut (impl Read + Write), address: &str, id: &str) -> Result<(), String> {
    if !valid_address(address) || !id.bytes().all(|b| b.is_ascii_hexdigit()) || id.is_empty() {
        return Err("Cannot use these credentials for the test message".into());
    }
    let message = format!("From: {address}\r\nTo: {address}\r\nSubject: Waypoint connectivity test\r\nMessage-ID: <waypoint-test-{id}@localhost>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nThis is a self-addressed Waypoint connectivity test. It is not a job application.\r\n");
    crate::smtp_handoff::handoff(stream, address, address, message.as_bytes()).map_err(
        |error| match error {
            crate::smtp_handoff::HandoffError::InvalidMessage => {
                "Cannot prepare the Gmail test message"
            }
            crate::smtp_handoff::HandoffError::BeforeData => {
                "Gmail rejected or could not receive the test message before handoff"
            }
            crate::smtp_handoff::HandoffError::Uncertain => {
                "Delivery is uncertain; check the mailbox before trying again"
            }
        },
    )?;
    let _ = stream.write_all(b"QUIT\r\n");
    Ok(())
}

pub fn start(id: String, ws: &Workspace) -> Result<(), String> {
    let bytes = Zeroizing::new(
        ws.gmail_credentials()
            .map_err(|_| "Cannot read Gmail credentials")?
            .ok_or("Connect Gmail first")?,
    );
    let credentials: Credentials = serde_json::from_slice(&bytes)
        .map_err(|_| "Self-test currently requires a Gmail app-password connection")?;
    if credentials.auth_type != "app_password" || !valid_address(&credentials.email) {
        return Err("Self-test currently requires a Gmail app-password connection".into());
    }
    let mut task = TASK.lock().map_err(|_| "Gmail self-test unavailable")?;
    if task.is_some() {
        return Err("A Gmail self-test is running or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            let mut stream =
                crate::gmail_password::open_smtp(&credentials.email, &credentials.password)?;
            transmit(&mut stream, &credentials.email, &id)
        })
        .unwrap_or_else(|_| {
            Err("Test stopped unexpectedly; delivery may be uncertain, check the mailbox".into())
        });
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str) -> Result<Value, String> {
    let mut task = TASK.lock().map_err(|_| "Gmail self-test unavailable")?;
    let task_ref = task
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Gmail self-test not found; check your mailbox before retrying")?;
    let Some(result) = task_ref.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    *task = None;
    Ok(match result {
        Ok(()) => json!({"status":"accepted"}),
        Err(error) => json!({"status":"failed", "error":error}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        responses: std::io::Cursor<Vec<u8>>,
        written: Vec<u8>,
    }
    impl Read for Fixture {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.responses.read(buf)
        }
    }
    impl Write for Fixture {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn sends_only_the_fixed_self_addressed_message_and_reports_uncertain_handoff() {
        let mut fixture = Fixture {
            responses: std::io::Cursor::new(
                b"250 ok\r\n250 ok\r\n354 go\r\n250 accepted\r\n".to_vec(),
            ),
            written: vec![],
        };
        transmit(&mut fixture, "example@gmail.com", "abc123").unwrap();
        let wire = String::from_utf8(fixture.written).unwrap();
        assert!(wire.contains("MAIL FROM:<example@gmail.com>\r\nRCPT TO:<example@gmail.com>"));
        assert!(wire.contains("It is not a job application."));
        let mut lost = Fixture {
            responses: std::io::Cursor::new(b"250 ok\r\n250 ok\r\n354 go\r\n".to_vec()),
            written: vec![],
        };
        assert!(transmit(&mut lost, "example@gmail.com", "abc123")
            .unwrap_err()
            .contains("uncertain"));
        assert!(!valid_address(
            "victim@example.com\r\nRCPT TO:<other@example.com>"
        ));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "sends one fixed test email to the supplied Gmail account itself"]
    fn live_self_test_sends_and_finds_the_message_in_the_test_inbox() {
        let email = Zeroizing::new(
            std::env::var("WAYPOINT_TEST_GMAIL_USER").expect("test account required"),
        );
        let password = Zeroizing::new(
            std::env::var("WAYPOINT_TEST_GMAIL_PASSWORD").expect("test password required"),
        );
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        let encoded = Zeroizing::new(serde_json::to_vec(&json!({"auth_type":"app_password","email":email.as_str(),"password":password.as_str()})).unwrap());
        ws.save_gmail_credentials(&encoded).unwrap();
        let id = crate::random_id().unwrap();
        start(id.clone(), &ws).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            let result = poll(&id).unwrap();
            match result["status"].as_str() {
                Some("accepted") => break,
                Some("running") if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(250))
                }
                _ => panic!(
                    "Self-test failed: {}",
                    result["error"].as_str().unwrap_or("timeout; check inbox")
                ),
            }
        }
        let mut imap = crate::gmail_password::open_imap(&email, &password).unwrap();
        imap.write_all(b"a002 EXAMINE INBOX\r\n").unwrap();
        loop {
            let line = crate::gmail_password::imap_line(&mut imap).unwrap();
            if line.starts_with("a002 ") {
                assert!(line.starts_with("a002 OK"));
                break;
            }
        }
        let mut found = false;
        for _ in 0..6 {
            imap.write_all(
                format!("a003 SEARCH HEADER Message-ID \"<waypoint-test-{id}@localhost>\"\r\n")
                    .as_bytes(),
            )
            .unwrap();
            loop {
                let line = crate::gmail_password::imap_line(&mut imap).unwrap();
                if let Some(matches) = line.strip_prefix("* SEARCH") {
                    found = !matches.trim().is_empty();
                }
                if line.starts_with("a003 ") {
                    assert!(line.starts_with("a003 OK"));
                    break;
                }
            }
            if found {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        assert!(
            found,
            "SMTP accepted the test but it was not found in the inbox"
        );
        let _ = imap.write_all(b"a004 LOGOUT\r\n");
        ws.clear_gmail_credentials().unwrap();
    }
}
