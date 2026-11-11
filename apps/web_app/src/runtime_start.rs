//! Start local Ollama, privately provisioning it when no runtime is available.
use serde_json::{json, Value};
use std::sync::Mutex;

struct Task {
    id: String,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

#[cfg(any(windows, test))]
fn candidates(
    executable: Option<&std::path::Path>,
    local: Option<&std::path::Path>,
    program_files: Option<&std::path::Path>,
    path: Option<&std::ffi::OsStr>,
) -> Vec<std::path::PathBuf> {
    let mut items = Vec::new();
    if let Some(parent) = executable.and_then(std::path::Path::parent) {
        items.push(parent.join("dependencies/ollama/ollama.exe"));
    }
    if let Some(local) = local {
        items.push(local.join("Programs/Ollama/ollama.exe"));
    }
    if let Some(program_files) = program_files {
        items.push(program_files.join("Ollama/ollama.exe"));
    }
    if let Some(path) = path {
        items.extend(
            std::env::split_paths(path)
                .filter(|part| part.is_absolute())
                .map(|part| part.join("ollama.exe")),
        );
    }
    items
}

#[cfg(windows)]
fn runtime_ready(addr: std::net::SocketAddr) -> bool {
    waypoint_inference::ollama::http_request(addr, "GET", "/api/version", None, 1000)
        .ok()
        .is_some_and(|(status, bytes)| {
            status == 200
                && serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .is_some_and(|body| {
                        body["version"]
                            .as_str()
                            .is_some_and(|value| !value.is_empty() && value.len() <= 100)
                    })
        })
}

#[cfg(windows)]
fn classify_port(addr: std::net::SocketAddr) -> Result<bool, String> {
    if runtime_ready(addr) {
        return Ok(true);
    }
    if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).is_ok() {
        return Err(
            "The local runtime port is occupied by an unrecognized service; it was left unchanged"
                .into(),
        );
    }
    Ok(false)
}

#[cfg(windows)]
fn runtime_command(binary: &std::path::Path, addr: std::net::SocketAddr) -> processkit::Command {
    use processkit::{Command, Stdin, StdioMode};
    Command::new(binary)
        .arg("serve")
        .env("OLLAMA_HOST", addr.to_string())
        .env("OLLAMA_MAX_LOADED_MODELS", "1")
        .env("OLLAMA_NUM_PARALLEL", "1")
        .env("OLLAMA_NO_CLOUD", "1")
        .stdin(Stdin::empty())
        .stdout(StdioMode::Null)
        .stderr(StdioMode::Null)
        .create_no_window()
}

#[cfg(windows)]
static OWNED: Mutex<Option<crate::owned_runtime::OwnedRuntime>> = Mutex::new(None);
static SHUTTING_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn shutdown() -> Result<(), String> {
    SHUTTING_DOWN.store(true, std::sync::atomic::Ordering::Release);
    #[cfg(windows)]
    if let Some(mut owner) = OWNED
        .lock()
        .map_err(|_| "Runtime ownership is unavailable")?
        .take()
    {
        owner.stop()?;
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_runtime(
    addr: std::net::SocketAddr,
    child: &crate::owned_runtime::OwnedRuntime,
) -> Result<(), String> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if !child.running()? {
            return Err("The local runtime exited before it became available".into());
        }
        if runtime_ready(addr) && child.running()? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("Runtime startup was not confirmed; its owned process will be stopped".into())
}

#[cfg(windows)]
fn run() -> Result<Value, String> {
    let addr = "127.0.0.1:11434"
        .parse()
        .map_err(|_| "Invalid local runtime address")?;
    if classify_port(addr)? {
        return Ok(json!({"already_running":true}));
    }
    let binary = installed_binary()
        .map(Ok)
        .unwrap_or_else(|| crate::runtime_setup::ensure(crate::runtime_setup::Tool::Ollama))?;
    if SHUTTING_DOWN.load(std::sync::atomic::Ordering::Acquire) {
        return Err("The application is shutting down".into());
    }
    let mut system = sysinfo::System::new().map_err(|_| "Cannot sample runtime memory")?;
    system.refresh_memory();
    let memory =
        crate::owned_runtime::memory_budget(system.available_memory(), system.total_memory())?;
    let mut child =
        crate::owned_runtime::OwnedRuntime::spawn(runtime_command(&binary, addr), memory)?;
    if let Err(error) = wait_for_runtime(addr, &child) {
        return match child.stop() {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!("{error}; runtime cleanup failed: {cleanup}")),
        };
    }
    let identity = child.identity();
    let result = json!({"already_running":false,"owned_by_app":true,"pid":identity.pid(),"creation_token":identity.start_time(),"executable":identity.exe_name(),"parent_pid":identity.ppid(),"process_memory_limit":memory});
    let mut owner = OWNED
        .lock()
        .map_err(|_| "Runtime ownership is unavailable")?;
    if SHUTTING_DOWN.load(std::sync::atomic::Ordering::Acquire) {
        child.stop()?;
        return Err("The application stopped during runtime startup".into());
    }
    if let Some(mut previous) = owner.take() {
        previous.stop()?;
    }
    *owner = Some(child);
    Ok(result)
}
#[cfg(windows)]
pub(crate) fn installed_binary() -> Option<std::path::PathBuf> {
    if let Some(path) = crate::runtime_setup::managed(crate::runtime_setup::Tool::Ollama) {
        return Some(path);
    }
    let executable = std::env::current_exe().ok();
    let local = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let program = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from);
    let path = std::env::var_os("PATH");
    candidates(
        executable.as_deref(),
        local.as_deref(),
        program.as_deref(),
        path.as_deref(),
    )
    .into_iter()
    .find(|candidate| candidate.is_absolute() && candidate.is_file())
}
#[cfg(windows)]
pub(crate) fn available() -> bool {
    installed_binary().is_some()
        || runtime_ready(std::net::SocketAddr::from(([127, 0, 0, 1], 11434)))
}
#[cfg(not(windows))]
fn run() -> Result<Value, String> {
    Err("Starting the local runtime is supported on Windows only".into())
}

pub fn start(id: String) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "Runtime startup unavailable")?;
    if task.is_some() {
        return Err("Runtime startup is running or awaiting its result".into());
    }
    let lease = crate::doc_suggest::lease()?;
    *task = Some(Task {
        id: id.clone(),
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let _lease = lease;
        let result = std::panic::catch_unwind(run).unwrap_or_else(|_| {
            Err("Runtime startup stopped unexpectedly; check status before retrying".into())
        });
        if let Ok(mut tasks) = TASK.lock() {
            if let Some(task) = tasks.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}
pub fn active() -> Result<Option<String>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Runtime startup unavailable")?
        .as_ref()
        .map(|task| task.id.clone()))
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Runtime startup unavailable")?;
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Runtime startup unavailable")?
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| "Runtime startup not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    let result = match result {
        Ok(value) => json!({"status":"complete","result":value}),
        Err(error) => json!({"status":"failed","error":error}),
    };
    *COMPLETED
        .lock()
        .map_err(|_| "Runtime startup unavailable")? = Some((id.into(), result.clone()));
    *tasks = None;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_candidates_prioritize_bundled_then_standard_locations() {
        let executable = std::path::Path::new("C:/fixture/waypoint_web.exe");
        let local = std::path::Path::new("C:/fixture/local");
        let program = std::path::Path::new("C:/fixture/program");
        let items = candidates(Some(executable), Some(local), Some(program), None);
        assert!(items[0].ends_with("dependencies/ollama/ollama.exe"));
        assert!(items[1].ends_with("Programs/Ollama/ollama.exe"));
        assert!(items[2].ends_with("Ollama/ollama.exe"));
        assert!(candidates(None, None, None, None).is_empty());
    }
    #[cfg(windows)]
    #[test]
    fn occupied_non_runtime_port_is_not_replaced() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .unwrap();
            let _ = listener.accept().unwrap();
        });
        assert!(classify_port(addr).unwrap_err().contains("occupied"));
        server.join().unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "starts an isolated local Ollama process without loading a model"]
    fn launches_an_isolated_runtime_without_affecting_existing_service() {
        let existing = crate::resident_model_status().unwrap();
        let local = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
        let binary = candidates(None, local.as_deref(), None, None)
            .into_iter()
            .find(|path| path.is_file())
            .expect("installed Ollama binary");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let directory = tempfile::Builder::new()
            .prefix("runtime-test-")
            .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        let mut child = crate::owned_runtime::OwnedRuntime::spawn(
            runtime_command(&binary, addr).env("OLLAMA_MODELS", directory.path()),
            1024 * 1024 * 1024,
        )
        .unwrap();
        wait_for_runtime(addr, &child).unwrap();
        let (status, body) =
            waypoint_inference::ollama::http_request(addr, "GET", "/api/ps", None, 3000).unwrap();
        assert_eq!(status, 200);
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["models"].as_array().is_some_and(Vec::is_empty));
        child.stop().unwrap();
        assert!(!runtime_ready(addr));
        assert_eq!(crate::resident_model_status().unwrap(), existing);
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires a running local Ollama service; does not start or stop it"]
    fn recognizes_existing_runtime_without_replacing_it() {
        assert!(runtime_ready("127.0.0.1:11434".parse().unwrap()));
        assert_eq!(run().unwrap()["already_running"], true);
    }
}
