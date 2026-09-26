//! T025 — correction propagation (differentiator D1).
//!
//! Correcting a candidate fact must identify every artifact that used it,
//! invalidate affected approvals, preserve unaffected material, and never
//! touch already-submitted packets — it creates a new revision and a
//! reviewable diff instead.

use serde::{Deserialize, Serialize};
use waypoint_domain::GrantId;
use waypoint_domain::{ApplicationState, ClaimId};

use crate::tree::SemanticDocument;

/// Where a claim is used — one artifact or one pending application payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UsageSite {
    Document {
        document_id: String,
        node_ids: Vec<String>,
    },
    Answer {
        answer_id: String,
    },
    PendingApplication {
        application_id: String,
    },
}

/// An approval grant that must be invalidated because its content changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidateApproval {
    pub grant_id: GrantId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectionImpact {
    pub claim: ClaimId,
    pub affected_documents: Vec<String>,
    pub affected_answers: Vec<String>,
    pub affected_pending_applications: Vec<String>,
    pub approvals_to_invalidate: Vec<InvalidateApproval>,
    /// Submitted artifacts are listed but NEVER modified.
    pub submitted_but_affected: Vec<String>,
}

/// The executable plan after a fact correction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectionPlan {
    pub impact: CorrectionImpact,
    pub new_profile_revision: u64,
    /// Text diffs per document: (document_id, (old_text, new_text)).
    pub diffs: Vec<(String, String, String)>,
}

/// Compute who uses the claim. Documents: searched by tree citations.
/// Applications/answers: their payload cites claims by id (simplified map
/// supplied by the caller, which owns persistence).
pub fn assess_impact(
    corrected_claim: &ClaimId,
    docs: &[SemanticDocument],
    answer_claim_map: &[(String, Vec<ClaimId>)],
    application_claim_map: &[(String, Vec<ClaimId>, ApplicationState)],
) -> CorrectionImpact {
    let mut affected_documents = vec![];
    for d in docs {
        if !d.spans_citing(corrected_claim).is_empty() || root_cites_claim(&d.root, corrected_claim)
        {
            affected_documents.push(d.id.clone());
        }
    }
    let affected_answers: Vec<String> = answer_claim_map
        .iter()
        .filter(|(_, cs)| cs.contains(corrected_claim))
        .map(|(a, _)| a.clone())
        .collect();

    let mut affected_pending = vec![];
    let mut submitted_but_affected = vec![];
    for (app_id, claims, state) in application_claim_map {
        if claims.contains(corrected_claim) {
            match state {
                ApplicationState::Prepared
                | ApplicationState::Approved
                | ApplicationState::Shortlisted => affected_pending.push(app_id.clone()),
                // Already external: immutable; recorded, never rewritten.
                ApplicationState::Submitting
                | ApplicationState::Confirmed
                | ApplicationState::Uncertain
                | ApplicationState::ExternalAttested
                | ApplicationState::Rejected
                | ApplicationState::Interview
                | ApplicationState::Offer => submitted_but_affected.push(app_id.clone()),
                ApplicationState::Discovered => {}
            }
        }
    }

    CorrectionImpact {
        claim: corrected_claim.clone(),
        affected_documents,
        affected_answers,
        affected_pending_applications: affected_pending,
        // The caller (policy layer) derives grant invalidations from pending
        // applications + bound hashes; we surface which applications matter.
        approvals_to_invalidate: vec![],
        submitted_but_affected,
    }
}

fn root_cites_claim(root: &crate::tree::DocNode, claim: &ClaimId) -> bool {
    root.claim_ids.contains(claim) || root.children.iter().any(|c| root_cites_claim(c, claim))
}

/// Produce the executable correction plan: new revision, diffs for unlocked
/// spans; already-submitted artifacts untouched.
pub fn plan_correction(
    corrected_claim: &ClaimId,
    new_text: &str,
    docs: &mut [SemanticDocument],
    answer_claim_map: &[(String, Vec<ClaimId>)],
    application_claim_map: &[(String, Vec<ClaimId>, ApplicationState)],
    current_profile_revision: u64,
) -> CorrectionPlan {
    let impact = assess_impact(
        corrected_claim,
        docs,
        answer_claim_map,
        application_claim_map,
    );
    let mut diffs = vec![];
    for d in docs.iter_mut() {
        if impact.affected_documents.contains(&d.id) {
            let old = d.to_plain_text();
            let changed = d.root.replace_in_claim_spans(corrected_claim, new_text);
            if !changed.is_empty() {
                let new = d.to_plain_text();
                diffs.push((d.id.clone(), old, new));
                d.profile_revision = current_profile_revision + 1;
            }
        }
    }
    CorrectionPlan {
        impact,
        new_profile_revision: current_profile_revision + 1,
        diffs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{DocNode, DocNodeKind};
    use waypoint_domain::{Claim, ClaimStatus, SourceKind};

    fn claim(id: &str) -> Claim {
        Claim {
            id: ClaimId::new(id).unwrap(),
            profile_revision: 1,
            text: "Worked 2019-2021".into(),
            status: ClaimStatus::UserAttested,
            evidence_ids: vec![],
            source_kind: SourceKind::CandidateAttestation,
            requires_review: false,
            supersedes: None,
        }
    }

    fn cv(claim: &Claim) -> SemanticDocument {
        let span = DocNode::text_span("Worked 2019-2021", Some(claim.id.clone()));
        SemanticDocument {
            id: "cv-1".into(),
            kind: "cv".into(),
            profile_revision: 1,
            root: DocNode {
                id: "r".into(),
                kind: DocNodeKind::Document,
                text: String::new(),
                claim_ids: vec![],
                locked: false,
                children: vec![span],
            },
        }
    }

    #[test]
    fn correction_propagates_to_docs_and_pending_but_not_submitted() {
        let c = claim("c1");
        let mut docs = vec![cv(&c)];

        let answers = vec![("ans-1".to_string(), vec![c.id.clone()])];
        let applications = vec![
            (
                "app-PENDING".to_string(),
                vec![c.id.clone()],
                ApplicationState::Prepared,
            ),
            (
                "app-SUBMITTED".to_string(),
                vec![c.id.clone()],
                ApplicationState::Confirmed,
            ),
            (
                "app-OTHER".to_string(),
                vec![ClaimId::new("c9").unwrap()],
                ApplicationState::Prepared,
            ),
        ];

        let plan = plan_correction(
            &c.id,
            "Worked 2020-2022",
            &mut docs,
            &answers,
            &applications,
            1,
        );

        assert_eq!(plan.impact.affected_documents, vec!["cv-1"]);
        assert_eq!(plan.impact.affected_answers, vec!["ans-1"]);
        assert_eq!(
            plan.impact.affected_pending_applications,
            vec!["app-PENDING"]
        );
        // submitted application is listed, not silently rewritten
        assert_eq!(plan.impact.submitted_but_affected, vec!["app-SUBMITTED"]);
        assert!(!plan
            .impact
            .affected_pending_applications
            .contains(&"app-SUBMITTED".to_string()));

        // the CV diff contains the old and new dates
        assert_eq!(plan.diffs.len(), 1);
        assert!(plan.diffs[0].1.contains("2019-2021"));
        assert!(plan.diffs[0].2.contains("2020-2022"));
        assert_eq!(plan.new_profile_revision, 2);
        assert_eq!(docs[0].profile_revision, 2);
    }

    #[test]
    fn locked_spans_are_never_rewritten_by_correction() {
        let c = claim("c1");
        let mut span = DocNode::text_span("Worked 2019-2021", Some(c.id.clone()));
        span.locked = true;
        let mut docs = vec![SemanticDocument {
            id: "cv-2".into(),
            kind: "cv".into(),
            profile_revision: 1,
            root: DocNode {
                id: "r".into(),
                kind: DocNodeKind::Document,
                text: String::new(),
                claim_ids: vec![],
                locked: false,
                children: vec![span],
            },
        }];
        let plan = plan_correction(&c.id, "Worked 2020-2022", &mut docs, &[], &[], 1);
        assert!(plan.diffs.is_empty(), "locked wording must not change");
        // but the impact still reports the document as affected
        assert_eq!(plan.impact.affected_documents, vec!["cv-2"]);
    }
}
