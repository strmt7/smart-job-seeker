pub mod domain;
pub mod pipeline;
pub mod scoring;

pub use domain::{JobOpportunity, UserProfile};
pub use pipeline::{rank_jobs, RankedJob};
pub use scoring::Recommendation;
