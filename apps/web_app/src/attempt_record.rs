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
        serde_json::to_value(review).map_err(|_| "Cannot display saved Gmail review")?;
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
        attempts.push(json!({"attempt_id":attempt_id,"status":status,"started_at":started,"folder":entry.path().to_string_lossy(),"submitted":value["submitted"],"submitted_by_waypoint":value["submitted_by_waypoint"],"gmail_review":gmail_review,"email_outcome_file":has_record("email-handoff.json"),"email_reconciliation_file":has_record("email-reconciliation.json"),"document_visual_review":fs::symlink_metadata(entry.path().join("document-visual-review.json")).is_ok_and(|m|m.is_file() && !m.file_type().is_symlink() && m.len() <= 512*1024)}));
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
        let Some(root) = ws.db_path.as_ref().and_then(|p| p.parent()) else {
            return Ok(None);
        };
        let job = ws
            .job(id)
            .map_err(|e| e.to_string())?
            .ok_or("Job not found")?;
        let description = ws.job_description(id).map_err(|e| e.to_string())?;
        let captured = ws.job_capture(id).map_err(|e| e.to_string())?;
        let hash = Sha256Digest::of(id.as_str());
        let dir = root
            .join("application-records")
            .join(format!("{}-{attempt_id}", &hash.as_str()[..16]));
        fs::create_dir_all(dir.join("screenshots")).map_err(|e| e.to_string())?;
        fs::create_dir_all(dir.join("materials")).map_err(|e| e.to_string())?;
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
        let meta = json!({"attempt_id":attempt_id,"job_id":id.as_str(),"employer":job.employer,"title":job.title,"job_url":job.canonical_url,"started_at":timestamp,"status":"preparation_in_progress","submitted":null,"submitted_by_waypoint":false,"note":"External submission is not monitored; this record does not prove whether an application was submitted."});
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
        if let Some(image) = screenshot {
            self.screenshot(image)?;
        }
        self.event(json!({"type":"page_inspected","time":timestamp,"origin":origin,"manual_review":reasons,"screenshot_available":screenshot.is_some()}))?;
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
    pub fn screenshot(&mut self, data: &str) -> Result<(), String> {
        let bytes = STANDARD.decode(data).map_err(|e| e.to_string())?;
        if bytes.len() > 6 * 1024 * 1024 {
            return Err("Screenshot exceeds limit".into());
        }
        self.sequence += 1;
        fs::write(
            self.dir
                .join("screenshots")
                .join(format!("{:04}.png", self.sequence)),
            bytes,
        )
        .map_err(|e| e.to_string())
    }
    pub fn material(&mut self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_control()
                    || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let safe = if safe.trim_matches('.').trim().is_empty() {
            "document"
        } else {
            safe.as_str()
        };
        self.sequence += 1;
        let filename = format!("{:04}-{safe}", self.sequence);
        fs::write(self.dir.join("materials").join(&filename), bytes).map_err(|e| e.to_string())?;
        Ok(filename)
    }
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
    let mut y = 0.0;
    let mut count = 0;
    while y < capped_height {
        let segment_height = (capped_height - y).min(2000.0);
        let image = driver
            .screenshot_segment(width_limited, y, segment_height)
            .map_err(|e| e.to_string())?;
        record.screenshot(&image)?;
        y += segment_height;
        count += 1;
    }
    let within_limits = width <= 2048.0 && height <= 12000.0;
    record.event(json!({"type":"page_capture","time":timestamp,"page_width":width,"page_height":height,"segments":count,"full_dimensions_captured":within_limits,"note":"Page dimensions alone do not guarantee capture of hidden content or nested scrollers."}))?;
    if !within_limits {
        return Err("Page exceeded screenshot capture limits".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::ApplicationState;
    use waypoint_store::StoredJob;

    #[test]
    fn saved_document_review_binds_job_and_pdf_snapshot() {
        let root = tempfile::tempdir().unwrap();
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
        let evidence = json!({"status":"failed","error":"fixture review incomplete","material":material,"document_sha256":waypoint_domain::sha256_hex(bytes)});
        record.document_visual_report(&evidence).unwrap();
        assert_eq!(
            document_visual_review(&ws, &id, attempt).unwrap()["status"],
            "failed"
        );
        assert_eq!(
            list(&ws, &id).unwrap()["attempts"][0]["document_visual_review"],
            true
        );
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
        let root = tempfile::tempdir().unwrap();
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
        assert_eq!(
            fs::read_dir(record.dir.join("screenshots"))
                .unwrap()
                .count(),
            3
        );
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
        let listing: Value =
            serde_json::from_slice(&fs::read(record.dir.join("original-job.json")).unwrap())
                .unwrap();
        assert_eq!(listing["description"], "Fictional job description.");
        assert_eq!(listing["source_url"], "https://example.org/jobs/1");
        assert_eq!(listing["description_available"], true);
        #[cfg(windows)]
        {
            assert_eq!(listing["page_capture_available"], true);
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
        let events = fs::read_to_string(record.dir.join("events.jsonl")).unwrap();
        assert!(events.contains("Sign-in required"));
    }
}
