use smart_job_core::{InMemoryRepository, JobOpportunity, JobSeekerService, UserProfile};

fn main() {
    let mut service = JobSeekerService::new(InMemoryRepository::new());

    service.set_profile(UserProfile::new(
        vec!["rust", "llm", "sqlite"],
        vec!["seattle"],
        false,
    ));

    service.replace_jobs(vec![
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
    ]);

    println!("smart-job-seeker skeleton: ranked opportunities");
    match service.ranked_shortlist(10) {
        Ok(ranked) => {
            for item in ranked {
                println!(
                    "- {} at {} => score {:.2}",
                    item.job.title, item.job.company, item.recommendation.score
                );
            }
        }
        Err(err) => {
            eprintln!("Failed to rank opportunities: {err:?}");
        }
    }
}
