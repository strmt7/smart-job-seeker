//! One background extraction task; no document content is persisted here.
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};
struct Task {
    id: String,
    outcome: Option<Result<(String, Option<Value>), String>>,
    completed: Option<Instant>,
    collected: bool,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
const RETENTION: Duration = Duration::from_secs(60);

fn expire_completed(task: &mut Option<Task>) {
    if task
        .as_ref()
        .and_then(|t| t.completed)
        .is_some_and(|at| at.elapsed() >= RETENTION)
    {
        *task = None;
    }
}

pub fn start(id: String, name: String, bytes: Vec<u8>) -> Result<(), String> {
    start_with_mode(id, name, bytes, false)
}

pub fn layout_mode(value: Option<&Value>) -> Result<bool, String> {
    match value {
        None => Ok(false),
        Some(Value::String(mode)) if mode == "normal" => Ok(false),
        Some(Value::String(mode)) if mode == "layout" => Ok(true),
        _ => Err("Invalid document extraction mode".into()),
    }
}

pub fn start_with_mode(
    id: String,
    name: String,
    bytes: Vec<u8>,
    layout: bool,
) -> Result<(), String> {
    if layout && !name.to_ascii_lowercase().ends_with(".pdf") {
        return Err("Layout text review requires a PDF".into());
    }
    let mut task = TASK
        .lock()
        .map_err(|_| "Extraction task state unavailable")?;
    expire_completed(&mut task);
    if task.as_ref().is_some_and(|t| !t.collected) {
        return Err("A document extraction is running or its result is awaiting collection; uncollected results expire after 60 seconds".into());
    }
    *task = Some(Task {
        id: id.clone(),
        outcome: None,
        completed: None,
        collected: false,
    });
    drop(task);
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| {
            if layout {
                crate::pdf_extract::extract_layout_with_coverage(&bytes)
                    .map(|(text, coverage)| (text, Some(coverage)))
            } else {
                crate::document_extract::extract_for_review(&name, &bytes)
            }
        })
        .unwrap_or_else(|_| Err("Document extraction stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut() {
                if task.id == id {
                    task.outcome = Some(outcome);
                    task.completed = Some(Instant::now());
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
    expire_completed(&mut tasks);
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Extraction task not found")?;
    match &task.outcome {
        None => Ok(json!({"status":"running"})),
        Some(Ok((text, coverage))) => {
            task.collected = true;
            Ok(json!({"status":"complete","text":text,"coverage":coverage}))
        }
        Some(Err(error)) => {
            task.collected = true;
            Ok(json!({"status":"failed","error":error}))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn extraction_modes_are_explicit_and_layout_requires_pdf() {
        assert!(!super::layout_mode(None).unwrap());
        assert!(!super::layout_mode(Some(&serde_json::json!("normal"))).unwrap());
        assert!(super::layout_mode(Some(&serde_json::json!("layout"))).unwrap());
        for value in [
            serde_json::json!(true),
            serde_json::json!(null),
            serde_json::json!("unknown"),
        ] {
            assert!(super::layout_mode(Some(&value)).is_err());
        }
        assert!(super::start_with_mode(
            "invalid".into(),
            "cv.docx".into(),
            b"fictional".to_vec(),
            true
        )
        .is_err());
    }
    #[test]
    #[ignore = "requires an explicitly configured local pypdf runtime"]
    fn background_pdf_extraction_reports_unreadable_pages() {
        super::start(
            "partial-fixture".into(),
            "cv.pdf".into(),
            include_bytes!("../tests/fictional-image-page.pdf").to_vec(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let result = super::poll("partial-fixture").unwrap();
            if result["status"] != "running" {
                assert_eq!(result["status"], "complete", "{result}");
                assert_eq!(
                    result["coverage"]["pages_without_selectable_text"],
                    serde_json::json!([2])
                );
                assert!(result["text"].as_str().is_some_and(|text| !text.is_empty()));
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn background_extraction_returns_text() {
        super::start(
            "fixture".into(),
            "cv.txt".into(),
            b"Fictional qualifications".to_vec(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while super::TASK
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .outcome
            .is_none()
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(super::start("second".into(), "other.txt".into(), b"Other".to_vec()).is_err());
        loop {
            let result = super::poll("fixture").unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["text"], "Fictional qualifications");
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            super::poll("fixture").unwrap()["text"],
            "Fictional qualifications"
        );
        super::start("second".into(), "other.txt".into(), b"Other".to_vec()).unwrap();
        assert!(super::poll("fixture").is_err());
        loop {
            let result = super::poll("second").unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["text"], "Other");
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn completed_result_is_retained_until_collection_or_expiry() {
        let mut task = Some(super::Task {
            id: "first".into(),
            outcome: Some(Ok(("Fictional CV".into(), None))),
            completed: Some(std::time::Instant::now()),
            collected: false,
        });
        super::expire_completed(&mut task);
        assert_eq!(task.as_ref().unwrap().id, "first");
        task.as_mut().unwrap().completed = Some(std::time::Instant::now() - super::RETENTION);
        super::expire_completed(&mut task);
        assert!(task.is_none());
    }
}
