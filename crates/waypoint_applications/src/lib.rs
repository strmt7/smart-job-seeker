//! Supervised application engine (gate G4, T027–T032).
//!
//! D2 in one sentence: an application is a legible transaction — prepared
//! packets are not applications, clicks are not confirmations, timeouts are
//! uncertainty not failure, and no ambiguous write is ever retried
//! automatically.

pub mod form_mapping;
pub mod journal;
pub mod preflight;
pub mod receipt;
pub mod recovery;

pub use form_mapping::{validate_mapping, BrowserCommand, FormSchema, MappingError, PacketValue};
pub use journal::{JournalEntry, SubmissionJournal};
pub use preflight::{preflight_check, PreflightIssue};
pub use receipt::{verify_receipt, ReceiptKind, ReceiptVerdict};
pub use recovery::{reconcile_uncertain, RecoveryAction};
