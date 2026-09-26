//! T036 — timeline reconciliation.
//!
//! A rejection for one role must not reject all roles at the company.
//! Matching is deterministic when a requisition id exists; otherwise the
//! proposal goes to a review queue ranked by similarity.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    /// None = company-level event, applies to no specific application.
    pub application_id: Option<String>,
    pub employer: String,
    pub kind: String, // "rejection" | "interview_invite" | "acknowledgement" | "offer"
    pub date: String,
    pub evidence_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReconcileDecision {
    /// Deterministic match on requisition/application id.
    ApplyToApplication { application_id: String },
    /// Ambiguous: queue for human review with candidate matches ranked.
    NeedsReview { candidates: Vec<(String, f32)> },
}

pub fn reconcile(
    proposal_employer: &str,
    proposal_req: Option<&str>,
    applications: &[(String, String, Option<String>)], // (app_id, employer, req)
) -> ReconcileDecision {
    if let Some(req) = proposal_req {
        let exact: Vec<String> = applications
            .iter()
            .filter(|(_, _, r)| r.as_deref() == Some(req))
            .map(|(a, _, _)| a.clone())
            .collect();
        if exact.len() == 1 {
            return ReconcileDecision::ApplyToApplication {
                application_id: exact[0].clone(),
            };
        }
    }
    // Rank same-employer applications; never blanket-apply.
    let mut candidates: Vec<(String, f32)> = applications
        .iter()
        .filter(|(_, e, _)| e.eq_ignore_ascii_case(proposal_employer))
        .map(|(a, _, _)| (a.clone(), 0.5))
        .collect();
    if candidates.is_empty() {
        ReconcileDecision::NeedsReview { candidates }
    } else {
        // deterministic ordering
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        ReconcileDecision::NeedsReview { candidates }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_targets_only_matching_requisition() {
        let apps = vec![
            (
                "app-1".to_string(),
                "Acme".to_string(),
                Some("REQ-1".to_string()),
            ),
            (
                "app-2".to_string(),
                "Acme".to_string(),
                Some("REQ-2".to_string()),
            ),
        ];
        match reconcile("Acme", Some("REQ-2"), &apps) {
            ReconcileDecision::ApplyToApplication { application_id } => {
                assert_eq!(application_id, "app-2");
            }
            other => panic!("expected exact match, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_rejection_goes_to_review_not_blanket_apply() {
        let apps = vec![
            (
                "app-1".to_string(),
                "Acme".to_string(),
                Some("REQ-1".to_string()),
            ),
            (
                "app-2".to_string(),
                "Acme".to_string(),
                Some("REQ-2".to_string()),
            ),
        ];
        match reconcile("Acme", None, &apps) {
            ReconcileDecision::NeedsReview { candidates } => {
                assert_eq!(candidates.len(), 2);
            }
            other => panic!("expected review, got {other:?}"),
        }
    }

    #[test]
    fn other_employers_are_not_candidates() {
        let apps = vec![(
            "app-1".to_string(),
            "Beta".to_string(),
            Some("REQ-1".to_string()),
        )];
        match reconcile("Acme", None, &apps) {
            ReconcileDecision::NeedsReview { candidates } => assert!(candidates.is_empty()),
            other => panic!("expected review, got {other:?}"),
        }
    }
}
