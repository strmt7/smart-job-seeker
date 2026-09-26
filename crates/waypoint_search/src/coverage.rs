//! Source coverage reporting (T019, gate G2).
//!
//! The app says which sources it searched, which failed, what remains stale
//! and which operations are unsupported — with explicit denominators (D5).
//! A source failure must never silently look like an empty search.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFetchState {
    /// Completed within budget; results present.
    Ok,
    /// Completed; genuinely zero results.
    Empty,
    /// Network/HTTP failure — last-known results may exist but are stale.
    NetworkError,
    /// 401/403/404 or auth wall.
    AccessDenied,
    /// Source locale or board not supported by any adapter.
    Unsupported,
    /// Parser drift detected (fixture mismatch, T034).
    ParserDrift,
    /// Completed earlier; results older than the source's stale window.
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceStatus {
    pub source: String,
    pub state: SourceFetchState,
    pub results: u64,
    pub last_checked: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageReport {
    pub sources: Vec<SourceStatus>,
}

impl CoverageReport {
    /// Human-facing summary that always names a denominator.
    pub fn summary_line(&self) -> String {
        let total: u64 = self.sources.iter().map(|s| s.results).sum();
        let failed: usize = self
            .sources
            .iter()
            .filter(|s| !matches!(s.state, SourceFetchState::Ok | SourceFetchState::Empty))
            .count();
        format!(
            "{} results from {} sources checked; {} sources degraded",
            total,
            self.sources.len(),
            failed
        )
    }

    /// Any stale or failed source blocks a "fully verified" claim.
    pub fn all_fresh_and_ok(&self) -> bool {
        self.sources
            .iter()
            .all(|s| matches!(s.state, SourceFetchState::Ok | SourceFetchState::Empty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_names_denominator_and_degraded_sources() {
        let r = CoverageReport {
            sources: vec![
                SourceStatus {
                    source: "greenhouse".into(),
                    state: SourceFetchState::Ok,
                    results: 42,
                    last_checked: Some("t1".into()),
                    detail: None,
                },
                SourceStatus {
                    source: "lever".into(),
                    state: SourceFetchState::NetworkError,
                    results: 0,
                    last_checked: Some("t0".into()),
                    detail: Some("timeout".into()),
                },
                SourceStatus {
                    source: "ashby".into(),
                    state: SourceFetchState::Stale,
                    results: 7,
                    last_checked: Some("old".into()),
                    detail: None,
                },
            ],
        };
        assert_eq!(
            r.summary_line(),
            "49 results from 3 sources checked; 2 sources degraded"
        );
        assert!(!r.all_fresh_and_ok());
    }

    #[test]
    fn empty_results_are_not_failures_but_no_full_verification_either() {
        let r = CoverageReport {
            sources: vec![SourceStatus {
                source: "greenhouse".into(),
                state: SourceFetchState::Empty,
                results: 0,
                last_checked: Some("t".into()),
                detail: None,
            }],
        };
        assert!(r.all_fresh_and_ok());
        assert_eq!(
            r.summary_line(),
            "0 results from 1 sources checked; 0 sources degraded"
        );
    }
}
