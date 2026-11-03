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
#[cfg(not(windows))]
use std::process::Child;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

// Only the DOM reply may use the deeper decoder, and only after a lexical
// nesting check. The separate bounded stack avoids exhausting the caller's
// stack during parsing; other uses of Value still require the depth ceiling.
const MAX_DOM_JSON_DEPTH: usize = 224;
fn bounded_json_depth(raw: &str, limit: usize) -> bool {
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for byte in raw.bytes() {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > limit {
                        return false;
                    }
                }
                b'}' | b']' => {
                    let Some(next) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = next;
                }
                _ => {}
            }
        }
    }
    !string && depth == 0
}

fn parse_protocol_message(raw: &str, allow_deeper_dom: bool) -> Result<Value, String> {
    match serde_json::from_str(raw) {
        Ok(value) => Ok(value),
        Err(error)
            if allow_deeper_dom && error.to_string().contains("recursion limit exceeded") =>
        {
            if raw.len() > MAX_PROTOCOL_MESSAGE || !bounded_json_depth(raw, MAX_DOM_JSON_DEPTH) {
                return Err("browser response exceeds the bounded DOM nesting limit".into());
            }
            std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .name("waypoint-dom-decode".into())
                    .stack_size(8 * 1024 * 1024)
                    .spawn_scoped(scope, || {
                        let mut decoder = serde_json::Deserializer::from_str(raw);
                        decoder.disable_recursion_limit();
                        let value = Value::deserialize(&mut decoder).map_err(|e| e.to_string())?;
                        decoder.end().map_err(|e| e.to_string())?;
                        Ok(value)
                    })
                    .map_err(|_| "could not start bounded DOM decoder".to_owned())?
                    .join()
                    .map_err(|_| "bounded DOM decoder failed".to_owned())?
            })
        }
        Err(error) => Err(error.to_string()),
    }
}

pub type Ws = WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;
const MAX_PROTOCOL_MESSAGE: usize = 16 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENTS: usize = 64;

#[derive(Debug)]
pub struct CdpError(pub String);

/// Stable discovery failure, separate from errors while starting a browser.
pub const NO_BROWSER: &str = "no Chromium-family browser found (Edge/Chrome/Chromium)";

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
    let explicit_bundle = explicit.as_ref().is_some_and(|path| {
        root.is_some_and(|root| {
            same_binary_path(path, &root.join("dependencies/chromium/chrome.exe"))
        })
    });
    if let Some(path) = explicit {
        candidates.push(path);
    }
    candidates.extend(discovered);
    let bundled = root.map(|root| root.join("dependencies/chromium/chrome.exe"));
    let mut found: Vec<PathBuf> = Vec::new();
    for path in candidates {
        if path.is_file()
            && !found
                .iter()
                .any(|existing| same_binary_path(existing, &path))
        {
            found.push(path);
        }
    }
    // Prefer installed browsers, which may update independently. Keep the
    // bundled browser within the launcher's twelve-candidate budget so it
    // remains available when the installed candidates cannot start.
    if let Some(bundled) = bundled.filter(|path| path.is_file()) {
        if let Some(index) = found
            .iter()
            .position(|existing| same_binary_path(existing, &bundled))
        {
            // An explicit browser remains the user's first choice.
            if !explicit_bundle {
                found.remove(index);
                found.truncate(11);
                found.push(bundled);
            }
        } else {
            found.truncate(11);
            found.push(bundled);
        }
    }
    found
}

fn same_binary_path(first: &Path, second: &Path) -> bool {
    #[cfg(windows)]
    {
        first.to_string_lossy().replace('/', "\\").to_lowercase()
            == second.to_string_lossy().replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        first == second
    }
}

fn candidate_budget(remaining: Duration, index: usize, total: usize) -> Duration {
    if index + 1 == total {
        return remaining;
    }
    let reserve = (remaining / 2).min(Duration::from_secs(5));
    ((remaining - reserve) / (total - index - 1) as u32).min(Duration::from_secs(5))
}

#[cfg(windows)]
fn parse_registered_browser(output: &[u8], expected_name: &str) -> Option<PathBuf> {
    let text = std::str::from_utf8(output).ok()?;
    let mut candidates = text.lines().filter_map(|line| {
        let (prefix, value) = line.split_once("REG_SZ")?;
        if prefix.trim().is_empty() || !value.starts_with(char::is_whitespace) {
            return None;
        }
        let value = value.trim().trim_matches('"');
        if value.is_empty() || value.chars().any(char::is_control) {
            return None;
        }
        let path = PathBuf::from(value);
        (path.is_absolute()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(expected_name)))
        .then_some(path)
    });
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        None
    } else {
        Some(candidate)
    }
}

/// Query only fixed Windows App Paths entries. Each OS process has a short
/// deadline and a bounded output reader; no shell or browser profile is used.
#[cfg(windows)]
fn registered_browsers() -> Vec<PathBuf> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    let Some(system_root) = std::env::var_os("SystemRoot") else {
        return Vec::new();
    };
    let reg = PathBuf::from(system_root).join("System32/reg.exe");
    if !reg.is_file() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for hive in ["HKCU", "HKLM"] {
        for name in ["msedge.exe", "chrome.exe", "chromium.exe"] {
            let key = format!(r"{hive}\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{name}");
            let Ok(mut child) = Command::new(&reg)
                .args(["query", &key, "/ve"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
            else {
                continue;
            };
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                continue;
            };
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut data = Vec::new();
                let result = stdout.take(16 * 1024).read_to_end(&mut data).map(|_| data);
                let _ = sender.send(result);
            });
            let deadline = Instant::now() + Duration::from_millis(800);
            let success = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status.success(),
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    _ => {
                        let _ = child.kill();
                        break false;
                    }
                }
            };
            if success {
                if let Ok(Ok(data)) = receiver.recv_timeout(Duration::from_millis(100)) {
                    if let Some(path) = parse_registered_browser(&data, name) {
                        paths.push(path);
                    }
                }
            }
        }
    }
    paths
}

/// A browser process owned by us, with a private profile.
pub struct BrowserProcess {
    child: OwnedBrowserChild,
    pub port: u16,
    pub ws_url: String,
    pub user_data_dir: PathBuf,
}

#[cfg(not(windows))]
struct OwnedBrowserChild(Child);

#[cfg(not(windows))]
impl OwnedBrowserChild {
    fn spawn(command: &mut Command, _timeout: Duration, _profile: &Path) -> Result<Self, String> {
        command.spawn().map(Self).map_err(|e| e.to_string())
    }
    fn exited(&mut self) -> Result<bool, String> {
        self.0
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|e| e.to_string())
    }
    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(windows)]
enum BrowserWorkerCommand {
    Check(std::sync::mpsc::Sender<Result<bool, String>>),
    Stop(std::sync::mpsc::Sender<()>),
}

#[cfg(windows)]
struct OwnedBrowserChild {
    commands: std::sync::mpsc::Sender<BrowserWorkerCommand>,
}

#[cfg(all(windows, test))]
static TEST_OWNER_READY_DELAY_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(windows)]
impl OwnedBrowserChild {
    fn spawn(command: &mut Command, timeout: Duration, profile: &Path) -> Result<Self, String> {
        use processkit::{Command as GroupCommand, ProcessGroup, Stdin, StdioMode};
        let program = command.get_program().to_owned();
        let args: Vec<_> = command.get_args().map(std::ffi::OsStr::to_owned).collect();
        let profile = profile.to_owned();
        let (commands, receiver) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("waypoint-browser-owner".into())
            .spawn(move || {
                let setup = || -> Result<_, String> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    let group = ProcessGroup::new().map_err(|e| e.to_string())?;
                    let command = GroupCommand::new(program)
                        .args(args)
                        .stdin(Stdin::empty())
                        .stdout(StdioMode::Null)
                        .stderr(StdioMode::Null)
                        .create_no_window();
                    let process = runtime
                        .block_on(group.start(&command))
                        .map_err(|e| e.to_string())?;
                    let pid = process
                        .pid()
                        .ok_or("Browser process identity unavailable")?;
                    Ok((runtime, group, process, pid))
                };
                let (runtime, group, process, pid) = match setup() {
                    Ok(owned) => owned,
                    Err(error) => {
                        if ready.send(Err(error)).is_err() {
                            let _ = std::fs::remove_dir_all(&profile);
                        }
                        return;
                    }
                };
                #[cfg(test)]
                std::thread::sleep(Duration::from_millis(
                    TEST_OWNER_READY_DELAY_MS.swap(0, std::sync::atomic::Ordering::SeqCst),
                ));
                if ready.send(Ok(())).is_err() {
                    let _ = group.kill_all();
                    let _ = runtime.block_on(async {
                        tokio::time::timeout(Duration::from_secs(1), process.wait()).await
                    });
                    drop(group);
                    drop(runtime);
                    let _ = std::fs::remove_dir_all(&profile);
                    return;
                }
                while let Ok(command) = receiver.recv() {
                    match command {
                        BrowserWorkerCommand::Check(reply) => {
                            let state = group
                                .members()
                                .map(|members| !members.contains(&pid))
                                .map_err(|e| e.to_string());
                            let _ = reply.send(state);
                        }
                        BrowserWorkerCommand::Stop(reply) => {
                            let _ = group.kill_all();
                            let _ = runtime.block_on(async {
                                tokio::time::timeout(Duration::from_secs(1), process.wait()).await
                            });
                            drop(group);
                            drop(runtime);
                            let _ = reply.send(());
                            return;
                        }
                    }
                }
                drop(process);
                drop(group);
                drop(runtime);
                let _ = std::fs::remove_dir_all(&profile);
            })
            .map_err(|e| format!("Cannot start browser ownership worker: {e}"))?;
        started
            .recv_timeout(timeout.min(Duration::from_secs(5)))
            .map_err(|_| "Browser ownership setup timed out".to_string())??;
        Ok(Self { commands })
    }
    fn exited(&mut self) -> Result<bool, String> {
        let (reply, response) = std::sync::mpsc::channel();
        self.commands
            .send(BrowserWorkerCommand::Check(reply))
            .map_err(|_| "Browser ownership worker stopped")?;
        response
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| "Browser ownership check timed out".to_string())?
    }
    fn stop(&mut self) {
        let (reply, response) = std::sync::mpsc::channel();
        if self
            .commands
            .send(BrowserWorkerCommand::Stop(reply))
            .is_ok()
        {
            let _ = response.recv_timeout(Duration::from_secs(2));
        }
    }
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
            push(PathBuf::from(format!(
                "{local}\\Chromium\\Application\\chrome.exe"
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
        #[cfg(windows)]
        for path in registered_browsers() {
            push(path);
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
            return err(NO_BROWSER);
        }
        let deadline = Instant::now() + timeout;
        let mut last_error = None;
        let candidates = &binaries[..binaries.len().min(12)];
        let mut attempted = 0;
        for (index, exe) in candidates.iter().enumerate() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let budget = candidate_budget(remaining, index, candidates.len());
            attempted += 1;
            match Self::launch_one(exe, budget, visible) {
                Ok(browser) => return Ok(browser),
                Err(error) => last_error = Some(error),
            }
        }
        let error = last_error.unwrap_or_else(|| CdpError("browser startup timed out".into()));
        Err(CdpError(format!(
            "{} (tried {attempted} of {} browser candidates)",
            error.0,
            binaries.len()
        )))
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
        cmd.arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = match OwnedBrowserChild::spawn(&mut cmd, timeout, &user_data_dir) {
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
            match browser.child.exited() {
                Ok(true) => return err("browser exited before its debugging endpoint was ready"),
                Ok(false) => {}
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
        // Ask the owned browser to close its own process tree before falling
        // back to a forced kill. Bound the handshake and wait: an unhealthy
        // browser must never hold the app open indefinitely.
        if self.port != 0 && !self.ws_url.is_empty() && matches!(self.child.exited(), Ok(false)) {
            let prefix = format!("ws://127.0.0.1:{}/", self.port);
            if self.ws_url.starts_with(&prefix) {
                let address = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
                if let Ok(socket) = TcpStream::connect_timeout(&address, Duration::from_millis(250))
                {
                    let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));
                    let _ = socket.set_write_timeout(Some(Duration::from_millis(250)));
                    if let Ok((mut ws, _)) = tungstenite::client(self.ws_url.as_str(), socket) {
                        let _ = ws.send(Message::Text(
                            json!({"id":1,"method":"Browser.close"}).to_string().into(),
                        ));
                    }
                }
            }
            self.ws_url.clear();
            let deadline = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < deadline {
                match self.child.exited() {
                    Ok(true) | Err(_) => break,
                    Ok(false) => std::thread::sleep(Duration::from_millis(25)),
                }
            }
        }
        self.child.stop();
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
    observed_non_read_requests: u64,
    paused_event_lost: bool,
}

fn is_non_read_request(value: &Value) -> bool {
    match value.get("method").and_then(Value::as_str) {
        Some("Network.webSocketFrameSent") => true,
        Some("Network.requestWillBeSent") => value
            .pointer("/params/request/method")
            .and_then(Value::as_str)
            .is_some_and(|method| {
                !method.is_empty()
                    && !method.eq_ignore_ascii_case("GET")
                    && !method.eq_ignore_ascii_case("HEAD")
            }),
        _ => false,
    }
}

// Returns true if a paused request cannot be retained for servicing.
fn retain_event(events: &mut Vec<(String, Value)>, value: Value, raw_len: usize) -> bool {
    let Some(name) = value
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return false;
    };
    // Keep only bounded metadata for document responses. Network events can
    // contain headers and URLs; neither is needed to check the HTTP status.
    let value = if name.starts_with("Network.") {
        if name != "Network.responseReceived"
            || value.pointer("/params/type").and_then(Value::as_str) != Some("Document")
        {
            return false;
        }
        json!({"params":{"frameId":value.pointer("/params/frameId"),"loaderId":value.pointer("/params/loaderId"),"status":value.pointer("/params/response/status")}})
    } else if name == "Fetch.requestPaused" {
        // A paused request can contain credentials, URLs and form data. Keep
        // only bounded protocol identifiers and classification, never payload.
        let Some(id) = value
            .pointer("/params/requestId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        else {
            return true;
        };
        let method = value
            .pointer("/params/request/method")
            .and_then(Value::as_str)
            .filter(|s| {
                !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphabetic())
            });
        let session = value
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control));
        if value.get("sessionId").is_some() && session.is_none() {
            return true;
        }
        let response_status = value
            .pointer("/params/responseStatusCode")
            .and_then(Value::as_u64)
            .filter(|s| (100..=599).contains(s));
        json!({"sessionId":session,"params":{"requestId":id,"request":{"method":method},"responseStatusCode":response_status}})
    } else {
        if raw_len > MAX_EVENT_BYTES {
            return false;
        }
        value
    };
    if name == "Page.loadEventFired" {
        events.retain(|(event, _)| event != "Page.loadEventFired");
    }
    if events.len() >= MAX_EVENTS {
        let oldest_non_load = events
            .iter()
            .position(|(event, _)| {
                event != "Page.loadEventFired"
                    && event != "Network.responseReceived"
                    && event != "Fetch.requestPaused"
            })
            .or_else(|| {
                events
                    .iter()
                    .position(|(event, _)| event != "Fetch.requestPaused")
            });
        if let Some(index) = oldest_non_load {
            events.remove(index);
        } else {
            return name == "Fetch.requestPaused";
        }
    }
    events.push((name, value));
    false
}

impl Cdp {
    pub fn new(ws: Ws) -> Result<Self, CdpError> {
        let mut cdp = Self {
            ws,
            next_id: 0,
            session: String::new(),
            events: vec![],
            timeout: Duration::from_secs(20),
            observed_non_read_requests: 0,
            paused_event_lost: false,
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

    /// A call scoped to an explicitly attached child target owned by this page.
    pub fn call_session(
        &mut self,
        session: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, CdpError> {
        self.call_inner(method, params, Some(session.to_owned()))
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
            let value =
                parse_protocol_message(&raw, method == "DOM.getDocument").map_err(|error| {
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
            if is_non_read_request(&value) {
                self.observed_non_read_requests = self.observed_non_read_requests.saturating_add(1);
            }
            self.paused_event_lost |= retain_event(&mut self.events, value, raw.len());
        }
    }

    /// Drain the socket for `duration`, recording any events that arrive.
    /// Used to wait for a load without blocking forever.
    pub fn pump(&mut self, duration: Duration) {
        let _ = self.pump_checked(duration);
    }

    /// Drain events while distinguishing an ordinary idle timeout from loss of
    /// the protocol stream. This does not itself continuously service Fetch.
    pub fn pump_checked(&mut self, duration: Duration) -> Result<(), CdpError> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = self.ws.get_ref() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if tcp
                    .set_read_timeout(Some(remaining.max(Duration::from_millis(10))))
                    .is_err()
                {
                    self.paused_event_lost = true;
                    return err("cannot set browser event timeout");
                }
            }
            let raw = match self.ws.read() {
                Ok(Message::Text(t)) => t.to_string(),
                Ok(Message::Binary(b)) => match String::from_utf8(b.to_vec()) {
                    Ok(text) => text,
                    Err(_) => {
                        self.paused_event_lost = true;
                        return err("invalid browser event encoding");
                    }
                },
                Ok(Message::Close(_)) => {
                    self.paused_event_lost = true;
                    return err("browser event connection closed");
                }
                Ok(_) => continue,
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(())
                }
                Err(_) => {
                    self.paused_event_lost = true;
                    return err("browser event connection failed");
                }
            };
            let value = match parse_protocol_message(&raw, false) {
                Ok(value) => value,
                Err(_) => {
                    self.paused_event_lost = true;
                    return err("invalid browser event");
                }
            };
            if is_non_read_request(&value) {
                self.observed_non_read_requests = self.observed_non_read_requests.saturating_add(1);
            }
            self.paused_event_lost |= retain_event(&mut self.events, value, raw.len());
        }
        Ok(())
    }

    pub fn take_events(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.events)
    }

    /// Fail one already-buffered Fetch pause in its originating session.
    ///
    /// This is a protocol primitive, not a continuous interception guard:
    /// callers must separately arrange attachment, interception and servicing.
    /// A failed protocol command leaves the event available for diagnosis.
    pub fn fail_next_paused_request(&mut self) -> Result<bool, CdpError> {
        if self.paused_event_lost {
            return err("paused request event lost; interception state is incomplete");
        }
        let Some((_, event)) = self
            .events
            .iter()
            .find(|(name, _)| name == "Fetch.requestPaused")
        else {
            return Ok(false);
        };
        let request_id = event
            .pointer("/params/requestId")
            .and_then(Value::as_str)
            .ok_or_else(|| CdpError("paused request has no request identifier".into()))?
            .to_owned();
        let session = event
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.call_inner(
            "Fetch.failRequest",
            json!({"requestId":request_id,"errorReason":"BlockedByClient"}),
            session.clone(),
        )?;
        if self.paused_event_lost {
            return err("paused request event lost; interception state is incomplete");
        }
        if let Some(index) = self.events.iter().position(|(name, event)| {
            name == "Fetch.requestPaused"
                && event.pointer("/params/requestId").and_then(Value::as_str)
                    == Some(request_id.as_str())
                && event.get("sessionId").and_then(Value::as_str) == session.as_deref()
        }) {
            self.events.remove(index);
        }
        Ok(true)
    }

    pub fn observed_non_read_requests(&self) -> u64 {
        self.observed_non_read_requests
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
    fn checked_pump_distinguishes_idle_from_invalid_protocol() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            std::thread::sleep(Duration::from_millis(120));
            socket.send(Message::Text("invalid-json".into())).unwrap();
        });
        let (socket, _) = tungstenite::connect(format!("ws://{addr}")).unwrap();
        let mut cdp = Cdp {
            ws: socket,
            next_id: 0,
            session: String::new(),
            events: vec![],
            timeout: Duration::from_secs(2),
            observed_non_read_requests: 0,
            paused_event_lost: false,
        };
        assert!(cdp.pump_checked(Duration::from_millis(30)).is_ok());
        assert!(!cdp.paused_event_lost);
        assert!(cdp.pump_checked(Duration::from_secs(1)).is_err());
        assert!(cdp.fail_next_paused_request().is_err());
        worker.join().unwrap();
    }

    #[test]
    fn paused_requests_are_preserved_and_overflow_is_reported() {
        let mut events = Vec::new();
        for index in 0..MAX_EVENTS {
            assert!(!retain_event(
                &mut events,
                json!({"method":"Fetch.requestPaused","sessionId":"s","params":{"requestId":format!("r{index}"),"request":{"method":"GET"}}}),
                100
            ));
        }
        assert!(!retain_event(
            &mut events,
            json!({"method":"Other.event"}),
            100
        ));
        assert_eq!(events.len(), MAX_EVENTS);
        assert!(events.iter().all(|(name, _)| name == "Fetch.requestPaused"));
        assert!(retain_event(
            &mut events,
            json!({"method":"Fetch.requestPaused","sessionId":"s","params":{"requestId":"overflow"}}),
            100
        ));
        assert!(retain_event(
            &mut events,
            json!({"method":"Fetch.requestPaused","sessionId":"","params":{"requestId":"bad-session"}}),
            100
        ));
        assert_eq!(events.len(), MAX_EVENTS);
    }

    #[cfg(windows)]
    #[test]
    fn late_browser_owner_cleans_its_unclaimed_profile() {
        use std::sync::atomic::Ordering;
        let temp = std::env::temp_dir();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let profile = temp.join(format!(
            "waypoint-owner-timeout-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&profile).unwrap();
        let system_root = std::env::var_os("SystemRoot").unwrap();
        let mut command = Command::new(PathBuf::from(system_root).join("System32/cmd.exe"));
        command.args(["/C", "ping -n 10 127.0.0.1 >nul"]);
        struct ResetDelay;
        impl Drop for ResetDelay {
            fn drop(&mut self) {
                TEST_OWNER_READY_DELAY_MS.store(0, Ordering::SeqCst);
            }
        }
        let _reset = ResetDelay;
        TEST_OWNER_READY_DELAY_MS.store(200, Ordering::SeqCst);
        let result = OwnedBrowserChild::spawn(&mut command, Duration::from_millis(25), &profile);
        assert!(
            result.is_err(),
            "the owner should time out before the delayed handoff"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while profile.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !profile.exists(),
            "late owner did not remove its private profile"
        );
    }

    #[test]
    fn non_read_observation_does_not_retain_request_contents() {
        let request = json!({"method":"Network.requestWillBeSent","params":{"request":{"method":"POST","url":"https://example.invalid/private","headers":{"Authorization":"fictional-secret"},"postData":"fictional-body"}}});
        assert!(is_non_read_request(&request));
        let mut events = Vec::new();
        retain_event(&mut events, request, 100);
        assert!(events.is_empty());
        for method in ["GET", "head"] {
            assert!(!is_non_read_request(
                &json!({"method":"Network.requestWillBeSent","params":{"request":{"method":method}}})
            ));
        }
        let frame = json!({"method":"Network.webSocketFrameSent","params":{"response":{"payloadData":"fictional-private-data"}}});
        assert!(is_non_read_request(&frame));
        retain_event(&mut events, frame, 100);
        assert!(events.is_empty());
        assert!(!is_non_read_request(
            &json!({"method":"Network.webSocketFrameReceived"})
        ));
    }

    #[test]
    fn paused_request_retains_only_bounded_routing_metadata() {
        let mut events = Vec::new();
        let event = json!({"method":"Fetch.requestPaused","sessionId":"session-1","params":{"requestId":"req-1","request":{"method":"POST","url":"https://example.invalid/private","headers":{"Authorization":"fictional-secret"},"postData":"fictional-body"}}});
        retain_event(&mut events, event, 100);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["params"]["requestId"], "req-1");
        assert_eq!(events[0].1["params"]["request"]["method"], "POST");
        assert_eq!(events[0].1["sessionId"], "session-1");
        let stored = events[0].1.to_string();
        assert!(
            !stored.contains("fictional-secret")
                && !stored.contains("fictional-body")
                && !stored.contains("example.invalid")
        );
        retain_event(
            &mut events,
            json!({"method":"Fetch.requestPaused","params":{"requestId":"x".repeat(513)}}),
            600,
        );
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn deeper_dom_decoder_is_bounded_and_ignores_brackets_in_strings() {
        let quoted = r#"{"value":"[{}] \" ["}"#;
        assert!(bounded_json_depth(quoted, 1));
        assert!(parse_protocol_message(quoted, true).is_ok());
        let nested = format!("{}0{}", "[".repeat(160), "]".repeat(160));
        assert!(parse_protocol_message(&nested, false).is_err());
        assert!(parse_protocol_message(&nested, true).is_ok());
        let excessive = format!(
            "{}0{}",
            "[".repeat(MAX_DOM_JSON_DEPTH + 1),
            "]".repeat(MAX_DOM_JSON_DEPTH + 1)
        );
        assert!(parse_protocol_message(&excessive, true)
            .unwrap_err()
            .contains("nesting limit"));
        assert!(!bounded_json_depth("[}", 224) || parse_protocol_message("[}", true).is_err());
        assert!(!bounded_json_depth("[\"unterminated", 224));
    }

    #[cfg(windows)]
    #[test]
    fn registered_browser_values_are_bounded_to_matching_absolute_executables() {
        let valid = b"HKEY_LOCAL_MACHINE\\fixture\r\n    (Default)    REG_SZ    C:\\Custom Browser\\Application\\msedge.exe\r\n";
        assert_eq!(
            parse_registered_browser(valid, "msedge.exe"),
            Some(PathBuf::from(r"C:\Custom Browser\Application\msedge.exe"))
        );
        assert!(parse_registered_browser(valid, "chrome.exe").is_none());
        assert!(parse_registered_browser(
            b"    (Default)    REG_SZ    relative\\chrome.exe\r\n",
            "chrome.exe"
        )
        .is_none());
        assert!(parse_registered_browser(
            b"    (Default)    REG_EXPAND_SZ    %LOCALAPPDATA%\\chrome.exe\r\n",
            "chrome.exe"
        )
        .is_none());
        assert!(parse_registered_browser(b"    (Default)    REG_SZ    C:\\a\\chrome.exe\r\n    (Default)    REG_SZ    C:\\b\\chrome.exe\r\n", "chrome.exe").is_none());
        assert!(parse_registered_browser(&[0xff, 0xfe], "chrome.exe").is_none());
        assert!(same_binary_path(
            Path::new(r"C:\Browser\CHROME.EXE"),
            Path::new("c:/browser/chrome.exe")
        ));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "reads this Windows host's browser App Paths registry entries"]
    fn live_registered_browser_discovery_reads_windows_app_paths() {
        let paths = registered_browsers();
        println!("Registered browser paths: {paths:?}");
        assert!(
            paths.iter().any(|path| path.is_file()),
            "no existing registered browser on this host"
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "starts one registered local browser using a temporary profile"]
    fn live_registered_browser_starts_with_private_profile() {
        let paths: Vec<_> = registered_browsers()
            .into_iter()
            .filter(|path| path.is_file())
            .collect();
        let browser =
            BrowserProcess::launch_candidates(&paths, Duration::from_secs(20), false).unwrap();
        assert_ne!(browser.port, 0);
        assert!(browser.user_data_dir.is_dir());
    }

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
    fn discovery_prefers_explicit_then_installed_and_retains_bundled_fallback() {
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
        let installed = root.join("installed.exe");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"fixture").unwrap();
        std::fs::write(&explicit, b"fixture").unwrap();
        std::fs::write(&installed, b"fixture").unwrap();
        let found = candidate_binaries(
            Some(explicit.clone()),
            Some(&root),
            vec![installed.clone(), bundled.clone(), root.join("missing.exe")],
        );
        assert_eq!(found, vec![explicit, installed, bundled]);
        let many: Vec<_> = (0..15)
            .map(|n| {
                let path = root.join(format!("installed-{n}.exe"));
                std::fs::write(&path, b"fixture").unwrap();
                path
            })
            .collect();
        let found = candidate_binaries(None, Some(&root), many);
        assert_eq!(found.len(), 12);
        assert_eq!(
            found.last(),
            Some(&root.join("dependencies/chromium/chrome.exe"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn browser_startup_budget_reserves_time_for_fallback() {
        let remaining = Duration::from_secs(20);
        let first = candidate_budget(remaining, 0, 12);
        assert!(first > Duration::ZERO);
        assert!(first * 11 <= Duration::from_secs(15));
        assert_eq!(
            candidate_budget(Duration::from_secs(5), 11, 12),
            Duration::from_secs(5)
        );
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
    fn document_status_survives_event_flood_without_storing_headers_or_urls() {
        let mut events = Vec::new();
        retain_event(
            &mut events,
            json!({"method":"Network.responseReceived","params":{"type":"Document","frameId":"frame","loaderId":"loader","response":{"status":404,"url":"https://example.invalid/private","headers":{"Authorization":"secret"}}}}),
            MAX_EVENT_BYTES + 1,
        );
        for index in 0..MAX_EVENTS * 3 {
            retain_event(
                &mut events,
                json!({"method":"Runtime.event","index":index}),
                100,
            );
        }
        assert_eq!(events.len(), MAX_EVENTS);
        let event = events
            .iter()
            .find(|(name, _)| name == "Network.responseReceived")
            .unwrap();
        assert_eq!(event.1.pointer("/params/status"), Some(&json!(404)));
        let serialized = event.1.to_string();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("example.invalid"));
        retain_event(
            &mut events,
            json!({"method":"Network.responseReceived","params":{"type":"Image","response":{"status":404}}}),
            100,
        );
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "Network.responseReceived")
                .count(),
            1
        );
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
            observed_non_read_requests: 0,
            paused_event_lost: false,
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
            observed_non_read_requests: 0,
            paused_event_lost: false,
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
