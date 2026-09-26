//! T040 — learning and public support. Gaps map to small verifiable
//! activities with official source links. A completed tutorial is never
//! equated to years of experience.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapActivity {
    pub gap: String,
    pub activity: String,
    pub official_source: String,
    /// Honest label: this is verifiable learning, NOT experience.
    pub outcome_claim: &'static str,
}

pub fn map_gaps(missing_requirements: &[String]) -> Vec<GapActivity> {
    missing_requirements
        .iter()
        .map(|gap| GapActivity {
            gap: gap.clone(),
            activity: format!("Guided project demonstrating {gap}"),
            official_source: "official documentation / course (linked in UI)".into(),
            outcome_claim: "verifiable learning, not professional experience",
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_map_to_honest_activities() {
        let acts = map_gaps(&["Kubernetes operations".into()]);
        assert_eq!(acts.len(), 1);
        assert!(acts[0].activity.contains("Kubernetes operations"));
        assert_eq!(
            acts[0].outcome_claim,
            "verifiable learning, not professional experience"
        );
    }
}
