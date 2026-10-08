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
            Screen::Facts => {
                self.facts_body(ui);
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
        self.detail_body(ui);
        self.rows_body(ui);
    }

    /// The selected job, its material/approval status, and only the actions the
    /// state machine actually allows. The pipeline stays the authority; this
    /// simply refuses to offer an impossible step.
    fn detail_body(&mut self, ui: &mut egui::Ui) {
        let Some(detail) = self.model.selection.clone() else {
            return;
        };
        let busy = self.model.busy.is_some();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("{} - {}", detail.title, detail.employer));
                ui.label(format!("[{}]", detail.state_label));
                if detail.remote {
                    ui.label("(remote)");
                }
            });
            if let Some(action) = &self.model.last_action {
                ui.colored_label(egui::Color32::from_rgb(140, 200, 140), action);
            }
            match &detail.packet {
                None => {
                    ui.label("No packet prepared for this role yet.");
                }
                Some(packet) => {
                    ui.label(format!(
                        "Packet {} - {} supported span(s)",
                        packet.hash_prefix, packet.supported_spans
                    ));
                    if packet.invalidated_at.is_some() {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 190, 120),
                            "Stale: a fact it cites was corrected. Re-prepare before approving.",
                        );
                    }
                    for reason in &packet.unsupported {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 150, 120),
                            format!("Needs evidence - {reason}"),
                        );
                    }
                }
            }
            if let Some(grant) = &detail.grant {
                ui.label(format!("Your approval: {}", grant.state_label()));
                ui.label(format!("  bound to {}", grant.destination_origin));
            }
            ui.horizontal(|ui| {
                let identity = detail.identity.clone();
                let can_shortlist = detail.state == waypoint_domain::ApplicationState::Discovered;
                if ui
                    .add_enabled(can_shortlist, egui::Button::new("Shortlist"))
                    .clicked()
                {
                    self.commands.push(ShellCommand::Shortlist(identity.clone()));
                }
                let can_prepare = matches!(
                    detail.state,
                    waypoint_domain::ApplicationState::Shortlisted
                        | waypoint_domain::ApplicationState::Prepared
                );
                if ui
                    .add_enabled(can_prepare, egui::Button::new("Prepare packet"))
                    .on_hover_text("Builds a packet from your attested facts only. Nothing is sent.")
                    .clicked()
                {
                    self.commands
                        .push(ShellCommand::PreparePacket(identity.clone()));
                }
                let can_approve = detail.state == waypoint_domain::ApplicationState::Prepared
                    && detail.packet.as_ref().is_some_and(|p| p.is_approvable());
                if ui
                    .add_enabled(can_approve, egui::Button::new("Approve (one use)"))
                    .on_hover_text(
                        "Records your explicit approval, bound to this exact packet and destination. \
                         It can be spent once and is voided if a fact changes.",
                    )
                    .clicked()
                {
                    self.commands
                        .push(ShellCommand::ApprovePacket(identity.clone()));
                }
                let can_export = detail.packet.is_some();
                let can_submit = detail.can_submit();
                if ui
                    .add_enabled(can_submit, egui::Button::new("Submit application"))
                    .on_hover_text(
                        "Opens the employer's form in a private browser profile, fills only \
                         what your packet genuinely answers, and submits once using your \
                         approval. A confirmation is only believed if the site gives a \
                         stable reference.",
                    )
                    .clicked()
                {
                    self.commands
                        .push(ShellCommand::SubmitPacket(identity.clone()));
                }
                if ui
                    .add_enabled(can_export, egui::Button::new("Export DOCX"))
                    .on_hover_text("Writes the prepared packet as a real .docx file.")
                    .clicked()
                {
                    self.commands
                        .push(ShellCommand::ExportPacket(identity.clone()));
                }
                if let Some(next) = detail.next_actions().first() {
                    ui.label(format!("(next: {next})"));
                }
            });

            if detail.needs_reconciliation() {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 150, 120),
                    "The last attempt was ambiguous: the employer may or may not have \
                     received it. Nothing is retried automatically.",
                );
                ui.horizontal(|ui| {
                    let identity = detail.identity.clone();
                    if ui
                        .add_enabled(!busy, egui::Button::new("I checked: it landed"))
                        .on_hover_text(
                            "Marks it confirmed because you verified it with the employer.",
                        )
                        .clicked()
                    {
                        self.commands
                            .push(ShellCommand::ConfirmLanded(identity.clone()));
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Retry anyway (may duplicate)"))
                        .on_hover_text(
                            "Records a deliberate retry after you accept the risk of \
                             applying twice.",
                        )
                        .clicked()
                    {
                        self.commands
                            .push(ShellCommand::RetrySubmit(identity.clone()));
                    }
                });
            }

            if !detail.history.is_empty() {
                ui.separator();
                ui.label("Submission history (append-only):");
                for row in detail.history.iter().rev().take(6) {
                    ui.label(format!("  {} — {} — {}", row.at, row.state_label, row.reason));
                }
            }
        });
    }

    /// Facts are the only source of material content. Inferred facts are
    /// labelled and must be confirmed before they can support an approval, so
    /// the panel never presents model guesses as the candidate's own words.
    fn facts_body(&mut self, ui: &mut egui::Ui) {
        ui.heading("Your facts");
        ui.label(
            "Facts are statements you stand behind. Anything the model inferred is labelled \
             and cannot support an approval until you confirm it.",
        );
        ui.separator();
        ui.label("Add facts (one per line):");
        ui.add(
            egui::TextEdit::multiline(&mut self.model.facts_draft)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Nine years building distributed systems"),
        );
        let draft = self.model.facts_draft.clone();
        let busy = self.model.busy.is_some();
        if ui
            .add_enabled(
                !busy && !draft.trim().is_empty(),
                egui::Button::new("Save facts"),
            )
            .clicked()
        {
            self.commands.push(ShellCommand::SaveFacts(draft));
        }

        ui.separator();
        if self.model.facts.is_empty() {
            ui.label("No facts yet: a packet prepared now would contain nothing to approve.");
            return;
        }
        ui.label(format!("{} fact(s) in force", self.model.facts.len()));
        let facts = self.model.facts.clone();
        if self.model.fact_edits.len() != facts.len() {
            self.model.fact_edits = facts.iter().map(|f| f.text.clone()).collect();
        }
        let mut edits = std::mem::take(&mut self.model.fact_edits);
        let mut to_correct: Option<(String, String)> = None;
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(ui, |ui| {
                for (i, fact) in facts.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if fact.inferred {
                            ui.colored_label(egui::Color32::from_rgb(220, 190, 120), "inferred");
                        }
                        ui.add(egui::TextEdit::singleline(&mut edits[i]).desired_width(500.0));
                        let changed = edits[i].trim() != fact.text.trim();
                        if ui
                            .add_enabled(changed && !busy, egui::Button::new("Correct"))
                            .on_hover_text(
                                "Corrects this fact everywhere it is used: affected packets become \
                                 stale and approvals that relied on it are voided.",
                            )
                            .clicked()
                        {
                            to_correct = Some((fact.id.clone(), edits[i].clone()));
                        }
                        if fact.inferred
                            && ui
                                .add_enabled(!busy, egui::Button::new("Confirm as mine"))
                                .clicked()
                        {
                            to_correct = Some((fact.id.clone(), fact.text.clone()));
                        }
                        if !fact.inferred && changed {
                            ui.label(format!("(currently: {})", fact.text));
                        }
                    });
                }
            });
        self.model.fact_edits = edits;
        if let Some((claim_id, new_text)) = to_correct {
            self.commands
                .push(ShellCommand::CorrectFact { claim_id, new_text });
        }
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
                        let identity = self.model.rows[i].identity.clone();
                        self.commands.push(ShellCommand::SelectJob(identity));
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
