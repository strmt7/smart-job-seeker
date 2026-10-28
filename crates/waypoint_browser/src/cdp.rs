//! Minimal Chrome DevTools Protocol transport.
//!
//! Scope is deliberately small: launch a browser with a private profile, talk
//! WebSocket to it, and issue protocol calls. All protocol *semantics* live in
//! `lib.rs`; keeping the transport boring makes the driver auditable.
//!
//! Notable choices:
//! - The debugging port is discovered from the browser's own
//!   `DevToolsActivePort` file rather than picked by us, so there is no port
//!   race and no fixed port for another process to target.
//! - The profile directory is a private temp dir, so the user's real browser
//!   profile (cookies, sessions) is never touched or exposed.
//! - Flat sessions: every call carries the `sessionId` of the page we own.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

pub type Ws = WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;
const MAX_PROTOCOL_MESSAGE: usize = 16 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENTS: usize = 64;

#[derive(Debug)]
pub struct CdpError(pub String);

impl std::fmt::Display for CdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CdpError {}

fn err<T>(m: impl Into<String>) -> Result<T, CdpError> {
    Err(CdpError(m.into()))
}

fn candidate_binaries(
    explicit: Option<PathBuf>,
    root: Option<&Path>,
    discovered: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = explicit {
        candidates.push(path);
    }
    if let Some(root) = root {
        candidates.push(root.join("dependencies/chromium/chrome.exe"));
    }
    candidates.extend(discovered);
    let mut found = Vec::new();
    for path in candidates {
        if path.is_file() && !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

/// A browser process owned by us, with a private profile.
pub struct BrowserProcess {
    child: Child,
    pub port: u16,
    pub ws_url: String,
    pub user_data_dir: PathBuf,
}

impl BrowserProcess {
    /// Candidate executables, most-preferred first. Only paths that exist are
    /// returned, so a missing browser is reported rather than guessed at.
    pub fn discover_binaries() -> Vec<PathBuf> {
        let mut discovered = vec![];
        let mut push = |path: PathBuf| discovered.push(path);
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            push(PathBuf::from(format!(
                "{local}\\Microsoft\\Edge\\Application\\msedge.exe"
            )));
            push(PathBuf::from(format!(
                "{local}\\Google\\Chrome\\Application\\chrome.exe"
            )));
        }
        if let Ok(pf) = std::env::var("PROGRAMFILES") {
            push(PathBuf::from(format!(
                "{pf}\\Google\\Chrome\\Application\\chrome.exe"
            )));
            push(PathBuf::from(format!(
                "{pf}\\Microsoft\\Edge\\Application\\msedge.exe"
            )));
        }
        if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
            push(PathBuf::from(format!(
                "{pf86}\\Microsoft\\Edge\\Application\\msedge.exe"
            )));
            push(PathBuf::from(format!(
                "{pf86}\\Google\\Chrome\\Application\\chrome.exe"
            )));
        }
        for p in [
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/google-chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ] {
            push(PathBuf::from(p));
        }
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                for name in ["chrome.exe", "msedge.exe", "chromium.exe"] {
                    push(dir.join(name));
                }
            }
        }
        let executable = std::env::current_exe().ok();
        candidate_binaries(
            std::env::var_os("WAYPOINT_BROWSER").map(PathBuf::from),
            executable.as_deref().and_then(Path::parent),
            discovered,
        )
    }

    /// Launch with a throwaway profile and wait until it is actually talking.
    pub fn launch(timeout: Duration) -> Result<Self, CdpError> {
        Self::launch_with_mode(timeout, false)
    }

    pub fn launch_with_mode(timeout: Duration, visible: bool) -> Result<Self, CdpError> {
        let binaries = Self::discover_binaries();
        Self::launch_candidates(&binaries, timeout, visible)
    }

    fn launch_candidates(
        binaries: &[PathBuf],
        timeout: Duration,
        visible: bool,
    ) -> Result<Self, CdpError> {
        if binaries.is_empty() {
            return err("no Chromium-family browser found (Edge/Chrome/Chromium)");
        }
        let deadline = Instant::now() + timeout;
        let mut last_error = None;
        let candidates = &binaries[..binaries.len().min(5)];
        for (index, exe) in candidates.iter().enumerate() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let budget = if index + 1 == candidates.len() {
                remaining
            } else {
                (remaining / 2).min(Duration::from_secs(5))
            };
            match Self::launch_one(exe, budget, visible) {
                Ok(browser) => return Ok(browser),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| CdpError("browser startup timed out".into())))
    }

    fn launch_one(exe: &Path, timeout: Duration, visible: bool) -> Result<Self, CdpError> {
        // Unique per launch: pid alone is not enough (two launches in one
        // process, or a recycled pid, would collide).
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let user_data_dir =
            std::env::temp_dir().join(format!("waypoint-profile-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&user_data_dir).map_err(|e| CdpError(e.to_string()))?;

        let mut cmd = Command::new(exe);
        if !visible {
            cmd.arg("--headless=new");
        }
        cmd.arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", user_data_dir.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-extensions")
            .arg("--disable-background-networking")
            .arg("--disable-sync")
            .arg("--disable-gpu")
            .arg("--window-size=1280,900");
        // Container CI images often cannot use the Chromium sandbox. This is
        // opt-in through the environment, so a normal desktop run keeps it on.
        if std::env::var("WAYPOINT_BROWSER_NO_SANDBOX").is_ok_and(|v| v == "1") {
            cmd.arg("--no-sandbox");
        }
        let child = match cmd
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&user_data_dir);
                return err(format!("spawn {}: {error}", exe.display()));
            }
        };
        // Own the child immediately, so every later error path runs Drop.
        let mut browser = Self {
            child,
            port: 0,
            ws_url: String::new(),
            user_data_dir,
        };

        let port_file = browser.user_data_dir.join("DevToolsActivePort");
        let deadline = Instant::now() + timeout;
        let mut port = 0u16;
        while Instant::now() < deadline {
            match browser.child.try_wait() {
                Ok(Some(status)) => {
                    return err(format!(
                        "browser exited before its debugging endpoint was ready: {status}"
                    ));
                }
                Ok(None) => {}
                Err(error) => {
                    return err(format!("could not inspect browser startup: {error}"));
                }
            }
            if let Ok(text) = std::fs::read_to_string(&port_file) {
                if let Some(first) = text.lines().next() {
                    if let Ok(p) = first.trim().parse::<u16>() {
                        port = p;
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if port == 0 {
            return err("browser did not report a debugging port in time");
        }
        browser.port = port;

        // Ask the browser itself for the WebSocket endpoint.
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let version = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return err("browser debugging endpoint not ready before the startup deadline");
            }
            let request_timeout_ms = remaining.as_millis().clamp(1, 2_000) as u64;
            match waypoint_net::request_loopback(
                addr,
                "GET",
                "/json/version",
                None,
                request_timeout_ms,
                64 * 1024,
            ) {
                Ok(resp) if resp.status == 200 => break resp,
                other => {
                    if Instant::now() >= deadline {
                        return err(format!(
                            "browser debugging endpoint not ready: {:?}",
                            other.map(|r| r.status).map_err(|e| e.to_string())
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        };
        let parsed: Value = serde_json::from_slice(&version.body)
            .map_err(|e| CdpError(format!("/json/version: {e}")))?;
        browser.ws_url = parsed
            .get("webSocketDebuggerUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CdpError("no webSocketDebuggerUrl reported".into()))?
            .to_string();

        Ok(browser)
    }

    pub fn connect(&self) -> Result<Ws, CdpError> {
        let (mut ws, _resp) = tungstenite::connect(self.ws_url.as_str())
            .map_err(|e| CdpError(format!("websocket connect: {e}")))?;
        ws.set_config(|config| {
            config.max_message_size = Some(MAX_PROTOCOL_MESSAGE);
            config.max_frame_size = Some(MAX_PROTOCOL_MESSAGE);
        });
        // Bound every read so a wedged browser cannot hang the app forever.
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = ws.get_ref() {
            let _ = tcp.set_read_timeout(Some(Duration::from_secs(20)));
        }
        Ok(ws)
    }

    /// Stop the browser and delete its private profile.
    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.user_data_dir);
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// One CDP session: request/response correlation plus event buffering.
pub struct Cdp {
    ws: Ws,
    next_id: u64,
    pub session: String,
    /// Events observed while waiting for replies, newest last. Kept because a
    /// driver legitimately needs them (e.g. `Page.loadEventFired`).
    pub events: Vec<(String, Value)>,
    pub timeout: Duration,
}

fn retain_event(events: &mut Vec<(String, Value)>, value: Value, raw_len: usize) {
    if raw_len > MAX_EVENT_BYTES {
        return;
    }
    let Some(name) = value
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    if name == "Page.loadEventFired" {
        events.retain(|(event, _)| event != "Page.loadEventFired");
    }
    if events.len() >= MAX_EVENTS {
        let oldest_non_load = events
            .iter()
            .position(|(event, _)| event != "Page.loadEventFired")
            .unwrap_or(0);
        events.remove(oldest_non_load);
    }
    events.push((name, value));
}

impl Cdp {
    pub fn new(ws: Ws) -> Result<Self, CdpError> {
        let mut cdp = Self {
            ws,
            next_id: 0,
            session: String::new(),
            events: vec![],
            timeout: Duration::from_secs(20),
        };
        let target = cdp.call_browser("Target.createTarget", json!({ "url": "about:blank" }))?;
        let target_id = target
            .get("targetId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CdpError("Target.createTarget returned no targetId".into()))?
            .to_string();
        // Bring the controlled tab forward in visible review sessions.
        cdp.call_browser("Target.activateTarget", json!({ "targetId": target_id }))?;
        let attached = cdp.call_browser(
            "Target.attachToTarget",
            json!({ "targetId": target_id, "flatten": true }),
        )?;
        cdp.session = attached
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CdpError("Target.attachToTarget returned no sessionId".into()))?
            .to_string();
        Ok(cdp)
    }

    /// A call without a session (browser-scoped commands).
    pub fn call_browser(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        self.call_inner(method, params, None)
    }

    /// A call scoped to our page.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        let session = self.session.clone();
        self.call_inner(method, params, Some(session))
    }

    fn call_inner(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
    ) -> Result<Value, CdpError> {
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = Value::String(s);
        }
        self.ws
            .send(Message::Text(msg.to_string().into()))
            .map_err(|e| CdpError(format!("send {method}: {e}")))?;

        let deadline = Instant::now() + self.timeout;
        loop {
            if Instant::now() >= deadline {
                return err(format!("{method}: timed out waiting for a reply"));
            }
            if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = self.ws.get_ref() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                tcp.set_read_timeout(Some(remaining.max(Duration::from_millis(1))))
                    .map_err(|e| CdpError(format!("{method}: cannot set read timeout: {e}")))?;
            }
            let raw = match self.ws.read() {
                Ok(Message::Text(t)) => t.as_str().to_string(),
                Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
                Ok(Message::Close(_)) => return err(format!("{method}: connection closed")),
                Ok(_) => continue,
                Err(e) => return err(format!("{method}: read: {e}")),
            };
            let value: Value = serde_json::from_str(&raw).map_err(|error| {
                CdpError(format!(
                    "{method}: invalid or too deeply nested browser response: {error}"
                ))
            })?;
            if value.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if let Some(e) = value.get("error") {
                    return err(format!("{method}: protocol error {e}"));
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
            retain_event(&mut self.events, value, raw.len());
        }
    }

    /// Drain the socket for `duration`, recording any events that arrive.
    /// Used to wait for a load without blocking forever.
    pub fn pump(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = self.ws.get_ref() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let _ = tcp.set_read_timeout(Some(remaining.max(Duration::from_millis(10))));
            }
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    if let Ok(value) = serde_json::from_str::<Value>(t.as_str()) {
                        retain_event(&mut self.events, value, t.len());
                    }
                }
                Ok(Message::Binary(b)) => {
                    if let Ok(value) = serde_json::from_str::<Value>(&String::from_utf8_lossy(&b)) {
                        retain_event(&mut self.events, value, b.len());
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.events)
    }

    pub fn saw_event(&self, name: &str) -> bool {
        self.events.iter().any(|(e, _)| e == name)
    }
}

/// Windows-safe temporary directory helper used by tests and the driver.
pub fn ensure_dir(path: &Path) -> Result<(), CdpError> {
    std::fs::create_dir_all(path).map_err(|e| CdpError(format!("{}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "starts a locally installed browser with a temporary profile"]
    fn falls_back_after_invalid_browser_executable() {
        let actual = BrowserProcess::discover_binaries()
            .into_iter()
            .next()
            .expect("a local browser is required");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("waypoint-invalid-browser-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let invalid = root.join("invalid.exe");
        std::fs::write(&invalid, b"not an executable").unwrap();
        let browser =
            BrowserProcess::launch_candidates(&[invalid, actual], Duration::from_secs(20), false)
                .unwrap();
        assert_ne!(browser.port, 0);
        drop(browser);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_prefers_explicit_then_bundled_and_skips_missing_duplicates() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!(
                "waypoint-browser-discovery-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let bundled = root.join("dependencies/chromium/chrome.exe");
        let explicit = root.join("custom.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"fixture").unwrap();
        std::fs::write(&explicit, b"fixture").unwrap();
        let found = candidate_binaries(
            Some(explicit.clone()),
            Some(&root),
            vec![bundled.clone(), root.join("missing.exe")],
        );
        assert_eq!(found, vec![explicit, bundled]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn event_buffer_is_bounded_and_retains_a_load_signal() {
        let mut events = Vec::new();
        retain_event(&mut events, json!({"method":"Page.loadEventFired"}), 100);
        for index in 0..(MAX_EVENTS * 3) {
            retain_event(
                &mut events,
                json!({"method":"Runtime.event","index":index}),
                100,
            );
        }
        assert_eq!(events.len(), MAX_EVENTS);
        assert!(events.iter().any(|(name, _)| name == "Page.loadEventFired"));
        assert_eq!(
            events.last().unwrap().1["index"],
            (MAX_EVENTS * 3 - 1) as u64
        );
        retain_event(
            &mut events,
            json!({"method":"oversized"}),
            MAX_EVENT_BYTES + 1,
        );
        assert_eq!(events.len(), MAX_EVENTS);
        assert!(!events.iter().any(|(name, _)| name == "oversized"));
    }

    #[test]
    fn command_timeout_is_not_inherited_from_short_event_poll() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let request = socket.read().unwrap().into_text().unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            std::thread::sleep(Duration::from_millis(250));
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":{"ok":true}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        });
        let (socket, _) = tungstenite::connect(format!("ws://{addr}")).unwrap();
        let mut cdp = Cdp {
            ws: socket,
            next_id: 0,
            session: String::new(),
            events: vec![],
            timeout: Duration::from_secs(2),
        };
        cdp.pump(Duration::from_millis(40));
        let response = cdp
            .call_browser("Test.delayed", json!({}))
            .expect("the command gets its own timeout");
        assert_eq!(response["ok"], true);
        worker.join().unwrap();
    }

    #[test]
    fn unparseable_response_is_reported_without_waiting_for_another_reply() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let request: Value =
                serde_json::from_str(&socket.read().unwrap().into_text().unwrap()).unwrap();
            let nested = format!("{}0{}", "[".repeat(140), "]".repeat(140));
            socket
                .send(Message::Text(
                    format!("{{\"id\":{},\"result\":{nested}}}", request["id"]).into(),
                ))
                .unwrap();
        });
        let (socket, _) = tungstenite::connect(format!("ws://{addr}")).unwrap();
        let mut cdp = Cdp {
            ws: socket,
            next_id: 0,
            session: String::new(),
            events: vec![],
            timeout: Duration::from_secs(2),
        };
        let error = cdp.call_browser("Test.deep", json!({})).unwrap_err();
        assert!(
            error
                .0
                .contains("invalid or too deeply nested browser response"),
            "{error}"
        );
        worker.join().unwrap();
    }
}
