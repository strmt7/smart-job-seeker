//! User-initiated Gmail app-password authentication. This module never sends mail.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use waypoint_workspace::Workspace;
use zeroize::Zeroizing;

struct Task {
    id: String,
    cancelled: Arc<AtomicBool>,
    result: Option<Result<Zeroizing<Vec<u8>>, String>>,
    finished_at: Option<Instant>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
const RESULT_LIFETIME: Duration = Duration::from_secs(600);

fn expire_finished(task: &mut Option<Task>, now: Instant) {
    if task
        .as_ref()
        .and_then(|task| task.finished_at)
        .is_some_and(|finished| now.saturating_duration_since(finished) >= RESULT_LIFETIME)
    {
        *task = None;
    }
}

pub fn busy() -> bool {
    TASK.lock()
        .map(|mut task| {
            expire_finished(&mut task, Instant::now());
            task.is_some()
        })
        .unwrap_or(true)
}

fn reply(stream: &mut impl Read, expected: u16) -> Result<(), String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    for _ in 0..30 {
        let mut line = Vec::new();
        for _ in 0..2048 {
            if std::time::Instant::now() >= deadline {
                return Err("Gmail response timed out".into());
            }
            let mut byte = [0u8; 1];
            stream
                .read_exact(&mut byte)
                .map_err(|_| "Gmail connection closed or timed out")?;
            line.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        if !line.ends_with(b"\n") || line.len() < 4 {
            return Err("Invalid Gmail response".into());
        }
        let code = std::str::from_utf8(&line[..3])
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or("Invalid Gmail response")?;
        if code != expected {
            return Err(format!("Gmail rejected the connection (SMTP {code})"));
        }
        if line[3] == b' ' {
            return Ok(());
        }
        if line[3] != b'-' {
            return Err("Invalid Gmail response".into());
        }
    }
    Err("Gmail response exceeded its limit".into())
}

fn connect(host: &str, port: u16) -> Result<native_tls::TlsStream<TcpStream>, String> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| "Cannot resolve Gmail")?;
    let mut stream = None;
    for address in addresses.take(4) {
        if let Ok(socket) = TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
            stream = Some(socket);
            break;
        }
    }
    let socket = stream.ok_or("Cannot connect to Gmail")?;
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| "Cannot set Gmail timeout")?;
    socket
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| "Cannot set Gmail timeout")?;
    let tls = native_tls::TlsConnector::new().map_err(|_| "Cannot initialize TLS")?;
    tls.connect(host, socket)
        .map_err(|_| "Gmail TLS connection failed".into())
}

fn authenticate_smtp(email: &str, password: &str) -> Result<(), String> {
    let mut tls = connect("smtp.gmail.com", 465)?;
    reply(&mut tls, 220)?;
    tls.write_all(b"EHLO localhost\r\n")
        .map_err(|_| "Cannot contact Gmail")?;
    reply(&mut tls, 250)?;
    let plain = Zeroizing::new(format!("\0{email}\0{password}"));
    let encoded = Zeroizing::new(STANDARD.encode(plain.as_bytes()));
    let command = Zeroizing::new(format!("AUTH PLAIN {}\r\n", encoded.as_str()));
    tls.write_all(command.as_bytes())
        .map_err(|_| "Cannot authenticate with Gmail")?;
    reply(&mut tls, 235)?;
    let _ = tls.write_all(b"QUIT\r\n");
    Ok(())
}

fn imap_line(stream: &mut impl Read) -> Result<String, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    let mut line = Vec::new();
    for _ in 0..4096 {
        if std::time::Instant::now() >= deadline {
            return Err("Gmail IMAP response timed out".into());
        }
        let mut byte = [0u8; 1];
        stream
            .read_exact(&mut byte)
            .map_err(|_| "Gmail IMAP connection closed or timed out")?;
        line.push(byte[0]);
        if byte[0] == b'\n' {
            return String::from_utf8(line).map_err(|_| "Invalid Gmail IMAP response".into());
        }
    }
    Err("Gmail IMAP response exceeded its limit".into())
}

fn open_imap(email: &str, password: &str) -> Result<native_tls::TlsStream<TcpStream>, String> {
    let mut tls = connect("imap.gmail.com", 993)?;
    if !imap_line(&mut tls)?.starts_with("* OK") {
        return Err("Gmail IMAP did not accept the connection".into());
    }
    tls.write_all(b"a001 AUTHENTICATE PLAIN\r\n")
        .map_err(|_| "Cannot contact Gmail IMAP")?;
    if !imap_line(&mut tls)?.starts_with('+') {
        return Err("Gmail IMAP authentication is unavailable".into());
    }
    let plain = Zeroizing::new(format!("\0{email}\0{password}"));
    let encoded = Zeroizing::new(STANDARD.encode(plain.as_bytes()));
    let command = Zeroizing::new(format!("{}\r\n", encoded.as_str()));
    tls.write_all(command.as_bytes())
        .map_err(|_| "Cannot authenticate with Gmail IMAP")?;
    for _ in 0..20 {
        let line = imap_line(&mut tls)?;
        if line.starts_with("a001 OK") {
            return Ok(tls);
        }
        if line.starts_with("a001 ") {
            return Err("Gmail IMAP rejected authentication".into());
        }
    }
    Err("Gmail IMAP authentication response exceeded its limit".into())
}

fn authenticate_imap(email: &str, password: &str) -> Result<(), String> {
    let mut tls = open_imap(email, password)?;
    let _ = tls.write_all(b"a002 LOGOUT\r\n");
    Ok(())
}

fn parse_inbox_status(line: &str) -> Result<(u64, u64), String> {
    if !line.starts_with("* STATUS ") {
        return Err("Unexpected Gmail inbox status".into());
    }
    let (mailbox, data) = line.split_once('(').ok_or("Invalid Gmail inbox status")?;
    let mailbox = mailbox
        .strip_prefix("* STATUS ")
        .ok_or("Invalid Gmail inbox status")?
        .trim()
        .trim_matches('"');
    if !mailbox.eq_ignore_ascii_case("INBOX") {
        return Err("Gmail returned status for another mailbox".into());
    }
    let (data, _) = data.split_once(')').ok_or("Invalid Gmail inbox status")?;
    let mut tokens = data.split_ascii_whitespace();
    let mut messages = None;
    let mut unseen = None;
    while let Some(name) = tokens.next() {
        let value = tokens
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or("Invalid Gmail inbox status")?;
        match name.to_ascii_uppercase().as_str() {
            "MESSAGES" if messages.is_none() => messages = Some(value),
            "UNSEEN" if unseen.is_none() => unseen = Some(value),
            _ => return Err("Unexpected Gmail inbox status".into()),
        }
    }
    let messages = messages.ok_or("Gmail did not report message count")?;
    let unseen = unseen.ok_or("Gmail did not report unread count")?;
    if unseen > messages {
        return Err("Gmail returned inconsistent inbox counts".into());
    }
    Ok((messages, unseen))
}

pub fn inbox_counts(email: &str, password: &str) -> Result<(u64, u64), String> {
    let mut tls = open_imap(email, password)?;
    tls.write_all(b"a003 STATUS INBOX (MESSAGES UNSEEN)\r\n")
        .map_err(|_| "Cannot request Gmail inbox status")?;
    let mut counts = None;
    for _ in 0..20 {
        let line = imap_line(&mut tls)?;
        if line.starts_with("* STATUS ") {
            if counts.is_some() {
                return Err("Gmail returned multiple inbox statuses".into());
            }
            counts = Some(parse_inbox_status(&line)?);
        } else if line.starts_with("a003 OK") {
            let _ = tls.write_all(b"a004 LOGOUT\r\n");
            return counts.ok_or("Gmail did not return inbox status".into());
        } else if line.starts_with("a003 ") {
            return Err("Gmail rejected the inbox status request".into());
        }
    }
    Err("Gmail inbox status response exceeded its limit".into())
}

pub fn start(id: String, email: String, password: String, ws: &Workspace) -> Result<(), String> {
    let password = Zeroizing::new(password);
    if !ws.can_store_gmail_credentials() {
        return Err("Gmail connection requires Windows DPAPI credential storage".into());
    }
    if crate::gmail_connect::busy() {
        return Err("A Gmail connection is already running".into());
    }
    if email.len() > 254
        || email.matches('@').count() != 1
        || !email.to_ascii_lowercase().ends_with("@gmail.com")
        || email.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("Enter a Gmail address".into());
    }
    if password.len() != 16 || !password.bytes().all(|b| b.is_ascii_lowercase()) {
        return Err("Enter the 16-letter Google app password without spaces".into());
    }
    let mut task = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    expire_finished(&mut task, Instant::now());
    if task.is_some() {
        return Err("A Gmail connection is already running".into());
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    *task = Some(Task {
        id: id.clone(),
        cancelled: cancelled.clone(),
        result: None,
        finished_at: None,
    });
    drop(task);
    std::thread::spawn(move || {
        #[derive(serde::Serialize)]
        struct Credentials<'a> {
            auth_type: &'static str,
            email: &'a str,
            password: &'a str,
        }
        let result = std::panic::catch_unwind(|| {
            authenticate_smtp(&email, &password)
                .map_err(|error| format!("SMTP check: {error}"))
                .and_then(|_| {
                    if cancelled.load(Ordering::SeqCst) {
                        return Err("Gmail connection cancelled".into());
                    }
                    authenticate_imap(&email, &password)
                        .map_err(|error| format!("IMAP check: {error}"))
                })
                .and_then(|_| {
                    if cancelled.load(Ordering::SeqCst) {
                        return Err("Gmail connection cancelled".into());
                    }
                    serde_json::to_vec(&Credentials {
                        auth_type: "app_password",
                        email: &email,
                        password: &password,
                    })
                    .map(Zeroizing::new)
                    .map_err(|_| "Cannot encode Gmail credentials".into())
                })
        })
        .unwrap_or_else(|_| Err("Gmail connection stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
                task.finished_at = Some(Instant::now());
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str, ws: &Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    expire_finished(&mut tasks, Instant::now());
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Gmail connection not found")?;
    match task.result.take() {
        None => {
            Ok(json!({"status":"running","cancel_requested":task.cancelled.load(Ordering::SeqCst)}))
        }
        Some(Err(error)) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
        Some(Ok(credentials)) => {
            if task.cancelled.load(Ordering::SeqCst) {
                *tasks = None;
                return Ok(json!({"status":"failed","error":"Gmail connection cancelled"}));
            }
            *tasks = None;
            ws.save_gmail_credentials(&credentials)
                .map_err(|_| "Cannot save Gmail credentials securely")?;
            Ok(json!({"status":"complete"}))
        }
    }
}

pub fn cancel(id: &str) -> Result<(), String> {
    let tasks = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    let task = tasks
        .as_ref()
        .filter(|task| task.id == id)
        .ok_or("Gmail connection not found")?;
    task.cancelled.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smtp_responses_are_bounded_and_status_checked() {
        assert!(reply(&mut &b"250-one\r\n250 two\r\n"[..], 250).is_ok());
        assert!(reply(&mut &b"535 rejected\r\n"[..], 235).is_err());
        assert!(reply(&mut &vec![b'a'; 2049][..], 250).is_err());
        assert!(imap_line(&mut &b"* OK ready\r\n"[..])
            .unwrap()
            .starts_with("* OK"));
        assert!(imap_line(&mut &vec![b'a'; 4097][..]).is_err());
        assert_eq!(
            parse_inbox_status("* STATUS INBOX (MESSAGES 8 UNSEEN 2)\r\n").unwrap(),
            (8, 2)
        );
        assert!(parse_inbox_status("* STATUS INBOX (MESSAGES 1 UNSEEN 2)\r\n").is_err());
        assert!(parse_inbox_status("* STATUS INBOX (MESSAGES 1 MESSAGES 2 UNSEEN 0)\r\n").is_err());
    }

    #[test]
    fn cancelled_completed_connection_does_not_save_credentials() {
        let ws = Workspace::in_memory().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        *TASK.lock().unwrap() = Some(Task {
            id: "cancel-fixture".into(),
            cancelled,
            result: Some(Ok(Zeroizing::new(b"fictional".to_vec()))),
            finished_at: Some(Instant::now()),
        });
        cancel("cancel-fixture").unwrap();
        let result = poll("cancel-fixture", &ws).unwrap();
        assert_eq!(result["status"], "failed");
        assert!(result["error"].as_str().unwrap().contains("cancelled"));
        assert!(ws.gmail_credentials().unwrap().is_none());
    }

    #[test]
    fn only_expired_finished_results_are_discarded() {
        let now = Instant::now();
        let make_task = |finished_at: Option<Instant>| {
            Some(Task {
                id: "fixture".into(),
                cancelled: Arc::new(AtomicBool::new(false)),
                result: finished_at.map(|_| Ok(Zeroizing::new(b"fictional".to_vec()))),
                finished_at,
            })
        };
        let mut running = make_task(None);
        expire_finished(&mut running, now + RESULT_LIFETIME);
        assert!(running.is_some());
        let mut fresh = make_task(Some(now));
        expire_finished(&mut fresh, now + RESULT_LIFETIME - Duration::from_secs(1));
        assert!(fresh.is_some());
        expire_finished(&mut fresh, now + RESULT_LIFETIME);
        assert!(fresh.is_none());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires explicitly supplied test Gmail credentials and authenticates without sending"]
    fn live_app_password_connects_and_stores_in_temporary_workspace() {
        let email = Zeroizing::new(
            std::env::var("WAYPOINT_TEST_GMAIL_USER").expect("test account is required"),
        );
        let password = Zeroizing::new(
            std::env::var("WAYPOINT_TEST_GMAIL_PASSWORD").expect("test app password is required"),
        );
        let directory = tempfile::tempdir().unwrap();
        let ws = Workspace::open(directory.path()).unwrap();
        assert!(ws.can_store_gmail_credentials());
        start(
            "live-gmail-test".into(),
            email.to_string(),
            password.to_string(),
            &ws,
        )
        .unwrap();
        let started = std::time::Instant::now();
        loop {
            let status = poll("live-gmail-test", &ws).unwrap();
            match status["status"].as_str() {
                Some("complete") => break,
                Some("running") if started.elapsed() < Duration::from_secs(55) => {
                    std::thread::sleep(Duration::from_millis(250))
                }
                _ => panic!(
                    "Gmail connection failed: {}",
                    status["error"].as_str().unwrap_or("timeout")
                ),
            }
        }
        let stored = Zeroizing::new(
            ws.gmail_credentials()
                .unwrap()
                .expect("credentials should be saved"),
        );
        let data: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(data["auth_type"], "app_password");
        assert!(data["email"].as_str() == Some(email.as_str()));
        assert!(data["password"].as_str() == Some(password.as_str()));
        drop(data);
        ws.clear_gmail_credentials().unwrap();
    }
}
