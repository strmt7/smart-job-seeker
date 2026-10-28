//! Install optional PDF dependencies into a new per-user virtual environment.
use serde_json::{json, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};
#[cfg(not(windows))]
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

struct Task {
    id: String,
    outcome: Option<Result<String, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

pub fn managed_python(control: &Path) -> Option<PathBuf> {
    let id = std::fs::read_to_string(control.join("pdf-runtime.json")).ok()?;
    if id.len() > 256 {
        return None;
    }
    let value: Value = serde_json::from_str(&id).ok()?;
    let id = value["id"].as_str()?;
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let python = control
        .join("pdf-runtimes")
        .join(id)
        .join("Scripts")
        .join("python.exe");
    python.is_file().then_some(python)
}
fn run(command: &mut Command, seconds: u64, label: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let result = crate::pdf_job::run(command, None, 768 * 1024 * 1024, seconds, 0)?;
        if result.success {
            Ok(())
        } else {
            Err(format!(
                "{label} failed ({:?}); check Python compatibility and internet access",
                result.code
            ))
        }
    }
    #[cfg(not(windows))]
    {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start {label}: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(format!(
                        "{label} failed ({status}); check Python compatibility and internet access"
                    ))
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{label} timed out or could not be monitored"));
                }
            }
        }
    }
}

fn install(control: &Path, id: &str, python: &Path) -> Result<String, String> {
    let _lease = crate::pdf_extract::acquire()?;
    if id.len() != 32
        || !id.bytes().all(|b| b.is_ascii_hexdigit())
        || !python.is_absolute()
        || !python.is_file()
    {
        return Err("Choose an existing absolute Python executable path".into());
    }
    let root = control.join("pdf-runtimes");
    std::fs::create_dir_all(&root).map_err(|e| format!("Cannot create PDF tools folder: {e}"))?;
    let target = root.join(id);
    std::fs::create_dir(&target)
        .map_err(|e| format!("Cannot create a new PDF tools environment: {e}"))?;
    let setup = run(
        Command::new(python).args(["-I", "-m", "venv"]).arg(&target),
        90,
        "Python environment setup",
    );
    if let Err(e) = setup {
        return Err(format!("{e}. Partial files are in {}", target.display()));
    }
    let venv_python = target.join("Scripts").join("python.exe");
    let pip = run(
        Command::new(&venv_python).args([
            "-I",
            "-m",
            "pip",
            "--isolated",
            "install",
            "--no-input",
            "--disable-pip-version-check",
            "--no-cache-dir",
            "--only-binary=:all:",
            "--no-deps",
            "--timeout",
            "15",
            "--retries",
            "1",
            "--index-url",
            "https://pypi.org/simple",
            "pypdf==6.10.0",
            "pypdfium2==5.14.0",
            "Pillow==12.3.0",
        ]),
        180,
        "PDF dependency installation",
    );
    if let Err(e) = pip {
        return Err(format!("{e}. Partial files are in {}", target.display()));
    }
    run(
        Command::new(&venv_python).args([
            "-I",
            "-c",
            "import pypdf, pypdfium2, PIL.Image; print('ok')",
        ]),
        30,
        "PDF dependency check",
    )?;
    let mut file = tempfile::NamedTempFile::new_in(control)
        .map_err(|e| format!("Cannot save PDF tools setting: {e}"))?;
    write!(file, "{}", json!({"id":id}))
        .and_then(|_| file.flush())
        .map_err(|e| format!("Cannot save PDF tools setting: {e}"))?;
    file.persist(control.join("pdf-runtime.json"))
        .map_err(|e| format!("Cannot activate PDF tools: {e}"))?;
    Ok(venv_python.display().to_string())
}
pub fn start(id: String, python: PathBuf) -> Result<(), String> {
    if !python.is_absolute() || !python.is_file() {
        return Err("Choose an existing absolute Python executable path".into());
    }
    let control = crate::storage_settings::control_dir()?;
    let mut task = TASK.lock().map_err(|_| "PDF setup unavailable")?;
    if task.is_some() {
        return Err("PDF setup is running or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        outcome: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| install(&control, &id, &python))
            .unwrap_or_else(|_| Err("PDF setup stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(t) = task.as_mut().filter(|t| t.id == id) {
                t.outcome = Some(outcome);
            }
        }
    });
    Ok(())
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "PDF setup unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|t| t.id == id)
        .ok_or("PDF setup not found")?;
    match task.outcome.take() {
        None => Ok(json!({"status":"running"})),
        Some(Ok(path)) => {
            *tasks = None;
            Ok(json!({"status":"complete","path":path}))
        }
        Some(Err(error)) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    #[ignore = "downloads pinned PDF packages into a temporary isolated environment"]
    fn installs_and_activates_a_private_environment() {
        let root =
            tempfile::tempdir_in(std::env::var_os("WAYPOINT_SETUP_TEST_DIR").unwrap()).unwrap();
        let bootstrap = PathBuf::from(std::env::var_os("WAYPOINT_BOOTSTRAP_PYTHON").unwrap());
        let id = "0123456789abcdef0123456789abcdef";
        let installed = install(root.path(), id, &bootstrap).unwrap();
        assert_eq!(
            managed_python(root.path()).unwrap(),
            PathBuf::from(&installed)
        );
        let fixture = std::env::var_os("WAYPOINT_PDF_FIXTURE").unwrap();
        let input = std::fs::read(fixture).unwrap();
        let mut child = Command::new(&installed)
            .args(["-I", "-c", include_str!("pdf_render.py"), "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let rendered: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(rendered["page"], 0);
        assert!(install(root.path(), id, &bootstrap).is_err());
    }
}
