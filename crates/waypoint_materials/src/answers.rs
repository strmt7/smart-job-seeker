//! T026 — answer library.
//!
//! Reuse offers a *suggestion with provenance*; it never silently copies an
//! old salary expectation or authorization answer into a new employer's form
//! (§8). Sensitive categories always require a fresh explicit answer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerCategory {
    Motivation,
    Behavioral,
    Technical,
    Compensation,
    WorkAuthorization,
    Availability,
    Demographic,
    ConflictOfInterest,
    LegalAttestation,
}

impl AnswerCategory {
    /// Answers whose reuse across employers is forbidden outright.
    pub fn requires_fresh_answer(self) -> bool {
        matches!(
            self,
            AnswerCategory::Compensation
                | AnswerCategory::WorkAuthorization
                | AnswerCategory::Availability
                | AnswerCategory::Demographic
                | AnswerCategory::ConflictOfInterest
                | AnswerCategory::LegalAttestation
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryAnswer {
    pub id: String,
    pub category: AnswerCategory,
    /// Normalized question meaning, e.g. "tell_about_gap_in_cv".
    pub question_meaning: String,
    pub text: String,
    pub created_for_employer: Option<String>,
    pub created_at: String,
    pub claim_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionDecision {
    /// Offer as a suggestion with provenance shown.
    Suggest {
        answer_id: String,
        provenance: String,
    },
    /// Category requires a fresh answer for this employer.
    RequireFresh { category: AnswerCategory },
}

pub fn reuse_decision(answer: &LibraryAnswer, new_employer: &str) -> SuggestionDecision {
    if answer.category.requires_fresh_answer() {
        return SuggestionDecision::RequireFresh {
            category: answer.category,
        };
    }
    // Never suggest an answer written *for the same employer* as a "reuse"
    // provenance string would be misleading; also never cross-employer for
    // sensitive categories (already handled above).
    if answer.created_for_employer.as_deref() == Some(new_employer) {
        return SuggestionDecision::Suggest {
            answer_id: answer.id.clone(),
            provenance: format!("previously written for {new_employer}"),
        };
    }
    SuggestionDecision::Suggest {
        answer_id: answer.id.clone(),
        provenance: format!(
            "written for {} (question: {})",
            answer
                .created_for_employer
                .as_deref()
                .unwrap_or("an earlier application"),
            answer.question_meaning
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(cat: AnswerCategory, employer: Option<&str>) -> LibraryAnswer {
        LibraryAnswer {
            id: "a1".into(),
            category: cat,
            question_meaning: "motivation".into(),
            text: "Because.".into(),
            created_for_employer: employer.map(Into::into),
            created_at: "2026-01-01".into(),
            claim_ids: vec![],
        }
    }

    #[test]
    fn compensation_is_never_silently_reused() {
        let a = answer(AnswerCategory::Compensation, Some("Acme"));
        assert_eq!(
            reuse_decision(&a, "Beta"),
            SuggestionDecision::RequireFresh {
                category: AnswerCategory::Compensation
            }
        );
    }

    #[test]
    fn authorization_and_demographic_require_fresh() {
        for cat in [
            AnswerCategory::WorkAuthorization,
            AnswerCategory::Demographic,
            AnswerCategory::ConflictOfInterest,
            AnswerCategory::LegalAttestation,
        ] {
            let a = answer(cat, Some("Acme"));
            assert!(matches!(
                reuse_decision(&a, "Beta"),
                SuggestionDecision::RequireFresh { .. }
            ));
        }
    }

    #[test]
    fn motivation_suggestion_carries_provenance() {
        let a = answer(AnswerCategory::Motivation, Some("Acme"));
        match reuse_decision(&a, "Beta") {
            SuggestionDecision::Suggest { provenance, .. } => {
                assert!(provenance.contains("Acme"));
                assert!(provenance.contains("motivation"));
            }
            other => panic!("expected suggestion, got {other:?}"),
        }
    }
}
