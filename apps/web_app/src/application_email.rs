//! Reviewed application-email packet and Gmail transport adapter.
//! No HTTP endpoint calls this module; callers must preserve an attempt record
//! and run the blocking transport on a bounded worker before exposing it.
use serde::Deserialize;
use waypoint_domain::JobIdentityId;
use waypoint_store::email_handoff::EmailHandoffOutcome;
use waypoint_workspace::{email_handoff::ReviewedEmail, Workspace};
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
struct Credentials {
    auth_type: String,
    email: String,
    password: String,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

pub struct PreparedApplicationEmail {
    sender: String,
    recipient: String,
    mime: Vec<u8>,
}

impl PreparedApplicationEmail {
    /// Construct a packet once; subsequent approval and handoff use its exact bytes.
    pub fn prepare(
        sender: &str,
        recipient: &str,
        subject: &str,
        body: &str,
        attachments: &[(String, Vec<u8>)],
        id: &str,
    ) -> Result<Self, String> {
        let recipient = recipient.trim();
        if !crate::smtp_handoff::address_valid(sender)
            || !crate::smtp_handoff::address_valid(recipient)
            || id.is_empty()
            || id.len() > 64
            || !id.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid sender, recipient, or packet identity".into());
        }
        let draft = crate::email_draft::build(recipient, subject, body, attachments, id)?;
        let mut mime =
            format!("From: {sender}\r\nMessage-ID: <waypoint-{id}@localhost>\r\n").into_bytes();
        mime.extend_from_slice(&draft);
        if mime.len() > 15 * 1024 * 1024 {
            return Err("Email packet exceeds the transport limit".into());
        }
        Ok(Self {
            sender: sender.to_owned(),
            recipient: recipient.to_owned(),
            mime,
        })
    }
    fn reviewed(&self) -> ReviewedEmail<'_> {
        ReviewedEmail {
            sender: &self.sender,
            recipient: &self.recipient,
            mime: &self.mime,
        }
    }
    /// Packet bytes are suitable for saving in the application's attempt folder.
    pub fn mime(&self) -> &[u8] {
        &self.mime
    }
    pub fn approve(
        &self,
        ws: &mut Workspace,
        job: &JobIdentityId,
        approval_id: &str,
        now: i64,
        explicit_user_gesture: bool,
    ) -> Result<(), String> {
        ws.approve_reviewed_email(
            job,
            approval_id,
            &self.reviewed(),
            now,
            explicit_user_gesture,
        )
    }
    fn handoff_with<F>(
        &self,
        ws: &mut Workspace,
        job: &JobIdentityId,
        approval_id: &str,
        handoff_id: &str,
        now: i64,
        transport: F,
    ) -> Result<EmailHandoffOutcome, String>
    where
        F: FnOnce(&ReviewedEmail<'_>) -> EmailHandoffOutcome,
    {
        ws.handoff_reviewed_email(
            job,
            approval_id,
            handoff_id,
            &self.reviewed(),
            now,
            transport,
        )
    }
    /// Sends at most once after consuming a matching approval. The caller must
    /// execute this on a bounded worker and surface uncertain results for review.
    pub fn handoff_gmail(
        &self,
        ws: &mut Workspace,
        job: &JobIdentityId,
        approval_id: &str,
        handoff_id: &str,
        now: i64,
    ) -> Result<EmailHandoffOutcome, String> {
        let bytes = Zeroizing::new(
            ws.gmail_credentials()
                .map_err(|_| "Cannot read Gmail credentials")?
                .ok_or("Connect Gmail first")?,
        );
        let credentials: Credentials = serde_json::from_slice(&bytes)
            .map_err(|_| "Application email currently requires a Gmail app-password connection")?;
        if credentials.auth_type != "app_password" || credentials.email != self.sender {
            return Err("Connected Gmail account does not match the reviewed sender".into());
        }
        self.handoff_with(ws, job, approval_id, handoff_id, now, |email| {
            let Ok(mut stream) =
                crate::gmail_password::open_smtp(&credentials.email, &credentials.password)
            else {
                return EmailHandoffOutcome::NotSent;
            };
            match crate::smtp_handoff::handoff(
                &mut stream,
                email.sender,
                email.recipient,
                email.mime,
            ) {
                Ok(()) => EmailHandoffOutcome::Accepted,
                Err(crate::smtp_handoff::HandoffError::Uncertain) => EmailHandoffOutcome::Uncertain,
                Err(_) => EmailHandoffOutcome::NotSent,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use waypoint_domain::ApplicationState;
    struct Server {
        responses: std::io::Cursor<Vec<u8>>,
        written: Vec<u8>,
    }
    impl Read for Server {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.responses.read(b)
        }
    }
    impl Write for Server {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn reviewed_pdf_packet_is_handed_off_once_with_a_simulated_server() {
        let mut ws = Workspace::in_memory().unwrap();
        let job = JobIdentityId::new("email-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
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
        let packet = PreparedApplicationEmail::prepare(
            "sender@example.invalid",
            "recipient@example.invalid",
            "Application",
            "Hello",
            &[("cv.pdf".into(), b"%PDF-fictional".to_vec())],
            "abc123",
        )
        .unwrap();
        packet.approve(&mut ws, &job, "approval", 1, true).unwrap();
        let mut server = Server {
            responses: std::io::Cursor::new(
                b"250 ok\r\n250 ok\r\n354 go\r\n250 accepted\r\n".to_vec(),
            ),
            written: vec![],
        };
        let outcome = packet
            .handoff_with(&mut ws, &job, "approval", "attempt", 2, |email| {
                match crate::smtp_handoff::handoff(
                    &mut server,
                    email.sender,
                    email.recipient,
                    email.mime,
                ) {
                    Ok(()) => EmailHandoffOutcome::Accepted,
                    Err(_) => EmailHandoffOutcome::Uncertain,
                }
            })
            .unwrap();
        assert_eq!(outcome, EmailHandoffOutcome::Accepted);
        assert_eq!(
            ws.email_handoff_history(&job).unwrap().0[0].state,
            "accepted"
        );
        assert!(server
            .written
            .windows(b"Content-Type: application/pdf".len())
            .any(|p| p == b"Content-Type: application/pdf"));
        assert!(packet
            .handoff_with(&mut ws, &job, "approval", "duplicate", 3, |_| panic!(
                "must not send twice"
            ))
            .is_err());
    }
}
