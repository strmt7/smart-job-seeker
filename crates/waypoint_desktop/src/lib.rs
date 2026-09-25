//! Waypoint native desktop shell (T005, gate G1).
//!
//! eframe/egui shell with typed view models and virtualized long lists.
//! Rendering is separate from domain actions: the shell holds immutable
//! view-model rows produced off the UI critical path, and interaction
//! produces typed `ShellMessage`s rather than touching stores directly.

pub mod shell;
pub mod view_model;

pub use shell::{run, ShellMessage, WaypointShell};
pub use view_model::{JobRow, Screen, ShellViewModel};
