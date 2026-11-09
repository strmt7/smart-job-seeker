//! Recover missing tools into immutable per-user versions; never install globally.
use serde_json::{json, Value};
#[cfg(any(windows, test))]
use std::io::Write;
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[cfg(all(test, windows))]
#[path = "runtime_setup_qualification.rs"]
mod qualification;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Browser,
    Office,
    Ollama,
}
impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Office => "office",
            Self::Ollama => "ollama",
        }
    }
    fn relative(self) -> &'static str {
        match self {
            Self::Browser => "app/dependencies/chromium/chrome-headless-shell.exe",
            Self::Office => "app/dependencies/libreoffice/program/soffice.com",
            Self::Ollama => "app/dependencies/ollama/ollama.exe",
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "browser" => Ok(Self::Browser),
            "office" => Ok(Self::Office),
            "ollama" => Ok(Self::Ollama),
            _ => Err("Unknown runtime component".into()),
        }
    }
}

struct Task {
    id: String,
    phase: String,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);
#[cfg(windows)]
static INSTALL: Mutex<()> = Mutex::new(());

fn checked_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn pointer(control: &Path, tool: Tool) -> PathBuf {
    control.join(format!("tool-{}.json", tool.name()))
}
fn complete_inventory(root: &Path, tool: Tool) -> bool {
    let mut bytes = Vec::new();
    let Ok(mut file) =
        std::fs::File::open(root.join("inventory.json")).map(|file| file.take(8 * 1024 * 1024 + 1))
    else {
        return false;
    };
    if file.read_to_end(&mut bytes).is_err() || bytes.len() > 8 * 1024 * 1024 {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    let Some(files) = value["files"].as_object() else {
        return false;
    };
    if value["tool"] != tool.name()
        || files.is_empty()
        || files.len() > 50000
        || !files.contains_key(tool.relative().trim_start_matches("app/"))
    {
        return false;
    }
    files.iter().all(|(relative, digest)| {
        !relative.is_empty()
            && !relative
                .chars()
                .any(|c| c.is_control() || "\\:*?\"<>|".contains(c))
            && relative.split('/').all(|part| {
                !part.is_empty() && !matches!(part, "." | "..") && !part.ends_with(['.', ' '])
            })
            && digest
                .as_str()
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            && root.join("app").join(relative).is_file()
    })
}
fn managed_at(control: &Path, tool: Tool) -> Option<PathBuf> {
    let mut bytes = Vec::new();
    std::fs::File::open(pointer(control, tool))
        .ok()?
        .take(257)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 256 {
        return None;
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let id = value["id"].as_str()?;
    if !checked_id(id) {
        return None;
    }
    let root = control.join("runtime-tools").join(id);
    let binary = root.join(tool.relative());
    (binary.is_file() && complete_inventory(&root, tool)).then_some(binary)
}
pub(crate) fn managed(tool: Tool) -> Option<PathBuf> {
    managed_at(&crate::storage_settings::control_dir().ok()?, tool)
}
pub(crate) fn browser_path() -> Option<PathBuf> {
    if std::env::var_os("WAYPOINT_BROWSER").is_none() {
        if let Some(path) = managed(Tool::Browser) {
            return Some(path);
        }
    }
    waypoint_browser::cdp::BrowserProcess::discover_binaries()
        .into_iter()
        .next()
}
pub(crate) fn launch_browser(
    upload_dirs: Vec<PathBuf>,
    visible: bool,
) -> Result<waypoint_browser::CdpDriver, waypoint_applications::driver::DriverError> {
    if std::env::var_os("WAYPOINT_BROWSER").is_none() {
        if let Some(binary) = managed(Tool::Browser) {
            return waypoint_browser::CdpDriver::launch_with_binary(upload_dirs, visible, &binary);
        }
        #[cfg(windows)]
        if waypoint_browser::cdp::BrowserProcess::discover_binaries().is_empty() {
            let binary =
                ensure(Tool::Browser).map_err(waypoint_applications::driver::DriverError)?;
            return waypoint_browser::CdpDriver::launch_with_binary(upload_dirs, visible, &binary);
        }
    }
    waypoint_browser::CdpDriver::launch_with_mode(upload_dirs, visible)
}
#[cfg(any(windows, test))]
fn activate(control: &Path, tool: Tool, id: &str) -> Result<(), String> {
    if !checked_id(id)
        || !control
            .join("runtime-tools")
            .join(id)
            .join(tool.relative())
            .is_file()
        || !complete_inventory(&control.join("runtime-tools").join(id), tool)
    {
        return Err("Cannot activate an incomplete runtime".into());
    }
    let mut file = tempfile::NamedTempFile::new_in(control).map_err(|e| e.to_string())?;
    write!(file, "{}", json!({"id":id}))
        .and_then(|_| file.flush())
        .map_err(|e| e.to_string())?;
    file.persist(pointer(control, tool))
        .map_err(|e| format!("Cannot activate runtime: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn check_setup_memory() -> Result<(), String> {
    let mut system = sysinfo::System::new()
        .map_err(|_| "System memory could not be sampled; tool setup paused")?;
    system.refresh_memory();
    setup_memory_gate(system.available_memory(), system.total_memory())
}
#[cfg(any(windows, test))]
fn setup_memory_gate(available: u64, total: u64) -> Result<(), String> {
    if total == 0 || available > total || available < 3 * 1024 * 1024 * 1024 {
        return Err("Tool setup needs at least 3 GiB of available host RAM; existing tools were left unchanged".into());
    }
    Ok(())
}
#[cfg(windows)]
fn private_python(control: &Path) -> Result<PathBuf, String> {
    check_setup_memory()?;
    let verify = |python: &Path| {
        let mut cmd = std::process::Command::new(python);
        cmd.args([
            "-I",
            "-c",
            "import json,tomllib,ssl,zipfile,pypdf,pypdfium2,PIL.Image",
        ]);
        let output = crate::pdf_job::run(&cmd, None, 256 * 1024 * 1024, 30, 0)?;
        output
            .success
            .then_some(())
            .ok_or_else(|| "Private Python verification failed".into())
    };
    let _lease = crate::pdf_extract::acquire()?;
    if let Some(python) = crate::pdf_setup::managed_python(control).filter(|p| verify(p).is_ok()) {
        return Ok(python);
    }
    crate::pdf_runtime::install(control, &crate::random_id()?, verify).map(PathBuf::from)
}

#[cfg(windows)]
fn install(control: &Path, tool: Tool, python: &Path) -> Result<Value, String> {
    check_setup_memory()?;
    let root = control.join("runtime-tools");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // Retain failed stages: an MSI administrative extraction can outlive its
    // client after a service timeout. Never delete a directory it may still use.
    let stage = tempfile::Builder::new()
        .prefix(".setup-")
        .tempdir_in(&root)
        .map_err(|e| e.to_string())?
        .keep();
    let outcome = (|| {
        for (relative, data) in [
            (
                "apps/web_app/scripts/package-windows.py",
                include_bytes!("../scripts/package-windows.py").as_slice(),
            ),
            (
                "apps/web_app/scripts/runtime-setup.py",
                include_bytes!("../scripts/runtime-setup.py").as_slice(),
            ),
            (
                "apps/web_app/scripts/provision-vcrt.ps1",
                include_bytes!("../scripts/provision-vcrt.ps1").as_slice(),
            ),
            (
                "apps/web_app/windows-runtime-artifacts.json",
                include_bytes!("../windows-runtime-artifacts.json").as_slice(),
            ),
            (
                "docs/MSVC_RUNTIME_LICENSE.docx",
                include_bytes!("../../../docs/MSVC_RUNTIME_LICENSE.docx").as_slice(),
            ),
            (
                "docs/OLLAMA_LICENSE.txt",
                include_bytes!("../../../docs/OLLAMA_LICENSE.txt").as_slice(),
            ),
        ] {
            let dest = stage.join(relative);
            std::fs::create_dir_all(dest.parent().ok_or("Invalid private tool path")?)
                .map_err(|e| e.to_string())?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dest)
                .map_err(|e| e.to_string())?;
            file.write_all(data).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir(stage.join("target")).map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new(python);
        cmd.args(["-I"])
            .arg(stage.join("apps/web_app/scripts/runtime-setup.py"))
            .arg(tool.name());
        let result = crate::pdf_job::run(&cmd, None, 768 * 1024 * 1024, 1200, 0)?;
        let mut bytes = Vec::new();
        std::fs::File::open(stage.join("result.json"))
            .map_err(|e| format!("Runtime setup produced no report: {e}"))?
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8192 {
            return Err("Runtime setup report is too large".into());
        }
        let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if !result.success || report["ok"] != true {
            return Err(report["error"]
                .as_str()
                .unwrap_or("Runtime setup failed")
                .to_string());
        }
        let id = crate::random_id()?;
        std::fs::rename(stage.join("output"), root.join(&id))
            .map_err(|e| format!("Cannot publish runtime: {e}"))?;
        activate(control, tool, &id)?;
        Ok(
            json!({"tool":tool.name(),"path":root.join(id).join(tool.relative()).display().to_string(),"details":report["result"]}),
        )
    })();
    match outcome {
        Ok(value) => {
            if let Err(error) = std::fs::remove_dir_all(&stage) {
                return Ok(
                    json!({"runtime":value,"cleanup_warning":format!("Runtime activated; setup files remain at {}: {error}",stage.display())}),
                );
            }
            Ok(value)
        }
        Err(error) => Err(format!(
            "{error}. Previous runtimes were preserved. Diagnostic setup files: {}",
            stage.display()
        )),
    }
}

#[cfg(windows)]
pub(crate) fn ensure(tool: Tool) -> Result<PathBuf, String> {
    if let Some(path) = managed(tool) {
        return Ok(path);
    }
    #[cfg(test)]
    {
        Err("Missing tools are never downloaded implicitly in ordinary tests; use the explicit private-runtime qualification".into())
    }
    #[cfg(not(test))]
    {
        let _guard = INSTALL
            .try_lock()
            .map_err(|_| "Tool setup is already running; check Settings > Local tools and setup")?;
        let control = crate::storage_settings::control_dir()?;
        std::fs::create_dir_all(&control).map_err(|e| e.to_string())?;
        let python = private_python(&control)?;
        install(&control, tool, &python)?;
        managed_at(&control, tool).ok_or_else(|| "Installed runtime is not available".into())
    }
}
#[cfg(windows)]
pub(crate) fn ensure_pdf_tools() -> Result<PathBuf, String> {
    #[cfg(test)]
    {
        Err("Implicit PDF downloads are disabled in ordinary tests".into())
    }
    #[cfg(not(test))]
    {
        let _guard = INSTALL
            .try_lock()
            .map_err(|_| "Tool setup is already running; check Settings > Local tools and setup")?;
        let control = crate::storage_settings::control_dir()?;
        std::fs::create_dir_all(&control).map_err(|e| e.to_string())?;
        private_python(&control)
    }
}

#[cfg(windows)]
fn available(tool: Tool) -> bool {
    if managed(tool).is_some() {
        return true;
    }
    match tool {
        Tool::Browser => !waypoint_browser::cdp::BrowserProcess::discover_binaries().is_empty(),
        Tool::Office => crate::document_convert::office_path().is_some(),
        Tool::Ollama => crate::runtime_start::available(),
    }
}
pub fn active() -> Result<Option<String>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Tool setup unavailable")?
        .as_ref()
        .map(|task| task.id.clone()))
}
pub fn start(id: String, choice: &str) -> Result<(), String> {
    if !checked_id(&id) {
        return Err("Invalid tool setup identity".into());
    }
    let tools = if choice == "missing" {
        vec![Tool::Browser, Tool::Office, Tool::Ollama]
    } else {
        vec![Tool::parse(choice)?]
    };
    let missing_only = choice == "missing";
    let mut task = TASK.lock().map_err(|_| "Tool setup unavailable")?;
    if task.is_some() {
        return Err("Tool setup is running or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        phase: "Checking private runtimes".into(),
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| setup(&id, tools, missing_only))
            .unwrap_or_else(|_| Err("Tool setup stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}
#[cfg(windows)]
fn setup(id: &str, tools: Vec<Tool>, missing_only: bool) -> Result<Value, String> {
    let _guard = INSTALL
        .try_lock()
        .map_err(|_| "Another automatic tool setup is running")?;
    let control = crate::storage_settings::control_dir()?;
    std::fs::create_dir_all(&control).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    let python = private_python(&control)?;
    results.push(json!({"tool":"pdf","path":python.display().to_string()}));
    for tool in tools {
        if missing_only && available(tool) {
            results.push(json!({"tool":tool.name(),"already_available":true}));
            continue;
        }
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|task| task.id == id) {
                task.phase = format!(
                    "Installing {} privately; large downloads can take several minutes",
                    tool.name()
                );
            }
        }
        results.push(install(&control, tool, &python)?);
    }
    Ok(json!({"components":results}))
}
#[cfg(not(windows))]
fn setup(_id: &str, _tools: Vec<Tool>, _missing_only: bool) -> Result<Value, String> {
    Err("Automatic runtime setup supports Windows x64 only".into())
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Tool setup unavailable")?;
    let mut completed = COMPLETED.lock().map_err(|_| "Tool setup unavailable")?;
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        return completed
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| "Tool setup not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running","phase":task.phase}));
    };
    let value = match result {
        Ok(result) => json!({"status":"complete","result":result}),
        Err(error) => json!({"status":"failed","error":error}),
    };
    *completed = Some((id.to_string(), value.clone()));
    *tasks = None;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_pressure_and_invalid_samples_never_start_setup() {
        let gib = 1024 * 1024 * 1024;
        assert!(setup_memory_gate(3 * gib, 16 * gib).is_ok());
        for (available, total) in [(3 * gib - 1, 16 * gib), (0, 0), (5 * gib, 4 * gib)] {
            assert!(setup_memory_gate(available, total).is_err());
        }
    }
    #[test]
    fn pointer_refuses_traversal_oversize_missing_and_wrong_component() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path();
        for id in ["../outside", "", &"f".repeat(33), &"z".repeat(32)] {
            std::fs::write(
                pointer(control, Tool::Browser),
                json!({"id":id}).to_string(),
            )
            .unwrap();
            assert!(managed_at(control, Tool::Browser).is_none());
        }
        std::fs::write(pointer(control, Tool::Browser), " ".repeat(300)).unwrap();
        assert!(managed_at(control, Tool::Browser).is_none());
        assert!(activate(control, Tool::Browser, &"a".repeat(32)).is_err());
    }
    #[test]
    fn failed_activation_preserves_previous_pointer_and_other_components() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path();
        let id = "b".repeat(32);
        let root = control.join("runtime-tools").join(&id);
        let binary = root.join(Tool::Office.relative());
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, b"fictional").unwrap();
        std::fs::write(root.join("inventory.json"), json!({"tool":"office","files":{"dependencies/libreoffice/program/soffice.com":"a".repeat(64)}}).to_string()).unwrap();
        activate(control, Tool::Office, &id).unwrap();
        assert!(activate(control, Tool::Office, &"c".repeat(32)).is_err());
        assert_eq!(managed_at(control, Tool::Office), Some(binary));
        std::fs::remove_file(root.join(Tool::Office.relative())).unwrap();
        assert!(managed_at(control, Tool::Office).is_none());
        assert!(managed_at(control, Tool::Browser).is_none());
    }
    #[test]
    fn setup_rejects_unknown_choices_before_starting() {
        assert!(start("d".repeat(32), "../office").is_err());
        assert!(start("bad".into(), "missing").is_err());
    }

    #[test]
    fn completed_results_are_repeatable_and_foreign_ids_do_not_consume_them() {
        let id = "e".repeat(32);
        *TASK.lock().unwrap() = Some(Task {
            id: id.clone(),
            phase: "Check".into(),
            result: Some(Ok(json!({"components":[]}))),
        });
        assert!(poll(&"f".repeat(32)).is_err());
        let value = poll(&id).unwrap();
        assert_eq!(value["status"], "complete");
        assert_eq!(poll(&id).unwrap(), value);
        assert!(active().unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "explicit pinned downloads into a temporary private control folder; headless browser smoke"]
    fn private_runtime_setup_downloads_activates_and_runs_a_browser() {
        let control = tempfile::Builder::new()
            .prefix("runtime-repair-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap()
            .keep();
        println!("Private runtime setup evidence: {}", control.display());
        let python = private_python(&control).unwrap();
        let result = install(&control, Tool::Browser, &python).unwrap();
        let binary = managed_at(&control, Tool::Browser).unwrap();
        let mut driver =
            waypoint_browser::CdpDriver::launch_with_binary(vec![], false, &binary).unwrap();
        use waypoint_applications::driver::BrowserDriver;
        let fixture = control.join("fictional.html");
        std::fs::write(&fixture,"<!doctype html><title>Fictional setup check</title><label>Name<input name='name'></label>").unwrap();
        let schema = driver
            .open(url::Url::from_file_path(&fixture).unwrap().as_str())
            .unwrap();
        assert_eq!(schema.schema.fields.len(), 1);
        assert!(!driver.screenshot_base64().unwrap().is_empty());
        drop(driver);
        std::fs::write(control.join("qualification.private.json"),json!({"complete":true,"scope":"temporary private runtime and fictional read-only browser fixture","result":result,"sources":{"setup":waypoint_domain::sha256_hex(include_str!("runtime_setup.rs").as_bytes()),"worker":waypoint_domain::sha256_hex(include_str!("../scripts/runtime-setup.py").as_bytes()),"packager":waypoint_domain::sha256_hex(include_str!("../scripts/package-windows.py").as_bytes()),"catalog":waypoint_domain::sha256_hex(include_str!("../windows-runtime-artifacts.json").as_bytes())}}).to_string()).unwrap();
    }
}
