//! Typed view models. Rendering consumes these; domain actions never borrow UI.

use waypoint_domain::ApplicationState;
use waypoint_search::coverage::CoverageReport;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    YourNextMove,
    Discovery,
    Coverage,
    ModelQualification,
}

impl Screen {
    pub const ALL: [Screen; 4] = [
        Screen::YourNextMove,
        Screen::Discovery,
        Screen::Coverage,
        Screen::ModelQualification,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Screen::YourNextMove => "Your next move",
            Screen::Discovery => "Discover roles",
            Screen::Coverage => "Source coverage",
            Screen::ModelQualification => "Model qualification",
        }
    }
}
