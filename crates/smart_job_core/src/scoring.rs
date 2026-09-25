use crate::domain::{JobOpportunity, UserProfile};

#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    pub score: f32,
    pub matched_skills: usize,
    pub missing_skills: Vec<String>,
    pub reasons: Vec<String>,
}

pub fn score_job(profile: &UserProfile, job: &JobOpportunity) -> Recommendation {
    let (skill_score, matched_skills, missing_skills) = skill_score(profile, job);
    let location_score = location_score(profile, job);
    let salary_score = salary_score(profile, job);

    let mut reasons = Vec::with_capacity(3);
    reasons.push(format!("Matched {} skill(s)", matched_skills));

    if !missing_skills.is_empty() {
        reasons.push(format!(
            "Missing {} skill(s): {}",
            missing_skills.len(),
            missing_skills.join(", ")
        ));
    }

    if job.remote {
        reasons.push("Remote role".to_string());
    }

    let score = ((skill_score * 0.7) + (location_score * 0.2) + (salary_score * 0.1)).clamp(0.0, 1.0);

    Recommendation {
        score,
        matched_skills,
        missing_skills,
        reasons,
    }
}

fn skill_score(profile: &UserProfile, job: &JobOpportunity) -> (f32, usize, Vec<String>) {
    if job.required_skills.is_empty() {
        return (0.5, 0, Vec::new());
    }

    let mut matched = 0usize;
    let mut missing = Vec::new();

    for skill in &job.required_skills {
        if profile.skills.iter().any(|s| s == skill) {
            matched += 1;
        } else {
            missing.push(skill.clone());
        }
    }

    (matched as f32 / job.required_skills.len() as f32, matched, missing)
}

fn location_score(profile: &UserProfile, job: &JobOpportunity) -> f32 {
    if profile.remote_only {
        return if job.remote { 1.0 } else { 0.0 };
    }

    if job.remote {
        return 1.0;
    }

    let location = job.location.trim().to_lowercase();
    if profile
        .preferred_locations
        .iter()
        .any(|preferred| preferred == &location)
    {
        1.0
    } else {
        0.4
    }
}

fn salary_score(profile: &UserProfile, job: &JobOpportunity) -> f32 {
    match (profile.min_salary_expectation_usd, job.min_salary_usd) {
        (Some(expected), Some(offered)) if offered >= expected => 1.0,
        (Some(_), Some(_)) => 0.2,
        _ => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{JobOpportunity, UserProfile};

    #[test]
    fn scores_higher_when_more_skills_match() {
        let profile = UserProfile::new(vec!["rust", "sql", "llm"], vec!["seattle"], false);

        let strong = JobOpportunity::new(
            "1",
            "Senior Rust Engineer",
            "A",
            "Seattle",
            false,
            vec!["rust", "sql"],
        );
        let weak = JobOpportunity::new(
            "2",
            "Frontend Engineer",
            "B",
            "Seattle",
            false,
            vec!["react", "css"],
        );

        let strong_score = score_job(&profile, &strong).score;
        let weak_score = score_job(&profile, &weak).score;

        assert!(strong_score > weak_score);
    }

    #[test]
    fn remote_only_profile_penalizes_non_remote_jobs() {
        let profile = UserProfile::new(vec!["rust"], vec!["nyc"], true);

        let remote = JobOpportunity::new("1", "Backend", "A", "Remote", true, vec!["rust"]);
        let on_site = JobOpportunity::new("2", "Backend", "A", "NYC", false, vec!["rust"]);

        assert!(score_job(&profile, &remote).score > score_job(&profile, &on_site).score);
    }
}
