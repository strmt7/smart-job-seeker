//! Bounded internet research runs (T020, gate G2).
//!
//! A research run has an explicit plan: unresolved questions, source hierarchy,
//! query/fetch budget and stopping criteria (MASTER_PLAN §7). Retrieval and
//! inference are distinct stages; candidate PII never enters a query unless
//! the query's purpose explicitly declares that category.

use serde::{Deserialize, Serialize};
use waypoint_inference::{Inference, InferenceError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchBudget {
    pub max_queries: u8,
    pub max_fetches: u8,
    pub max_verification_passes: u8,
}

impl Default for ResearchBudget {
    fn default() -> Self {
        // MASTER_PLAN §7 defaults
        Self {
            max_queries: 4,
            max_fetches: 8,
            max_verification_passes: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchPlan {
    pub employer: String,
    pub unresolved_questions: Vec<String>,
    /// e.g. ["employer_ats","employer_official_site","official_authorities"]
    pub source_hierarchy: Vec<String>,
    pub budget: ResearchBudget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DataCategory {
    PublicEmployerFact,
    CandidatePii,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchQuery {
    pub text: String,
    /// Declared data categories this query contains. The network broker
    /// refuses a query containing CandidatePii unless the plan opts in.
    pub declares: Vec<DataCategory>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResearchError {
    BudgetExceeded(&'static str),
    PiiNotDisclosed,
    Inference(InferenceError),
}

impl From<InferenceError> for ResearchError {
    fn from(e: InferenceError) -> Self {
        ResearchError::Inference(e)
    }
}

impl std::fmt::Display for ResearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResearchError::BudgetExceeded(w) => write!(f, "budget exceeded: {w}"),
            ResearchError::PiiNotDisclosed => {
                write!(
                    f,
                    "query contains candidate PII without explicit disclosure"
                )
            }
            ResearchError::Inference(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ResearchError {}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ResearchRun {
    pub plan: Option<ResearchPlan>,
    pub queries_issued: Vec<ResearchQuery>,
    pub fetches_used: u8,
    pub verification_passes: u8,
}

pub struct Researcher<I: Inference> {
    inference: I,
    run: ResearchRun,
}

impl<I: Inference> Researcher<I> {
    pub fn new(inference: I) -> Self {
        Self {
            inference,
            run: ResearchRun::default(),
        }
    }

    pub fn begin(&mut self, plan: ResearchPlan) {
        self.run = ResearchRun {
            plan: Some(plan),
            ..Default::default()
        };
    }

    /// Enforces budget + PII policy before the network broker would see it.
    pub fn issue_query(&mut self, q: ResearchQuery) -> Result<(), ResearchError> {
        let budget = self.run.plan.as_ref().map(|p| p.budget).unwrap_or_default();
        if self.run.queries_issued.len() as u8 >= budget.max_queries {
            return Err(ResearchError::BudgetExceeded("queries"));
        }
        if q.declares.contains(&DataCategory::CandidatePii) {
            // An employer-fact query must not smuggle candidate PII.
            return Err(ResearchError::PiiNotDisclosed);
        }
        self.run.queries_issued.push(q);
        Ok(())
    }

    pub fn record_fetch(&mut self) -> Result<(), ResearchError> {
        let budget = self.run.plan.as_ref().map(|p| p.budget).unwrap_or_default();
        if self.run.fetches_used >= budget.max_fetches {
            return Err(ResearchError::BudgetExceeded("fetches"));
        }
        self.run.fetches_used += 1;
        Ok(())
    }

    /// A verification pass is a bounded second look, never unlimited
    /// "thinking" (§7: stop after two repair attempts).
    pub fn verify<F: FnOnce(&mut I) -> Result<bool, InferenceError>>(
        &mut self,
        check: F,
    ) -> Result<bool, ResearchError> {
        let budget = self.run.plan.as_ref().map(|p| p.budget).unwrap_or_default();
        if self.run.verification_passes >= budget.max_verification_passes {
            return Err(ResearchError::BudgetExceeded("verification passes"));
        }
        self.run.verification_passes += 1;
        Ok(check(&mut self.inference)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_inference::StubInference;

    fn plan() -> ResearchPlan {
        ResearchPlan {
            employer: "Acme".into(),
            unresolved_questions: vec!["does Acme sponsor non-EU permits?".into()],
            source_hierarchy: vec!["employer_official_site".into()],
            budget: ResearchBudget::default(),
        }
    }

    fn query(text: &str) -> ResearchQuery {
        ResearchQuery {
            text: text.into(),
            declares: vec![DataCategory::PublicEmployerFact],
        }
    }

    #[test]
    fn enforces_query_budget() {
        let mut r = Researcher::new(StubInference::qualified());
        r.begin(plan());
        for _ in 0..4 {
            r.issue_query(query("q")).unwrap();
        }
        assert_eq!(
            r.issue_query(query("q5")),
            Err(ResearchError::BudgetExceeded("queries"))
        );
    }

    #[test]
    fn refuses_undeclared_pii_in_query() {
        let mut r = Researcher::new(StubInference::qualified());
        r.begin(plan());
        let bad = ResearchQuery {
            text: "does Acme want <candidate full name and history>".into(),
            declares: vec![DataCategory::CandidatePii],
        };
        assert_eq!(r.issue_query(bad), Err(ResearchError::PiiNotDisclosed));
    }

    #[test]
    fn verification_stops_after_two_passes() {
        let mut r = Researcher::new(StubInference::qualified());
        r.begin(plan());
        let ok = |_: &mut StubInference| Ok(true);
        assert!(r.verify(ok).unwrap());
        assert!(r.verify(ok).unwrap());
        assert_eq!(
            r.verify(ok),
            Err(ResearchError::BudgetExceeded("verification passes"))
        );
    }
}
