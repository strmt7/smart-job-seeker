//! One bounded, user-sealed web-search result for recovery after restart.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::SealerKind;

const MAX_BYTES: usize = 12 * 1024 * 1024;
fn failure(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}
impl Store {
    pub fn save_selected_web_candidates(
        &self,
        expected: u64,
        content: &[u8],
    ) -> Result<u64, StoreError> {
        if content.is_empty() || content.len() > 300 * 1024 {
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid or oversized candidate selection",
            )));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let sealed = sealer.seal(content).map_err(failure)?;
        let changed = if expected == 0 {
            self.conn.execute("INSERT OR IGNORE INTO selected_web_candidates(id,revision,sealed_content) VALUES(1,1,?1)", [sealed])?
        } else {
            self.conn.execute("UPDATE selected_web_candidates SET revision=revision+1,sealed_content=?1 WHERE id=1 AND revision=?2 AND revision<9223372036854775807", params![sealed,expected])?
        };
        if changed != 1 {
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Selection changed in another window; reload before saving",
            )));
        }
        Ok(expected + 1)
    }
    pub fn selected_web_candidates(&self) -> Result<Option<(u64, Vec<u8>)>, StoreError> {
        let data: Option<(u64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT revision,sealed_content FROM selected_web_candidates WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        data.map(|(revision, sealed)| {
            let sealer = self
                .sealer
                .as_ref()
                .filter(|s| s.kind() == SealerKind::Dpapi)
                .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
            Ok((revision, sealer.unseal(&sealed).map_err(failure)?))
        })
        .transpose()
    }
    pub fn save_latest_web_search(&self, task_id: &str, content: &[u8]) -> Result<(), StoreError> {
        if task_id.is_empty()
            || task_id.len() > 128
            || !task_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || content.is_empty()
            || content.len() > MAX_BYTES
        {
            return Err(failure(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid or oversized web search history",
            )));
        }
        let sealer = self
            .sealer
            .as_ref()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let sealed = sealer.seal(content).map_err(failure)?;
        self.conn.execute("INSERT INTO latest_web_search(id,task_id,sealed_content) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET task_id=excluded.task_id,sealed_content=excluded.sealed_content", params![task_id,sealed])?;
        Ok(())
    }
    pub fn latest_web_search(&self) -> Result<Option<(String, Vec<u8>)>, StoreError> {
        let row: Option<(String, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT task_id,sealed_content FROM latest_web_search WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(id, sealed)| {
            let sealer = self
                .sealer
                .as_ref()
                .filter(|s| s.kind() == SealerKind::Dpapi)
                .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
            Ok((id, sealer.unseal(&sealed).map_err(failure)?))
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_requires_user_sealing_and_replaces_only_the_latest_result() {
        let plain = Store::in_memory().unwrap();
        assert!(plain
            .save_latest_web_search("one", b"private query")
            .is_err());
        let Some(sealer) =
            waypoint_seal::platform_sealer().filter(|s| s.kind() == SealerKind::Dpapi)
        else {
            return;
        };
        let store = Store::in_memory().unwrap().with_sealer(sealer);
        store
            .save_latest_web_search("one", b"private query")
            .unwrap();
        let raw: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_content FROM latest_web_search WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!raw
            .windows(b"private query".len())
            .any(|value| value == b"private query"));
        assert!(store
            .save_latest_web_search("bad id", b"replacement")
            .is_err());
        assert_eq!(
            store.latest_web_search().unwrap().unwrap().1,
            b"private query"
        );
        store.save_latest_web_search("two", b"next query").unwrap();
        assert_eq!(
            store.latest_web_search().unwrap(),
            Some(("two".into(), b"next query".to_vec()))
        );
        assert!(plain.save_selected_web_candidates(0, b"[]").is_err());
        assert_eq!(
            store
                .save_selected_web_candidates(0, b"private selected role")
                .unwrap(),
            1
        );
        let selected: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_content FROM selected_web_candidates WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!selected
            .windows(b"private selected role".len())
            .any(|value| value == b"private selected role"));
        assert_eq!(
            store.selected_web_candidates().unwrap().unwrap(),
            (1, b"private selected role".to_vec())
        );
        assert!(store.save_selected_web_candidates(0, b"stale").is_err());
        assert_eq!(store.selected_web_candidates().unwrap().unwrap().0, 1);
    }
}
