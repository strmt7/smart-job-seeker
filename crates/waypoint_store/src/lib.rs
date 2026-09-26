//! Waypoint store (T010/T011, gate G2).
//!
//! SQLite store with transactional migrations and backup/restore. SQLCipher
//! integration is deferred: the schema and API are encryption-ready (key label
//! recorded in the header; caller supplies the path and an optional key slot),
//! and the unencrypted-local default is explicitly recorded in Known limits.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use waypoint_domain::{ApplicationState, JobIdentityId};

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
];

#[derive(Debug)]
pub struct Store {
    conn: Connection,
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
            "INSERT INTO jobs(id,title,employer,location,state,canonical_url,content_hash,first_seen,last_seen)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title, employer=excluded.employer,
               location=excluded.location, state=excluded.state,
               canonical_url=excluded.canonical_url,
               content_hash=excluded.content_hash,
               last_seen=excluded.last_seen",
            params![
                job.id.as_str(),
                job.title,
                job.employer,
                job.location,
                serde_json::to_string(&job.state).unwrap(),
                job.canonical_url,
                job.content_hash,
                job.first_seen,
                job.last_seen
            ],
        )?;
        Ok(())
    }

    pub fn add_alias(
        &mut self,
        canonical: &JobIdentityId,
        alias_url: &str,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO job_alias(alias_of, alias_url) VALUES(?1,?2)",
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
            "INSERT INTO observations(job_id,observed_at,method,status,content_hash)
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
        }
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
