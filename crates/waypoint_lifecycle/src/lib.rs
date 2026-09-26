//! Lifecycle features (gate G5, T035–T043).
//!
//! Everything here is proposal-based: connectors and the model PROPOSE
//! timeline updates, mails, events and followups; Rust policy + explicit
//! user approval make them real. A rejection for one role never rejects all
//! roles at a company. Analytics show denominators and censoring honestly.

pub mod analytics;
pub mod calendar;
pub mod contacts;
pub mod inbox;
pub mod interview;
pub mod learning;
pub mod offers;
pub mod sharing;
pub mod timeline;

pub use analytics::{censoring_note, OutcomeCounts};
pub use calendar::{check_conflicts, ProposedEvent};
pub use contacts::{Contact, FollowupProposal};
pub use inbox::{IngestScope, MailConsent, ProposedTimelineUpdate};
pub use interview::{build_practice_set, InterviewQuestion};
pub use learning::{map_gaps, GapActivity};
pub use offers::{compare_offers, OfferComponent, OfferScenario};
pub use sharing::{redact_for_review, ReviewPacket};
pub use timeline::{reconcile, TimelineEvent};
