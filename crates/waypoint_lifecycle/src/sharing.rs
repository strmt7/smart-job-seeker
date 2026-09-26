//! T043 — scoped review sharing. A coach receives a narrow, expiring view
//! chosen by the candidate; sensitive declarations are never exported
//! silently; what a share contains is shown before it leaves.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPacket {
    pub share_id: String,
    pub includes_profile: bool,
    pub includes_documents: Vec<String>,
    pub includes_applications: Vec<String>,
    pub expires_at: String,
    pub redactions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareError {
    NothingSelected,
    ExpiryMissing,
}

pub fn redact_for_review(
    share_id: &str,
    includes_profile: bool,
    documents: &[String],
    applications: &[String],
    expires_at: &str,
) -> Result<ReviewPacket, ShareError> {
    if expires_at.trim().is_empty() {
        return Err(ShareError::ExpiryMissing);
    }
    if !includes_profile && documents.is_empty() && applications.is_empty() {
        return Err(ShareError::NothingSelected);
    }
    Ok(ReviewPacket {
        share_id: share_id.into(),
        includes_profile,
        includes_documents: documents.to_vec(),
        includes_applications: applications.to_vec(),
        expires_at: expires_at.into(),
        // Sensitive declarations (salary history, authorization, demographic,
        // accommodations) are always redacted unless separately consented.
        redactions: vec![
            "salary_history".into(),
            "work_authorization_details".into(),
            "demographic_answers".into(),
            "accommodation_requests".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_requires_expiry_and_content() {
        assert_eq!(
            redact_for_review("s1", false, &[], &[], "2026-10-01"),
            Err(ShareError::NothingSelected)
        );
        assert_eq!(
            redact_for_review("s1", true, &[], &[], " "),
            Err(ShareError::ExpiryMissing)
        );
    }

    #[test]
    fn sensitive_categories_always_redacted() {
        let p = redact_for_review(
            "s1",
            true,
            &["cv-1".into()],
            &["app-1".into()],
            "2026-10-01",
        )
        .unwrap();
        assert!(p
            .redactions
            .contains(&"work_authorization_details".to_string()));
        assert_eq!(p.includes_documents, vec!["cv-1".to_string()]);
    }
}
