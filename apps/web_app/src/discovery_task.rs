//! Bounded sequential public-board discovery with incremental persistence.
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use waypoint_workspace::{DiscoveryTarget, FetchFailure, Fetcher, StaticFetcher, Workspace};

const MAX_TARGETS: usize = 10;
struct Task {
    id: String,
    rx: Receiver<Message>,
    cancel: Arc<AtomicBool>,
    total: usize,
    processed: usize,
    history: Vec<Value>,
}
enum Message {
    Board(DiscoveryTarget, String, Result<String, FetchFailure>),
    Done,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

pub fn pending() -> Result<Value, String> {
    let tasks = TASK.lock().map_err(|_| "Discovery task unavailable")?;
    Ok(match tasks.as_ref() {
        Some(task) => json!({"active":true,"task_id":task.id,"history":task.history}),
        None => json!({"active":false}),
    })
}
struct BoundedFetcher;
impl Fetcher for BoundedFetcher {
    fn get(&self, url: &str) -> Result<String, FetchFailure> {
        match waypoint_net::get_capped(url, 15_000, 4 * 1024 * 1024) {
            Ok(r) if (200..300).contains(&r.status) => Ok(r.text()),
            Ok(r) if matches!(r.status, 401 | 403 | 404) => {
                Err(FetchFailure::AccessDenied(format!("http {}", r.status)))
            }
            Ok(r) => Err(FetchFailure::Network(format!("http {}", r.status))),
            Err(e) => Err(FetchFailure::Network(e.to_string())),
        }
    }
}
fn send_bounded(sender: &SyncSender<Message>, mut message: Message, cancel: &AtomicBool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match sender.try_send(message) {
            Ok(()) => return true,
            Err(TrySendError::Full(m)) => {
                message = m;
                if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}
pub fn start(id: String, targets: Vec<DiscoveryTarget>, ws: &mut Workspace) -> Result<(), String> {
    start_with_fetcher(id, targets, Arc::new(BoundedFetcher), Some(ws))
}
fn start_with_fetcher(
    id: String,
    targets: Vec<DiscoveryTarget>,
    fetcher: Arc<dyn Fetcher>,
    workspace: Option<&mut Workspace>,
) -> Result<(), String> {
    if targets.is_empty() || targets.len() > MAX_TARGETS {
        return Err("Choose 1 to 10 job boards".into());
    }
    if targets.iter().filter(|t| t.source == "jobicy").count() > 1 {
        return Err("Only one Jobicy search is allowed per run".into());
    }
    for t in &targets {
        let valid = match t.source.as_str() {
            "jobicy" => {
                t.board.trim().chars().count() >= 3
                    && t.board.chars().count() <= 50
                    && t.board.len() <= 200
                    && !t.board.chars().any(char::is_control)
            }
            "greenhouse" | "lever" | "ashby" => {
                !t.board.is_empty()
                    && t.board.len() <= 100
                    && t.board
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            }
            _ => false,
        };
        if !valid {
            return Err("Invalid source or search term".into());
        }
    }
    let mut task = TASK.lock().map_err(|_| "Discovery task unavailable")?;
    if task.is_some() {
        return Err("A discovery run is already active or has uncollected results".into());
    }
    if targets.iter().any(|t| t.source == "jobicy") {
        let ws = workspace.ok_or("Jobicy rate limit storage unavailable")?;
        if !ws
            .reserve_jobicy_fetch(crate::now())
            .map_err(|e| e.to_string())?
        {
            return Err(
                "Jobicy search is limited to once per hour; existing results remain available"
                    .into(),
            );
        }
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let total = targets.len();
    *task = Some(Task {
        id,
        rx,
        cancel,
        total,
        processed: 0,
        history: Vec::new(),
    });
    drop(task);
    std::thread::spawn(move || {
        let registry = waypoint_workspace::default_registry();
        for target in targets {
            if worker_cancel.load(Ordering::Acquire) {
                break;
            }
            match waypoint_connectors::discovery_plan(&registry, &target.source, &target.board) {
                Ok(plan) => {
                    let result = fetcher.get(&plan.url);
                    if !send_bounded(
                        &tx,
                        Message::Board(target, plan.url, result),
                        &worker_cancel,
                    ) {
                        return;
                    }
                }
                Err(e) => {
                    let result = Err(FetchFailure::Network(e.to_string()));
                    if !send_bounded(
                        &tx,
                        Message::Board(target, String::new(), result),
                        &worker_cancel,
                    ) {
                        return;
                    }
                }
            }
        }
        let _ = send_bounded(&tx, Message::Done, &AtomicBool::new(false));
    });
    Ok(())
}
pub fn poll(id: &str, ws: &mut Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Discovery task unavailable")?;
    let Some(task) = tasks.as_mut().filter(|t| t.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Discovery task unavailable")?
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone())
            .ok_or_else(|| "Discovery run not found".into());
    };
    match task.rx.try_recv() {
        Ok(Message::Board(target, url, result)) => {
            let more = if target.source == "jobicy" {
                result
                    .as_ref()
                    .ok()
                    .and_then(|body| serde_json::from_str::<Value>(body).ok())
                    .and_then(|data| data.get("hasMore").and_then(Value::as_bool))
            } else {
                None
            };
            let fetcher = match result {
                Ok(body) => StaticFetcher::default().route(&url, &body),
                Err(error) => StaticFetcher::default().failing(&url, error),
            };
            let outcome = ws.discover(std::slice::from_ref(&target), &fetcher, crate::now());
            task.processed += 1;
            let report = json!({"source":target.source,"board":target.board,"summary":outcome.summary(),"coverage":outcome.coverage,"has_more":more});
            task.history.push(report.clone());
            Ok(
                json!({"status":"running","source":report["source"],"board":report["board"],"processed":task.processed,"total":task.total,"summary":report["summary"],"coverage":report["coverage"],"has_more":more,"history":task.history}),
            )
        }
        Ok(Message::Done) => {
            let processed = task.processed;
            let total = task.total;
            let cancelled = task.cancel.load(Ordering::Acquire);
            let result = json!({"status":"complete","processed":processed,"total":total,"cancelled":cancelled,"history":task.history});
            *COMPLETED.lock().map_err(|_| "Discovery task unavailable")? =
                Some((id.to_owned(), result.clone()));
            *tasks = None;
            Ok(result)
        }
        Err(TryRecvError::Empty) => Ok(
            json!({"status":"running","processed":task.processed,"total":task.total,"history":task.history}),
        ),
        Err(TryRecvError::Disconnected) => {
            let cancelled = task.cancel.load(Ordering::Acquire);
            let processed = task.processed;
            let total = task.total;
            let result = if cancelled {
                json!({"status":"complete","processed":processed,"total":total,"cancelled":true,"history":task.history})
            } else {
                json!({"status":"failed","processed":processed,"total":total,"history":task.history,"error":"Discovery worker stopped before completion"})
            };
            *COMPLETED.lock().map_err(|_| "Discovery task unavailable")? =
                Some((id.to_owned(), result.clone()));
            *tasks = None;
            Ok(result)
        }
    }
}
pub fn cancel(id: &str) -> Result<(), String> {
    let tasks = TASK.lock().map_err(|_| "Discovery task unavailable")?;
    let task = tasks
        .as_ref()
        .filter(|t| t.id == id)
        .ok_or("Discovery run not found")?;
    task.cancel.store(true, Ordering::Release);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_results_are_persisted_incrementally() {
        let target = DiscoveryTarget::new("greenhouse", "fixture");
        let plan = waypoint_connectors::discovery_plan(
            &waypoint_workspace::default_registry(),
            "greenhouse",
            "fixture",
        )
        .unwrap();
        let fetcher=StaticFetcher::default().route(&plan.url,r#"[{"id":123,"title":"Engineer","location":{"name":"Remote"},"absolute_url":"https://example.com/job"}]"#);
        let failed_target = DiscoveryTarget::new("lever", "missing");
        start_with_fetcher(
            "fixture".into(),
            vec![target, failed_target],
            Arc::new(fetcher),
            None,
        )
        .unwrap();
        assert_eq!(pending().unwrap()["task_id"], "fixture");
        let mut ws = Workspace::in_memory().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = false;
        let mut degraded = false;
        loop {
            let result = poll("fixture", &mut ws).unwrap();
            if result.get("summary").is_some() {
                seen = true;
                assert_eq!(ws.job_count().unwrap(), 1);
                if result["source"] == "lever" {
                    degraded = result["summary"].as_str().unwrap().contains("1 degraded");
                }
            }
            if result["status"] == "complete" {
                assert_eq!(result["history"].as_array().unwrap().len(), 2);
                assert_eq!(result["history"][0]["source"], "greenhouse");
                assert_eq!(result["history"][1]["source"], "lever");
                assert_eq!(poll("fixture", &mut ws).unwrap(), result);
                assert_eq!(pending().unwrap()["active"], false);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen);
        assert!(degraded);
        let jobicy = DiscoveryTarget::new("jobicy", "materials engineer");
        let plan = waypoint_connectors::discovery_plan(
            &waypoint_workspace::default_registry(),
            "jobicy",
            "materials engineer",
        )
        .unwrap();
        let fetcher=StaticFetcher::default().route(&plan.url,r#"{"success":true,"hasMore":true,"jobs":[{"id":44,"url":"https://jobicy.com/jobs/44-example","jobTitle":"Materials engineer","companyName":"Fixture","jobGeo":"Europe","jobExcerpt":"Research"}]}"#);
        start_with_fetcher(
            "jobicy-fixture".into(),
            vec![jobicy.clone()],
            Arc::new(fetcher.clone()),
            Some(&mut ws),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = poll("jobicy-fixture", &mut ws).unwrap();
            if result.get("summary").is_some() {
                assert_eq!(result["has_more"], true);
                assert_eq!(ws.job_count().unwrap(), 2);
                let job = ws
                    .jobs()
                    .unwrap()
                    .into_iter()
                    .find(|j| j.source == "jobicy")
                    .unwrap();
                assert_eq!(
                    job.canonical_url.as_deref(),
                    Some("https://jobicy.com/jobs/44-example")
                );
            }
            if result["status"] == "complete" {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(start_with_fetcher(
            "again".into(),
            vec![jobicy],
            Arc::new(fetcher),
            Some(&mut ws)
        )
        .is_err());
        let (sender, rx) = mpsc::sync_channel(1);
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "interrupted".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 2,
            processed: 1,
            history: vec![json!({"source":"greenhouse","board":"fixture","summary":"1 saved"})],
        });
        let failed = poll("interrupted", &mut ws).unwrap();
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["history"].as_array().unwrap().len(), 1);
        assert_eq!(poll("interrupted", &mut ws).unwrap(), failed);
    }
}
