//! One sealed, revisioned local-model choice; never part of candidate facts.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::SealerKind;

fn invalid(message: &'static str) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message,
    )))
}

impl Store {
    pub fn model_preferences(&self) -> Result<(u64, Option<Vec<u8>>), StoreError> {
        let row: Option<(u64, Option<Vec<u8>>)> = self
            .conn
            .query_row(
                "SELECT revision,sealed_content FROM model_preferences WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((revision, content)) = row else {
            return Ok((0, None));
        };
        let content = content
            .map(|bytes| {
                if bytes.len() > 64 * 1024 {
                    return Err(invalid("Sealed model settings exceed their bounds"));
                }
                self.sealer
                    .as_ref()
                    .filter(|s| s.kind() == SealerKind::Dpapi)
                    .ok_or_else(|| invalid("Model settings need Windows user encryption"))?
                    .unseal(&bytes)
                    .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))
            })
            .transpose()?;
        Ok((revision, content))
    }

    pub fn save_model_preferences(
        &self,
        expected: u64,
        content: Option<&[u8]>,
    ) -> Result<Option<u64>, StoreError> {
        if expected >= i64::MAX as u64
            || content.is_some_and(|bytes| bytes.is_empty() || bytes.len() > 8192)
        {
            return Err(invalid("Model settings exceed their bounds"));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| invalid("Model settings need Windows user encryption"))?;
        let sealed = content
            .map(|bytes| {
                sealer
                    .seal(bytes)
                    .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))
            })
            .transpose()?;
        let changed = if expected == 0 {
            self.conn.execute("INSERT INTO model_preferences(id,revision,sealed_content) VALUES(1,1,?1) ON CONFLICT(id) DO NOTHING", params![sealed])?
        } else {
            self.conn.execute("UPDATE model_preferences SET revision=revision+1,sealed_content=?1 WHERE id=1 AND revision=?2",params![sealed,expected])?
        };
        Ok((changed == 1).then_some(expected + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_encryption_and_oversized_settings_are_refused() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.model_preferences().unwrap(), (0, None));
        assert!(store.save_model_preferences(0, Some(b"fictional")).is_err());
        assert!(store
            .save_model_preferences(0, Some(&vec![0; 8193]))
            .is_err());
        assert_eq!(store.model_preferences().unwrap(), (0, None));
    }

    #[cfg(windows)]
    #[test]
    fn choice_is_sealed_and_clear_preserves_revision_without_touching_profile() {
        let store = Store::in_memory()
            .unwrap()
            .with_sealer(waypoint_seal::platform_sealer().unwrap());
        store.save_candidate_profile("candidate facts").unwrap();
        assert_eq!(
            store
                .save_model_preferences(0, Some(b"fictional model"))
                .unwrap(),
            Some(1)
        );
        let raw: Vec<u8> = store
            .conn
            .query_row("SELECT sealed_content FROM model_preferences", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(waypoint_seal::is_sealed(&raw));
        assert!(!raw
            .windows(b"fictional model".len())
            .any(|bytes| bytes == b"fictional model"));
        assert_eq!(
            store.model_preferences().unwrap(),
            (1, Some(b"fictional model".to_vec()))
        );
        assert_eq!(
            store.save_model_preferences(0, Some(b"stale")).unwrap(),
            None
        );
        assert_eq!(store.save_model_preferences(1, None).unwrap(), Some(2));
        assert_eq!(store.model_preferences().unwrap(), (2, None));
        assert_eq!(
            store
                .save_model_preferences(0, Some(b"stale after clear"))
                .unwrap(),
            None
        );
        assert_eq!(
            store.candidate_profile().unwrap().as_deref(),
            Some("candidate facts")
        );
    }
}
