//! T042 — outcome analytics with honest denominators and censoring.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutcomeCounts {
    pub opportunities_seen: u64,
    pub shortlisted: u64,
    pub prepared: u64,
    pub submitted_confirmed: u64,
    pub externally_attested: u64,
    pub responses: u64,
    pub interviews: u64,
    pub offers: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimit {
    /// Denominator is zero: no rate may be shown as 0% or 100%.
    EmptyDenominator,
}

pub fn rate(numerator: u64, denominator: u64) -> Result<f32, RateLimit> {
    if denominator == 0 {
        return Err(RateLimit::EmptyDenominator);
    }
    Ok(numerator as f32 / denominator as f32)
}

/// Censoring note: recent applications have had less time to be answered.
/// "No response" is not necessarily rejection.
pub fn censoring_note(as_of: &str, earliest_submission: Option<&str>) -> String {
    match earliest_submission {
        None => "no submissions yet".to_string(),
        Some(first) => format!(
            "counts as of {as_of}; applications submitted since {first} have had less time \
             to receive responses — no response is not necessarily rejection"
        ),
    }
}

/// Effort metrics (D5): active candidate time and valid completed
/// applications, never raw sends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EffortMetrics {
    pub active_minutes: u64,
    pub confirmed_applications: u64,
    pub raw_sends: u64,
}

pub fn effort_summary(m: &EffortMetrics) -> String {
    format!(
        "{} active minutes; {} confirmed applications ({} raw sends — raw sends are not the success metric)",
        m.active_minutes, m.confirmed_applications, m.raw_sends
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_denominator_never_shows_a_rate() {
        assert_eq!(rate(0, 0), Err(RateLimit::EmptyDenominator));
        assert!((rate(2, 4).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn censoring_is_stated_with_dates() {
        let note = censoring_note("2026-09-25", Some("2026-09-24"));
        assert!(note.contains("less time"));
        assert!(note.contains("not necessarily rejection"));
        assert!(censoring_note("2026-09-25", None).contains("no submissions"));
    }

    #[test]
    fn effort_summary_prefers_confirmed_over_raw_sends() {
        let s = effort_summary(&EffortMetrics {
            active_minutes: 120,
            confirmed_applications: 5,
            raw_sends: 9,
        });
        assert!(s.contains("not the success metric"));
    }
}
