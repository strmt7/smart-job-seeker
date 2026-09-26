//! Native shell: navigation + virtualized long list. Keyboard navigable.
//!
//! Gate G1 acceptance honoured here:
//! - No Node/Python/WSL runtime (pure Rust, eframe/wgpu).
//! - Virtualized rows via egui_extras StripBuilder + manual range slice; we
//!   only *layout* visible rows (50k fixture rows render fine).
//! - UI thread does no blocking work: view models arrive pre-built.

use crate::view_model::{Screen, ShellViewModel};

/// Everything the user can do, as typed messages. The controller (outside
/// the UI) decides what effect each has.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellMessage {
    Navigate(Screen),
    SelectJob(usize),
    RefreshJob(usize),
}

const ROW_HEIGHT: f32 = 26.0;

#[derive(Default)]
pub struct WaypointShell {
    pub model: ShellViewModel,
    pub selected: Option<usize>,
    /// Messages produced this frame, drained by the controller.
    pending: Vec<ShellMessage>,
}

impl WaypointShell {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn drain_messages(&mut self) -> Vec<ShellMessage> {
        std::mem::take(&mut self.pending)
    }

    /// Test-only probes so integration tests can drive a frame headlessly.
    #[doc(hidden)]
    pub fn top_bar_for_test(&mut self, ui: &mut egui::Ui) {
        self.top_bar(ui);
    }

    #[doc(hidden)]
    pub fn body_for_test(&mut self, ui: &mut egui::Ui) {
        self.body(ui);
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Waypoint");
            ui.separator();
            for screen in Screen::ALL {
                let selected = self.model.screen == screen;
                if ui.selectable_label(selected, screen.title()).clicked() {
                    self.pending.push(ShellMessage::Navigate(screen));
                }
            }
        });
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        ui.label(format!("{} indexed opportunities", self.model.total_jobs));
        if let Some(n) = &self.model.notice {
            ui.colored_label(egui::Color32::YELLOW, n);
        }
        ui.separator();

        match self.model.screen {
            Screen::Coverage => {
                // T019: per-source truth with explicit denominators. A failed
                // source is shown as degraded — never as "no results".
                if let Some(report) = &self.model.coverage {
                    ui.heading("Source coverage");
                    ui.label(report.summary_line());
                    ui.separator();
                    egui::ScrollArea::vertical().show_rows(
                        ui,
                        ROW_HEIGHT,
                        report.sources.len(),
                        |ui, range| {
                            for i in range {
                                let s = &report.sources[i];
                                let state = match s.state {
                                    waypoint_search::coverage::SourceFetchState::Ok => "OK",
                                    waypoint_search::coverage::SourceFetchState::Empty => {
                                        "empty (verified)"
                                    }
                                    waypoint_search::coverage::SourceFetchState::NetworkError => {
                                        "NETWORK ERROR"
                                    }
                                    waypoint_search::coverage::SourceFetchState::AccessDenied => {
                                        "ACCESS DENIED"
                                    }
                                    waypoint_search::coverage::SourceFetchState::Unsupported => {
                                        "UNSUPPORTED"
                                    }
                                    waypoint_search::coverage::SourceFetchState::ParserDrift => {
                                        "PARSER DRIFT"
                                    }
                                    waypoint_search::coverage::SourceFetchState::Stale => "STALE",
                                };
                                let detail = s.detail.as_deref().unwrap_or("");
                                let response = ui.selectable_label(
                                    false,
                                    format!(
                                        "{} — {} — {} results — last checked {} {detail}",
                                        s.source,
                                        state,
                                        s.results,
                                        s.last_checked.as_deref().unwrap_or("never")
                                    ),
                                );
                                let _ = response;
                            }
                        },
                    );
                } else {
                    ui.label("No sources checked yet.");
                }
                return;
            }
            Screen::ModelQualification => {
                ui.heading("Local model qualification");
                match &self.model.model_status {
                    Some(status) => {
                        ui.label(status);
                    }
                    None => {
                        ui.label("No model probe has run in this session.");
                    }
                }
                ui.separator();
                ui.label("See docs/g1/QUALIFICATION.md for the recorded hardware measurements.");
                return;
            }
            _ => {}
        }

        // Virtualized long list: only the visible row range is laid out, so a
        // 50k-row model costs the same as a 50-row one per frame.
        let total = self.model.rows.len();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let avail = ui.available_height();
                let visible = (avail / ROW_HEIGHT).ceil() as usize + 2;
                let first = ((viewport.top().abs()) / ROW_HEIGHT) as usize;
                let first = first.min(total.saturating_sub(1));
                let last = (first + visible).min(total);
                // Reserve total scroll space so the scroll handle maps to the
                // whole list.
                ui.add_space(first as f32 * ROW_HEIGHT);
                let mut clicked = None;
                for i in first..last {
                    let row = &self.model.rows[i];
                    let text = format!(
                        "{} — {} ({}) [{}]",
                        row.title, row.employer, row.location, row.state_label
                    );
                    let response = ui.selectable_label(self.selected == Some(i), text);
                    if response.clicked() {
                        clicked = Some(i);
                    }
                }
                ui.add_space(total.saturating_sub(last) as f32 * ROW_HEIGHT);
                if let Some(i) = clicked {
                    self.pending.push(ShellMessage::SelectJob(i));
                }
            });
    }
}

impl eframe::App for WaypointShell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("nav").show(ui, |ui| self.top_bar(ui));
        self.body(ui);
    }
}

/// Launch the native window and run the event loop. Blocks until closed.
pub fn run(model: ShellViewModel) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_title("Waypoint"),
        ..Default::default()
    };
    eframe::run_native(
        "waypoint",
        options,
        Box::new(move |_cc| {
            Ok(Box::new({
                let mut shell = WaypointShell::new();
                shell.model = model;
                shell
            }))
        }),
    )
}
