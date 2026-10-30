//! Waypoint store (T010/T011, gate G2).
//!
//! SQLite store with transactional migrations and backup/restore. SQLCipher
//! integration is deferred: the schema and API are encryption-ready (key label
//! recorded in the header; caller supplies the path and an optional key slot),
//! and the unencrypted-local default is explicitly recorded in Known limits.

use std::sync::Arc;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use waypoint_domain::{ApplicationState, JobIdentityId};
use waypoint_seal::{SealError, Sealer, SealerKind};

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
    // v5 — sealed-at-rest storage for the candidate's own material.
    //
    // Job postings stay in plain columns: they are public data, and searching
    // them is the point. The candidate's facts and prepared documents get
    // shadow columns that hold sealed bytes; the plaintext columns are then
    // emptied by `seal_existing_rows`, which also VACUUMs so the freed pages
    // stop holding the old text.
    "ALTER TABLE claims ADD COLUMN text_sealed BLOB;
     ALTER TABLE packets ADD COLUMN body_sealed BLOB;
     ALTER TABLE packets ADD COLUMN doc_json_sealed BLOB;
     UPDATE meta SET value='5' WHERE key='schema_version';",
    // v6 — applicant-provided files. Both name and content use the active
    // sealer; an explicitly plaintext workspace stores raw bytes instead.
    "CREATE TABLE documents(
       id TEXT PRIMARY KEY,
       name BLOB NOT NULL,
       content BLOB NOT NULL,
       sealed INTEGER NOT NULL,
       size INTEGER NOT NULL,
       created_at INTEGER NOT NULL
     );
     UPDATE meta SET value='6' WHERE key='schema_version';",
    // v7 — applicant details and instructions, kept in one sealed record.
    "CREATE TABLE candidate_profile(id INTEGER PRIMARY KEY CHECK(id=1), content BLOB NOT NULL, sealed INTEGER NOT NULL);
     UPDATE meta SET value='7' WHERE key='schema_version';",
    // v8 — per-job manual handoff. This does not change application state.
    "CREATE TABLE manual_reviews(job_id TEXT PRIMARY KEY REFERENCES jobs(id), reason TEXT NOT NULL, updated_at INTEGER NOT NULL);
     UPDATE meta SET value='8' WHERE key='schema_version';",
    "CREATE TABLE job_descriptions(job_id TEXT PRIMARY KEY REFERENCES jobs(id), content TEXT);
     UPDATE meta SET value='9' WHERE key='schema_version';",
    // v10 — one-use authorizations for prefill, separate from submission.
    "CREATE TABLE prefill_grants(id TEXT PRIMARY KEY, job_id TEXT NOT NULL REFERENCES jobs(id), intent_id TEXT NOT NULL, payload_hash TEXT NOT NULL, profile_hash TEXT NOT NULL, schema_hash TEXT NOT NULL, destination TEXT NOT NULL, created_at INTEGER NOT NULL, consumed INTEGER NOT NULL DEFAULT 0);
     UPDATE meta SET value='10' WHERE key='schema_version';",
    // v11 — provider credentials are accepted only by an encrypted sealer.
    "CREATE TABLE mail_credentials(provider TEXT PRIMARY KEY, sealed_content BLOB NOT NULL);
     UPDATE meta SET value='11' WHERE key='schema_version';",
    "CREATE INDEX jobs_employer_lookup ON jobs(lower(trim(employer)));
     CREATE TABLE possible_duplicates(job_a TEXT NOT NULL REFERENCES jobs(id), job_b TEXT NOT NULL REFERENCES jobs(id), score INTEGER NOT NULL, reason TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', preferred_job TEXT, PRIMARY KEY(job_a,job_b), CHECK(job_a < job_b));
     CREATE INDEX possible_duplicates_b ON possible_duplicates(job_b,status);
     CREATE TRIGGER duplicate_recheck_after_job_change AFTER UPDATE OF title,employer,location,content_hash ON jobs WHEN OLD.title IS NOT NEW.title OR OLD.employer IS NOT NEW.employer OR OLD.location IS NOT NEW.location OR OLD.content_hash IS NOT NEW.content_hash BEGIN UPDATE possible_duplicates SET status='pending', preferred_job=NULL WHERE job_a=NEW.id OR job_b=NEW.id; END;
     UPDATE meta SET value='12' WHERE key='schema_version';",
    "CREATE TABLE fit_assessments(job_id TEXT PRIMARY KEY REFERENCES jobs(id), sealed_content BLOB NOT NULL);
     UPDATE meta SET value='13' WHERE key='schema_version';",
    "CREATE TABLE job_employer_tokens(job_id TEXT NOT NULL REFERENCES jobs(id), token TEXT NOT NULL, PRIMARY KEY(job_id,token));
     CREATE INDEX job_employer_token_lookup ON job_employer_tokens(token,job_id);
     CREATE TABLE job_employer_indexed(job_id TEXT PRIMARY KEY REFERENCES jobs(id));
     UPDATE meta SET value='14' WHERE key='schema_version';",
    "CREATE TABLE job_capture_metadata(job_id TEXT PRIMARY KEY REFERENCES jobs(id), sealed_metadata BLOB NOT NULL);
     CREATE TABLE job_capture_segments(job_id TEXT NOT NULL REFERENCES jobs(id), sequence INTEGER NOT NULL, sealed_png BLOB NOT NULL, PRIMARY KEY(job_id,sequence));
     UPDATE meta SET value='15' WHERE key='schema_version';",
    // v16 — email handoff reservations and immutable outcome history.
    "CREATE TABLE email_handoffs(id TEXT PRIMARY KEY, job_id TEXT NOT NULL REFERENCES jobs(id), approval_id TEXT NOT NULL UNIQUE, message_hash TEXT NOT NULL, sender_hash TEXT NOT NULL, recipient_hash TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('reserved','accepted','uncertain','not_sent')), created_at INTEGER NOT NULL);
     CREATE UNIQUE INDEX email_handoff_active_job ON email_handoffs(job_id) WHERE state IN ('reserved','accepted','uncertain');
     CREATE TABLE email_handoff_events(seq INTEGER PRIMARY KEY AUTOINCREMENT, handoff_id TEXT NOT NULL REFERENCES email_handoffs(id), state TEXT NOT NULL, timestamp INTEGER NOT NULL);
     CREATE TRIGGER email_handoff_events_no_update BEFORE UPDATE ON email_handoff_events BEGIN SELECT RAISE(ABORT, 'email handoff history is append-only'); END;
     CREATE TRIGGER email_handoff_events_no_delete BEFORE DELETE ON email_handoff_events BEGIN SELECT RAISE(ABORT, 'email handoff history is append-only'); END;
     UPDATE meta SET value='16' WHERE key='schema_version';",
    // v17 — separate one-use, hash-bound approval for an email handoff.
    "CREATE TABLE email_handoff_approvals(id TEXT PRIMARY KEY, job_id TEXT NOT NULL REFERENCES jobs(id), message_hash TEXT NOT NULL, sender_hash TEXT NOT NULL, recipient_hash TEXT NOT NULL, created_at INTEGER NOT NULL, consumed INTEGER NOT NULL DEFAULT 0 CHECK(consumed IN (0,1)));
     UPDATE meta SET value='17' WHERE key='schema_version';",
    // v18 — invalidate older email approvals and bind new ones to their context.
    "ALTER TABLE email_handoff_approvals ADD COLUMN context_hash TEXT NOT NULL DEFAULT '';
     CREATE TRIGGER email_approval_profile_insert AFTER INSERT ON candidate_profile BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE consumed=0; END;
     CREATE TRIGGER email_approval_profile_update AFTER UPDATE ON candidate_profile BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE consumed=0; END;
     CREATE TRIGGER email_approval_profile_delete AFTER DELETE ON candidate_profile BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE consumed=0; END;
     CREATE TRIGGER email_approval_claim_insert AFTER INSERT ON claims BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE consumed=0; END;
     CREATE TRIGGER email_approval_claim_update AFTER UPDATE ON claims BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE consumed=0; END;
     CREATE TRIGGER email_approval_claim_delete AFTER DELETE ON claims BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE consumed=0; END;
     CREATE TRIGGER email_approval_job_update AFTER UPDATE ON jobs BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE job_id=NEW.id AND consumed=0; END;
     CREATE TRIGGER email_approval_description_insert AFTER INSERT ON job_descriptions BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE job_id=NEW.job_id AND consumed=0; END;
     CREATE TRIGGER email_approval_description_update AFTER UPDATE ON job_descriptions BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE job_id=NEW.job_id AND consumed=0; END;
     CREATE TRIGGER email_approval_description_delete AFTER DELETE ON job_descriptions BEGIN UPDATE email_handoff_approvals SET consumed=1 WHERE job_id=OLD.job_id AND consumed=0; END;
     UPDATE meta SET value='18' WHERE key='schema_version';",
    // v19 — explicit user reconciliation of ambiguous email handoffs.
    "CREATE TABLE email_handoff_reconciliations(handoff_id TEXT PRIMARY KEY REFERENCES email_handoffs(id), decision TEXT NOT NULL CHECK(decision IN ('user_reports_sent','user_reports_not_sent')), timestamp INTEGER NOT NULL);
     CREATE TRIGGER email_reconciliation_no_update BEFORE UPDATE ON email_handoff_reconciliations BEGIN SELECT RAISE(ABORT, 'email reconciliation is append-only'); END;
     CREATE TRIGGER email_reconciliation_no_delete BEFORE DELETE ON email_handoff_reconciliations BEGIN SELECT RAISE(ABORT, 'email reconciliation is append-only'); END;
     UPDATE meta SET value='19' WHERE key='schema_version';",
    // v20 — preserve whether bounded duplicate comparisons were complete.
    "CREATE TABLE job_duplicate_coverage(job_id TEXT PRIMARY KEY REFERENCES jobs(id), status TEXT NOT NULL CHECK(status IN ('complete','bounded','missing_employer')), candidates INTEGER NOT NULL);
     DELETE FROM job_employer_indexed;
     CREATE TRIGGER duplicate_coverage_job_update AFTER UPDATE OF title,employer,location,content_hash,requisition_id ON jobs WHEN OLD.title IS NOT NEW.title OR OLD.employer IS NOT NEW.employer OR OLD.location IS NOT NEW.location OR OLD.content_hash IS NOT NEW.content_hash OR OLD.requisition_id IS NOT NEW.requisition_id BEGIN DELETE FROM job_duplicate_coverage WHERE job_id=NEW.id; DELETE FROM job_employer_indexed WHERE job_id=NEW.id; END;
     CREATE TRIGGER duplicate_coverage_description_insert AFTER INSERT ON job_descriptions BEGIN DELETE FROM job_duplicate_coverage WHERE job_id=NEW.job_id; DELETE FROM job_employer_indexed WHERE job_id=NEW.job_id; END;
     CREATE TRIGGER duplicate_coverage_description_update AFTER UPDATE ON job_descriptions WHEN OLD.content IS NOT NEW.content BEGIN DELETE FROM job_duplicate_coverage WHERE job_id=NEW.job_id; DELETE FROM job_employer_indexed WHERE job_id=NEW.job_id; END;
     CREATE TRIGGER duplicate_coverage_description_delete AFTER DELETE ON job_descriptions BEGIN DELETE FROM job_duplicate_coverage WHERE job_id=OLD.job_id; DELETE FROM job_employer_indexed WHERE job_id=OLD.job_id; END;
     UPDATE meta SET value='20' WHERE key='schema_version';",
    // v21 — bind email approval to duplicate-comparison coverage.
    "ALTER TABLE email_handoff_approvals ADD COLUMN coverage_revision INTEGER NOT NULL DEFAULT -1;
     ALTER TABLE email_handoff_approvals ADD COLUMN coverage_ack INTEGER NOT NULL DEFAULT 0 CHECK(coverage_ack IN (0,1));
     INSERT INTO meta(key,value) VALUES('duplicate_coverage_revision','0');
     CREATE TRIGGER duplicate_epoch_coverage_insert AFTER INSERT ON job_duplicate_coverage BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_coverage_update AFTER UPDATE ON job_duplicate_coverage BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_coverage_delete AFTER DELETE ON job_duplicate_coverage BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_index_insert AFTER INSERT ON job_employer_indexed BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_index_delete AFTER DELETE ON job_employer_indexed BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_pair_insert AFTER INSERT ON possible_duplicates BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_pair_update AFTER UPDATE ON possible_duplicates BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_pair_delete AFTER DELETE ON possible_duplicates BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_job_insert AFTER INSERT ON jobs BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     CREATE TRIGGER duplicate_epoch_job_delete AFTER DELETE ON jobs BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='duplicate_coverage_revision'; END;
     UPDATE meta SET value='21' WHERE key='schema_version';",
    // v22 — retain distinct unresolved reasons while preserving the existing
    // one-row-per-job queue API and migrating earlier review flags.
    "CREATE TABLE manual_review_reasons(
       job_id TEXT NOT NULL REFERENCES manual_reviews(job_id) ON DELETE CASCADE,
       reason TEXT NOT NULL,
       updated_at INTEGER NOT NULL,
       PRIMARY KEY(job_id, reason)
     );
     INSERT INTO manual_review_reasons(job_id,reason,updated_at)
       SELECT job_id,reason,updated_at FROM manual_reviews;
     CREATE TRIGGER manual_review_reason_limit BEFORE INSERT ON manual_review_reasons
       WHEN NOT EXISTS(SELECT 1 FROM manual_review_reasons WHERE job_id=NEW.job_id AND reason=NEW.reason)
        AND (SELECT COUNT(*) FROM manual_review_reasons WHERE job_id=NEW.job_id)>=20
       BEGIN SELECT RAISE(ABORT, 'Manual review has 20 unresolved reasons; review and clear them first'); END;
     CREATE TRIGGER manual_review_reason_insert AFTER INSERT ON manual_reviews
       BEGIN INSERT INTO manual_review_reasons(job_id,reason,updated_at) VALUES(NEW.job_id,NEW.reason,NEW.updated_at)
         ON CONFLICT(job_id,reason) DO UPDATE SET updated_at=excluded.updated_at; END;
     CREATE TRIGGER manual_review_reason_update AFTER UPDATE ON manual_reviews
       BEGIN INSERT INTO manual_review_reasons(job_id,reason,updated_at) VALUES(NEW.job_id,NEW.reason,NEW.updated_at)
         ON CONFLICT(job_id,reason) DO UPDATE SET updated_at=excluded.updated_at; END;
     UPDATE meta SET value='22' WHERE key='schema_version';",
    // v23 — a monotonic revision prevents resolving an occurrence that was
    // reported again after the user opened the review panel.
    "ALTER TABLE manual_review_reasons ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
     INSERT INTO meta(key,value) VALUES('manual_review_revision','0');
     DROP TRIGGER manual_review_reason_insert;
     DROP TRIGGER manual_review_reason_update;
     CREATE TRIGGER manual_review_reason_insert AFTER INSERT ON manual_reviews
       BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='manual_review_revision';
         INSERT INTO manual_review_reasons(job_id,reason,updated_at,revision)
           VALUES(NEW.job_id,NEW.reason,NEW.updated_at,(SELECT CAST(value AS INTEGER) FROM meta WHERE key='manual_review_revision'))
           ON CONFLICT(job_id,reason) DO UPDATE SET updated_at=excluded.updated_at,revision=excluded.revision; END;
     CREATE TRIGGER manual_review_reason_update AFTER UPDATE ON manual_reviews
       BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='manual_review_revision';
         INSERT INTO manual_review_reasons(job_id,reason,updated_at,revision)
           VALUES(NEW.job_id,NEW.reason,NEW.updated_at,(SELECT CAST(value AS INTEGER) FROM meta WHERE key='manual_review_revision'))
           ON CONFLICT(job_id,reason) DO UPDATE SET updated_at=excluded.updated_at,revision=excluded.revision; END;
     UPDATE meta SET value='23' WHERE key='schema_version';",
    // v24 — index URL lookups and rebuild employer tokens with updated normalization.
    "CREATE INDEX jobs_canonical_url_lookup ON jobs(canonical_url, COALESCE(last_seen,'') DESC, id ASC);
     DELETE FROM job_employer_indexed;
     UPDATE meta SET value='24' WHERE key='schema_version';",
    // v25 — retain a first listing source even when no screenshot was captured.
    "CREATE TABLE job_source_only(job_id TEXT PRIMARY KEY REFERENCES jobs(id), sealed_metadata BLOB NOT NULL);
     UPDATE meta SET value='25' WHERE key='schema_version';",
    // v26 — rebuild bounded employer tokens to include acronym candidates.
    "DELETE FROM job_employer_indexed;
     UPDATE meta SET value='26' WHERE key='schema_version';",
    // v27 — reconsider only capped jobs that have a usable requisition.
    "CREATE INDEX jobs_requisition_lookup ON jobs(lower(trim(requisition_id)),id);
     DELETE FROM job_employer_indexed WHERE job_id IN (
       SELECT j.id FROM jobs j JOIN job_duplicate_coverage c ON c.job_id=j.id
       WHERE c.status='bounded' AND length(CAST(trim(j.requisition_id) AS BLOB))>=3
     );
     UPDATE meta SET value='27' WHERE key='schema_version';",
    // v28 — keep per-application history lookup bounded as other jobs accumulate.
    "CREATE INDEX submission_journal_application_seq ON submission_journal(application_id,seq);
     UPDATE meta SET value='28' WHERE key='schema_version';",
    // v29 — distinguish reporting a review from refreshing its queue summary.
    "ALTER TABLE manual_reviews ADD COLUMN report_generation INTEGER NOT NULL DEFAULT 0;
     DROP TRIGGER manual_review_reason_update;
     CREATE TRIGGER manual_review_reason_update AFTER UPDATE ON manual_reviews
       WHEN NEW.report_generation != OLD.report_generation
       BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='manual_review_revision';
         INSERT INTO manual_review_reasons(job_id,reason,updated_at,revision)
           VALUES(NEW.job_id,NEW.reason,NEW.updated_at,(SELECT CAST(value AS INTEGER) FROM meta WHERE key='manual_review_revision'))
           ON CONFLICT(job_id,reason) DO UPDATE SET updated_at=excluded.updated_at,revision=excluded.revision; END;
     UPDATE meta SET value='29' WHERE key='schema_version';",
];

pub mod documents;
pub mod duplicates;
pub mod email_handoff;
pub mod fit_assessments;
pub mod job_capture;
pub mod mail_credentials;
pub mod prefill_grants;
pub mod profile;
pub mod review;
#[cfg(feature = "sqlcipher")]
pub mod sqlcipher;
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
    pub fn set_job_description(
        &self,
        id: &JobIdentityId,
        description: Option<&str>,
    ) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO job_descriptions(job_id,content) VALUES(?1,?2) ON CONFLICT(job_id) DO UPDATE SET content=excluded.content", params![id.as_str(), description])?;
        Ok(())
    }

    pub fn job_description(&self, id: &JobIdentityId) -> Result<Option<String>, StoreError> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT content FROM job_descriptions WHERE job_id=?1",
                [id.as_str()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }
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

pub struct Store {
    pub(crate) conn: Connection,
    /// When set, the candidate's own material is sealed before it is written
    /// and unsealed after it is read. `None` means plaintext columns are used,
    /// and the metadata says so — nothing silently claims to be encrypted.
    pub(crate) sealer: Option<Arc<dyn Sealer>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("sealing", &self.sealer.as_ref().map(|s| s.kind()))
            .finish_non_exhaustive()
    }
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

    /// Attach a sealing backend for the candidate's own material. Call
    /// [`Store::seal_existing_rows`] once after attaching, to migrate any
    /// rows that were written before sealing was enabled.
    pub fn with_sealer(mut self, sealer: Arc<dyn Sealer>) -> Self {
        let kind = sealer.kind();
        self.sealer = Some(sealer);
        let _ = self.conn.execute(
            "UPDATE meta SET value=?1 WHERE key='sealing'",
            params![kind.as_str()],
        );
        let _ = self.conn.execute(
            "INSERT OR IGNORE INTO meta(key,value) VALUES('sealing',?1)",
            params![kind.as_str()],
        );
        self
    }

    /// Which backend is active, if any.
    pub fn sealing_kind(&self) -> Option<SealerKind> {
        self.sealer.as_ref().map(|s| s.kind())
    }

    fn seal(&self, plaintext: &str) -> Result<Option<Vec<u8>>, SealError> {
        match &self.sealer {
            Some(s) => Ok(Some(s.seal(plaintext.as_bytes())?)),
            None => Ok(None),
        }
    }

    fn unseal_str(&self, sealed: &[u8]) -> Result<String, SealError> {
        let Some(s) = &self.sealer else {
            return Err(SealError::Unavailable);
        };
        let bytes = s.unseal(sealed)?;
        String::from_utf8(bytes).map_err(|e| SealError::Backend(e.to_string()))
    }

    /// Seal every row that still holds plaintext, then purge the freed pages.
    ///
    /// Idempotent: rows that are already sealed are skipped. Ends with VACUUM
    /// because SQLite leaves the old text in freed pages until it rewrites the
    /// file, and "the plaintext is gone" has to be true of the file on disk.
    pub fn seal_existing_rows(&mut self) -> Result<usize, StoreError> {
        if self.sealer.is_none() {
            return Ok(0);
        }
        let mut migrated = 0usize;

        let claims: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id,text FROM claims WHERE text_sealed IS NULL")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (id, text) in claims {
            let sealed = self
                .seal(&text)
                .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
            self.conn.execute(
                "UPDATE claims SET text_sealed=?1, text='' WHERE id=?2",
                params![sealed, id],
            )?;
            migrated += 1;
        }

        let packets: Vec<(String, String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT job_id,body,doc_json FROM packets WHERE body_sealed IS NULL")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        for (job_id, body, doc_json) in packets {
            let sealed_body = self
                .seal(&body)
                .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
            let sealed_doc = self
                .seal(&doc_json)
                .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
            self.conn.execute(
                "UPDATE packets SET body_sealed=?1, doc_json_sealed=?2, body='', doc_json='' WHERE job_id=?3",
                params![sealed_body, sealed_doc, job_id],
            )?;
            migrated += 1;
        }

        // Read one file at a time to avoid loading an entire document library
        // into memory during migration.
        loop {
            use rusqlite::OptionalExtension;
            let row: Option<(String, Vec<u8>, Vec<u8>)> = self
                .conn
                .query_row(
                    "SELECT id,name,content FROM documents WHERE sealed=0 LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((id, name, content)) = row else {
                break;
            };
            let sealer = self.sealer.as_ref().expect("checked above");
            let sealed_name = sealer
                .seal(&name)
                .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
            let sealed_content = sealer
                .seal(&content)
                .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
            self.conn.execute(
                "UPDATE documents SET name=?1,content=?2,sealed=1 WHERE id=?3",
                params![sealed_name, sealed_content, id],
            )?;
            migrated += 1;
        }

        let profile: Option<Vec<u8>> = {
            use rusqlite::OptionalExtension;
            self.conn
                .query_row(
                    "SELECT content FROM candidate_profile WHERE id=1 AND sealed=0",
                    [],
                    |r| r.get(0),
                )
                .optional()?
        };
        if let Some(content) = profile {
            let sealed = self
                .sealer
                .as_ref()
                .expect("checked above")
                .seal(&content)
                .map_err(|e| StoreError::ToSqlConversionFailure(Box::new(e)))?;
            self.conn.execute(
                "UPDATE candidate_profile SET content=?1,sealed=1 WHERE id=1",
                [sealed],
            )?;
            migrated += 1;
        }

        if migrated > 0 {
            self.conn.execute_batch("VACUUM;")?;
        }
        Ok(migrated)
    }

    /// True when this store holds sealed candidate material.
    pub fn has_sealed_material(&self) -> Result<bool, StoreError> {
        let claims: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM claims WHERE text_sealed IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        let packets: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM packets WHERE body_sealed IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        let documents: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM documents WHERE sealed=1", [], |r| {
                    r.get(0)
                })?;
        let profile: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM candidate_profile WHERE sealed=1",
            [],
            |r| r.get(0),
        )?;
        Ok(claims + packets + documents + profile > 0)
    }

    /// The value actually stored for a claim's text: sealed bytes when the
    /// column holds them, plaintext otherwise. Used by tests and by the
    /// sealing pass to prove what is on disk.
    pub fn raw_claim_text(&self, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.conn
            .query_row(
                "SELECT text_sealed, text FROM claims WHERE id=?1",
                params![id],
                |r| {
                    let sealed: Option<Vec<u8>> = r.get(0)?;
                    let plain: Option<String> = r.get(1)?;
                    Ok(match (sealed, plain) {
                        (Some(s), _) => Some(s),
                        (None, Some(p)) => Some(p.into_bytes()),
                        (None, None) => None,
                    })
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .map(|opt| opt.flatten())
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
             INSERT OR IGNORE INTO meta(key,value) VALUES('sealing','none');
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
        Ok(Self { conn, sealer: None })
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

    /// Atomically reserve a Jobicy fetch, conservatively limiting requests to one per hour.
    pub fn reserve_jobicy_fetch(&mut self, timestamp: i64) -> Result<bool, rusqlite::Error> {
        use rusqlite::OptionalExtension;
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let last: Option<String> = tx
            .query_row(
                "SELECT value FROM meta WHERE key='jobicy_last_fetch_unix'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(last) = last {
            let value = last.parse::<i64>().unwrap_or(i64::MAX);
            if timestamp.saturating_sub(value) < 3600 {
                return Ok(false);
            }
        }
        tx.execute("INSERT INTO meta(key,value) VALUES('jobicy_last_fetch_unix',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![timestamp.to_string()])?;
        tx.commit()?;
        Ok(true)
    }

    /// Insert a manually entered job, description and duplicate evidence together.
    pub fn insert_manual_job(
        &mut self,
        job: &StoredJob,
        description: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute("INSERT INTO jobs(id,title,employer,location,state,canonical_url,content_hash,first_seen,last_seen,remote,source,requisition_id,posted_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![job.id.as_str(),job.title,job.employer,job.location,serde_json::to_string(&job.state).unwrap(),job.canonical_url,job.content_hash,job.first_seen,job.last_seen,job.remote as i64,job.source,job.requisition_id,job.posted_at])?;
        transaction.execute(
            "INSERT INTO job_descriptions(job_id,content) VALUES(?1,?2)",
            params![job.id.as_str(), description],
        )?;
        self.find_possible_duplicates(job)?;
        transaction.commit()
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

    /// Visit jobs and their optional descriptions in the same order as all_jobs.
    /// Rows are processed one at a time so descriptions are not accumulated in memory.
    pub fn visit_jobs_with_descriptions(
        &self,
        mut visit: impl FnMut(StoredJob, Option<String>),
    ) -> Result<(), rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT j.id,j.title,j.employer,j.location,j.state,j.canonical_url,j.content_hash,j.first_seen,j.last_seen,
                    j.remote,j.source,j.requisition_id,j.posted_at,d.content
             FROM jobs j LEFT JOIN job_descriptions d ON d.job_id=j.id
             ORDER BY COALESCE(j.last_seen,'') DESC, j.id ASC",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            visit(Self::row_to_job(row)?, row.get(13)?);
        }
        Ok(())
    }

    /// Match the first job in the same order as `all_jobs`, without loading it all.
    pub fn job_id_by_canonical_url(&self, url: &str) -> Result<Option<String>, rusqlite::Error> {
        self.conn.query_row(
            "SELECT id FROM jobs WHERE canonical_url = ?1 ORDER BY COALESCE(last_seen,'') DESC, id ASC LIMIT 1",
            params![url],
            |row| row.get(0),
        ).optional()
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
    fn streamed_job_descriptions_match_individual_lookups_and_order() {
        let mut store = Store::in_memory().unwrap();
        let mut first = job("first");
        first.last_seen = Some("2".into());
        let mut second = job("second");
        second.last_seen = Some("1".into());
        let third = job("third");
        for job in [&first, &second, &third] {
            store.upsert_job(job).unwrap();
        }
        store
            .set_job_description(&first.id, Some("Python research"))
            .unwrap();
        store.set_job_description(&second.id, None).unwrap();
        let expected: Vec<_> = store
            .all_jobs()
            .unwrap()
            .into_iter()
            .map(|job| {
                let description = store.job_description(&job.id).unwrap();
                (job, description)
            })
            .collect();
        let mut actual = Vec::new();
        store
            .visit_jobs_with_descriptions(|job, description| actual.push((job, description)))
            .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn canonical_url_lookup_migrates_and_preserves_order() {
        let conn = Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        for migration in &MIGRATIONS[..23] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES('legacy','Role','Example BV','','\"discovered\"')", []).unwrap();
        conn.execute(
            "INSERT INTO job_employer_indexed(job_id) VALUES('legacy')",
            [],
        )
        .unwrap();
        let mut store = Store::migrate(conn).unwrap();
        assert!(store.duplicate_index_pending().unwrap());
        let mut older = job("older");
        older.canonical_url = Some("https://example.com/job".into());
        older.last_seen = Some("1".into());
        let mut newer = job("newer");
        newer.canonical_url = older.canonical_url.clone();
        newer.last_seen = Some("2".into());
        store.upsert_job(&older).unwrap();
        store.upsert_job(&newer).unwrap();
        assert_eq!(
            store
                .job_id_by_canonical_url("https://example.com/job")
                .unwrap()
                .as_deref(),
            Some("newer")
        );
        assert_eq!(
            store
                .job_id_by_canonical_url("https://example.com/other")
                .unwrap(),
            None
        );
        let plan: String = store.conn.query_row(
            "EXPLAIN QUERY PLAN SELECT id FROM jobs WHERE canonical_url = ?1 ORDER BY COALESCE(last_seen,'') DESC, id ASC LIMIT 1",
            params!["https://example.com/job"], |row| row.get(3),
        ).unwrap();
        assert!(plan.contains("jobs_canonical_url_lookup"), "{plan}");
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
    fn jobicy_fetch_reservation_limits_frequency() {
        let mut store = Store::in_memory().unwrap();
        assert!(store.reserve_jobicy_fetch(1000).unwrap());
        assert!(!store.reserve_jobicy_fetch(4599).unwrap());
        assert!(!store.reserve_jobicy_fetch(900).unwrap());
        assert!(store.reserve_jobicy_fetch(4600).unwrap());
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
    fn requisition_priority_migration_reindexes_without_discarding_decisions() {
        let conn = Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        for migration in &MIGRATIONS[..26] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute_batch("INSERT INTO jobs(id,title,employer,location,state,requisition_id) VALUES('a','Role','Example','','\"discovered\"','REQ-42'),('b','Role','Example','','\"discovered\"',NULL);
            INSERT INTO job_employer_tokens(job_id,token) VALUES('a','example'),('b','example');
            INSERT INTO job_employer_indexed(job_id) VALUES('a'),('b');
            INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES('a','bounded',200),('b','complete',1);
            INSERT INTO possible_duplicates(job_a,job_b,score,reason,status) VALUES('a','b',90,'Fictional','different');").unwrap();
        let before: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='duplicate_coverage_revision'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let store = Store::migrate(conn).unwrap();
        assert!(store.duplicate_index_pending().unwrap());
        assert_eq!(store.duplicate_coverage_counts().unwrap().1, 1);
        assert!(store.duplicate_coverage_revision().unwrap() > before);
        let status: String = store
            .conn
            .query_row(
                "SELECT status FROM possible_duplicates WHERE job_a='a' AND job_b='b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "different");
        let index: i64 = store.conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name='jobs_requisition_lookup'", [], |r| r.get(0)).unwrap();
        assert_eq!(index, 1);
        let mut plan = store.conn.prepare("EXPLAIN QUERY PLAN SELECT j.id FROM jobs j INDEXED BY jobs_requisition_lookup JOIN job_employer_tokens t ON t.job_id=j.id WHERE lower(trim(j.requisition_id))=?1 AND t.token=?2 AND j.id<>?3 ORDER BY j.id LIMIT 200").unwrap();
        let steps = plan
            .query_map(rusqlite::params!["req-42", "example", "a"], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            steps
                .iter()
                .any(|step| step.contains("SEARCH j USING INDEX jobs_requisition_lookup")),
            "requisition lookup must use the indexed key: {steps:?}"
        );
    }

    #[test]
    fn source_only_metadata_migrates_from_v24() {
        let conn = Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        for migration in &MIGRATIONS[..24] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES('legacy','Role','Example','','\"discovered\"')", []).unwrap();
        conn.execute(
            "INSERT INTO job_capture_metadata(job_id,sealed_metadata) VALUES('legacy',?1)",
            [b"fictional sealed payload".as_slice()],
        )
        .unwrap();
        let store = Store::migrate(conn).unwrap();
        let version: String = store
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "29");
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM job_source_only", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(store.job_count().unwrap(), 1);
        let payload: Vec<u8> = store
            .conn
            .query_row(
                "SELECT sealed_metadata FROM job_capture_metadata WHERE job_id='legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(payload, b"fictional sealed payload");
    }

    #[test]
    fn duplicate_coverage_migration_reindexes_legacy_markers_without_losing_jobs() {
        let conn = Connection::open_in_memory().unwrap();
        for migration in &MIGRATIONS[..19] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES('legacy','Role','Example','', '\"discovered\"')", []).unwrap();
        conn.execute(
            "INSERT INTO job_employer_indexed(job_id) VALUES('legacy')",
            [],
        )
        .unwrap();
        let store = Store::migrate(conn).unwrap();
        assert_eq!(store.job_count().unwrap(), 1);
        assert_eq!(store.duplicate_coverage_counts().unwrap(), (1, 1, 0, 0));
        assert_eq!(
            store.index_next_duplicate_job("").unwrap().as_deref(),
            Some("legacy")
        );
        assert_eq!(store.duplicate_coverage_counts().unwrap(), (1, 0, 0, 0));
    }

    #[test]
    fn acronym_index_migration_rechecks_existing_jobs_and_preserves_decisions() {
        let conn = Connection::open_in_memory().unwrap();
        for migration in &MIGRATIONS[..25] {
            conn.execute_batch(migration).unwrap();
        }
        for (id, employer) in [
            ("a", "IBM Ltd"),
            ("b", "International Business Machines Corporation"),
        ] {
            conn.execute("INSERT INTO jobs(id,title,employer,location,state) VALUES(?1,'Software Engineer',?2,'Berlin','\"discovered\"')", params![id, employer]).unwrap();
            conn.execute("INSERT INTO job_employer_indexed(job_id) VALUES(?1)", [id])
                .unwrap();
            conn.execute("INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,'complete',0)", [id]).unwrap();
        }
        conn.execute("INSERT INTO possible_duplicates(job_a,job_b,score,reason,status) VALUES('a','b',100,'prior review','different')", []).unwrap();
        let store = Store::migrate(conn).unwrap();
        assert_eq!(store.duplicate_coverage_counts().unwrap().1, 2);
        assert_eq!(
            store.index_next_duplicate_job("").unwrap().as_deref(),
            Some("a")
        );
        assert_eq!(
            store.index_next_duplicate_job("a").unwrap().as_deref(),
            Some("b")
        );
        assert!(!store.duplicate_index_pending().unwrap());
        assert!(store.possible_duplicates().unwrap().is_empty());
        assert!(!store.has_duplicate_hold("a").unwrap());
        assert_eq!(store.job_count().unwrap(), 2);
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
    fn journal_index_migration_preserves_interleaved_history() {
        let conn = Connection::open_in_memory().unwrap();
        #[cfg(feature = "sqlcipher")]
        conn.execute_batch("PRAGMA key = 'fictional-test-key';")
            .unwrap();
        for migration in &MIGRATIONS[..27] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute_batch("INSERT INTO submission_journal(application_id,state,timestamp,reason) VALUES ('a','\"approved\"','1','review'),('b','\"approved\"','2','review'),('a','\"submitting\"','3','write_started');").unwrap();
        let store = Store::migrate(conn).unwrap();
        let history = store.journal_entries("a").unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].reason, "review");
        assert_eq!(history[1].reason, "write_started");
        let plan: String = store.conn.query_row("EXPLAIN QUERY PLAN SELECT application_id,state,timestamp,reason FROM submission_journal WHERE application_id=?1 ORDER BY seq ASC", ["a"], |row| row.get(3)).unwrap();
        assert!(
            plan.contains("submission_journal_application_seq"),
            "{plan}"
        );
        assert!(store
            .conn
            .execute(
                "DELETE FROM submission_journal WHERE application_id='a'",
                []
            )
            .is_err());
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

#[cfg(test)]
mod seal_tests;
