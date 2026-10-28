//! One background extraction task; no document content is persisted here.
use serde_json::{json, Value};
use std::sync::Mutex;
struct Task {
    id: String,
    outcome: Option<Result<String, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
pub fn start(id: String, name: String, bytes: Vec<u8>) -> Result<(), String> {
    let mut task = TASK
        .lock()
        .map_err(|_| "Extraction task state unavailable")?;
    if task.as_ref().is_some_and(|t| t.outcome.is_none()) {
        return Err("A document extraction is already running".into());
    }
    *task = Some(Task {
        id: id.clone(),
        outcome: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| crate::document_extract::extract(&name, &bytes))
            .unwrap_or_else(|_| Err("Document extraction stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut() {
                if task.id == id {
                    task.outcome = Some(outcome);
                }
            }
        }
    });
    Ok(())
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK
        .lock()
        .map_err(|_| "Extraction task state unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Extraction task not found")?;
    match task.outcome.take() {
        None => Ok(json!({"status":"running"})),
        Some(Ok(text)) => {
            *tasks = None;
            Ok(json!({"status":"complete","text":text}))
        }
        Some(Err(error)) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn background_extraction_returns_text() {
        super::start(
            "fixture".into(),
            "cv.txt".into(),
            b"Fictional qualifications".to_vec(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let result = super::poll("fixture").unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["text"], "Fictional qualifications");
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
