//! Local candidate document storage using the workspace's active sealer.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentInfo {
    pub id: String,
    pub name: String,
    pub size: usize,
    pub created_at: i64,
}

fn conversion(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}

impl Store {
    pub fn add_document(
        &self,
        id: &str,
        name: &str,
        bytes: &[u8],
        created_at: i64,
    ) -> Result<(), StoreError> {
        let (name, content, sealed) = match &self.sealer {
            Some(sealer) => (
                sealer.seal(name.as_bytes()).map_err(conversion)?,
                sealer.seal(bytes).map_err(conversion)?,
                1,
            ),
            None => (name.as_bytes().to_vec(), bytes.to_vec(), 0),
        };
        self.conn.execute("INSERT INTO documents(id,name,content,sealed,size,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![id, name, content, sealed, bytes.len() as i64, created_at])?;
        Ok(())
    }

    fn document_name(&self, bytes: &[u8], sealed: bool) -> Result<String, StoreError> {
        let raw = if sealed {
            self.sealer
                .as_ref()
                .ok_or_else(|| conversion(waypoint_seal::SealError::Unavailable))?
                .unseal(bytes)
                .map_err(conversion)?
        } else {
            bytes.to_vec()
        };
        String::from_utf8(raw).map_err(conversion)
    }

    pub fn documents(&self) -> Result<Vec<DocumentInfo>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id,name,sealed,size,created_at FROM documents ORDER BY created_at DESC, id DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, name, sealed, size, created_at) = row?;
            let size = usize::try_from(size).map_err(conversion)?;
            result.push(DocumentInfo {
                id,
                name: self.document_name(&name, sealed)?,
                size,
                created_at,
            });
        }
        Ok(result)
    }

    pub fn document_content(&self, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let row: Option<(Vec<u8>, bool)> = self
            .conn
            .query_row(
                "SELECT content,sealed FROM documents WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match row {
            Some((bytes, true)) => Ok(Some(
                self.sealer
                    .as_ref()
                    .ok_or_else(|| conversion(waypoint_seal::SealError::Unavailable))?
                    .unseal(&bytes)
                    .map_err(conversion)?,
            )),
            Some((bytes, false)) => Ok(Some(bytes)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_round_trip_and_missing_file() {
        let store = Store::in_memory().unwrap();
        let contents = [0, 1, 255, 4];
        store
            .add_document("one", "Résumé.pdf", &contents, 7)
            .unwrap();
        assert_eq!(store.documents().unwrap()[0].name, "Résumé.pdf");
        assert_eq!(
            store.document_content("one").unwrap(),
            Some(contents.to_vec())
        );
        assert_eq!(store.document_content("missing").unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_seals_new_and_migrated_files() {
        let mut store = Store::in_memory().unwrap();
        store
            .add_document("old", "private-name.txt", b"private-content", 1)
            .unwrap();
        store = store.with_sealer(waypoint_seal::platform_sealer().unwrap());
        assert_eq!(store.seal_existing_rows().unwrap(), 1);
        store
            .add_document("new", "new-private.txt", b"new-private-content", 2)
            .unwrap();
        for id in ["old", "new"] {
            let (name, content, sealed): (Vec<u8>, Vec<u8>, bool) = store
                .conn
                .query_row(
                    "SELECT name,content,sealed FROM documents WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert!(sealed);
            assert!(waypoint_seal::is_sealed(&name));
            assert!(waypoint_seal::is_sealed(&content));
        }
        assert_eq!(
            store.document_content("old").unwrap().unwrap(),
            b"private-content"
        );
        assert_eq!(store.documents().unwrap().len(), 2);
        assert!(store.has_sealed_material().unwrap());
    }
}
