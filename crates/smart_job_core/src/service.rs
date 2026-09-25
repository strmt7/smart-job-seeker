use crate::domain::{JobOpportunity, UserProfile};
use crate::pipeline::{rank_jobs, RankedJob};
use crate::repository::{JobRepository, ProfileRepository};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    MissingProfile,
}

pub struct JobSeekerService<R> {
    repo: R,
}

impl<R> JobSeekerService<R>
where
    R: JobRepository + ProfileRepository,
{
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub fn replace_jobs(&mut self, jobs: Vec<JobOpportunity>) {
        self.repo.replace_jobs(jobs);
    }

    pub fn set_profile(&mut self, profile: UserProfile) {
        self.repo.set_profile(profile);
    }

    pub fn ranked_shortlist(&self, limit: usize) -> Result<Vec<RankedJob>, ServiceError> {
        let profile = self.repo.profile().ok_or(ServiceError::MissingProfile)?;
        let mut ranked = rank_jobs(profile, self.repo.all_jobs());
        ranked.truncate(limit);
        Ok(ranked)
    }

    pub fn into_inner(self) -> R {
        self.repo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{JobOpportunity, UserProfile};
    use crate::repository::InMemoryRepository;

    #[test]
    fn returns_error_without_profile() {
        let mut service = JobSeekerService::new(InMemoryRepository::new());
        service.replace_jobs(vec![JobOpportunity::new(
            "1",
            "Rust Engineer",
            "A",
            "Seattle",
            false,
            vec!["rust"],
        )]);

        let result = service.ranked_shortlist(5);
        assert_eq!(result, Err(ServiceError::MissingProfile));
    }

    #[test]
    fn returns_ranked_shortlist_with_limit() {
        let mut service = JobSeekerService::new(InMemoryRepository::new());
        service.set_profile(UserProfile::new(vec!["rust", "sql"], vec!["seattle"], false));
        service.replace_jobs(vec![
            JobOpportunity::new("a", "Rust Platform", "A", "Seattle", false, vec!["rust", "sql"]),
            JobOpportunity::new("b", "Frontend", "B", "Remote", true, vec!["react"]),
            JobOpportunity::new("c", "Ops", "C", "Austin", false, vec!["terraform"]),
        ]);

        let ranked = service.ranked_shortlist(2).expect("shortlist");

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].job.id, "a");
        assert!(ranked[0].recommendation.score >= ranked[1].recommendation.score);
    }
}
