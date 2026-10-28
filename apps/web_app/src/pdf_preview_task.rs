//! One bounded, on-demand PDF page rendering task.
use serde_json::{json, Value};
use std::sync::Mutex;
struct Task {
    id: String,
    outcome: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
pub fn start(id: String, bytes: Vec<u8>, page: usize) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "PDF preview unavailable")?;
    if task.as_ref().is_some_and(|t| t.outcome.is_none()) {
        return Err("A PDF preview is already running".into());
    }
    *task = Some(Task {
        id: id.clone(),
        outcome: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| crate::pdf_extract::render_page(&bytes, page))
            .unwrap_or_else(|_| Err("PDF preview stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|t| t.id == id) {
                task.outcome = Some(outcome);
            }
        }
    });
    Ok(())
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "PDF preview unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|t| t.id == id)
        .ok_or("PDF preview not found")?;
    match task.outcome.take() {
        None => Ok(json!({"status":"running"})),
        Some(Ok(page)) => {
            *tasks = None;
            Ok(json!({"status":"complete","page":page}))
        }
        Some(Err(error)) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
    }
}
