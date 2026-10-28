//! Durable, one-use prefill authorization records, independent of submission.
use crate::{Store, StoreError};
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefillGrantRecord {
    pub id: String,
    pub job_id: String,
    pub intent_id: String,
    pub payload_hash: String,
    pub profile_hash: String,
    pub schema_hash: String,
    pub destination: String,
    pub created_at: i64,
}

impl Store {
    /// Record and consume before the first field is written. No persisted
    /// record can be resurrected as an unconsumed grant after a restart.
    pub fn record_consumed_prefill_grant(
        &mut self,
        grant: &PrefillGrantRecord,
    ) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO prefill_grants(id,job_id,intent_id,payload_hash,profile_hash,schema_hash,destination,created_at,consumed) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1)", params![grant.id,grant.job_id,grant.intent_id,grant.payload_hash,grant.profile_hash,grant.schema_hash,grant.destination,grant.created_at])?;
        tx.commit()?;
        Ok(())
    }

    pub fn prefill_grant_consumed(&self, id: &str) -> Result<Option<bool>, StoreError> {
        use rusqlite::OptionalExtension;
        self.conn
            .query_row(
                "SELECT consumed FROM prefill_grants WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredJob;
    use waypoint_domain::{ApplicationState, JobIdentityId};

    #[test]
    fn consumed_grant_is_one_use_and_does_not_change_submission_state() {
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
        let grant = PrefillGrantRecord {
            id: "g1".into(),
            job_id: "fixture".into(),
            intent_id: "intent".into(),
            payload_hash: "payload".into(),
            profile_hash: "profile".into(),
            schema_hash: "schema".into(),
            destination: "https://example.test".into(),
            created_at: 1,
        };
        store.record_consumed_prefill_grant(&grant).unwrap();
        assert_eq!(store.prefill_grant_consumed("g1").unwrap(), Some(true));
        assert!(store.record_consumed_prefill_grant(&grant).is_err());
        assert_eq!(
            store.get_job(&id).unwrap().unwrap().state,
            ApplicationState::Shortlisted
        );
    }
}
