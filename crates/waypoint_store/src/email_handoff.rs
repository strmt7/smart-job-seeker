//! Durable reservations for future explicitly approved email submissions.
//! This module performs no network operations and does not create approvals.
use crate::Store;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use waypoint_domain::{ApplicationState, JobIdentityId, Sha256Digest};

pub struct EmailHandoffReservation<'a> {
    pub id: &'a str,
    pub job_id: &'a JobIdentityId,
    pub approval_id: &'a str,
    pub message_hash: &'a Sha256Digest,
    pub sender_hash: &'a Sha256Digest,
    pub recipient_hash: &'a Sha256Digest,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailHandoffOutcome {
    Accepted,
    Uncertain,
    NotSent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailHandoffRecord {
    pub id: String,
    pub approval_id: String,
    pub message_hash: String,
    pub sender_hash: String,
    pub recipient_hash: String,
    pub state: String,
    pub created_at: i64,
}
impl EmailHandoffOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Uncertain => "uncertain",
            Self::NotSent => "not_sent",
        }
    }
}

impl Store {
    /// Record an already authorized intent before any external write. A caller
    /// must independently validate and consume a user approval bound to these
    /// hashes; this record alone is not an approval. A reserved record left by
    /// a crash must be treated as uncertain, never automatically retried.
    pub fn reserve_email_handoff(
        &mut self,
        reservation: &EmailHandoffReservation<'_>,
    ) -> Result<(), String> {
        if [reservation.id, reservation.approval_id]
            .iter()
            .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        {
            return Err("Invalid email handoff identity".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let state: Option<String> = tx
            .query_row(
                "SELECT state FROM jobs WHERE id=?1",
                [reservation.job_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let state: Option<ApplicationState> =
            state.and_then(|state| serde_json::from_str(&state).ok());
        if !matches!(
            state,
            Some(ApplicationState::Shortlisted | ApplicationState::Prepared)
        ) {
            return Err("Job is not eligible for an email handoff".into());
        }
        let held: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM possible_duplicates WHERE (job_a=?1 OR job_b=?1) AND (status='pending' OR (status='same' AND preferred_job<>?1)))", [reservation.job_id.as_str()], |row| row.get(0)).map_err(|e| e.to_string())?;
        let related_active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM possible_duplicates p JOIN email_handoffs h ON h.job_id=CASE WHEN p.job_a=?1 THEN p.job_b ELSE p.job_a END WHERE (p.job_a=?1 OR p.job_b=?1) AND p.status='same' AND h.state IN ('reserved','accepted','uncertain'))", [reservation.job_id.as_str()], |row| row.get(0)).map_err(|e| e.to_string())?;
        if held || related_active {
            return Err("Review duplicate jobs or an existing email handoff first".into());
        }
        tx.execute("INSERT INTO email_handoffs(id,job_id,approval_id,message_hash,sender_hash,recipient_hash,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,'reserved',?7)", params![reservation.id,reservation.job_id.as_str(),reservation.approval_id,reservation.message_hash.as_str(),reservation.sender_hash.as_str(),reservation.recipient_hash.as_str(),reservation.created_at]).map_err(|e| format!("Cannot reserve email handoff: {e}"))?;
        tx.execute(
            "INSERT INTO email_handoff_events(handoff_id,state,timestamp) VALUES(?1,'reserved',?2)",
            params![reservation.id, reservation.created_at],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Only record NotSent when the transport proves DATA was never handed off.
    /// SMTP acceptance is not confirmation that the recipient received the mail.
    pub fn finish_email_handoff(
        &mut self,
        id: &str,
        outcome: EmailHandoffOutcome,
        timestamp: i64,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let changed = tx
            .execute(
                "UPDATE email_handoffs SET state=?1 WHERE id=?2 AND state='reserved'",
                params![outcome.as_str(), id],
            )
            .map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("Email handoff is missing or already finalized".into());
        }
        tx.execute(
            "INSERT INTO email_handoff_events(handoff_id,state,timestamp) VALUES(?1,?2,?3)",
            params![id, outcome.as_str(), timestamp],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Return a bounded history for recovery and review. `reserved` after a
    /// restart means the handoff could have started and requires reconciliation.
    pub fn email_handoffs_for_job(
        &self,
        job_id: &JobIdentityId,
    ) -> rusqlite::Result<(Vec<EmailHandoffRecord>, bool)> {
        let mut stmt = self.conn.prepare("SELECT id,approval_id,message_hash,sender_hash,recipient_hash,state,created_at FROM email_handoffs WHERE job_id=?1 ORDER BY created_at DESC,id DESC LIMIT 101")?;
        let rows = stmt.query_map([job_id.as_str()], |row| {
            Ok(EmailHandoffRecord {
                id: row.get(0)?,
                approval_id: row.get(1)?,
                message_hash: row.get(2)?,
                sender_hash: row.get(3)?,
                recipient_hash: row.get(4)?,
                state: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        let mut records: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
        let truncated = records.len() > 100;
        records.truncate(100);
        Ok((records, truncated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredJob;
    #[test]
    fn reservation_survives_restart_and_prevents_duplicate_handoff() {
        let path = std::env::temp_dir().join(format!(
            "waypoint-email-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let job = JobIdentityId::new("fictional-email-job").unwrap();
        let digest = Sha256Digest::of("fictional");
        let reservation = |id, approval| EmailHandoffReservation {
            id,
            job_id: &job,
            approval_id: approval,
            message_hash: &digest,
            sender_hash: &digest,
            recipient_hash: &digest,
            created_at: 1,
        };
        {
            let mut store = Store::open(&path).unwrap();
            store
                .upsert_job(&StoredJob {
                    id: job.clone(),
                    title: "Role".into(),
                    employer: "Fixture".into(),
                    location: "".into(),
                    state: ApplicationState::Shortlisted,
                    canonical_url: None,
                    content_hash: None,
                    first_seen: None,
                    last_seen: None,
                    remote: false,
                    source: "fixture".into(),
                    requisition_id: None,
                    posted_at: None,
                })
                .unwrap();
            store
                .reserve_email_handoff(&reservation("one", "approval-one"))
                .unwrap();
        }
        {
            let mut store = Store::open(&path).unwrap();
            assert_eq!(
                store.email_handoffs_for_job(&job).unwrap().0[0].state,
                "reserved"
            );
            assert!(store
                .reserve_email_handoff(&reservation("two", "approval-two"))
                .is_err());
            store
                .finish_email_handoff("one", EmailHandoffOutcome::Uncertain, 2)
                .unwrap();
            assert_eq!(
                store.email_handoffs_for_job(&job).unwrap().0[0].state,
                "uncertain"
            );
            let other = JobIdentityId::new("fictional-email-job-copy").unwrap();
            let mut copy = store.get_job(&job).unwrap().unwrap();
            copy.id = other.clone();
            store.upsert_job(&copy).unwrap();
            store.conn.execute("INSERT INTO possible_duplicates(job_a,job_b,score,reason,status,preferred_job) VALUES(?1,?2,100,'fixture','same',?2)", params![job.as_str(), other.as_str()]).unwrap();
            let related = EmailHandoffReservation {
                id: "related",
                job_id: &other,
                approval_id: "approval-related",
                message_hash: &digest,
                sender_hash: &digest,
                recipient_hash: &digest,
                created_at: 3,
            };
            assert!(store.reserve_email_handoff(&related).is_err());
            assert!(store
                .reserve_email_handoff(&reservation("two", "approval-two"))
                .is_err());
            assert!(store
                .finish_email_handoff("one", EmailHandoffOutcome::NotSent, 3)
                .is_err());
            assert!(store
                .conn
                .execute("DELETE FROM email_handoff_events", [])
                .is_err());
        }
        std::fs::remove_file(path).unwrap();
    }
}
