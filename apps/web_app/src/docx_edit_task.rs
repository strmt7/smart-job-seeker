//! Save a reviewed DOCX replacement only after its PDF is generated.
use serde_json::{json, Value};
use std::sync::Mutex;
use waypoint_domain::sha256_hex;
type EditedFiles = (Option<Vec<u8>>, Vec<u8>);
struct Task {
    id: String,
    document_id: String,
    digest: String,
    name: String,
    result: Option<Result<EditedFiles, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
pub fn start_batch(
    id: String,
    document_id: String,
    name: String,
    source: Vec<u8>,
    edits: Vec<(String, String)>,
) -> Result<(), String> {
    if edits.is_empty() || edits.len() > 5 {
        return Err("Queue between one and five changes".into());
    }
    for (before, after) in &edits {
        if before.is_empty()
            || before.len() > 1000
            || after.len() > 4000
            || before.chars().chain(after.chars()).any(char::is_control)
        {
            return Err("Each change needs a single-line passage within the size limits".into());
        }
    }
    let is_pdf = name.to_ascii_lowercase().ends_with(".pdf");
    if !is_pdf && !name.to_ascii_lowercase().ends_with(".docx") {
        return Err("Choose a PDF or DOCX template".into());
    }
    let mut task = TASK.lock().map_err(|_| "Template editing unavailable")?;
    if task.is_some() {
        return Err("Finish or recover the previous template edit first".into());
    }
    *task = Some(Task {
        id: id.clone(),
        document_id,
        digest: sha256_hex(&source),
        name,
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            let mut edited = source;
            for (index, (before, after)) in edits.iter().enumerate() {
                edited = if is_pdf {
                    crate::pdf_extract::edit(&edited, before, after)
                } else {
                    crate::docx_edit::replace(&edited, before, after)
                }
                .map_err(|error| format!("Change {}: {error}", index + 1))?;
            }
            let (edited, pdf) = if is_pdf {
                (None, edited)
            } else {
                let pdf = crate::document_convert::docx_to_pdf(&edited)?;
                (Some(edited), pdf)
            };
            if pdf.len() > 10 * 1024 * 1024 {
                return Err("Exported PDF exceeds 10 MiB".into());
            }
            Ok((edited, pdf))
        })
        .unwrap_or_else(|_| Err("Template editing stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(t) = task.as_mut().filter(|t| t.id == id) {
                t.result = Some(result);
            }
        }
    });
    Ok(())
}
pub fn poll(id: &str, ws: &waypoint_workspace::Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Template editing unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|t| t.id == id)
        .ok_or("Template edit not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    match result {
        Err(error) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
        Ok((docx, pdf)) => {
            let current = match ws.document_content(&task.document_id) {
                Ok(Some(value)) => value,
                Ok(None) => {
                    *tasks = None;
                    return Ok(
                        json!({"status":"failed","error":"Original template no longer exists"}),
                    );
                }
                Err(error) => {
                    task.result = Some(Ok((docx, pdf)));
                    return Err(format!("Could not read template; retry status: {error}"));
                }
            };
            if sha256_hex(&current) != task.digest {
                *tasks = None;
                return Ok(
                    json!({"status":"failed","error":"The template changed; review it again"}),
                );
            }
            let stem = task
                .name
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .unwrap_or("document");
            let docx_name = format!("{stem}-edited.docx");
            let pdf_name = format!("{stem}-edited.pdf");
            let saved = (|| -> Result<(Option<String>, String), String> {
                let pdf_id = crate::random_id()?;
                let now = crate::now();
                if let Some(docx) = &docx {
                    let docx_id = crate::random_id()?;
                    ws.add_documents_atomic(&[
                        (&docx_id, &docx_name, docx, now),
                        (&pdf_id, &pdf_name, &pdf, now),
                    ])
                    .map_err(|e| e.to_string())?;
                    Ok((Some(docx_id), pdf_id))
                } else {
                    ws.add_document(&pdf_id, &pdf_name, &pdf, now)
                        .map_err(|e| e.to_string())?;
                    Ok((None, pdf_id))
                }
            })();
            match saved {
                Ok((docx_id, pdf_id)) => {
                    *tasks = None;
                    Ok(
                        json!({"status":"complete","docx_id":docx_id,"docx_name":docx_id.as_ref().map(|_| docx_name),"pdf_id":pdf_id,"pdf_name":pdf_name}),
                    )
                }
                Err(error) => {
                    task.result = Some(Ok((docx, pdf)));
                    Err(format!(
                        "Could not save edited files; retry status: {error}"
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "uses a fictional DOCX fixture"]
    fn failed_batch_saves_nothing() {
        let original = std::fs::read(std::env::var("WAYPOINT_EDIT_FIXTURE").unwrap()).unwrap();
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        ws.add_document("source", "fictional.docx", &original, 1)
            .unwrap();
        start_batch(
            "failed".into(),
            "source".into(),
            "fictional.docx".into(),
            original.clone(),
            vec![
                ("Fictional CV".into(), "Example CV".into()),
                ("missing text".into(), "replacement".into()),
            ],
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let result = poll("failed", &ws).unwrap();
            if result["status"] == "failed" {
                assert!(result["error"].as_str().unwrap().starts_with("Change 2:"));
                assert_eq!(ws.documents().unwrap().len(), 1);
                assert_eq!(ws.document_content("source").unwrap().unwrap(), original);
                break;
            }
            assert_eq!(result["status"], "running");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    #[test]
    #[ignore = "edits a fictional PDF using local PDFium"]
    fn saves_reviewed_pdf_edit_and_preserves_original() {
        let path = std::env::var("WAYPOINT_EDIT_PDF_FIXTURE").unwrap();
        let original = std::fs::read(path).unwrap();
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        ws.add_document("pdf-source", "fictional.pdf", &original, 1)
            .unwrap();
        assert!(crate::pdf_extract::edit(&original, "research", "testing")
            .unwrap_err()
            .contains("glyph"));
        assert!(
            crate::pdf_extract::edit(&original, "research", "research research research").is_err()
        );
        assert!(crate::pdf_extract::edit(&original, "not present", "anything").is_err());
        start_batch(
            "pdf-edit".into(),
            "pdf-source".into(),
            "fictional.pdf".into(),
            original.clone(),
            vec![
                ("research".into(), "search".into()),
                ("qualifications".into(), "qualification".into()),
            ],
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        loop {
            let result = poll("pdf-edit", &ws).unwrap();
            if result["status"] == "complete" {
                let pdf = ws
                    .document_content(result["pdf_id"].as_str().unwrap())
                    .unwrap()
                    .unwrap();
                assert!(result["docx_id"].is_null());
                assert!(crate::pdf_extract::extract(&pdf)
                    .unwrap()
                    .contains("Fictional search and qualification."));
                assert_eq!(
                    ws.document_content("pdf-source").unwrap().unwrap(),
                    original
                );
                if let Some(path) = std::env::var_os("WAYPOINT_EDIT_PDF_OUTPUT") {
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
    #[ignore = "converts a fictional DOCX with local LibreOffice"]
    fn edited_docx_and_pdf_are_saved_together() {
        let path = std::env::var("WAYPOINT_EDIT_FIXTURE").expect("fixture path");
        let original = std::fs::read(path).unwrap();
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        ws.add_document("source", "fictional.docx", &original, 1)
            .unwrap();
        start_batch(
            "fixture-edit".into(),
            "source".into(),
            "fictional.docx".into(),
            original.clone(),
            vec![
                ("Fictional CV".into(), "Example CV".into()),
                (
                    "Synthetic experience and qualifications.".into(),
                    "Fictional research and qualifications.".into(),
                ),
            ],
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
        loop {
            let result = poll("fixture-edit", &ws).unwrap();
            if result["status"] == "complete" {
                let docx = ws
                    .document_content(result["docx_id"].as_str().unwrap())
                    .unwrap()
                    .unwrap();
                let pdf = ws
                    .document_content(result["pdf_id"].as_str().unwrap())
                    .unwrap()
                    .unwrap();
                let text = crate::document_extract::extract("file.docx", &docx).unwrap();
                assert!(text.contains("Example CV"));
                assert!(text.contains("Fictional research and qualifications."));
                assert!(pdf.starts_with(b"%PDF-"));
                assert_eq!(ws.document_content("source").unwrap().unwrap(), original);
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
