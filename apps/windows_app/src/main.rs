use smart_job_core::{JobOpportunity, JobSeekerService, SqliteRepository, UserProfile};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repository = SqliteRepository::in_memory()
        .map_err(|err| format!("failed to initialize SQLite repository: {err:?}"))?;
    let mut service = JobSeekerService::new(repository);

    service
        .set_profile(UserProfile::new(
            vec!["rust", "llm", "sqlite"],
            vec!["seattle"],
            false,
        ))
        .map_err(|err| format!("failed to store profile: {err:?}"))?;

    service
        .replace_jobs(vec![
            JobOpportunity::new(
                "role-1",
                "Rust Platform Engineer",
                "Acme Systems",
                "Seattle",
                false,
                vec!["rust", "sqlite", "llm"],
            ),
            JobOpportunity::new(
                "role-2",
                "AI Product Engineer",
                "Nova Labs",
                "Remote",
                true,
                vec!["python", "transformers"],
            ),
        ])
        .map_err(|err| format!("failed to store jobs: {err:?}"))?;

    println!("smart-job-seeker skeleton: ranked opportunities");
    let ranked = service
        .ranked_shortlist(10)
        .map_err(|err| format!("failed to rank opportunities: {err:?}"))?;

    for item in ranked {
        println!(
            "- {} at {} => score {:.2}",
            item.job.title, item.job.company, item.recommendation.score
        );
    }

    Ok(())
}
