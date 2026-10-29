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
    digest: String,
    reviewed_text: String,
    result: Option<Result<Vec<u8>, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);
pub fn pending() -> Result<Option<(String, String)>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Export state unavailable")?
        .as_ref()
        .map(|task| (task.id.clone(), task.job_id.clone())))
}
pub fn start(
    id: String,
    job_id: String,
    kind: String,
    text: String,
    digest: String,
) -> Result<(), String> {
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
    if task.is_some() {
        return Err("An export is running or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        job_id,
        kind,
        digest,
        reviewed_text: text.clone(),
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
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Export state unavailable")?
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone())
            .ok_or("Letter export not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    let job_id = JobIdentityId::new(&task.job_id).map_err(|e| e.to_string())?;
    match result {
        Err(error) => {
            let output = json!({"status":"failed","error":error});
            *COMPLETED.lock().map_err(|_| "Export state unavailable")? =
                Some((id.to_owned(), output.clone()));
            *tasks = None;
            Ok(output)
        }
        Ok(pdf) => {
            if crate::draft_context_digest(ws, &job_id, &task.kind)
                .ok()
                .as_deref()
                != Some(task.digest.as_str())
            {
                let output = json!({"status":"failed","error":"The job, qualifications, profile or template changed during export; draft and review again"});
                *COMPLETED.lock().map_err(|_| "Export state unavailable")? =
                    Some((id.to_owned(), output.clone()));
                *tasks = None;
                return Ok(output);
            }
            if pdf.len() > 10 * 1024 * 1024 {
                let output = json!({"status":"failed","error":"Exported PDF exceeds 10 MiB"});
                *COMPLETED.lock().map_err(|_| "Export state unavailable")? =
                    Some((id.to_owned(), output.clone()));
                *tasks = None;
                return Ok(output);
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
                    let record = record_generated(
                        ws,
                        &job_id,
                        &task.kind,
                        &task.reviewed_text,
                        &pdf,
                        &task.digest,
                    );
                    let record_saved = record.is_ok();
                    let record_error = record.err();
                    let job = task.job_id.clone();
                    let kind = task.kind.clone();
                    let output = json!({"status":"complete","id":doc_id,"name":name,"job_id":job,"kind":kind,"record_saved":record_saved,"record_error":record_error});
                    *COMPLETED.lock().map_err(|_| "Export state unavailable")? =
                        Some((id.to_owned(), output.clone()));
                    *tasks = None;
                    Ok(output)
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
fn record_generated(
    ws: &Workspace,
    job_id: &JobIdentityId,
    kind: &str,
    text: &str,
    pdf: &[u8],
    context_digest: &str,
) -> Result<(), String> {
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(profile) => serde_json::from_str(&profile).map_err(|_| "Stored profile is invalid")?,
        None => json!({}),
    };
    let key = match kind {
        "cv" => "cv_template_id",
        "cover_letter" => "cover_template_id",
        _ => return Err("Unknown document type".into()),
    };
    let template = if let Some(id) = profile[key].as_str().filter(|id| !id.is_empty()) {
        let info = ws
            .documents()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|doc| doc.id == id)
            .ok_or("Selected template is unavailable")?;
        let bytes = ws
            .document_content(id)
            .map_err(|e| e.to_string())?
            .ok_or("Selected template is unavailable")?;
        Some((info.name, bytes))
    } else {
        None
    };
    let mut record = crate::attempt_record::AttemptRecord::start(
        ws,
        job_id,
        &crate::random_id()?,
        crate::now(),
    )?
    .ok_or("Application record requires a disk workspace")?;
    let template_name = template
        .as_ref()
        .map(|(name, bytes)| record.material(&format!("original-{name}"), bytes))
        .transpose()?;
    let text_name = record.material(&format!("reviewed-{kind}.txt"), text.as_bytes())?;
    let pdf_name = record.material(&format!("reviewed-{kind}.pdf"), pdf)?;
    record.event(json!({"type":"draft_exported","time":crate::now(),"kind":kind,"template":template_name,"template_sha256":template.as_ref().map(|(_, bytes)| waypoint_domain::sha256_hex(bytes)),"reviewed_text":text_name,"pdf":pdf_name,"pdf_sha256":waypoint_domain::sha256_hex(pdf),"context_sha256":context_digest,"note":"Reviewed text exported in a simple layout. Template formatting is not preserved; no application was submitted."}))?;
    record.status("manual_review", crate::now())?;
    Ok(())
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
    static TASK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn completed_export_cannot_be_replaced_before_collection() {
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        *super::TASK.lock().unwrap() = Some(super::Task {
            id: "first".into(),
            job_id: "fictional".into(),
            kind: "cv".into(),
            digest: "fixture".into(),
            reviewed_text: "Fictional text".into(),
            result: Some(Err("Fictional conversion error".into())),
        });
        let error = super::start(
            "second".into(),
            "fictional".into(),
            "cv".into(),
            "Fictional text".into(),
            "fixture".into(),
        )
        .unwrap_err();
        assert!(error.contains("awaiting its result"));
        assert_eq!(
            super::pending().unwrap(),
            Some(("first".into(), "fictional".into()))
        );
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        assert_eq!(
            super::poll("first", &ws).unwrap()["error"],
            "Fictional conversion error"
        );
        assert_eq!(
            super::poll("first", &ws).unwrap()["error"],
            "Fictional conversion error"
        );
        let mut ws = ws;
        let job_id = waypoint_domain::JobIdentityId::new("fictional").unwrap();
        ws.import_manual_job(
            waypoint_store::StoredJob {
                id: job_id.clone(),
                title: "Role".into(),
                employer: "Example".into(),
                location: String::new(),
                state: waypoint_domain::ApplicationState::Shortlisted,
                canonical_url: Some("https://example.org/job".into()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional role"),
        )
        .unwrap();
        let digest = crate::draft_context_digest(&ws, &job_id, "cv").unwrap();
        *super::TASK.lock().unwrap() = Some(super::Task {
            id: "stale".into(),
            job_id: "fictional".into(),
            kind: "cv".into(),
            digest,
            reviewed_text: "Fictional text".into(),
            result: Some(Ok(b"%PDF-fictional".to_vec())),
        });
        ws.add_fact(
            "New qualification",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        let result = super::poll("stale", &ws).unwrap();
        assert_eq!(result["status"], "failed");
        assert!(ws.documents().unwrap().is_empty());
    }
    #[test]
    fn successful_export_replays_after_lost_response_without_another_document() {
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        use waypoint_domain::{ApplicationState, JobIdentityId};
        use waypoint_store::StoredJob;
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let id = JobIdentityId::new("fictional-replay").unwrap();
        ws.import_job(StoredJob {
            id: id.clone(),
            title: "Role".into(),
            employer: "Example".into(),
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
        let digest = crate::draft_context_digest(&ws, &id, "cv").unwrap();
        *super::TASK.lock().unwrap() = Some(super::Task {
            id: "replay".into(),
            job_id: id.as_str().into(),
            kind: "cv".into(),
            digest,
            reviewed_text: "Fictional text".into(),
            result: Some(Ok(b"%PDF-fictional".to_vec())),
        });
        let first = super::poll("replay", &ws).unwrap();
        assert_eq!(first["status"], "complete");
        assert_eq!(super::poll("replay", &ws).unwrap(), first);
        assert_eq!(ws.documents().unwrap().len(), 1);
        assert!(super::poll("other", &ws).is_err());
    }
    #[test]
    fn generated_record_includes_template_reviewed_text_and_pdf() {
        use waypoint_domain::{ApplicationState, JobIdentityId};
        use waypoint_store::StoredJob;
        let root = tempfile::tempdir().unwrap();
        let mut ws = waypoint_workspace::Workspace::open(root.path()).unwrap();
        let id = JobIdentityId::new("fictional-export-record").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: id.clone(),
                title: "Role".into(),
                employer: "Example".into(),
                location: "".into(),
                state: ApplicationState::Shortlisted,
                canonical_url: None,
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "fixture".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional posting"),
        )
        .unwrap();
        ws.add_document("template", "template.docx", b"fictional-template", 1)
            .unwrap();
        ws.save_candidate_profile(&serde_json::json!({"cv_template_id":"template"}).to_string())
            .unwrap();
        super::record_generated(
            &ws,
            &id,
            "cv",
            "Reviewed fictional text",
            b"%PDF-fictional",
            "context",
        )
        .unwrap();
        let dir = std::fs::read_dir(root.path().join("application-records"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let event: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("events.jsonl")).unwrap()).unwrap();
        assert_eq!(event["type"], "draft_exported");
        for (key, expected) in [
            ("template", b"fictional-template".as_slice()),
            ("reviewed_text", b"Reviewed fictional text".as_slice()),
            ("pdf", b"%PDF-fictional".as_slice()),
        ] {
            assert_eq!(
                std::fs::read(dir.join("materials").join(event[key].as_str().unwrap())).unwrap(),
                expected
            );
        }
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("attempt.json")).unwrap()).unwrap();
        assert_eq!(metadata["status"], "manual_review");
        assert_eq!(metadata["submitted_by_waypoint"], false);
    }
    #[test]
    fn document_preserves_unicode_and_xml_characters() {
        let doc = super::build_docx("Résumé & systems\nSecond paragraph").unwrap();
        let text = crate::document_extract::extract("letter.docx", &doc).unwrap();
        assert_eq!(text, "Résumé & systems\nSecond paragraph");
    }
    #[test]
    #[ignore = "runs a local LibreOffice conversion of fictional text"]
    fn exports_fixture() {
        let _guard = TASK_TEST_LOCK.lock().unwrap();
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
        let digest = crate::draft_context_digest(&ws, &id, "cv").unwrap();
        super::start("test-task".into(),id.as_str().into(),"cv".into(),"Fictional Candidate\n\nExperience\nBuilt reliable systems and worked with colleagues to improve software.\n\nSkills\nRust and SQL".into(), digest).unwrap();
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
        assert_eq!(result["job_id"], "fixture-letter");
        assert_eq!(result["kind"], "cv");
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
