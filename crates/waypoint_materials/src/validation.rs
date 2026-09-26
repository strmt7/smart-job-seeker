//! T022 — evidence validation.
//!
//! Truth checks distinguish paraphrase from new claims. A sentence copied
//! from a job ad is not evidence the candidate did it. Numeric achievements,
//! certifications, tools and ownership assertions need candidate evidence or
//! a fresh attestation.

use waypoint_domain::{Claim, ClaimStatus, SourceKind};

use crate::tree::{DocNode, DocNodeKind, SemanticDocument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOutcome {
    Supported {
        node_id: String,
        claim: String,
    },
    /// The span asserts something no cited claim supports -> blocked until
    /// the user adds evidence or a new explicit attestation.
    Unsupported {
        node_id: String,
        text: String,
        reason: String,
    },
    /// Locked wording is left alone but reported.
    LockedUnsupported {
        node_id: String,
        text: String,
    },
}

/// Heuristic "new claim" detectors over a text span. Deliberately
/// conservative: prefer false "unsupported" flags for human review over
/// letting invented facts through (MASTER_PLAN §8).
fn looks_like_new_claim(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    // numeric achievements: "increased X by 40%", "led a team of 12"
    if lower.contains(" by ") && text.chars().any(|c| c.is_ascii_digit()) {
        return Some("numeric achievement");
    }
    if lower.contains("certified") || lower.contains("certification") {
        return Some("certification");
    }
    if lower.starts_with("owned ") || lower.contains("sole owner") || lower.contains("i built") {
        return Some("ownership assertion");
    }
    if lower.contains("years of experience") && text.chars().any(|c| c.is_ascii_digit()) {
        return Some("quantified experience");
    }
    None
}

pub fn validate_claim_support(doc: &SemanticDocument, claims: &[Claim]) -> Vec<ValidationOutcome> {
    let mut out = vec![];
    fn walk(n: &DocNode, claims: &[Claim], out: &mut Vec<ValidationOutcome>) {
        if n.kind == DocNodeKind::TextSpan && !n.text.trim().is_empty() {
            if n.claim_ids.is_empty() {
                if let Some(reason) = looks_like_new_claim(&n.text) {
                    if n.locked {
                        out.push(ValidationOutcome::LockedUnsupported {
                            node_id: n.id.clone(),
                            text: n.text.clone(),
                        });
                    } else {
                        out.push(ValidationOutcome::Unsupported {
                            node_id: n.id.clone(),
                            text: n.text.clone(),
                            reason: reason.into(),
                        });
                    }
                }
            } else {
                for c in claims {
                    if n.claim_ids.iter().any(|cid| cid == &c.id) {
                        let supported = match c.status {
                            ClaimStatus::UserAttested | ClaimStatus::ExternallySupported => true,
                            ClaimStatus::Inferred | ClaimStatus::Unknown => false,
                            ClaimStatus::Disputed => false,
                        };
                        if !supported {
                            out.push(ValidationOutcome::Unsupported {
                                node_id: n.id.clone(),
                                text: n.text.clone(),
                                reason: format!("cited claim {} is {:?}", c.id.as_str(), c.status),
                            });
                        } else if c.source_kind == SourceKind::Inference {
                            out.push(ValidationOutcome::Unsupported {
                                node_id: n.id.clone(),
                                text: n.text.clone(),
                                reason: "cited claim only backed by inference".into(),
                            });
                        }
                    }
                }
            }
        }
        for ch in &n.children {
            walk(ch, claims, out);
        }
    }
    walk(&doc.root, claims, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::{ClaimId, EvidenceId};

    fn claim(id: &str, status: ClaimStatus, kind: SourceKind) -> Claim {
        Claim {
            id: ClaimId::new(id).unwrap(),
            profile_revision: 1,
            text: "Led deployment of a thing".into(),
            status,
            evidence_ids: if kind == SourceKind::ExternalConfirmation {
                vec![EvidenceId::new("e1").unwrap()]
            } else {
                vec![]
            },
            source_kind: kind,
            requires_review: false,
            supersedes: None,
        }
    }

    fn doc_with_span(text: &str, claim_id: Option<ClaimId>, locked: bool) -> SemanticDocument {
        let span = DocNode::text_span(text, claim_id);
        let mut span = span;
        span.locked = locked;
        SemanticDocument {
            id: "d".into(),
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
    fn numeric_achievement_without_claim_is_flagged() {
        let doc = doc_with_span("Increased revenue by 40%", None, false);
        let out = validate_claim_support(&doc, &[]);
        assert!(matches!(
            out[0],
            ValidationOutcome::Unsupported { ref reason, .. } if reason == "numeric achievement"
        ));
    }

    #[test]
    fn cited_attested_claim_is_supported_and_silent() {
        let c = claim(
            "c1",
            ClaimStatus::UserAttested,
            SourceKind::CandidateAttestation,
        );
        let doc = doc_with_span("Led deployment of a thing", Some(c.id.clone()), false);
        assert!(validate_claim_support(&doc, &[c]).is_empty());
    }

    #[test]
    fn cited_inferred_claim_is_not_supported() {
        let c = claim("c1", ClaimStatus::Inferred, SourceKind::Inference);
        let doc = doc_with_span("Led deployment of a thing", Some(c.id.clone()), false);
        let out = validate_claim_support(&doc, &[c]);
        assert!(matches!(out[0], ValidationOutcome::Unsupported { .. }));
    }

    #[test]
    fn locked_unsupported_wording_is_reported_not_mutated() {
        let doc = doc_with_span("Certified in everything", None, true);
        let out = validate_claim_support(&doc, &[]);
        assert!(matches!(
            out[0],
            ValidationOutcome::LockedUnsupported { .. }
        ));
    }
}
