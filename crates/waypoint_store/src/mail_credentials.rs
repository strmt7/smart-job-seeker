//! Explicitly sealed storage for mail-provider credentials.
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
use waypoint_seal::SealerKind;

fn failure(error: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::ToSqlConversionFailure(Box::new(error))
}

impl Store {
    pub fn can_store_gmail_credentials(&self) -> bool {
        self.sealer
            .as_ref()
            .is_some_and(|sealer| sealer.kind() == SealerKind::Dpapi)
    }
    /// Never fall back to plaintext, including test and unsupported platforms.
    pub fn save_gmail_credentials(&self, content: &[u8]) -> Result<(), StoreError> {
        let sealer = self
            .sealer
            .as_ref()
            .filter(|sealer| sealer.kind() == SealerKind::Dpapi)
            .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
        let encrypted = sealer.seal(content).map_err(failure)?;
        self.conn.execute("INSERT INTO mail_credentials(provider,sealed_content) VALUES('gmail',?1) ON CONFLICT(provider) DO UPDATE SET sealed_content=excluded.sealed_content", params![encrypted])?;
        Ok(())
    }

    pub fn gmail_credentials(&self) -> Result<Option<Vec<u8>>, StoreError> {
        let data: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT sealed_content FROM mail_credentials WHERE provider='gmail'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|value| {
            let sealer = self
                .sealer
                .as_ref()
                .filter(|sealer| sealer.kind() == SealerKind::Dpapi)
                .ok_or_else(|| failure(waypoint_seal::SealError::Unavailable))?;
            sealer.unseal(&value).map_err(failure)
        })
        .transpose()
    }

    pub fn clear_gmail_credentials(&self) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM mail_credentials WHERE provider='gmail'", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_require_real_encryption_and_round_trip_when_available() {
        let plain = Store::in_memory().unwrap();
        assert!(plain.save_gmail_credentials(b"fictional-secret").is_err());
        assert_eq!(plain.gmail_credentials().unwrap(), None);
        let Some(sealer) =
            waypoint_seal::platform_sealer().filter(|s| s.kind() == SealerKind::Dpapi)
        else {
            return;
        };
        let store = Store::in_memory().unwrap().with_sealer(sealer);
        store.save_gmail_credentials(b"fictional-secret").unwrap();
        assert_eq!(
            store.gmail_credentials().unwrap().as_deref(),
            Some(b"fictional-secret".as_slice())
        );
        store.clear_gmail_credentials().unwrap();
        assert_eq!(store.gmail_credentials().unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_seals_credential_bytes_in_disk_database() {
        let sealer = waypoint_seal::platform_sealer().expect("Windows DPAPI must be available");
        assert_eq!(sealer.kind(), SealerKind::Dpapi);
        let path = std::env::temp_dir().join(format!(
            "waypoint-mail-credential-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let value = b"fictional-credential-unique-test-value";
        {
            let store = Store::open(&path).unwrap().with_sealer(sealer);
            store.save_gmail_credentials(value).unwrap();
            assert_eq!(
                store.gmail_credentials().unwrap().as_deref(),
                Some(value.as_slice())
            );
        }
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(value.len()).any(|part| part == value));
        std::fs::remove_file(path).unwrap();
    }
}
