//! User-initiated Gmail app-password authentication. This module never sends mail.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::Duration;
use waypoint_workspace::Workspace;
use zeroize::Zeroizing;

struct Task {
    id: String,
    result: Option<Result<Zeroizing<Vec<u8>>, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
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

fn authenticate_imap(email: &str, password: &str) -> Result<(), String> {
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
            let _ = tls.write_all(b"a002 LOGOUT\r\n");
            return Ok(());
        }
        if line.starts_with("a001 ") {
            return Err("Gmail IMAP rejected authentication".into());
        }
    }
    Err("Gmail IMAP authentication response exceeded its limit".into())
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
    if task.is_some() {
        return Err("A Gmail connection is already running".into());
    }
    *task = Some(Task {
        id: id.clone(),
        result: None,
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
                    authenticate_imap(&email, &password)
                        .map_err(|error| format!("IMAP check: {error}"))
                })
                .and_then(|_| {
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
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str, ws: &Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Gmail connection not found")?;
    match task.result.take() {
        None => Ok(json!({"status":"running"})),
        Some(Err(error)) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
        Some(Ok(credentials)) => {
            *tasks = None;
            ws.save_gmail_credentials(&credentials)
                .map_err(|_| "Cannot save Gmail credentials securely")?;
            Ok(json!({"status":"complete"}))
        }
    }
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
