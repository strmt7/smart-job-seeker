//! Candidate-specific assessments are never persisted in plaintext.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::SealerKind;

fn failure(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}
impl Store {
    pub fn save_fit_assessment(&self, job_id: &str, content: &[u8]) -> Result<(), StoreError> {
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let sealed = sealer.seal(content).map_err(failure)?;
        self.conn.execute("INSERT INTO fit_assessments(job_id,sealed_content) VALUES(?1,?2) ON CONFLICT(job_id) DO UPDATE SET sealed_content=excluded.sealed_content", params![job_id,sealed])?;
        Ok(())
    }
    pub fn fit_assessment(&self, job_id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let data: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT sealed_content FROM fit_assessments WHERE job_id=?1",
                [job_id],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|data| {
            let sealer = self
                .sealer
                .as_ref()
                .filter(|s| s.kind() == SealerKind::Dpapi)
                .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
            sealer.unseal(&data).map_err(failure)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredJob;
    use waypoint_domain::{ApplicationState, JobIdentityId};
    #[test]
    fn assessment_requires_dpapi_and_roundtrips_sealed() {
        let plain = Store::in_memory().unwrap();
        assert!(plain
            .save_fit_assessment("fictional", b"private fit")
            .is_err());
        let Some(sealer) =
            waypoint_seal::platform_sealer().filter(|s| s.kind() == SealerKind::Dpapi)
        else {
            return;
        };
        let mut store = Store::in_memory().unwrap().with_sealer(sealer);
        let id = JobIdentityId::new("fictional").unwrap();
        store
            .upsert_job(&StoredJob {
                id: id.clone(),
                title: "Role".into(),
                employer: "Example".into(),
                location: "".into(),
                state: ApplicationState::Discovered,
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
        let content = b"private fit assessment";
        store.save_fit_assessment(id.as_str(), content).unwrap();
        assert_eq!(
            store.fit_assessment(id.as_str()).unwrap().as_deref(),
            Some(content.as_slice())
        );
        let raw: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_content FROM fit_assessments WHERE job_id=?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!raw.windows(content.len()).any(|v| v == content));
    }
}
