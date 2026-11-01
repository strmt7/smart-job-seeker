//! Individually identified generated drafts, sealed before database writes.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::SealerKind;

fn failure(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}

impl Store {
    pub fn save_generated_draft(
        &self,
        id: &str,
        job_id: &str,
        kind: &str,
        created_at: i64,
        content: &[u8],
    ) -> Result<(), StoreError> {
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !matches!(kind, "cv" | "cover_letter")
            || content.is_empty()
            || content.len() > 6 * 1024 * 1024
        {
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid generated draft",
            )));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let existing: Option<(String, String, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT job_id,kind,sealed_content FROM generated_drafts WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((old_job, old_kind, bytes)) = existing {
            if old_job == job_id
                && old_kind == kind
                && sealer.unseal(&bytes).map_err(failure)? == content
            {
                return Ok(());
            }
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Draft identity already exists",
            )));
        }
        let sealed = sealer.seal(content).map_err(failure)?;
        self.conn.execute("INSERT INTO generated_drafts(id,job_id,kind,created_at,sealed_content) VALUES(?1,?2,?3,?4,?5)", params![id,job_id,kind,created_at,sealed])?;
        Ok(())
    }
    pub fn generated_draft(&self, id: &str, job_id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let data: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT sealed_content FROM generated_drafts WHERE id=?1 AND job_id=?2",
                params![id, job_id],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|bytes| {
            self.sealer
                .as_ref()
                .filter(|s| s.kind() == SealerKind::Dpapi)
                .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?
                .unseal(&bytes)
                .map_err(failure)
        })
        .transpose()
    }
    pub fn generated_drafts(&self, job_id: &str) -> Result<Vec<(String, String, i64)>, StoreError> {
        let mut statement = self.conn.prepare("SELECT id,kind,created_at FROM generated_drafts WHERE job_id=?1 ORDER BY created_at DESC,id DESC LIMIT 51")?;
        let rows =
            statement.query_map([job_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredJob;
    use waypoint_domain::{ApplicationState, JobIdentityId};
    #[test]
    fn drafts_require_sealing_and_are_idempotent_and_job_bound() {
        let plain = Store::in_memory().unwrap();
        assert!(plain
            .save_generated_draft("one", "job", "cv", 1, b"private draft")
            .is_err());
        let Some(sealer) =
            waypoint_seal::platform_sealer().filter(|s| s.kind() == SealerKind::Dpapi)
        else {
            return;
        };
        let mut store = Store::in_memory().unwrap().with_sealer(sealer);
        let job = JobIdentityId::new("fictional-job").unwrap();
        store
            .upsert_job(&StoredJob {
                id: job.clone(),
                title: "Role".into(),
                employer: "Example".into(),
                location: String::new(),
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
        let content = b"fictional private generated draft";
        store
            .save_generated_draft("one", job.as_str(), "cv", 1, content)
            .unwrap();
        store
            .save_generated_draft("one", job.as_str(), "cv", 2, content)
            .unwrap();
        assert!(store
            .save_generated_draft("one", job.as_str(), "cv", 2, b"different")
            .is_err());
        assert!(store
            .save_generated_draft("../bad", job.as_str(), "cv", 1, content)
            .is_err());
        assert_eq!(
            store
                .generated_draft("one", job.as_str())
                .unwrap()
                .as_deref(),
            Some(content.as_slice())
        );
        assert!(store
            .generated_draft("one", "another-job")
            .unwrap()
            .is_none());
        assert_eq!(store.generated_drafts(job.as_str()).unwrap().len(), 1);
        assert!(store.has_sealed_material().unwrap());
        let raw: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_content FROM generated_drafts WHERE id='one'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!raw.windows(content.len()).any(|part| part == content));
    }
}
