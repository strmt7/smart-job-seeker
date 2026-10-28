//! Approval-bound orchestration for future application email delivery.
//! There is no network client here and no automatic retry.
use crate::Workspace;
use waypoint_domain::{sha256_hex, JobIdentityId, Sha256Digest};
use waypoint_store::email_handoff::{
    EmailHandoffApproval, EmailHandoffOutcome, EmailHandoffReservation,
};

pub struct ReviewedEmail<'a> {
    pub sender: &'a str,
    pub recipient: &'a str,
    pub mime: &'a [u8],
}

struct Digests {
    message: Sha256Digest,
    sender: Sha256Digest,
    recipient: Sha256Digest,
}
fn digests(email: &ReviewedEmail<'_>) -> Result<Digests, String> {
    if email.sender.is_empty()
        || email.sender.len() > 254
        || email.recipient.is_empty()
        || email.recipient.len() > 254
        || email.mime.is_empty()
        || email.mime.len() > 15 * 1024 * 1024
    {
        return Err("Email packet is outside permitted limits".into());
    }
    Ok(Digests {
        message: Sha256Digest::new(sha256_hex(email.mime)).map_err(|e| e.to_string())?,
        sender: Sha256Digest::of(email.sender),
        recipient: Sha256Digest::of(email.recipient),
    })
}

impl Workspace {
    /// A conservative snapshot of the job and candidate data used for review.
    /// Any change requires the email to be reviewed and approved again.
    pub fn email_review_context(&self, job: &JobIdentityId) -> Result<Sha256Digest, String> {
        let job_data = self
            .job(job)
            .map_err(|e| e.to_string())?
            .ok_or("Job not found")?;
        let description = self.job_description(job).map_err(|e| e.to_string())?;
        let profile = self.candidate_profile().map_err(|e| e.to_string())?;
        let mut facts = self.facts().map_err(|e| e.to_string())?;
        facts.sort_by(|a, b| a.id.cmp(&b.id));
        let facts: Vec<_> = facts.into_iter().map(|fact| serde_json::json!({"id":fact.id,"revision":fact.profile_revision,"text":fact.text,"status":format!("{:?}",fact.status),"source":format!("{:?}",fact.source_kind),"requires_review":fact.requires_review})).collect();
        Ok(Sha256Digest::of(&serde_json::json!({"job":job_data,"description":description,"profile":profile,"facts":facts}).to_string()))
    }
    /// The caller must present the exact email and receive a user gesture.
    pub fn approve_reviewed_email(
        &mut self,
        job: &JobIdentityId,
        approval_id: &str,
        email: &ReviewedEmail<'_>,
        now: i64,
        explicit_user_gesture: bool,
    ) -> Result<(), String> {
        let digest = digests(email)?;
        let context = self.email_review_context(job)?;
        self.store.approve_email_handoff(&EmailHandoffApproval {
            id: approval_id,
            job_id: job,
            message_hash: &digest.message,
            sender_hash: &digest.sender,
            recipient_hash: &digest.recipient,
            context_hash: &context,
            created_at: now,
            explicit_user_gesture,
        })
    }

    /// Persist and spend approval before starting a transport worker.
    pub fn reserve_reviewed_email(
        &mut self,
        job: &JobIdentityId,
        approval_id: &str,
        handoff_id: &str,
        email: &ReviewedEmail<'_>,
        now: i64,
    ) -> Result<(), String> {
        let digest = digests(email)?;
        let context = self.email_review_context(job)?;
        self.store.reserve_email_handoff(&EmailHandoffReservation {
            id: handoff_id,
            job_id: job,
            approval_id,
            message_hash: &digest.message,
            sender_hash: &digest.sender,
            recipient_hash: &digest.recipient,
            context_hash: &context,
            created_at: now,
        })
    }

    /// Persist the one observed outcome. Failure leaves the durable reservation
    /// unresolved; it must not cause the transport to be invoked again.
    pub fn finish_reviewed_email(
        &mut self,
        handoff_id: &str,
        outcome: EmailHandoffOutcome,
    ) -> Result<(), String> {
        let finished = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .ok().and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .ok_or("Cannot timestamp email handoff; treat delivery as uncertain and do not retry automatically")?;
        self.store.finish_email_handoff(handoff_id, outcome, finished).map_err(|_| "Email handoff outcome could not be recorded; treat delivery as uncertain and do not retry automatically".to_owned())
    }

    /// Synchronous coordinator for local fixtures and bounded worker callers.
    pub fn handoff_reviewed_email<F>(
        &mut self,
        job: &JobIdentityId,
        approval_id: &str,
        handoff_id: &str,
        email: &ReviewedEmail<'_>,
        now: i64,
        transport: F,
    ) -> Result<EmailHandoffOutcome, String>
    where
        F: FnOnce(&ReviewedEmail<'_>) -> EmailHandoffOutcome,
    {
        self.reserve_reviewed_email(job, approval_id, handoff_id, email, now)?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| transport(email)))
            .unwrap_or(EmailHandoffOutcome::Uncertain);
        self.finish_reviewed_email(handoff_id, outcome)?;
        Ok(outcome)
    }

    pub fn email_handoff_history(
        &self,
        job: &JobIdentityId,
    ) -> Result<(Vec<waypoint_store::email_handoff::EmailHandoffRecord>, bool), String> {
        self.store
            .email_handoffs_for_job(job)
            .map_err(|e| e.to_string())
    }

    pub fn reconcile_email_handoff(
        &mut self,
        job: &JobIdentityId,
        id: &str,
        decision: waypoint_store::email_handoff::EmailReconciliation,
        now: i64,
        explicit_user_gesture: bool,
    ) -> Result<(), String> {
        self.store
            .reconcile_email_handoff(job, id, decision, now, explicit_user_gesture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use waypoint_domain::ApplicationState;
    #[test]
    fn changing_candidate_facts_invalidates_email_approval() {
        let mut ws = Workspace::in_memory().unwrap();
        ws.test_insert_job("fact-fixture", "Role", "Fixture");
        let job = JobIdentityId::new("fact-fixture").unwrap();
        ws.set_state(&job, ApplicationState::Shortlisted).unwrap();
        let email = ReviewedEmail {
            sender: "sender@example.invalid",
            recipient: "recipient@example.invalid",
            mime: b"Subject: Fixture\r\n\r\nHi\r\n",
        };
        ws.approve_reviewed_email(&job, "fact-approval", &email, 1, true)
            .unwrap();
        ws.add_fact("Fictional qualification", crate::FactOrigin::UserAttested)
            .unwrap();
        assert!(ws
            .handoff_reviewed_email(
                &job,
                "fact-approval",
                "fact-attempt",
                &email,
                2,
                |_| panic!("changed facts must invalidate the approval")
            )
            .is_err());
    }
    #[test]
    fn exact_approval_is_spent_before_one_transport_and_panic_is_uncertain() {
        let mut ws = Workspace::in_memory().unwrap();
        ws.test_insert_job("fixture", "Role", "Fixture");
        let job = JobIdentityId::new("fixture").unwrap();
        ws.set_state(&job, ApplicationState::Shortlisted).unwrap();
        ws.save_candidate_profile("{\"name\":\"Initial\"}").unwrap();
        let email = ReviewedEmail {
            sender: "sender@example.invalid",
            recipient: "recipient@example.invalid",
            mime: b"Subject: Fixture\r\n\r\nHi\r\n",
        };
        assert!(ws
            .approve_reviewed_email(&job, "approval", &email, 1, false)
            .is_err());
        ws.approve_reviewed_email(&job, "approval", &email, 1, true)
            .unwrap();
        ws.save_candidate_profile("{\"name\":\"Changed\"}").unwrap();
        assert!(ws
            .handoff_reviewed_email(&job, "approval", "stale", &email, 2, |_| panic!(
                "stale context must not call transport"
            ))
            .is_err());
        ws.save_candidate_profile("{\"name\":\"Initial\"}").unwrap();
        assert!(ws
            .handoff_reviewed_email(&job, "approval", "restored", &email, 2, |_| panic!(
                "old approval must remain invalidated"
            ))
            .is_err());
        ws.approve_reviewed_email(&job, "fresh-approval", &email, 1, true)
            .unwrap();
        let changed = ReviewedEmail {
            recipient: "other@example.invalid",
            ..email
        };
        let called = Cell::new(false);
        assert!(ws
            .handoff_reviewed_email(&job, "fresh-approval", "attempt", &changed, 2, |_| {
                called.set(true);
                EmailHandoffOutcome::Accepted
            })
            .is_err());
        assert!(!called.get());
        let outcome = ws
            .handoff_reviewed_email(&job, "fresh-approval", "attempt", &email, 2, |_| {
                called.set(true);
                panic!("simulated transport failure")
            })
            .unwrap();
        assert_eq!(outcome, EmailHandoffOutcome::Uncertain);
        assert!(called.get());
        assert_eq!(
            ws.email_handoff_history(&job).unwrap().0[0].state,
            "uncertain"
        );
        assert!(ws
            .handoff_reviewed_email(&job, "approval", "second", &email, 3, |_| {
                EmailHandoffOutcome::Accepted
            })
            .is_err());
    }
}
