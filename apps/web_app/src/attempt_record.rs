//! Local, human-readable preparation-attempt evidence. Records are plaintext.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{fs, io::Write, path::PathBuf};
use waypoint_domain::{JobIdentityId, Sha256Digest};
use waypoint_workspace::Workspace;

pub struct AttemptRecord {
    dir: PathBuf,
    sequence: u32,
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
        let meta = json!({"attempt_id":attempt_id,"job_id":id.as_str(),"employer":job.employer,"title":job.title,"job_url":job.canonical_url,"started_at":timestamp,"status":"preparation_in_progress","submitted":false,"note":"This record does not prove submission."});
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
    pub fn email_draft(&mut self, bytes: &[u8]) -> Result<(), String> {
        fs::write(self.dir.join("email-draft.eml"), bytes).map_err(|e| e.to_string())
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
        let record = AttemptRecord::start(&ws, &id, "attempt-fixture", 123)
            .unwrap()
            .unwrap();
        let listing: Value =
            serde_json::from_slice(&fs::read(record.dir.join("original-job.json")).unwrap())
                .unwrap();
        assert_eq!(listing["description"], "Fictional job description.");
        assert_eq!(listing["source_url"], "https://example.org/jobs/1");
        assert_eq!(listing["description_available"], true);
        assert_eq!(
            fs::read_to_string(record.dir.join("original-job.txt")).unwrap(),
            "Fictional job description."
        );
        assert_eq!(
            listing["description_sha256"],
            Sha256Digest::of("Fictional job description.").as_str()
        );
    }
}
