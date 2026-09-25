pub mod domain;
pub mod pipeline;
pub mod repository;
pub mod scoring;
pub mod service;

pub use domain::{JobOpportunity, UserProfile};
pub use pipeline::{rank_jobs, RankedJob};
pub use repository::InMemoryRepository;
pub use scoring::Recommendation;
pub use service::{JobSeekerService, ServiceError};
