//! Waypoint native desktop shell (T005, gate G1).
//!
//! eframe/egui shell with typed view models and virtualized long lists.
//! Rendering is separate from domain actions: the shell holds immutable
//! view-model rows produced off the UI critical path, and interaction
//! produces typed `ShellMessage`s rather than touching stores directly.

pub use eframe;
/// Re-exported so the application never has to pin a matching UI toolkit
/// version (a classic source of "two egui versions" compile pain).
pub use egui;

pub mod shell;
pub mod view_model;

pub use shell::{run, Controller, ShellMessage, WaypointShell};
pub use view_model::{
    rows_from_ranked, state_label, ExcludedRow, JobRow, Screen, ShellCommand, ShellViewModel,
};
