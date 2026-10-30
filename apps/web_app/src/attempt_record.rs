//! Local, human-readable preparation-attempt evidence. Records are plaintext.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{fs, io::Write, path::PathBuf};
use waypoint_domain::{JobIdentityId, Sha256Digest};
use waypoint_workspace::Workspace;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GmailReview {
    pub job_id: String,
    pub attempt_id: String,
    pub sender: String,
    pub recipient: String,
    pub subject: String,
    pub message: String,
    pub attachments: Vec<GmailReviewAttachment>,
    pub mime_sha256: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GmailReviewAttachment {
    pub name: String,
    pub stored_name: String,
    #[serde(default)]
    pub visual_review_file: Option<String>,
}

/// Copy a recorded outcome to the human-readable attempt folder. The database
/// remains authoritative if this optional file cannot be written.
pub fn record_email_outcome(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    outcome: waypoint_store::email_handoff::EmailHandoffOutcome,
    timestamp: i64,
) -> Result<(), String> {
    let state = match outcome {
        waypoint_store::email_handoff::EmailHandoffOutcome::Accepted => "accepted",
        waypoint_store::email_handoff::EmailHandoffOutcome::Uncertain => "uncertain",
        waypoint_store::email_handoff::EmailHandoffOutcome::NotSent => "not_sent",
    };
    record_email_file(
        ws,
        job,
        attempt,
        "email-handoff.json",
        &json!({"handoff_id":attempt,"state":state,"recorded_at":timestamp,"note":"SMTP acceptance does not prove inbox delivery. Check the mailbox before any retry."}),
    )
}

pub fn record_email_reconciliation(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    decision: &str,
    timestamp: i64,
) -> Result<(), String> {
    if !matches!(decision, "user_reports_sent" | "user_reports_not_sent") {
        return Err("Invalid reconciliation".into());
    }
    record_email_file(
        ws,
        job,
        attempt,
        "email-reconciliation.json",
        &json!({"handoff_id":attempt,"decision":decision,"recorded_at":timestamp,"note":"This is the user's report, not independently verified delivery evidence."}),
    )
}

pub fn record_email_rehearsal(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    rehearsal: &str,
    timestamp: i64,
    bytes: usize,
) -> Result<(), String> {
    if rehearsal.len() != 32 || !rehearsal.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid rehearsal identity".into());
    }
    record_email_file(
        ws,
        job,
        attempt,
        &format!("email-rehearsal-{rehearsal}.json"),
        &json!({"rehearsal_id":rehearsal,"recorded_at":timestamp,"simulated":true,"email_sent":false,"smtp_bytes_simulated":bytes,"note":"No network connection was opened; a later send must recheck all conditions."}),
    )
}

fn record_email_file(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    filename: &str,
    value: &Value,
) -> Result<(), String> {
    if attempt.len() != 32 || !attempt.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid application record".into());
    }
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Application records require a disk workspace")?;
    let hash = Sha256Digest::of(job.as_str());
    let dir = root
        .join("application-records")
        .join(format!("{}-{attempt}", &hash.as_str()[..16]));
    let meta = fs::symlink_metadata(&dir).map_err(|_| "Application record not found")?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Invalid application record".into());
    }
    let info_path = dir.join("attempt.json");
    let info_meta =
        fs::symlink_metadata(&info_path).map_err(|_| "Application record metadata not found")?;
    if !info_meta.is_file() || info_meta.file_type().is_symlink() || info_meta.len() > 65536 {
        return Err("Invalid application record metadata".into());
    }
    let info: Value =
        serde_json::from_slice(&fs::read(info_path).map_err(|_| "Cannot read application record")?)
            .map_err(|_| "Invalid application record metadata")?;
    if info["job_id"].as_str() != Some(job.as_str()) || info["attempt_id"].as_str() != Some(attempt)
    {
        return Err("Application record does not match the handoff".into());
    }
    let mut file =
        tempfile::NamedTempFile::new_in(&dir).map_err(|_| "Cannot write email record")?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|_| "Cannot write email record")?;
    file.flush().map_err(|_| "Cannot write email record")?;
    file.persist_noclobber(dir.join(filename))
        .map_err(|_| "Cannot save email record")?;
    Ok(())
}

/// Rebuild the packet from its saved review details and materials before
/// returning those details. This is a review aid, not submission approval.
pub fn gmail_review(ws: &Workspace, job: &JobIdentityId, attempt: &str) -> Result<Value, String> {
    let (review, _) = load_gmail_review(ws, job, attempt)?;
    let mut value =
        serde_json::to_value(&review).map_err(|_| "Cannot display saved Gmail review")?;
    let visual_context = crate::document_visual_context(ws, job)?;
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Application records require a disk workspace")?;
    let job_hash = Sha256Digest::of(job.as_str());
    let dir = root
        .join("application-records")
        .join(format!("{}-{attempt}", &job_hash.as_str()[..16]));
    for (index, attachment) in review.attachments.iter().enumerate() {
        let mut status = "not_linked";
        let mut findings = Value::Null;
        if let Some(filename) = attachment.visual_review_file.as_deref() {
            status = "invalid";
            let path = std::path::Path::new(filename);
            if path.components().count() == 1
                && matches!(
                    path.components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                let file = dir.join(path);
                if fs::symlink_metadata(&file).is_ok_and(|m| {
                    m.is_file() && !m.file_type().is_symlink() && m.len() <= 512 * 1024
                }) {
                    if let Some(saved) = fs::read(&file)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    {
                        let visual = &saved["review"];
                        let material = dir.join("materials").join(&attachment.stored_name);
                        if let Ok(bytes) = fs::read(material) {
                            if visual["document_sha256"].as_str()
                                == Some(waypoint_domain::sha256_hex(&bytes).as_str())
                                && visual["status"] == "complete"
                                && visual["report"]["complete"] == true
                            {
                                status = if visual["context_sha256"].as_str()
                                    == Some(Sha256Digest::of(&visual_context).as_str())
                                {
                                    "current"
                                } else {
                                    "stale"
                                };
                                findings = visual["report"]["findings"].clone();
                            }
                        }
                    }
                }
            }
        }
        value["attachments"][index]["visual_review_status"] = json!(status);
        value["attachments"][index]["visual_review_findings"] = findings;
    }
    value["context_sha256"] = json!(ws.email_review_context(job)?.as_str());
    let (total, pending, bounded, missing_employer) =
        ws.duplicate_coverage_counts().map_err(|e| e.to_string())?;
    value["duplicate_coverage"] = json!({"total":total,"pending":pending,"bounded":bounded,"missing_employer":missing_employer});
    Ok(value)
}

pub fn load_gmail_review(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
) -> Result<
    (
        GmailReview,
        crate::application_email::PreparedApplicationEmail,
    ),
    String,
> {
    if attempt.len() != 32 || !attempt.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid application record".into());
    }
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Application records require a disk workspace")?;
    let hash = Sha256Digest::of(job.as_str());
    let dir = root
        .join("application-records")
        .join(format!("{}-{attempt}", &hash.as_str()[..16]));
    let metadata = fs::symlink_metadata(&dir).map_err(|_| "Application record not found")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Invalid application record".into());
    }
    let read_bounded = |path: &std::path::Path, limit: u64| -> Result<Vec<u8>, String> {
        let meta = fs::symlink_metadata(path).map_err(|_| "Saved packet file is unavailable")?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
            return Err("Saved packet file is invalid or too large".into());
        }
        fs::read(path).map_err(|_| "Cannot read saved packet file".into())
    };
    let review: GmailReview =
        serde_json::from_slice(&read_bounded(&dir.join("gmail-review.json"), 65536)?)
            .map_err(|_| "Saved Gmail review is invalid")?;
    if review.job_id != job.as_str() || review.attempt_id != attempt || review.attachments.len() > 5
    {
        return Err("Saved Gmail review does not match this job".into());
    }
    let mut attachments = Vec::new();
    let mut total = 0usize;
    for attachment in &review.attachments {
        let path = std::path::Path::new(&attachment.stored_name);
        if path.components().count() != 1
            || !matches!(
                path.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err("Invalid saved attachment name".into());
        }
        let bytes = read_bounded(&dir.join("materials").join(path), 10 * 1024 * 1024)?;
        total = total
            .checked_add(bytes.len())
            .ok_or("Attachments exceed the limit")?;
        if total > 10 * 1024 * 1024 {
            return Err("Attachments exceed the limit".into());
        }
        attachments.push((attachment.name.clone(), bytes));
    }
    let packet = crate::application_email::PreparedApplicationEmail::prepare(
        &review.sender,
        &review.recipient,
        &review.subject,
        &review.message,
        &attachments,
        attempt,
    )?;
    let saved = read_bounded(&dir.join("email-draft.eml"), 15 * 1024 * 1024)?;
    if packet.mime() != saved || waypoint_domain::sha256_hex(&saved) != review.mime_sha256 {
        return Err("Saved message or materials changed; prepare a new packet".into());
    }
    Ok((review, packet))
}

/// Read a saved visual review only after verifying its job and PDF snapshot.
pub fn document_visual_review(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
) -> Result<Value, String> {
    if attempt.len() != 32 || !attempt.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid application record".into());
    }
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Application records require a disk workspace")?;
    let hash = Sha256Digest::of(job.as_str());
    let dir = root
        .join("application-records")
        .join(format!("{}-{attempt}", &hash.as_str()[..16]));
    let meta = fs::symlink_metadata(&dir).map_err(|_| "Application record not found")?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Invalid application record".into());
    }
    let read = |path: &std::path::Path, limit: u64| -> Result<Vec<u8>, String> {
        let meta = fs::symlink_metadata(path).map_err(|_| "Saved review file unavailable")?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
            return Err("Invalid saved review file".into());
        }
        fs::read(path).map_err(|_| "Cannot read saved review file".into())
    };
    let info: Value = serde_json::from_slice(&read(&dir.join("attempt.json"), 65536)?)
        .map_err(|_| "Invalid attempt metadata")?;
    if info["job_id"].as_str() != Some(job.as_str()) || info["attempt_id"].as_str() != Some(attempt)
    {
        return Err("Saved review does not match the job".into());
    }
    let report: Value =
        serde_json::from_slice(&read(&dir.join("document-visual-review.json"), 512 * 1024)?)
            .map_err(|_| "Invalid visual review")?;
    let name = report["material"]
        .as_str()
        .ok_or("Visual review has no PDF snapshot")?;
    let path = std::path::Path::new(name);
    if path.components().count() != 1
        || !matches!(
            path.components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Err("Invalid visual review material".into());
    }
    let materials = dir.join("materials");
    let metadata = fs::symlink_metadata(&materials).map_err(|_| "Review materials unavailable")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Invalid review materials folder".into());
    }
    let pdf = read(&materials.join(path), 10 * 1024 * 1024)?;
    if report["document_sha256"].as_str() != Some(waypoint_domain::sha256_hex(&pdf).as_str()) {
        return Err("Saved PDF snapshot changed; this report cannot be verified".into());
    }
    Ok(report)
}

/// Find a bounded set of reviews for these exact document bytes and job.
pub fn matching_document_reviews(
    ws: &Workspace,
    job: &JobIdentityId,
    document_sha: &str,
    context_sha: &str,
) -> Result<Value, String> {
    let listing = list(ws, job)?;
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Application records require a disk workspace")?;
    let hash = Sha256Digest::of(job.as_str());
    let mut reviews = Vec::new();
    let mut unreadable = 0usize;
    let mut limited = false;
    for item in listing["attempts"]
        .as_array()
        .ok_or("Invalid application record listing")?
    {
        if item["document_visual_review"] != true {
            continue;
        }
        let Some(attempt) = item["attempt_id"]
            .as_str()
            .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        else {
            unreadable += 1;
            continue;
        };
        let path = root
            .join("application-records")
            .join(format!("{}-{attempt}", &hash.as_str()[..16]))
            .join("document-visual-review.json");
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= 512 * 1024 => m,
            _ => {
                unreadable += 1;
                continue;
            }
        };
        let _ = meta;
        let parsed: Value = match fs::read(&path)
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
        {
            Some(v) => v,
            None => {
                unreadable += 1;
                continue;
            }
        };
        if parsed["document_sha256"].as_str() != Some(document_sha) {
            continue;
        }
        if reviews.len() >= 5 {
            limited = true;
            break;
        }
        match document_visual_review(ws,job,attempt) {
            Ok(review)=>reviews.push(json!({"attempt_id":attempt,"started_at":item["started_at"],"status":review["status"],"context_current":review["context_sha256"].as_str()==Some(context_sha),"report":review["report"]})),
            Err(_)=>unreadable+=1,
        }
    }
    Ok(
        json!({"reviews":reviews,"truncated":limited || listing["truncated"]==true,"unreadable":unreadable + listing["unreadable"].as_u64().unwrap_or(0) as usize}),
    )
}

pub struct GeneratedEvidence {
    pub attempt_id: String,
    pub kind: String,
    pub context_sha256: String,
    pub evidence: Option<Vec<u8>>,
    pub template: Option<(String, Vec<u8>)>,
}

pub struct GeneratedEvidenceLookup {
    pub found: Option<GeneratedEvidence>,
    pub incomplete: bool,
}

/// Find evidence only for the exact saved document and bytes in this job's
/// bounded attempt listing. Missing receipts are normal for user-supplied PDFs.
pub fn matching_generated_evidence(
    ws: &Workspace,
    job: &JobIdentityId,
    document_id: &str,
    pdf: &[u8],
) -> Result<GeneratedEvidenceLookup, String> {
    let listing = list(ws, job)?;
    matching_generated_evidence_in_listing(ws, job, document_id, pdf, &listing)
}

pub fn matching_generated_evidence_in_listing(
    ws: &Workspace,
    job: &JobIdentityId,
    document_id: &str,
    pdf: &[u8],
    listing: &Value,
) -> Result<GeneratedEvidenceLookup, String> {
    let root = ws
        .db_path
        .as_ref()
        .and_then(|path| path.parent())
        .ok_or("Application records require a disk workspace")?;
    let hash = Sha256Digest::of(job.as_str());
    let pdf_hash = waypoint_domain::sha256_hex(pdf);
    let mut found = None;
    let mut incomplete =
        listing["truncated"] == true || listing["unreadable"].as_u64().unwrap_or(0) > 0;
    let read = |path: &std::path::Path, limit: u64| -> Result<Vec<u8>, String> {
        let meta = fs::symlink_metadata(path).map_err(|_| "Evidence file unavailable")?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
            return Err("Invalid evidence file".into());
        }
        fs::read(path).map_err(|_| "Cannot read evidence file".into())
    };
    let component = |value: &Value| -> Result<String, String> {
        let name = value.as_str().ok_or("Missing material name")?;
        let path = std::path::Path::new(name);
        if path.components().count() != 1
            || !matches!(
                path.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err("Invalid material name".into());
        }
        Ok(name.to_owned())
    };
    for item in listing["attempts"]
        .as_array()
        .ok_or("Invalid application record listing")?
    {
        let Some(attempt) = item["attempt_id"]
            .as_str()
            .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        else {
            incomplete = true;
            continue;
        };
        let dir = root
            .join("application-records")
            .join(format!("{}-{attempt}", &hash.as_str()[..16]));
        let receipt_path = dir.join("document-export.json");
        if let Err(error) = fs::symlink_metadata(&receipt_path) {
            if error.kind() == std::io::ErrorKind::NotFound {
                continue;
            }
            incomplete = true;
            continue;
        }
        let candidate = (|| -> Result<Option<GeneratedEvidence>, String> {
            let dir_meta = fs::symlink_metadata(&dir).map_err(|_| "Invalid record")?;
            if !dir_meta.is_dir() || dir_meta.file_type().is_symlink() {
                return Err("Invalid record".into());
            }
            let info: Value = serde_json::from_slice(&read(&dir.join("attempt.json"), 65536)?)
                .map_err(|_| "Invalid record metadata")?;
            if info["job_id"].as_str() != Some(job.as_str())
                || info["attempt_id"].as_str() != Some(attempt)
            {
                return Err("Wrong record identity".into());
            }
            let receipt: Value = serde_json::from_slice(&read(&receipt_path, 65536)?)
                .map_err(|_| "Invalid export receipt")?;
            if receipt["document_id"].as_str() != Some(document_id) {
                return Ok(None);
            }
            if receipt["pdf_sha256"].as_str() != Some(pdf_hash.as_str()) {
                return Err("Document changed since export".into());
            }
            let kind = receipt["kind"]
                .as_str()
                .filter(|kind| matches!(*kind, "cv" | "cover_letter"))
                .ok_or("Invalid document kind")?;
            let context = receipt["context_sha256"]
                .as_str()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or("Invalid context digest")?;
            let materials = dir.join("materials");
            let meta = fs::symlink_metadata(&materials).map_err(|_| "Invalid materials folder")?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err("Invalid materials folder".into());
            }
            let source_pdf = read(
                &materials.join(component(&receipt["pdf"])?),
                10 * 1024 * 1024,
            )?;
            if waypoint_domain::sha256_hex(&source_pdf) != pdf_hash {
                return Err("Saved PDF changed".into());
            }
            let evidence = if receipt["evidence"].is_null() {
                if !receipt["evidence_sha256"].is_null() {
                    return Err("Invalid draft evidence receipt".into());
                }
                None
            } else {
                let bytes = read(
                    &materials.join(component(&receipt["evidence"])?),
                    128 * 1024,
                )?;
                if receipt["evidence_sha256"].as_str()
                    != Some(waypoint_domain::sha256_hex(&bytes).as_str())
                    || serde_json::from_slice::<Value>(&bytes).is_err()
                {
                    return Err("Saved draft evidence changed".into());
                }
                Some(bytes)
            };
            let template = if receipt["template"].is_null() {
                None
            } else {
                let original = receipt["template_original_name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 2000)
                    .ok_or("Invalid template name")?;
                let bytes = read(
                    &materials.join(component(&receipt["template"])?),
                    10 * 1024 * 1024,
                )?;
                if receipt["template_sha256"].as_str()
                    != Some(waypoint_domain::sha256_hex(&bytes).as_str())
                {
                    return Err("Saved template changed".into());
                }
                Some((original.to_owned(), bytes))
            };
            if evidence.is_none() && template.is_none() {
                return Err("Export receipt has no retained evidence or template".into());
            }
            Ok(Some(GeneratedEvidence {
                attempt_id: attempt.to_owned(),
                kind: kind.to_owned(),
                context_sha256: context.to_owned(),
                evidence,
                template,
            }))
        })();
        match candidate {
            Ok(Some(value)) if found.is_none() => found = Some(value),
            Ok(Some(_)) => {
                found = None;
                incomplete = true;
                break;
            }
            Ok(None) => {}
            Err(_) => incomplete = true,
        }
    }
    Ok(GeneratedEvidenceLookup { found, incomplete })
}

pub struct AttemptRecord {
    dir: PathBuf,
    sequence: u32,
}

/// List a bounded number of locally saved attempts without reading their materials.
pub fn list(ws: &Workspace, id: &JobIdentityId) -> Result<Value, String> {
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Attempt records require a disk-backed workspace")?
        .join("application-records");
    if !root.exists() {
        return Ok(json!({"attempts":[],"truncated":false,"unreadable":0}));
    }
    let hash = Sha256Digest::of(id.as_str());
    let prefix = format!("{}-", &hash.as_str()[..16]);
    let mut attempts = Vec::new();
    let mut unreadable = 0;
    let mut truncated = false;
    for (index, entry) in fs::read_dir(&root).map_err(|e| e.to_string())?.enumerate() {
        if index >= 2000 {
            truncated = true;
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let metadata = match entry.file_type() {
            Ok(kind) if kind.is_dir() && !kind.is_symlink() => entry.path().join("attempt.json"),
            _ => {
                unreadable += 1;
                continue;
            }
        };
        let file = match fs::metadata(&metadata) {
            Ok(meta) if meta.is_file() && meta.len() <= 65536 => metadata,
            _ => {
                unreadable += 1;
                continue;
            }
        };
        let value: Value = match fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            Some(value) => value,
            None => {
                unreadable += 1;
                continue;
            }
        };
        if value["job_id"].as_str() != Some(id.as_str()) {
            unreadable += 1;
            continue;
        }
        let (Some(attempt_id), Some(status), Some(started)) = (
            value["attempt_id"].as_str(),
            value["status"].as_str(),
            value["started_at"].as_i64(),
        ) else {
            unreadable += 1;
            continue;
        };
        let gmail_review =
            fs::symlink_metadata(entry.path().join("gmail-review.json")).is_ok_and(|meta| {
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 65536
            });
        let has_record = |name| {
            fs::symlink_metadata(entry.path().join(name)).is_ok_and(|meta| {
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 65536
            })
        };
        attempts.push(json!({"attempt_id":attempt_id,"status":status,"started_at":started,"folder":entry.path().to_string_lossy(),"submitted":value["submitted"],"submitted_by_waypoint":value["submitted_by_waypoint"],"page_capture_available":value["page_capture_available"],"page_capture_source_url":value["page_capture_source_url"],"page_capture_matches_current_url":value["page_capture_matches_current_url"],"gmail_review":gmail_review,"email_outcome_file":has_record("email-handoff.json"),"email_reconciliation_file":has_record("email-reconciliation.json"),"document_visual_review":fs::symlink_metadata(entry.path().join("document-visual-review.json")).is_ok_and(|m|m.is_file() && !m.file_type().is_symlink() && m.len() <= 512*1024)}));
    }
    attempts.sort_by_key(|item| std::cmp::Reverse(item["started_at"].as_i64().unwrap_or_default()));
    if attempts.len() > 50 {
        attempts.truncate(50);
        truncated = true;
    }
    Ok(json!({"attempts":attempts,"truncated":truncated,"unreadable":unreadable}))
}
impl AttemptRecord {
    pub fn start(
        ws: &Workspace,
        id: &JobIdentityId,
        attempt_id: &str,
        timestamp: i64,
    ) -> Result<Option<Self>, String> {
        if attempt_id.is_empty()
            || attempt_id.len() > 64
            || !attempt_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("Invalid attempt identity".into());
        }
        let Some(root) = ws.db_path.as_ref().and_then(|p| p.parent()) else {
            return Ok(None);
        };
        let job = ws
            .job(id)
            .map_err(|e| e.to_string())?
            .ok_or("Job not found")?;
        let description = ws.job_description(id).map_err(|e| e.to_string())?;
        let captured = ws.job_capture(id).map_err(|e| e.to_string())?;
        let source_only = ws.job_source_only(id).map_err(|e| e.to_string())?;
        let capture_source_url = captured.as_ref().and_then(|(metadata, _)| {
            serde_json::from_slice::<Value>(metadata)
                .ok()
                .and_then(|capture| capture["source_url"].as_str().map(str::to_owned))
        });
        let capture_matches_current_url = capture_source_url
            .as_deref()
            .zip(job.canonical_url.as_deref())
            .map(|(captured_url, current_url)| captured_url == current_url);
        let hash = Sha256Digest::of(id.as_str());
        let records = root.join("application-records");
        fs::create_dir_all(&records).map_err(|e| e.to_string())?;
        let dir = records.join(format!("{}-{attempt_id}", &hash.as_str()[..16]));
        fs::create_dir(&dir).map_err(|e| format!("Could not create a new attempt folder: {e}"))?;
        fs::create_dir(dir.join("screenshots")).map_err(|e| e.to_string())?;
        fs::create_dir(dir.join("materials")).map_err(|e| e.to_string())?;
        let posting = json!({
            "job_id": id.as_str(),
            "title": job.title,
            "employer": job.employer,
            "location": job.location,
            "source": job.source,
            "source_url": job.canonical_url,
            "requisition_id": job.requisition_id,
            "posted_at": job.posted_at,
            "first_seen": job.first_seen,
            "last_seen": job.last_seen,
            "remote": job.remote,
            "snapshot_at": timestamp,
            "description_available": description.is_some(),
            "description_sha256": description.as_deref().map(|text| Sha256Digest::of(text).as_str().to_owned()),
            "description": description,
            "page_capture_available": captured.is_some(),
            "source_only_metadata_available": source_only.is_some(),
            "page_capture_source_url": capture_source_url,
            "page_capture_matches_current_url": capture_matches_current_url,
            "note": "Workspace listing text as saved at the start of this attempt; it may have been extracted or pasted and is not a capture of the original page."
        });
        fs::write(
            dir.join("original-job.json"),
            serde_json::to_vec_pretty(&posting).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let listing_text = posting["description"].as_str().unwrap_or(
            "No job description was available in the workspace when this attempt started.",
        );
        fs::write(dir.join("original-job.txt"), listing_text).map_err(|e| e.to_string())?;
        if let Some(source) = source_only {
            fs::write(dir.join("original-job-source-metadata.json"), source)
                .map_err(|e| e.to_string())?;
        }
        if let Some((metadata, segments)) = captured {
            let source_dir = dir.join("original-job-page");
            fs::create_dir_all(&source_dir).map_err(|e| e.to_string())?;
            fs::write(source_dir.join("capture.json"), &metadata).map_err(|e| e.to_string())?;
            if let Ok(capture) = serde_json::from_slice::<Value>(&metadata) {
                if matches!(
                    capture["page_text_status"].as_str(),
                    Some("captured" | "truncated")
                ) {
                    let text = capture["page_text"]
                        .as_str()
                        .filter(|text| text.len() <= 32_000)
                        .ok_or("Invalid captured page text")?;
                    fs::write(source_dir.join("page-text.txt"), text).map_err(|e| e.to_string())?;
                }
                if capture["structured_source_status"] == "captured" {
                    let scripts = capture["structured_source_scripts"]
                        .as_array()
                        .ok_or("Invalid structured source capture")?;
                    if scripts.len() > 8 || scripts.iter().any(|s| s.as_str().is_none()) {
                        return Err("Invalid structured source capture".into());
                    }
                    for (index, script) in scripts.iter().enumerate() {
                        fs::write(
                            source_dir.join(format!("structured-{:04}.json.txt", index + 1)),
                            script.as_str().unwrap().as_bytes(),
                        )
                        .map_err(|e| e.to_string())?;
                    }
                }
            }
            for (index, png) in segments.iter().enumerate() {
                fs::write(source_dir.join(format!("{:04}.png", index + 1)), png)
                    .map_err(|e| e.to_string())?;
            }
        }
        let meta = json!({"attempt_id":attempt_id,"job_id":id.as_str(),"employer":job.employer,"title":job.title,"job_url":job.canonical_url,"started_at":timestamp,"status":"preparation_in_progress","submitted":null,"submitted_by_waypoint":false,"page_capture_available":posting["page_capture_available"],"source_only_metadata_available":posting["source_only_metadata_available"],"page_capture_source_url":posting["page_capture_source_url"],"page_capture_matches_current_url":posting["page_capture_matches_current_url"],"note":"External submission is not monitored; this record does not prove whether an application was submitted."});
        fs::write(
            dir.join("attempt.json"),
            serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if let Some(packet) = ws.packet(id).map_err(|e| e.to_string())? {
            fs::write(
                dir.join("materials").join("prepared-draft.txt"),
                packet.body,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(Some(Self { dir, sequence: 0 }))
    }
    pub fn event(&mut self, value: Value) -> Result<(), String> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("events.jsonl"))
            .map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut f, &value).map_err(|e| e.to_string())?;
        f.write_all(b"\n").map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())
    }
    pub fn status(&mut self, status: &str, timestamp: i64) -> Result<(), String> {
        if !matches!(
            status,
            "preparation_in_progress" | "manual_review" | "review_session_closed"
        ) {
            return Err("Invalid attempt status".into());
        }
        let path = self.dir.join("attempt.json");
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("Attempt metadata exceeds limit".into());
        }
        let mut meta: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if meta["status"] != "manual_review" || status == "manual_review" {
            meta["status"] = json!(status);
        }
        meta["updated_at"] = json!(timestamp);
        let mut tmp = tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut tmp, &meta).map_err(|e| e.to_string())?;
        tmp.flush().map_err(|e| e.to_string())?;
        tmp.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn inspection(
        &mut self,
        origin: &str,
        reasons: &[String],
        screenshot: Option<&str>,
        timestamp: i64,
    ) -> Result<(), String> {
        let name = screenshot
            .map(|image| self.screenshot_with_name(image))
            .transpose()?;
        self.event(json!({"type":"page_inspected","time":timestamp,"origin":origin,"manual_review":reasons,"screenshot_available":name.is_some(),"screenshot":name.map(|name| format!("screenshots/{name}"))}))?;
        self.status(
            if reasons.is_empty() {
                "preparation_in_progress"
            } else {
                "manual_review"
            },
            timestamp,
        )
    }
    pub fn email_draft(&mut self, bytes: &[u8]) -> Result<(), String> {
        fs::write(self.dir.join("email-draft.eml"), bytes).map_err(|e| e.to_string())
    }
    pub fn gmail_review(&mut self, review: &GmailReview) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(review).map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("Gmail review exceeds size limit".into());
        }
        fs::write(self.dir.join("gmail-review.json"), bytes).map_err(|e| e.to_string())
    }
    pub fn document_export_receipt(&mut self, receipt: &Value) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(receipt).map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("Document export receipt exceeds limit".into());
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.dir.join("document-export.json"))
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())
    }
    pub fn document_visual_evidence(
        &mut self,
        source_attempt: &str,
        review: &Value,
    ) -> Result<String, String> {
        if source_attempt.len() != 32 || !source_attempt.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid source review".into());
        }
        let bytes =
            serde_json::to_vec_pretty(&json!({"source_attempt":source_attempt,"review":review}))
                .map_err(|e| e.to_string())?;
        if bytes.len() > 512 * 1024 {
            return Err("Visual review evidence exceeds limit".into());
        }
        self.sequence += 1;
        let name = format!("document-visual-{:04}.json", self.sequence);
        let mut file = tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        file.persist_noclobber(self.dir.join(&name))
            .map_err(|e| e.to_string())?;
        Ok(name)
    }
    pub fn document_visual_report(&mut self, value: &Value) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
        if bytes.len() > 512 * 1024 {
            return Err("Document review report exceeds limit".into());
        }
        let mut file = tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        file.persist_noclobber(self.dir.join("document-visual-review.json"))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn visual_result_path(&self, task_id: &str) -> Result<PathBuf, String> {
        if task_id.len() != 32 || !task_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid visual task identity".into());
        }
        Ok(self.dir.join(format!("visual-result-{task_id}.json")))
    }
    pub fn screenshot_with_name(&mut self, data: &str) -> Result<String, String> {
        let bytes = STANDARD.decode(data).map_err(|e| e.to_string())?;
        if bytes.len() > 6 * 1024 * 1024 {
            return Err("Screenshot exceeds limit".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Screenshot sequence exhausted")?;
        let name = format!("{:04}.png", self.sequence);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.dir.join("screenshots").join(&name))
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        Ok(name)
    }
    pub fn material(&mut self, name: &str, bytes: &[u8]) -> Result<String, String> {
        // Reserve space for our sequence and a short extension even when the
        // original upload already uses the filesystem's component limit.
        // The name requested by the caller is retained in the manifest below.
        let safe = portable_name(name, 96);
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Material sequence exhausted")?;
        let filename = format!("{:04}-{safe}", self.sequence);
        // On Windows canonicalization yields an extended-length path, so a
        // valid component remains writable in a deeply nested workspace.
        let materials = fs::canonicalize(self.dir.join("materials")).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(&materials).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        file.persist_noclobber(materials.join(&filename))
            .map_err(|e| e.to_string())?;
        let mut manifest = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(materials.join("manifest.jsonl"))
            .map_err(|e| format!("Material was saved but its manifest could not be opened: {e}"))?;
        serde_json::to_writer(
            &mut manifest,
            &json!({"requested_name":name,"stored_name":filename,"bytes":bytes.len(),"sha256":waypoint_domain::sha256_hex(bytes)}),
        )
        .map_err(|e| format!("Material was saved but its manifest could not be written: {e}"))?;
        manifest.write_all(b"\n").map_err(|e| {
            format!("Material was saved but its manifest could not be written: {e}")
        })?;
        manifest.flush().map_err(|e| {
            format!("Material was saved but its manifest could not be flushed: {e}")
        })?;
        Ok(filename)
    }
}

fn portable_name(name: &str, stem_limit: usize) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let safe = safe.trim_end_matches(['.', ' ']);
    let (stem, extension) = match safe.rsplit_once('.') {
        Some((stem, extension))
            if !extension.is_empty()
                && extension.len() <= 32
                && extension.chars().all(|c| !c.is_whitespace()) =>
        {
            (stem, format!(".{extension}"))
        }
        _ => (safe, String::new()),
    };
    let mut end = stem.len().min(stem_limit);
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    let stem = stem[..end].trim_end_matches(['.', ' ']);
    let stem = if stem.trim_matches('.').trim().is_empty() {
        "document"
    } else {
        stem
    };
    format!("{stem}{extension}")
}

/// Name presented to a website when attaching a saved document. A separate
/// prefix is needed for Windows device names because this name has no sequence.
pub fn attachment_filename(name: &str) -> String {
    let safe = portable_name(name, 120);
    let stem = safe.split('.').next().unwrap_or("").trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    if reserved {
        format!("_{safe}")
    } else {
        safe
    }
}

/// Save a model result independently of the review session or browser polling.
pub fn write_visual_result(
    path: &std::path::Path,
    task_id: &str,
    result: &Result<Value, String>,
    timestamp: i64,
) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid visual result path")?;
    let value = match result {
        Ok(review) => {
            json!({"type":"visual_result","time":timestamp,"task_id":task_id,"status":"complete","visual_review":review})
        }
        Err(error) => {
            json!({"type":"visual_result","time":timestamp,"task_id":task_id,"status":"failed","error":error})
        }
    };
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, &value).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist_noclobber(path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Record the reviewed page and bounded evidence for off-screen content.
pub fn capture_inspection(
    driver: &mut waypoint_browser::CdpDriver,
    record: &mut AttemptRecord,
    origin: &str,
    reasons: &[String],
    screenshot: Option<&str>,
    timestamp: i64,
) -> Result<(), String> {
    record.inspection(origin, reasons, screenshot, timestamp)?;
    if let Err(error) = capture_page(driver, record, timestamp) {
        let _ = record.event(json!({"type":"capture_incomplete","time":timestamp,"error":error}));
        let _ = record.status("manual_review", timestamp);
        return Err(error);
    }
    Ok(())
}

/// Save a maximum of six segments; report when a page exceeds the limit.
pub fn capture_page(
    driver: &mut waypoint_browser::CdpDriver,
    record: &mut AttemptRecord,
    timestamp: i64,
) -> Result<(), String> {
    let (width, height) = driver.page_dimensions().map_err(|e| e.to_string())?;
    let width_limited = width.min(2048.0);
    let capped_height = height.min(12000.0);
    let capture_id = crate::random_id()?;
    record.event(json!({"type":"page_capture_started","time":timestamp,"capture_id":capture_id,"page_width":width,"page_height":height}))?;
    let mut y = 0.0;
    let mut count = 0;
    while y < capped_height {
        let segment_height = (capped_height - y).min(2000.0);
        let image = driver
            .screenshot_segment(width_limited, y, segment_height)
            .map_err(|e| e.to_string())?;
        let name = record.screenshot_with_name(&image)?;
        record.event(json!({"type":"page_capture_segment","time":timestamp,"capture_id":capture_id,"index":count,"x":0,"y":y,"width":width_limited,"height":segment_height,"screenshot":format!("screenshots/{name}")}))?;
        y += segment_height;
        count += 1;
    }
    let within_limits = width <= 2048.0 && height <= 12000.0;
    record.event(json!({"type":"page_capture","time":timestamp,"capture_id":capture_id,"page_width":width,"page_height":height,"segments":count,"full_dimensions_captured":within_limits,"note":"Page dimensions alone do not guarantee capture of hidden content or nested scrollers."}))?;
    if !within_limits {
        return Err("Page exceeded screenshot capture limits".into());
    }
    Ok(())
}

/// Preserve the current section immediately before opening a reviewed link.
/// Capture failure remains visible even if the browser subsequently navigates.
pub fn capture_before_navigation(
    driver: &mut waypoint_browser::CdpDriver,
    record: &mut AttemptRecord,
    timestamp: i64,
) -> Result<(), String> {
    record.event(json!({"type":"navigation_capture_started","time":timestamp}))?;
    if let Err(error) = capture_page(driver, record, timestamp) {
        let _ = record
            .event(json!({"type":"navigation_capture_incomplete","time":timestamp,"error":error}));
        let _ = record.status("manual_review", timestamp);
        return Err(error);
    }
    record.event(json!({"type":"navigation_capture_complete","time":timestamp}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::ApplicationState;
    use waypoint_store::StoredJob;

    #[test]
    fn attachment_names_are_windows_safe_and_keep_the_extension() {
        assert_eq!(attachment_filename("resume.pdf"), "resume.pdf");
        assert_eq!(attachment_filename("CON.pdf"), "_CON.pdf");
        assert_eq!(attachment_filename("lpt1.pdf"), "_lpt1.pdf");
        assert_eq!(attachment_filename("COM¹.pdf"), "_COM¹.pdf");
        assert_eq!(attachment_filename("résumé.pdf"), "résumé.pdf");
        let long = format!("{}.pdf", "é".repeat(120));
        let name = attachment_filename(&long);
        assert!(name.ends_with(".pdf") && name.len() <= 153);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        std::fs::write(dir.path().join(&name), b"fictional pdf").unwrap();
        assert_eq!(
            std::fs::read(dir.path().join(&name)).unwrap(),
            b"fictional pdf"
        );
    }

    #[test]
    fn materials_preserve_bytes_with_long_names_and_do_not_overwrite() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        std::fs::create_dir(dir.path().join("materials")).unwrap();
        let mut record = AttemptRecord {
            dir: dir.path().to_path_buf(),
            sequence: 0,
        };
        let original = format!("{}.pdf", "x".repeat(251));
        let name = record.material(&original, b"fictional pdf").unwrap();
        assert!(name.ends_with(".pdf"));
        assert!(name.len() <= 160);
        assert_eq!(
            std::fs::read(dir.path().join("materials").join(&name)).unwrap(),
            b"fictional pdf"
        );
        let unicode = format!("{}.pdf", "é".repeat(120));
        let second = record.material(&unicode, b"another pdf").unwrap();
        assert!(second.ends_with(".pdf") && second.len() <= 160);
        assert_eq!(
            std::fs::read(dir.path().join("materials").join(&second)).unwrap(),
            b"another pdf"
        );
        record.sequence = 0;
        assert!(record.material(&original, b"replacement").is_err());
        assert_eq!(
            std::fs::read(dir.path().join("materials").join(&name)).unwrap(),
            b"fictional pdf"
        );
        let manifest =
            std::fs::read_to_string(dir.path().join("materials/manifest.jsonl")).unwrap();
        let entries: Vec<Value> = manifest
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["requested_name"], original);
        assert_eq!(entries[0]["stored_name"], name);
        assert_eq!(
            entries[0]["sha256"],
            waypoint_domain::sha256_hex(b"fictional pdf")
        );
        assert_eq!(entries[1]["requested_name"], unicode);
    }

    #[test]
    fn visual_result_can_be_saved_without_session_polling() {
        let dir = tempfile::tempdir().unwrap();
        let record = AttemptRecord {
            dir: dir.path().to_path_buf(),
            sequence: 0,
        };
        let id = "0123456789abcdef0123456789abcdef";
        let path = record.visual_result_path(id).unwrap();
        let worker_path = path.clone();
        std::thread::spawn(move || {
            write_visual_result(&worker_path, id, &Ok(json!({"observations":[]})), 42).unwrap();
        })
        .join()
        .unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["task_id"], id);
        assert_eq!(saved["status"], "complete");
        assert_eq!(saved["visual_review"]["observations"], json!([]));
        assert!(write_visual_result(&path, id, &Err("failure".into()), 43).is_err());
        assert!(record.visual_result_path("../escape").is_err());
    }
    #[test]
    fn visual_capture_filename_identifies_saved_bytes_and_event() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        std::fs::create_dir(dir.path().join("screenshots")).unwrap();
        let mut record = AttemptRecord {
            dir: dir.path().to_path_buf(),
            sequence: 0,
        };
        let name = record.screenshot_with_name("aW1hZ2U=").unwrap();
        record.event(json!({"type":"visual_capture","task_id":"test-task","screenshot":format!("screenshots/{name}")})).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("screenshots").join(&name)).unwrap(),
            b"image"
        );
        let event: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("events.jsonl")).unwrap())
                .unwrap();
        assert_eq!(event["task_id"], "test-task");
        assert_eq!(event["screenshot"], format!("screenshots/{name}"));
        fs::write(dir.path().join("screenshots/0002.png"), b"existing").unwrap();
        assert!(record.screenshot_with_name("bmV3").is_err());
        assert_eq!(
            fs::read(dir.path().join("screenshots/0002.png")).unwrap(),
            b"existing"
        );
    }
    #[test]
    fn saved_document_review_binds_job_and_pdf_snapshot() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        let id = JobIdentityId::new("fictional-document-review").unwrap();
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
            Some("Fictional job"),
        )
        .unwrap();
        let attempt = "0123456789abcdef0123456789abcdef";
        let mut record = AttemptRecord::start(&ws, &id, attempt, 1).unwrap().unwrap();
        let bytes = b"%PDF-fictional";
        let material = record.material("candidate.pdf", bytes).unwrap();
        let evidence = json!({"status":"complete","material":material,"document_sha256":waypoint_domain::sha256_hex(bytes),"context_sha256":"fictional-context","report":{"complete":true,"findings":[]}});
        record.document_visual_report(&evidence).unwrap();
        assert_eq!(
            document_visual_review(&ws, &id, attempt).unwrap()["status"],
            "complete"
        );
        assert_eq!(
            list(&ws, &id).unwrap()["attempts"][0]["document_visual_review"],
            true
        );
        let candidates = matching_document_reviews(
            &ws,
            &id,
            &waypoint_domain::sha256_hex(bytes),
            "fictional-context",
        )
        .unwrap();
        assert_eq!(candidates["reviews"][0]["context_current"], true);
        let stale = matching_document_reviews(
            &ws,
            &id,
            &waypoint_domain::sha256_hex(bytes),
            "changed-context",
        )
        .unwrap();
        assert_eq!(stale["reviews"][0]["context_current"], false);
        assert!(matching_document_reviews(
            &ws,
            &id,
            &waypoint_domain::sha256_hex(b"other"),
            "fictional-context"
        )
        .unwrap()["reviews"]
            .as_array()
            .unwrap()
            .is_empty());
        let mut application = AttemptRecord::start(&ws, &id, "abcdef0123456789abcdef0123456789", 2)
            .unwrap()
            .unwrap();
        let saved = application
            .document_visual_evidence(attempt, &evidence)
            .unwrap();
        assert!(application.dir.join(saved).exists());
        let other = JobIdentityId::new("other-job").unwrap();
        assert!(document_visual_review(&ws, &other, attempt).is_err());
        fs::write(record.dir.join("materials").join(material), b"changed").unwrap();
        assert!(document_visual_review(&ws, &id, attempt)
            .unwrap_err()
            .contains("changed"));
    }

    #[test]
    #[ignore = "starts one headless browser against a local fixture"]
    fn inspection_archives_bounded_offscreen_segments() {
        use waypoint_applications::driver::BrowserDriver;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        let id = JobIdentityId::new("inspection-fixture").unwrap();
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
            None,
        )
        .unwrap();
        let mut record = AttemptRecord::start(&ws, &id, "inspection", 1)
            .unwrap()
            .unwrap();
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = format!(
            "file:///{}",
            fixture.display().to_string().replace('\\', "/")
        );
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(&url).unwrap();
        let viewport = driver.screenshot_base64().unwrap();
        capture_inspection(&mut driver, &mut record, "file://", &[], Some(&viewport), 2).unwrap();
        let events = fs::read_to_string(record.dir.join("events.jsonl")).unwrap();
        let entries: Vec<Value> = events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(entries
            .iter()
            .any(|event| event["type"] == "page_inspected"));
        assert!(entries.iter().any(|event| event["type"] == "page_capture"
            && event["segments"] == 2
            && event["full_dimensions_captured"] == true));
        let segments: Vec<_> = entries
            .iter()
            .filter(|event| event["type"] == "page_capture_segment")
            .collect();
        assert_eq!(segments.len(), 2);
        let started = entries
            .iter()
            .find(|event| event["type"] == "page_capture_started")
            .unwrap();
        let finished = entries
            .iter()
            .find(|event| event["type"] == "page_capture")
            .unwrap();
        assert_eq!(started["capture_id"], finished["capture_id"]);
        for (index, event) in segments.iter().enumerate() {
            assert_eq!(event["capture_id"], started["capture_id"]);
            assert_eq!(event["index"], index);
            assert_eq!(event["y"].as_f64().unwrap(), index as f64 * 2000.0);
            let name = event["screenshot"].as_str().unwrap();
            assert!(record.dir.join(name).is_file());
        }
        let viewport = entries
            .iter()
            .find(|event| event["type"] == "page_inspected")
            .unwrap();
        assert!(record
            .dir
            .join(viewport["screenshot"].as_str().unwrap())
            .is_file());
        assert_eq!(
            fs::read_dir(record.dir.join("screenshots"))
                .unwrap()
                .count(),
            3
        );
        // Force a collision on the second segment. The first segment and its
        // capture ID must remain attributable even without a completion event.
        fs::write(record.dir.join("screenshots/0005.png"), b"existing").unwrap();
        assert!(capture_before_navigation(&mut driver, &mut record, 3).is_err());
        assert_eq!(
            fs::read(record.dir.join("screenshots/0005.png")).unwrap(),
            b"existing"
        );
        assert!(capture_page(&mut driver, &mut record, 4).is_ok());
        let entries: Vec<Value> = fs::read_to_string(record.dir.join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let starts: Vec<_> = entries
            .iter()
            .filter(|event| event["type"] == "page_capture_started")
            .collect();
        assert_eq!(starts.len(), 3);
        assert!(entries
            .iter()
            .any(|event| event["type"] == "navigation_capture_incomplete"));
        assert!(!entries
            .iter()
            .any(|event| event["type"] == "navigation_capture_complete"));
        assert_ne!(starts[1]["capture_id"], starts[2]["capture_id"]);
        let failed_id = &starts[1]["capture_id"];
        assert_eq!(
            entries
                .iter()
                .filter(|event| event["type"] == "page_capture_segment"
                    && &event["capture_id"] == failed_id)
                .count(),
            1
        );
        assert!(!entries
            .iter()
            .any(|event| event["type"] == "page_capture" && &event["capture_id"] == failed_id));
        assert!(entries.iter().any(|event| event["type"] == "page_capture"
            && event["capture_id"] == starts[2]["capture_id"]));
        driver
            .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                selector: "#candidate".into(),
                value: waypoint_applications::form_mapping::PacketValue::Text(
                    "Manual fixture change".into(),
                ),
            })
            .unwrap();
        capture_before_navigation(&mut driver, &mut record, 5).unwrap();
        let entries: Vec<Value> = fs::read_to_string(record.dir.join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let start = entries
            .iter()
            .rposition(|event| event["type"] == "navigation_capture_started")
            .unwrap();
        let end = entries
            .iter()
            .rposition(|event| event["type"] == "navigation_capture_complete")
            .unwrap();
        assert!(start < end);
        let captured: Vec<_> = entries[start..end]
            .iter()
            .filter(|event| event["type"] == "page_capture_segment")
            .collect();
        assert_eq!(captured.len(), 2);
        assert!(captured.iter().all(|event| record
            .dir
            .join(event["screenshot"].as_str().unwrap())
            .is_file()));
        let next = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/application-check-contact.html")
            .canonicalize()
            .unwrap();
        driver
            .open(url::Url::from_file_path(next).unwrap().as_str())
            .unwrap();
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }

    #[test]
    fn attempt_archives_the_saved_listing_and_source() {
        let root = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        let id = JobIdentityId::new("fictional-posting").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: id.clone(),
                title: "Engineer".into(),
                employer: "Example".into(),
                location: "Remote".into(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some("https://example.org/jobs/1".into()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: true,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional job description."),
        )
        .unwrap();
        #[cfg(windows)]
        {
            let first_source = br#"{"source_url":"https://example.org/jobs/1","segments":0}"#;
            assert!(ws.save_job_source_only(&id, first_source).unwrap());
            let png = b"\x89PNG\r\n\x1a\nfictional-data".to_vec();
            let metadata=json!({"source_url":"https://example.org/jobs/1","segments":1,"full_dimensions_captured":true,"page_text_status":"captured","page_text":"Fictional original page text","structured_source_status":"captured","structured_source_scripts":["{\"@type\":\"JobPosting\",\"description\":\"<p>Fictional role</p>\"}"]}).to_string();
            assert!(ws
                .save_job_capture(&id, metadata.as_bytes(), std::slice::from_ref(&png))
                .unwrap());
            assert!(!ws
                .save_job_capture(&id, metadata.as_bytes(), std::slice::from_ref(&png))
                .unwrap());
        }
        let mut record = AttemptRecord::start(&ws, &id, "attempt-fixture", 123)
            .unwrap()
            .unwrap();
        assert!(AttemptRecord::start(&ws, &id, "attempt-fixture", 124).is_err());
        assert!(AttemptRecord::start(&ws, &id, "../invalid", 124).is_err());
        let listing: Value =
            serde_json::from_slice(&fs::read(record.dir.join("original-job.json")).unwrap())
                .unwrap();
        assert_eq!(listing["description"], "Fictional job description.");
        assert_eq!(listing["source_url"], "https://example.org/jobs/1");
        assert_eq!(listing["description_available"], true);
        #[cfg(windows)]
        {
            assert_eq!(listing["source_only_metadata_available"], true);
            assert_eq!(
                fs::read(record.dir.join("original-job-source-metadata.json")).unwrap(),
                br#"{"source_url":"https://example.org/jobs/1","segments":0}"#
            );
            assert_eq!(listing["page_capture_available"], true);
            assert_eq!(listing["page_capture_matches_current_url"], true);
            assert_eq!(
                listing["page_capture_source_url"],
                "https://example.org/jobs/1"
            );
            assert_eq!(
                fs::read(record.dir.join("original-job-page/0001.png")).unwrap(),
                b"\x89PNG\r\n\x1a\nfictional-data"
            );
            assert!(record.dir.join("original-job-page/capture.json").exists());
            assert_eq!(
                fs::read_to_string(record.dir.join("original-job-page/page-text.txt")).unwrap(),
                "Fictional original page text"
            );
            assert_eq!(
                fs::read_to_string(
                    record
                        .dir
                        .join("original-job-page/structured-0001.json.txt")
                )
                .unwrap(),
                "{\"@type\":\"JobPosting\",\"description\":\"<p>Fictional role</p>\"}"
            );
        }
        assert_eq!(
            fs::read_to_string(record.dir.join("original-job.txt")).unwrap(),
            "Fictional job description."
        );
        assert_eq!(
            listing["description_sha256"],
            Sha256Digest::of("Fictional job description.").as_str()
        );
        record
            .inspection(
                "https://example.org",
                &["Sign-in required".into()],
                None,
                124,
            )
            .unwrap();
        record.status("review_session_closed", 125).unwrap();
        let meta: Value =
            serde_json::from_slice(&fs::read(record.dir.join("attempt.json")).unwrap()).unwrap();
        assert_eq!(meta["status"], "manual_review");
        assert!(meta["submitted"].is_null());
        assert_eq!(meta["submitted_by_waypoint"], false);
        let records = list(&ws, &id).unwrap();
        assert_eq!(records["attempts"].as_array().unwrap().len(), 1);
        assert_eq!(records["attempts"][0]["status"], "manual_review");
        assert_eq!(records["attempts"][0]["started_at"], 123);
        #[cfg(windows)]
        assert_eq!(
            records["attempts"][0]["page_capture_matches_current_url"],
            true
        );
        assert_eq!(records["unreadable"], 0);
        let other = JobIdentityId::new("other-fictional-job").unwrap();
        assert!(list(&ws, &other).unwrap()["attempts"]
            .as_array()
            .unwrap()
            .is_empty());
        fs::write(record.dir.join("attempt.json"), b"not JSON").unwrap();
        let invalid = list(&ws, &id).unwrap();
        assert_eq!(invalid["unreadable"], 1);
        assert!(invalid["attempts"].as_array().unwrap().is_empty());
        #[cfg(windows)]
        {
            let mut changed = ws.job(&id).unwrap().unwrap();
            changed.canonical_url = Some("https://example.org/jobs/updated".into());
            ws.import_job(changed).unwrap();
            let updated = AttemptRecord::start(&ws, &id, "attempt-updated", 124)
                .unwrap()
                .unwrap();
            let updated_listing: Value =
                serde_json::from_slice(&fs::read(updated.dir.join("original-job.json")).unwrap())
                    .unwrap();
            assert_eq!(updated_listing["page_capture_matches_current_url"], false);
            assert_eq!(
                updated_listing["page_capture_source_url"],
                "https://example.org/jobs/1"
            );
        }
        let events = fs::read_to_string(record.dir.join("events.jsonl")).unwrap();
        assert!(events.contains("Sign-in required"));
    }
}
