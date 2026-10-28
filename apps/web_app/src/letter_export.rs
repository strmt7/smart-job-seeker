//! Explicit user-reviewed cover letter export. The original template is not modified.
use serde_json::{json, Value};
use std::{
    io::{Cursor, Write},
    sync::Mutex,
};
use waypoint_domain::JobIdentityId;
use waypoint_workspace::Workspace;
struct Task {
    id: String,
    job_id: String,
    kind: String,
    result: Option<Result<Vec<u8>, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
pub fn start(id: String, job_id: String, kind: String, text: String) -> Result<(), String> {
    if !matches!(kind.as_str(), "cv" | "cover_letter") {
        return Err("Unknown document type".into());
    }
    if text.trim().is_empty() || text.len() > 16000 {
        return Err("Letter must contain up to 16000 bytes of text".into());
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("Letter contains unsupported control characters".into());
    }
    let mut task = TASK.lock().map_err(|_| "Export state unavailable")?;
    if task.as_ref().is_some_and(|task| task.result.is_none()) {
        return Err("A letter export is already running".into());
    }
    *task = Some(Task {
        id: id.clone(),
        job_id,
        kind,
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            let docx = build_docx(&text)?;
            crate::document_convert::docx_to_pdf(&docx)
        })
        .unwrap_or_else(|_| Err("Letter export stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut() {
                if task.id == id {
                    task.result = Some(result)
                }
            }
        }
    });
    Ok(())
}
pub fn poll(id: &str, ws: &Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Export state unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Letter export not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    let job_id = JobIdentityId::new(&task.job_id).map_err(|e| e.to_string())?;
    match result {
        Err(error) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
        Ok(pdf) => {
            if pdf.len() > 10 * 1024 * 1024 {
                *tasks = None;
                return Err("Exported PDF exceeds 10 MiB".into());
            }
            let saved = (|| -> Result<(String, String), String> {
                let job = ws
                    .job(&job_id)
                    .map_err(|e| e.to_string())?
                    .ok_or("Job no longer exists")?;
                let safe = |value: &str| -> String {
                    let value: String = value
                        .chars()
                        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
                        .take(40)
                        .collect();
                    if value.trim().is_empty() {
                        "position".into()
                    } else {
                        value.trim().to_owned()
                    }
                };
                let prefix = if task.kind == "cv" {
                    "cv"
                } else {
                    "cover-letter"
                };
                let name = format!("{prefix}-{}-{}.pdf", safe(&job.employer), safe(&job.title));
                let doc_id = crate::random_id()?;
                ws.add_document(&doc_id, &name, &pdf, crate::now())
                    .map_err(|e| e.to_string())?;
                Ok((doc_id, name))
            })();
            match saved {
                Ok((doc_id, name)) => {
                    *tasks = None;
                    Ok(json!({"status":"complete","id":doc_id,"name":name}))
                }
                Err(error) => {
                    task.result = Some(Ok(pdf));
                    Err(format!(
                        "PDF was created but could not be saved; retry the export status: {error}"
                    ))
                }
            }
        }
    }
}
fn build_docx(text: &str) -> Result<Vec<u8>, String> {
    let mut body = String::new();
    for line in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let escaped = quick_xml::escape::escape(line);
        body.push_str(&format!(
            "<w:p><w:r><w:t xml:space=\"preserve\">{escaped}</w:t></w:r></w:p>"
        ));
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"#
    );
    let types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in [
        ("[Content_Types].xml", types),
        ("_rels/.rels", rels),
        ("word/document.xml", document.as_str()),
    ] {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}
#[cfg(test)]
mod tests {
    #[test]
    fn document_preserves_unicode_and_xml_characters() {
        let doc = super::build_docx("Résumé & systems\nSecond paragraph").unwrap();
        let text = crate::document_extract::extract("letter.docx", &doc).unwrap();
        assert_eq!(text, "Résumé & systems\nSecond paragraph");
    }
    #[test]
    #[ignore = "runs a local LibreOffice conversion of fictional text"]
    fn exports_fixture() {
        use waypoint_domain::{ApplicationState, JobIdentityId};
        use waypoint_store::StoredJob;
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let id = JobIdentityId::new("fixture-letter").unwrap();
        ws.import_job(StoredJob {
            id: id.clone(),
            title: "Engineer".into(),
            employer: "Fixture".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: None,
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        super::start("test-task".into(),id.as_str().into(),"cv".into(),"Fictional Candidate\n\nExperience\nBuilt reliable systems and worked with colleagues to improve software.\n\nSkills\nRust and SQL".into()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
        let result = loop {
            let result = super::poll("test-task", &ws).unwrap();
            if result["status"] != "running" {
                break result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(result["status"], "complete");
        assert!(result["name"].as_str().unwrap().starts_with("cv-"));
        let pdf = ws
            .document_content(result["id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let path = std::env::var("WAYPOINT_LETTER_FIXTURE_OUTPUT").unwrap();
        std::fs::write(path, pdf).unwrap();
    }
}
