//! Discovery foundation: hard constraints, identity/dedup, freshness,
//! lexical matching (T013, T016, T017, T018 — gate G2).
//!
//! Hard constraints are evaluated *before* ranking; unknown is neither pass
//! nor fail (MASTER_PLAN §6 D3). No silent relaxation.

use waypoint_domain::{Amount, JobIdentityId};

/// Hard vs soft constraint. Unknown facts never silently pass a hard check.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintValue {
    Required,
    Excluded,
    Range { min: Option<i64>, max: Option<i64> },
    Unknown,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CampaignConstraints {
    /// Required languages for the job itself (not ad language).
    pub language_required: Vec<String>,
    pub salary: Option<(i64, i64)>, // (min,max) in minor units, same currency as compared amount
    pub allow_unknown_salary: bool,
    pub sponsorship_required: Option<bool>,
    pub remote_only: bool,
    pub locations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintOutcome {
    Pass,
    Fail(String),
    Unknown(String),
}

pub fn check_constraints(
    c: &CampaignConstraints,
    job_languages: &[String],
    job_salary: Option<&Amount>,
    sponsorship_stated: Option<bool>,
    remote: bool,
    location: &str,
) -> Vec<(String, ConstraintOutcome)> {
    let mut out = Vec::new();

    if !c.language_required.is_empty() {
        let missing: Vec<_> = c
            .language_required
            .iter()
            .filter(|l| !job_languages.iter().any(|j| j.eq_ignore_ascii_case(l)))
            .collect();
        if missing.is_empty() {
            out.push(("language".into(), ConstraintOutcome::Pass));
        } else if job_languages.is_empty() {
            out.push((
                "language".into(),
                ConstraintOutcome::Unknown("job does not state a language".into()),
            ));
        } else {
            out.push((
                "language".into(),
                ConstraintOutcome::Fail(format!(
                    "requires {missing:?}; ad lists {job_languages:?}"
                )),
            ));
        }
    }

    if let Some((min, _)) = c.salary {
        match job_salary {
            None => out.push((
                "salary".into(),
                if c.allow_unknown_salary {
                    ConstraintOutcome::Unknown("salary not stated".into())
                } else {
                    ConstraintOutcome::Fail("salary not stated".into())
                },
            )),
            Some(a) => {
                // Never compare across basis/period without normalization.
                if a.currency != "CHF" && a.currency != "USD" && a.currency != "EUR" {
                    out.push((
                        "salary".into(),
                        ConstraintOutcome::Unknown(format!(
                            "non-comparable currency {}",
                            a.currency
                        )),
                    ));
                } else if let Some(max_minor) = a.max_minor_units.or(a.min_minor_units) {
                    if max_minor >= min {
                        out.push(("salary".into(), ConstraintOutcome::Pass));
                    } else {
                        out.push((
                            "salary".into(),
                            ConstraintOutcome::Fail("stated max below required minimum".into()),
                        ));
                    }
                } else {
                    out.push((
                        "salary".into(),
                        ConstraintOutcome::Unknown("employer stated no numeric range".into()),
                    ));
                }
            }
        }
    }

    if let Some(req) = c.sponsorship_required {
        match sponsorship_stated {
            None => out.push((
                "sponsorship".into(),
                ConstraintOutcome::Unknown(
                    "sponsorship not stated; history is not a promise".into(),
                ),
            )),
            Some(s) if s == req => out.push(("sponsorship".into(), ConstraintOutcome::Pass)),
            Some(_) => out.push((
                "sponsorship".into(),
                ConstraintOutcome::Fail("sponsorship stated as not offered".into()),
            )),
        }
    }

    if c.remote_only && !remote {
        out.push((
            "remote".into(),
            ConstraintOutcome::Fail("not remote".into()),
        ));
    } else if !c.locations.is_empty()
        && !c.locations.iter().any(|l| location.eq_ignore_ascii_case(l))
        && !remote
    {
        out.push((
            "location".into(),
            ConstraintOutcome::Fail(format!("location '{location}' not in allowed set")),
        ));
    }

    out
}

/* ------------------------------ identity/dedup ------------------------------ */

/// Identity key: employer namespace + requisition id when present.
/// A different URL alone must not imply a new opportunity.
pub fn identity_key(
    employer_namespace: &str,
    requisition_id: Option<&str>,
    canonical_url: &str,
) -> String {
    match requisition_id {
        Some(r) if !r.is_empty() => format!("{employer_namespace}::req::{r}"),
        _ => format!("{employer_namespace}::url::{canonical_url}"),
    }
}

/// Repost detection: same identity + unchanged content hash is the same
/// observation; changed location/contract/requirements create a new one.
#[derive(Debug, PartialEq, Eq)]
pub enum DedupDecision {
    SameObservation,
    NewObservation { reason: String },
    DifferentJob,
}

pub fn dedup_decision(
    existing: &JobIdentityId,
    incoming: &JobIdentityId,
    existing_hash: Option<&str>,
    incoming_hash: &str,
) -> DedupDecision {
    if existing == incoming {
        if existing_hash == Some(incoming_hash) {
            DedupDecision::SameObservation
        } else {
            DedupDecision::NewObservation {
                reason: "identity matches but content hash changed".into(),
            }
        }
    } else {
        DedupDecision::DifferentJob
    }
}

/* -------------------------------- freshness --------------------------------- */

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    Fresh { age_seconds: i64 },
    Stale { age_seconds: i64 },
    Unknown,
}

pub fn freshness(observed_at_unix: i64, now_unix: i64, stale_after_seconds: i64) -> Freshness {
    if observed_at_unix <= 0 {
        return Freshness::Unknown; // never observed / unrecorded timestamp
    }
    let age = now_unix - observed_at_unix;
    if age <= stale_after_seconds {
        Freshness::Fresh { age_seconds: age }
    } else {
        Freshness::Stale { age_seconds: age }
    }
}

/* ------------------------------ lexical matching ----------------------------- */

/// Interpretable overlap score in [0,1]: fraction of required skills the
/// profile evidence covers. No ML here; this is the honest baseline (§6).
pub fn lexical_overlap(required: &[String], profile_skills: &[String]) -> (f32, Vec<String>) {
    if required.is_empty() {
        return (1.0, vec![]);
    }
    let missing: Vec<String> = required
        .iter()
        .filter(|r| {
            !profile_skills
                .iter()
                .any(|p| p.to_lowercase().contains(&r.to_lowercase()))
        })
        .cloned()
        .collect();
    let score = (required.len() - missing.len()) as f32 / required.len() as f32;
    (score, missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::{AmountBasis, CompensationCategory, PayPeriod};

    fn job_salary_chf(min: i64, max: i64) -> Amount {
        Amount {
            currency: "CHF".into(),
            basis: AmountBasis::Gross,
            period: PayPeriod::Annual,
            category: CompensationCategory::Base,
            min_minor_units: Some(min),
            max_minor_units: Some(max),
            source: "employer_published".into(),
        }
    }

    #[test]
    fn unknown_salary_is_failable_or_unknown_but_never_silent_pass() {
        // Default policy: unknown salary FAILs a stated minimum (the user did
        // not opt into including unknown-salary roles). Unknown is never a
        // silent pass either way.
        let c = CampaignConstraints {
            salary: Some((9_000_000, 15_000_000)),
            ..Default::default()
        };
        let r = check_constraints(&c, &[], None, None, false, "Zurich");
        assert!(r
            .iter()
            .any(|(_, o)| matches!(o, ConstraintOutcome::Fail(_))));
        assert!(!r.iter().any(|(_, o)| matches!(o, ConstraintOutcome::Pass)));

        // Opt-in: unknown salary becomes Unknown, still never a silent Pass.
        let c2 = CampaignConstraints {
            salary: Some((9_000_000, 15_000_000)),
            allow_unknown_salary: true,
            ..Default::default()
        };
        let r = check_constraints(&c2, &[], None, None, false, "Zurich");
        assert!(r
            .iter()
            .any(|(_, o)| matches!(o, ConstraintOutcome::Unknown(_))));
        assert!(!r.iter().any(|(_, o)| matches!(o, ConstraintOutcome::Pass)));
    }

    #[test]
    fn hard_language_never_silently_relaxed() {
        let c = CampaignConstraints {
            language_required: vec!["de".into()],
            ..Default::default()
        };
        let r = check_constraints(&c, &["en".into()], None, None, false, "");
        assert!(r
            .iter()
            .any(|(_, o)| matches!(o, ConstraintOutcome::Fail(_))));
    }

    #[test]
    fn repost_same_id_same_hash_is_same_observation() {
        let id = JobIdentityId::new("greenhouse::req::123").unwrap();
        assert_eq!(
            dedup_decision(&id, &id, Some("h"), "h"),
            DedupDecision::SameObservation
        );
        assert!(matches!(
            dedup_decision(&id, &id, Some("h"), "h2"),
            DedupDecision::NewObservation { .. }
        ));
        let other = JobIdentityId::new("greenhouse::req::124").unwrap();
        assert_eq!(
            dedup_decision(&id, &other, Some("h"), "h"),
            DedupDecision::DifferentJob
        );
    }

    #[test]
    fn freshness_respects_stale_after() {
        assert!(matches!(freshness(100, 200, 300), Freshness::Fresh { .. }));
        // Zero/negative timestamps are unrecorded observations, never Fresh.
        assert!(matches!(freshness(0, 200, 300), Freshness::Unknown));
        assert!(matches!(freshness(-1, 200, 300), Freshness::Unknown));
        // A real observation past its stale window is Stale.
        assert!(matches!(
            freshness(100, 10_000, 300),
            Freshness::Stale { .. }
        ));
    }

    #[test]
    fn lexical_overlap_reports_missing() {
        let (score, missing) = lexical_overlap(
            &["rust".into(), "sqlite".into(), "llm".into()],
            &["rust".into(), "sqlite".into()],
        );
        assert_eq!(score, 2.0 / 3.0);
        assert_eq!(missing, vec!["llm".to_string()]);
    }

    #[test]
    fn salary_threshold_compares_like_units_only() {
        let c = CampaignConstraints {
            salary: Some((9_000_000, 0)),
            allow_unknown_salary: false,
            ..Default::default()
        };
        let ok = job_salary_chf(9_500_000, 11_000_000);
        let r = check_constraints(&c, &[], Some(&ok), None, true, "Remote");
        assert!(r.iter().any(|(_, o)| matches!(o, ConstraintOutcome::Pass)));
        let low = job_salary_chf(7_000_000, 8_000_000);
        let r = check_constraints(&c, &[], Some(&low), None, true, "Remote");
        assert!(r
            .iter()
            .any(|(_, o)| matches!(o, ConstraintOutcome::Fail(_))));
    }
}
