//! Explicit SQLCipher operations for an encrypted workspace and non-destructive export.
//! Existing plaintext workspaces are retained during a separate encrypted export.
use crate::Store;
use rusqlite::{params, Connection};
use std::{io::Read, path::Path};
use zeroize::Zeroizing;

fn key_text(key: &[u8; 32]) -> Zeroizing<String> {
    Zeroizing::new(key.iter().map(|b| format!("{b:02x}")).collect())
}

impl Store {
    pub fn open_encrypted(path: &Path, key: &[u8; 32]) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let key = key_text(key);
        // `key` is a 64-character hexadecimal string generated above, never arbitrary SQL.
        let statement = Zeroizing::new(format!("PRAGMA key = '{}';", key.as_str()));
        conn.execute_batch(&statement)?;
        let _: String = conn.query_row("PRAGMA cipher_version", [], |r| r.get(0))?;
        let _: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
        Self::migrate(conn)
    }

    /// Export a plaintext database into a newly created, separate encrypted file.
    /// The original is deliberately retained; the caller owns any later cutover.
    pub fn export_plaintext_to_encrypted(
        source: &Path,
        destination: &Path,
        key: &[u8; 32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if source == destination {
            return Err("Source and destination must differ".into());
        }
        let mut header = [0u8; 16];
        std::fs::File::open(source)?.read_exact(&mut header)?;
        if &header != b"SQLite format 3\0" {
            return Err("Source is not a plaintext SQLite database".into());
        }
        // Refuse overwrite even if the destination is an empty or partial file.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let conn = Connection::open(source)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let key = key_text(key);
        conn.execute(
            "ATTACH DATABASE ?1 AS encrypted KEY ?2",
            params![destination.to_string_lossy().as_ref(), key.as_str()],
        )?;
        conn.execute_batch("BEGIN EXCLUSIVE;")?;
        conn.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
        conn.execute_batch(&format!(
            "PRAGMA encrypted.user_version={version}; COMMIT; DETACH DATABASE encrypted;"
        ))?;
        drop(conn);
        let test = Connection::open(destination)?;
        let statement = Zeroizing::new(format!("PRAGMA key = '{}';", key.as_str()));
        test.execute_batch(&statement)?;
        let integrity: String = test.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err("Encrypted database integrity check failed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_workspace_opens_and_rejects_incorrect_key() {
        let dir = std::env::temp_dir().join(format!(
            "waypoint-encrypted-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("jobs.sqlite");
        let key = [12u8; 32];
        {
            let store = Store::open_encrypted(&path, &key).unwrap();
            assert_eq!(store.job_count().unwrap(), 0);
        }
        assert!(!std::fs::read(&path)
            .unwrap()
            .starts_with(b"SQLite format 3"));
        assert!(Store::open_encrypted(&path, &[13u8; 32]).is_err());
        assert_eq!(
            Store::open_encrypted(&path, &key)
                .unwrap()
                .job_count()
                .unwrap(),
            0
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn encrypted_export_preserves_source_and_rejects_wrong_key() {
        let dir = std::env::temp_dir().join(format!(
            "waypoint-sqlcipher-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let source = dir.join("source.sqlite");
        let target = dir.join("encrypted.sqlite");
        let key = [42u8; 32];
        {
            let conn = Connection::open(&source).unwrap();
            conn.execute_batch("CREATE TABLE sample(value TEXT); INSERT INTO sample VALUES ('fictional-position-private'); PRAGMA user_version=12;").unwrap();
        }
        Store::export_plaintext_to_encrypted(&source, &target, &key).unwrap();
        assert!(std::fs::read(&source)
            .unwrap()
            .starts_with(b"SQLite format 3"));
        let encrypted = std::fs::read(&target).unwrap();
        assert!(!encrypted.starts_with(b"SQLite format 3"));
        assert!(!encrypted
            .windows(b"fictional-position-private".len())
            .any(|w| w == b"fictional-position-private"));
        let wrong = Connection::open(&target).unwrap();
        wrong.execute_batch("PRAGMA key='wrong'").unwrap();
        assert!(wrong
            .query_row("SELECT value FROM sample", [], |r| r.get::<_, String>(0))
            .is_err());
        drop(wrong);
        let conn = Connection::open(&target).unwrap();
        conn.execute_batch(&format!("PRAGMA key='{}'", key_text(&key).as_str()))
            .unwrap();
        assert_eq!(
            conn.query_row("SELECT value FROM sample", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "fictional-position-private"
        );
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            12
        );
        drop(conn);
        assert!(Store::export_plaintext_to_encrypted(&source, &target, &key).is_err());
        std::fs::remove_file(source).unwrap();
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
