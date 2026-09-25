use crate::domain::{JobOpportunity, UserProfile};

pub type RepositoryResult<T> = Result<T, RepositoryError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryError {
    Storage(String),
    Serialization(String),
}

pub trait JobRepository {
    fn replace_jobs(&mut self, jobs: Vec<JobOpportunity>) -> RepositoryResult<()>;
    fn all_jobs(&self) -> RepositoryResult<Vec<JobOpportunity>>;
}

pub trait ProfileRepository {
    fn set_profile(&mut self, profile: UserProfile) -> RepositoryResult<()>;
    fn profile(&self) -> RepositoryResult<Option<UserProfile>>;
}

#[derive(Debug, Default)]
pub struct InMemoryRepository {
    jobs: Vec<JobOpportunity>,
    profile: Option<UserProfile>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl JobRepository for InMemoryRepository {
    fn replace_jobs(&mut self, jobs: Vec<JobOpportunity>) -> RepositoryResult<()> {
        self.jobs = jobs;
        Ok(())
    }

    fn all_jobs(&self) -> RepositoryResult<Vec<JobOpportunity>> {
        Ok(self.jobs.clone())
    }
}

impl ProfileRepository for InMemoryRepository {
    fn set_profile(&mut self, profile: UserProfile) -> RepositoryResult<()> {
        self.profile = Some(profile);
        Ok(())
    }

    fn profile(&self) -> RepositoryResult<Option<UserProfile>> {
        Ok(self.profile.clone())
    }
}
