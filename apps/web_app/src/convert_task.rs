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

pub fn start(id: String, name: String, bytes: Vec<u8>, image: bool) -> Result<(), String> {
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
        let result = std::panic::catch_unwind(|| {
            if image {
                crate::pdf_extract::image_to_pdf(&bytes)
            } else {
                crate::document_convert::docx_to_pdf(&bytes)
            }
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
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Conversion not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    match result {
        Err(error) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
        Ok(pdf) => {
            if pdf.len() > crate::MAX_DOCUMENT {
                *tasks = None;
                return Err("Converted PDF exceeds 10 MiB".into());
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
                    *tasks = None;
                    Ok(json!({"status":"complete","id":new_id,"name":name}))
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
    #[ignore = "converts a fictional certificate image with local Pillow"]
    fn background_image_conversion_saves_a_reviewable_pdf() {
        let fixture = std::env::var("WAYPOINT_IMAGE_FIXTURE").unwrap();
        let bytes = std::fs::read(fixture).unwrap();
        assert!(crate::pdf_extract::image_to_pdf(b"invalid image").is_err());
        let ws = Workspace::in_memory().unwrap();
        start("image-task".into(), "certificate.pdf".into(), bytes, true).unwrap();
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
        let fixture = std::env::var("WAYPOINT_CONVERSION_FIXTURE").unwrap();
        let ws = Workspace::in_memory().unwrap();
        start(
            "fixture-task".into(),
            "fictional.pdf".into(),
            std::fs::read(fixture).unwrap(),
            false,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        loop {
            let result = poll("fixture-task", &ws).unwrap();
            if result["status"] == "complete" {
                let id = result["id"].as_str().unwrap();
                assert!(ws
                    .document_content(id)
                    .unwrap()
                    .unwrap()
                    .starts_with(b"%PDF-"));
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
