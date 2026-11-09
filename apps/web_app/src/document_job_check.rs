//! Read-only snapshot of saved evidence for one exact document and job.
use serde_json::{json, Value};
use waypoint_domain::JobIdentityId;
use waypoint_workspace::Workspace;

fn visual_summary(candidates: &Value) -> Result<Value, String> {
    let reviews = candidates["reviews"]
        .as_array()
        .ok_or("Visual review inventory is invalid")?;
    let truncated = candidates["truncated"]
        .as_bool()
        .ok_or("Visual review coverage is unavailable")?;
    let unreadable = candidates["unreadable"]
        .as_u64()
        .ok_or("Visual review coverage is unavailable")?;
    let current_completed = reviews
        .iter()
        .filter(|r| {
            r["context_current"] == true
                && r["status"] == "complete"
                && r["report"]["complete"] == true
        })
        .count();
    let references: Vec<_> = reviews.iter().filter_map(|review| {
        let id = review["attempt_id"].as_str().filter(|id| id.len()==32 && id.bytes().all(|b| b.is_ascii_hexdigit()))?;
        Some(json!({"attempt_id":id,"context_current":review["context_current"],"status":review["status"]}))
    }).take(12).collect();
    Ok(
        json!({"status":"available","recorded":reviews.len(),"current_completed":current_completed,"other":reviews.len()-current_completed,"incomplete":truncated || unreadable>0,"references_omitted":reviews.len().saturating_sub(references.len()),"references":references}),
    )
}

/// Recheck the saved PDF and job before exposing one immutable review record.
pub fn read_review(
    ws: &Workspace,
    job: &JobIdentityId,
    document: &str,
    attempt: &str,
) -> Result<Value, String> {
    ws.job(job)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let metadata = ws
        .documents()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|doc| doc.id == document)
        .ok_or("Document not found")?;
    let bytes = ws
        .document_content(document)
        .map_err(|e| e.to_string())?
        .ok_or("Document not found")?;
    if !metadata.name.to_ascii_lowercase().ends_with(".pdf") || !bytes.starts_with(b"%PDF-") {
        return Err("Choose a saved PDF".into());
    }
    let review = crate::attempt_record::document_visual_review(ws, job, attempt)?;
    let digest = waypoint_domain::sha256_hex(&bytes);
    if review["document_sha256"].as_str() != Some(digest.as_str()) {
        return Err(
            "Saved review does not match these document bytes; check the document again".into(),
        );
    }
    let current = crate::document_visual_context(ws, job)
        .ok()
        .map(|context| review["context_sha256"].as_str() == Some(context.digest.as_str()));
    Ok(
        json!({"document_id":document,"document_sha256":digest,"job_id":job.as_str(),"attempt_id":attempt,"context_current":current,"status":review["status"],"error":review["error"],"report":review["report"],"requires_user_review":true}),
    )
}

pub fn inspect(ws: &Workspace, job: &JobIdentityId, document: &str) -> Result<Value, String> {
    let posting = ws
        .job(job)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let metadata = ws
        .documents()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|doc| doc.id == document)
        .ok_or("Document not found")?;
    let bytes = ws
        .document_content(document)
        .map_err(|e| e.to_string())?
        .ok_or("Document not found")?;
    if !metadata.name.to_ascii_lowercase().ends_with(".pdf") || !bytes.starts_with(b"%PDF-") {
        return Err("Choose a saved PDF; conversion and PDF inspection are separate steps".into());
    }
    let digest = waypoint_domain::sha256_hex(&bytes);
    let provenance = match crate::attempt_record::matching_generated_evidence(
        ws, job, document, &bytes,
    ) {
        Ok(lookup) => match lookup.found {
            Some(evidence) => {
                let current = crate::draft_context_digest(ws, job, &evidence.kind)
                    .ok()
                    .map(|context| context == evidence.context_sha256);
                json!({"status":"found","kind":evidence.kind,"source_attempt":evidence.attempt_id,"context_current":current,"incomplete":lookup.incomplete})
            }
            None => json!({"status":"not_found","incomplete":lookup.incomplete}),
        },
        Err(_) => json!({"status":"unavailable","incomplete":true}),
    };
    let visual = crate::document_visual_context(ws, job)
        .and_then(|context| {
            crate::attempt_record::matching_document_reviews(ws, job, &digest, &context.digest)
        })
        .and_then(|candidates| visual_summary(&candidates))
        .unwrap_or_else(|_| json!({"status":"unavailable","incomplete":true}));
    Ok(
        json!({"document_id":document,"document_sha256":digest,"job_id":job.as_str(),"job_title":posting.title,"checked_at":crate::now(),"provenance":provenance,"visual":visual,"requires_user_review":true,"note":"Snapshot of bounded saved records for these document bytes and job context. Completed visual reviews can contain findings. Missing generation evidence is normal for imported PDFs. This check does not establish factual correctness, ATS acceptance, attachment or authorization to submit."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_inventory_and_stale_reviews_remain_explicit() {
        let summary = visual_summary(&json!({"reviews":[
            {"context_current":true,"status":"complete","report":{"complete":true}},
            {"context_current":false,"status":"complete","report":{"complete":true}},
            {"context_current":true,"status":"failed","report":{"complete":false}}
        ],"truncated":true,"unreadable":0}))
        .unwrap();
        assert_eq!(summary["current_completed"], 1);
        assert_eq!(summary["other"], 2);
        assert_eq!(summary["incomplete"], true);
        assert!(visual_summary(&json!({"reviews":[]})).is_err());
        let reviews: Vec<_> = (0..13).map(|n|json!({"attempt_id":format!("{n:032x}"),"context_current":false,"status":"complete","report":{"complete":true}})).collect();
        let bounded =
            visual_summary(&json!({"reviews":reviews,"truncated":false,"unreadable":0})).unwrap();
        assert_eq!(bounded["references"].as_array().unwrap().len(), 12);
        assert_eq!(bounded["references_omitted"], 1);
    }

    #[test]
    fn document_check_binds_existing_job_and_pdf_and_never_authorizes() {
        use waypoint_domain::ApplicationState;
        use waypoint_store::StoredJob;
        let mut ws = Workspace::in_memory().unwrap();
        let job = JobIdentityId::new("fictional-material-check").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: job.clone(),
                title: "Fictional role".into(),
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
            Some("Fictional description"),
        )
        .unwrap();
        ws.add_document("pdf", "resume.pdf", b"%PDF-fictional", 1)
            .unwrap();
        let result = inspect(&ws, &job, "pdf").unwrap();
        assert_eq!(
            result["document_sha256"],
            waypoint_domain::sha256_hex(b"%PDF-fictional")
        );
        assert_eq!(result["requires_user_review"], true);
        assert_eq!(result["provenance"]["status"], "unavailable");
        assert_eq!(result["provenance"]["incomplete"], true);
        assert!(inspect(&ws, &job, "missing").is_err());
        assert!(inspect(&ws, &JobIdentityId::new("other").unwrap(), "pdf").is_err());
        ws.add_document("fake", "not-a-pdf.pdf", b"Fictional plain text", 1)
            .unwrap();
        assert!(inspect(&ws, &job, "fake").is_err());
        let root = tempfile::tempdir().unwrap();
        let mut disk = Workspace::open(root.path()).unwrap();
        disk.import_manual_job(
            ws.job(&job).unwrap().unwrap(),
            Some("Fictional description"),
        )
        .unwrap();
        disk.add_document("pdf", "resume.pdf", b"%PDF-fictional", 1)
            .unwrap();
        let disk_result = inspect(&disk, &job, "pdf").unwrap();
        assert_eq!(disk_result["provenance"]["status"], "not_found");
        assert_eq!(disk_result["provenance"]["incomplete"], false);
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = json!({"job_id":job.as_str(),"document_id":"pdf"}).to_string();
        write!(client, "POST /api/documents/job-check HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut disk, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        let payload: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["document_id"], "pdf");
        assert_eq!(payload["job_id"], job.as_str());
        assert_eq!(payload["requires_user_review"], true);
        let context = crate::draft_context_digest(&disk, &job, "cv").unwrap();
        crate::letter_export::record_generated_fixture(
            &disk,
            &job,
            "pdf",
            b"%PDF-fictional",
            &context,
        )
        .unwrap();
        let current = inspect(&disk, &job, "pdf").unwrap();
        assert_eq!(current["provenance"]["status"], "found");
        assert_eq!(current["provenance"]["context_current"], true);
        disk.add_fact(
            "New fictional qualification",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        let changed = inspect(&disk, &job, "pdf").unwrap();
        assert_eq!(changed["provenance"]["context_current"], false);
        let visual_context = crate::document_visual_context(&disk, &job).unwrap();
        let attempt = "0123456789abcdef0123456789abcdef";
        let mut record = crate::attempt_record::AttemptRecord::start(&disk, &job, attempt, 1)
            .unwrap()
            .unwrap();
        let material = record.material("resume.pdf", b"%PDF-fictional").unwrap();
        record.document_visual_report(&json!({"status":"complete","material":material,"document_sha256":waypoint_domain::sha256_hex(b"%PDF-fictional"),"context_sha256":visual_context.digest,"report":{"complete":true,"findings":[]}})).unwrap();
        assert_eq!(
            read_review(&disk, &job, "pdf", attempt).unwrap()["context_current"],
            true
        );
        disk.add_fact(
            "Another fictional qualification",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        assert_eq!(
            read_review(&disk, &job, "pdf", attempt).unwrap()["context_current"],
            false
        );
        disk.add_document("other-pdf", "other.pdf", b"%PDF-different", 1)
            .unwrap();
        assert!(read_review(&disk, &job, "other-pdf", attempt).is_err());
        assert!(read_review(
            &disk,
            &JobIdentityId::new("unknown-job").unwrap(),
            "pdf",
            attempt
        )
        .is_err());
        assert!(read_review(&disk, &job, "pdf", "../bad").is_err());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body =
            json!({"job_id":job.as_str(),"document_id":"pdf","attempt_id":attempt}).to_string();
        write!(client, "POST /api/documents/job-check-review HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut disk, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        let payload: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["attempt_id"], attempt);
        assert_eq!(payload["context_current"], false);
    }
}
