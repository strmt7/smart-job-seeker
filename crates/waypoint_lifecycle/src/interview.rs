//! T039 — interview practice. Built from the role's actual requirements,
//! candidate evidence and a current company brief. Practice and permitted
//! accommodations only — no covert real-time assistance.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterviewQuestion {
    pub prompt: String,
    pub kind: &'static str, // behavioral | technical | portfolio | salary
    /// Which requirement this probes.
    pub probes_requirement: String,
}

pub fn build_practice_set(
    role_requirements: &[String],
    evidence_topics: &[String],
    company_brief_points: &[String],
) -> Vec<InterviewQuestion> {
    let mut out = vec![];
    for req in role_requirements {
        out.push(InterviewQuestion {
            prompt: format!("Describe an experience where you applied: {req}. What was your specific contribution?"),
            kind: "technical",
            probes_requirement: req.clone(),
        });
    }
    for topic in evidence_topics {
        out.push(InterviewQuestion {
            prompt: format!(
                "Walk me through your work on {topic} — what did you own, what was the team's?"
            ),
            kind: "portfolio",
            probes_requirement: topic.clone(),
        });
    }
    if !company_brief_points.is_empty() {
        out.push(InterviewQuestion {
            prompt: format!(
                "Why this company? Reference what you know: {}",
                company_brief_points.join("; ")
            ),
            kind: "behavioral",
            probes_requirement: "company_fit".into(),
        });
    }
    out.push(InterviewQuestion {
        prompt: "What are your compensation expectations, and how did you arrive at them?".into(),
        kind: "salary",
        probes_requirement: "compensation".into(),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn practice_set_is_built_from_actual_requirements_not_generic() {
        let qs = build_practice_set(
            &["Rust async runtime tuning".into()],
            &["thin-film deposition tooling".into()],
            &["sensors for industrial ovens".into()],
        );
        assert!(qs
            .iter()
            .any(|q| q.prompt.contains("Rust async runtime tuning")));
        assert!(qs
            .iter()
            .any(|q| q.prompt.contains("thin-film deposition tooling")));
        assert!(qs.iter().any(|q| q.kind == "salary"));
        // every technical question is traceable to a requirement
        assert!(qs
            .iter()
            .filter(|q| q.kind == "technical")
            .all(|q| !q.probes_requirement.is_empty()));
    }
}
