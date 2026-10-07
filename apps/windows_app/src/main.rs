//! Waypoint — native Windows job-search client.
//!
//! Wiring only: this binary owns the things that can block (SQLite, HTTPS, the
//! local model) and hands finished plain data to the shell. No business rule
//! lives here — the crates own those.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use waypoint_desktop::shell::Controller;
use waypoint_desktop::view_model::ShellCommand;
use waypoint_desktop::view_model::{rows_from_ranked, Screen, ShellViewModel};
use waypoint_inference::{Inference, OllamaInference};
use waypoint_search::CampaignConstraints;
use waypoint_workspace::{
    DiscoveryOutcome, DiscoveryTarget, LiveFetcher, RankedResults, Workspace, WorkspaceError,
};

/// Public ATS boards the demo ships with. These are boards the source
/// permission registry marks `allowed_public` for discovery; the registry is
/// re-checked inside the pipeline for every fetch, so this list cannot widen
/// what the app is permitted to do.
const DEMO_BOARDS: &[(&str, &str)] = &[
    ("greenhouse", "vercel"),
    ("lever", "spotify"),
    ("ashby", "ashby"),
];

/// Local reasoning runtime. A literal loopback address by construction — the
/// adapter refuses anything else, so there is no cloud path to configure.
const INFERENCE_ORIGIN: &str = "http://127.0.0.1:11434";
const INFERENCE_MODEL: &str = "qwen3.5:9b";

/// Results of background work, as plain data.
enum Update {
    Discovered {
        coverage: waypoint_search::coverage::CoverageReport,
        summary: String,
        ranked: RankedResults,
    },
    Ranked(RankedResults),
    Model(String),
    Failed(String),
}

struct AppController {
    workspace: Arc<Mutex<Workspace>>,
    tx: Sender<Update>,
    rx: Receiver<Update>,
}

impl AppController {
    fn new(workspace: Workspace) -> Self {
        let (tx, rx) = channel();
        Self {
            workspace: Arc::new(Mutex::new(workspace)),
            tx,
            rx,
        }
    }

    fn constraints(model: &ShellViewModel) -> CampaignConstraints {
        // Salary is entered in major units (e.g. 90000 CHF) and stored in minor
        // units, matching how `Amount` is compared everywhere else.
        let salary = if model.salary_floor_major > 0 {
            Some((model.salary_floor_major.saturating_mul(100), i64::MAX / 4))
        } else {
            None
        };
        CampaignConstraints {
            remote_only: model.strict_remote_only,
            salary,
            ..CampaignConstraints::default()
        }
    }

    fn spawn_discovery(
        &mut self,
        model: &mut ShellViewModel,
        ctx: &waypoint_desktop::egui::Context,
    ) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        let targets: Vec<DiscoveryTarget> = DEMO_BOARDS
            .iter()
            .map(|(s, b)| DiscoveryTarget::new(s, b))
            .collect();
        model.busy = Some("Discovering public boards…".into());
        ctx.request_repaint();
        std::thread::spawn(move || {
            let msg = match ws.lock() {
                Ok(mut ws) => {
                    let outcome: DiscoveryOutcome =
                        ws.discover(&targets, &LiveFetcher::default(), now_unix());
                    let ranked = ws
                        .ranked(&constraints, &terms, now_unix())
                        .unwrap_or_default();
                    Update::Discovered {
                        coverage: outcome.coverage.clone(),
                        summary: outcome.summary(),
                        ranked,
                    }
                }
                Err(_) => Update::Failed("workspace lock poisoned".into()),
            };
            let _ = tx.send(msg);
        });
    }

    fn spawn_ranking(&mut self, model: &mut ShellViewModel, ctx: &waypoint_desktop::egui::Context) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        model.busy = Some("Applying filters…".into());
        ctx.request_repaint();
        std::thread::spawn(move || {
            let msg = match ws.lock() {
                Ok(ws) => match ws.ranked(&constraints, &terms, now_unix()) {
                    Ok(r) => Update::Ranked(r),
                    Err(e) => Update::Failed(e.to_string()),
                },
                Err(_) => Update::Failed("workspace lock poisoned".into()),
            };
            let _ = tx.send(msg);
        });
    }

    fn spawn_probe(&mut self, model: &mut ShellViewModel, ctx: &waypoint_desktop::egui::Context) {
        let tx = self.tx.clone();
        model.busy = Some("Probing the local model (first load can take ~12 s)…".into());
        ctx.request_repaint();
        std::thread::spawn(move || {
            let msg = match OllamaInference::new(INFERENCE_ORIGIN, INFERENCE_MODEL) {
                Ok(mut rt) => match rt.probe() {
                    Ok(cap) => {
                        let vram = cap
                            .gpu_reserved_bytes
                            .map(|b| {
                                format!("{:.2} GiB reserved", b as f64 / 1024.0 / 1024.0 / 1024.0)
                            })
                            .unwrap_or_else(|| "VRAM not reported".into());
                        let offload = if cap.cpu_offload_bytes == 0 {
                            "fully GPU-resident".to_string()
                        } else {
                            format!("{} bytes on CPU", cap.cpu_offload_bytes)
                        };
                        Update::Model(format!(
                            "Qualified: {} ({}) — {} — {} — context {} tokens — structured output: yes, reasoning: yes",
                            cap.model_name,
                            cap.quantization,
                            vram,
                            offload,
                            cap.context_window_tokens
                        ))
                    }
                    Err(e) => Update::Model(format!("Not qualified: {e}")),
                },
                Err(e) => Update::Failed(e.to_string()),
            };
            let _ = tx.send(msg);
        });
    }
}

impl Controller for AppController {
    fn poll(&mut self, _ctx: &waypoint_desktop::egui::Context, model: &mut ShellViewModel) {
        while let Ok(update) = self.rx.try_recv() {
            model.busy = None;
            match update {
                Update::Discovered {
                    coverage,
                    summary,
                    ranked,
                } => {
                    let (rows, excluded) = rows_from_ranked(&ranked);
                    model.total_jobs = rows.len();
                    model.rows = rows;
                    model.excluded = excluded;
                    model.coverage = Some(coverage);
                    model.notice = Some(summary);
                }
                Update::Ranked(ranked) => {
                    let (rows, excluded) = rows_from_ranked(&ranked);
                    model.total_jobs = rows.len();
                    model.rows = rows;
                    model.excluded = excluded;
                }
                Update::Model(status) => model.model_status = Some(status),
                Update::Failed(message) => model.notice = Some(format!("Failed: {message}")),
            }
        }
    }

    fn handle(
        &mut self,
        command: ShellCommand,
        ctx: &waypoint_desktop::egui::Context,
        model: &mut ShellViewModel,
    ) {
        // One worker at a time: bounded by construction (MASTER_PLAN §13).
        if model.busy.is_some() {
            return;
        }
        match command {
            ShellCommand::RunDiscovery => self.spawn_discovery(model, ctx),
            ShellCommand::ApplyFilters => self.spawn_ranking(model, ctx),
            ShellCommand::ProbeModel => self.spawn_probe(model, ctx),
        }
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Where the store lives. Per-user application data, never the repo.
fn data_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|base| PathBuf::from(base).join("smart-job-seeker"))
        .unwrap_or_else(|_| PathBuf::from(".waypoint"))
}

fn main() -> eframe::Result<()> {
    let mut model = ShellViewModel {
        screen: Screen::YourNextMove,
        workspace_attached: false,
        ..Default::default()
    };

    let controller: Option<Box<dyn Controller>> = match Workspace::open(&data_dir()) {
        Ok(ws) => {
            model.workspace_attached = true;
            let controller = AppController::new(ws);
            // First frame shows what is already tracked, without any network.
            match controller.spawn_initial_load(&mut model) {
                Ok(()) => {}
                Err(e) => model.notice = Some(format!("Could not read the store: {e}")),
            }
            Some(Box::new(controller))
        }
        Err(e) => {
            model.notice = Some(format!(
                "Could not open the local store: {e}. Running read-only."
            ));
            None
        }
    };

    waypoint_desktop::run(model, controller)
}

impl AppController {
    /// Load what is already stored, on a worker, so startup never waits on I/O.
    fn spawn_initial_load(&self, model: &mut ShellViewModel) -> Result<(), WorkspaceError> {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        std::thread::spawn(move || {
            let msg = match ws.lock() {
                Ok(ws) => match ws.ranked(&constraints, &terms, now_unix()) {
                    Ok(r) => Update::Ranked(r),
                    Err(e) => Update::Failed(e.to_string()),
                },
                Err(_) => Update::Failed("workspace lock poisoned".into()),
            };
            let _ = tx.send(msg);
        });
        Ok(())
    }
}
