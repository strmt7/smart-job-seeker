//! Local, human-readable preparation-attempt evidence. Records are plaintext.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};
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
                                    == Some(visual_context.digest.as_str())
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
    pub reviewed_text: Option<Vec<u8>>,
}

pub struct GeneratedEvidenceLookup {
    pub found: Option<GeneratedEvidence>,
    pub incomplete: bool,
}

/// Append a recheck only to the already validated export attempt.
pub fn record_export_text_recheck(
    ws: &Workspace,
    job: &JobIdentityId,
    result: &Value,
    timestamp: i64,
) -> Result<(), String> {
    let attempt = result["attempt_id"]
        .as_str()
        .ok_or("Missing export attempt")?;
    if attempt.len() != 32 || !attempt.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid export attempt".into());
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
    let metadata = fs::symlink_metadata(&dir).map_err(|_| "Export record unavailable")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Invalid export record".into());
    }
    let mut record = AttemptRecord { dir, sequence: 0 };
    record.event(json!({"type":"export_text_rechecked","time":timestamp,"document_id":result["document_id"],"pdf_sha256":result["pdf_sha256"],"reviewed_text_sha256":result["text_sha256"],"text_check":result["text_check"],"note":"Selectable text comparison only; visual layout and factual correctness require separate review."}))
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

/// Check one explicitly selected attempt without relying on the bounded history listing.
pub fn matching_generated_evidence_for_attempt(
    ws: &Workspace,
    job: &JobIdentityId,
    document_id: &str,
    pdf: &[u8],
    attempt_id: &str,
) -> Result<GeneratedEvidenceLookup, String> {
    if attempt_id.len() != 32 || !attempt_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid export attempt".into());
    }
    let listing = json!({"attempts":[{"attempt_id":attempt_id}],"truncated":false,"unreadable":0});
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
            let reviewed_text = if receipt["reviewed_text"].is_null() {
                if !receipt["reviewed_text_sha256"].is_null() {
                    return Err("Invalid reviewed text receipt".into());
                }
                None
            } else {
                let bytes = read(
                    &materials.join(component(&receipt["reviewed_text"])?),
                    16_000,
                )?;
                if receipt["reviewed_text_sha256"].as_str()
                    != Some(waypoint_domain::sha256_hex(&bytes).as_str())
                    || std::str::from_utf8(&bytes).is_err()
                {
                    return Err("Saved reviewed text changed".into());
                }
                Some(bytes)
            };
            if evidence.is_none() && template.is_none() && reviewed_text.is_none() {
                return Err("Export receipt has no retained evidence or template".into());
            }
            Ok(Some(GeneratedEvidence {
                attempt_id: attempt.to_owned(),
                kind: kind.to_owned(),
                context_sha256: context.to_owned(),
                evidence,
                template,
                reviewed_text,
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

#[cfg(test)]
impl AttemptRecord {
    pub(crate) fn test_directory(&self) -> &std::path::Path {
        &self.dir
    }
}

// Checks presence only, not image contents or provenance.
fn listing_capture_files(dir: &std::path::Path, value: &Value, followup: bool) -> &'static str {
    let (available, count_key, folder) = if followup {
        (
            "followup_capture_available",
            "followup_capture_segment_count",
            "followup-job-page",
        )
    } else {
        (
            "page_capture_available",
            "page_capture_segment_count",
            "original-job-page",
        )
    };
    if value[available] != true {
        return "unknown";
    }
    let Some(count) = value[count_key].as_u64().filter(|n| (1..=5).contains(n)) else {
        return "unknown";
    };
    let capture = dir.join(folder);
    if !fs::symlink_metadata(&capture).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return "missing_or_inaccessible";
    }
    let present = |name: &str| {
        fs::symlink_metadata(capture.join(name))
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink() && m.len() > 0)
    };
    if present("capture.json")
        && present("view.html")
        && (1..=count).all(|i| present(&format!("{i:04}.png")))
    {
        "present"
    } else {
        "missing_or_inaccessible"
    }
}

// The manifest is unsigned. Matching means only that the present bytes match
// the present manifest; it cannot authenticate either one. Reads share a budget
// across the history response, and the manifest cannot name arbitrary paths.
fn listing_capture_integrity(
    dir: &std::path::Path,
    value: &Value,
    followup: bool,
    budget: &mut u64,
) -> &'static str {
    let (available, count_key, folder) = if followup {
        (
            "followup_capture_available",
            "followup_capture_segment_count",
            "followup-job-page",
        )
    } else {
        (
            "page_capture_available",
            "page_capture_segment_count",
            "original-job-page",
        )
    };
    if value[available] != true {
        return "unavailable";
    }
    let Some(count) = value[count_key].as_u64().filter(|n| (1..=5).contains(n)) else {
        return "unavailable";
    };
    let capture = dir.join(folder);
    if !fs::symlink_metadata(&capture).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return "unavailable";
    }
    let manifest_path = capture.join("capture-hashes.json");
    let meta = match fs::symlink_metadata(&manifest_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return "legacy",
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= 8192 => m,
        _ => return "unavailable",
    };
    if meta.len() > *budget {
        return "budget_exhausted";
    }
    let mut bytes = Vec::new();
    if fs::File::open(manifest_path)
        .and_then(|f| f.take(8193).read_to_end(&mut bytes))
        .is_err()
        || bytes.len() > 8192
    {
        return "unavailable";
    }
    *budget = budget.saturating_sub(bytes.len() as u64);
    let Ok(manifest) = serde_json::from_slice::<Value>(&bytes) else {
        return "unavailable";
    };
    let Some(files) = manifest["files"].as_object() else {
        return "unavailable";
    };
    if manifest["algorithm"] != "SHA-256" {
        return "unavailable";
    }
    let full = manifest["version"] == 2;
    let mut names: Vec<String> = std::iter::once("capture.json".to_owned())
        .chain((1..=count).map(|i| format!("{i:04}.png")))
        .collect();
    if full {
        names.push("view.html".into());
        if files.contains_key("page-text.txt") {
            names.push("page-text.txt".into());
        }
        for i in 1..=8 {
            let name = format!("structured-{i:04}.json.txt");
            if files.contains_key(&name) {
                names.push(name);
            }
        }
    } else if !manifest["version"].is_null() {
        return "unavailable";
    }
    if files.len() != names.len() {
        return "unavailable";
    }
    let mut capture_metadata = Vec::new();
    for name in names {
        let Some(expected) = files.get(&name).and_then(Value::as_str).filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        }) else {
            return "unavailable";
        };
        let is_metadata = name == "capture.json";
        let path = capture.join(name);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            return "unavailable";
        };
        if !meta.is_file() || meta.file_type().is_symlink() {
            return "unavailable";
        }
        if full && is_metadata && meta.len() > 512 * 1024 {
            return "unavailable";
        }
        if meta.len() > *budget {
            return "budget_exhausted";
        }
        let Ok(mut file) = fs::File::open(path) else {
            return "unavailable";
        };
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 32768];
        loop {
            let remaining = *budget;
            if remaining == 0 {
                return "budget_exhausted";
            }
            let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
            let Ok(n) = file.read(&mut buffer[..limit]) else {
                return "unavailable";
            };
            if n == 0 {
                break;
            }
            *budget -= n as u64;
            digest.update(&buffer[..n]);
            if full && is_metadata {
                if capture_metadata.len().saturating_add(n) > 512 * 1024 {
                    return "unavailable";
                }
                capture_metadata.extend_from_slice(&buffer[..n]);
            }
        }
        if format!("{:x}", digest.finalize()) != expected {
            return "mismatch";
        }
    }
    if !full {
        return "partial_match";
    }
    let Ok(metadata) = serde_json::from_slice::<Value>(&capture_metadata) else {
        return "unavailable";
    };
    let text_expected = matches!(
        metadata["page_text_status"].as_str(),
        Some("captured" | "truncated")
    );
    if text_expected {
        let Some(text) = metadata["page_text"].as_str().filter(|s| s.len() <= 32_000) else {
            return "unavailable";
        };
        if files.get("page-text.txt").and_then(Value::as_str)
            != Some(waypoint_domain::sha256_hex(text.as_bytes()).as_str())
        {
            return "mismatch";
        }
    } else if files.contains_key("page-text.txt") {
        return "mismatch";
    }
    let scripts = if metadata["structured_source_status"] == "captured" {
        let Some(scripts) = metadata["structured_source_scripts"]
            .as_array()
            .filter(|scripts| scripts.len() <= 8 && scripts.iter().all(Value::is_string))
        else {
            return "unavailable";
        };
        scripts.as_slice()
    } else {
        &[]
    };
    for i in 0..8 {
        let name = format!("structured-{:04}.json.txt", i + 1);
        let expected = scripts
            .get(i)
            .and_then(Value::as_str)
            .map(|text| waypoint_domain::sha256_hex(text.as_bytes()));
        if files.get(&name).and_then(Value::as_str) != expected.as_deref() {
            return "mismatch";
        }
    }
    "match"
}

// A comparison of saved workspace data, not a claim about the live vacancy or
// authentication of the plaintext attempt files.
fn listing_snapshot_comparison(
    dir: &std::path::Path,
    current: Option<&waypoint_store::StoredJob>,
    description: Option<&str>,
    budget: &mut u64,
) -> &'static str {
    let (Some(current), Some(description)) = (current, description) else {
        return "unavailable";
    };
    let path = dir.join("original-job.json");
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return "unavailable";
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 1024 * 1024
        || metadata.len() > *budget
    {
        return "unavailable";
    }
    let mut bytes = Vec::new();
    let limit = (*budget).min(1024 * 1024);
    let Ok(file) = fs::File::open(path) else {
        return "unavailable";
    };
    if file.take(limit + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > limit {
        return "unavailable";
    }
    *budget = budget.saturating_sub(bytes.len() as u64);
    let Ok(saved) = serde_json::from_slice::<Value>(&bytes) else {
        return "unavailable";
    };
    let Some(saved_description) = saved["description"].as_str() else {
        return "unavailable";
    };
    if saved["job_id"].as_str() != Some(current.id.as_str())
        || saved["description_sha256"].as_str()
            != Some(Sha256Digest::of(saved_description).as_str())
    {
        return "unavailable";
    }
    // The readable copy is part of the archive too. Do not say the listing
    // snapshot matches when this separately saved artifact is missing or changed.
    let text_path = dir.join("original-job.txt");
    let Ok(text_meta) = fs::symlink_metadata(&text_path) else {
        return "unavailable";
    };
    if !text_meta.is_file()
        || text_meta.file_type().is_symlink()
        || text_meta.len() > 1024 * 1024
        || text_meta.len() > *budget
    {
        return "unavailable";
    }
    let limit = (*budget).min(1024 * 1024);
    let Ok(text_file) = fs::File::open(text_path) else {
        return "unavailable";
    };
    let mut text_bytes = Vec::new();
    if text_file
        .take(limit + 1)
        .read_to_end(&mut text_bytes)
        .is_err()
        || text_bytes.len() as u64 > limit
    {
        return "unavailable";
    }
    *budget = budget.saturating_sub(text_bytes.len() as u64);
    if text_bytes != saved_description.as_bytes() {
        return "unavailable";
    }
    if saved["title"].as_str() == Some(current.title.as_str())
        && saved["employer"].as_str() == Some(current.employer.as_str())
        && saved["location"].as_str() == Some(current.location.as_str())
        && saved["source"].as_str() == Some(current.source.as_str())
        && saved["source_url"].as_str() == current.canonical_url.as_deref()
        && saved["requisition_id"].as_str() == current.requisition_id.as_deref()
        && saved_description == description
    {
        "matches"
    } else {
        "changed"
    }
}

fn reviewed_export_document_id(dir: &std::path::Path) -> Option<String> {
    let path = dir.join("document-export.json");
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65_536 {
        return None;
    }
    let receipt: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let document = receipt["document_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control))?;
    let text = receipt["reviewed_text"].as_str()?;
    if std::path::Path::new(text).components().count() != 1
        || !matches!(
            std::path::Path::new(text).components().next(),
            Some(std::path::Component::Normal(_))
        )
        || !["reviewed_text_sha256", "pdf_sha256"].iter().all(|key| {
            receipt[*key]
                .as_str()
                .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        })
    {
        return None;
    }
    // The recheck endpoint independently validates the receipt and file bytes.
    Some(document.to_owned())
}

/// List a bounded number of locally saved attempts without reading their materials.
pub fn list(ws: &Workspace, id: &JobIdentityId) -> Result<Value, String> {
    list_page(ws, id, 0)
}

/// Return one bounded page from the first 2,000 directory entries inspected.
pub fn list_page(ws: &Workspace, id: &JobIdentityId, page: usize) -> Result<Value, String> {
    if page >= 40 {
        return Err("Application history page exceeds the scan limit".into());
    }
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Attempt records require a disk-backed workspace")?
        .join("application-records");
    if !root.exists() {
        return Ok(
            json!({"attempts":[],"truncated":false,"unreadable":0,"page":page,"has_more":false,"scan_truncated":false}),
        );
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
        attempts.push(json!({"attempt_id":attempt_id,"status":status,"started_at":started,"folder":entry.path().to_string_lossy(),"submitted":value["submitted"],"submitted_by_waypoint":value["submitted_by_waypoint"],"material_manifest_incomplete":value["material_manifest_incomplete"],"page_capture_available":value["page_capture_available"],"page_capture_coverage":value["page_capture_coverage"],"followup_capture_available":value["followup_capture_available"],"followup_capture_files":listing_capture_files(&entry.path(), &value, true),"page_capture_files":listing_capture_files(&entry.path(), &value, false),"page_capture_segment_count":value["page_capture_segment_count"],"followup_capture_segment_count":value["followup_capture_segment_count"],"page_capture_source_url":value["page_capture_source_url"],"page_capture_matches_current_url":value["page_capture_matches_current_url"],"gmail_review":gmail_review,"email_outcome_file":has_record("email-handoff.json"),"email_reconciliation_file":has_record("email-reconciliation.json"),"export_document_id":reviewed_export_document_id(&entry.path()),"document_visual_review":fs::symlink_metadata(entry.path().join("document-visual-review.json")).is_ok_and(|m|m.is_file() && !m.file_type().is_symlink() && m.len() <= 512*1024)}));
    }
    attempts.sort_by(|a, b| {
        b["started_at"]
            .as_i64()
            .cmp(&a["started_at"].as_i64())
            .then_with(|| b["attempt_id"].as_str().cmp(&a["attempt_id"].as_str()))
    });
    let scan_truncated = truncated;
    let start = page * 50;
    let has_more = attempts.len() > start + 50;
    truncated |= start > 0 || has_more;
    let mut attempts: Vec<_> = attempts.into_iter().skip(start).take(50).collect();
    let mut integrity_budget = 32 * 1024 * 1024u64;
    let current_job = ws.job(id).ok().flatten();
    let current_description = ws.job_description(id).ok().flatten();
    for attempt in &mut attempts {
        let Some(folder) = attempt["folder"].as_str() else {
            continue;
        };
        let dir = PathBuf::from(folder);
        attempt["listing_snapshot_comparison"] = json!(listing_snapshot_comparison(
            &dir,
            current_job.as_ref(),
            current_description.as_deref(),
            &mut integrity_budget
        ));
        let original = listing_capture_integrity(&dir, attempt, false, &mut integrity_budget);
        let followup = listing_capture_integrity(&dir, attempt, true, &mut integrity_budget);
        attempt["page_capture_integrity"] = json!(original);
        attempt["followup_capture_integrity"] = json!(followup);
    }
    Ok(
        json!({"attempts":attempts,"truncated":truncated,"unreadable":unreadable,"page":page,"has_more":has_more,"scan_truncated":scan_truncated}),
    )
}

fn attempt_directory(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
) -> Result<PathBuf, String> {
    if attempt.is_empty()
        || attempt.len() > 64
        || !attempt
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid attempt identity".into());
    }
    let root = ws
        .db_path
        .as_ref()
        .and_then(|p| p.parent())
        .ok_or("Attempt records require a disk-backed workspace")?;
    let hash = Sha256Digest::of(job.as_str());
    let dir = root
        .join("application-records")
        .join(format!("{}-{attempt}", &hash.as_str()[..16]));
    let metadata = fs::symlink_metadata(&dir).map_err(|_| "Application record not found")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Invalid application record".into());
    }
    let info = dir.join("attempt.json");
    let info_meta =
        fs::symlink_metadata(&info).map_err(|_| "Application record metadata not found")?;
    if !info_meta.is_file() || info_meta.file_type().is_symlink() || info_meta.len() > 65536 {
        return Err("Invalid application record metadata".into());
    }
    let info: Value =
        serde_json::from_slice(&fs::read(info).map_err(|_| "Cannot read application record")?)
            .map_err(|_| "Invalid application record metadata")?;
    if info["job_id"].as_str() != Some(job.as_str()) || info["attempt_id"].as_str() != Some(attempt)
    {
        return Err("Application record does not match the selected job".into());
    }
    Ok(dir)
}

/// Read a bounded set of answer snapshots from the selected job's own attempt.
/// Older records may not contain the answers directory.
pub fn answer_history(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    after: u32,
) -> Result<Value, String> {
    let dir = attempt_directory(ws, job, attempt)?;
    let directory = dir.join("answers");
    let meta = match fs::symlink_metadata(&directory) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"available":false,"items":[],"truncated":false,"unreadable":0}))
        }
        Err(_) => return Err("Cannot inspect answer records".into()),
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Invalid answer record directory".into());
    }
    let mut files = Vec::new();
    let mut truncated = false;
    let mut unreadable = 0;
    for (index, entry) in fs::read_dir(directory)
        .map_err(|_| "Cannot inspect answer records")?
        .enumerate()
    {
        if index >= 2000 {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else {
            unreadable += 1;
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(number) = name
            .strip_suffix(".json")
            .filter(|s| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u32>().ok())
        else {
            unreadable += 1;
            continue;
        };
        if name != format!("{number:04}.json") {
            unreadable += 1;
            continue;
        }
        if number > after {
            files.push((number, name, entry.path()));
        }
    }
    files.sort_by_key(|(number, _, _)| *number);
    let mut items = Vec::new();
    let mut bytes_read = 0u64;
    let mut next_after = None;
    let mut last_processed = None;
    for (index, (number, name, path)) in files.into_iter().enumerate() {
        if index >= 64 {
            next_after = last_processed;
            break;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            last_processed = Some(number);
            unreadable += 1;
            continue;
        };
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 256 * 1024 {
            last_processed = Some(number);
            unreadable += 1;
            continue;
        }
        if bytes_read.saturating_add(meta.len()) > 2 * 1024 * 1024 {
            next_after = last_processed;
            break;
        }
        last_processed = Some(number);
        bytes_read += meta.len();
        let value: Value = match fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
        {
            Some(value) => value,
            None => {
                unreadable += 1;
                continue;
            }
        };
        if !matches!(
            value["type"].as_str(),
            Some(
                "fields_inspected"
                    | "fields_requested"
                    | "fill_commands_verified"
                    | "fill_incomplete"
            )
        ) {
            unreadable += 1;
            continue;
        }
        items.push(json!({"file":format!("answers/{name}"),"data":value}));
    }
    Ok(
        json!({"available":true,"items":items,"truncated":truncated,"unreadable":unreadable,"next_after":next_after}),
    )
}

fn screenshot_number(name: &str) -> Option<u32> {
    let digits = name.strip_suffix(".png")?;
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let number: u32 = digits.parse().ok()?;
    (number > 0 && name == format!("{number:04}.png")).then_some(number)
}

fn screenshot_directory(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
) -> Result<PathBuf, String> {
    let path = attempt_directory(ws, job, attempt)?.join("screenshots");
    let meta = fs::symlink_metadata(&path).map_err(|_| "Screenshot directory is unavailable")?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Invalid screenshot directory".into());
    }
    Ok(path)
}

fn screenshot_context(directory: &std::path::Path, names: &[String]) -> (Value, &'static str) {
    use std::io::Read;
    let Some(parent) = directory.parent() else {
        return (json!({}), "unavailable");
    };
    let path = parent.join("events.jsonl");
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return (json!({}), "unavailable");
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
        return (json!({}), "unavailable");
    }
    let Ok(file) = fs::File::open(path) else {
        return (json!({}), "unavailable");
    };
    const LIMIT: usize = 2 * 1024 * 1024;
    let mut bytes = Vec::new();
    if file.take(LIMIT as u64 + 1).read_to_end(&mut bytes).is_err() {
        return (json!({}), "unavailable");
    }
    let mut partial = bytes.len() > LIMIT;
    bytes.truncate(LIMIT);
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        partial = true;
    }
    let mut contexts = serde_json::Map::new();
    for (index, line) in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .enumerate()
    {
        if index >= 5000 {
            partial = true;
            break;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            partial = true;
            continue;
        };
        let Some(kind) = value["type"].as_str().filter(|kind| {
            matches!(
                *kind,
                "page_inspected"
                    | "fields_filled"
                    | "fill_failure_viewport"
                    | "attachment_viewport"
                    | "page_capture_segment"
                    | "page_capture_changed"
                    | "visual_capture"
                    | "visual_focus_capture"
                    | "control_focus_capture"
                    | "control_visual_capture"
                    | "visual_freshness_checked"
            )
        }) else {
            continue;
        };
        let Some(name) = value["screenshot"]
            .as_str()
            .and_then(|path| path.strip_prefix("screenshots/"))
        else {
            continue;
        };
        if !names.iter().any(|expected| expected == name) {
            continue;
        }
        let timestamp = value["time"]
            .as_i64()
            .filter(|time| (1..=253_402_300_799).contains(time));
        let label = value["field_label"].as_str().filter(|label| {
            !label.trim().is_empty() && label.len() <= 500 && !label.chars().any(char::is_control)
        });
        let mut context = json!({"type":kind,"time":timestamp});
        if matches!(kind, "page_capture_segment" | "page_capture_changed") {
            if let Some(id) = value["capture_id"]
                .as_str()
                .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                context["capture_id"] = json!(id);
            }
        }
        if let Some(label) = label {
            context["field_label"] = json!(label);
        }
        if kind == "visual_capture" {
            if let Some(task_id) = value["task_id"]
                .as_str()
                .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                context["visual_task_id"] = json!(task_id);
            }
            if let Some(complete) = value["coverage_complete"].as_bool() {
                context["coverage_complete"] = json!(complete);
            }
            if let Some(truncated) = value["comparisons_truncated"].as_bool() {
                context["comparisons_truncated"] = json!(truncated);
            }
            if let Some(labels) = value["omitted_labels"].as_array() {
                let accepted: Vec<_> = labels
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|label| {
                        !label.trim().is_empty()
                            && label.len() <= 500
                            && !label.chars().any(char::is_control)
                    })
                    .take(20)
                    .collect();
                context["omitted_labels_limited"] = json!(accepted.len() != labels.len());
                context["omitted_labels"] = json!(accepted);
            }
        }
        // Later events may record that a previously captured page changed.
        contexts.insert(name.to_owned(), context);
    }
    // A screenshot segment is attributable to one capture, even when a later
    // retry succeeds. A changed or interrupted capture must not look complete.
    let mut captures = std::collections::HashMap::<String, &'static str>::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .take(5000)
    {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let Some(id) = value["capture_id"]
            .as_str()
            .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        else {
            continue;
        };
        match value["type"].as_str() {
            Some("page_capture_started") => {
                captures.entry(id.to_owned()).or_insert("incomplete");
            }
            Some("page_capture_changed") => {
                captures.insert(id.to_owned(), "changed");
            }
            Some("page_capture") if captures.get(id) == Some(&"incomplete") => {
                let outcome = match value["full_dimensions_captured"].as_bool() {
                    Some(true) => "complete",
                    Some(false) => "partial",
                    None => "unknown",
                };
                captures.insert(id.to_owned(), outcome);
            }
            _ => {}
        }
    }
    for context in contexts.values_mut() {
        if let Some(id) = context["capture_id"].as_str() {
            let outcome = captures.get(id).copied().unwrap_or("unknown");
            context["capture_outcome"] = json!(if partial && outcome == "incomplete" {
                "unknown"
            } else {
                outcome
            });
        } else if context["type"] == "page_capture_segment" {
            context["capture_outcome"] = json!("unknown");
        }
    }
    // Follow-up checks refer to the original task, and often have no screenshot.
    // Retain the latest recorded outcome without treating it as a current result.
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .take(5000)
    {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let Some(task_id) = value["task_id"].as_str() else {
            continue;
        };
        let outcome = match value["type"].as_str() {
            Some("visual_page_changed") => json!({"status":"page_changed"}),
            Some("visual_freshness_checked") => {
                let Some(status) = value["status"]
                    .as_str()
                    .filter(|s| matches!(*s, "same" | "changed" | "unavailable"))
                else {
                    continue;
                };
                json!({"status":status,"live_values_verified":value["live_values_verified"].as_u64().filter(|n| *n <= 200),"live_values_unconfirmed":value["live_values_unconfirmed"].as_u64().filter(|n| *n <= 200)})
            }
            _ => continue,
        };
        for context in contexts.values_mut() {
            if context["visual_task_id"].as_str() == Some(task_id) {
                context["visual_followup"] = outcome.clone();
            }
        }
    }
    (
        Value::Object(contexts),
        if partial { "partial" } else { "available" },
    )
}

/// List a bounded page; each image is loaded only when explicitly requested.
pub fn screenshot_index(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    after: u32,
) -> Result<Value, String> {
    let directory = screenshot_directory(ws, job, attempt)?;
    let mut names = Vec::new();
    let (mut truncated, mut unreadable) = (false, 0usize);
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|_| "Cannot inspect screenshots")?
        .enumerate()
    {
        if index >= 2000 {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else {
            unreadable += 1;
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(number) = screenshot_number(&name) else {
            unreadable += 1;
            continue;
        };
        if number <= after {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            unreadable += 1;
            continue;
        };
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 6 * 1024 * 1024 {
            unreadable += 1;
            continue;
        }
        names.push((number, name));
    }
    names.sort_by_key(|(number, _)| *number);
    let next_after = if names.len() > 24 {
        Some(names[23].0)
    } else {
        None
    };
    names.truncate(24);
    let names: Vec<_> = names.into_iter().map(|(_, name)| name).collect();
    let (contexts, context_status) = screenshot_context(&directory, &names);
    Ok(
        json!({"items":names,"contexts":contexts,"context_status":context_status,"next_after":next_after,"truncated":truncated,"unreadable":unreadable}),
    )
}

pub fn screenshot_image(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    name: &str,
) -> Result<Value, String> {
    use std::io::Read;
    screenshot_number(name).ok_or("Invalid screenshot name")?;
    let path = screenshot_directory(ws, job, attempt)?.join(name);
    let meta = fs::symlink_metadata(&path).map_err(|_| "Screenshot is unavailable")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 6 * 1024 * 1024 {
        return Err("Invalid screenshot file".into());
    }
    let file = fs::File::open(path).map_err(|_| "Cannot read screenshot")?;
    let mut bytes = Vec::new();
    file.take(6 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read screenshot")?;
    if bytes.len() > 6 * 1024 * 1024 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Invalid screenshot content".into());
    }
    Ok(json!({"name":name,"image_base64":STANDARD.encode(bytes)}))
}

/// The worker records this before subsequent live-value and freshness checks.
pub fn saved_visual_result(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    task_id: &str,
) -> Result<Value, String> {
    use std::io::Read;
    if task_id.len() != 32 || !task_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid visual review identity".into());
    }
    let path = attempt_directory(ws, job, attempt)?.join(format!("visual-result-{task_id}.json"));
    let meta = fs::symlink_metadata(&path).map_err(|_| "Saved visual review is unavailable")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 512 * 1024 {
        return Err("Invalid saved visual review".into());
    }
    let file = fs::File::open(path).map_err(|_| "Cannot read saved visual review")?;
    let mut bytes = Vec::new();
    file.take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read saved visual review")?;
    if bytes.len() > 512 * 1024 {
        return Err("Saved visual review exceeds its size limit".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid saved visual review")?;
    if value["type"] != "visual_result"
        || value["task_id"].as_str() != Some(task_id)
        || !matches!(value["status"].as_str(), Some("complete" | "failed"))
    {
        return Err("Saved visual review identity does not match".into());
    }
    Ok(value)
}
/// Make a self-contained index for locally saved PNGs. Metadata supplies only
/// validated numbers; captured page text and URLs never become HTML markup.
type CaptureLayout = (f64, f64, Vec<(f64, f64, f64, f64)>);

fn listing_capture_layout(capture: &Value, count: usize) -> Option<CaptureLayout> {
    (|| {
        let width = capture["page_width"].as_f64()?;
        let height = capture["page_height"].as_f64()?;
        let rects = capture["segment_rectangles"].as_array()?;
        if !(1.0..=20_000.0).contains(&width)
            || !(1.0..=20_000.0).contains(&height)
            || count == 0
            || count > 5
            || rects.len() != count
        {
            return None;
        }
        let mut result = Vec::new();
        for rect in rects {
            let (x, y, w, h) = (
                rect["x"].as_f64()?,
                rect["y"].as_f64()?,
                rect["width"].as_f64()?,
                rect["height"].as_f64()?,
            );
            if !x.is_finite()
                || !y.is_finite()
                || !w.is_finite()
                || !h.is_finite()
                || x < 0.0
                || y < 0.0
                || w <= 0.0
                || h <= 0.0
                || w > 1600.0
                || h > 1600.0
                || x + w > width
                || y + h > height
            {
                return None;
            }
            result.push((x, y, w, h));
        }
        Some((width, height, result))
    })()
}

fn listing_capture_coverage(metadata: &[u8], count: usize) -> &'static str {
    let Ok(capture) = serde_json::from_slice::<Value>(metadata) else {
        return "unavailable";
    };
    let Some((width, height, rects)) = listing_capture_layout(&capture, count) else {
        return if capture["full_dimensions_captured"] == false {
            "partial"
        } else {
            "unavailable"
        };
    };
    let area: f64 = rects.iter().map(|(_, _, w, h)| w * h).sum();
    let overlap = rects.iter().enumerate().any(|(i, &(x, y, w, h))| {
        rects
            .iter()
            .skip(i + 1)
            .any(|&(x2, y2, w2, h2)| x < x2 + w2 && x2 < x + w && y < y2 + h2 && y2 < y + h)
    });
    if capture["full_dimensions_captured"] == true
        && !overlap
        && (area - width * height).abs() <= 0.01
    {
        "dimensions_covered"
    } else {
        "partial"
    }
}

fn listing_capture_view(metadata: &[u8], count: usize) -> String {
    let capture: Value = serde_json::from_slice(metadata).unwrap_or(Value::Null);
    let layout = listing_capture_layout(&capture, count);
    let mut html = String::from("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Saved job page</title><style>body{font:16px system-ui,sans-serif;max-width:1200px;margin:2rem auto;padding:0 1rem;color:#202a35}.page{position:relative;width:100%;background:#eee}.page img{position:absolute}.fallback img{display:block;max-width:100%;height:auto;margin:1rem 0}</style></head><body><h1>Saved job page</h1><p>This is a captured image, not a live page. Hidden content and nested scrollers may be missing.</p>");
    if let Some((width, height, rects)) = layout {
        if listing_capture_coverage(metadata, count) != "dimensions_covered" {
            html.push_str("<p>Capture is incomplete; parts of the page may be missing.</p>");
        }
        html.push_str(&format!(
            "<div class=\"page\" style=\"aspect-ratio:{width}/{height}\">"
        ));
        for (index, (x, y, w, h)) in rects.iter().enumerate() {
            html.push_str(&format!("<img src=\"{:04}.png\" alt=\"Saved page segment {}\" style=\"left:{:.6}%;top:{:.6}%;width:{:.6}%;height:{:.6}%\">", index + 1, index + 1, x / width * 100.0, y / height * 100.0, w / width * 100.0, h / height * 100.0));
        }
        html.push_str("</div>");
    } else {
        html.push_str("<p>Page layout is unavailable; up to five images are shown separately.</p><div class=\"fallback\">");
        for index in 0..count.min(5) {
            html.push_str(&format!(
                "<img src=\"{:04}.png\" alt=\"Saved page segment {}\">",
                index + 1,
                index + 1
            ));
        }
        html.push_str("</div>");
    }
    html.push_str("</body></html>");
    html
}

/// Extract the bounded page text from a source-only capture without claiming
/// that it matches the current job or that a truncated capture is complete.
fn source_only_text(metadata: Option<&[u8]>) -> (&'static str, Option<String>, Option<String>) {
    let Some(metadata) = metadata else {
        return ("unavailable", None, None);
    };
    let Ok(value) = serde_json::from_slice::<Value>(metadata) else {
        return ("invalid", None, None);
    };
    let url = value["source_url"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    match value["page_text_status"].as_str() {
        Some(status @ ("captured" | "truncated")) => match value["page_text"].as_str() {
            Some(text) if text.len() <= 32_000 => (
                if status == "captured" {
                    "captured"
                } else {
                    "truncated"
                },
                Some(text.to_owned()),
                url,
            ),
            _ => ("invalid", None, url),
        },
        _ => ("unavailable", None, url),
    }
}

/// Persist the immediate post-attachment viewport or report why it is missing.
pub fn attachment_viewport(
    record: &mut AttemptRecord,
    selector: &str,
    field_label: Option<&str>,
    image: Result<String, String>,
    timestamp: i64,
) -> Result<(), String> {
    let image = image.map_err(|_| "Post-attachment screenshot unavailable")?;
    let name = record.screenshot_with_name(&image)?;
    record.event(json!({"type":"attachment_viewport","time":timestamp,"selector":selector,"field_label":field_label,"screenshot":format!("screenshots/{name}")}))
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
        let remote_status = ws.job_remote_status(id).map_err(|e| e.to_string())?;
        let captured = ws.job_capture(id).map_err(|e| e.to_string())?;
        let followup = ws.job_capture_followup(id).map_err(|e| e.to_string())?;
        let source_only = ws.job_source_only(id).map_err(|e| e.to_string())?;
        let requested_url = |metadata: &[u8]| {
            serde_json::from_slice::<Value>(metadata)
                .ok()
                .and_then(|capture| capture["requested_url"].as_str().map(str::to_owned))
        };
        let capture_requested_url = captured
            .as_ref()
            .and_then(|(metadata, _)| requested_url(metadata));
        let source_only_requested_url = source_only.as_deref().and_then(requested_url);
        let page_capture_coverage = captured
            .as_ref()
            .map(|(metadata, segments)| listing_capture_coverage(metadata, segments.len()))
            .unwrap_or("unavailable");
        let capture_source_url = captured.as_ref().and_then(|(metadata, _)| {
            serde_json::from_slice::<Value>(metadata)
                .ok()
                .and_then(|capture| capture["source_url"].as_str().map(str::to_owned))
        });
        let (source_text_status, source_text, source_url) =
            source_only_text(source_only.as_deref());
        let source_matches_current_url = source_url
            .as_deref()
            .zip(job.canonical_url.as_deref())
            .map(|(source, current)| source == current);
        let capture_matches_current_url = capture_source_url
            .as_deref()
            .zip(job.canonical_url.as_deref())
            .map(|(captured_url, current_url)| captured_url == current_url);
        let hash = Sha256Digest::of(id.as_str());
        let records = root.join("application-records");
        fs::create_dir_all(&records).map_err(|e| e.to_string())?;
        let dir = records.join(format!("{}-{attempt_id}", &hash.as_str()[..16]));
        fs::create_dir(&dir).map_err(|e| format!("Could not create a new attempt folder: {e}"))?;
        // Establish a discoverable incomplete record before writing the rest of
        // the archive. A later failure must not look like a completed snapshot.
        let meta = json!({"attempt_id":attempt_id,"job_id":id.as_str(),"employer":job.employer,"title":job.title,"job_url":job.canonical_url,"started_at":timestamp,"status":"record_incomplete","submitted":null,"submitted_by_waypoint":false,"page_capture_available":captured.is_some(),"page_capture_coverage":page_capture_coverage,"followup_capture_available":followup.is_some(),"followup_capture_segment_count":followup.as_ref().map(|(_, segments)| segments.len()),"page_capture_segment_count":captured.as_ref().map(|(_, segments)| segments.len()),"source_only_metadata_available":source_only.is_some(),"page_capture_source_url":capture_source_url,"page_capture_matches_current_url":capture_matches_current_url,"note":"Archive initialization did not complete; files may be missing. External submission is not monitored."});
        let mut initial = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut initial, &meta).map_err(|e| e.to_string())?;
        initial.flush().map_err(|e| e.to_string())?;
        initial
            .persist_noclobber(dir.join("attempt.json"))
            .map_err(|e| e.to_string())?;
        fs::create_dir(dir.join("screenshots")).map_err(|e| e.to_string())?;
        fs::create_dir(dir.join("materials")).map_err(|e| e.to_string())?;
        fs::create_dir(dir.join("answers")).map_err(|e| e.to_string())?;
        let posting = json!({
            "job_id": id.as_str(),
            "title": job.title,
            "employer": job.employer,
            "location": job.location,
            "source": job.source,
            "source_url": job.canonical_url,
            "page_capture_requested_url": capture_requested_url,
            "source_only_requested_url": source_only_requested_url,
            "requisition_id": job.requisition_id,
            "posted_at": job.posted_at,
            "first_seen": job.first_seen,
            "last_seen": job.last_seen,
            "remote": remote_status,
            "remote_status_note": "Null means unconfirmed. A true or false indicator does not establish geographic eligibility or hybrid arrangements.",
            "snapshot_at": timestamp,
            "description_available": description.is_some(),
            "description_sha256": description.as_deref().map(|text| Sha256Digest::of(text).as_str().to_owned()),
            "description": description,
            "page_capture_available": captured.is_some(),
            "page_capture_coverage": page_capture_coverage,
            "followup_capture_available": followup.is_some(),
            "source_only_metadata_available": source_only.is_some(),
            "source_only_text_status": source_text_status,
            "source_only_source_url": source_url,
            "source_only_matches_current_url": source_matches_current_url,
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
        if let Some(text) = source_text {
            fs::write(dir.join("original-job-source-text.txt"), text).map_err(|e| e.to_string())?;
        }
        for (folder, capture) in [
            ("original-job-page", captured),
            ("followup-job-page", followup),
        ] {
            if let Some((metadata, segments)) = capture {
                let mut extra_hashes = serde_json::Map::new();
                let source_dir = dir.join(folder);
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
                        fs::write(source_dir.join("page-text.txt"), text)
                            .map_err(|e| e.to_string())?;
                        extra_hashes.insert(
                            "page-text.txt".into(),
                            json!(waypoint_domain::sha256_hex(text.as_bytes())),
                        );
                    }
                    if capture["structured_source_status"] == "captured" {
                        let scripts = capture["structured_source_scripts"]
                            .as_array()
                            .ok_or("Invalid structured source capture")?;
                        if scripts.len() > 8 || scripts.iter().any(|s| s.as_str().is_none()) {
                            return Err("Invalid structured source capture".into());
                        }
                        for (index, script) in scripts.iter().enumerate() {
                            let name = format!("structured-{:04}.json.txt", index + 1);
                            let bytes = script.as_str().unwrap().as_bytes();
                            fs::write(source_dir.join(&name), bytes).map_err(|e| e.to_string())?;
                            extra_hashes.insert(name, json!(waypoint_domain::sha256_hex(bytes)));
                        }
                    }
                }
                for (index, png) in segments.iter().enumerate() {
                    fs::write(source_dir.join(format!("{:04}.png", index + 1)), png)
                        .map_err(|e| e.to_string())?;
                }
                let view = listing_capture_view(&metadata, segments.len());
                fs::write(source_dir.join("view.html"), &view).map_err(|e| e.to_string())?;
                // These fingerprints detect accidental changes when compared later;
                // the plaintext manifest is not an authenticated signature.
                let mut hashes = extra_hashes;
                hashes.insert(
                    "view.html".into(),
                    json!(waypoint_domain::sha256_hex(view.as_bytes())),
                );
                hashes.insert(
                    "capture.json".into(),
                    json!(waypoint_domain::sha256_hex(&metadata)),
                );
                for (index, png) in segments.iter().enumerate() {
                    hashes.insert(
                        format!("{:04}.png", index + 1),
                        json!(waypoint_domain::sha256_hex(png)),
                    );
                }
                let manifest = json!({"version":2, "algorithm":"SHA-256", "files":hashes,
                    "note":"Compare these fingerprints with the saved files to detect changes. This manifest is unsigned and does not prove origin or authenticity."});
                fs::write(
                    source_dir.join("capture-hashes.json"),
                    serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if let Some(packet) = ws.packet(id).map_err(|e| e.to_string())? {
            fs::write(
                dir.join("materials").join("prepared-draft.txt"),
                packet.body,
            )
            .map_err(|e| e.to_string())?;
        }
        let mut record = Self { dir, sequence: 0 };
        record.status("preparation_in_progress", timestamp)?;
        Ok(Some(record))
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
    /// Save one reviewed fill request separately from the event log. These
    /// are intended values, not evidence of successful filling or submission.
    pub fn requested_answers(&mut self, value: &Value) -> Result<String, String> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Answer sequence exhausted")?;
        let name = format!("{:04}.json", self.sequence);
        let directory = self.dir.join("answers");
        let mut file = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut file, value).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        file.persist_noclobber(directory.join(&name))
            .map_err(|e| e.to_string())?;
        Ok(format!("answers/{name}"))
    }
    /// Record the observed questions without recording values from the page.
    pub fn inspected_fields(
        &mut self,
        origin: &str,
        fields: &[waypoint_applications::form_mapping::FormField],
        timestamp: i64,
    ) -> Result<(), String> {
        let mut kept = Vec::new();
        let mut bytes = 0usize;
        for field in fields.iter().take(200) {
            let value = json!({"selector":field.selector,"question":field.label,"field_type":field.field_type,"required":field.required,"sensitive":field.sensitive});
            let size = serde_json::to_vec(&value)
                .map_err(|_| "Cannot encode inspected question")?
                .len();
            if bytes.saturating_add(size) > 220 * 1024 {
                break;
            }
            bytes += size;
            kept.push(value);
        }
        let omitted = fields.len().saturating_sub(kept.len());
        let value = json!({"type":"fields_inspected","time":timestamp,"origin":origin,"fields":kept,"omitted":omitted,"note":"Observed questions only; existing field values are not recorded."});
        let path = self.requested_answers(&value)?;
        self.event(json!({"type":"fields_inspected","time":timestamp,"answer_file":path,"omitted":omitted}))
    }
    /// Preserve a separate outcome alongside the event that points to it.
    pub fn answer_outcome(&mut self, mut value: Value) -> Result<(), String> {
        let path = self.requested_answers(&value)?;
        value["outcome_file"] = json!(path);
        self.event(value)
    }
    /// Record an interrupted fill and independently mark the attempt for review.
    pub fn incomplete_fill(&mut self, outcome: Value, timestamp: i64) -> Result<(), String> {
        let outcome_error = self.answer_outcome(outcome.clone()).err();
        if outcome_error.is_some() {
            let _ = self.event(outcome);
        }
        let status_error = self.status("manual_review", timestamp).err();
        match (outcome_error, status_error) {
            (None, None) => Ok(()),
            (Some(error), None) => Err(format!("Answer outcome could not be saved: {error}")),
            (None, Some(error)) => Err(format!("Attempt review status could not be saved: {error}")),
            (Some(outcome), Some(status)) => Err(format!(
                "Answer outcome could not be saved: {outcome}; attempt review status could not be saved: {status}"
            )),
        }
    }

    /// Record a failure viewport or an explicit gap in the evidence. The
    /// boolean indicates an incomplete capture; an error means even that gap
    /// could not be recorded.
    pub fn failure_viewport(
        &mut self,
        image: Result<String, String>,
        timestamp: i64,
    ) -> Result<bool, String> {
        let saved = image.and_then(|image| self.screenshot_with_name(&image)).and_then(|name| {
            self.event(json!({"type":"fill_failure_viewport","time":timestamp,"screenshot":format!("screenshots/{name}")}))
        });
        if saved.is_err() {
            self.event(json!({"type":"capture_incomplete","time":timestamp,"note":"The failure viewport could not be captured or saved."}))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn status(&mut self, status: &str, timestamp: i64) -> Result<(), String> {
        self.update_status(status, timestamp, false)
    }

    fn update_status(
        &mut self,
        status: &str,
        timestamp: i64,
        material_manifest_incomplete: bool,
    ) -> Result<(), String> {
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
        if meta["status"] == "preparation_in_progress" {
            meta["note"] = json!("External submission is not monitored; this record does not prove whether an application was submitted.");
        }
        if material_manifest_incomplete {
            meta["material_manifest_incomplete"] = json!(true);
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

    pub fn source_application_route(
        &mut self,
        source: &str,
        route: &str,
        timestamp: i64,
    ) -> Result<(), String> {
        let parsed = url::Url::parse(route).map_err(|_| "Invalid source application route")?;
        if source.is_empty()
            || source.len() > 80
            || source.chars().any(char::is_control)
            || route.len() > 2048
            || parsed.scheme() != "https"
            || !waypoint_connectors::is_public_host_url(route)
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("Invalid source application route".into());
        }
        self.event(json!({"type":"source_application_route","time":timestamp,"source":source,"url":route,"note":"Source-reported starting route; this does not establish a usable form or submission."}))
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
        const MAX_SCREENSHOT_BYTES: usize = 6 * 1024 * 1024;
        // Reject oversized input before base64 decoding allocates another buffer.
        // The decoded-size check remains necessary for padded boundary values.
        if data.len() > MAX_SCREENSHOT_BYTES.div_ceil(3) * 4 {
            return Err("Screenshot exceeds limit".into());
        }
        let bytes = STANDARD.decode(data).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_SCREENSHOT_BYTES {
            return Err("Screenshot exceeds limit".into());
        }
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err("Invalid screenshot content".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Screenshot sequence exhausted")?;
        let name = format!("{:04}.png", self.sequence);
        let directory = self.dir.join("screenshots");
        let mut file = tempfile::NamedTempFile::new_in(&directory).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        file.persist_noclobber(directory.join(&name))
            .map_err(|e| e.to_string())?;
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
        let manifest_result = (|| -> Result<(), String> {
            let mut manifest = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(materials.join("manifest.jsonl"))
                .map_err(|e| {
                    format!("Material was saved but its manifest could not be opened: {e}")
                })?;
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
            Ok(())
        })();
        if let Err(error) = manifest_result {
            // The material exists but its manifest entry may be missing or partial.
            // Keep it for recovery and independently mark the attempt for review.
            let timestamp = crate::now();
            let event_error = self.event(json!({"type":"material_manifest_incomplete","time":timestamp,"stored_name":filename,"note":"Material was saved but its manifest entry could not be completed; inspect the archive."})).err();
            let status_error = self.update_status("manual_review", timestamp, true).err();
            let mut details = error;
            if let Some(error) = event_error {
                details.push_str(&format!("; archive event could not be saved: {error}"));
            }
            if let Some(error) = status_error {
                details.push_str(&format!(
                    "; attempt review status could not be saved: {error}"
                ));
            }
            return Err(details);
        }
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
    fields: &[waypoint_applications::form_mapping::FormField],
    screenshot: Option<&str>,
    timestamp: i64,
) -> Result<(), String> {
    record_inspection_evidence(record, origin, reasons, fields, screenshot, timestamp)?;
    if let Err(error) = capture_page(driver, record, timestamp) {
        return Err(record_capture_failure(record, error, timestamp));
    }
    Ok(())
}

fn record_inspection_evidence(
    record: &mut AttemptRecord,
    origin: &str,
    reasons: &[String],
    fields: &[waypoint_applications::form_mapping::FormField],
    screenshot: Option<&str>,
    timestamp: i64,
) -> Result<(), String> {
    // Preserve either artifact even if writing the other one fails.
    let questions = record.inspected_fields(origin, fields, timestamp);
    let page = record_initial_inspection(record, origin, reasons, screenshot, timestamp);
    match (questions, page) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(record_capture_failure(record, error, timestamp)),
        (Ok(()), Err(error)) => Err(error),
        (Err(questions), Err(page)) => Err(record_capture_failure(
            record,
            format!("Questions could not be saved: {questions}; page capture failed: {page}"),
            timestamp,
        )),
    }
}

fn record_initial_inspection(
    record: &mut AttemptRecord,
    origin: &str,
    reasons: &[String],
    screenshot: Option<&str>,
    timestamp: i64,
) -> Result<(), String> {
    if let Err(error) = record.inspection(origin, reasons, screenshot, timestamp) {
        // Persist the incomplete state even when the first screenshot or event
        // cannot be recorded. The original error remains visible to the caller.
        return Err(record_capture_failure(record, error, timestamp));
    }
    Ok(())
}

fn record_capture_failure(record: &mut AttemptRecord, error: String, timestamp: i64) -> String {
    let event_error = record
        .event(json!({"type":"capture_incomplete","time":timestamp,"error":error}))
        .err();
    let status_error = record.status("manual_review", timestamp).err();
    let mut message = error;
    if let Some(problem) = event_error {
        message.push_str(&format!("; could not record failure: {problem}"));
    }
    if let Some(problem) = status_error {
        message.push_str(&format!(
            "; could not mark attempt for manual review: {problem}"
        ));
    }
    message
}

/// Map known capture failures to bounded UI guidance without exposing raw
/// browser errors, which can contain site-provided or local data.
pub fn capture_failure_guidance(error: &str) -> Option<&'static str> {
    if error.starts_with("Page image changed during capture") {
        Some("The page image changed during capture. A blinking cursor, animation or a real page change may be responsible; inspect the page and capture again.")
    } else if error.starts_with("Page dimensions changed during both captures") {
        Some("The page dimensions changed during capture. Inspect the page and capture again.")
    } else if error.starts_with("Page address changed during capture")
        || error.starts_with("Page address changed before capture")
    {
        Some("The page address changed during capture. Inspect the current destination and capture again.")
    } else if error == "Page exceeded screenshot capture limits" {
        Some("The page exceeded screenshot capture limits. Some content was not captured and needs manual review.")
    } else {
        None
    }
}

/// Build at most six bounded tiles, reporting when dimensions exceed the budget.
fn capture_tiles(width: f64, height: f64) -> (Vec<(f64, f64, f64, f64)>, bool) {
    let capped_width = width.min(4096.0);
    let capped_height = height.min(12000.0);
    let mut tiles = Vec::new();
    let mut y = 0.0;
    while y < capped_height {
        // Keep the final tile full-height when possible. It overlaps the
        // previous tile but still records its actual page coordinates.
        let capture_y = if y > 0.0 && capped_height - y < 2000.0 {
            capped_height - 2000.0
        } else {
            y
        };
        let tile_height = (capped_height - capture_y).min(2000.0);
        let mut x = 0.0;
        while x < capped_width {
            if tiles.len() == 6 {
                return (tiles, false);
            }
            let tile_width = (capped_width - x).min(2048.0);
            tiles.push((x, capture_y, tile_width, tile_height));
            x += tile_width;
        }
        y += tile_height;
    }
    (tiles, width <= 4096.0 && height <= 12000.0)
}

fn capture_page_unchanged(
    initial_url: &str,
    initial_dimensions: (f64, f64),
    current_url: &str,
    current_dimensions: (f64, f64),
) -> bool {
    initial_url == current_url && initial_dimensions == current_dimensions
}

#[derive(Debug, PartialEq, Eq)]
enum CaptureChange {
    Address,
    Dimensions,
    FirstTile,
}

fn final_capture_change(
    initial_url: &str,
    initial_dimensions: (f64, f64),
    current_url: &str,
    current_dimensions: (f64, f64),
    first_tile_changed: bool,
) -> Option<CaptureChange> {
    if initial_url != current_url {
        Some(CaptureChange::Address)
    } else if initial_dimensions != current_dimensions {
        Some(CaptureChange::Dimensions)
    } else if first_tile_changed {
        Some(CaptureChange::FirstTile)
    } else {
        None
    }
}

enum CaptureError {
    DimensionsChanged,
    FirstTileChanged,
    Other(String),
}
impl From<String> for CaptureError {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}
impl From<&str> for CaptureError {
    fn from(error: &str) -> Self {
        Self::Other(error.to_owned())
    }
}

/// Save at most six segments per capture. A dimension or first-tile change
/// gets one fresh capture; both attempts remain in the archive.
pub fn capture_page(
    driver: &mut waypoint_browser::CdpDriver,
    record: &mut AttemptRecord,
    timestamp: i64,
) -> Result<(), String> {
    // Chromium's first beyond-viewport capture can change its reported content
    // width while it initializes scrollbar/layout state. Warm the same bounded
    // capture path before planning tiles, without treating the image as evidence.
    let before = driver.current_url().map_err(|error| error.to_string())?;
    let (width, height) = driver
        .page_dimensions()
        .map_err(|error| error.to_string())?;
    if let Some((x, y, tile_width, tile_height)) = capture_tiles(width, height).0.first().copied() {
        driver
            .screenshot_tile(x, y, tile_width, tile_height)
            .map_err(|error| error.to_string())?;
    }
    if driver.current_url().map_err(|error| error.to_string())? != before {
        return Err("Page address changed before capture; inspect and capture again".into());
    }
    for attempt in 0..2 {
        match capture_page_once(driver, record, timestamp) {
            Ok(()) => return Ok(()),
            Err(CaptureError::DimensionsChanged) if attempt == 0 => {
                record.event(json!({"type":"page_capture_retry","time":timestamp,"note":"Page dimensions changed; starting one new capture."}))?;
            }
            Err(CaptureError::DimensionsChanged) => {
                return Err(
                    "Page dimensions changed during both captures; inspect and capture again"
                        .into(),
                )
            }
            Err(CaptureError::FirstTileChanged) if attempt == 0 => {
                record.event(json!({"type":"page_capture_retry","time":timestamp,"note":"Page image changed; starting one new capture. The earlier capture may show a different state."}))?;
            }
            Err(CaptureError::FirstTileChanged) => {
                return Err("Page image changed during capture; this may be a blinking cursor, animation, or a real page change. Inspect the page and capture again".into());
            }
            Err(CaptureError::Other(error)) => return Err(error),
        }
    }
    unreachable!("the retry loop always returns")
}

fn capture_page_once(
    driver: &mut waypoint_browser::CdpDriver,
    record: &mut AttemptRecord,
    timestamp: i64,
) -> Result<(), CaptureError> {
    let initial_url = driver.current_url().map_err(|e| e.to_string())?;
    let (width, height) = driver.page_dimensions().map_err(|e| e.to_string())?;
    let (tiles, within_limits) = capture_tiles(width, height);
    let capture_id = crate::random_id()?;
    record.event(json!({"type":"page_capture_started","time":timestamp,"capture_id":capture_id,"page_width":width,"page_height":height}))?;
    let mut first_image_hash = None;
    for (count, (x, y, tile_width, tile_height)) in tiles.iter().copied().enumerate() {
        let image = driver
            .screenshot_tile(x, y, tile_width, tile_height)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            first_image_hash = Some(waypoint_domain::Sha256Digest::of(&image));
        }
        let name = record.screenshot_with_name(&image)?;
        record.event(json!({"type":"page_capture_segment","time":timestamp,"capture_id":capture_id,"index":count,"x":x,"y":y,"width":tile_width,"height":tile_height,"screenshot":format!("screenshots/{name}")}))?;
        let current_url = driver.current_url().map_err(|e| e.to_string())?;
        let dimensions = driver.page_dimensions().map_err(|e| e.to_string())?;
        if !capture_page_unchanged(&initial_url, (width, height), &current_url, dimensions) {
            record.event(json!({"type":"page_capture_changed","time":timestamp,"capture_id":capture_id,"after_segment":count,"initial_dimensions":[width,height],"observed_dimensions":[dimensions.0,dimensions.1],"address_changed":current_url != initial_url,"note":"Page address or dimensions changed while capturing; screenshots may show different states."}))?;
            return if current_url == initial_url {
                Err(CaptureError::DimensionsChanged)
            } else {
                Err("Page address changed during capture; inspect and capture again".into())
            };
        }
    }
    if let (Some((x, y, tile_width, tile_height)), Some(original)) =
        (tiles.first().copied(), first_image_hash)
    {
        let image = driver
            .screenshot_tile(x, y, tile_width, tile_height)
            .map_err(|e| e.to_string())?;
        let current_url = driver.current_url().map_err(|e| e.to_string())?;
        let dimensions = driver.page_dimensions().map_err(|e| e.to_string())?;
        let first_tile_changed = original != waypoint_domain::Sha256Digest::of(&image);
        if let Some(change) = final_capture_change(
            &initial_url,
            (width, height),
            &current_url,
            dimensions,
            first_tile_changed,
        ) {
            let name = record.screenshot_with_name(&image)?;
            record.event(json!({"type":"page_capture_changed","time":timestamp,"capture_id":capture_id,"screenshot":format!("screenshots/{name}"),"address_changed":current_url != initial_url,"initial_dimensions":[width,height],"observed_dimensions":[dimensions.0,dimensions.1],"first_tile_changed":first_tile_changed,"note":"The first tile, page address or dimensions changed during capture; screenshots may show different states."}))?;
            return match change {
                CaptureChange::Dimensions => Err(CaptureError::DimensionsChanged),
                CaptureChange::Address => {
                    Err("Page address changed during capture; inspect and capture again".into())
                }
                CaptureChange::FirstTile => Err(CaptureError::FirstTileChanged),
            };
        }
    }
    record.event(json!({"type":"page_capture","time":timestamp,"capture_id":capture_id,"page_width":width,"page_height":height,"segments":tiles.len(),"full_dimensions_captured":within_limits,"note":"Address and dimensions stayed the same and the first tile matched a later capture. Other content changes, hidden content and nested scrollers may still be missed."}))?;
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
    #[test]
    fn export_recheck_hint_requires_reviewed_text_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("document-export.json");
        let mut receipt = serde_json::json!({"document_id":"doc", "reviewed_text":"reviewed.txt", "reviewed_text_sha256":"a".repeat(64), "pdf_sha256":"b".repeat(64)});
        let write = |receipt: &serde_json::Value| {
            std::fs::write(&path, serde_json::to_vec(receipt).unwrap()).unwrap()
        };
        write(&receipt);
        assert_eq!(
            super::reviewed_export_document_id(dir.path()).as_deref(),
            Some("doc")
        );
        receipt.as_object_mut().unwrap().remove("reviewed_text");
        write(&receipt);
        assert!(super::reviewed_export_document_id(dir.path()).is_none());
        receipt["reviewed_text"] = "../outside".into();
        write(&receipt);
        assert!(super::reviewed_export_document_id(dir.path()).is_none());
        receipt["reviewed_text"] = "reviewed.txt".into();
        receipt["pdf_sha256"] = "not a digest".into();
        write(&receipt);
        assert!(super::reviewed_export_document_id(dir.path()).is_none());
    }
    #[test]
    fn capture_failure_guidance_is_specific_and_does_not_expose_raw_errors() {
        assert!(
            super::capture_failure_guidance("Page image changed during capture; fixture")
                .unwrap()
                .contains("blinking cursor")
        );
        assert!(super::capture_failure_guidance(
            "Page dimensions changed during both captures; fixture"
        )
        .unwrap()
        .contains("dimensions"));
        assert!(
            super::capture_failure_guidance("Page address changed during capture; fixture")
                .unwrap()
                .contains("destination")
        );
        assert!(
            super::capture_failure_guidance("Page address changed before capture; fixture")
                .unwrap()
                .contains("destination")
        );
        assert!(
            super::capture_failure_guidance("Page exceeded screenshot capture limits")
                .unwrap()
                .contains("not captured")
        );
        assert_eq!(
            super::capture_failure_guidance("arbitrary site or local data"),
            None
        );
    }
    use super::*;
    #[test]
    fn page_capture_detects_navigation_and_dimension_changes() {
        let baseline = (1200.0, 3000.0);
        assert!(capture_page_unchanged(
            "https://example.invalid/a",
            baseline,
            "https://example.invalid/a",
            baseline
        ));
        assert!(!capture_page_unchanged(
            "https://example.invalid/a",
            baseline,
            "https://example.invalid/b",
            baseline
        ));
        assert_eq!(
            final_capture_change("a", baseline, "a", baseline, false),
            None
        );
        assert_eq!(
            final_capture_change("a", baseline, "a", baseline, true),
            Some(CaptureChange::FirstTile)
        );
        assert_eq!(
            final_capture_change("a", baseline, "a", (1200.0, 3001.0), true),
            Some(CaptureChange::Dimensions)
        );
        assert_eq!(
            final_capture_change("a", baseline, "b", (1200.0, 3001.0), true),
            Some(CaptureChange::Address)
        );
        assert!(!capture_page_unchanged(
            "https://example.invalid/a",
            baseline,
            "https://example.invalid/a",
            (1200.0, 3001.0)
        ));
    }

    #[test]
    fn incomplete_fill_marks_review_even_if_outcome_cannot_be_saved() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let directory = tempfile::tempdir_in(base).unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let added = crate::manual_job::add(&mut ws, &json!({"url":"https://example.invalid/interrupted","title":"Role","employer":"Fixture"}), 1).unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let mut record = AttemptRecord::start(&ws, &job, "interrupted-fixture", 2)
            .unwrap()
            .unwrap();
        fs::rename(record.dir.join("answers"), record.dir.join("old-answers")).unwrap();
        let error = record
            .incomplete_fill(json!({"type":"fill_incomplete","completed_commands":[]}), 3)
            .unwrap_err();
        assert!(error.contains("Answer outcome could not be saved"));
        let meta: Value =
            serde_json::from_slice(&fs::read(record.dir.join("attempt.json")).unwrap()).unwrap();
        assert_eq!(meta["status"], "manual_review");
        assert!(fs::read_to_string(record.dir.join("events.jsonl"))
            .unwrap()
            .contains("fill_incomplete"));
    }

    #[test]
    fn failure_viewport_reports_missing_screenshot_directory() {
        let directory = tempfile::tempdir().unwrap();
        let mut record = AttemptRecord {
            dir: directory.path().to_owned(),
            sequence: 0,
        };
        let image = STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture");
        assert!(record.failure_viewport(Ok(image.clone()), 3).unwrap());
        let events = fs::read_to_string(directory.path().join("events.jsonl")).unwrap();
        assert!(events.contains("capture_incomplete"));
        assert!(!events.contains("fill_failure_viewport"));
        fs::create_dir(directory.path().join("screenshots")).unwrap();
        assert!(!record.failure_viewport(Ok(image), 4).unwrap());
        let events = fs::read_to_string(directory.path().join("events.jsonl")).unwrap();
        assert!(events.contains("fill_failure_viewport"));
        // If even the gap cannot be logged, the caller must surface that error.
        fs::remove_file(directory.path().join("events.jsonl")).unwrap();
        fs::create_dir(directory.path().join("events.jsonl")).unwrap();
        assert!(record
            .failure_viewport(Err("fixture failure".into()), 5)
            .is_err());
    }

    #[test]
    fn source_application_route_record_omits_tokenized_or_invalid_urls() {
        let root = tempfile::tempdir().unwrap();
        let mut record = AttemptRecord {
            dir: root.path().to_owned(),
            sequence: 0,
        };
        let route = "https://jobs.ashbyhq.com/example/fixture/application";
        record
            .source_application_route("ashby", route, 123)
            .unwrap();
        for invalid in [
            "https://jobs.ashbyhq.com/example?token=secret",
            "http://jobs.ashbyhq.com/example",
            "https://user@jobs.ashbyhq.com/example",
        ] {
            assert!(record
                .source_application_route("ashby", invalid, 124)
                .is_err());
        }
        let events = fs::read_to_string(root.path().join("events.jsonl")).unwrap();
        let rows: Vec<Value> = events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["type"], "source_application_route");
        assert_eq!(rows[0]["url"], route);
        assert!(!events.contains("secret"));
    }

    #[test]
    fn answer_history_is_bound_to_attempt_and_reports_incomplete_files() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let directory = tempfile::tempdir_in(base).unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let added = crate::manual_job::add(&mut ws, &json!({"url":"https://example.invalid/fictional-answer-history","title":"Role","employer":"Fixture"}), 1).unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let mut record = AttemptRecord::start(&ws, &job, "answer-fixture", 2)
            .unwrap()
            .unwrap();
        let field: waypoint_applications::form_mapping::FormField = serde_json::from_value(json!({"selector":"#name","label":"Name","field_type":"text","required":true,"sensitive":false})).unwrap();
        record
            .inspected_fields("https://example.invalid", &vec![field; 201], 3)
            .unwrap();
        let requested = json!({"type":"fields_requested","fields":[{"question":"Name","value":{"text":"Fictional Candidate"}}]});
        let path = record.requested_answers(&requested).unwrap();
        record
            .incomplete_fill(
                json!({"type":"fill_incomplete","answer_file":path,"completed_commands":[]}),
                4,
            )
            .unwrap();
        let meta: Value =
            serde_json::from_slice(&fs::read(record.dir.join("attempt.json")).unwrap()).unwrap();
        assert_eq!(meta["status"], "manual_review");
        fs::write(record.dir.join("answers").join("9999.json"), b"invalid").unwrap();
        let history = answer_history(&ws, &job, "answer-fixture", 0).unwrap();
        assert_eq!(history["available"], true);
        assert_eq!(history["items"].as_array().unwrap().len(), 3);
        assert_eq!(history["items"][0]["data"]["type"], "fields_inspected");
        assert_eq!(
            history["items"][0]["data"]["origin"],
            "https://example.invalid"
        );
        assert_eq!(
            history["items"][0]["data"]["fields"]
                .as_array()
                .unwrap()
                .len(),
            200
        );
        assert_eq!(history["items"][0]["data"]["omitted"], 1);
        assert!(!history["items"][0]["data"]
            .to_string()
            .contains("Fictional Candidate"));
        assert_eq!(history["items"][1]["data"], requested);
        assert_eq!(
            history["items"][2]["data"]["answer_file"],
            history["items"][1]["file"]
        );
        assert_eq!(history["unreadable"], 1);
        assert!(answer_history(&ws, &job, "../invalid", 0).is_err());
        assert!(answer_history(
            &ws,
            &JobIdentityId::new("different-job").unwrap(),
            "answer-fixture",
            0
        )
        .is_err());
        fs::rename(record.dir.join("answers"), record.dir.join("old-answers")).unwrap();
        assert_eq!(
            answer_history(&ws, &job, "answer-fixture", 0).unwrap()["available"],
            false
        );
    }
    #[test]
    fn screenshot_viewer_is_paged_bounded_and_bound_to_the_attempt() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let directory = tempfile::tempdir_in(base).unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let added = crate::manual_job::add(&mut ws, &json!({"url":"https://example.invalid/screenshots","title":"Role","employer":"Fixture"}), 1).unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let mut record = AttemptRecord::start(&ws, &job, "screens-fixture", 2)
            .unwrap()
            .unwrap();
        for index in 1..=25 {
            fs::write(
                record
                    .dir
                    .join("screenshots")
                    .join(format!("{index:04}.png")),
                b"\x89PNG\r\n\x1a\nfixture",
            )
            .unwrap();
        }
        fs::write(record.dir.join("screenshots/not-a-capture.txt"), b"ignored").unwrap();
        fs::File::create(record.dir.join("screenshots/9999.png"))
            .unwrap()
            .set_len(6 * 1024 * 1024 + 1)
            .unwrap();
        record
            .event(json!({"type":"fields_filled","time":123,"screenshot":"screenshots/0001.png"}))
            .unwrap();
        record.event(json!({"type":"page_capture_segment","time":124,"screenshot":"screenshots/0002.png"})).unwrap();
        record.event(json!({"type":"page_capture_changed","time":125,"screenshot":"screenshots/0002.png"})).unwrap();
        record.event(json!({"type":"attachment_viewport","time":126,"field_label":"Resume","screenshot":"screenshots/0025.png"})).unwrap();
        let failed_capture = "11111111111111111111111111111111";
        let good_capture = "22222222222222222222222222222222";
        let pending_capture = "33333333333333333333333333333333";
        record
            .event(json!({"type":"page_capture_started","capture_id":failed_capture}))
            .unwrap();
        record.event(json!({"type":"page_capture_segment","capture_id":failed_capture,"screenshot":"screenshots/0004.png"})).unwrap();
        record
            .event(json!({"type":"page_capture_changed","capture_id":failed_capture}))
            .unwrap();
        record
            .event(json!({"type":"page_capture_started","capture_id":good_capture}))
            .unwrap();
        record.event(json!({"type":"page_capture_segment","capture_id":good_capture,"screenshot":"screenshots/0005.png"})).unwrap();
        record
            .event(json!({"type":"page_capture","capture_id":good_capture,"full_dimensions_captured":true}))
            .unwrap();
        record
            .event(json!({"type":"page_capture_started","capture_id":pending_capture}))
            .unwrap();
        record.event(json!({"type":"page_capture_segment","capture_id":pending_capture,"screenshot":"screenshots/0006.png"})).unwrap();
        let partial_capture = "44444444444444444444444444444444";
        record
            .event(json!({"type":"page_capture_started","capture_id":partial_capture}))
            .unwrap();
        record.event(json!({"type":"page_capture_segment","capture_id":partial_capture,"screenshot":"screenshots/0007.png"})).unwrap();
        record.event(json!({"type":"page_capture","capture_id":partial_capture,"full_dimensions_captured":false})).unwrap();
        record.event(json!({"type":"page_capture","capture_id":failed_capture,"full_dimensions_captured":true})).unwrap();
        let visual_id = "0123456789abcdef0123456789abcdef";
        let omitted_labels: Vec<_> = (0..22).map(|n| format!("Field {n}")).collect();
        record.event(json!({"type":"visual_capture","time":127,"task_id":visual_id,"screenshot":"screenshots/0003.png","coverage_complete":false,"comparisons_truncated":true,"omitted_labels":omitted_labels})).unwrap();
        record.event(json!({"type":"visual_freshness_checked","task_id":visual_id,"status":"same","live_values_verified":1,"live_values_unconfirmed":0,"screenshot":null})).unwrap();
        record
            .event(json!({"type":"visual_page_changed","task_id":"different-task"}))
            .unwrap();
        write_visual_result(
            &record.visual_result_path(visual_id).unwrap(),
            visual_id,
            &Ok(json!({"observations":[],"comparisons":[]})),
            128,
        )
        .unwrap();
        let first = screenshot_index(&ws, &job, "screens-fixture", 0).unwrap();
        assert_eq!(first["contexts"]["0004.png"]["capture_outcome"], "changed");
        assert_eq!(first["contexts"]["0005.png"]["capture_outcome"], "complete");
        assert_eq!(
            first["contexts"]["0006.png"]["capture_outcome"],
            "incomplete"
        );
        assert_eq!(first["contexts"]["0007.png"]["capture_outcome"], "partial");
        assert_eq!(first["contexts"]["0003.png"]["visual_task_id"], visual_id);
        assert_eq!(first["contexts"]["0003.png"]["coverage_complete"], false);
        assert_eq!(first["contexts"]["0003.png"]["comparisons_truncated"], true);
        assert_eq!(
            first["contexts"]["0003.png"]["visual_followup"],
            json!({"status":"same","live_values_verified":1,"live_values_unconfirmed":0})
        );
        record
            .event(json!({"type":"visual_page_changed","task_id":visual_id}))
            .unwrap();
        assert_eq!(
            screenshot_index(&ws, &job, "screens-fixture", 0).unwrap()["contexts"]["0003.png"]
                ["visual_followup"]["status"],
            "page_changed"
        );
        assert_eq!(
            first["contexts"]["0003.png"]["omitted_labels"]
                .as_array()
                .unwrap()
                .len(),
            20
        );
        assert_eq!(
            first["contexts"]["0003.png"]["omitted_labels"][0],
            "Field 0"
        );
        assert_eq!(
            first["contexts"]["0003.png"]["omitted_labels_limited"],
            true
        );
        assert_eq!(
            saved_visual_result(&ws, &job, "screens-fixture", visual_id).unwrap()["status"],
            "complete"
        );
        assert!(saved_visual_result(&ws, &job, "screens-fixture", "../invalid").is_err());
        assert!(saved_visual_result(
            &ws,
            &JobIdentityId::new("different-job").unwrap(),
            "screens-fixture",
            visual_id
        )
        .is_err());
        assert_eq!(first["items"].as_array().unwrap().len(), 24);
        assert_eq!(first["next_after"], 24);
        assert_eq!(first["unreadable"], 2);
        assert_eq!(first["context_status"], "available");
        assert_eq!(
            first["contexts"]["0001.png"],
            json!({"type":"fields_filled","time":123})
        );
        assert_eq!(
            first["contexts"]["0002.png"]["type"],
            "page_capture_changed"
        );
        assert!(first["contexts"]["0025.png"].is_null());
        let second = screenshot_index(&ws, &job, "screens-fixture", 24).unwrap();
        assert_eq!(second["items"], json!(["0025.png"]));
        assert!(second["next_after"].is_null());
        assert_eq!(
            second["contexts"]["0025.png"]["type"],
            "attachment_viewport"
        );
        assert_eq!(second["contexts"]["0025.png"]["field_label"], "Resume");
        let image = screenshot_image(&ws, &job, "screens-fixture", "0001.png").unwrap();
        assert_eq!(
            STANDARD
                .decode(image["image_base64"].as_str().unwrap())
                .unwrap(),
            b"\x89PNG\r\n\x1a\nfixture"
        );
        {
            use std::io::{Read, Write};
            use std::net::{TcpListener, TcpStream};
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let body = json!({"id":job.as_str(),"attempt_id":"screens-fixture","name":"0001.png"})
                .to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client, "POST /api/attempts/screenshot HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            let body: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body, image);
        }
        assert!(screenshot_image(&ws, &job, "screens-fixture", "../attempt.json").is_err());
        assert!(screenshot_image(&ws, &job, "screens-fixture", "9999.png").is_err());
        assert!(screenshot_image(
            &ws,
            &JobIdentityId::new("different-job").unwrap(),
            "screens-fixture",
            "0001.png"
        )
        .is_err());
        fs::write(record.dir.join("screenshots/0001.png"), b"not a PNG").unwrap();
        assert!(screenshot_image(&ws, &job, "screens-fixture", "0001.png").is_err());
        let file = fs::OpenOptions::new()
            .write(true)
            .open(record.dir.join("events.jsonl"))
            .unwrap();
        file.set_len(2 * 1024 * 1024 + 1).unwrap();
        assert_eq!(
            screenshot_index(&ws, &job, "screens-fixture", 0).unwrap()["context_status"],
            "partial"
        );
    }
    #[test]
    fn answer_history_pages_in_order_across_unreadable_and_large_records() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added = crate::manual_job::add(
            &mut ws,
            &json!({"url":"https://example.invalid/paged","title":"Role","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let mut record = AttemptRecord::start(&ws, &job, "paged-fixture", 2)
            .unwrap()
            .unwrap();
        for n in 1..=130 {
            record
                .requested_answers(&json!({"type":"fields_requested","number":n}))
                .unwrap();
        }
        fs::write(record.dir.join("answers/0010.json"), b"invalid").unwrap();
        let first = answer_history(&ws, &job, "paged-fixture", 0).unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 63);
        assert_eq!(first["unreadable"], 1);
        assert_eq!(first["next_after"], 64);
        let second = answer_history(&ws, &job, "paged-fixture", 64).unwrap();
        assert_eq!(second["items"][0]["data"]["number"], 65);
        assert_eq!(second["next_after"], 128);
        let third = answer_history(&ws, &job, "paged-fixture", 128).unwrap();
        assert_eq!(third["items"].as_array().unwrap().len(), 2);
        assert!(third["next_after"].is_null());
        let mut large = AttemptRecord::start(&ws, &job, "large-fixture", 3)
            .unwrap()
            .unwrap();
        for n in 1..=12 {
            large
                .requested_answers(
                    &json!({"type":"fields_requested","number":n,"text":"x".repeat(200_000)}),
                )
                .unwrap();
        }
        let page = answer_history(&ws, &job, "large-fixture", 0).unwrap();
        let cursor = page["next_after"].as_u64().unwrap() as u32;
        assert!(cursor > 0 && cursor < 12);
        let remainder = answer_history(&ws, &job, "large-fixture", cursor).unwrap();
        assert_eq!(remainder["items"][0]["data"]["number"], cursor + 1);
        assert!(remainder["next_after"].is_null());
    }

    #[test]
    fn reviewed_answer_snapshots_preserve_each_request_without_overwriting() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        fs::create_dir(dir.path().join("answers")).unwrap();
        let mut record = AttemptRecord {
            dir: dir.path().to_path_buf(),
            sequence: 0,
        };
        let first = json!({"fields":[{"question":"Name","value":{"text":"Fictional Candidate"}}],"note":"Requested only; not proof of filling or submission."});
        let second = json!({"fields":[{"question":"Available","value":{"bool":true}}]});
        let first_path = record.requested_answers(&first).unwrap();
        let second_path = record.requested_answers(&second).unwrap();
        assert_ne!(first_path, second_path);
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(dir.path().join(&first_path)).unwrap())
                .unwrap(),
            first
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(dir.path().join(second_path)).unwrap())
                .unwrap(),
            second
        );
        let outcome = json!({"type":"fill_incomplete","answer_file":first_path,"completed_commands":[{"selector":"#name"}],"note":"Not proof of submission"});
        record.answer_outcome(outcome.clone()).unwrap();
        let event: Value =
            serde_json::from_slice(&fs::read(dir.path().join("events.jsonl")).unwrap()).unwrap();
        let saved: Value = serde_json::from_slice(
            &fs::read(dir.path().join(event["outcome_file"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        assert_eq!(saved, outcome);
        record.dir = dir.path().join("missing");
        assert!(record.requested_answers(&first).is_err());
    }
    #[test]
    fn saved_listing_view_reconstructs_tiles_and_rejects_untrusted_markup() {
        let metadata = json!({"page_width":3000,"page_height":2000,"full_dimensions_captured":true,
            "source_url":"<script>not markup</script>",
            "segment_rectangles":[{"x":0,"y":0,"width":1600,"height":1600},{"x":1600,"y":0,"width":1400,"height":1600},{"x":0,"y":1600,"width":1600,"height":400},{"x":1600,"y":1600,"width":1400,"height":400}]}).to_string();
        let view = listing_capture_view(metadata.as_bytes(), 4);
        assert!(view.contains("src=\"0004.png\""));
        assert!(view.contains("left:53.333333%;top:80.000000%"));
        assert!(!view.contains("<script>"));
        assert!(!view.contains("Capture is incomplete"));
        let incomplete = listing_capture_view(metadata.as_bytes(), 3);
        assert!(incomplete.contains("Page layout is unavailable"));
        assert!(!incomplete.contains("0004.png"));
        let overlap = json!({"page_width":100,"page_height":100,"full_dimensions_captured":true,"segment_rectangles":[{"x":0,"y":0,"width":100,"height":100},{"x":0,"y":0,"width":100,"height":100}]}).to_string();
        assert!(listing_capture_view(overlap.as_bytes(), 2).contains("Capture is incomplete"));
        assert!(listing_capture_view(b"not json", 1).contains("Page layout is unavailable"));
    }
    #[test]
    #[ignore = "starts a local browser against fictional saved listing images"]
    fn saved_listing_view_opens_with_positioned_images() {
        use waypoint_applications::driver::BrowserDriver;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        let metadata = json!({"page_width":2000,"page_height":1000,"full_dimensions_captured":true,"segment_rectangles":[{"x":0,"y":0,"width":1000,"height":1000},{"x":1000,"y":0,"width":1000,"height":1000}]}).to_string();
        fs::write(
            root.path().join("view.html"),
            listing_capture_view(metadata.as_bytes(), 2),
        )
        .unwrap();
        let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/a0sAAAAASUVORK5CYII=").unwrap();
        fs::write(root.path().join("0001.png"), &png).unwrap();
        fs::write(root.path().join("0002.png"), &png).unwrap();
        let url = url::Url::from_file_path(root.path().join("view.html")).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        let style = driver
            .read_attribute(".page img:nth-child(2)", "style")
            .unwrap()
            .unwrap();
        assert!(style.contains("left:50.000000%"));
        assert!(!driver.screenshot_base64().unwrap().is_empty());
    }
    #[test]
    fn tile_planning_preserves_budget_and_reports_incomplete_pages() {
        let (tiles, complete) = capture_tiles(3000.0, 2400.0);
        assert!(complete);
        assert_eq!(
            tiles,
            vec![
                (0.0, 0.0, 2048.0, 2000.0),
                (2048.0, 0.0, 952.0, 2000.0),
                (0.0, 400.0, 2048.0, 2000.0),
                (2048.0, 400.0, 952.0, 2000.0)
            ]
        );
        let (tiles, complete) = capture_tiles(4096.0, 12000.0);
        assert_eq!(tiles.len(), 6);
        assert!(!complete);
        assert!(!capture_tiles(4097.0, 1000.0).1);
        assert!(capture_tiles(2048.0, 12000.0).1);
    }
    #[test]
    #[ignore = "starts one headless browser against a fictional narrow form"]
    fn captures_stable_narrow_page_with_scrollbar() {
        use waypoint_applications::driver::BrowserDriver;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        fs::create_dir(root.path().join("screenshots")).unwrap();
        let mut record = AttemptRecord {
            dir: root.path().to_path_buf(),
            sequence: 0,
        };
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-narrow-capture.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        capture_page(&mut driver, &mut record, 1).unwrap();
        let events = fs::read_to_string(root.path().join("events.jsonl")).unwrap();
        assert!(events.contains("full_dimensions_captured\":true"));
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional changing form"]
    fn changing_page_content_is_recorded_as_incomplete() {
        use waypoint_applications::driver::BrowserDriver;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        fs::create_dir(root.path().join("screenshots")).unwrap();
        let mut record = AttemptRecord {
            dir: root.path().to_path_buf(),
            sequence: 0,
        };
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-changing-capture.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(capture_page(&mut driver, &mut record, 1)
            .unwrap_err()
            .contains("changed"));
        let events: Vec<Value> = fs::read_to_string(root.path().join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let changed = events
            .iter()
            .find(|event| event["type"] == "page_capture_changed")
            .unwrap();
        assert!(root
            .path()
            .join(changed["screenshot"].as_str().unwrap())
            .is_file());
        assert!(!events.iter().any(|event| event["type"] == "page_capture"));
        let starts: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "page_capture_started")
            .collect();
        assert_eq!(starts.len(), 2);
        assert_ne!(starts[0]["capture_id"], starts[1]["capture_id"]);
        assert!(events
            .iter()
            .any(|event| event["type"] == "page_capture_retry"));
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "page_capture_changed")
                .count(),
            2
        );
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional wide form"]
    fn captures_bounded_horizontal_tiles_on_wide_page() {
        use waypoint_applications::driver::BrowserDriver;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        fs::create_dir(root.path().join("screenshots")).unwrap();
        let mut record = AttemptRecord {
            dir: root.path().to_path_buf(),
            sequence: 0,
        };
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-wide-capture.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        capture_page(&mut driver, &mut record, 1).unwrap();
        let events: Vec<Value> = fs::read_to_string(root.path().join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let segments: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "page_capture_segment")
            .collect();
        assert_eq!(segments.len(), 4);
        let completed = events
            .iter()
            .find(|event| event["type"] == "page_capture")
            .unwrap();
        let last_y = (completed["page_height"].as_f64().unwrap() - 2000.0).max(0.0);
        assert!(segments.iter().any(
            |event| event["x"].as_f64() == Some(2048.0) && event["y"].as_f64() == Some(last_y)
        ));
        assert!(events
            .iter()
            .any(|event| event["type"] == "page_capture"
                && event["full_dimensions_captured"] == true));
        assert!(segments.iter().all(|event| root
            .path()
            .join(event["screenshot"].as_str().unwrap())
            .is_file()));
        assert!(driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }
    use waypoint_domain::ApplicationState;
    use waypoint_store::StoredJob;

    #[test]
    fn failed_initial_screenshot_marks_attempt_for_manual_review() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        fs::create_dir(root.path().join("screenshots")).unwrap();
        fs::write(
            root.path().join("attempt.json"),
            br#"{"status":"preparation_in_progress"}"#,
        )
        .unwrap();
        let mut record = AttemptRecord {
            dir: root.path().to_path_buf(),
            sequence: 0,
        };
        let error = record_initial_inspection(
            &mut record,
            "https://example.invalid",
            &[],
            Some("not base64!"),
            2,
        )
        .unwrap_err();
        let meta: Value =
            serde_json::from_slice(&fs::read(root.path().join("attempt.json")).unwrap()).unwrap();
        assert_eq!(meta["status"], "manual_review", "record error: {error}");
        let events = fs::read_to_string(root.path().join("events.jsonl")).unwrap();
        let event: Value = serde_json::from_str(events.trim()).unwrap();
        assert_eq!(event["type"], "capture_incomplete");
        fs::remove_file(root.path().join("attempt.json")).unwrap();
        let error = record_initial_inspection(
            &mut record,
            "https://example.invalid",
            &[],
            Some("not base64!"),
            3,
        )
        .unwrap_err();
        assert!(error.contains("could not mark attempt for manual review"));
    }

    #[test]
    fn inspection_retains_questions_when_the_screenshot_fails() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        fs::create_dir(root.path().join("screenshots")).unwrap();
        fs::create_dir(root.path().join("answers")).unwrap();
        fs::write(
            root.path().join("attempt.json"),
            br#"{"status":"preparation_in_progress"}"#,
        )
        .unwrap();
        let mut record = AttemptRecord {
            dir: root.path().to_path_buf(),
            sequence: 0,
        };
        let field = serde_json::from_value(json!({"selector":"#email","label":"Email","field_type":"email","required":true,"sensitive":false})).unwrap();
        assert!(record_inspection_evidence(
            &mut record,
            "https://example.invalid",
            &[],
            &[field],
            Some("invalid image"),
            2
        )
        .is_err());
        let saved: Value =
            serde_json::from_slice(&fs::read(root.path().join("answers/0001.json")).unwrap())
                .unwrap();
        assert_eq!(saved["type"], "fields_inspected");
        assert_eq!(saved["fields"][0]["question"], "Email");
        let meta: Value =
            serde_json::from_slice(&fs::read(root.path().join("attempt.json")).unwrap()).unwrap();
        assert_eq!(meta["status"], "manual_review");
    }

    #[test]
    fn inspection_retains_page_event_when_questions_cannot_be_saved() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        fs::write(
            root.path().join("attempt.json"),
            br#"{"status":"preparation_in_progress"}"#,
        )
        .unwrap();
        let mut record = AttemptRecord {
            dir: root.path().to_path_buf(),
            sequence: 0,
        };
        assert!(record_inspection_evidence(
            &mut record,
            "https://example.invalid",
            &[],
            &[],
            None,
            2
        )
        .is_err());
        let events = fs::read_to_string(root.path().join("events.jsonl")).unwrap();
        assert!(events
            .lines()
            .any(|line| serde_json::from_str::<Value>(line).unwrap()["type"] == "page_inspected"));
        let meta: Value =
            serde_json::from_slice(&fs::read(root.path().join("attempt.json")).unwrap()).unwrap();
        assert_eq!(meta["status"], "manual_review");
    }

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
    fn manifest_failure_keeps_material_and_marks_attempt_for_review() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("materials")).unwrap();
        fs::create_dir(dir.path().join("materials/manifest.jsonl")).unwrap();
        fs::write(
            dir.path().join("attempt.json"),
            br#"{"status":"preparation_in_progress"}"#,
        )
        .unwrap();
        let mut record = AttemptRecord {
            dir: dir.path().to_path_buf(),
            sequence: 0,
        };
        let error = record.material("resume.pdf", b"fictional PDF").unwrap_err();
        assert!(error.contains("manifest"), "{error}");
        assert_eq!(
            fs::read(dir.path().join("materials/0001-resume.pdf")).unwrap(),
            b"fictional PDF"
        );
        let metadata: Value =
            serde_json::from_slice(&fs::read(dir.path().join("attempt.json")).unwrap()).unwrap();
        assert_eq!(metadata["status"], "manual_review");
        assert_eq!(metadata["material_manifest_incomplete"], true);
        record.status("review_session_closed", 0).unwrap();
        let after: Value =
            serde_json::from_slice(&fs::read(dir.path().join("attempt.json")).unwrap()).unwrap();
        assert_eq!(after["material_manifest_incomplete"], true);
        let event: Value =
            serde_json::from_slice(&fs::read(dir.path().join("events.jsonl")).unwrap()).unwrap();
        assert_eq!(event["type"], "material_manifest_incomplete");
        assert_eq!(event["stored_name"], "0001-resume.pdf");
        fs::remove_file(dir.path().join("events.jsonl")).unwrap();
        fs::create_dir(dir.path().join("events.jsonl")).unwrap();
        let error = record.material("second.pdf", b"second PDF").unwrap_err();
        assert!(
            error.contains("archive event could not be saved"),
            "{error}"
        );
        let metadata: Value =
            serde_json::from_slice(&fs::read(dir.path().join("attempt.json")).unwrap()).unwrap();
        assert_eq!(metadata["status"], "manual_review");
        fs::remove_file(dir.path().join("attempt.json")).unwrap();
        fs::create_dir(dir.path().join("attempt.json")).unwrap();
        let error = record.material("third.pdf", b"third PDF").unwrap_err();
        assert!(
            error.contains("attempt review status could not be saved"),
            "{error}"
        );
    }

    #[test]
    fn attempt_listing_exposes_material_manifest_failure() {
        let root = tempfile::tempdir().unwrap();
        let ws = Workspace::open(root.path()).unwrap();
        let job = JobIdentityId::new("fictional-manifest-listing").unwrap();
        let hash = Sha256Digest::of(job.as_str());
        let dir = root
            .path()
            .join("application-records")
            .join(format!("{}-fixture", &hash.as_str()[..16]));
        fs::create_dir_all(dir.join("materials/manifest.jsonl")).unwrap();
        fs::write(dir.join("attempt.json"), serde_json::to_vec(&json!({"job_id":job.as_str(),"attempt_id":"fixture","status":"preparation_in_progress","started_at":1})).unwrap()).unwrap();
        let mut record = AttemptRecord { dir, sequence: 0 };
        assert!(record.material("resume.pdf", b"fictional PDF").is_err());
        let result = list(&ws, &job).unwrap();
        assert_eq!(result["attempts"][0]["material_manifest_incomplete"], true);
        assert_eq!(result["attempts"][0]["status"], "manual_review");
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
        let image = b"\x89PNG\r\n\x1a\nfictional";
        let encoded = STANDARD.encode(image);
        let name = record.screenshot_with_name(&encoded).unwrap();
        record.event(json!({"type":"visual_capture","task_id":"test-task","screenshot":format!("screenshots/{name}")})).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("screenshots").join(&name)).unwrap(),
            image
        );
        let event: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("events.jsonl")).unwrap())
                .unwrap();
        assert_eq!(event["task_id"], "test-task");
        assert_eq!(event["screenshot"], format!("screenshots/{name}"));
        fs::write(dir.path().join("screenshots/0002.png"), b"existing").unwrap();
        assert!(record.screenshot_with_name(&encoded).is_err());
        assert_eq!(
            fs::read(dir.path().join("screenshots/0002.png")).unwrap(),
            b"existing"
        );
        assert_eq!(
            fs::read_dir(dir.path().join("screenshots"))
                .unwrap()
                .count(),
            2
        );
    }
    #[test]
    fn oversized_screenshot_is_rejected_before_decoding_or_recording() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("screenshots")).unwrap();
        let mut record = AttemptRecord {
            dir: dir.path().to_path_buf(),
            sequence: 0,
        };
        // Invalid base64 of this size would produce a decoding error if decoded.
        let oversized = "!".repeat((6 * 1024 * 1024 / 3) * 4 + 1);
        assert_eq!(
            record.screenshot_with_name(&oversized).unwrap_err(),
            "Screenshot exceeds limit"
        );
        assert_eq!(record.sequence, 0);
        assert_eq!(
            fs::read_dir(dir.path().join("screenshots"))
                .unwrap()
                .count(),
            0
        );
        assert!(record.screenshot_with_name("!").is_err());
        assert_eq!(
            record.screenshot_with_name("aW1hZ2U=").unwrap_err(),
            "Invalid screenshot content"
        );
        assert_eq!(record.sequence, 0);
        drop(oversized);
        let mut bytes = vec![0; 6 * 1024 * 1024];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let boundary = STANDARD.encode(bytes);
        let name = record.screenshot_with_name(&boundary).unwrap();
        assert_eq!(
            fs::metadata(dir.path().join("screenshots").join(name))
                .unwrap()
                .len(),
            6 * 1024 * 1024
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
    #[ignore = "starts a local browser and observes fictional focused-field screenshots"]
    fn focused_text_capture_stability_probe() {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::{BrowserCommand, PacketValue};
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#candidate".into(),
                value: PacketValue::Text("Fictional".into()),
            })
            .unwrap();
        let (initial_width, initial_height) = driver.page_dimensions().unwrap();
        driver
            .screenshot_tile(
                0.0,
                0.0,
                initial_width.min(2048.0),
                initial_height.min(2000.0),
            )
            .unwrap();
        let (width, height) = driver.page_dimensions().unwrap();
        let tile = (width.min(2048.0), height.min(2000.0));
        let mut hashes = Vec::new();
        for _ in 0..4 {
            let image = driver.screenshot_tile(0.0, 0.0, tile.0, tile.1).unwrap();
            hashes.push(waypoint_domain::Sha256Digest::of(&image));
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
        println!(
            "Initial dimensions: {:?}; warmed dimensions: {:?}; focused field screenshot hashes equal to first: {:?}",
            (initial_width, initial_height), (width, height),
            hashes
                .iter()
                .map(|hash| hash == &hashes[0])
                .collect::<Vec<_>>()
        );
        assert_eq!(driver.page_dimensions().unwrap(), (width, height));
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
        capture_inspection(
            &mut driver,
            &mut record,
            "file://",
            &[],
            &[],
            Some(&viewport),
            2,
        )
        .unwrap();
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
        let finished = entries
            .iter()
            .find(|event| event["type"] == "page_capture")
            .unwrap();
        let started = entries
            .iter()
            .find(|event| {
                event["type"] == "page_capture_started"
                    && event["capture_id"] == finished["capture_id"]
            })
            .unwrap();
        let segments: Vec<_> = entries
            .iter()
            .filter(|event| {
                event["type"] == "page_capture_segment"
                    && event["capture_id"] == finished["capture_id"]
            })
            .collect();
        assert_eq!(segments.len(), 2);
        assert_eq!(started["capture_id"], finished["capture_id"]);
        for (index, event) in segments.iter().enumerate() {
            assert_eq!(event["capture_id"], started["capture_id"]);
            assert_eq!(event["index"], index);
            let expected_y = if index == 0 {
                0.0
            } else {
                finished["page_height"].as_f64().unwrap() - 2000.0
            };
            assert_eq!(event["y"].as_f64().unwrap(), expected_y);
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
        assert!(
            fs::read_dir(record.dir.join("screenshots"))
                .unwrap()
                .count()
                >= 3
        );
        // Force a collision on the second segment. The first segment and its
        // capture ID must remain attributable even without a completion event.
        let collision = record
            .dir
            .join("screenshots")
            .join(format!("{:04}.png", record.sequence + 2));
        fs::write(&collision, b"existing").unwrap();
        assert!(capture_before_navigation(&mut driver, &mut record, 3).is_err());
        assert_eq!(fs::read(&collision).unwrap(), b"existing");
        assert!(capture_page(&mut driver, &mut record, 4).is_ok());
        let entries: Vec<Value> = fs::read_to_string(record.dir.join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let starts: Vec<_> = entries
            .iter()
            .filter(|event| {
                event["type"] == "page_capture_started"
                    && (event["time"] == 3 || event["time"] == 4)
            })
            .collect();
        assert_eq!(starts.len(), 2);
        assert!(entries
            .iter()
            .any(|event| event["type"] == "navigation_capture_incomplete"));
        assert!(!entries
            .iter()
            .any(|event| event["type"] == "navigation_capture_complete"));
        assert_ne!(starts[0]["capture_id"], starts[1]["capture_id"]);
        let failed_id = &starts[0]["capture_id"];
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
            && event["capture_id"] == starts[1]["capture_id"]));
        driver
            .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                selector: "#candidate".into(),
                value: waypoint_applications::form_mapping::PacketValue::Text(
                    "Manual fixture change".into(),
                ),
            })
            .unwrap();
        let capture_result = capture_before_navigation(&mut driver, &mut record, 5);
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
            .rposition(|event| {
                event["type"]
                    == if capture_result.is_ok() {
                        "navigation_capture_complete"
                    } else {
                        "navigation_capture_incomplete"
                    }
                    && event["time"] == 5
            })
            .unwrap();
        assert!(start < end);
        let captured: Vec<_> = entries[start..end]
            .iter()
            .filter(|event| event["type"] == "page_capture_segment")
            .collect();
        assert!(!captured.is_empty());
        assert!(captured.iter().all(|event| record
            .dir
            .join(event["screenshot"].as_str().unwrap())
            .is_file()));
        if capture_result.is_ok() {
            let finished_id = &entries[start..end]
                .iter()
                .find(|event| event["type"] == "page_capture")
                .unwrap()["capture_id"];
            assert_eq!(
                captured
                    .iter()
                    .filter(|event| &event["capture_id"] == finished_id)
                    .count(),
                2
            );
        } else {
            assert!(!entries[start..end]
                .iter()
                .any(|event| event["type"] == "page_capture"));
        }
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

    #[cfg(windows)]
    #[test]
    fn failed_initial_archive_remains_visible_as_incomplete() {
        let root = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        let id = JobIdentityId::new("fictional-incomplete").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: id.clone(),
                title: "Engineer".into(),
                employer: "Example".into(),
                location: "Remote".into(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some("https://example.org/job".into()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: true,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional posting"),
        )
        .unwrap();
        let invalid_capture = json!({"page_text_status":"captured"}).to_string();
        ws.save_job_capture(
            &id,
            invalid_capture.as_bytes(),
            &[b"\x89PNG\r\n\x1a\nfictional".to_vec()],
        )
        .unwrap();
        assert!(AttemptRecord::start(&ws, &id, "failed-fixture", 123).is_err());
        let listing = list(&ws, &id).unwrap();
        assert_eq!(listing["unreadable"], 0);
        assert_eq!(listing["attempts"][0]["status"], "record_incomplete");
        assert_eq!(listing["attempts"][0]["submitted_by_waypoint"], false);
        assert!(listing["attempts"][0]["submitted"].is_null());
    }

    #[test]
    fn missing_attachment_viewport_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let mut record = AttemptRecord {
            dir: root.path().into(),
            sequence: 0,
        };
        let error = attachment_viewport(
            &mut record,
            "#resume",
            Some("Resume"),
            Err("fixture failure".into()),
            1,
        )
        .unwrap_err();
        assert!(error.contains("screenshot unavailable"));
        assert!(!root.path().join("events.jsonl").exists());
        let image = STANDARD.encode(b"\x89PNG\r\n\x1a\nfictional");
        assert!(
            attachment_viewport(&mut record, "#resume", Some("Resume"), Ok(image.clone()), 1)
                .is_err()
        );
        fs::create_dir(root.path().join("screenshots")).unwrap();
        attachment_viewport(&mut record, "#resume", Some("Resume"), Ok(image), 2).unwrap();
        let event: Value =
            serde_json::from_slice(&fs::read(root.path().join("events.jsonl")).unwrap()).unwrap();
        assert_eq!(event["type"], "attachment_viewport");
        assert_eq!(event["field_label"], "Resume");
        assert!(root
            .path()
            .join(event["screenshot"].as_str().unwrap())
            .is_file());
    }

    #[test]
    fn source_only_text_reports_missing_truncated_and_invalid_capture() {
        assert_eq!(source_only_text(None).0, "unavailable");
        assert_eq!(source_only_text(Some(b"invalid")).0, "invalid");
        assert_eq!(
            source_only_text(Some(
                br#"{"page_text_status":"truncated","page_text":"Fictional"}"#
            ))
            .0,
            "truncated"
        );
        let oversized =
            json!({"page_text_status":"captured","page_text":"x".repeat(32_001)}).to_string();
        assert_eq!(source_only_text(Some(oversized.as_bytes())).0, "invalid");
    }

    #[test]
    fn capture_inventory_reports_unknown_for_legacy_and_invalid_counts() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            listing_capture_files(dir.path(), &json!({"page_capture_available": true}), false),
            "unknown"
        );
        assert_eq!(
            listing_capture_files(
                dir.path(),
                &json!({"page_capture_available": true, "page_capture_segment_count": 6}),
                false
            ),
            "unknown"
        );
        assert_eq!(
            listing_capture_files(
                dir.path(),
                &json!({"page_capture_available": true, "page_capture_segment_count": 1}),
                false
            ),
            "missing_or_inaccessible"
        );
    }

    #[test]
    fn capture_integrity_reports_changes_legacy_and_read_budget() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("original-job-page");
        fs::create_dir(&dir).unwrap();
        let info = json!({"page_capture_available":true,"page_capture_segment_count":1});
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 100_000),
            "legacy"
        );
        fs::write(dir.join("capture.json"), b"metadata").unwrap();
        fs::write(dir.join("0001.png"), b"image").unwrap();
        let manifest = json!({"algorithm":"SHA-256","files":{
            "capture.json":waypoint_domain::sha256_hex(b"metadata"),
            "0001.png":waypoint_domain::sha256_hex(b"image")}});
        fs::write(
            dir.join("capture-hashes.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 100_000),
            "partial_match"
        );
        fs::write(dir.join("view.html"), b"viewer").unwrap();
        fs::write(dir.join("page-text.txt"), b"listing text").unwrap();
        fs::write(dir.join("structured-0001.json.txt"), b"structured").unwrap();
        let capture = json!({"page_text_status":"captured", "page_text":"listing text", "structured_source_status":"captured", "structured_source_scripts":["structured"]}).to_string();
        fs::write(dir.join("capture.json"), &capture).unwrap();
        let mut complete = manifest.clone();
        complete["version"] = json!(2);
        complete["files"]["capture.json"] = json!(waypoint_domain::sha256_hex(capture.as_bytes()));
        complete["files"]["view.html"] = json!(waypoint_domain::sha256_hex(b"viewer"));
        complete["files"]["page-text.txt"] = json!(waypoint_domain::sha256_hex(b"listing text"));
        complete["files"]["structured-0001.json.txt"] =
            json!(waypoint_domain::sha256_hex(b"structured"));
        fs::write(
            dir.join("capture-hashes.json"),
            serde_json::to_vec(&complete).unwrap(),
        )
        .unwrap();
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 100_000),
            "match"
        );
        for (name, original) in [
            ("view.html", &b"viewer"[..]),
            ("page-text.txt", &b"listing text"[..]),
            ("structured-0001.json.txt", &b"structured"[..]),
        ] {
            fs::write(dir.join(name), b"changed").unwrap();
            assert_eq!(
                listing_capture_integrity(root.path(), &info, false, &mut 100_000),
                "mismatch"
            );
            fs::write(dir.join(name), original).unwrap();
        }
        for name in ["page-text.txt", "structured-0001.json.txt"] {
            let mut incomplete = complete.clone();
            incomplete["files"].as_object_mut().unwrap().remove(name);
            fs::write(
                dir.join("capture-hashes.json"),
                serde_json::to_vec(&incomplete).unwrap(),
            )
            .unwrap();
            assert_eq!(
                listing_capture_integrity(root.path(), &info, false, &mut 100_000),
                "mismatch"
            );
        }
        fs::write(dir.join("page-text.txt"), b"changed").unwrap();
        let mut inconsistent = complete.clone();
        inconsistent["files"]["page-text.txt"] = json!(waypoint_domain::sha256_hex(b"changed"));
        fs::write(
            dir.join("capture-hashes.json"),
            serde_json::to_vec(&inconsistent).unwrap(),
        )
        .unwrap();
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 100_000),
            "mismatch"
        );
        fs::write(dir.join("page-text.txt"), b"listing text").unwrap();
        fs::write(dir.join("capture.json"), b"metadata").unwrap();
        fs::write(
            dir.join("capture-hashes.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 1),
            "budget_exhausted"
        );
        fs::write(dir.join("0001.png"), b"changed").unwrap();
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 100_000),
            "mismatch"
        );
        fs::remove_file(dir.join("0001.png")).unwrap();
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 100_000),
            "unavailable"
        );
        let mut invalid = manifest;
        invalid["files"]["../other"] = json!("a".repeat(64));
        fs::write(
            dir.join("capture-hashes.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert_eq!(
            listing_capture_integrity(root.path(), &info, false, &mut 100_000),
            "unavailable"
        );
    }

    #[test]
    fn capture_coverage_requires_valid_complete_nonoverlapping_geometry() {
        let mut capture = json!({"page_width": 2000, "page_height": 1000,
        "full_dimensions_captured": true, "segment_rectangles": [
            {"x": 0, "y": 0, "width": 1000, "height": 1000},
            {"x": 1000, "y": 0, "width": 1000, "height": 1000}
        ]});
        let bytes = |value: &Value| serde_json::to_vec(value).unwrap();
        assert_eq!(
            listing_capture_coverage(&bytes(&capture), 2),
            "dimensions_covered"
        );
        capture["full_dimensions_captured"] = json!(false);
        assert_eq!(listing_capture_coverage(&bytes(&capture), 2), "partial");
        capture["full_dimensions_captured"] = json!(true);
        capture["segment_rectangles"][1]["x"] = json!(500);
        assert_eq!(listing_capture_coverage(&bytes(&capture), 2), "partial");
        assert_eq!(listing_capture_coverage(&bytes(&capture), 1), "unavailable");
        assert_eq!(listing_capture_coverage(b"invalid", 2), "unavailable");
    }

    #[test]
    fn attempt_records_unknown_remote_status_as_null() {
        let root = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        let id = JobIdentityId::new("unknown-remote-fixture").unwrap();
        ws.import_manual_job(
            StoredJob {
                id: id.clone(),
                title: "Engineer".into(),
                employer: "Example".into(),
                location: String::new(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some("https://example.org/role".into()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional listing"),
        )
        .unwrap();
        let record = AttemptRecord::start(&ws, &id, "unknown-fixture", 1)
            .unwrap()
            .unwrap();
        let listing: Value =
            serde_json::from_slice(&fs::read(record.dir.join("original-job.json")).unwrap())
                .unwrap();
        assert!(listing["remote"].is_null());
        assert!(listing["remote_status_note"]
            .as_str()
            .unwrap()
            .contains("unconfirmed"));
    }

    #[test]
    fn attempt_records_explicit_non_remote_source_evidence() {
        let root = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        let body = r#"{"jobs":[{"id":"onsite","title":"Engineer","jobUrl":"https://jobs.ashbyhq.com/ashby/onsite","isRemote":false}]}"#;
        let fetcher = waypoint_workspace::StaticFetcher::default()
            .route("https://api.ashbyhq.com/posting-api/job-board/ashby", body);
        assert_eq!(
            ws.discover(
                &[waypoint_workspace::DiscoveryTarget::new("ashby", "ashby")],
                &fetcher,
                1
            )
            .inserted,
            1
        );
        let id = ws.jobs().unwrap().remove(0).id;
        let record = AttemptRecord::start(&ws, &id, "onsite-fixture", 1)
            .unwrap()
            .unwrap();
        let listing: Value =
            serde_json::from_slice(&fs::read(record.dir.join("original-job.json")).unwrap())
                .unwrap();
        assert_eq!(listing["remote"], false);
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
            let first_source = br#"{"requested_url":"https://example.org/redirect/1","source_url":"https://example.org/jobs/1","segments":0,"page_text_status":"truncated","page_text":"Source-only fictional text"}"#;
            assert!(ws.save_job_source_only(&id, first_source).unwrap());
            let source_only_record = AttemptRecord::start(&ws, &id, "source-only", 122)
                .unwrap()
                .unwrap();
            let source_only_listing: Value = serde_json::from_slice(
                &fs::read(source_only_record.dir.join("original-job.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(source_only_listing["page_capture_available"], false);
            assert_eq!(source_only_listing["source_only_text_status"], "truncated");
            assert_eq!(
                fs::read_to_string(source_only_record.dir.join("original-job-source-text.txt"))
                    .unwrap(),
                "Source-only fictional text"
            );
            let source_path = source_only_record.dir.canonicalize().unwrap();
            assert!(source_path.starts_with(root.path().canonicalize().unwrap()));
            fs::remove_dir_all(source_path).unwrap();
            let png = b"\x89PNG\r\n\x1a\nfictional-data".to_vec();
            let metadata=json!({"requested_url":"https://example.org/redirect/2","source_url":"https://example.org/jobs/1","segments":1,"full_dimensions_captured":false,"page_text_status":"captured","page_text":"Fictional original page text","structured_source_status":"captured","structured_source_scripts":["{\"@type\":\"JobPosting\",\"description\":\"<p>Fictional role</p>\"}"]}).to_string();
            assert!(ws
                .save_job_capture(&id, metadata.as_bytes(), std::slice::from_ref(&png))
                .unwrap());
            assert!(!ws
                .save_job_capture(&id, metadata.as_bytes(), std::slice::from_ref(&png))
                .unwrap());
            let mut followup: Value = serde_json::from_str(&metadata).unwrap();
            followup["full_dimensions_captured"] = json!(true);
            followup["page_text"] = json!("Fictional later page text");
            assert!(ws
                .save_job_capture_followup(
                    &id,
                    followup.to_string().as_bytes(),
                    std::slice::from_ref(&png)
                )
                .unwrap());
        }
        let mut record = AttemptRecord::start(&ws, &id, "attempt-fixture", 123)
            .unwrap()
            .unwrap();
        let initial: Value =
            serde_json::from_slice(&fs::read(record.dir.join("attempt.json")).unwrap()).unwrap();
        assert_eq!(initial["status"], "preparation_in_progress");
        assert!(AttemptRecord::start(&ws, &id, "attempt-fixture", 124).is_err());
        assert!(AttemptRecord::start(&ws, &id, "../invalid", 124).is_err());
        let listing: Value =
            serde_json::from_slice(&fs::read(record.dir.join("original-job.json")).unwrap())
                .unwrap();
        assert_eq!(listing["remote"], true);
        assert!(listing["remote_status_note"]
            .as_str()
            .unwrap()
            .contains("Null means unconfirmed"));
        assert_eq!(listing["description"], "Fictional job description.");
        assert_eq!(listing["source_url"], "https://example.org/jobs/1");
        assert_eq!(listing["description_available"], true);
        #[cfg(windows)]
        {
            assert_eq!(listing["source_only_metadata_available"], true);
            assert_eq!(listing["source_only_text_status"], "truncated");
            assert_eq!(listing["source_only_matches_current_url"], true);
            assert_eq!(
                fs::read_to_string(record.dir.join("original-job-source-text.txt")).unwrap(),
                "Source-only fictional text"
            );
            assert_eq!(
                listing["page_capture_requested_url"],
                "https://example.org/redirect/2"
            );
            assert_eq!(
                listing["source_only_requested_url"],
                "https://example.org/redirect/1"
            );
            assert_eq!(
                fs::read(record.dir.join("original-job-source-metadata.json")).unwrap(),
                br#"{"requested_url":"https://example.org/redirect/1","source_url":"https://example.org/jobs/1","segments":0,"page_text_status":"truncated","page_text":"Source-only fictional text"}"#
            );
            assert_eq!(listing["page_capture_available"], true);
            assert_eq!(listing["page_capture_coverage"], "partial");
            assert_eq!(listing["followup_capture_available"], true);
            assert_eq!(
                fs::read_to_string(record.dir.join("followup-job-page/page-text.txt")).unwrap(),
                "Fictional later page text"
            );
            assert!(record.dir.join("followup-job-page/0001.png").exists());
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
            for folder in ["original-job-page", "followup-job-page"] {
                let dir = record.dir.join(folder);
                let manifest: Value =
                    serde_json::from_slice(&fs::read(dir.join("capture-hashes.json")).unwrap())
                        .unwrap();
                assert_eq!(manifest["algorithm"], "SHA-256");
                assert_eq!(manifest["version"], 2);
                for name in [
                    "capture.json",
                    "0001.png",
                    "view.html",
                    "page-text.txt",
                    "structured-0001.json.txt",
                ] {
                    let bytes = fs::read(dir.join(name)).unwrap();
                    assert_eq!(manifest["files"][name], waypoint_domain::sha256_hex(&bytes));
                }
            }
            for followup in [false, true] {
                assert_eq!(
                    listing_capture_integrity(&record.dir, &initial, followup, &mut 100_000),
                    "match"
                );
            }
            assert!(
                fs::read_to_string(record.dir.join("original-job-page/view.html"))
                    .unwrap()
                    .contains("Page layout is unavailable")
            );
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
        let saved_bytes = fs::read(record.dir.join("original-job.json")).unwrap();
        let current = ws.job(&id).unwrap().unwrap();
        let mut budget = 2 * 1024 * 1024;
        assert_eq!(
            listing_snapshot_comparison(
                &record.dir,
                Some(&current),
                Some("Changed description"),
                &mut budget
            ),
            "changed"
        );
        assert_eq!(
            listing_snapshot_comparison(&record.dir, Some(&current), None, &mut budget),
            "unavailable"
        );
        let mut damaged: Value = serde_json::from_slice(&saved_bytes).unwrap();
        damaged["description"] = json!("Changed without its fingerprint");
        fs::write(
            record.dir.join("original-job.json"),
            serde_json::to_vec(&damaged).unwrap(),
        )
        .unwrap();
        assert_eq!(
            listing_snapshot_comparison(
                &record.dir,
                Some(&current),
                Some("Fictional job description."),
                &mut budget
            ),
            "unavailable"
        );
        fs::write(record.dir.join("original-job.json"), saved_bytes).unwrap();
        fs::write(record.dir.join("original-job.txt"), "Altered copy").unwrap();
        assert_eq!(
            listing_snapshot_comparison(
                &record.dir,
                Some(&current),
                Some("Fictional job description."),
                &mut budget
            ),
            "unavailable"
        );
        fs::write(
            record.dir.join("original-job.txt"),
            "Fictional job description.",
        )
        .unwrap();
        assert_eq!(
            listing_snapshot_comparison(
                &record.dir,
                Some(&current),
                Some("Fictional job description."),
                &mut budget
            ),
            "matches"
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
        assert_eq!(
            records["attempts"][0]["listing_snapshot_comparison"],
            "matches"
        );
        let mut changed_job = ws.job(&id).unwrap().unwrap();
        changed_job.title = "Updated Engineer".into();
        ws.import_job(changed_job).unwrap();
        assert_eq!(
            list(&ws, &id).unwrap()["attempts"][0]["listing_snapshot_comparison"],
            "changed"
        );
        let original: Value =
            serde_json::from_slice(&fs::read(record.dir.join("original-job.json")).unwrap())
                .unwrap();
        assert_eq!(original["title"], "Engineer");
        assert_eq!(
            fs::read_to_string(record.dir.join("original-job.txt")).unwrap(),
            "Fictional job description."
        );
        assert_eq!(
            records["attempts"][0]["page_capture_coverage"],
            if cfg!(windows) {
                "partial"
            } else {
                "unavailable"
            }
        );
        #[cfg(windows)]
        assert_eq!(
            records["attempts"][0]["page_capture_matches_current_url"],
            true
        );
        #[cfg(windows)]
        {
            assert_eq!(records["attempts"][0]["followup_capture_available"], true);
            assert_eq!(records["attempts"][0]["followup_capture_files"], "present");
            assert_eq!(
                records["attempts"][0]["followup_capture_integrity"],
                "match"
            );
            assert_eq!(records["attempts"][0]["page_capture_integrity"], "match");
            fs::write(record.dir.join("followup-job-page/0001.png"), b"changed").unwrap();
            assert_eq!(
                list(&ws, &id).unwrap()["attempts"][0]["followup_capture_integrity"],
                "mismatch"
            );
            fs::remove_file(record.dir.join("followup-job-page/0001.png")).unwrap();
            assert_eq!(
                list(&ws, &id).unwrap()["attempts"][0]["followup_capture_files"],
                "missing_or_inaccessible"
            );
            assert_eq!(records["attempts"][0]["page_capture_files"], "present");
            fs::remove_file(record.dir.join("original-job-page/0001.png")).unwrap();
            assert_eq!(
                list(&ws, &id).unwrap()["attempts"][0]["page_capture_files"],
                "missing_or_inaccessible"
            );
        }
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
