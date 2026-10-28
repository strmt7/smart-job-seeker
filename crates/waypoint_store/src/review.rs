//! Explicit manual-review queue, independent of submission state.
use crate::{Store, StoreError};
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualReview {
    pub job_id: String,
    pub reason: String,
    pub updated_at: i64,
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
            "SELECT job_id,reason,updated_at FROM manual_reviews ORDER BY updated_at DESC,job_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ManualReview {
                job_id: r.get(0)?,
                reason: r.get(1)?,
                updated_at: r.get(2)?,
            })
        })?;
        rows.collect()
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
