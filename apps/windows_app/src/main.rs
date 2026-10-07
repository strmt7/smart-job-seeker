//! Waypoint — native Windows job-search client.
//!
//! Wiring only: this binary owns the things that can block (SQLite, HTTPS, the
//! local model) and hands finished plain data to the shell. No business rule
//! lives here — the crates own those, including every approval invariant.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// The UI toolkit comes from the shell crate, so the app can never pin a
// mismatched egui/eframe version.
use waypoint_desktop::detail::detail_from;
use waypoint_desktop::egui;
use waypoint_desktop::shell::Controller;
use waypoint_desktop::view_model::{rows_from_ranked, Screen, ShellCommand, ShellViewModel};
use waypoint_desktop::JobDetail;
use waypoint_domain::{ApplicationState, JobIdentityId, Sha256Digest};
use waypoint_inference::{Inference, OllamaInference};
use waypoint_search::CampaignConstraints;
use waypoint_workspace::{
    DiscoveryOutcome, DiscoveryTarget, FactOrigin, LiveFetcher, RankedResults, Workspace,
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
    Facts(Vec<waypoint_store::work::StoredClaim>),
    Detail(Box<JobDetail>),
    Model(String),
    Notice(String),
    Failed(String),
}

struct AppController {
    workspace: Arc<Mutex<Workspace>>,
    tx: Sender<Update>,
    rx: Receiver<Update>,
    /// The job whose detail panel is open, reloaded after every mutation.
    selection: Option<String>,
    data_dir: PathBuf,
}

impl AppController {
    fn new(workspace: Workspace, data_dir: PathBuf) -> Self {
        let (tx, rx) = channel();
        Self {
            workspace: Arc::new(Mutex::new(workspace)),
            tx,
            rx,
            selection: None,
            data_dir,
        }
    }

    fn constraints(model: &ShellViewModel) -> CampaignConstraints {
        // Salary is entered in major units (e.g. 90000 CHF); it is converted to
        // minor units because that is how `Amount` is compared everywhere.
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

    /// Busy label set for one worker run.
    fn start(&self, model: &mut ShellViewModel, ctx: &egui::Context, label: &str) {
        model.busy = Some(label.to_string());
        ctx.request_repaint();
    }

    /// After any mutation: republish facts, ranked rows and the open detail.
    fn publish(
        ws: &Workspace,
        tx: &Sender<Update>,
        selection: &Option<String>,
        constraints: &CampaignConstraints,
        terms: &[String],
    ) {
        if let Ok(facts) = ws.facts() {
            let _ = tx.send(Update::Facts(facts));
        }
        if let Ok(ranked) = ws.ranked(constraints, terms, now_unix()) {
            let _ = tx.send(Update::Ranked(ranked));
        }
        if let Some(identity) = selection {
            if let Some(detail) = build_detail(ws, identity) {
                let _ = tx.send(Update::Detail(Box::new(detail)));
            }
        }
    }

    fn spawn_discovery(&mut self, model: &mut ShellViewModel, ctx: &egui::Context) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        let selection = self.selection.clone();
        let targets: Vec<DiscoveryTarget> = DEMO_BOARDS
            .iter()
            .map(|(s, b)| DiscoveryTarget::new(s, b))
            .collect();
        self.start(model, ctx, "Discovering public boards…");
        std::thread::spawn(move || match ws.lock() {
            Ok(mut ws) => {
                let outcome: DiscoveryOutcome =
                    ws.discover(&targets, &LiveFetcher::default(), now_unix());
                let ranked = ws
                    .ranked(&constraints, &terms, now_unix())
                    .unwrap_or_default();
                let _ = tx.send(Update::Discovered {
                    coverage: outcome.coverage.clone(),
                    summary: outcome.summary(),
                    ranked,
                });
                if let Ok(facts) = ws.facts() {
                    let _ = tx.send(Update::Facts(facts));
                }
                if let Some(identity) = &selection {
                    if let Some(detail) = build_detail(&ws, identity) {
                        let _ = tx.send(Update::Detail(Box::new(detail)));
                    }
                }
            }
            Err(_) => {
                let _ = tx.send(Update::Failed("workspace lock poisoned".into()));
            }
        });
    }

    fn spawn_ranking(&mut self, model: &mut ShellViewModel, ctx: &egui::Context) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        self.start(model, ctx, "Applying filters…");
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

    fn spawn_probe(&mut self, model: &mut ShellViewModel, ctx: &egui::Context) {
        let tx = self.tx.clone();
        self.start(
            model,
            ctx,
            "Probing the local model (first load can take ~12 s)…",
        );
        std::thread::spawn(move || {
            let msg = match OllamaInference::new(INFERENCE_ORIGIN, INFERENCE_MODEL) {
                Ok(mut rt) => match rt.probe() {
                    Ok(cap) => {
                        let vram = cap
                            .gpu_reserved_bytes
                            .map(|b| format!("{:.2} GiB reserved", b as f64 / 1073741824.0))
                            .unwrap_or_else(|| "VRAM not reported".into());
                        let offload = if cap.cpu_offload_bytes == 0 {
                            "fully GPU-resident".to_string()
                        } else {
                            format!("{} bytes on CPU", cap.cpu_offload_bytes)
                        };
                        Update::Model(format!(
                            "Qualified: {} ({}) — {} — {} — context {} tokens — structured output: yes, reasoning: yes",
                            cap.model_name, cap.quantization, vram, offload, cap.context_window_tokens
                        ))
                    }
                    Err(e) => Update::Model(format!("Not qualified: {e}")),
                },
                Err(e) => Update::Failed(e.to_string()),
            };
            let _ = tx.send(msg);
        });
    }

    fn spawn_select(&mut self, identity: String, model: &mut ShellViewModel, ctx: &egui::Context) {
        self.selection = Some(identity.clone());
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        self.start(model, ctx, "Loading role…");
        std::thread::spawn(move || {
            let msg = match ws.lock() {
                Ok(ws) => match build_detail(&ws, &identity) {
                    Some(detail) => Update::Detail(Box::new(detail)),
                    None => Update::Failed(format!("no tracked job {identity}")),
                },
                Err(_) => Update::Failed("workspace lock poisoned".into()),
            };
            let _ = tx.send(msg);
        });
    }

    fn spawn_save_facts(&mut self, text: String, model: &mut ShellViewModel, ctx: &egui::Context) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        let selection = self.selection.clone();
        self.start(model, ctx, "Saving facts…");
        std::thread::spawn(move || {
            let outcome = match ws.lock() {
                Ok(mut ws) => {
                    let mut added = 0usize;
                    let mut failure: Option<String> = None;
                    for line in text.lines() {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        match ws.add_fact(line, FactOrigin::UserAttested) {
                            Ok(_) => added += 1,
                            Err(e) => failure = Some(e.to_string()),
                        }
                    }
                    Self::publish(&ws, &tx, &selection, &constraints, &terms);
                    match failure {
                        Some(e) => Update::Failed(e),
                        None => Update::Notice(format!("Saved {added} fact(s).")),
                    }
                }
                Err(_) => Update::Failed("workspace lock poisoned".into()),
            };
            let _ = tx.send(outcome);
        });
    }

    fn spawn_correct_fact(
        &mut self,
        claim_id: String,
        new_text: String,
        model: &mut ShellViewModel,
        ctx: &egui::Context,
    ) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        let selection = self.selection.clone();
        self.start(model, ctx, "Propagating the correction…");
        std::thread::spawn(move || {
            let outcome = match ws.lock() {
                Ok(mut ws) => match ws.correct_fact(&claim_id, &new_text, now_unix()) {
                    Ok(report) => {
                        Self::publish(&ws, &tx, &selection, &constraints, &terms);
                        Update::Notice(format!(
                            "Fact corrected (revision {}). {} packet(s) became stale, {} approval(s) voided, {} role(s) reopened, {} submitted application(s) left untouched.",
                            report.new_profile_revision,
                            report.invalidated_packets.len(),
                            report.expired_grants.len(),
                            report.reopened_to_prepared.len(),
                            report.submitted_untouched.len()
                        ))
                    }
                    Err(e) => Update::Failed(e.to_string()),
                },
                Err(_) => Update::Failed("workspace lock poisoned".into()),
            };
            let _ = tx.send(outcome);
        });
    }

    fn spawn_job_action(
        &mut self,
        identity: String,
        action: JobAction,
        model: &mut ShellViewModel,
        ctx: &egui::Context,
    ) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        let data_dir = self.data_dir.clone();
        let selection = Some(identity.clone());
        self.selection = selection.clone();
        let label = match action {
            JobAction::Shortlist => "Shortlisting…",
            JobAction::Prepare => "Preparing the packet…",
            JobAction::Approve => "Recording your approval…",
            JobAction::Export => "Exporting DOCX…",
        };
        self.start(model, ctx, label);
        std::thread::spawn(move || {
            let outcome = match ws.lock() {
                Ok(mut ws) => {
                    let id = match JobIdentityId::new(identity.clone()) {
                        Ok(id) => id,
                        Err(e) => {
                            let _ = tx.send(Update::Failed(e.to_string()));
                            return;
                        }
                    };
                    let result: Result<String, String> = match action {
                        JobAction::Shortlist => ws
                            .set_state(&id, ApplicationState::Shortlisted)
                            .map(|_| "Shortlisted. Next: prepare the packet.".to_string())
                            .map_err(|e| e.to_string()),
                        JobAction::Prepare => ws
                            .prepare_packet(&id, now_unix())
                            .map(|p| {
                                if p.is_approvable() {
                                    format!(
                                        "Packet {} prepared from {} attested fact(s). You can approve it now.",
                                        &p.packet_sha256[..12],
                                        p.supported_spans
                                    )
                                } else {
                                    format!(
                                        "Packet {} prepared, but approval is blocked: {}.",
                                        &p.packet_sha256[..12],
                                        if p.unsupported.is_empty() {
                                            "it cites no attested facts".to_string()
                                        } else {
                                            p.unsupported.join("; ")
                                        }
                                    )
                                }
                            })
                            .map_err(|e| e.to_string()),
                        JobAction::Approve => {
                            let job = ws.job(&id).ok().flatten();
                            let source = job.as_ref().map(|j| j.source.clone()).unwrap_or_default();
                            let origin = job
                                .as_ref()
                                .and_then(|j| j.canonical_url.as_deref())
                                .and_then(origin_of)
                                .unwrap_or_else(|| "https://unknown.invalid".to_string());
                            let schema = form_schema_digest(&source);
                            ws.approve_packet(&id, &origin, &schema, now_unix())
                                .map(|g| {
                                    format!(
                                        "Approval {} recorded (one use, bound to packet and destination). Nothing has been sent.",
                                        g.id
                                    )
                                })
                                .map_err(|e| e.to_string())
                        }
                        JobAction::Export => {
                            let file = export_path(&data_dir, &identity);
                            ws.export_packet(&id, &file)
                                .map(|bytes| format!("Wrote {} bytes to {}", bytes, file.display()))
                                .map_err(|e| e.to_string())
                        }
                    };
                    Self::publish(&ws, &tx, &selection, &constraints, &terms);
                    match result {
                        Ok(message) => Update::Notice(message),
                        Err(message) => Update::Failed(message),
                    }
                }
                Err(_) => Update::Failed("workspace lock poisoned".into()),
            };
            let _ = tx.send(outcome);
        });
    }
}

#[derive(Clone, Copy)]
enum JobAction {
    Shortlist,
    Prepare,
    Approve,
    Export,
}

impl Controller for AppController {
    fn poll(&mut self, _ctx: &egui::Context, model: &mut ShellViewModel) {
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
                Update::Facts(facts) => {
                    model.facts = waypoint_desktop::facts_from(&facts);
                    model.fact_edits = model.facts.iter().map(|f| f.text.clone()).collect();
                }
                Update::Detail(detail) => {
                    model.selected_row = model
                        .rows
                        .iter()
                        .position(|r| r.identity == detail.identity);
                    model.selection = Some(*detail);
                }
                Update::Model(status) => model.model_status = Some(status),
                Update::Notice(message) => model.last_action = Some(message),
                Update::Failed(message) => model.last_action = Some(format!("Failed: {message}")),
            }
        }
    }

    fn handle(&mut self, command: ShellCommand, ctx: &egui::Context, model: &mut ShellViewModel) {
        // One worker at a time: bounded by construction (MASTER_PLAN §13).
        if model.busy.is_some() {
            return;
        }
        match command {
            ShellCommand::RunDiscovery => self.spawn_discovery(model, ctx),
            ShellCommand::ApplyFilters => self.spawn_ranking(model, ctx),
            ShellCommand::ProbeModel => self.spawn_probe(model, ctx),
            ShellCommand::SelectJob(identity) => self.spawn_select(identity, model, ctx),
            ShellCommand::SaveFacts(text) => self.spawn_save_facts(text, model, ctx),
            ShellCommand::CorrectFact { claim_id, new_text } => {
                self.spawn_correct_fact(claim_id, new_text, model, ctx)
            }
            ShellCommand::Shortlist(identity) => {
                self.spawn_job_action(identity, JobAction::Shortlist, model, ctx)
            }
            ShellCommand::PreparePacket(identity) => {
                self.spawn_job_action(identity, JobAction::Prepare, model, ctx)
            }
            ShellCommand::ApprovePacket(identity) => {
                self.spawn_job_action(identity, JobAction::Approve, model, ctx)
            }
            ShellCommand::ExportPacket(identity) => {
                self.spawn_job_action(identity, JobAction::Export, model, ctx)
            }
        }
    }
}

/// Assemble the detail view for one tracked job (plain data only).
fn build_detail(ws: &Workspace, identity: &str) -> Option<JobDetail> {
    let id = JobIdentityId::new(identity.to_string()).ok()?;
    let job = ws.job(&id).ok().flatten()?;
    let packet = ws.packet(&id).ok().flatten();
    let readiness = ws
        .packet_readiness(&id)
        .ok()
        .flatten()
        .map(|r| (r.supported_spans, r.unsupported));
    let grant = ws.grant(&id).ok().flatten();
    Some(detail_from(
        &job,
        packet.as_ref(),
        readiness,
        grant.as_ref(),
    ))
}

/// The adapter's declared form-schema identity. A real submission re-binds to
/// the live form schema, so a mismatch makes this approval unusable rather than
/// silently reusable — the failure mode is refusal, not over-permission.
fn form_schema_digest(source: &str) -> Sha256Digest {
    Sha256Digest::of(&format!("waypoint-form-schema:{source}:v1"))
}

/// `https://host/path` -> `https://host`
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}"))
}

/// Exports live with the rest of the user's data, never in the repository.
fn export_path(data_dir: &std::path::Path, identity: &str) -> PathBuf {
    let safe: String = identity
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    data_dir.join("exports").join(format!("{safe}.docx"))
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

    let dir = data_dir();
    let controller: Option<Box<dyn Controller>> = match Workspace::open(&dir) {
        Ok(ws) => {
            model.workspace_attached = true;
            let controller = AppController::new(ws, dir);
            controller.spawn_initial_load(&mut model);
            Some(Box::new(controller))
        }
        Err(e) => {
            model.last_action = Some(format!(
                "Could not open the local store: {e}. Running read-only."
            ));
            None
        }
    };

    waypoint_desktop::run(model, controller)
}

impl AppController {
    /// Load what is already stored (jobs, facts, and the open detail), on a
    /// worker, so startup never waits on I/O.
    fn spawn_initial_load(&self, model: &mut ShellViewModel) {
        let ws = Arc::clone(&self.workspace);
        let tx = self.tx.clone();
        let constraints = Self::constraints(model);
        let terms = model.query_terms();
        let selection = self.selection.clone();
        std::thread::spawn(move || match ws.lock() {
            Ok(ws) => Self::publish(&ws, &tx, &selection, &constraints, &terms),
            Err(_) => {
                let _ = tx.send(Update::Failed("workspace lock poisoned".into()));
            }
        });
    }
}
