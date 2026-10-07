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
                // Positive support is reported once per span, and every
                // problem is reported per cited claim. A citation that no
                // longer resolves is itself a problem: an unknown fact must
                // never read as "supported".
                let mut any_supported = false;
                for cid in &n.claim_ids {
                    match claims.iter().find(|c| &c.id == cid) {
                        None => out.push(ValidationOutcome::Unsupported {
                            node_id: n.id.clone(),
                            text: n.text.clone(),
                            reason: format!("cites claim {} which no longer exists", cid.as_str()),
                        }),
                        Some(c) => {
                            let attested = matches!(
                                c.status,
                                ClaimStatus::UserAttested | ClaimStatus::ExternallySupported
                            );
                            if !attested {
                                out.push(ValidationOutcome::Unsupported {
                                    node_id: n.id.clone(),
                                    text: n.text.clone(),
                                    reason: format!(
                                        "cited claim {} is {:?}",
                                        c.id.as_str(),
                                        c.status
                                    ),
                                });
                            } else if c.source_kind == SourceKind::Inference {
                                out.push(ValidationOutcome::Unsupported {
                                    node_id: n.id.clone(),
                                    text: n.text.clone(),
                                    reason: "cited claim only backed by inference".into(),
                                });
                            } else {
                                any_supported = true;
                            }
                        }
                    }
                }
                if any_supported {
                    out.push(ValidationOutcome::Supported {
                        node_id: n.id.clone(),
                        claim: n
                            .claim_ids
                            .iter()
                            .map(|c| c.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                    });
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
    fn cited_attested_claim_is_reported_as_supported() {
        let c = claim(
            "c1",
            ClaimStatus::UserAttested,
            SourceKind::CandidateAttestation,
        );
        let doc = doc_with_span("Led deployment of a thing", Some(c.id.clone()), false);
        let out = validate_claim_support(&doc, &[c]);
        assert_eq!(out.len(), 1, "support is reported positively: {out:?}");
        assert!(matches!(out[0], ValidationOutcome::Supported { .. }));
    }

    #[test]
    fn a_citation_that_no_longer_resolves_is_unsupported_not_silent() {
        let stale = ClaimId::new("gone").unwrap();
        let doc = doc_with_span("Led deployment of a thing", Some(stale), false);
        let out = validate_claim_support(&doc, &[]);
        assert_eq!(
            out.len(),
            1,
            "a dangling citation must be reported: {out:?}"
        );
        match &out[0] {
            ValidationOutcome::Unsupported { reason, .. } => {
                assert!(reason.contains("no longer exists"), "{reason}")
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
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
