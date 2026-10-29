//! Bounded, account-protected listing images; the first captured listing is retained.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::SealerKind;

pub type JobCapture = (Vec<u8>, Vec<Vec<u8>>);

fn failure(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}
fn invalid(message: &str) -> StoreError {
    failure(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.to_owned(),
    ))
}
impl Store {
    pub fn save_job_capture(
        &self,
        id: &str,
        metadata: &[u8],
        segments: &[Vec<u8>],
    ) -> Result<bool, StoreError> {
        if metadata.len() > 512 * 1024
            || segments.is_empty()
            || segments.len() > 5
            || segments
                .iter()
                .any(|p| p.len() > 2 * 1024 * 1024 || !p.starts_with(b"\x89PNG\r\n\x1a\n"))
            || segments.iter().map(Vec::len).sum::<usize>() > 6 * 1024 * 1024
        {
            return Err(invalid("Invalid or oversized listing capture"));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let encrypted_metadata = sealer.seal(metadata).map_err(failure)?;
        let transaction = self.conn.unchecked_transaction()?;
        let added=transaction.execute("INSERT INTO job_capture_metadata(job_id,sealed_metadata) VALUES(?1,?2) ON CONFLICT(job_id) DO NOTHING",params![id,encrypted_metadata])?;
        if added == 1 {
            for (index, png) in segments.iter().enumerate() {
                let sealed = sealer.seal(png).map_err(failure)?;
                transaction.execute(
                    "INSERT INTO job_capture_segments(job_id,sequence,sealed_png) VALUES(?1,?2,?3)",
                    params![id, index as i64, sealed],
                )?;
            }
        }
        transaction.commit()?;
        Ok(added == 1)
    }
    /// Read the bounded metadata without loading or decrypting screenshot segments.
    pub fn job_capture_metadata(&self, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let metadata: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT sealed_metadata FROM job_capture_metadata WHERE job_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(metadata) = metadata else {
            return Ok(None);
        };
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        Ok(Some(sealer.unseal(&metadata).map_err(failure)?))
    }

    pub fn job_capture(&self, id: &str) -> Result<Option<JobCapture>, StoreError> {
        let Some(metadata) = self.job_capture_metadata(id)? else {
            return Ok(None);
        };
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let mut statement = self.conn.prepare(
            "SELECT sealed_png FROM job_capture_segments WHERE job_id=?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([id], |r| r.get::<_, Vec<u8>>(0))?;
        let mut segments = Vec::new();
        for row in rows {
            segments.push(sealer.unseal(&row?).map_err(failure)?);
        }
        Ok(Some((metadata, segments)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredJob;
    use waypoint_domain::{ApplicationState, JobIdentityId};
    #[test]
    fn captures_require_dpapi_and_preserve_first_version() {
        let plain = Store::in_memory().unwrap();
        let png = b"\x89PNG\r\n\x1a\nfictional listing image".to_vec();
        assert!(plain
            .save_job_capture("job", b"{}", std::slice::from_ref(&png))
            .is_err());
        let Some(sealer) =
            waypoint_seal::platform_sealer().filter(|s| s.kind() == SealerKind::Dpapi)
        else {
            return;
        };
        let mut store = Store::in_memory().unwrap().with_sealer(sealer);
        let id = JobIdentityId::new("job").unwrap();
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
        let metadata = br#"{"structured_source_scripts":["fictional original listing"]}"#;
        assert!(store
            .save_job_capture("job", metadata, std::slice::from_ref(&png))
            .unwrap());
        assert!(!store
            .save_job_capture("job", b"second", std::slice::from_ref(&png))
            .unwrap());
        let (metadata, segments) = store.job_capture("job").unwrap().unwrap();
        assert_eq!(
            metadata,
            br#"{"structured_source_scripts":["fictional original listing"]}"#
        );
        assert_eq!(segments, vec![png.clone()]);
        assert_eq!(
            store.job_capture_metadata("job").unwrap().unwrap(),
            metadata
        );
        assert!(store.job_capture_metadata("missing").unwrap().is_none());
        let encrypted: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_png FROM job_capture_segments WHERE job_id='job'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!encrypted.windows(png.len()).any(|w| w == png));
        let encrypted_metadata: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_metadata FROM job_capture_metadata WHERE job_id='job'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!encrypted_metadata
            .windows(b"fictional original listing".len())
            .any(|w| w == b"fictional original listing"));
        store
            .conn
            .execute(
                "UPDATE job_capture_segments SET sealed_png=?1 WHERE job_id='job'",
                [b"invalid".as_slice()],
            )
            .unwrap();
        assert_eq!(
            store.job_capture_metadata("job").unwrap().unwrap(),
            metadata
        );
        assert!(store.job_capture("job").is_err());
    }
}
