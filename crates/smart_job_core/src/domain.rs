#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobOpportunity {
    pub id: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub remote: bool,
    pub required_skills: Vec<String>,
    pub min_salary_usd: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserProfile {
    pub skills: Vec<String>,
    pub preferred_locations: Vec<String>,
    pub remote_only: bool,
    pub min_salary_expectation_usd: Option<u32>,
}

impl UserProfile {
    pub fn new(skills: Vec<&str>, preferred_locations: Vec<&str>, remote_only: bool) -> Self {
        Self {
            skills: skills.into_iter().map(normalize).collect(),
            preferred_locations: preferred_locations.into_iter().map(normalize).collect(),
            remote_only,
            min_salary_expectation_usd: None,
        }
    }
}

impl JobOpportunity {
    pub fn new(
        id: &str,
        title: &str,
        company: &str,
        location: &str,
        remote: bool,
        required_skills: Vec<&str>,
    ) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            company: company.to_string(),
            location: location.to_string(),
            remote,
            required_skills: required_skills.into_iter().map(normalize).collect(),
            min_salary_usd: None,
        }
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}
