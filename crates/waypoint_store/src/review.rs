//! Explicit manual-review queue, independent of submission state.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualReview {
    pub job_id: String,
    pub reason: String,
    pub updated_at: i64,
    pub revision: i64,
    pub reason_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReviewReason {
    pub reason: String,
    pub revision: i64,
}

impl Store {
    pub fn mark_manual_review(
        &self,
        job_id: &str,
        reason: &str,
        timestamp: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO manual_reviews(job_id,reason,updated_at) VALUES(?1,?2,?3) ON CONFLICT(job_id) DO UPDATE SET reason=excluded.reason, updated_at=excluded.updated_at", params![job_id, reason, timestamp])?;
        Ok(())
    }
    pub fn manual_reviews(&self) -> Result<Vec<ManualReview>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.job_id, COALESCE((SELECT group_concat(reason, char(10)) FROM (SELECT reason FROM manual_review_reasons WHERE job_id=m.job_id ORDER BY updated_at DESC, reason)), m.reason), m.updated_at, COALESCE((SELECT MAX(revision) FROM manual_review_reasons WHERE job_id=m.job_id),0), (SELECT COUNT(*) FROM manual_review_reasons WHERE job_id=m.job_id) FROM manual_reviews m ORDER BY m.updated_at DESC,m.job_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ManualReview {
                job_id: r.get(0)?,
                reason: r.get(1)?,
                updated_at: r.get(2)?,
                revision: r.get(3)?,
                reason_count: r.get(4)?,
            })
        })?;
        rows.collect()
    }
    pub fn manual_review_reasons(&self, job_id: &str) -> Result<Vec<ReviewReason>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT reason,revision FROM manual_review_reasons WHERE job_id=?1 ORDER BY updated_at DESC, reason LIMIT 20")?;
        let rows = stmt.query_map([job_id], |row| {
            Ok(ReviewReason {
                reason: row.get(0)?,
                revision: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// Resolve one exact reason atomically, retaining the other review reasons.
    pub fn clear_manual_review_reason(
        &self,
        job_id: &str,
        reason: &str,
        revision: i64,
    ) -> Result<bool, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let removed = tx.execute(
            "DELETE FROM manual_review_reasons WHERE job_id=?1 AND reason=?2 AND revision=?3",
            params![job_id, reason, revision],
        )? > 0;
        if removed {
            let next: Option<(String, i64)> = tx.query_row(
                "SELECT reason,updated_at FROM manual_review_reasons WHERE job_id=?1 ORDER BY updated_at DESC,reason LIMIT 1",
                [job_id], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            if let Some((next_reason, timestamp)) = next {
                tx.execute(
                    "UPDATE manual_reviews SET reason=?2,updated_at=?3 WHERE job_id=?1",
                    params![job_id, next_reason, timestamp],
                )?;
            } else {
                tx.execute("DELETE FROM manual_reviews WHERE job_id=?1", [job_id])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Clear the complete queue only if none of its reasons changed since review.
    pub fn clear_manual_review_if_current(
        &self,
        job_id: &str,
        revision: i64,
    ) -> Result<bool, StoreError> {
        let count = self.conn.execute(
            "DELETE FROM manual_reviews WHERE job_id=?1 AND EXISTS(SELECT 1 FROM manual_review_reasons WHERE job_id=?1) AND (SELECT MAX(revision) FROM manual_review_reasons WHERE job_id=?1)=?2",
            params![job_id, revision],
        )?;
        Ok(count > 0)
    }

    pub fn clear_manual_review(&self, job_id: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM manual_reviews WHERE job_id=?1", [job_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredJob;
    use waypoint_domain::{ApplicationState, JobIdentityId};

    #[test]
    fn migration_preserves_and_accumulates_bounded_review_reasons() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        for migration in &crate::MIGRATIONS[..21] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES('legacy','Role','Fixture','','\"shortlisted\"')", []).unwrap();
        conn.execute("INSERT INTO manual_reviews(job_id,reason,updated_at) VALUES('legacy','Existing reason',1)", []).unwrap();
        let store = Store::migrate(conn).unwrap();
        store
            .mark_manual_review("legacy", "Another reason", 2)
            .unwrap();
        store
            .mark_manual_review("legacy", "Another reason", 3)
            .unwrap();
        let reviews = store.manual_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].reason_count, 2);
        assert_eq!(reviews[0].reason, "Another reason\nExisting reason");
        for index in 0..18 {
            store
                .mark_manual_review("legacy", &format!("Reason {index}"), 4 + index)
                .unwrap();
        }
        assert!(store
            .mark_manual_review("legacy", "Over the limit", 99)
            .is_err());
        let review = &store.manual_reviews().unwrap()[0];
        assert!(review.reason.contains("Existing reason"));
        assert!(!review.reason.contains("Over the limit"));
        store
            .mark_manual_review("legacy", "Existing reason", 100)
            .unwrap();
        assert!(!store
            .clear_manual_review_reason("legacy", "Not present", 0)
            .unwrap());
        let current = store
            .manual_review_reasons("legacy")
            .unwrap()
            .into_iter()
            .find(|reason| reason.reason == "Another reason")
            .unwrap();
        assert!(store
            .clear_manual_review_reason("legacy", &current.reason, current.revision)
            .unwrap());
        let reasons = store.manual_review_reasons("legacy").unwrap();
        assert_eq!(reasons.len(), 19);
        assert!(!reasons
            .iter()
            .any(|reason| reason.reason == "Another reason"));
        assert!(reasons
            .iter()
            .any(|reason| reason.reason == "Existing reason"));
        store
            .mark_manual_review("legacy", "Over the limit", 101)
            .unwrap();
        while let Some(reason) = store
            .manual_review_reasons("legacy")
            .unwrap()
            .into_iter()
            .next()
        {
            assert!(store
                .clear_manual_review_reason("legacy", &reason.reason, reason.revision)
                .unwrap());
        }
        assert!(store.manual_reviews().unwrap().is_empty());
        store
            .mark_manual_review("legacy", "Repeated issue", 102)
            .unwrap();
        let old = store.manual_review_reasons("legacy").unwrap().remove(0);
        store
            .mark_manual_review("legacy", "Repeated issue", 102)
            .unwrap();
        assert!(!store
            .clear_manual_review_reason("legacy", &old.reason, old.revision)
            .unwrap());
        let recent = store.manual_review_reasons("legacy").unwrap().remove(0);
        assert!(store
            .clear_manual_review_reason("legacy", &recent.reason, recent.revision)
            .unwrap());
        store
            .mark_manual_review("legacy", "Repeated issue", 102)
            .unwrap();
        assert!(!store
            .clear_manual_review_reason("legacy", &old.reason, old.revision)
            .unwrap());
        let stale_queue = store.manual_reviews().unwrap()[0].revision;
        store
            .mark_manual_review("legacy", "New issue", 102)
            .unwrap();
        assert!(!store
            .clear_manual_review_if_current("legacy", stale_queue)
            .unwrap());
        assert_eq!(store.manual_review_reasons("legacy").unwrap().len(), 2);
        let current_queue = store.manual_reviews().unwrap()[0].revision;
        assert!(store
            .clear_manual_review_if_current("legacy", current_queue)
            .unwrap());
        store
            .mark_manual_review("legacy", "Repeated issue", 102)
            .unwrap();
        assert!(!store
            .clear_manual_review_if_current("legacy", current_queue)
            .unwrap());
        store.clear_manual_review("legacy").unwrap();
        assert!(store.manual_reviews().unwrap().is_empty());
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM manual_review_reasons", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn revision_migration_preserves_v22_reasons() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        for migration in &crate::MIGRATIONS[..22] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES('legacy22','Role','Fixture','','\"shortlisted\"')", []).unwrap();
        conn.execute("INSERT INTO manual_reviews(job_id,reason,updated_at) VALUES('legacy22','Existing issue',1)", []).unwrap();
        let store = Store::migrate(conn).unwrap();
        let old = store.manual_review_reasons("legacy22").unwrap().remove(0);
        assert_eq!(old.reason, "Existing issue");
        assert_eq!(old.revision, 0);
        store
            .mark_manual_review("legacy22", "Existing issue", 1)
            .unwrap();
        assert!(!store
            .clear_manual_review_reason("legacy22", &old.reason, old.revision)
            .unwrap());
        assert_eq!(store.manual_review_reasons("legacy22").unwrap().len(), 1);
    }

    #[test]
    #[cfg(feature = "sqlcipher")]
    fn encrypted_disk_migration_preserves_review_reasons() {
        let dir = std::env::temp_dir().join(format!(
            "waypoint-review-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("fixture.sqlite");
        let key = [12u8; 32];
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(&format!(
                "PRAGMA key = '{}'; PRAGMA foreign_keys=ON;",
                "0c".repeat(32)
            ))
            .unwrap();
            for migration in &crate::MIGRATIONS[..21] {
                conn.execute_batch(migration).unwrap();
            }
            conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES('legacy-disk','Role','Fixture','','\"shortlisted\"')", []).unwrap();
            conn.execute("INSERT INTO manual_reviews(job_id,reason,updated_at) VALUES('legacy-disk','Existing reason',1)", []).unwrap();
        }
        {
            let store = Store::open_encrypted(&path, &key).unwrap();
            store
                .mark_manual_review("legacy-disk", "New reason", 2)
                .unwrap();
            assert_eq!(store.manual_reviews().unwrap()[0].reason_count, 2);
        }
        assert!(!std::fs::read(&path)
            .unwrap()
            .starts_with(b"SQLite format 3\0"));
        assert!(Store::open_encrypted(&path, &[13u8; 32]).is_err());
        let reopened = Store::open_encrypted(&path, &key).unwrap();
        let review = reopened.manual_reviews().unwrap().remove(0);
        assert_eq!(review.reason_count, 2);
        assert!(review.reason.contains("Existing reason") && review.reason.contains("New reason"));
        drop(reopened);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn review_queue_does_not_change_application_state() {
        let mut store = Store::in_memory().unwrap();
        let id = JobIdentityId::new("fixture").unwrap();
        store
            .upsert_job(&StoredJob {
                id: id.clone(),
                title: "Role".into(),
                employer: "Fixture".into(),
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
            })
            .unwrap();
        store
            .mark_manual_review(id.as_str(), "Manual review requested", 1)
            .unwrap();
        assert_eq!(store.manual_reviews().unwrap()[0].job_id, id.as_str());
        assert_eq!(
            store.get_job(&id).unwrap().unwrap().state,
            ApplicationState::Shortlisted
        );
        store.clear_manual_review(id.as_str()).unwrap();
        assert!(store.manual_reviews().unwrap().is_empty());
        assert_eq!(
            store.get_job(&id).unwrap().unwrap().state,
            ApplicationState::Shortlisted
        );
    }
}
