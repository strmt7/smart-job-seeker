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

/// Rebuild the packet from its saved review details and materials before
/// returning those details. This is a review aid, not submission approval.
pub fn gmail_review(ws: &Workspace, job: &JobIdentityId, attempt: &str) -> Result<Value, String> {
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
    serde_json::to_value(review).map_err(|_| "Cannot display saved Gmail review".into())
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
        attempts.push(json!({"attempt_id":attempt_id,"status":status,"started_at":started,"folder":entry.path().to_string_lossy(),"submitted":value["submitted"],"submitted_by_waypoint":value["submitted_by_waypoint"],"gmail_review":gmail_review}));
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
