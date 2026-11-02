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
    skipped: usize,
    history: Vec<Value>,
    listing: Option<(String, String)>,
}
enum Message {
    Board(DiscoveryTarget, String, Result<String, FetchFailure>),
    Skipped(DiscoveryTarget),
    Done,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

pub fn pending() -> Result<Value, String> {
    let tasks = TASK.lock().map_err(|_| "Discovery task unavailable")?;
    Ok(match tasks.as_ref() {
        Some(task) => json!({"active":true,"task_id":task.id,"history":task.history}),
        None => {
            let completed = COMPLETED.lock().map_err(|_| "Discovery task unavailable")?;
            match completed.as_ref() {
                Some((id, result)) => json!({"active":false,"last":{"task_id":id,"result":result}}),
                None => json!({"active":false}),
            }
        }
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
pub fn start(
    id: String,
    targets: Vec<DiscoveryTarget>,
    ws: &mut Workspace,
    listing: Option<(String, String)>,
) -> Result<(), String> {
    start_with_fetcher(id, targets, Arc::new(BoundedFetcher), Some(ws), listing)
}
fn start_with_fetcher(
    id: String,
    targets: Vec<DiscoveryTarget>,
    fetcher: Arc<dyn Fetcher>,
    workspace: Option<&mut Workspace>,
    listing: Option<(String, String)>,
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
        skipped: 0,
        history: Vec::new(),
        listing,
    });
    drop(task);
    std::thread::spawn(move || {
        let registry = waypoint_workspace::default_registry();
        let mut limited_sources = std::collections::HashSet::new();
        for target in targets {
            if worker_cancel.load(Ordering::Acquire) {
                break;
            }
            if limited_sources.contains(&target.source) {
                if !send_bounded(&tx, Message::Skipped(target), &worker_cancel) {
                    return;
                }
                continue;
            }
            match waypoint_connectors::discovery_plan(&registry, &target.source, &target.board) {
                Ok(plan) => {
                    let result = fetcher.get(&plan.url);
                    let rate_limited = matches!(&result, Err(FetchFailure::Network(message)) if message == "http 429");
                    if rate_limited {
                        limited_sources.insert(target.source.clone());
                    }
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
fn listing_response_status(
    target: &DiscoveryTarget,
    result: &Result<String, FetchFailure>,
    checked_url: &str,
) -> &'static str {
    let Ok(body) = result else {
        return "unavailable";
    };
    let parsed = match target.source.as_str() {
        "greenhouse" => waypoint_connectors::parse_greenhouse(&target.board, body),
        "lever" => waypoint_connectors::parse_lever(&target.board, body),
        "ashby" => waypoint_connectors::parse_ashby(&target.board, body),
        _ => return "unavailable",
    };
    match parsed {
        Ok(jobs) if jobs.iter().any(|job| job.source_url == checked_url) => "found",
        Ok(_) => "not_seen",
        Err(_) => "unavailable",
    }
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
            let rate_limited =
                matches!(&result, Err(FetchFailure::Network(message)) if message == "http 429");
            let listing_check = task.listing.as_ref().map(|(job_id, checked_url)| {
                let still_current = waypoint_domain::JobIdentityId::new(job_id)
                    .ok()
                    .and_then(|id| ws.job(&id).ok().flatten())
                    .is_some_and(|job| {
                        job.source == target.source
                            && job.canonical_url.as_deref() == Some(checked_url)
                    });
                let status = if !still_current {
                    "changed"
                } else {
                    listing_response_status(&target, &result, checked_url)
                };
                json!({"job_id":job_id,"status":status})
            });
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
            let summary = if rate_limited {
                format!("{} Provider returned HTTP 429; further boards from this provider will be skipped.", outcome.summary())
            } else {
                outcome.summary()
            };
            let report = json!({"source":target.source,"board":target.board,"summary":summary,"coverage":outcome.coverage,"has_more":more,"listing_check":listing_check});
            task.history.push(report.clone());
            Ok(
                json!({"status":"running","source":report["source"],"board":report["board"],"processed":task.processed,"skipped":task.skipped,"total":task.total,"summary":report["summary"],"coverage":report["coverage"],"has_more":more,"history":task.history}),
            )
        }
        Ok(Message::Skipped(target)) => {
            task.processed += 1;
            task.skipped += 1;
            task.history.push(json!({"source":target.source,"board":target.board,"summary":"Not attempted: this provider returned HTTP 429 earlier in this run.","skipped":"rate_limited"}));
            Ok(
                json!({"status":"running","processed":task.processed,"skipped":task.skipped,"total":task.total,"history":task.history}),
            )
        }
        Ok(Message::Done) => {
            let processed = task.processed;
            let total = task.total;
            let cancelled = task.cancel.load(Ordering::Acquire);
            let result = json!({"status":"complete","processed":processed,"skipped":task.skipped,"total":total,"cancelled":cancelled,"history":task.history});
            *COMPLETED.lock().map_err(|_| "Discovery task unavailable")? =
                Some((id.to_owned(), result.clone()));
            *tasks = None;
            Ok(result)
        }
        Err(TryRecvError::Empty) => Ok(
            json!({"status":"running","processed":task.processed,"skipped":task.skipped,"total":task.total,"history":task.history}),
        ),
        Err(TryRecvError::Disconnected) => {
            let cancelled = task.cancel.load(Ordering::Acquire);
            let processed = task.processed;
            let total = task.total;
            let result = if cancelled {
                json!({"status":"complete","processed":processed,"skipped":task.skipped,"total":total,"cancelled":true,"history":task.history})
            } else {
                json!({"status":"failed","processed":processed,"skipped":task.skipped,"total":total,"history":task.history,"error":"Discovery worker stopped before completion"})
            };
            *COMPLETED.lock().map_err(|_| "Discovery task unavailable")? =
                Some((id.to_owned(), result.clone()));
            *tasks = None;
            Ok(result)
        }
    }
}

/// Drain at most one result on the workspace-owning thread even when the UI
/// is closed. Completed results remain available through `poll` and `pending`.
pub fn collect_background(ws: &mut Workspace) -> Result<(), String> {
    let id = TASK
        .lock()
        .map_err(|_| "Discovery task unavailable")?
        .as_ref()
        .map(|task| task.id.clone());
    if let Some(id) = id {
        poll(&id, ws)?;
    }
    Ok(())
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
    fn rate_limit_skips_same_source_and_continues_other_providers() {
        use std::sync::atomic::AtomicUsize;
        struct RateLimited(Arc<AtomicUsize>);
        impl Fetcher for RateLimited {
            fn get(&self, url: &str) -> Result<String, FetchFailure> {
                self.0.fetch_add(1, Ordering::Relaxed);
                if url.contains("greenhouse") {
                    Err(FetchFailure::Network("http 429".into()))
                } else {
                    Ok("[]".into())
                }
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        start_with_fetcher(
            "rate-limit".into(),
            vec![
                DiscoveryTarget::new("greenhouse", "one"),
                DiscoveryTarget::new("greenhouse", "two"),
                DiscoveryTarget::new("lever", "three"),
            ],
            Arc::new(RateLimited(calls.clone())),
            None,
            None,
        )
        .unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = poll("rate-limit", &mut ws).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["processed"], 3);
                assert_eq!(result["skipped"], 1);
                assert_eq!(result["total"], 3);
                assert_eq!(result["history"][1]["skipped"], "rate_limited");
                assert_eq!(result["history"][2]["source"], "lever");
                assert_eq!(calls.load(Ordering::Relaxed), 2);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn listing_response_checks_exact_url_without_guessing_closure() {
        let target = DiscoveryTarget::new("greenhouse", "fixture");
        let body = Ok(r#"[{"id":1,"title":"Engineer","location":{"name":"Berlin"},"absolute_url":"https://example.org/job"}]"#.to_string());
        assert_eq!(
            listing_response_status(&target, &body, "https://example.org/job"),
            "found"
        );
        assert_eq!(
            listing_response_status(&target, &body, "https://example.org/other"),
            "not_seen"
        );
        assert_eq!(
            listing_response_status(&target, &Ok("not json".into()), "https://example.org/job"),
            "unavailable"
        );
        assert_eq!(
            listing_response_status(
                &target,
                &Err(FetchFailure::Network("fixture".into())),
                "https://example.org/job"
            ),
            "unavailable"
        );
    }

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
        let saved = ws.jobs().unwrap().into_iter().next().unwrap();
        let listing_id = saved.id.as_str().to_owned();
        let listing_url = saved.canonical_url.unwrap();
        let target = DiscoveryTarget::new("greenhouse", "fixture");
        let fetcher = StaticFetcher::default().route(&plan.url, r#"[{"id":123,"title":"Engineer","location":{"name":"Remote"},"absolute_url":"https://example.com/job"}]"#);
        start_with_fetcher(
            "listing-check".into(),
            vec![target],
            Arc::new(fetcher),
            None,
            Some((listing_id.clone(), listing_url)),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = poll("listing-check", &mut ws).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["history"][0]["listing_check"]["job_id"], listing_id);
                assert_eq!(result["history"][0]["listing_check"]["status"], "found");
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let background_target = DiscoveryTarget::new("greenhouse", "background");
        let background_plan = waypoint_connectors::discovery_plan(
            &waypoint_workspace::default_registry(),
            "greenhouse",
            "background",
        )
        .unwrap();
        let background_fetcher = StaticFetcher::default().route(&background_plan.url,
            r#"[{"id":124,"title":"Scientist","location":{"name":"Remote"},"absolute_url":"https://example.com/other-job"}]"#);
        start_with_fetcher(
            "background".into(),
            vec![background_target],
            Arc::new(background_fetcher),
            None,
            None,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            collect_background(&mut ws).unwrap();
            if pending().unwrap()["active"] == false {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(ws.job_count().unwrap(), 2);
        assert_eq!(pending().unwrap()["last"]["task_id"], "background");
        assert_eq!(poll("background", &mut ws).unwrap()["status"], "complete");
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
            None,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = poll("jobicy-fixture", &mut ws).unwrap();
            if result.get("summary").is_some() {
                assert_eq!(result["has_more"], true);
                assert_eq!(ws.job_count().unwrap(), 3);
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
            Some(&mut ws),
            None
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
            skipped: 0,
            history: vec![json!({"source":"greenhouse","board":"fixture","summary":"1 saved"})],
            listing: None,
        });
        let failed = poll("interrupted", &mut ws).unwrap();
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["history"].as_array().unwrap().len(), 1);
        assert_eq!(poll("interrupted", &mut ws).unwrap(), failed);
    }
}
