//! Shell integration tests: virtualization, non-blocking frames, and the
//! controller boundary (the UI never touches a store or the network itself).

use egui::Context;
use waypoint_desktop::shell::Controller;
use waypoint_desktop::{JobRow, Screen, ShellCommand, ShellMessage, ShellViewModel, WaypointShell};
use waypoint_domain::ApplicationState;

fn fixture_rows(n: usize) -> Vec<JobRow> {
    (0..n)
        .map(|i| JobRow {
            identity: format!("job-{i}"),
            title: format!("Role {i}"),
            employer: format!("Employer {}", i % 97),
            location: "Zurich".into(),
            state: ApplicationState::Discovered,
            state_label: "discovered".into(),
            last_checked: Some("1758900000".into()),
            source: "greenhouse".into(),
            score: 0.0,
            matched: String::new(),
        })
        .collect()
}

fn render_one_frame(shell: &mut WaypointShell) -> egui::FullOutput {
    let ctx = Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1180.0, 760.0),
        )),
        ..Default::default()
    };
    ctx.begin_pass(raw);
    // Exactly the shipping frame path (poll -> render -> dispatch).
    egui::Window::new("body").show(&ctx, |ui| {
        shell.frame(ui);
    });
    ctx.end_pass()
}

#[test]
fn shell_builds_with_50k_virtualized_rows() {
    let mut shell = WaypointShell::new();
    shell.model = ShellViewModel {
        total_jobs: 50_000,
        rows: fixture_rows(50_000),
        ..Default::default()
    };
    assert_eq!(shell.model.rows.len(), 50_000);
}

/// One frame with 50k rows must stay far inside the paint budget: only the
/// visible range is laid out (measured time is printed for the record).
#[test]
fn one_frame_completes_quickly_with_50k_rows() {
    let mut shell = WaypointShell::new();
    shell.model = ShellViewModel {
        screen: Screen::Discovery,
        total_jobs: 50_000,
        rows: fixture_rows(50_000),
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let mut full_output = render_one_frame(&mut shell);
    full_output.textures_delta.clear();
    let elapsed = start.elapsed();

    eprintln!("frame time with 50k rows: {elapsed:?}");
    assert!(elapsed.as_millis() < 500, "frame too slow: {elapsed:?}");
    assert!(!full_output.shapes.is_empty());
}

/// The shell must never perform work itself: with no controller attached,
/// asking for discovery is a no-op rather than a blocking call.
#[test]
fn commands_are_inert_without_a_controller() {
    let mut shell = WaypointShell::new();
    shell.push_command_for_test(ShellCommand::RunDiscovery);
    let commands = shell.drain_commands();
    assert_eq!(commands, vec![ShellCommand::RunDiscovery]);
    assert!(
        shell.model.busy.is_none(),
        "the shell must not set work state itself"
    );
}

/// A recording controller proves the boundary: the UI emits typed commands,
/// and finished data flows back through the view model only.
struct RecordingController {
    seen: std::sync::Arc<std::sync::Mutex<Vec<ShellCommand>>>,
    /// Set by `handle`, consumed by the next `poll` — exactly like a worker
    /// that finishes between two frames.
    pending_result: bool,
}

impl Controller for RecordingController {
    fn poll(&mut self, _ctx: &Context, model: &mut ShellViewModel) {
        if self.pending_result {
            self.pending_result = false;
            // Simulate a worker having completed: plain data, no I/O here.
            model.busy = None;
            model.total_jobs = 1;
            model.rows = fixture_rows(1);
            model.notice = Some("1 new, 0 updated from 1 sources".into());
        }
    }

    fn handle(&mut self, command: ShellCommand, _ctx: &Context, model: &mut ShellViewModel) {
        self.seen.lock().unwrap().push(command);
        model.busy = Some("Discovering public boards…".into());
        self.pending_result = true;
    }
}

#[test]
fn controller_receives_commands_and_publishes_results() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let controller = RecordingController {
        seen: std::sync::Arc::clone(&seen),
        pending_result: false,
    };
    let mut shell = WaypointShell::with_controller(Box::new(controller));

    shell.push_command_for_test(ShellCommand::RunDiscovery);
    let commands = shell.drain_commands();
    assert_eq!(commands.len(), 1);
    for command in commands {
        shell.dispatch_command_for_test(command);
    }
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(
        shell.model.busy.as_deref(),
        Some("Discovering public boards…")
    );

    // Next frame: the controller publishes the finished rows.
    let mut full_output = render_one_frame(&mut shell);
    full_output.textures_delta.clear();
    assert_eq!(shell.model.total_jobs, 1);
    assert!(shell.model.busy.is_none());
}

#[test]
fn navigation_messages_are_typed() {
    let mut shell = WaypointShell::new();
    let none: Vec<ShellMessage> = shell.drain_messages();
    assert!(none.is_empty());
    assert_eq!(
        Screen::ALL.len(),
        5,
        "screens: next move, discovery, facts, coverage, model"
    );
}
