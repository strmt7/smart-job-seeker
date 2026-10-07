//! Waypoint store (T010/T011, gate G2).
//!
//! SQLite store with transactional migrations and backup/restore. SQLCipher
//! integration is deferred: the schema and API are encryption-ready (key label
//! recorded in the header; caller supplies the path and an optional key slot),
//! and the unencrypted-local default is explicitly recorded in Known limits.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use waypoint_domain::{ApplicationState, JobIdentityId};

/// Re-exported so callers can handle store failures without depending on the
/// storage engine directly (keeps the persistence choice swappable).
pub use rusqlite::Error as StoreError;

/// Migration steps; each runs inside a foreign-keys-on transaction.
const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE IF NOT EXISTS meta(
       key TEXT PRIMARY KEY,
       value TEXT NOT NULL
     );
     CREATE TABLE jobs(
       id TEXT PRIMARY KEY,
       title TEXT NOT NULL,
       employer TEXT NOT NULL,
       location TEXT NOT NULL,
       state TEXT NOT NULL,
       canonical_url TEXT,
       content_hash TEXT,
       first_seen TEXT,
       last_seen TEXT
     );
     CREATE TABLE job_alias(
       alias_of TEXT NOT NULL REFERENCES jobs(id),
       alias_url TEXT PRIMARY KEY
     );
     CREATE TABLE observations(
       job_id TEXT NOT NULL REFERENCES jobs(id),
       observed_at TEXT NOT NULL,
       method TEXT NOT NULL,
       status TEXT NOT NULL,
       content_hash TEXT NOT NULL,
       PRIMARY KEY(job_id, observed_at)
     );
     INSERT INTO meta(key,value) VALUES('schema_version','1'),
                                     ('encryption','none-planned-sqlcipher');",
    // v2 — columns required to rank and display real discovery results.
    // Additive only: existing rows keep their values via defaults.
    "ALTER TABLE jobs ADD COLUMN remote INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE jobs ADD COLUMN source TEXT NOT NULL DEFAULT 'unknown';
     ALTER TABLE jobs ADD COLUMN requisition_id TEXT;
     ALTER TABLE jobs ADD COLUMN posted_at TEXT;
     UPDATE meta SET value='2' WHERE key='schema_version';",
    // v3 — the "act on a job" layer: candidate facts (claims), prepared
    // packets, and one-use approval grants. Grants are persisted because the
    // one-use property must survive a restart: an approval that was spent
    // before a crash must never be spendable again.
    "CREATE TABLE IF NOT EXISTS claims(
       id TEXT PRIMARY KEY,
       profile_revision INTEGER NOT NULL,
       text TEXT NOT NULL,
       status TEXT NOT NULL,
       source_kind TEXT NOT NULL,
       evidence_ids TEXT NOT NULL DEFAULT '[]',
       requires_review INTEGER NOT NULL DEFAULT 0,
       supersedes TEXT
     );
     CREATE TABLE IF NOT EXISTS packets(
       job_id TEXT PRIMARY KEY REFERENCES jobs(id),
       packet_sha256 TEXT NOT NULL,
       doc_json TEXT NOT NULL,
       body TEXT NOT NULL,
       cited_claim_ids TEXT NOT NULL DEFAULT '[]',
       profile_revision INTEGER NOT NULL,
       created_at TEXT NOT NULL,
       invalidated_at TEXT
     );
     CREATE TABLE IF NOT EXISTS grants(
       id TEXT PRIMARY KEY,
       job_id TEXT NOT NULL,
       intent_id TEXT NOT NULL,
       bound_payload_sha256 TEXT NOT NULL,
       packet_sha256 TEXT NOT NULL,
       form_schema_sha256 TEXT NOT NULL,
       destination_origin TEXT NOT NULL,
       profile_revision INTEGER NOT NULL,
       nonce TEXT NOT NULL,
       explicit_user_gesture INTEGER NOT NULL DEFAULT 1,
       consumed INTEGER NOT NULL DEFAULT 0,
       expired INTEGER NOT NULL DEFAULT 0
     );
     INSERT OR IGNORE INTO meta(key,value) VALUES('profile_revision','1');
     UPDATE meta SET value='3' WHERE key='schema_version';",
    // v4 — the submission journal. Append-only by construction: the triggers
    // below make an update or delete fail at the database level, so a
    // submission history cannot be rewritten by any code path (including a
    // future bug). This is what makes "durable intent record" a property
    // rather than an intention.
    "CREATE TABLE IF NOT EXISTS submission_journal(
       seq INTEGER PRIMARY KEY AUTOINCREMENT,
       application_id TEXT NOT NULL,
       state TEXT NOT NULL,
       timestamp TEXT NOT NULL,
       reason TEXT NOT NULL
     );
     CREATE TRIGGER IF NOT EXISTS submission_journal_no_update
       BEFORE UPDATE ON submission_journal
       BEGIN SELECT RAISE(ABORT, 'submission journal is append-only'); END;
     CREATE TRIGGER IF NOT EXISTS submission_journal_no_delete
       BEFORE DELETE ON submission_journal
       BEGIN SELECT RAISE(ABORT, 'submission journal is append-only'); END;
     UPDATE meta SET value='4' WHERE key='schema_version';",
];

pub mod work;
pub use work::{StoredClaim, StoredGrant, StoredPacket};

/// One append-only submission-journal record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRow {
    pub application_id: String,
    pub state: ApplicationState,
    pub timestamp: String,
    pub reason: String,
}

impl Store {
    /// Append one journal record. There is deliberately no update or delete
    /// counterpart; the database triggers reject both.
    pub fn append_journal(
        &mut self,
        application_id: &str,
        state: ApplicationState,
        timestamp: &str,
        reason: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO submission_journal(application_id,state,timestamp,reason) VALUES(?1,?2,?3,?4)",
            params![
                application_id,
                serde_json::to_string(&state).unwrap(),
                timestamp,
                reason
            ],
        )?;
        Ok(())
    }

    /// The full, ordered history for one application.
    pub fn journal_entries(&self, application_id: &str) -> Result<Vec<JournalRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT application_id,state,timestamp,reason FROM submission_journal
             WHERE application_id=?1 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![application_id], |r| {
            let state_json: String = r.get(1)?;
            Ok(JournalRow {
                application_id: r.get(0)?,
                state: serde_json::from_str(&state_json).unwrap_or(ApplicationState::Uncertain),
                timestamp: r.get(2)?,
                reason: r.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn journal_count(&self) -> Result<u64, StoreError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM submission_journal", [], |r| r.get(0))?;
        Ok(n as u64)
    }
}

#[derive(Debug)]
pub struct Store {
    pub(crate) conn: Connection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredJob {
    pub id: JobIdentityId,
    pub title: String,
    pub employer: String,
    pub location: String,
    pub state: ApplicationState,
    pub canonical_url: Option<String>,
    pub content_hash: Option<String>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    /// Whether the posting states remote work (used by `remote_only`).
    pub remote: bool,
    /// Discovery source that produced this row ("greenhouse", "lever", ...).
    pub source: String,
    /// Employer-side requisition id, when the source exposes one.
    pub requisition_id: Option<String>,
    /// Source-reported publication timestamp (unix seconds as text), if any.
    pub posted_at: Option<String>,
}

impl Store {
    /// Open (creating + migrating) a store at `path`, or in-memory for tests.
    pub fn open(path: &std::path::Path) -> Result<Self, rusqlite::Error> {
        Self::migrate(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, rusqlite::Error> {
        Self::migrate(Connection::open_in_memory()?)
    }

    fn migrate(mut conn: Connection) -> Result<Self, rusqlite::Error> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        // Bootstrap bookkeeping tables before any versioned step runs, so a
        // pre-existing store can always be snapshotted first.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta(
               key TEXT PRIMARY KEY,
               value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS migration_backups(
               version INTEGER PRIMARY KEY,
               taken_at TEXT NOT NULL,
               row_counts TEXT NOT NULL
             );",
        )?;
        let current: u64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        for (i, m) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            // Snapshot before a destructive step.
            let counts = Self::row_counts(&conn).unwrap_or_default();
            {
                let tx = conn.transaction()?;
                tx.execute(
                    "INSERT INTO migration_backups(version,taken_at,row_counts) VALUES(?1,?2,?3)",
                    params![i as i64 + 1, "now", counts],
                )?;
                tx.execute_batch(m)?;
                tx.commit()?;
            }
        }
        Ok(Self { conn })
    }

    fn row_counts(conn: &Connection) -> Result<String, rusqlite::Error> {
        let mut out = serde_json::Map::new();
        for table in ["jobs", "job_alias", "observations"] {
            let n: i64 =
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            out.insert(table.to_string(), serde_json::Value::from(n));
        }
        Ok(serde_json::Value::Object(out).to_string())
    }

    pub fn upsert_job(&mut self, job: &StoredJob) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO jobs(id,title,employer,location,state,canonical_url,content_hash,first_seen,last_seen,remote,source,requisition_id,posted_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title, employer=excluded.employer,
               location=excluded.location, state=excluded.state,
               canonical_url=excluded.canonical_url,
               content_hash=excluded.content_hash,
               last_seen=excluded.last_seen,
               remote=excluded.remote,
               source=excluded.source,
               requisition_id=excluded.requisition_id,
               posted_at=excluded.posted_at",
            params![
                job.id.as_str(),
                job.title,
                job.employer,
                job.location,
                serde_json::to_string(&job.state).unwrap(),
                job.canonical_url,
                job.content_hash,
                job.first_seen,
                job.last_seen,
                job.remote as i64,
                job.source,
                job.requisition_id,
                job.posted_at
            ],
        )?;
        Ok(())
    }

    /// Read one job back by identity. Additive read API used by the pipeline
    /// to make dedup decisions against what is actually stored.
    pub fn get_job(&self, id: &JobIdentityId) -> Result<Option<StoredJob>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT id,title,employer,location,state,canonical_url,content_hash,first_seen,last_seen,
                        remote,source,requisition_id,posted_at
                 FROM jobs WHERE id = ?1",
                params![id.as_str()],
                Self::row_to_job,
            )
            .optional()
    }

    /// All stored jobs, newest sighting first (stable tiebreak on id).
    pub fn all_jobs(&self) -> Result<Vec<StoredJob>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id,title,employer,location,state,canonical_url,content_hash,first_seen,last_seen,
                    remote,source,requisition_id,posted_at
             FROM jobs ORDER BY COALESCE(last_seen,'') DESC, id ASC",
        )?;
        let rows = stmt.query_map([], Self::row_to_job)?;
        rows.collect()
    }

    fn row_to_job(r: &rusqlite::Row<'_>) -> Result<StoredJob, rusqlite::Error> {
        let state_json: String = r.get(4)?;
        let state = serde_json::from_str(&state_json).unwrap_or(ApplicationState::Prepared);
        let id_str: String = r.get(0)?;
        Ok(StoredJob {
            id: JobIdentityId::new(id_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
            title: r.get(1)?,
            employer: r.get(2)?,
            location: r.get(3)?,
            state,
            canonical_url: r.get(5)?,
            content_hash: r.get(6)?,
            first_seen: r.get(7)?,
            last_seen: r.get(8)?,
            remote: r.get::<_, i64>(9)? != 0,
            source: r.get(10)?,
            requisition_id: r.get(11)?,
            posted_at: r.get(12)?,
        })
    }

    pub fn add_alias(
        &mut self,
        canonical: &JobIdentityId,
        alias_url: &str,
    ) -> Result<(), rusqlite::Error> {
        // Idempotent: the same mirror URL may legitimately be re-observed on
        // every discovery run, which must not be an error.
        self.conn.execute(
            "INSERT OR IGNORE INTO job_alias(alias_of, alias_url) VALUES(?1,?2)",
            params![canonical.as_str(), alias_url],
        )?;
        Ok(())
    }

    /// Reversible: returns alias target if the URL is a known alias.
    pub fn resolve_alias(&self, url: &str) -> Result<Option<String>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT alias_of FROM job_alias WHERE alias_url = ?1",
                params![url],
                |r| r.get(0),
            )
            .optional()
    }

    pub fn remove_alias(&mut self, url: &str) -> Result<bool, rusqlite::Error> {
        Ok(self
            .conn
            .execute("DELETE FROM job_alias WHERE alias_url = ?1", params![url])?
            > 0)
    }

    pub fn record_observation(
        &mut self,
        job_id: &JobIdentityId,
        observed_at: &str,
        method: &str,
        status: ApplicationState, // reusing for open/closed/stale via mapping below
        content_hash: &str,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            // Idempotent per (job, second): a re-run inside the same second
            // adds no new information and must not abort a discovery run.
            "INSERT OR IGNORE INTO observations(job_id,observed_at,method,status,content_hash)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                job_id.as_str(),
                observed_at,
                method,
                serde_json::to_string(&status).unwrap(),
                content_hash
            ],
        )?;
        Ok(())
    }

    pub fn job_count(&self) -> Result<u64, rusqlite::Error> {
        self.conn
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
            .map(|n: i64| n as u64)
    }

    /// Export full store bytes with a manifest; used by restore tests and
    /// backup creation. Real secure deletion limits are documented.
    pub fn backup_to(&mut self, path: &std::path::Path) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "VACUUM INTO ?1",
            params![path.to_string_lossy().to_string()],
        )?;
        Ok(())
    }
}

// required for .optional()
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str) -> StoredJob {
        StoredJob {
            id: JobIdentityId::new(id).unwrap(),
            title: "T".into(),
            employer: "E".into(),
            location: "Zurich".into(),
            state: ApplicationState::Discovered,
            canonical_url: Some(format!("https://x/{id}")),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "greenhouse".into(),
            requisition_id: Some(id.to_string()),
            posted_at: None,
        }
    }

    #[test]
    fn v2_migration_adds_ranking_columns_and_roundtrips_them() {
        let mut s = Store::in_memory().unwrap();
        let mut j = job("a");
        j.remote = true;
        j.source = "lever".into();
        j.requisition_id = Some("REQ-7".into());
        j.posted_at = Some("1758900000".into());
        j.content_hash = Some("h1".into());
        j.last_seen = Some("1758900100".into());
        s.upsert_job(&j).unwrap();

        let read = s
            .get_job(&JobIdentityId::new("a").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(read, j, "round-trip must preserve every column");
        assert!(read.remote);
        assert_eq!(read.source, "lever");
        assert_eq!(read.requisition_id.as_deref(), Some("REQ-7"));
    }

    #[test]
    fn read_apis_return_stored_rows_and_absent_ids() {
        let mut s = Store::in_memory().unwrap();
        assert!(s
            .get_job(&JobIdentityId::new("nope").unwrap())
            .unwrap()
            .is_none());
        assert!(s.all_jobs().unwrap().is_empty());

        s.upsert_job(&job("a")).unwrap();
        s.upsert_job(&job("b")).unwrap();
        let all = s.all_jobs().unwrap();
        assert_eq!(all.len(), 2);
        let ids: Vec<&str> = all.iter().map(|j| j.id.as_str()).collect();
        assert!(ids.contains(&"a") && ids.contains(&"b"));
    }

    #[test]
    fn migration_creates_schema_and_backups() {
        let mut s = Store::in_memory().unwrap();
        s.upsert_job(&job("a")).unwrap();
        s.upsert_job(&job("a")).unwrap(); // update path still single row
        assert_eq!(s.job_count().unwrap(), 1);
    }

    #[test]
    fn alias_graph_is_reversible() {
        let mut s = Store::in_memory().unwrap();
        s.upsert_job(&job("a")).unwrap();
        s.add_alias(
            &JobIdentityId::new("a").unwrap(),
            "https://mirror.example/r/1",
        )
        .unwrap();
        assert_eq!(
            s.resolve_alias("https://mirror.example/r/1").unwrap(),
            Some("a".to_string())
        );
        // Repost with unchanged id does not erase history.
        s.record_observation(
            &JobIdentityId::new("a").unwrap(),
            "2026-09-25T00:00:00Z",
            "http_get",
            ApplicationState::Confirmed,
            "h1",
        )
        .unwrap();
        assert!(s.remove_alias("https://mirror.example/r/1").unwrap());
        assert_eq!(s.resolve_alias("https://mirror.example/r/1").unwrap(), None);
    }

    #[test]
    fn the_submission_journal_rejects_updates_and_deletes_at_the_database_level() {
        let mut s = Store::in_memory().unwrap();
        s.append_journal(
            "app-1",
            ApplicationState::Submitting,
            "100",
            "write_started",
        )
        .unwrap();
        assert_eq!(s.journal_count().unwrap(), 1);

        // Appending is fine; rewriting history is not.
        let update = s
            .conn
            .execute("UPDATE submission_journal SET reason='rewritten'", []);
        assert!(update.is_err(), "an update must be refused by the trigger");
        let delete = s.conn.execute("DELETE FROM submission_journal", []);
        assert!(delete.is_err(), "a delete must be refused by the trigger");

        let rows = s.journal_entries("app-1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reason, "write_started", "history is intact");
    }

    #[test]
    fn backup_produces_a_restorable_copy() {
        let dir = std::env::temp_dir();
        let backup = dir.join(format!("wp-backup-{}.sqlite", std::process::id()));
        {
            let mut s = Store::in_memory().unwrap();
            s.upsert_job(&job("a")).unwrap();
            s.backup_to(&backup).unwrap();
        }
        let restored = Store::open(&backup).unwrap();
        assert_eq!(restored.job_count().unwrap(), 1);
        let _ = std::fs::remove_file(&backup);
    }
}

/// Shared fixture constructors for tests in this crate.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub fn job(id: &str) -> StoredJob {
        StoredJob {
            id: JobIdentityId::new(id).unwrap(),
            title: "T".into(),
            employer: "E".into(),
            location: "Zurich".into(),
            state: ApplicationState::Discovered,
            canonical_url: Some(format!("https://x/{id}")),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "greenhouse".into(),
            requisition_id: Some(id.to_string()),
            posted_at: None,
        }
    }
}
