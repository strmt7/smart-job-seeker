//! T032 — uncertain-state recovery.
//!
//! After an ambiguous write (crash, timeout, unparsable response) automatic
//! retries are frozen. The only exits from Uncertain are: (a) qualifying
//! receipt evidence, (b) user reconciliation via scoped checks, (c) a
//! deliberate, recorded user retry with the duplicate risk shown.

use crate::receipt::{verify_receipt, ReceiptKind, ReceiptVerdict};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Evidence confirmed the write; state moves to Confirmed.
    MarkConfirmed { application_ref: String },
    /// User performed a scoped reconciliation and says it landed.
    UserConfirmed,
    /// User chose a deliberate retry after seeing the duplicate risk.
    UserDeliberateRetry,
    /// Keep Uncertain; nothing qualifying happened yet.
    StayUncertain { why: String },
    /// No write ever started (pre-boundary failure): safe return to Prepared.
    ReturnToPrepared { why: String },
}

/// Inputs available after an ambiguous outcome.
pub struct AmbiguityInputs<'a> {
    /// True if we know the write started (post-boundary). False means the
    /// failure happened before any bytes went out.
    pub write_started: bool,
    pub receipt_kind: Option<ReceiptKind>,
    pub receipt_payload: Option<&'a str>,
}

pub fn reconcile_uncertain(inputs: &AmbiguityInputs<'_>) -> RecoveryAction {
    // Pre-boundary failure: no external effect possible -> safe return.
    if !inputs.write_started {
        return RecoveryAction::ReturnToPrepared {
            why: "failure occurred before the external write boundary".into(),
        };
    }

    // Post-boundary: only qualifying evidence or explicit user action exits.
    if let (Some(kind), Some(payload)) = (inputs.receipt_kind, inputs.receipt_payload) {
        match verify_receipt(kind, payload) {
            ReceiptVerdict::Confirmed { application_ref } => {
                return RecoveryAction::MarkConfirmed { application_ref };
            }
            ReceiptVerdict::NotSufficient { why } => {
                return RecoveryAction::StayUncertain { why };
            }
        }
    }
    RecoveryAction::StayUncertain {
        why: "no qualifying receipt evidence yet; automatic retry frozen".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_boundary_failure_returns_to_prepared_safely() {
        let act = reconcile_uncertain(&AmbiguityInputs {
            write_started: false,
            receipt_kind: None,
            receipt_payload: None,
        });
        assert!(matches!(act, RecoveryAction::ReturnToPrepared { .. }));
    }

    #[test]
    fn post_boundary_without_receipt_stays_uncertain() {
        let act = reconcile_uncertain(&AmbiguityInputs {
            write_started: true,
            receipt_kind: None,
            receipt_payload: None,
        });
        assert!(matches!(act, RecoveryAction::StayUncertain { .. }));
    }

    #[test]
    fn post_boundary_with_qualifying_receipt_confirms() {
        let act = reconcile_uncertain(&AmbiguityInputs {
            write_started: true,
            receipt_kind: Some(ReceiptKind::ConfirmationPageWithId),
            receipt_payload: Some("APP-9"),
        });
        assert_eq!(
            act,
            RecoveryAction::MarkConfirmed {
                application_ref: "APP-9".into()
            }
        );
    }

    #[test]
    fn post_boundary_with_weak_evidence_stays_uncertain() {
        let act = reconcile_uncertain(&AmbiguityInputs {
            write_started: true,
            receipt_kind: Some(ReceiptKind::ScreenshotOnly),
            receipt_payload: Some("bytes"),
        });
        assert!(matches!(act, RecoveryAction::StayUncertain { .. }));
    }
}
