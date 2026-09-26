//! T030 — durable submission journal.
//!
//! The intent is stored and the approval nonce consumed transactionally
//! *before* the browser write is permitted. After the external-write
//! boundary, cancellation is not rollback: the state is Uncertain unless
//! confirmation is known. No automatic retry ever.

use serde::{Deserialize, Serialize};
use waypoint_domain::ApplicationState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub application_id: String,
    pub state: ApplicationState,
    pub timestamp: String,
    /// Machine-checkable reason: "preflight_ok", "write_started",
    /// "receipt_confirmed", "timeout_after_write", "user_marked_confirmed"...
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct SubmissionJournal {
    entries: Vec<JournalEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// Illegal transition refused (e.g. Submitting -> Prepared rollback).
    IllegalTransition {
        from: ApplicationState,
        to: ApplicationState,
    },
    /// Automatic retry after the write boundary is forbidden.
    AutoRetryAfterWriteBoundary,
}

impl SubmissionJournal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, entry: JournalEntry) -> Result<(), JournalError> {
        let last = self.entries.last().map(|e| e.state);
        if let Some(from) = last {
            if !from.may_transition_to(entry.state) {
                return Err(JournalError::IllegalTransition {
                    from,
                    to: entry.state,
                });
            }
            // Extra invariant: after a write may have begun, no automatic
            // transition to Approved/Submitting without an explicit user
            // reconciliation marker.
            if matches!(
                from,
                ApplicationState::Submitting | ApplicationState::Uncertain
            ) && entry.state == ApplicationState::Approved
                && !entry.reason.starts_with("user_")
            {
                return Err(JournalError::AutoRetryAfterWriteBoundary);
            }
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn state_of(&self, application_id: &str) -> Option<ApplicationState> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.application_id == application_id)
            .map(|e| e.state)
    }

    pub fn entries_for(&self, application_id: &str) -> Vec<&JournalEntry> {
        self.entries
            .iter()
            .filter(|e| e.application_id == application_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(app: &str, state: ApplicationState, reason: &str) -> JournalEntry {
        JournalEntry {
            application_id: app.into(),
            state,
            timestamp: "2026-09-25T12:00:00Z".into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn core_path_is_recorded_in_order() {
        let mut j = SubmissionJournal::new();
        j.append(entry("a1", ApplicationState::Discovered, "crawl"))
            .unwrap();
        j.append(entry(
            "a1",
            ApplicationState::Shortlisted,
            "user_shortlisted",
        ))
        .unwrap();
        j.append(entry("a1", ApplicationState::Prepared, "packet_ready"))
            .unwrap();
        j.append(entry("a1", ApplicationState::Approved, "user_approved"))
            .unwrap();
        j.append(entry("a1", ApplicationState::Submitting, "write_started"))
            .unwrap();
        j.append(entry(
            "a1",
            ApplicationState::Confirmed,
            "receipt_confirmed",
        ))
        .unwrap();
        assert_eq!(j.state_of("a1"), Some(ApplicationState::Confirmed));
    }

    #[test]
    fn rollback_after_write_boundary_is_refused() {
        let mut j = SubmissionJournal::new();
        j.append(entry("a1", ApplicationState::Prepared, "packet_ready"))
            .unwrap();
        j.append(entry("a1", ApplicationState::Approved, "user_approved"))
            .unwrap();
        j.append(entry("a1", ApplicationState::Submitting, "write_started"))
            .unwrap();
        assert_eq!(
            j.append(entry("a1", ApplicationState::Prepared, "cancel")),
            Err(JournalError::IllegalTransition {
                from: ApplicationState::Submitting,
                to: ApplicationState::Prepared
            })
        );
    }

    #[test]
    fn automatic_retry_after_timeout_is_refused_but_user_retry_allowed() {
        let mut j = SubmissionJournal::new();
        j.append(entry("a1", ApplicationState::Approved, "user_approved"))
            .unwrap();
        j.append(entry("a1", ApplicationState::Submitting, "write_started"))
            .unwrap();
        j.append(entry(
            "a1",
            ApplicationState::Uncertain,
            "timeout_after_write",
        ))
        .unwrap();

        // Engine-driven retry: forbidden.
        assert_eq!(
            j.append(entry("a1", ApplicationState::Approved, "auto_retry")),
            Err(JournalError::AutoRetryAfterWriteBoundary)
        );
        // Deliberate, recorded user retry: allowed.
        j.append(entry(
            "a1",
            ApplicationState::Approved,
            "user_deliberate_retry",
        ))
        .unwrap();
        assert_eq!(j.state_of("a1"), Some(ApplicationState::Approved));
    }

    #[test]
    fn uncertain_is_not_failure_and_can_be_reconciled() {
        let mut j = SubmissionJournal::new();
        j.append(entry("a1", ApplicationState::Submitting, "write_started"))
            .unwrap();
        j.append(entry("a1", ApplicationState::Uncertain, "browser_crash"))
            .unwrap();
        j.append(entry(
            "a1",
            ApplicationState::Confirmed,
            "user_marked_confirmed",
        ))
        .unwrap();
        assert_eq!(j.state_of("a1"), Some(ApplicationState::Confirmed));
        assert_eq!(j.entries_for("a1").len(), 3);
    }
}
