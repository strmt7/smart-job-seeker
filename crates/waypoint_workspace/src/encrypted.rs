//! Feature-gated Windows workspace encryption and non-destructive cutover.
use super::*;
use std::io::Write;
use waypoint_seal::{Sealer, SealerKind};
use zeroize::Zeroizing;

fn error(message: impl std::fmt::Display) -> WorkspaceError {
    WorkspaceError::Export(format!("Encrypted workspace: {message}"))
}

fn random_tag() -> Result<String, WorkspaceError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| error("random generation failed"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn read_key(path: &Path, sealer: &dyn Sealer) -> Result<Zeroizing<[u8; 32]>, WorkspaceError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16384 {
        return Err(error("key file is not a small regular file"));
    }
    let sealed = std::fs::read(path)?;
    let raw = Zeroizing::new(sealer.unseal(&sealed).map_err(error)?);
    let key: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| error("invalid key length"))?;
    Ok(Zeroizing::new(key))
}

fn create_key(
    dir: &Path,
    target: &Path,
    sealer: &dyn Sealer,
) -> Result<Zeroizing<[u8; 32]>, WorkspaceError> {
    let mut key = Zeroizing::new([0u8; 32]);
    getrandom::fill(key.as_mut()).map_err(|_| error("random generation failed"))?;
    let sealed = sealer.seal(key.as_ref()).map_err(error)?;
    let staging = dir.join(format!(".workspace-key-{}.tmp", random_tag()?));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    file.write_all(&sealed)?;
    file.sync_all()?;
    drop(file);
    // A hard link fails if another process created the destination first.
    std::fs::hard_link(&staging, target)?;
    std::fs::remove_file(&staging)?;
    Ok(key)
}

impl Workspace {
    pub(super) fn encrypted_duplicate_indexer(
        &self,
        path: &Path,
        lock: std::fs::File,
    ) -> Result<DuplicateIndexer, WorkspaceError> {
        let dir = path
            .parent()
            .ok_or_else(|| error("database has no parent directory"))?;
        let sealer = waypoint_seal::platform_sealer()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| error("Windows DPAPI is required"))?;
        let key = read_key(&dir.join("workspace-key.dpapi"), sealer.as_ref())?;
        let store = Store::open_encrypted(path, &key).map_err(error)?;
        store
            .configure_nonblocking_duplicate_indexer()
            .map_err(error)?;
        Ok(DuplicateIndexer {
            store,
            _workspace_lock: lock,
        })
    }

    /// Open or create an encrypted database. Existing plaintext is copied to a
    /// separate file and retained; an incomplete export is never selected.
    pub fn open_encrypted(dir: &Path) -> Result<Self, WorkspaceError> {
        std::fs::create_dir_all(dir)?;
        let workspace_lock = lock_workspace(dir)?;
        let sealer = waypoint_seal::platform_sealer()
            .filter(|s| s.kind() == SealerKind::Dpapi)
            .ok_or_else(|| error("Windows DPAPI is required"))?;
        let plain = dir.join("jobs.sqlite3");
        let encrypted = dir.join("jobs.encrypted.sqlite3");
        let key_path = dir.join("workspace-key.dpapi");
        if encrypted.exists() && !key_path.exists() {
            return Err(error("database key file is missing"));
        }
        let key = if key_path.exists() {
            read_key(&key_path, sealer.as_ref())?
        } else {
            create_key(dir, &key_path, sealer.as_ref())?
        };
        if !encrypted.exists() && plain.exists() {
            let staging = dir.join(format!(".jobs-encrypted-{}.tmp", random_tag()?));
            Store::export_plaintext_to_encrypted(&plain, &staging, &key).map_err(error)?;
            // Validate schema and migrations before publishing the new path.
            drop(Store::open_encrypted(&staging, &key)?);
            std::fs::hard_link(&staging, &encrypted)?;
            std::fs::remove_file(&staging)?;
        }
        let mut store = Store::open_encrypted(&encrypted, &key)?.with_sealer(sealer);
        let sealed_now = store.seal_existing_rows()?;
        Ok(Self {
            store,
            registry: default_registry(),
            budgets: ResourceBudgets::default(),
            coverage: CoverageReport::default(),
            last_outcome: None,
            db_path: Some(encrypted),
            sealed_now,
            database_encrypted: true,
            legacy_plaintext: plain.exists(),
            _workspace_lock: Some(workspace_lock),
        })
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn encrypted_workspace_preserves_plaintext_source_and_reopens() {
        let dir = std::env::temp_dir().join(format!(
            "waypoint-workspace-cipher-{}-{}",
            std::process::id(),
            random_tag().unwrap()
        ));
        std::fs::create_dir(&dir).unwrap();
        let original = dir.join("jobs.sqlite3");
        {
            let mut ws = Workspace::open(&dir).unwrap();
            ws.add_fact("fictional migrated qualification", FactOrigin::UserAttested)
                .unwrap();
            let job_id = JobIdentityId::new("fictional-migrated-job").unwrap();
            ws.import_manual_job(
                StoredJob {
                    id: job_id,
                    title: "fictional-position-private".into(),
                    employer: "Example".into(),
                    location: "Remote".into(),
                    state: ApplicationState::Discovered,
                    canonical_url: Some("https://example.org/job".into()),
                    content_hash: None,
                    first_seen: None,
                    last_seen: None,
                    remote: true,
                    source: "user_supplied_url".into(),
                    requisition_id: None,
                    posted_at: None,
                },
                Some("fictional-private-position-description"),
            )
            .unwrap();
            assert!(
                Workspace::open_encrypted(&dir).is_err(),
                "second workspace must be refused"
            );
        }
        let mut ws = Workspace::open_encrypted(&dir).unwrap();
        assert!(ws.sealing_status().contains("plaintext"));
        assert_eq!(
            ws.facts().unwrap()[0].text,
            "fictional migrated qualification"
        );
        assert!(original.exists());
        assert!(Workspace::open(&dir).is_err());
        let encrypted_bytes = std::fs::read(ws.db_path.as_ref().unwrap()).unwrap();
        assert!(!encrypted_bytes.starts_with(b"SQLite format 3"));
        let position = b"fictional-private-position-description";
        assert!(!encrypted_bytes
            .windows(position.len())
            .any(|w| w == position));
        assert_eq!(ws.jobs().unwrap()[0].title, "fictional-position-private");
        let id = JobIdentityId::new("fictional-migrated-job").unwrap();
        ws.store
            .set_job_description(&id, Some("updated fictional description"))
            .unwrap();
        assert!(ws.duplicate_index_pending().unwrap());
        let indexer = ws.duplicate_indexer().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            indexer.index_next("")
        });
        barrier.wait();
        ws.add_fact("fictional concurrent update", FactOrigin::UserAttested)
            .unwrap();
        let indexed = worker
            .join()
            .unwrap()
            .or_else(|_| ws.index_next_duplicate_job(""))
            .unwrap();
        assert_eq!(indexed.as_deref(), Some("fictional-migrated-job"));
        assert!(!ws.duplicate_index_pending().unwrap());
        let indexer = ws.duplicate_indexer().unwrap();
        drop(ws);
        assert!(
            Workspace::open_encrypted(&dir).is_err(),
            "indexer must keep the workspace locked"
        );
        drop(indexer);
        assert!(
            Workspace::open(&dir).is_err(),
            "plaintext build must not reopen the retained old database"
        );
        let reopened = Workspace::open_encrypted(&dir).unwrap();
        let facts = reopened.facts().unwrap();
        assert!(facts
            .iter()
            .any(|f| f.text == "fictional migrated qualification"));
        assert!(facts
            .iter()
            .any(|f| f.text == "fictional concurrent update"));
        drop(reopened);
        for name in [
            "jobs.sqlite3",
            "jobs.encrypted.sqlite3",
            "workspace-key.dpapi",
            ".workspace.lock",
        ] {
            std::fs::remove_file(dir.join(name)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
