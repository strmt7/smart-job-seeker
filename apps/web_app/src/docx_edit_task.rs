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
    job_id: Option<String>,
    context_digest: Option<String>,
    edits: Vec<(String, String)>,
    result: Option<Result<EditedFiles, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
fn job_context_digest(ws: &waypoint_workspace::Workspace, job: &str) -> Result<String, String> {
    let job = waypoint_domain::JobIdentityId::new(job).map_err(|e| e.to_string())?;
    let (_, _, _, fit_digest) = crate::fit_inputs(ws, &job)?;
    let revision = ws.profile_revision().map_err(|e| e.to_string())?;
    let listing = ws
        .job(&job)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let description = ws.job_description(&job).map_err(|e| e.to_string())?;
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(profile) => serde_json::from_str(&profile).map_err(|_| "Stored profile is invalid")?,
        None => json!({}),
    };
    let instructions: String = profile["document_instructions"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(2000)
        .collect();
    Ok(sha256_hex(
        json!({"job":listing, "description":description, "fit":fit_digest, "profile_revision":revision, "document_instructions":instructions})
            .to_string()
            .as_bytes(),
    ))
}
pub fn start_batch(
    ws: &waypoint_workspace::Workspace,
    id: String,
    document_id: String,
    name: String,
    source: Vec<u8>,
    job_id: Option<String>,
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
    let context_digest = job_id
        .as_deref()
        .map(|job| job_context_digest(ws, job))
        .transpose()?;
    let mut task = TASK.lock().map_err(|_| "Template editing unavailable")?;
    if task.is_some() {
        return Err("Finish or recover the previous template edit first".into());
    }
    *task = Some(Task {
        id: id.clone(),
        document_id,
        digest: sha256_hex(&source),
        name,
        job_id,
        context_digest,
        edits: edits.clone(),
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
            if let Some(job) = task.job_id.as_deref() {
                let current_context = match job_context_digest(ws, job) {
                    Ok(context) => context,
                    Err(error) => {
                        task.result = Some(Ok((docx, pdf)));
                        return Err(format!(
                            "Could not verify job context; retry status: {error}"
                        ));
                    }
                };
                if task.context_digest.as_deref() != Some(current_context.as_str()) {
                    *tasks = None;
                    return Ok(
                        json!({"status":"failed","error":"The job, qualifications or instructions changed during template editing; review again"}),
                    );
                }
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
                    let record = task.job_id.as_deref().map(|job_id| {
                        record_edit(
                            ws,
                            job_id,
                            &task.name,
                            &current,
                            docx.as_deref(),
                            &pdf,
                            &task.edits,
                        )
                    });
                    let record_saved = record.as_ref().map(|result| result.is_ok());
                    let record_error = record.and_then(Result::err);
                    *tasks = None;
                    Ok(
                        json!({"status":"complete","docx_id":docx_id,"docx_name":docx_id.as_ref().map(|_| docx_name),"pdf_id":pdf_id,"pdf_name":pdf_name,"record_saved":record_saved,"record_error":record_error}),
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

fn record_edit(
    ws: &waypoint_workspace::Workspace,
    job_id: &str,
    name: &str,
    original: &[u8],
    docx: Option<&[u8]>,
    pdf: &[u8],
    edits: &[(String, String)],
) -> Result<(), String> {
    let job = waypoint_domain::JobIdentityId::new(job_id).map_err(|e| e.to_string())?;
    let mut record =
        crate::attempt_record::AttemptRecord::start(ws, &job, &crate::random_id()?, crate::now())?
            .ok_or("Application record requires a disk workspace")?;
    let original_name = record.material(&format!("original-{name}"), original)?;
    let docx_name = docx
        .map(|bytes| record.material("edited-template.docx", bytes))
        .transpose()?;
    let pdf_name = record.material("edited-template.pdf", pdf)?;
    record.event(json!({"type":"template_edited","time":crate::now(),"original":original_name,"docx":docx_name,"pdf":pdf_name,"original_sha256":sha256_hex(original),"pdf_sha256":sha256_hex(pdf),"edits":edits,"note":"Reviewed template edit; no application was submitted."}))?;
    record.status("manual_review", crate::now())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    static TASK_TEST_LOCK: Mutex<()> = Mutex::new(());
    #[test]
    fn changed_job_context_prevents_saving_converted_edit() {
        use waypoint_domain::{ApplicationState, JobIdentityId};
        use waypoint_store::StoredJob;
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let job = JobIdentityId::new("fictional-edit-context").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: job.clone(),
                title: "Role".into(),
                employer: "Example".into(),
                location: String::new(),
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
            Some("Fictional role"),
        )
        .unwrap();
        ws.add_document("source", "fixture.pdf", b"fictional original", 1)
            .unwrap();
        let digest = job_context_digest(&ws, job.as_str()).unwrap();
        *TASK.lock().unwrap() = Some(Task {
            id: "context-test".into(),
            document_id: "source".into(),
            digest: sha256_hex(b"fictional original"),
            name: "fixture.pdf".into(),
            job_id: Some(job.as_str().into()),
            context_digest: Some(digest),
            edits: vec![("before".into(), "after".into())],
            result: Some(Ok((None, b"%PDF-fictional".to_vec()))),
        });
        ws.save_candidate_profile(
            &json!({"document_instructions":"Use a different style"}).to_string(),
        )
        .unwrap();
        let outcome = poll("context-test", &ws).unwrap();
        assert_eq!(outcome["status"], "failed");
        assert!(outcome["error"]
            .as_str()
            .unwrap()
            .contains("instructions changed"));
        assert_eq!(ws.documents().unwrap().len(), 1);
    }
    #[test]
    fn context_tracks_corrections_outside_the_selected_fit_facts() {
        use waypoint_domain::{ApplicationState, JobIdentityId};
        use waypoint_store::StoredJob;
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let job = JobIdentityId::new("fictional-correction").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: job.clone(),
                title: "Role".into(),
                employer: "Example".into(),
                location: String::new(),
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
            Some("Fictional role"),
        )
        .unwrap();
        let mut facts = Vec::new();
        for index in 0..25 {
            let text = format!("Unrelated qualification {index}");
            let id = ws
                .add_fact(&text, waypoint_workspace::FactOrigin::UserAttested)
                .unwrap();
            facts.push((id, text));
        }
        let selected = crate::fit_inputs(&ws, &job).unwrap().0;
        let (outside, _) = facts
            .iter()
            .find(|(_, text)| !selected.contains(text))
            .unwrap();
        let before_revision = ws.profile_revision().unwrap();
        let before = job_context_digest(&ws, job.as_str()).unwrap();
        ws.correct_fact(outside, "Revised unrelated qualification", 1)
            .unwrap();
        assert!(ws.profile_revision().unwrap() > before_revision);
        assert_ne!(before, job_context_digest(&ws, job.as_str()).unwrap());
    }
    #[test]
    fn job_record_preserves_original_edits_and_output() {
        use waypoint_domain::{ApplicationState, JobIdentityId};
        use waypoint_store::StoredJob;
        let root = tempfile::tempdir().unwrap();
        let mut ws = waypoint_workspace::Workspace::open(root.path()).unwrap();
        let job = JobIdentityId::new("fictional-template-job").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: job.clone(),
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
            Some("Fictional job description"),
        )
        .unwrap();
        record_edit(
            &ws,
            job.as_str(),
            "template.docx",
            b"original",
            Some(b"edited"),
            b"%PDF-example",
            &[("before".into(), "after".into())],
        )
        .unwrap();
        let records = crate::attempt_record::list(&ws, &job).unwrap();
        assert_eq!(records["attempts"].as_array().unwrap().len(), 1);
        let entry = std::fs::read_dir(root.path().join("application-records"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let event: Value =
            serde_json::from_slice(&std::fs::read(entry.join("events.jsonl")).unwrap()).unwrap();
        assert_eq!(event["type"], "template_edited");
        assert_eq!(event["edits"][0], json!(["before", "after"]));
        for (key, bytes) in [
            ("original", b"original".as_slice()),
            ("docx", b"edited".as_slice()),
            ("pdf", b"%PDF-example".as_slice()),
        ] {
            assert_eq!(
                std::fs::read(entry.join("materials").join(event[key].as_str().unwrap())).unwrap(),
                bytes
            );
        }
        let attempt: Value =
            serde_json::from_slice(&std::fs::read(entry.join("attempt.json")).unwrap()).unwrap();
        assert_eq!(attempt["status"], "manual_review");
        assert_eq!(attempt["submitted_by_waypoint"], false);
    }
    #[test]
    #[ignore = "edits a fictional DOCX using local LibreOffice"]
    fn job_linked_edit_saves_record_after_conversion() {
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        use waypoint_domain::{ApplicationState, JobIdentityId};
        use waypoint_store::StoredJob;
        let root = tempfile::tempdir().unwrap();
        let mut ws = waypoint_workspace::Workspace::open(root.path()).unwrap();
        let job = JobIdentityId::new("fictional-linked-edit").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: job.clone(),
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
            Some("Fictional listing"),
        )
        .unwrap();
        let source = std::fs::read(std::env::var("WAYPOINT_HEADER_FIXTURE").unwrap()).unwrap();
        ws.add_document("source", "fixture.docx", &source, 1)
            .unwrap();
        start_batch(
            &ws,
            "linked-edit".into(),
            "source".into(),
            "fixture.docx".into(),
            source,
            Some(job.as_str().to_owned()),
            vec![(
                "Synthetic Header Name".into(),
                "Fictional Header Name".into(),
            )],
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
        loop {
            let result = poll("linked-edit", &ws).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["record_saved"], true);
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(
            crate::attempt_record::list(&ws, &job).unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    #[ignore = "uses a fictional DOCX fixture"]
    fn failed_batch_saves_nothing() {
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        let original = std::fs::read(std::env::var("WAYPOINT_EDIT_FIXTURE").unwrap()).unwrap();
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        ws.add_document("source", "fictional.docx", &original, 1)
            .unwrap();
        start_batch(
            &ws,
            "failed".into(),
            "source".into(),
            "fictional.docx".into(),
            original.clone(),
            None,
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
        let _guard = TASK_TEST_LOCK.lock().unwrap();
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
            &ws,
            "pdf-edit".into(),
            "pdf-source".into(),
            "fictional.pdf".into(),
            original.clone(),
            None,
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
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        let path = std::env::var("WAYPOINT_EDIT_FIXTURE").expect("fixture path");
        let original = std::fs::read(path).unwrap();
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        ws.add_document("source", "fictional.docx", &original, 1)
            .unwrap();
        start_batch(
            &ws,
            "fixture-edit".into(),
            "source".into(),
            "fictional.docx".into(),
            original.clone(),
            None,
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
