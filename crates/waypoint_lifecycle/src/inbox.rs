//! T035 — inbox consent and ingestion.
//!
//! Mail connectors are opt-in and purpose-limited: an explicit folder/label
//! scope, no "connect everything" button. Every imported message becomes a
//! PROPOSED timeline update for review, never an automatic state change.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailConsent {
    pub account: String,
    /// Explicit folders/labels the user scoped. Empty = nothing is read.
    pub folders: Vec<String>,
    /// e.g. "application_status_only"
    pub purpose: String,
    pub granted_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestScope {
    pub folder: String,
    pub senders_allowlist: Vec<String>, // e.g. careers@employer.com
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestError {
    FolderOutsideConsent { folder: String },
    ConsentExpired,
    SenderNotInAllowlist { sender: String },
}

/// May we read from this folder under this consent?
pub fn authorize_ingest(consent: &MailConsent, scope: &IngestScope) -> Result<(), IngestError> {
    if consent.expires_at.as_str() < consent.granted_at.as_str() {
        return Err(IngestError::ConsentExpired);
    }
    if !consent.folders.iter().any(|f| f == &scope.folder) {
        return Err(IngestError::FolderOutsideConsent {
            folder: scope.folder.clone(),
        });
    }
    Ok(())
}

/// A message turned into a *proposal*. Deterministic extraction only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedTimelineUpdate {
    pub message_id: String,
    pub employer: String,
    pub requisition_ref: Option<String>,
    pub event_kind: String, // "rejection" | "interview_invite" | "acknowledgement"
    pub date: String,
    pub source_snippet: String,
    pub confidence: f32,
}

/// Extract a proposal from a message. No PII leaves the store; the snippet is
/// capped and shown to the user for approval.
pub fn extract_proposal(
    scope: &IngestScope,
    sender: &str,
    subject: &str,
    body: &str,
    message_id: &str,
    date: &str,
) -> Result<ProposedTimelineUpdate, IngestError> {
    if !scope.senders_allowlist.iter().any(|s| sender == s) {
        return Err(IngestError::SenderNotInAllowlist {
            sender: sender.to_string(),
        });
    }
    let lower_subject = subject.to_lowercase();
    let (kind, conf) =
        if lower_subject.contains("regret") || lower_subject.contains("unfortunately") {
            ("rejection", 0.7)
        } else if lower_subject.contains("interview") {
            ("interview_invite", 0.8)
        } else if lower_subject.contains("application") || lower_subject.contains("received") {
            ("acknowledgement", 0.9)
        } else {
            ("other", 0.3)
        };
    Ok(ProposedTimelineUpdate {
        message_id: message_id.to_string(),
        employer: sender.split('@').nth(1).unwrap_or("unknown").to_string(),
        requisition_ref: None, // adapters may fill from body patterns
        event_kind: kind.into(),
        date: date.into(),
        source_snippet: body.chars().take(200).collect(),
        confidence: conf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consent() -> MailConsent {
        MailConsent {
            account: "user@example.com".into(),
            folders: vec!["INBOX/JobSearch".into()],
            purpose: "application_status_only".into(),
            granted_at: "2026-09-01".into(),
            expires_at: "2026-12-01".into(),
        }
    }

    #[test]
    fn only_consented_folders_are_readable() {
        let ok_scope = IngestScope {
            folder: "INBOX/JobSearch".into(),
            senders_allowlist: vec![],
        };
        assert!(authorize_ingest(&consent(), &ok_scope).is_ok());
        let bad_scope = IngestScope {
            folder: "INBOX/Personal".into(),
            senders_allowlist: vec![],
        };
        assert!(matches!(
            authorize_ingest(&consent(), &bad_scope),
            Err(IngestError::FolderOutsideConsent { .. })
        ));
    }

    #[test]
    fn unknown_senders_are_not_ingested() {
        let scope = IngestScope {
            folder: "INBOX/JobSearch".into(),
            senders_allowlist: vec!["careers@acme.com".into()],
        };
        assert!(extract_proposal(
            &scope,
            "careers@acme.com",
            "Your application",
            "We received...",
            "m1",
            "2026-09-20"
        )
        .is_ok());
        assert!(matches!(
            extract_proposal(
                &scope,
                "spam@evil.io",
                "Your application",
                "...",
                "m2",
                "2026-09-20"
            ),
            Err(IngestError::SenderNotInAllowlist { .. })
        ));
    }

    #[test]
    fn extraction_is_a_proposal_with_capped_snippet() {
        let scope = IngestScope {
            folder: "INBOX/JobSearch".into(),
            senders_allowlist: vec!["careers@acme.com".into()],
        };
        let long_body = "x".repeat(5000);
        let p = extract_proposal(
            &scope,
            "careers@acme.com",
            "Interview invitation",
            &long_body,
            "m3",
            "2026-09-21",
        )
        .unwrap();
        assert_eq!(p.event_kind, "interview_invite");
        assert_eq!(p.source_snippet.len(), 200);
        assert!(p.confidence > 0.0 && p.confidence <= 1.0);
    }
}
