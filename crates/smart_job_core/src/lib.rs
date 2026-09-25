pub mod domain;
pub mod pipeline;
pub mod repository;
pub mod scoring;
pub mod service;
pub mod sqlite_repository;

pub use domain::{JobOpportunity, UserProfile};
pub use pipeline::{rank_jobs, RankedJob};
pub use repository::{InMemoryRepository, RepositoryError};
pub use scoring::Recommendation;
pub use service::{JobSeekerService, ServiceError};
pub use sqlite_repository::SqliteRepository;
