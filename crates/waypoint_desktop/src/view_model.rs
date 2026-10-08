//! Typed view models. Rendering consumes these; domain actions never borrow UI.
//!
//! Nothing here performs work: rows, coverage and statuses are produced on
//! worker threads and swapped in as plain data, so the UI thread can never be
//! blocked by network or model latency (efficiency contract, MASTER_PLAN §13).

use waypoint_domain::ApplicationState;
use waypoint_search::coverage::CoverageReport;
use waypoint_workspace::{ExcludedJob, RankedJob, RankedResults};

/// One row of the opportunity list — plain data, safe to produce on a worker.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub identity: String,
    pub title: String,
    pub employer: String,
    pub location: String,
    pub state: ApplicationState,
    pub state_label: String,
    pub last_checked: Option<String>,
    /// Discovery source that produced this job ("greenhouse", "lever", ...).
    pub source: String,
    /// Fraction of the candidate's skills evidenced in the posting text.
    pub score: f32,
    /// Comma-joined matched skills, e.g. "Rust, SQLite" — always explainable.
    pub matched: String,
}

/// A job excluded by the candidate's own strict filters, with the reasons.
/// Showing these is the difference between "honest" and "silently narrowed".
#[derive(Debug, Clone, PartialEq)]
pub struct ExcludedRow {
    pub identity: String,
    pub title: String,
    pub employer: String,
    pub reasons: Vec<String>,
}

/// A long-running action the shell has been asked to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellCommand {
    RunDiscovery,
    ProbeModel,
    /// Re-apply the strict filters currently held in the view model.
    ApplyFilters,
    /// Load the detail panel for one job.
    SelectJob(String),
    /// Record one fact per non-empty line (user-attested).
    SaveFacts(String),
    /// Correct a fact; propagation happens in the pipeline, not here.
    CorrectFact {
        claim_id: String,
        new_text: String,
    },
    Shortlist(String),
    PreparePacket(String),
    ApprovePacket(String),
    ExportPacket(String),
    /// The single external write, through a real browser.
    SubmitPacket(String),
    /// After an ambiguous write: the candidate says it did land.
    ConfirmLanded(String),
    /// After an ambiguous write: the candidate accepts the duplicate risk.
    RetrySubmit(String),
}

/// Immutable snapshot the shell renders from.
#[derive(Debug, Clone, Default)]
pub struct ShellViewModel {
    pub screen: Screen,
    pub total_jobs: usize,
    pub rows: Vec<JobRow>,
    pub notice: Option<String>,
    /// T019: truthful source coverage, shown with explicit denominators.
    pub coverage: Option<CoverageReport>,
    /// Model qualification summary from the last probe (T007/T008).
    pub model_status: Option<String>,
    /// Jobs the strict filters excluded, with reasons (D3 relaxed branch).
    pub excluded: Vec<ExcludedRow>,
    /// Set while a worker is running; the shell shows it and keeps repainting.
    pub busy: Option<String>,
    /// Strict filters, owned by the UI (immediate mode) and read by the worker.
    pub strict_remote_only: bool,
    /// Salary floor in *major* units, e.g. 90000. 0 = no floor.
    pub salary_floor_major: i64,
    /// What the candidate is looking for: free-text terms used for explainable
    /// evidence scoring ("rust sqlite"). Empty means score-only listing.
    pub query: String,
    /// Whether a workspace is attached at all (offline/demo runs say so).
    pub workspace_attached: bool,
    /// Candidate facts in force (render-ready).
    pub facts: Vec<crate::detail::FactRow>,
    /// Edit buffer per fact row, parallel to `facts`.
    pub fact_edits: Vec<String>,
    /// Draft for new facts: one fact per line.
    pub facts_draft: String,
    /// The selected job, with its packet and approval status.
    pub selection: Option<crate::detail::JobDetail>,
    /// Row index highlighted in the list (navigation bookkeeping only).
    pub selected_row: Option<usize>,
    /// Last completed action, reported verbatim to the candidate.
    pub last_action: Option<String>,
}

impl ShellViewModel {
    /// The query split into matchable terms. Deterministic and explainable.
    pub fn query_terms(&self) -> Vec<String> {
        self.query
            .split_whitespace()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// One-line truth about what the strict filters are hiding.
    pub fn exclusion_line(&self) -> Option<String> {
        if self.excluded.is_empty() {
            return None;
        }
        let first = self.excluded.first().map(|e| {
            format!(
                "{} — {}",
                e.title,
                e.reasons.first().cloned().unwrap_or_default()
            )
        });
        Some(format!(
            "{} role(s) excluded by your strict filters (unstated facts never count as a pass){}",
            self.excluded.len(),
            first.map(|f| format!("; e.g. {f}")).unwrap_or_default()
        ))
    }
}

/// Map pipeline results into render-ready rows. Pure function, unit-testable.
pub fn rows_from_ranked(results: &RankedResults) -> (Vec<JobRow>, Vec<ExcludedRow>) {
    let rows = results.strict.iter().map(row_from_ranked).collect();
    let excluded = results.excluded.iter().map(excluded_from).collect();
    (rows, excluded)
}

fn row_from_ranked(r: &RankedJob) -> JobRow {
    JobRow {
        identity: r.job.id.as_str().to_string(),
        title: r.job.title.clone(),
        employer: r.job.employer.clone(),
        location: r.job.location.clone(),
        state: r.job.state,
        state_label: state_label(r.job.state).to_string(),
        last_checked: r.job.last_seen.clone(),
        source: r.job.source.clone(),
        score: r.score,
        matched: r.matched_skills.join(", "),
    }
}

fn excluded_from(e: &ExcludedJob) -> ExcludedRow {
    ExcludedRow {
        identity: e.job.id.as_str().to_string(),
        title: e.job.title.clone(),
        employer: e.job.employer.clone(),
        reasons: e.reasons.clone(),
    }
}

/// Human labels for the application state machine.
pub fn state_label(state: ApplicationState) -> &'static str {
    match state {
        ApplicationState::Discovered => "discovered",
        ApplicationState::Shortlisted => "shortlisted",
        ApplicationState::Prepared => "prepared",
        ApplicationState::Approved => "approved",
        ApplicationState::Submitting => "submitting",
        ApplicationState::Confirmed => "confirmed",
        ApplicationState::Uncertain => "uncertain — reconcile before retry",
        ApplicationState::ExternalAttested => "applied outside Waypoint",
        ApplicationState::Rejected => "rejected",
        ApplicationState::Interview => "interview",
        ApplicationState::Offer => "offer",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    YourNextMove,
    Discovery,
    Facts,
    Coverage,
    ModelQualification,
}

impl Screen {
    pub const ALL: [Screen; 5] = [
        Screen::YourNextMove,
        Screen::Discovery,
        Screen::Facts,
        Screen::Coverage,
        Screen::ModelQualification,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Screen::YourNextMove => "Your next move",
            Screen::Discovery => "Discover roles",
            Screen::Facts => "Facts & materials",
            Screen::Coverage => "Source coverage",
            Screen::ModelQualification => "Model qualification",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::JobIdentityId;
    use waypoint_store::StoredJob;

    fn job(title: &str, source: &str) -> StoredJob {
        StoredJob {
            id: JobIdentityId::new(format!("id::{title}")).unwrap(),
            title: title.into(),
            employer: "acme".into(),
            location: "Zurich".into(),
            state: ApplicationState::Discovered,
            canonical_url: Some("https://x/1".into()),
            content_hash: Some("h".into()),
            first_seen: Some("100".into()),
            last_seen: Some("200".into()),
            remote: false,
            source: source.into(),
            requisition_id: Some("1".into()),
            posted_at: None,
        }
    }

    #[test]
    fn mapping_preserves_explainability() {
        let results = RankedResults {
            strict: vec![RankedJob {
                job: job("Rust Engineer", "ashby"),
                score: 0.5,
                matched_skills: vec!["Rust".into()],
                freshness: waypoint_search::Freshness::Fresh { age_seconds: 5 },
            }],
            excluded: vec![ExcludedJob {
                job: job("Sales Lead", "lever"),
                reasons: vec!["salary: salary not stated".into()],
            }],
        };
        let (rows, excluded) = rows_from_ranked(&results);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].matched, "Rust");
        assert_eq!(rows[0].source, "ashby");
        assert_eq!(rows[0].state_label, "discovered");
        assert_eq!(excluded.len(), 1);
        assert!(excluded[0].reasons[0].starts_with("salary:"));

        let mut model = ShellViewModel::default();
        assert!(model.exclusion_line().is_none());
        model.excluded = excluded;
        let line = model
            .exclusion_line()
            .expect("a line when roles are hidden");
        assert!(line.contains("1 role(s) excluded"));
        assert!(line.contains("never count as a pass"));
    }

    #[test]
    fn query_terms_are_split_and_trimmed() {
        let mut model = ShellViewModel::default();
        assert!(model.query_terms().is_empty());
        model.query = "  rust   sqlite ".into();
        assert_eq!(model.query_terms(), vec!["rust", "sqlite"]);
    }

    #[test]
    fn every_state_has_a_label_and_uncertain_warns() {
        assert_eq!(state_label(ApplicationState::Confirmed), "confirmed");
        assert!(state_label(ApplicationState::Uncertain).contains("reconcile"));
        assert_eq!(
            state_label(ApplicationState::ExternalAttested),
            "applied outside Waypoint"
        );
    }
}
