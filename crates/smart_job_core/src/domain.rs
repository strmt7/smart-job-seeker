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
    pub fn new(
        skills: impl IntoIterator<Item = impl AsRef<str>>,
        preferred_locations: impl IntoIterator<Item = impl AsRef<str>>,
        remote_only: bool,
    ) -> Self {
        Self {
            skills: normalize_iter(skills),
            preferred_locations: normalize_iter(preferred_locations),
            remote_only,
            min_salary_expectation_usd: None,
        }
    }

    pub fn with_min_salary_expectation(mut self, min_salary_expectation_usd: u32) -> Self {
        self.min_salary_expectation_usd = Some(min_salary_expectation_usd);
        self
    }
}

impl JobOpportunity {
    pub fn new(
        id: &str,
        title: &str,
        company: &str,
        location: &str,
        remote: bool,
        required_skills: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            company: company.to_string(),
            location: location.to_string(),
            remote,
            required_skills: normalize_iter(required_skills),
            min_salary_usd: None,
        }
    }

    pub fn with_min_salary(mut self, min_salary_usd: u32) -> Self {
        self.min_salary_usd = Some(min_salary_usd);
        self
    }
}

fn normalize_iter(values: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| normalize(value.as_ref()))
        .collect()
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}
