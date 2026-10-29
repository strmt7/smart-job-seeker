//! Convert one document in the background, then save the result from the workspace thread.
use serde_json::{json, Value};
use std::sync::Mutex;
use waypoint_workspace::Workspace;

struct Task {
    id: String,
    name: String,
    result: Option<Result<Vec<u8>, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
// A lost response can be retried without saving the same PDF a second time.
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

pub enum ConversionKind {
    Docx,
    Image,
    PlainText,
}

/// Recover an active or completed-but-uncollected conversion after a lost
/// start response. Only one task exists, and its PDF bytes stay in the worker.
pub fn pending() -> Result<Value, String> {
    let tasks = TASK.lock().map_err(|_| "Conversion state unavailable")?;
    Ok(match tasks.as_ref() {
        Some(task) => {
            json!({"active":true,"task_id":task.id,"name":task.name,"ready":task.result.is_some()})
        }
        None => json!({"active":false}),
    })
}

pub fn start(id: String, name: String, bytes: Vec<u8>, kind: ConversionKind) -> Result<(), String> {
    let mut tasks = TASK.lock().map_err(|_| "Conversion state unavailable")?;
    if tasks.is_some() {
        return Err("A conversion is running or awaiting its result".into());
    }
    *tasks = Some(Task {
        id: id.clone(),
        name,
        result: None,
    });
    drop(tasks);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| match kind {
            ConversionKind::Image => crate::pdf_extract::image_to_pdf(&bytes),
            ConversionKind::Docx => crate::document_convert::docx_to_pdf(&bytes),
            ConversionKind::PlainText => crate::document_convert::text_to_pdf(&bytes),
        })
        .unwrap_or_else(|_| Err("Conversion stopped unexpectedly".into()));
        if let Ok(mut tasks) = TASK.lock() {
            if let Some(task) = tasks.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str, ws: &Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Conversion state unavailable")?;
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        let completed = COMPLETED
            .lock()
            .map_err(|_| "Conversion state unavailable")?;
        return completed
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone())
            .ok_or_else(|| "Conversion not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    match result {
        Err(error) => {
            let result = json!({"status":"failed","error":error});
            *COMPLETED
                .lock()
                .map_err(|_| "Conversion state unavailable")? = Some((id.into(), result.clone()));
            *tasks = None;
            Ok(result)
        }
        Ok(pdf) => {
            if pdf.len() > crate::MAX_DOCUMENT {
                let result = json!({"status":"failed","error":"Converted PDF exceeds 10 MiB"});
                *COMPLETED
                    .lock()
                    .map_err(|_| "Conversion state unavailable")? =
                    Some((id.into(), result.clone()));
                *tasks = None;
                return Ok(result);
            }
            let new_id = match crate::random_id() {
                Ok(id) => id,
                Err(error) => {
                    task.result = Some(Ok(pdf));
                    return Err(error);
                }
            };
            match ws.add_document(&new_id, &task.name, &pdf, crate::now()) {
                Ok(()) => {
                    let name = task.name.clone();
                    let result = json!({"status":"complete","id":new_id,"name":name});
                    *COMPLETED
                        .lock()
                        .map_err(|_| "Conversion state unavailable")? =
                        Some((id.into(), result.clone()));
                    *tasks = None;
                    Ok(result)
                }
                Err(_) => {
                    task.result = Some(Ok(pdf));
                    Err("PDF created but could not be saved; retry status".into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_conversion_can_be_retried_without_duplicate_documents() {
        let _guard = TEST_LOCK.lock().unwrap();
        let ws = Workspace::in_memory().unwrap();
        *TASK.lock().unwrap() = Some(Task {
            id: "fictional-pdf-task".into(),
            name: "fictional.pdf".into(),
            result: Some(Ok(b"%PDF-fixture".to_vec())),
        });
        let first = poll("fictional-pdf-task", &ws).unwrap();
        let retry = poll("fictional-pdf-task", &ws).unwrap();
        assert_eq!(first, retry);
        assert_eq!(ws.documents().unwrap().len(), 1);
    }

    #[test]
    fn pending_conversion_can_be_recovered_before_and_after_worker_finishes() {
        let _guard = TEST_LOCK.lock().unwrap();
        let ws = Workspace::in_memory().unwrap();
        *TASK.lock().unwrap() = Some(Task {
            id: "lost-start".into(),
            name: "fictional.pdf".into(),
            result: None,
        });
        assert_eq!(
            pending().unwrap(),
            json!({"active":true,"task_id":"lost-start","name":"fictional.pdf","ready":false})
        );
        TASK.lock().unwrap().as_mut().unwrap().result = Some(Ok(b"%PDF-fixture".to_vec()));
        let ready = pending().unwrap();
        assert_eq!(ready["ready"], true);
        assert!(ready.get("bytes").is_none());
        let result = poll(ready["task_id"].as_str().unwrap(), &ws).unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(pending().unwrap(), json!({"active":false}));
        assert_eq!(poll("lost-start", &ws).unwrap(), result);
        assert_eq!(ws.documents().unwrap().len(), 1);
    }

    #[test]
    #[ignore = "uses installed LibreOffice to convert fictional UTF-8 text"]
    fn background_text_conversion_saves_one_pdf() {
        let _guard = TEST_LOCK.lock().unwrap();
        let ws = Workspace::in_memory().unwrap();
        let source = "Fictional CV\nAvery Example\nMünchen · São Paulo\n"
            .as_bytes()
            .to_vec();
        start(
            "text-task".into(),
            "fictional.pdf".into(),
            source,
            ConversionKind::PlainText,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        loop {
            let result = poll("text-task", &ws).unwrap();
            if result["status"] == "complete" {
                let pdf = ws
                    .document_content(result["id"].as_str().unwrap())
                    .unwrap()
                    .unwrap();
                assert!(pdf.starts_with(b"%PDF-"));
                assert_eq!(poll("text-task", &ws).unwrap(), result);
                assert_eq!(ws.documents().unwrap().len(), 1);
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    #[test]
    fn oversized_conversion_reports_a_terminal_result_on_retry() {
        let _guard = TEST_LOCK.lock().unwrap();
        let ws = Workspace::in_memory().unwrap();
        *TASK.lock().unwrap() = Some(Task {
            id: "oversized-fixture".into(),
            name: "fictional.pdf".into(),
            result: Some(Ok(vec![0; crate::MAX_DOCUMENT + 1])),
        });
        let first = poll("oversized-fixture", &ws).unwrap();
        assert_eq!(first["status"], "failed");
        assert_eq!(first, poll("oversized-fixture", &ws).unwrap());
        assert!(ws.documents().unwrap().is_empty());
    }
    #[test]
    #[ignore = "converts a fictional certificate image with local Pillow"]
    fn background_image_conversion_saves_a_reviewable_pdf() {
        let _guard = TEST_LOCK.lock().unwrap();
        let fixture = std::env::var("WAYPOINT_IMAGE_FIXTURE").unwrap();
        let bytes = std::fs::read(fixture).unwrap();
        assert!(crate::pdf_extract::image_to_pdf(b"invalid image").is_err());
        let ws = Workspace::in_memory().unwrap();
        start(
            "image-task".into(),
            "certificate.pdf".into(),
            bytes,
            ConversionKind::Image,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        loop {
            let result = poll("image-task", &ws).unwrap();
            if result["status"] == "complete" {
                let pdf = ws
                    .document_content(result["id"].as_str().unwrap())
                    .unwrap()
                    .unwrap();
                assert!(pdf.starts_with(b"%PDF-"));
                let rendered = crate::pdf_extract::render_page(&pdf, 0).unwrap();
                assert_eq!(rendered["pages"], 1);
                if let Some(path) = std::env::var_os("WAYPOINT_IMAGE_PDF_OUTPUT") {
                    std::fs::write(path, pdf).unwrap();
                }
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    #[test]
    #[ignore = "uses an installed LibreOffice and a fictional DOCX fixture"]
    fn background_conversion_saves_a_pdf() {
        let _guard = TEST_LOCK.lock().unwrap();
        let fixture = std::env::var("WAYPOINT_CONVERSION_FIXTURE").unwrap();
        let ws = Workspace::in_memory().unwrap();
        start(
            "fixture-task".into(),
            "fictional.pdf".into(),
            std::fs::read(fixture).unwrap(),
            ConversionKind::Docx,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        loop {
            let result = poll("fixture-task", &ws).unwrap();
            if result["status"] == "complete" {
                let id = result["id"].as_str().unwrap();
                let pdf = ws.document_content(id).unwrap().unwrap();
                assert!(pdf.starts_with(b"%PDF-"));
                if let Some(path) = std::env::var_os("WAYPOINT_CONVERSION_PDF_OUTPUT") {
                    std::fs::write(path, &pdf).unwrap();
                }
                if let Ok(expected) = std::env::var("WAYPOINT_EXPECTED_PDF_TEXT") {
                    let extracted = crate::pdf_extract::extract(&pdf).unwrap();
                    assert!(
                        extracted.contains(&expected),
                        "Expected fixture text missing from PDF"
                    );
                }
                if let Some(path) = std::env::var_os("WAYPOINT_CONVERSION_PREVIEW_OUTPUT") {
                    let preview = crate::pdf_extract::render_page(&pdf, 0).unwrap();
                    assert_eq!(preview["page"], 0);
                    assert!(preview["pages"].as_u64().unwrap() >= 1);
                    std::fs::write(path, serde_json::to_vec(&preview).unwrap()).unwrap();
                }
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
