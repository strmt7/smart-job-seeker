use crate::domain::{JobOpportunity, UserProfile};

pub trait JobRepository {
    fn replace_jobs(&mut self, jobs: Vec<JobOpportunity>);
    fn all_jobs(&self) -> &[JobOpportunity];
}

pub trait ProfileRepository {
    fn set_profile(&mut self, profile: UserProfile);
    fn profile(&self) -> Option<&UserProfile>;
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
    fn replace_jobs(&mut self, jobs: Vec<JobOpportunity>) {
        self.jobs = jobs;
    }

    fn all_jobs(&self) -> &[JobOpportunity] {
        &self.jobs
    }
}

impl ProfileRepository for InMemoryRepository {
    fn set_profile(&mut self, profile: UserProfile) {
        self.profile = Some(profile);
    }

    fn profile(&self) -> Option<&UserProfile> {
        self.profile.as_ref()
    }
}
