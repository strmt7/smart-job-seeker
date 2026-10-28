//! Bounded, on-demand local dependency check; never installs anything.
use serde_json::{json, Value};
use std::sync::Mutex;

struct Task {
    id: String,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

pub fn start(id: String) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "PDF check unavailable")?;
    if task.is_some() {
        return Err("A PDF check is running or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(crate::pdf_extract::probe_packages)
            .unwrap_or_else(|_| Err("PDF dependency check stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "PDF check unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("PDF check not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    *tasks = None;
    Ok(match result {
        Ok(versions) => json!({"status":"complete","versions":versions}),
        Err(error) => json!({"status":"failed","error":error}),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "checks an explicitly configured local Python PDF environment"]
    fn reports_installed_pdf_dependencies() {
        super::start("fixture".into()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        loop {
            let result = super::poll("fixture").unwrap();
            if result["status"] == "complete" {
                for key in ["python", "pypdf", "pypdfium2", "pillow"] {
                    assert!(!result["versions"][key].as_str().unwrap().is_empty());
                }
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
