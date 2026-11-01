//! Bounded storage for reviewed form experiences. Content is opaque here;
//! callers must validate its meaning before using it as untrusted guidance.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::SealerKind;

const MAX_EXPERIENCES: usize = 500;
const MAX_CONTENT: usize = 16 * 1024;

fn failure(kind: std::io::ErrorKind, message: &'static str) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(std::io::Error::new(kind, message)))
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Store {
    /// Create with expected revision zero; update only the exact revision read.
    pub fn save_form_experience(
        &self,
        id: &str,
        expected: u64,
        content: &[u8],
    ) -> Result<u64, StoreError> {
        if !valid_id(id)
            || content.is_empty()
            || content.len() > MAX_CONTENT
            || expected >= i64::MAX as u64
        {
            return Err(failure(
                std::io::ErrorKind::InvalidInput,
                "Invalid form experience",
            ));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| {
                StoreError::ToSqlConversionFailure(Box::new(waypoint_seal::SealError::Unavailable))
            })?;
        let sealed = sealer
            .seal(content)
            .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
        let changed = if expected == 0 {
            self.conn.execute("INSERT INTO form_experiences(id,revision,sealed_content) SELECT ?1,1,?2 WHERE (SELECT COUNT(*) FROM form_experiences WHERE sealed_content IS NOT NULL)< ?3 AND (SELECT COUNT(*) FROM form_experiences)<10000 ON CONFLICT(id) DO NOTHING", params![id,sealed,MAX_EXPERIENCES as i64])?
        } else {
            self.conn.execute("UPDATE form_experiences SET revision=revision+1,sealed_content=?1 WHERE id=?2 AND revision=?3 AND sealed_content IS NOT NULL", params![sealed,id,expected])?
        };
        if changed != 1 {
            return Err(failure(
                std::io::ErrorKind::AlreadyExists,
                "Experience changed, is missing, or the limit was reached",
            ));
        }
        Ok(expected + 1)
    }

    /// Read a single record. The caller must treat its contents as untrusted.
    pub fn form_experience(&self, id: &str) -> Result<Option<(u64, Vec<u8>)>, StoreError> {
        if !valid_id(id) {
            return Err(failure(
                std::io::ErrorKind::InvalidInput,
                "Invalid experience ID",
            ));
        }
        let row: Option<(u64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT revision,sealed_content FROM form_experiences WHERE id=?1 AND sealed_content IS NOT NULL",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(revision, sealed)| {
            let sealer = self
                .sealer
                .as_ref()
                .filter(|s| s.kind() == SealerKind::Dpapi)
                .ok_or_else(|| {
                    StoreError::ToSqlConversionFailure(Box::new(
                        waypoint_seal::SealError::Unavailable,
                    ))
                })?;
            let content = sealer
                .unseal(&sealed)
                .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
            if content.is_empty() || content.len() > MAX_CONTENT {
                return Err(failure(
                    std::io::ErrorKind::InvalidData,
                    "Invalid stored form experience",
                ));
            }
            Ok((revision, content))
        })
        .transpose()
    }

    /// List opaque identifiers for subsequent bounded, individually validated reads.
    pub fn form_experience_ids(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM form_experiences WHERE sealed_content IS NOT NULL ORDER BY id LIMIT 501")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if ids.len() > MAX_EXPERIENCES || ids.iter().any(|id| !valid_id(id)) {
            return Err(failure(
                std::io::ErrorKind::InvalidData,
                "Invalid experience inventory",
            ));
        }
        Ok(ids)
    }

    /// Revoke without permitting ID reuse; this is not secure erasure of old pages or backups.
    pub fn delete_form_experience(&self, id: &str, expected: u64) -> Result<bool, StoreError> {
        if !valid_id(id) || expected == 0 || expected > i64::MAX as u64 {
            return Err(failure(
                std::io::ErrorKind::InvalidInput,
                "Invalid experience revision",
            ));
        }
        Ok(self.conn.execute(
            "UPDATE form_experiences SET sealed_content=NULL WHERE id=?1 AND revision=?2 AND sealed_content IS NOT NULL",
            params![id, expected],
        )? == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unsealed_and_invalid_data() {
        let store = Store::in_memory().unwrap();
        assert!(store
            .save_form_experience("00000000000000000000000000000001", 0, b"fictional")
            .is_err());
        assert!(store
            .save_form_experience("bad id", 0, b"fictional")
            .is_err());
        assert!(store
            .save_form_experience(
                "00000000000000000000000000000001",
                0,
                &vec![0; MAX_CONTENT + 1]
            )
            .is_err());
        assert!(store.form_experience_ids().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn sealed_revision_and_revocation() {
        let store = Store::in_memory()
            .unwrap()
            .with_sealer(waypoint_seal::platform_sealer().unwrap());
        assert_eq!(
            store
                .save_form_experience(
                    "00000000000000000000000000000001",
                    0,
                    b"fictional correction"
                )
                .unwrap(),
            1
        );
        let bytes: Vec<u8> = store
            .conn
            .query_row("SELECT sealed_content FROM form_experiences", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(waypoint_seal::is_sealed(&bytes));
        assert_eq!(
            store
                .form_experience("00000000000000000000000000000001")
                .unwrap()
                .unwrap()
                .1,
            b"fictional correction"
        );
        assert_eq!(
            store.form_experience_ids().unwrap(),
            vec!["00000000000000000000000000000001"]
        );
        assert_eq!(
            store
                .save_form_experience("00000000000000000000000000000001", 1, b"updated")
                .unwrap(),
            2
        );
        assert!(store
            .save_form_experience("00000000000000000000000000000001", 1, b"stale")
            .is_err());
        assert!(!store
            .delete_form_experience("00000000000000000000000000000001", 1)
            .unwrap());
        assert!(store
            .delete_form_experience("00000000000000000000000000000001", 2)
            .unwrap());
        assert!(store
            .form_experience("00000000000000000000000000000001")
            .unwrap()
            .is_none());
        assert!(store
            .save_form_experience("00000000000000000000000000000001", 0, b"reused")
            .is_err());
        assert!(store
            .save_form_experience("00000000000000000000000000000001", 2, b"revived")
            .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn capacity_does_not_block_revision_or_delete() {
        let store = Store::in_memory()
            .unwrap()
            .with_sealer(waypoint_seal::platform_sealer().unwrap());
        for i in 0..MAX_EXPERIENCES {
            store
                .conn
                .execute(
                    "INSERT INTO form_experiences(id,revision,sealed_content) VALUES(?1,1,X'00')",
                    [format!("{i:032x}")],
                )
                .unwrap();
        }
        assert!(store
            .save_form_experience("ffffffffffffffffffffffffffffffff", 0, b"fictional")
            .is_err());
        assert_eq!(
            store
                .save_form_experience("00000000000000000000000000000000", 1, b"revised")
                .unwrap(),
            2
        );
        assert!(store
            .delete_form_experience("00000000000000000000000000000000", 2)
            .unwrap());
        assert_eq!(
            store
                .save_form_experience("ffffffffffffffffffffffffffffffff", 0, b"fictional")
                .unwrap(),
            1
        );
    }
}
