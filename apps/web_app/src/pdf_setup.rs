//! Install PDF tools in an app-private runtime, with an optional custom interpreter.
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
// Retain the latest completed result so a lost HTTP response can be retried.
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

/// Recover a setup whose start response was lost. Results are retained until polled.
pub fn pending() -> Result<Option<String>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "PDF setup unavailable")?
        .as_ref()
        .map(|task| task.id.clone()))
}

#[cfg(test)]
fn select_python(
    configured: &str,
    discover: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf, String> {
    let configured = configured.trim();
    if configured.is_empty() {
        discover().ok_or_else(|| "No compatible local Python with venv and ensurepip was found among the checked candidates. Install a compatible Python or choose its executable for PDF setup.".into())
    } else {
        Ok(PathBuf::from(configured))
    }
}

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
    let root = control.join("pdf-runtimes").join(id);
    let python = match value["kind"].as_str() {
        Some("portable") => root.join("python.exe"),
        Some("venv") | None => root.join("Scripts").join("python.exe"),
        _ => return None,
    };
    python.is_file().then_some(python)
}

pub(crate) fn activate(control: &Path, id: &str, kind: &str) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(control)
        .map_err(|e| format!("Cannot save PDF tools setting: {e}"))?;
    write!(file, "{}", json!({"id":id,"kind":kind}))
        .and_then(|_| file.flush())
        .map_err(|e| format!("Cannot save PDF tools setting: {e}"))?;
    file.persist(control.join("pdf-runtime.json"))
        .map_err(|e| format!("Cannot activate PDF tools: {e}"))?;
    Ok(())
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
            "pypdf==7.0.0",
            "pypdfium2==5.15.0",
            "Pillow==13.0.0",
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
    activate(control, id, "venv")?;
    Ok(venv_python.display().to_string())
}
pub fn start(id: String, configured: String) -> Result<(), String> {
    if configured.len() > 32_000 || configured.chars().any(char::is_control) {
        return Err("Invalid Python executable path".into());
    }
    if !configured.trim().is_empty() {
        let python = PathBuf::from(configured.trim());
        if !python.is_absolute() || !python.is_file() {
            return Err("Choose an existing absolute Python executable path".into());
        }
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
        let outcome = std::panic::catch_unwind(|| {
            if configured.trim().is_empty() {
                let _lease = crate::pdf_extract::acquire()?;
                crate::pdf_runtime::install(&control, &id, |python| {
                    run(Command::new(python).args(["-I", "-c", "import pypdf,pypdfium2,PIL.Image; assert pypdf.__version__ == '7.0.0'; assert PIL.__version__ == '13.0.0'"]), 30, "PDF runtime verification")
                })
            } else {
                install(&control, &id, &PathBuf::from(configured.trim()))
            }
        })
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
    let mut completed = COMPLETED.lock().map_err(|_| "PDF setup unavailable")?;
    let Some(task) = tasks.as_mut().filter(|t| t.id == id) else {
        return completed
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone())
            .ok_or_else(|| "PDF setup not found".into());
    };
    let Some(outcome) = task.outcome.take() else {
        return Ok(json!({"status":"running"}));
    };
    let result = match outcome {
        Ok(path) => json!({"status":"complete","path":path}),
        Err(error) => json!({"status":"failed","error":error}),
    };
    *completed = Some((id.to_owned(), result.clone()));
    *tasks = None;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn selects_discovered_python_only_when_no_path_was_given() {
        let discovered = PathBuf::from("discovered/python.exe");
        assert_eq!(
            select_python("  ", || Some(discovered.clone())).unwrap(),
            discovered
        );
        assert_eq!(
            select_python(" chosen/python.exe ", || panic!(
                "explicit path should not trigger discovery"
            ))
            .unwrap(),
            PathBuf::from("chosen/python.exe")
        );
        assert!(select_python("", || None)
            .unwrap_err()
            .contains("No compatible local Python"));
    }

    #[test]
    fn setup_rejects_invalid_explicit_paths_before_creating_a_task() {
        assert!(start("fixture".into(), "relative/python.exe".into()).is_err());
        assert!(start("fixture".into(), "invalid\npath".into()).is_err());
    }

    #[test]
    fn completed_setup_status_can_be_retried() {
        *TASK.lock().unwrap() = Some(Task {
            id: "fixture".into(),
            outcome: Some(Err("fixture failure".into())),
        });
        assert_eq!(pending().unwrap().as_deref(), Some("fixture"));
        let first = poll("fixture").unwrap();
        assert_eq!(first["status"], "failed");
        assert_eq!(poll("fixture").unwrap(), first);
        assert!(poll("other").is_err());
        assert!(pending().unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "installs pinned PDF packages into an explicitly isolated test profile"]
    fn automatic_setup_bootstraps_without_host_python_and_renders_pdf() {
        let root = PathBuf::from(std::env::var_os("WAYPOINT_SETUP_TEST_DIR").unwrap())
            .canonicalize()
            .unwrap();
        let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap())
            .canonicalize()
            .unwrap();
        assert!(appdata.starts_with(&root));
        let control = crate::storage_settings::control_dir().unwrap();
        std::fs::create_dir_all(&control).unwrap();
        assert!(managed_python(&control).is_none());
        let id = "1234567890abcdef1234567890abcdef";
        start(id.into(), String::new()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
        let result = loop {
            let result = poll(id).unwrap();
            if result["status"] != "running" {
                break result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "isolated setup timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        assert_eq!(result["status"], "complete", "{result}");
        assert_eq!(
            managed_python(&control).unwrap().display().to_string(),
            result["path"].as_str().unwrap()
        );
        let fixture = std::fs::read(std::env::var_os("WAYPOINT_PDF_FIXTURE").unwrap()).unwrap();
        let rendered = crate::pdf_extract::render_page(&fixture, 0).unwrap();
        assert_eq!(rendered["page"], 0);
    }

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
