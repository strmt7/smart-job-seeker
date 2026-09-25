use crate::domain::{JobOpportunity, UserProfile};
use crate::scoring::{score_job, Recommendation};

#[derive(Debug, Clone, PartialEq)]
pub struct RankedJob {
    pub job: JobOpportunity,
    pub recommendation: Recommendation,
}

pub fn rank_jobs(profile: &UserProfile, jobs: &[JobOpportunity]) -> Vec<RankedJob> {
    let mut ranked: Vec<RankedJob> = jobs
        .iter()
        .cloned()
        .map(|job| {
            let recommendation = score_job(profile, &job);
            RankedJob {
                job,
                recommendation,
            }
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.recommendation
            .score
            .total_cmp(&a.recommendation.score)
            .then_with(|| a.job.id.cmp(&b.job.id))
    });

    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{JobOpportunity, UserProfile};

    #[test]
    fn ranks_jobs_descending_by_score() {
        let profile = UserProfile::new(vec!["rust", "sql"], vec!["seattle"], false);

        let jobs = vec![
            JobOpportunity::new("2", "Data Engineer", "B", "Austin", false, vec!["python"]),
            JobOpportunity::new(
                "1",
                "Rust Engineer",
                "A",
                "Seattle",
                false,
                vec!["rust", "sql"],
            ),
        ];

        let ranked = rank_jobs(&profile, &jobs);
        assert_eq!(ranked[0].job.id, "1");
        assert!(ranked[0].recommendation.score >= ranked[1].recommendation.score);
    }
}
