//! Candidate profile persisted using the workspace sealing backend.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};

fn conversion(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}

impl Store {
    pub fn save_candidate_profile(&self, content: &str) -> Result<(), StoreError> {
        let (data, sealed) = match &self.sealer {
            Some(sealer) => (sealer.seal(content.as_bytes()).map_err(conversion)?, true),
            None => (content.as_bytes().to_vec(), false),
        };
        self.conn.execute("INSERT INTO candidate_profile(id,content,sealed) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET content=excluded.content,sealed=excluded.sealed", params![data,sealed])?;
        Ok(())
    }

    /// Atomically replace the profile only if the caller's snapshot still matches.
    pub fn save_candidate_profile_if_current(
        &self,
        expected_digest: &str,
        content: &str,
    ) -> Result<bool, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let current = self.candidate_profile()?;
        let digest = waypoint_domain::sha256_hex(current.as_deref().unwrap_or("").as_bytes());
        if digest != expected_digest {
            tx.rollback()?;
            return Ok(false);
        }
        self.save_candidate_profile(content)?;
        tx.commit()?;
        Ok(true)
    }

    pub fn candidate_profile(&self) -> Result<Option<String>, StoreError> {
        let row: Option<(Vec<u8>, bool)> = self
            .conn
            .query_row(
                "SELECT content,sealed FROM candidate_profile WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(data, sealed)| {
            let content = if sealed {
                self.sealer
                    .as_ref()
                    .ok_or_else(|| conversion(waypoint_seal::SealError::Unavailable))?
                    .unseal(&data)
                    .map_err(conversion)?
            } else {
                data
            };
            String::from_utf8(content).map_err(conversion)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_roundtrip_and_replace() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.candidate_profile().unwrap(), None);
        store.save_candidate_profile("first").unwrap();
        store.save_candidate_profile("second").unwrap();
        assert_eq!(
            store.candidate_profile().unwrap().as_deref(),
            Some("second")
        );
    }

    #[test]
    fn profile_compare_and_save_refuses_stale_snapshots() {
        let store = Store::in_memory().unwrap();
        let empty = waypoint_domain::sha256_hex(b"");
        assert!(store
            .save_candidate_profile_if_current(&empty, "first")
            .unwrap());
        assert!(!store
            .save_candidate_profile_if_current(&empty, "stale")
            .unwrap());
        assert_eq!(store.candidate_profile().unwrap().as_deref(), Some("first"));
        let first = waypoint_domain::sha256_hex(b"first");
        assert!(store
            .save_candidate_profile_if_current(&first, "second")
            .unwrap());
        assert!(!store
            .save_candidate_profile_if_current(&first, "stale")
            .unwrap());
        assert_eq!(
            store.candidate_profile().unwrap().as_deref(),
            Some("second")
        );
    }

    #[cfg(windows)]
    #[test]
    fn profile_migrates_to_dpapi() {
        let mut store = Store::in_memory().unwrap();
        store.save_candidate_profile("private").unwrap();
        store = store.with_sealer(waypoint_seal::platform_sealer().unwrap());
        assert_eq!(store.seal_existing_rows().unwrap(), 1);
        let (content, sealed): (Vec<u8>, bool) = store
            .conn
            .query_row(
                "SELECT content,sealed FROM candidate_profile WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(sealed && waypoint_seal::is_sealed(&content));
        assert_eq!(
            store.candidate_profile().unwrap().as_deref(),
            Some("private")
        );
        let expected = waypoint_domain::sha256_hex(b"private");
        assert!(store
            .save_candidate_profile_if_current(&expected, "revised private")
            .unwrap());
        assert!(!store
            .save_candidate_profile_if_current(&expected, "stale private")
            .unwrap());
        assert_eq!(
            store.candidate_profile().unwrap().as_deref(),
            Some("revised private")
        );
    }
}
