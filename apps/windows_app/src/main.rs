use smart_job_core::{rank_jobs, JobOpportunity, UserProfile};

fn main() {
    let profile = UserProfile::new(vec!["rust", "llm", "sqlite"], vec!["seattle"], false);

    let jobs = vec![
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
    ];

    let ranked = rank_jobs(&profile, &jobs);

    println!("smart-job-seeker skeleton: ranked opportunities");
    for item in ranked {
        println!(
            "- {} at {} => score {:.2}",
            item.job.title, item.job.company, item.recommendation.score
        );
    }
}
