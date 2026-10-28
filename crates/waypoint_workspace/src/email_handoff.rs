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
        self.store.approve_email_handoff(&EmailHandoffApproval {
            id: approval_id,
            job_id: job,
            message_hash: &digest.message,
            sender_hash: &digest.sender,
            recipient_hash: &digest.recipient,
            created_at: now,
            explicit_user_gesture,
        })
    }

    /// Reserve and spend the exact approval before invoking the supplied
    /// transport once. The caller must run this blocking work on a bounded
    /// worker, preserve an application record, and never auto-retry.
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
        let digest = digests(email)?;
        self.store.reserve_email_handoff(&EmailHandoffReservation {
            id: handoff_id,
            job_id: job,
            approval_id,
            message_hash: &digest.message,
            sender_hash: &digest.sender,
            recipient_hash: &digest.recipient,
            created_at: now,
        })?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| transport(email)))
            .unwrap_or(EmailHandoffOutcome::Uncertain);
        let finished = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .ok().and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .ok_or("Cannot timestamp email handoff; treat delivery as uncertain and do not retry automatically")?;
        self.store.finish_email_handoff(handoff_id, outcome, finished).map_err(|_| "Email handoff outcome could not be recorded; treat delivery as uncertain and do not retry automatically".to_owned())?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use waypoint_domain::ApplicationState;
    #[test]
    fn exact_approval_is_spent_before_one_transport_and_panic_is_uncertain() {
        let mut ws = Workspace::in_memory().unwrap();
        ws.test_insert_job("fixture", "Role", "Fixture");
        let job = JobIdentityId::new("fixture").unwrap();
        ws.set_state(&job, ApplicationState::Shortlisted).unwrap();
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
        let changed = ReviewedEmail {
            recipient: "other@example.invalid",
            ..email
        };
        let called = Cell::new(false);
        assert!(ws
            .handoff_reviewed_email(&job, "approval", "attempt", &changed, 2, |_| {
                called.set(true);
                EmailHandoffOutcome::Accepted
            })
            .is_err());
        assert!(!called.get());
        let outcome = ws
            .handoff_reviewed_email(&job, "approval", "attempt", &email, 2, |_| {
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
