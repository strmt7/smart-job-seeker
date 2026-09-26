//! T031 — receipt verification.
//!
//! "Confirmed" requires *qualifying* evidence: a stable application id on a
//! confirmation page, an employer acknowledgement email, or a permitted API
//! result. A generic marketing "success" page is NOT enough.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptKind {
    /// Confirmation page carrying a stable application identifier.
    ConfirmationPageWithId,
    EmployerAcknowledgementEmail,
    PermittedApiResult,
    /// Screenshot alone — supporting, never sufficient.
    ScreenshotOnly,
    /// Generic success text without stable identifier.
    GenericSuccessText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptVerdict {
    Confirmed { application_ref: String },
    NotSufficient { why: String },
}

pub fn verify_receipt(kind: ReceiptKind, payload: &str) -> ReceiptVerdict {
    match kind {
        ReceiptKind::ConfirmationPageWithId => {
            // The adapter must have extracted a stable identifier; policy:
            // non-empty, reasonably short, no whitespace-only.
            let id = payload.trim();
            if id.is_empty() || id.len() > 128 {
                return ReceiptVerdict::NotSufficient {
                    why: "confirmation page lacks a stable application identifier".into(),
                };
            }
            ReceiptVerdict::Confirmed {
                application_ref: id.to_string(),
            }
        }
        ReceiptKind::EmployerAcknowledgementEmail => {
            let lower = payload.to_lowercase();
            if lower.contains("application")
                && (lower.contains("received") || lower.contains("thank you"))
                && lower.contains('@')
            {
                ReceiptVerdict::Confirmed {
                    application_ref: format!(
                        "ack-email:{}",
                        payload.trim().chars().take(64).collect::<String>()
                    ),
                }
            } else {
                ReceiptVerdict::NotSufficient {
                    why: "email does not look like an application acknowledgement".into(),
                }
            }
        }
        ReceiptKind::PermittedApiResult => {
            // JSON with a non-empty id field qualifies.
            let v: serde_json::Value = match serde_json::from_str(payload) {
                Ok(v) => v,
                Err(_) => {
                    return ReceiptVerdict::NotSufficient {
                        why: "api result is not valid JSON".into(),
                    }
                }
            };
            let id = v
                .get("id")
                .or_else(|| v.get("applicationId"))
                .or_else(|| v.get("application_id"))
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            match id {
                Some(id) => ReceiptVerdict::Confirmed {
                    application_ref: id,
                },
                None => ReceiptVerdict::NotSufficient {
                    why: "api result carries no application id".into(),
                },
            }
        }
        ReceiptKind::ScreenshotOnly | ReceiptKind::GenericSuccessText => {
            ReceiptVerdict::NotSufficient {
                why: "screenshots and generic success text are not qualifying receipt evidence"
                    .into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_page_with_stable_id_confirms() {
        match verify_receipt(ReceiptKind::ConfirmationPageWithId, "  APP-88421  ") {
            ReceiptVerdict::Confirmed { application_ref } => {
                assert_eq!(application_ref, "APP-88421")
            }
            v => panic!("expected confirm, got {v:?}"),
        }
    }

    #[test]
    fn generic_success_text_never_confirms() {
        assert!(matches!(
            verify_receipt(ReceiptKind::GenericSuccessText, "Success! We got it."),
            ReceiptVerdict::NotSufficient { .. }
        ));
        assert!(matches!(
            verify_receipt(ReceiptKind::ScreenshotOnly, "image/png bytes"),
            ReceiptVerdict::NotSufficient { .. }
        ));
    }

    #[test]
    fn acknowledgement_email_needs_received_language_and_address() {
        assert!(matches!(
            verify_receipt(
                ReceiptKind::EmployerAcknowledgementEmail,
                "Thanks — your application has been received. careers@acme.com"
            ),
            ReceiptVerdict::Confirmed { .. }
        ));
        assert!(matches!(
            verify_receipt(
                ReceiptKind::EmployerAcknowledgementEmail,
                "Re: lunch plans tomorrow"
            ),
            ReceiptVerdict::NotSufficient { .. }
        ));
    }

    #[test]
    fn api_result_needs_an_id_field() {
        assert!(matches!(
            verify_receipt(
                ReceiptKind::PermittedApiResult,
                r#"{"id":"GH-77","status":"ok"}"#
            ),
            ReceiptVerdict::Confirmed { .. }
        ));
        assert!(matches!(
            verify_receipt(ReceiptKind::PermittedApiResult, r#"{"status":"ok"}"#),
            ReceiptVerdict::NotSufficient { .. }
        ));
        assert!(matches!(
            verify_receipt(ReceiptKind::PermittedApiResult, "not json"),
            ReceiptVerdict::NotSufficient { .. }
        ));
    }
}
