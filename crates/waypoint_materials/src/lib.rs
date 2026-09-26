//! Materials studio (gate G3): semantic document tree, evidence validation,
//! and correction propagation (T021, T022, T025).
//!
//! D1 in one sentence: every sentence in every artifact must trace to a
//! claim; correcting a claim must identify every artifact that used it,
//! invalidate stale approvals, and never mutate an already submitted packet.

pub mod answers;
pub mod drafting;
pub mod export;
pub mod propagation;
pub mod tree;
pub mod validation;

pub use export::{to_docx_bytes, to_typst};
pub use propagation::{CorrectionImpact, CorrectionPlan};
pub use tree::{DocNode, DocNodeKind, SemanticDocument};
pub use validation::{validate_claim_support, ValidationOutcome};
