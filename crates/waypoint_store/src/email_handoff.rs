//! Durable reservations for future explicitly approved email submissions.
//! This module performs no network operations.
use crate::Store;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use waypoint_domain::{ApplicationState, JobIdentityId, Sha256Digest};

const MAX_RELATED_JOBS: usize = 500;

fn coverage_flags(tx: &rusqlite::Transaction<'_>) -> Result<(bool, bool), String> {
    tx.query_row("SELECT EXISTS(SELECT 1 FROM jobs j LEFT JOIN job_employer_indexed i ON i.job_id=j.id LEFT JOIN job_duplicate_coverage c ON c.job_id=j.id WHERE i.job_id IS NULL OR c.job_id IS NULL), EXISTS(SELECT 1 FROM job_duplicate_coverage WHERE status IN ('bounded','missing_employer'))", [], |row| Ok((row.get(0)?, row.get(1)?))).map_err(|e| e.to_string())
}

// Confirmed duplicate links form a graph, not necessarily a star. Fail closed
// if its size or an unresolved edge prevents a complete bounded check.
fn check_related_jobs(tx: &rusqlite::Transaction<'_>, target: &str) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::from([target.to_owned()]);
    seen.insert(target.to_owned());
    while let Some(id) = queue.pop_front() {
        let email_active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM email_handoffs WHERE job_id=?1 AND state IN ('reserved','accepted','uncertain'))", [&id], |row| row.get(0)).map_err(|e| e.to_string())?;
        let browser_history: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM submission_journal WHERE application_id=?1 AND state IN ('\"submitting\"','\"uncertain\"','\"confirmed\"','\"external_attested\"','\"rejected\"','\"interview\"','\"offer\"'))", [&id], |row| row.get(0)).map_err(|e| e.to_string())?;
        let state: String = tx
            .query_row("SELECT state FROM jobs WHERE id=?1", [&id], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())?;
        let state: ApplicationState =
            serde_json::from_str(&state).map_err(|_| "Cannot verify a related job's state")?;
        if email_active
            || browser_history
            || !matches!(
                state,
                ApplicationState::Discovered
                    | ApplicationState::Shortlisted
                    | ApplicationState::Prepared
            )
        {
            return Err(
                "A related job has an existing or uncertain application; review it first".into(),
            );
        }
        let mut stmt = tx.prepare("SELECT job_a,job_b,status FROM possible_duplicates WHERE (job_a=?1 OR job_b=?1) AND status<>'different' LIMIT 501").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut count = 0usize;
        for row in rows {
            count += 1;
            if count > MAX_RELATED_JOBS {
                return Err("Too many related listings to check; manual review is required".into());
            }
            let (a, b, status) = row.map_err(|e| e.to_string())?;
            if status == "pending" {
                return Err("Review possible duplicate jobs before an email handoff".into());
            }
            if status != "same" {
                return Err("A duplicate decision is unknown; manual review is required".into());
            }
            let next = if a == id { b } else { a };
            if seen.insert(next.clone()) {
                if seen.len() > MAX_RELATED_JOBS {
                    return Err(
                        "Too many related listings to check; manual review is required".into(),
                    );
                }
                queue.push_back(next);
            }
        }
    }
    Ok(())
}

pub struct EmailHandoffReservation<'a> {
    pub id: &'a str,
    pub job_id: &'a JobIdentityId,
    pub approval_id: &'a str,
    pub message_hash: &'a Sha256Digest,
    pub sender_hash: &'a Sha256Digest,
    pub recipient_hash: &'a Sha256Digest,
    pub context_hash: &'a Sha256Digest,
    pub created_at: i64,
}

pub struct EmailHandoffApproval<'a> {
    pub id: &'a str,
    pub job_id: &'a JobIdentityId,
    pub message_hash: &'a Sha256Digest,
    pub sender_hash: &'a Sha256Digest,
    pub recipient_hash: &'a Sha256Digest,
    pub context_hash: &'a Sha256Digest,
    pub created_at: i64,
    pub explicit_user_gesture: bool,
    pub acknowledge_incomplete_coverage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailHandoffOutcome {
    Accepted,
    Uncertain,
    NotSent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailReconciliation {
    UserReportsSent,
    UserReportsNotSent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailHandoffRecord {
    pub id: String,
    pub approval_id: String,
    pub message_hash: String,
    pub sender_hash: String,
    pub recipient_hash: String,
    pub state: String,
    pub reconciliation: Option<String>,
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
    /// Record a user's mailbox check; this is a user report, not independent
    /// delivery evidence. No action here sends or retries a message.
    pub fn reconcile_email_handoff(
        &mut self,
        job: &JobIdentityId,
        id: &str,
        decision: EmailReconciliation,
        timestamp: i64,
        explicit_user_gesture: bool,
    ) -> Result<(), String> {
        if !explicit_user_gesture || timestamp <= 0 {
            return Err("An explicit user reconciliation is required".into());
        }
        let (state, label) = match decision {
            EmailReconciliation::UserReportsSent => ("accepted", "user_reports_sent"),
            EmailReconciliation::UserReportsNotSent => ("not_sent", "user_reports_not_sent"),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let changed = tx.execute("UPDATE email_handoffs SET state=?1 WHERE id=?2 AND job_id=?3 AND state IN ('reserved','uncertain')", params![state,id,job.as_str()]).map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("Email handoff is not awaiting reconciliation".into());
        }
        tx.execute("INSERT INTO email_handoff_reconciliations(handoff_id,decision,timestamp) VALUES(?1,?2,?3)", params![id,label,timestamp]).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO email_handoff_events(handoff_id,state,timestamp) VALUES(?1,?2,?3)",
            params![id, label, timestamp],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Persist an approval after the application has presented the exact
    /// message and destination to the user. Never call from a model decision.
    pub fn approve_email_handoff(
        &mut self,
        approval: &EmailHandoffApproval<'_>,
    ) -> Result<(), String> {
        if !approval.explicit_user_gesture
            || approval.id.is_empty()
            || approval.id.len() > 128
            || approval.id.chars().any(char::is_control)
            || approval.created_at <= 0
        {
            return Err("An explicit user approval is required".into());
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let state: Option<String> = tx
            .query_row(
                "SELECT state FROM jobs WHERE id=?1",
                [approval.job_id.as_str()],
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
            return Err("Job is not eligible for email approval".into());
        }
        let (pending, incomplete) = coverage_flags(&tx)?;
        if pending {
            return Err("Duplicate indexing is still pending; review after it finishes".into());
        }
        if incomplete && !approval.acknowledge_incomplete_coverage {
            return Err(
                "Review and acknowledge the incomplete duplicate comparison coverage".into(),
            );
        }
        let revision: i64 = tx
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='duplicate_coverage_revision'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO email_handoff_approvals(id,job_id,message_hash,sender_hash,recipient_hash,created_at,context_hash,coverage_revision,coverage_ack) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![approval.id, approval.job_id.as_str(), approval.message_hash.as_str(), approval.sender_hash.as_str(), approval.recipient_hash.as_str(), approval.created_at, approval.context_hash.as_str(),revision,approval.acknowledge_incomplete_coverage]).map_err(|e| format!("Cannot save email approval: {e}"))?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Consume the matching one-use approval and reserve before any external
    /// write in one transaction. A reservation left by a crash is uncertain.
    pub fn reserve_email_handoff(
        &mut self,
        reservation: &EmailHandoffReservation<'_>,
    ) -> Result<(), String> {
        self.check_or_reserve_email_handoff(reservation, false)
    }

    /// Run the complete reservation transaction and roll it back. This does
    /// not consume approval or guarantee that a later real handoff will pass.
    pub fn rehearse_email_handoff(
        &mut self,
        reservation: &EmailHandoffReservation<'_>,
    ) -> Result<(), String> {
        self.check_or_reserve_email_handoff(reservation, true)
    }

    fn check_or_reserve_email_handoff(
        &mut self,
        reservation: &EmailHandoffReservation<'_>,
        rehearsal: bool,
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
        if held {
            return Err("Review duplicate jobs or an existing email handoff first".into());
        }
        check_related_jobs(&tx, reservation.job_id.as_str())?;
        let (pending, incomplete) = coverage_flags(&tx)?;
        if pending {
            return Err("Duplicate indexing is still pending; review after it finishes".into());
        }
        let consumed = tx.execute("UPDATE email_handoff_approvals SET consumed=1 WHERE id=?1 AND job_id=?2 AND message_hash=?3 AND sender_hash=?4 AND recipient_hash=?5 AND consumed=0 AND created_at<=?6 AND created_at>=?7 AND context_hash=?8 AND coverage_revision=(SELECT CAST(value AS INTEGER) FROM meta WHERE key='duplicate_coverage_revision') AND (?9=0 OR coverage_ack=1)", params![reservation.approval_id,reservation.job_id.as_str(),reservation.message_hash.as_str(),reservation.sender_hash.as_str(),reservation.recipient_hash.as_str(),reservation.created_at,reservation.created_at.saturating_sub(600),reservation.context_hash.as_str(),incomplete]).map_err(|e| e.to_string())?;
        if consumed != 1 {
            return Err(
                "Email approval is missing, expired, spent or does not match the reviewed message"
                    .into(),
            );
        }
        tx.execute("INSERT INTO email_handoffs(id,job_id,approval_id,message_hash,sender_hash,recipient_hash,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,'reserved',?7)", params![reservation.id,reservation.job_id.as_str(),reservation.approval_id,reservation.message_hash.as_str(),reservation.sender_hash.as_str(),reservation.recipient_hash.as_str(),reservation.created_at]).map_err(|e| format!("Cannot reserve email handoff: {e}"))?;
        tx.execute(
            "INSERT INTO email_handoff_events(handoff_id,state,timestamp) VALUES(?1,'reserved',?2)",
            params![reservation.id, reservation.created_at],
        )
        .map_err(|e| e.to_string())?;
        if rehearsal {
            tx.rollback().map_err(|e| e.to_string())
        } else {
            tx.commit().map_err(|e| e.to_string())
        }
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
        let mut stmt = self.conn.prepare("SELECT h.id,h.approval_id,h.message_hash,h.sender_hash,h.recipient_hash,h.state,h.created_at,r.decision FROM email_handoffs h LEFT JOIN email_handoff_reconciliations r ON r.handoff_id=h.id WHERE h.job_id=?1 ORDER BY h.created_at DESC,h.id DESC LIMIT 101")?;
        let rows = stmt.query_map([job_id.as_str()], |row| {
            Ok(EmailHandoffRecord {
                id: row.get(0)?,
                approval_id: row.get(1)?,
                message_hash: row.get(2)?,
                sender_hash: row.get(3)?,
                recipient_hash: row.get(4)?,
                state: row.get(5)?,
                created_at: row.get(6)?,
                reconciliation: row.get(7)?,
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
    fn mark_indexed(store: &Store, id: &JobIdentityId, status: &str) {
        store
            .conn
            .execute(
                "INSERT INTO job_duplicate_coverage(job_id,status,candidates) VALUES(?1,?2,0)",
                params![id.as_str(), status],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_employer_indexed(job_id) VALUES(?1)",
                [id.as_str()],
            )
            .unwrap();
    }
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
            context_hash: &digest,
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
            let mut approval = EmailHandoffApproval {
                id: "approval-one",
                job_id: &job,
                message_hash: &digest,
                sender_hash: &digest,
                recipient_hash: &digest,
                context_hash: &digest,
                created_at: 1,
                explicit_user_gesture: false,
                acknowledge_incomplete_coverage: false,
            };
            assert!(store.approve_email_handoff(&approval).is_err());
            approval.explicit_user_gesture = true;
            assert!(store.approve_email_handoff(&approval).is_err()); // Index pending.
            mark_indexed(&store, &job, "bounded");
            assert!(store.approve_email_handoff(&approval).is_err()); // Acknowledgement required.
            approval.acknowledge_incomplete_coverage = true;
            store.approve_email_handoff(&approval).unwrap();
            store
                .rehearse_email_handoff(&reservation("rehearsal", "approval-one"))
                .unwrap();
            store
                .conn
                .execute(
                    "UPDATE job_duplicate_coverage SET status='complete' WHERE job_id=?1",
                    [job.as_str()],
                )
                .unwrap();
            assert!(store
                .rehearse_email_handoff(&reservation("stale", "approval-one"))
                .is_err());
            approval.id = "approval-fresh";
            store.approve_email_handoff(&approval).unwrap();
            let changed = Sha256Digest::of("changed recipient");
            let mut mismatched = reservation("one", "approval-fresh");
            mismatched.recipient_hash = &changed;
            assert!(store.reserve_email_handoff(&mismatched).is_err());
            let mut expired = reservation("one", "approval-fresh");
            expired.created_at = 602;
            assert!(store.reserve_email_handoff(&expired).is_err());
            let unspent: i64 = store
                .conn
                .query_row(
                    "SELECT consumed FROM email_handoff_approvals WHERE id='approval-fresh'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(unspent, 0);
            store
                .reserve_email_handoff(&reservation("one", "approval-fresh"))
                .unwrap();
            let spent: i64 = store
                .conn
                .query_row(
                    "SELECT consumed FROM email_handoff_approvals WHERE id='approval-fresh'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(spent, 1);
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
            let independent = JobIdentityId::new("independent-fixture").unwrap();
            let mut independent_job = store.get_job(&job).unwrap().unwrap();
            independent_job.id = independent.clone();
            store.upsert_job(&independent_job).unwrap();
            mark_indexed(&store, &independent, "complete");
            store
                .approve_email_handoff(&EmailHandoffApproval {
                    id: "independent-approval",
                    job_id: &independent,
                    message_hash: &digest,
                    sender_hash: &digest,
                    recipient_hash: &digest,
                    context_hash: &digest,
                    created_at: 1,
                    explicit_user_gesture: true,
                    acknowledge_incomplete_coverage: false,
                })
                .unwrap();
            let conflicting_id = EmailHandoffReservation {
                id: "one",
                job_id: &independent,
                approval_id: "independent-approval",
                message_hash: &digest,
                sender_hash: &digest,
                recipient_hash: &digest,
                context_hash: &digest,
                created_at: 2,
            };
            assert!(store.reserve_email_handoff(&conflicting_id).is_err());
            let unspent: i64 = store
                .conn
                .query_row(
                    "SELECT consumed FROM email_handoff_approvals WHERE id='independent-approval'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(unspent, 0);
            store
                .append_journal(
                    independent.as_str(),
                    ApplicationState::Uncertain,
                    "2",
                    "fixture",
                )
                .unwrap();
            let browser_blocked = EmailHandoffReservation {
                id: "independent-attempt",
                job_id: &independent,
                approval_id: "independent-approval",
                message_hash: &digest,
                sender_hash: &digest,
                recipient_hash: &digest,
                context_hash: &digest,
                created_at: 2,
            };
            assert!(store.reserve_email_handoff(&browser_blocked).is_err());
            assert_eq!(
                store.email_handoffs_for_job(&job).unwrap().0[0].state,
                "uncertain"
            );
            let other = JobIdentityId::new("fictional-email-job-copy").unwrap();
            let mut copy = store.get_job(&job).unwrap().unwrap();
            copy.id = other.clone();
            store.upsert_job(&copy).unwrap();
            mark_indexed(&store, &other, "complete");
            store.conn.execute("INSERT INTO possible_duplicates(job_a,job_b,score,reason,status,preferred_job) VALUES(?1,?2,100,'fixture','same',?2)", params![job.as_str(), other.as_str()]).unwrap();
            let related = EmailHandoffReservation {
                id: "related",
                job_id: &other,
                approval_id: "approval-related",
                message_hash: &digest,
                sender_hash: &digest,
                recipient_hash: &digest,
                context_hash: &digest,
                created_at: 3,
            };
            store
                .approve_email_handoff(&EmailHandoffApproval {
                    id: "approval-related",
                    job_id: &other,
                    message_hash: &digest,
                    sender_hash: &digest,
                    recipient_hash: &digest,
                    context_hash: &digest,
                    created_at: 1,
                    explicit_user_gesture: true,
                    acknowledge_incomplete_coverage: false,
                })
                .unwrap();
            assert!(store.reserve_email_handoff(&related).is_err());
            let third = JobIdentityId::new("fictional-email-job-copy-third").unwrap();
            let mut third_job = store.get_job(&other).unwrap().unwrap();
            third_job.id = third.clone();
            store.upsert_job(&third_job).unwrap();
            mark_indexed(&store, &third, "complete");
            store.conn.execute("INSERT INTO possible_duplicates(job_a,job_b,score,reason,status,preferred_job) VALUES(?1,?2,100,'fixture','same',?2)", params![other.as_str(), third.as_str()]).unwrap();
            store
                .approve_email_handoff(&EmailHandoffApproval {
                    id: "third-approval",
                    job_id: &third,
                    message_hash: &digest,
                    sender_hash: &digest,
                    recipient_hash: &digest,
                    context_hash: &digest,
                    created_at: 1,
                    explicit_user_gesture: true,
                    acknowledge_incomplete_coverage: false,
                })
                .unwrap();
            let transitive = EmailHandoffReservation {
                id: "third-attempt",
                job_id: &third,
                approval_id: "third-approval",
                message_hash: &digest,
                sender_hash: &digest,
                recipient_hash: &digest,
                context_hash: &digest,
                created_at: 3,
            };
            assert!(store.reserve_email_handoff(&transitive).is_err());
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
            assert!(store
                .reconcile_email_handoff(
                    &job,
                    "one",
                    EmailReconciliation::UserReportsNotSent,
                    4,
                    false
                )
                .is_err());
            assert!(store
                .reconcile_email_handoff(
                    &other,
                    "one",
                    EmailReconciliation::UserReportsNotSent,
                    4,
                    true
                )
                .is_err());
            store
                .reconcile_email_handoff(
                    &job,
                    "one",
                    EmailReconciliation::UserReportsNotSent,
                    4,
                    true,
                )
                .unwrap();
            let history = store.email_handoffs_for_job(&job).unwrap().0;
            assert_eq!(history[0].state, "not_sent");
            assert_eq!(
                history[0].reconciliation.as_deref(),
                Some("user_reports_not_sent")
            );
            assert!(store
                .reconcile_email_handoff(&job, "one", EmailReconciliation::UserReportsSent, 5, true)
                .is_err());
            assert!(store
                .conn
                .execute("DELETE FROM email_handoff_reconciliations", [])
                .is_err());
        }
        std::fs::remove_file(path).unwrap();
    }
}
