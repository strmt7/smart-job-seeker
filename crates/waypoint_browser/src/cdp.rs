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
        let mut out = vec![];
        let mut push = |p: String| {
            let path = PathBuf::from(p);
            if path.is_file() {
                out.push(path);
            }
        };
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            push(format!("{local}\\Microsoft\\Edge\\Application\\msedge.exe"));
            push(format!("{local}\\Google\\Chrome\\Application\\chrome.exe"));
        }
        if let Ok(pf) = std::env::var("PROGRAMFILES") {
            push(format!("{pf}\\Google\\Chrome\\Application\\chrome.exe"));
            push(format!("{pf}\\Microsoft\\Edge\\Application\\msedge.exe"));
        }
        if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
            push(format!("{pf86}\\Microsoft\\Edge\\Application\\msedge.exe"));
            push(format!("{pf86}\\Google\\Chrome\\Application\\chrome.exe"));
        }
        for p in [
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/google-chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ] {
            push(p.to_string());
        }
        out
    }

    /// Launch with a throwaway profile and wait until it is actually talking.
    pub fn launch(timeout: Duration) -> Result<Self, CdpError> {
        let binaries = Self::discover_binaries();
        let Some(exe) = binaries.first() else {
            return err("no Chromium-family browser found (Edge/Chrome/Chromium)");
        };
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
        cmd.arg("--headless=new")
            .arg("--remote-debugging-port=0")
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
        let child = cmd
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CdpError(format!("spawn {}: {e}", exe.display())))?;

        let port_file = user_data_dir.join("DevToolsActivePort");
        let deadline = Instant::now() + timeout;
        let mut port = 0u16;
        while Instant::now() < deadline {
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
            let mut me = Self {
                child,
                port: 0,
                ws_url: String::new(),
                user_data_dir,
            };
            me.shutdown();
            return err("browser did not report a debugging port in time");
        }

        // Ask the browser itself for the WebSocket endpoint.
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let version = loop {
            match waypoint_net::request_loopback(
                addr,
                "GET",
                "/json/version",
                None,
                2_000,
                64 * 1024,
            ) {
                Ok(resp) if resp.status == 200 => break resp,
                other => {
                    if Instant::now() >= deadline {
                        let mut me = Self {
                            child,
                            port,
                            ws_url: String::new(),
                            user_data_dir,
                        };
                        me.shutdown();
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
        let ws_url = parsed
            .get("webSocketDebuggerUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CdpError("no webSocketDebuggerUrl reported".into()))?
            .to_string();

        Ok(Self {
            child,
            port,
            ws_url,
            user_data_dir,
        })
    }

    pub fn connect(&self) -> Result<Ws, CdpError> {
        let (ws, _resp) = tungstenite::connect(self.ws_url.as_str())
            .map_err(|e| CdpError(format!("websocket connect: {e}")))?;
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
            let raw = match self.ws.read() {
                Ok(Message::Text(t)) => t.as_str().to_string(),
                Ok(Message::Binary(b)) => String::from_utf8_lossy(&b).into_owned(),
                Ok(Message::Close(_)) => return err(format!("{method}: connection closed")),
                Ok(_) => continue,
                Err(e) => return err(format!("{method}: read: {e}")),
            };
            let value: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if value.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if let Some(e) = value.get("error") {
                    return err(format!("{method}: protocol error {e}"));
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
            if let Some(event) = value.get("method").and_then(|v| v.as_str()) {
                self.events.push((event.to_string(), value));
            }
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
                        if let Some(event) = value.get("method").and_then(|v| v.as_str()) {
                            self.events.push((event.to_string(), value));
                        }
                    }
                }
                Ok(Message::Binary(b)) => {
                    if let Ok(value) = serde_json::from_str::<Value>(&String::from_utf8_lossy(&b)) {
                        if let Some(event) = value.get("method").and_then(|v| v.as_str()) {
                            self.events.push((event.to_string(), value));
                        }
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
