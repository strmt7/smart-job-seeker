//! Native shell: navigation, honest status, virtualized long list.
//!
//! Gate G1 acceptance honoured here:
//! - No Node/Python/WSL runtime (pure Rust, eframe/wgpu).
//! - Virtualized rows: only the visible range is laid out, so a 50k-row model
//!   costs the same per frame as a 50-row one.
//! - The UI thread never blocks: all long work is performed by an injected
//!   [`Controller`] on worker threads, which swaps finished plain data into the
//!   view model. The shell only ever renders and emits intents.

use crate::view_model::{Screen, ShellCommand, ShellViewModel};

/// Navigation and selection intents. Cheap, synchronous UI facts.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellMessage {
    Navigate(Screen),
    SelectJob(usize),
    RefreshJob(usize),
}

/// The controller owns everything that can block: network, store, model.
/// Implemented by the application (see `apps/windows_app`), which returns
/// results by mutating the view model from `poll`.
pub trait Controller {
    /// Called once per frame, before rendering. Apply finished async work here.
    fn poll(&mut self, ctx: &egui::Context, model: &mut ShellViewModel);

    /// Called with each command the user requested this frame.
    fn handle(&mut self, command: ShellCommand, ctx: &egui::Context, model: &mut ShellViewModel);
}

const ROW_HEIGHT: f32 = 26.0;

#[derive(Default)]
pub struct WaypointShell {
    pub model: ShellViewModel,
    pub selected: Option<usize>,
    /// Navigation/selection messages produced this frame.
    pending: Vec<ShellMessage>,
    /// Work commands produced this frame, drained by the controller.
    commands: Vec<ShellCommand>,
    controller: Option<Box<dyn Controller>>,
}

impl WaypointShell {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_controller(controller: Box<dyn Controller>) -> Self {
        Self {
            controller: Some(controller),
            ..Default::default()
        }
    }

    pub fn drain_messages(&mut self) -> Vec<ShellMessage> {
        std::mem::take(&mut self.pending)
    }

    pub fn drain_commands(&mut self) -> Vec<ShellCommand> {
        std::mem::take(&mut self.commands)
    }

    /// One complete frame, in the shipping order:
    /// `poll` background results -> render -> dispatch this frame's intents.
    ///
    /// The application's `eframe::App::ui` calls exactly this, so headless
    /// tests exercise the same code path that ships — no test-only rendering
    /// lane that could drift from production behaviour.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // 1. Apply any finished background work before rendering this frame.
        if let Some(controller) = self.controller.as_mut() {
            controller.poll(&ctx, &mut self.model);
        }

        egui::Panel::top("nav").show(ui, |ui| self.top_bar(ui));
        self.body(ui);

        // 2. Hand this frame's intents to the controller.
        let commands = std::mem::take(&mut self.commands);
        if let Some(controller) = self.controller.as_mut() {
            for command in commands {
                controller.handle(command, &ctx, &mut self.model);
            }
        }

        // 3. Keep polling while a worker is running (no busy-wait loop).
        if self.model.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    #[doc(hidden)]
    pub fn top_bar_for_test(&mut self, ui: &mut egui::Ui) {
        self.top_bar(ui);
    }

    #[doc(hidden)]
    pub fn body_for_test(&mut self, ui: &mut egui::Ui) {
        self.body(ui);
    }

    #[doc(hidden)]
    pub fn push_command_for_test(&mut self, command: ShellCommand) {
        self.commands.push(command);
    }

    /// Test-only: route one command through the attached controller, using a
    /// throwaway context, so the controller boundary is exercised headlessly.
    #[doc(hidden)]
    pub fn dispatch_command_for_test(&mut self, command: ShellCommand) {
        let ctx = egui::Context::default();
        if let Some(controller) = self.controller.as_mut() {
            controller.handle(command, &ctx, &mut self.model);
        }
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
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match &self.model.busy {
                    Some(what) => {
                        ui.spinner();
                        ui.label(what.clone());
                    }
                    None => {
                        ui.label(format!("{} tracked", self.model.total_jobs));
                    }
                },
            );
        });
    }

    /// The strict-filter toolbar. Filters are strict by construction: they can
    /// only ever exclude, and exclusions are always shown with reasons.
    fn filter_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let can_run = self.model.busy.is_none();
            if ui
                .add_enabled(can_run, egui::Button::new("Discover public boards"))
                .on_hover_text(
                    "Fetches public ATS boards you allowed (Greenhouse, Lever, Ashby). \
                     Uses network access; nothing is sent about you.",
                )
                .clicked()
            {
                self.commands.push(ShellCommand::RunDiscovery);
            }
            if ui
                .add_enabled(can_run, egui::Button::new("Probe local model"))
                .on_hover_text("Starts the local reasoning runtime and checks it is qualified.")
                .clicked()
            {
                self.commands.push(ShellCommand::ProbeModel);
            }
            ui.separator();
            ui.label("Look for:");
            ui.add(
                egui::TextEdit::singleline(&mut self.model.query)
                    .desired_width(180.0)
                    .hint_text("rust sqlite"),
            );
            ui.checkbox(&mut self.model.strict_remote_only, "Remote only");
            ui.label("Salary floor:");
            ui.add(
                egui::DragValue::new(&mut self.model.salary_floor_major)
                    .speed(1000.0)
                    .range(0..=5_000_000),
            );
            if ui
                .add_enabled(can_run, egui::Button::new("Apply filters"))
                .on_hover_text(
                    "Strict: roles that do not state a fact you require are excluded \
                     and listed, never silently kept.",
                )
                .clicked()
            {
                self.commands.push(ShellCommand::ApplyFilters);
            }
        });
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        if !self.model.workspace_attached {
            ui.colored_label(
                egui::Color32::from_rgb(200, 160, 60),
                "No workspace attached — showing an empty read-only view.",
            );
        }
        if let Some(n) = &self.model.notice {
            ui.colored_label(egui::Color32::from_rgb(140, 200, 140), n);
        }
        if let Some(line) = self.model.exclusion_line() {
            ui.colored_label(egui::Color32::from_rgb(220, 190, 120), line);
        }
        ui.separator();

        match self.model.screen {
            Screen::Coverage => {
                self.coverage_body(ui);
                return;
            }
            Screen::ModelQualification => {
                ui.heading("Local model qualification");
                match &self.model.model_status {
                    Some(status) => {
                        ui.label(status);
                    }
                    None => {
                        ui.label(
                            "No model probe has run in this session. \
                             Press \"Probe local model\" to check the local runtime.",
                        );
                    }
                }
                ui.separator();
                ui.label(
                    "Waypoint only ever dials a literal loopback address, and never \
                     falls back to a paid cloud model.",
                );
                return;
            }
            Screen::YourNextMove => {
                self.next_move_body(ui);
                return;
            }
            Screen::Discovery => {}
        }

        self.filter_bar(ui);
        ui.separator();
        self.rows_body(ui);
    }

    /// Deterministic "what should I do next" — no model call required.
    fn next_move_body(&mut self, ui: &mut egui::Ui) {
        ui.heading("Your next move");
        if self.model.rows.is_empty() && self.model.excluded.is_empty() {
            ui.label("Nothing tracked yet. Run a discovery pass over the public boards it is allowed to read.");
            if ui.button("Discover public boards").clicked() {
                self.commands.push(ShellCommand::RunDiscovery);
            }
            return;
        }
        ui.label(format!(
            "{} roles in strict results.",
            self.model.rows.len()
        ));
        if let Some(line) = self.model.exclusion_line() {
            ui.colored_label(egui::Color32::from_rgb(220, 190, 120), line);
        }
        if let Some(report) = &self.model.coverage {
            if !report.all_fresh_and_ok() {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 150, 120),
                    format!(
                        "Coverage is degraded: {}. Results below are partial, not empty.",
                        report.summary_line()
                    ),
                );
            }
        }
        ui.separator();
        ui.label("Best evidence matches:");
        self.rows_body(ui);
    }

    fn coverage_body(&mut self, ui: &mut egui::Ui) {
        // T019: per-source truth with explicit denominators. A failed source is
        // shown as degraded — never as "no results".
        let Some(report) = self.model.coverage.clone() else {
            ui.label("No sources checked yet. Run a discovery pass to populate coverage.");
            return;
        };
        ui.heading("Source coverage");
        ui.label(report.summary_line());
        if !report.all_fresh_and_ok() {
            ui.colored_label(
                egui::Color32::from_rgb(230, 150, 120),
                "Some sources are degraded — this is stated rather than hidden.",
            );
        }
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
                        waypoint_search::coverage::SourceFetchState::Empty => "empty (verified)",
                        waypoint_search::coverage::SourceFetchState::NetworkError => {
                            "NETWORK ERROR"
                        }
                        waypoint_search::coverage::SourceFetchState::AccessDenied => {
                            "ACCESS DENIED"
                        }
                        waypoint_search::coverage::SourceFetchState::Unsupported => "NOT PERMITTED",
                        waypoint_search::coverage::SourceFetchState::ParserDrift => "PARSER DRIFT",
                        waypoint_search::coverage::SourceFetchState::Stale => "STALE",
                    };
                    let detail = s.detail.as_deref().unwrap_or("");
                    ui.label(format!(
                        "{} — {} — {} results — checked {}{}",
                        s.source,
                        state,
                        s.results,
                        s.last_checked.as_deref().unwrap_or("never"),
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!(" — {detail}")
                        }
                    ));
                }
            },
        );
    }

    fn rows_body(&mut self, ui: &mut egui::Ui) {
        let total = self.model.rows.len();
        if total == 0 {
            ui.label("No rows in the strict result set.");
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let avail = ui.available_height();
                let visible = (avail / ROW_HEIGHT).ceil() as usize + 2;
                let first = ((viewport.top().abs()) / ROW_HEIGHT) as usize;
                let first = first.min(total.saturating_sub(1));
                let last = (first + visible).min(total);
                ui.add_space(first as f32 * ROW_HEIGHT);
                let mut clicked = None;
                for i in first..last {
                    let row = &self.model.rows[i];
                    let matched = if row.matched.is_empty() {
                        String::new()
                    } else {
                        format!(" · matched: {}", row.matched)
                    };
                    let text = format!(
                        "{} — {} ({}) [{}] · {}{}",
                        row.title, row.employer, row.location, row.state_label, row.source, matched
                    );
                    let response = ui
                        .selectable_label(self.selected == Some(i), text)
                        .on_hover_text(format!(
                            "identity {}\nlast observed {}",
                            row.identity,
                            row.last_checked.as_deref().unwrap_or("never")
                        ));
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
        self.frame(ui);
    }
}

/// Launch the native window and run the event loop. Blocks until closed.
pub fn run(model: ShellViewModel, controller: Option<Box<dyn Controller>>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_title("Waypoint — job search that does not lie to you"),
        ..Default::default()
    };
    eframe::run_native(
        "waypoint",
        options,
        Box::new(move |_cc| {
            Ok(Box::new({
                let mut shell = match controller {
                    Some(c) => WaypointShell::with_controller(c),
                    None => WaypointShell::new(),
                };
                shell.model = model;
                shell
            }))
        }),
    )
}
