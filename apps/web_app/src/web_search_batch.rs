//! Sequential, user-approved web-search phrases. Results are unverified candidates.
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
struct Task {
    id: String,
    rx: Receiver<Message>,
    cancel: Arc<AtomicBool>,
    total: usize,
    plan: Vec<(String, u8)>,
    provider: Option<String>,
    processed: usize,
    saved_processed: Option<usize>,
    checkpoint_failed: bool,
    pending: Option<Value>,
    history: Vec<Value>,
    timed_out: Arc<AtomicBool>,
    unreported: Arc<AtomicBool>,
}
enum Message {
    Query(String, u8, Result<Value, String>),
    Done(bool),
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value, bool)>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());
pub fn busy() -> bool {
    TASK.lock().map(|t| t.is_some()).unwrap_or(true)
}
pub fn pending() -> Result<Value, String> {
    let tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    Ok(match tasks.as_ref() {
        Some(task) => json!({"active":true,"task_id":task.id,"history":task.history}),
        None => {
            let completed = COMPLETED.lock().map_err(|_| "Web search unavailable")?;
            match completed.as_ref() {
                Some((id, result, _)) => {
                    json!({"active":false,"last":{"task_id":id,"result":result}})
                }
                None => json!({"active":false}),
            }
        }
    })
}
fn send(
    tx: &SyncSender<Message>,
    mut message: Message,
    cancel: &AtomicBool,
    timed_out: &AtomicBool,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match tx.try_send(message) {
            Ok(()) => return true,
            Err(TrySendError::Full(m)) => {
                message = m;
                if cancel.load(Ordering::Acquire) {
                    return false;
                }
                if Instant::now() >= deadline {
                    timed_out.store(true, Ordering::Release);
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

fn send_completion(
    tx: &SyncSender<Message>,
    limited: bool,
    cancel: &AtomicBool,
    timed_out: &AtomicBool,
    timeout: Duration,
) -> bool {
    send(tx, Message::Done(limited), cancel, timed_out, timeout)
}

/// Count requested provider pages, not jobs or distinct websites. Provider
/// engine availability and provider-side truncation are reported separately.
fn coverage(total: usize, history: &[Value]) -> Value {
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut limited = 0usize;
    let mut engines_unavailable = 0usize;
    let mut availability_unknown = 0usize;
    for page in history {
        if page.get("error").is_some() {
            failed += 1;
        } else if let Some(data) = page.get("data") {
            succeeded += 1;
            if data["limited"] == true {
                limited += 1;
            }
            match data["unresponsive_engines"].as_u64() {
                Some(0) => {}
                Some(_) => engines_unavailable += 1,
                None => availability_unknown += 1,
            }
        }
    }
    json!({"requested_pages":total,"attempted_pages":history.len(),"succeeded_pages":succeeded,"failed_pages":failed,"unattempted_pages":total.saturating_sub(history.len()),"limited_pages":limited,"pages_with_unresponsive_engines":engines_unavailable,"pages_with_unknown_engine_availability":availability_unknown})
}

/// Keep the global result cap visible when the final provider page has more
/// valid links than the remaining budget. Previously the page could appear
/// complete even though some of its parsed links were discarded here.
fn retain_batch_budget(data: &mut Value, seen: &mut HashSet<String>) -> bool {
    let mut omitted = 0usize;
    if let Some(items) = data["results"].as_array_mut() {
        items.retain(|item| {
            let Some(url) = item["url"].as_str() else {
                return false;
            };
            if seen.contains(url) {
                return false;
            }
            if seen.len() >= 600 {
                omitted += 1;
                return false;
            }
            seen.insert(url.to_owned())
        });
    }
    if omitted > 0 {
        data["limited"] = json!(true);
        data["batch_limit_omitted"] = json!(omitted);
    }
    seen.len() >= 600
}

pub fn start(id: String, provider: String, queries: Vec<String>, pages: u8) -> Result<(), String> {
    if crate::web_search::busy() {
        return Err("A web search is active".into());
    }
    if queries.is_empty() || queries.len() > 10 || !(1..=5).contains(&pages) {
        return Err("Enter 1 to 10 search phrases and 1 to 5 pages per phrase".into());
    }
    let mut seen = HashSet::new();
    let mut phrases = Vec::new();
    for query in queries {
        let query = query.trim().to_owned();
        if !seen.insert(query.to_lowercase()) {
            return Err("Search phrases must be distinct".into());
        }
        let url = crate::web_search::request_url(&provider, &query)?;
        phrases.push((query, url));
    }
    // Cover every phrase's first page before spending the shared candidate
    // budget on deeper pages of any one phrase.
    let mut requests = Vec::new();
    for page in 1..=pages {
        for (query, url) in &phrases {
            let mut page_url = url.clone();
            page_url
                .query_pairs_mut()
                .append_pair(crate::web_search::page_parameter(url), &page.to_string());
            requests.push((query.clone(), page, page_url));
        }
    }
    let mut task = TASK.lock().map_err(|_| "Web search unavailable")?;
    if task.is_some() {
        return Err("A batch search is running or awaiting its result".into());
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let timed_out = Arc::new(AtomicBool::new(false));
    let unreported = Arc::new(AtomicBool::new(false));
    let worker_timeout = timed_out.clone();
    let worker_unreported = unreported.clone();
    let total = requests.len();
    let plan = requests
        .iter()
        .map(|(query, page, _)| (query.clone(), *page))
        .collect();
    *task = Some(Task {
        id,
        rx,
        cancel,
        total,
        plan,
        provider: (provider.trim().len() <= 2048).then(|| provider.trim().to_owned()),
        processed: 0,
        saved_processed: None,
        checkpoint_failed: false,
        pending: None,
        history: Vec::new(),
        timed_out,
        unreported,
    });
    drop(task);
    std::thread::spawn(move || {
        let mut seen = HashSet::new();
        let mut limit_reached = false;
        for (index, (query, page, url)) in requests.into_iter().enumerate() {
            if stop.load(Ordering::Acquire) {
                break;
            }
            if index > 0 {
                for _ in 0..10 {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            if stop.load(Ordering::Acquire) {
                break;
            }
            let (result, provider_stop) =
                std::panic::catch_unwind(|| match crate::web_search::fetch(&url) {
                    Ok(body) => (crate::web_search::parse_provider(&body, &url, &seen), false),
                    Err(error) => (Err(error.message), error.stop_batch),
                })
                .unwrap_or_else(|_| (Err("Search stopped unexpectedly".into()), false));
            let result = result.map(|mut data| {
                limit_reached = retain_batch_budget(&mut data, &mut seen);
                data
            });
            if !send(
                &tx,
                Message::Query(query, page, result),
                &stop,
                &worker_timeout,
                Duration::from_secs(30),
            ) {
                worker_unreported.store(true, Ordering::Release);
                return;
            }
            if limit_reached || provider_stop {
                break;
            }
        }
        // A completed page may still occupy the one-slot channel when the
        // user cancels. Do not keep a worker waiting indefinitely for a
        // client that may never poll; disconnection preserves cancellation
        // and the buffered page is still available to the receiver.
        let _ = send_completion(
            &tx,
            limit_reached,
            &stop,
            &worker_timeout,
            Duration::from_secs(30),
        );
    });
    Ok(())
}
pub fn poll(id: &str, acknowledged: usize) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    let Some(task) = tasks.as_mut().filter(|t| t.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Web search unavailable")?
            .as_ref()
            .filter(|(task_id, _, _)| task_id == id)
            .map(|(_, result, _)| result.clone())
            .ok_or_else(|| "Batch web search not found".into());
    };
    if acknowledged > task.processed {
        return Err("Invalid batch acknowledgement".into());
    }
    if acknowledged
        < task
            .processed
            .saturating_sub(usize::from(task.pending.is_some()))
    {
        return Ok(
            json!({"status":"running","processed":task.processed,"total":task.total,"history":task.history}),
        );
    }
    if let Some(pending) = &task.pending {
        if acknowledged < task.processed {
            return Ok(pending.clone());
        }
        task.pending = None;
    }
    match task.rx.try_recv() {
        Ok(Message::Query(query, page, result)) => {
            task.processed += 1;
            let response = match result {
                Ok(data) => {
                    json!({"status":"running","processed":task.processed,"total":task.total,"query":query,"page":page,"data":data})
                }
                Err(error) => {
                    json!({"status":"running","processed":task.processed,"total":task.total,"query":query,"page":page,"error":error})
                }
            };
            task.pending = Some(response.clone());
            task.history.push(response.clone());
            Ok(response)
        }
        Ok(Message::Done(limit_reached)) => {
            let (processed, total, cancelled) = (
                task.processed,
                task.total,
                task.cancel.load(Ordering::Acquire),
            );
            let response = json!({"status":"complete","processed":processed,"total":total,"cancelled":cancelled,"limit_reached":limit_reached,"coverage":coverage(total, &task.history),"history":task.history});
            *COMPLETED.lock().map_err(|_| "Web search unavailable")? =
                Some((id.to_owned(), response.clone(), false));
            *tasks = None;
            Ok(response)
        }
        Err(TryRecvError::Empty) => {
            Ok(json!({"status":"running","processed":task.processed,"total":task.total}))
        }
        Err(TryRecvError::Disconnected) => {
            let (processed, total, cancelled) = (
                task.processed,
                task.total,
                task.cancel.load(Ordering::Acquire),
            );
            let unreported = usize::from(task.unreported.load(Ordering::Acquire));
            let mut report = coverage(total, &task.history);
            report["fetched_unreported_pages"] = json!(unreported);
            report["attempted_pages"] = json!(task.history.len().saturating_add(unreported));
            report["unattempted_pages"] =
                json!(total.saturating_sub(task.history.len().saturating_add(unreported)));
            let response = if cancelled {
                json!({"status":"complete","processed":processed,"total":total,"cancelled":true,"coverage":report,"history":task.history})
            } else {
                let error = if task.timed_out.load(Ordering::Acquire) {
                    "Batch search stopped because results were not collected within 30 seconds"
                } else {
                    "Batch search stopped before completion"
                };
                json!({"status":"failed","processed":processed,"total":total,"error":error,"coverage":report,"history":task.history})
            };
            *COMPLETED.lock().map_err(|_| "Web search unavailable")? =
                Some((id.to_owned(), response.clone(), false));
            *tasks = None;
            Ok(response)
        }
    }
}

/// Collect one page into bounded in-memory history even if no page is open.
/// Client acknowledgements can still recover every collected page from history.
pub fn collect_background() -> Result<(), String> {
    let task = TASK
        .lock()
        .map_err(|_| "Web search unavailable")?
        .as_ref()
        .map(|task| (task.id.clone(), task.processed));
    if let Some((id, processed)) = task {
        poll(&id, processed)?;
    }
    Ok(())
}

/// Retain the latest terminal result only after sealing and storage succeed.
pub fn persist_completed(ws: &waypoint_workspace::Workspace) -> Result<bool, String> {
    // Do not overwrite a newer in-progress checkpoint with an older result.
    let tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    if tasks.is_some() {
        return Ok(false);
    }
    let mut completed = COMPLETED.lock().map_err(|_| "Web search unavailable")?;
    let Some((id, result, saved)) = completed.as_mut() else {
        return Ok(false);
    };
    if *saved {
        return Ok(true);
    }
    let mut persisted = result.clone();
    persisted["history_saved"] = json!(true);
    let bytes =
        serde_json::to_vec(&persisted).map_err(|_| "Could not serialize web search history")?;
    if let Err(error) = ws.save_latest_web_search(id, &bytes) {
        result["history_saved"] = json!(false);
        return Err(error.to_string());
    }
    *result = persisted;
    *saved = true;
    Ok(true)
}

/// A restart cannot resume the worker, so store a clearly interrupted snapshot
/// of collected pages, not a claim that the provider finished its requests.
pub fn persist_progress(ws: &waypoint_workspace::Workspace) -> Result<bool, String> {
    let mut tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    let Some(task) = tasks.as_mut() else {
        return Ok(false);
    };
    if task.saved_processed == Some(task.processed) {
        return Ok(false);
    }
    let result = json!({
        "status":"failed", "processed":task.processed, "total":task.total,
        "history":task.history, "history_saved":true, "recovered_partial":true,
        "plan":task.plan.iter().map(|(query, page)| json!({"query":query,"page":page})).collect::<Vec<_>>(),
        "provider":task.provider,
        "error":"The app stopped before this search finished; another provider request may have been in progress"
    });
    let bytes =
        serde_json::to_vec(&result).map_err(|_| "Could not serialize web search checkpoint")?;
    if let Err(error) = ws.save_latest_web_search(&task.id, &bytes) {
        task.checkpoint_failed = true;
        return Err(error.to_string());
    }
    task.checkpoint_failed = false;
    task.saved_processed = Some(task.processed);
    Ok(true)
}

pub fn checkpoint_failed(id: &str) -> Result<bool, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Web search unavailable")?
        .as_ref()
        .is_some_and(|task| task.id == id && task.checkpoint_failed))
}

pub fn pending_with_saved(ws: &waypoint_workspace::Workspace) -> Result<Value, String> {
    let mut current = pending()?;
    if current["active"] == false && current.get("last").is_none() {
        if let Some((id, bytes)) = ws.latest_web_search().map_err(|error| error.to_string())? {
            let result: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "Saved web search history is invalid")?;
            if !matches!(result["status"].as_str(), Some("complete" | "failed"))
                || !result["history"].is_array()
            {
                return Err("Saved web search history is invalid".into());
            }
            current["last"] = json!({"task_id":id,"result":result});
        }
    }
    Ok(current)
}
pub fn cancel(id: &str) -> Result<(), String> {
    let tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    let task = tasks
        .as_ref()
        .filter(|t| t.id == id)
        .ok_or("Batch web search not found")?;
    task.cancel.store(true, Ordering::Release);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Instant;
    #[cfg(windows)]
    #[test]
    fn completed_history_is_recovered_after_workspace_reopens() {
        let _lock = TEST_LOCK.lock().unwrap();
        fn open(path: &std::path::Path) -> waypoint_workspace::Workspace {
            #[cfg(feature = "sqlcipher")]
            {
                waypoint_workspace::Workspace::open_encrypted(path).unwrap()
            }
            #[cfg(not(feature = "sqlcipher"))]
            {
                waypoint_workspace::Workspace::open(path).unwrap()
            }
        }
        let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::Builder::new()
            .prefix("waypoint-web-search-")
            .tempdir_in(parent)
            .unwrap();
        let expected = json!({"status":"complete","history":[{"query":"fictional","page":1,"data":{"results":[{"url":"https://example.test/job"}]}}]});
        {
            let ws = open(dir.path());
            *COMPLETED.lock().unwrap() = Some(("fixture-restart".into(), expected, false));
            assert!(persist_completed(&ws).unwrap());
        }
        *COMPLETED.lock().unwrap() = None;
        let ws = open(dir.path());
        let recovered = pending_with_saved(&ws).unwrap();
        assert_eq!(recovered["active"], false);
        assert_eq!(recovered["last"]["task_id"], "fixture-restart");
        assert_eq!(recovered["last"]["result"]["history_saved"], true);
        assert_eq!(
            recovered["last"]["result"]["history"][0]["data"]["results"][0]["url"],
            "https://example.test/job"
        );
    }
    #[cfg(windows)]
    #[test]
    fn interrupted_batch_recovers_saved_pages_without_replacing_them_with_older_results() {
        let _lock = TEST_LOCK.lock().unwrap();
        fn open(path: &std::path::Path) -> waypoint_workspace::Workspace {
            #[cfg(feature = "sqlcipher")]
            {
                waypoint_workspace::Workspace::open_encrypted(path).unwrap()
            }
            #[cfg(not(feature = "sqlcipher"))]
            {
                waypoint_workspace::Workspace::open(path).unwrap()
            }
        }
        let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::Builder::new()
            .prefix("waypoint-web-checkpoint-")
            .tempdir_in(parent)
            .unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        *TASK.lock().unwrap() = Some(Task {
            id: "active-fixture".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 3,
            plan: vec![
                ("fictional".into(), 1),
                ("fictional".into(), 2),
                ("other".into(), 1),
            ],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        *COMPLETED.lock().unwrap() = Some((
            "older".into(),
            json!({"status":"complete","history":[]}),
            false,
        ));
        {
            let ws = open(dir.path());
            assert!(persist_progress(&ws).unwrap());
            let initial: Value =
                serde_json::from_slice(&ws.latest_web_search().unwrap().unwrap().1).unwrap();
            assert_eq!(initial["processed"], 0);
            assert_eq!(initial["provider"], "http://127.0.0.1:9876/");
            assert_eq!(initial["plan"].as_array().unwrap().len(), 3);
            tx.send(Message::Query(
                "fictional".into(),
                1,
                Ok(json!({"results":[{"url":"https://example.test/job"}]})),
            ))
            .unwrap();
            collect_background().unwrap();
            let no_sealer = waypoint_workspace::Workspace::in_memory().unwrap();
            assert!(persist_progress(&no_sealer).is_err());
            assert!(checkpoint_failed("active-fixture").unwrap());
            assert!(persist_progress(&ws).unwrap());
            assert!(!checkpoint_failed("active-fixture").unwrap());
            assert!(!persist_progress(&ws).unwrap());
            assert!(!persist_completed(&ws).unwrap());
            assert_eq!(ws.latest_web_search().unwrap().unwrap().0, "active-fixture");
        }
        *TASK.lock().unwrap() = None;
        *COMPLETED.lock().unwrap() = None;
        let ws = open(dir.path());
        let recovered = pending_with_saved(&ws).unwrap();
        let result = &recovered["last"]["result"];
        assert_eq!(recovered["last"]["task_id"], "active-fixture");
        assert_eq!(result["status"], "failed");
        assert_eq!(result["recovered_partial"], true);
        assert_eq!(result["processed"], 1);
        assert_eq!(result["total"], 3);
        assert_eq!(result["plan"][1], json!({"query":"fictional","page":2}));
        assert_eq!(
            result["history"][0]["data"]["results"][0]["url"],
            "https://example.test/job"
        );
    }
    #[test]
    fn background_collection_retains_pages_for_later_client_recovery() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        *TASK.lock().unwrap() = Some(Task {
            id: "background-fixture".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 2,
            plan: vec![],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        tx.send(Message::Query(
            "fictional".into(),
            1,
            Ok(json!({"results":[{"url":"https://example.test/one"}]})),
        ))
        .unwrap();
        collect_background().unwrap();
        tx.send(Message::Query(
            "fictional".into(),
            2,
            Err("fixture failure".into()),
        ))
        .unwrap();
        collect_background().unwrap();
        let recovered = poll("background-fixture", 0).unwrap();
        assert_eq!(recovered["history"].as_array().unwrap().len(), 2);
        assert_eq!(
            recovered["history"][0]["data"]["results"][0]["url"],
            "https://example.test/one"
        );
        assert_eq!(recovered["history"][1]["error"], "fixture failure");
        tx.send(Message::Done(false)).unwrap();
        collect_background().unwrap();
        let result = poll("background-fixture", 0).unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(result["coverage"]["succeeded_pages"], 1);
        assert_eq!(result["coverage"]["failed_pages"], 1);
        assert_eq!(pending().unwrap()["last"]["result"], result);
    }
    #[test]
    fn cancelled_batch_releases_worker_and_keeps_buffered_page() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Message::Query(
            "fictional".into(),
            1,
            Ok(json!({"results":[]})),
        ))
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        *TASK.lock().unwrap() = Some(Task {
            id: "cancel-full-channel".into(),
            rx,
            cancel: stop.clone(),
            total: 2,
            plan: vec![],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        let (finished, result) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let sent = send_completion(
                &tx,
                false,
                &stop,
                &AtomicBool::new(false),
                Duration::from_secs(30),
            );
            finished.send(sent).unwrap();
        });
        cancel("cancel-full-channel").unwrap();
        assert!(!result.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
        let page = poll("cancel-full-channel", 0).unwrap();
        assert_eq!(page["query"], "fictional");
        let done = poll("cancel-full-channel", 1).unwrap();
        assert_eq!(done["status"], "complete");
        assert_eq!(done["cancelled"], true);
        assert_eq!(done["history"][0], page);
        assert_eq!(done["coverage"]["unattempted_pages"], 1);
    }
    #[test]
    fn uncollected_results_release_worker_and_account_for_fetched_page() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Message::Query(
            "fictional".into(),
            1,
            Ok(json!({"results":[]})),
        ))
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let timeout = Arc::new(AtomicBool::new(false));
        let unreported = Arc::new(AtomicBool::new(false));
        *TASK.lock().unwrap() = Some(Task {
            id: "abandoned".into(),
            rx,
            cancel: stop.clone(),
            total: 3,
            plan: vec![],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: timeout.clone(),
            unreported: unreported.clone(),
        });
        let worker = std::thread::spawn(move || {
            if !send(
                &tx,
                Message::Query("fictional".into(), 2, Ok(json!({"results":[]}))),
                &stop,
                &timeout,
                Duration::from_millis(10),
            ) {
                unreported.store(true, Ordering::Release);
            }
        });
        worker.join().unwrap();
        let page = poll("abandoned", 0).unwrap();
        assert_eq!(page["page"], 1);
        let result = poll("abandoned", 1).unwrap();
        assert_eq!(result["status"], "failed");
        assert!(result["error"].as_str().unwrap().contains("not collected"));
        assert_eq!(result["coverage"]["attempted_pages"], 2);
        assert_eq!(result["coverage"]["fetched_unreported_pages"], 1);
        assert_eq!(result["coverage"]["unattempted_pages"], 1);
        assert_eq!(result["history"][0], page);
    }
    #[test]
    fn batch_cap_reports_links_dropped_from_a_provider_page() {
        let mut seen: HashSet<String> = (0..598)
            .map(|i| format!("https://example.org/{i}"))
            .collect();
        let mut page = json!({"results":(598..602).map(|i| json!({"url":format!("https://example.org/{i}")})).collect::<Vec<_>>(),"limited":false});
        assert!(retain_batch_budget(&mut page, &mut seen));
        assert_eq!(seen.len(), 600);
        assert_eq!(page["results"].as_array().unwrap().len(), 2);
        assert_eq!(page["batch_limit_omitted"], 2);
        assert_eq!(page["limited"], true);
        assert_eq!(coverage(1, &[json!({"data":page})])["limited_pages"], 1);
    }
    #[test]
    fn coverage_distinguishes_failed_limited_and_unattempted_pages() {
        let history = vec![
            json!({"data":{"results":[],"limited":true,"unresponsive_engines":2}}),
            json!({"error":"Provider returned HTTP 503"}),
            json!({"data":{"results":[],"limited":false,"unresponsive_engines":null}}),
        ];
        assert_eq!(
            coverage(5, &history),
            json!({
                "requested_pages":5,"attempted_pages":3,"succeeded_pages":2,
                "failed_pages":1,"unattempted_pages":2,"limited_pages":1,
                "pages_with_unresponsive_engines":1,"pages_with_unknown_engine_availability":1
            })
        );
    }

    #[test]
    fn completed_batch_reports_failed_pages_in_coverage() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(3);
        *TASK.lock().unwrap() = Some(Task {
            id: "mixed-coverage".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 3,
            plan: vec![],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        tx.send(Message::Query(
            "role".into(),
            1,
            Ok(json!({"results":[],"limited":true,"unresponsive_engines":null})),
        ))
        .unwrap();
        tx.send(Message::Query(
            "role".into(),
            2,
            Err("provider failed".into()),
        ))
        .unwrap();
        tx.send(Message::Done(true)).unwrap();
        let _ = poll("mixed-coverage", 0).unwrap();
        let _ = poll("mixed-coverage", 1).unwrap();
        let done = poll("mixed-coverage", 2).unwrap();
        assert_eq!(done["coverage"]["succeeded_pages"], 1);
        assert_eq!(done["coverage"]["failed_pages"], 1);
        assert_eq!(done["coverage"]["unattempted_pages"], 1);
        assert_eq!(done["coverage"]["limited_pages"], 1);
        assert_eq!(done["limit_reached"], true);
        assert_eq!(poll("mixed-coverage", 0).unwrap(), done);
    }

    #[test]
    fn local_provider_failure_is_counted_after_a_successful_page() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            for page in 1..=2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request).unwrap();
                let (status, body) = if page == 1 {
                    ("200 OK", r#"{"results":[],"unresponsive_engines":[]}"#)
                } else {
                    ("503 Service Unavailable", "unavailable")
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        start(
            "mixed-local-provider".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["fixture role".into()],
            2,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut acknowledged = 0;
        loop {
            let result = poll("mixed-local-provider", acknowledged).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["coverage"]["succeeded_pages"], 1);
                assert_eq!(result["coverage"]["failed_pages"], 1);
                assert_eq!(result["coverage"]["unattempted_pages"], 0);
                assert!(result["history"][1]["error"]
                    .as_str()
                    .unwrap()
                    .contains("503"));
                break;
            }
            if result.get("query").is_some() {
                acknowledged += 1;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        provider.join().unwrap();
    }

    #[test]
    fn rate_limit_stops_remaining_batch_without_retry() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = listener.try_clone().unwrap();
        let provider = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });
        start(
            "limited-provider".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["first role".into(), "second role".into()],
            2,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut acknowledged = 0;
        loop {
            let result = poll("limited-provider", acknowledged).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["coverage"]["attempted_pages"], 1);
                assert_eq!(result["coverage"]["failed_pages"], 1);
                assert_eq!(result["coverage"]["unattempted_pages"], 3);
                assert!(result["history"][0]["error"]
                    .as_str()
                    .unwrap()
                    .contains("429"));
                break;
            }
            if result.get("query").is_some() {
                acknowledged += 1;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        provider.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    #[test]
    fn results_are_replayed_until_acknowledged_and_completion_is_recoverable() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(2);
        *TASK.lock().unwrap() = Some(Task {
            id: "replay".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 1,
            plan: vec![],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        tx.send(Message::Query("role".into(), 1, Ok(json!({"results":[]}))))
            .unwrap();
        tx.send(Message::Done(false)).unwrap();
        let first = poll("replay", 0).unwrap();
        assert_eq!(first["query"], "role");
        assert_eq!(poll("replay", 0).unwrap(), first);
        let recovered = pending().unwrap();
        assert_eq!(recovered["task_id"], "replay");
        assert_eq!(recovered["history"][0], first);
        assert!(poll("replay", 2).is_err());
        let done = poll("replay", 1).unwrap();
        assert_eq!(done["status"], "complete");
        assert_eq!(done["history"][0], first);
        assert_eq!(poll("replay", 1).unwrap(), done);
        assert_eq!(pending().unwrap()["last"]["result"], done);
    }

    #[test]
    fn interrupted_batch_preserves_received_pages() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        *TASK.lock().unwrap() = Some(Task {
            id: "interrupted".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 2,
            plan: vec![],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        tx.send(Message::Query("role".into(), 1, Ok(json!({"results":[]}))))
            .unwrap();
        drop(tx);
        let page = poll("interrupted", 0).unwrap();
        let failed = poll("interrupted", 1).unwrap();
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["history"][0], page);
        assert_eq!(poll("interrupted", 0).unwrap(), failed);
        assert_eq!(pending().unwrap()["last"]["result"], failed);
    }

    #[test]
    fn http_clients_can_recover_pages_after_another_client_finishes() {
        use std::net::{TcpListener, TcpStream};
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(3);
        *TASK.lock().unwrap() = Some(Task {
            id: "http-recovery".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 2,
            plan: vec![],
            provider: Some("http://127.0.0.1:9876/".into()),
            processed: 0,
            saved_processed: None,
            checkpoint_failed: false,
            pending: None,
            history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        tx.send(Message::Query("role".into(), 1, Ok(json!({"results":[]}))))
            .unwrap();
        tx.send(Message::Query("role".into(), 2, Ok(json!({"results":[]}))))
            .unwrap();
        tx.send(Message::Done(false)).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let mut request = |path: &str, data: Value| {
            let body = data.to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        let status = "/api/web-search/batch-status";
        let first = request(status, json!({"task_id":"http-recovery","acknowledged":0}));
        let second = request(status, json!({"task_id":"http-recovery","acknowledged":1}));
        // A reloaded client now retains only the task ID. Its zero cursor
        // must recover all already-received pages before acknowledging them.
        let reloaded = request(status, json!({"task_id":"http-recovery","acknowledged":0}));
        assert_eq!(reloaded["status"], "running");
        assert_eq!(reloaded["history"], json!([first.clone(), second.clone()]));
        let done = request(status, json!({"task_id":"http-recovery","acknowledged":2}));
        assert_eq!(done["history"], json!([first, second]));
        assert_eq!(
            request(status, json!({"task_id":"http-recovery","acknowledged":0})),
            done
        );
        let recover = request("/api/web-search/batch-pending", json!({}));
        assert_eq!(recover["last"]["result"], done);
    }
    #[test]
    fn searches_sequentially_deduplicates_and_reports_each_query() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            for n in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let len = stream.read(&mut request).unwrap();
                let request = std::str::from_utf8(&request[..len]).unwrap();
                assert!(request.contains(if n == 0 { "first" } else { "second" }));
                let mut results: Vec<Value> = (0..100)
                    .map(|i| json!({"title":"Repeated", "url":format!("https://example.org/{i}"),"engine":if n == 0 {"engine-a"} else {"engine-b"}}))
                    .collect();
                if n == 1 {
                    results.push(json!({"title":"New result", "url":"https://example.org/new"}));
                }
                let body = json!({"results":results}).to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        start(
            "test-batch".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["first".into(), "second".into()],
            1,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut batches = Vec::new();
        loop {
            let result = poll("test-batch", batches.len()).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["processed"], 2);
                break;
            }
            assert_eq!(result["status"], "running");
            if result.get("data").is_some() {
                batches.push(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0]["data"]["results"].as_array().unwrap().len(), 100);
        assert_eq!(batches[1]["data"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(batches[1]["data"]["results"][0]["title"], "New result");
        assert_eq!(batches[1]["data"]["previously_seen"], 100);
        assert_eq!(
            batches[1]["data"]["repeated_references"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
        assert_eq!(
            batches[1]["data"]["repeated_references"][0]["engines"],
            json!(["engine-b"])
        );
        assert_eq!(batches[1]["data"]["repeated_references_omitted"], 90);
        assert_eq!(batches[1]["data"]["results_examined"], 101);
        provider.join().unwrap();
    }

    #[test]
    #[ignore = "sends three fictional search phrases to the public Wiby API"]
    fn live_wiby_batch_of_planner_style_phrases() {
        let _lock = TEST_LOCK.lock().unwrap();
        start(
            "public-query-evaluation".into(),
            "https://wiby.me/".into(),
            vec![
                "microscopy technician careers".into(),
                "remote rust jobs".into(),
                "payroll accountant careers".into(),
            ],
            1,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(65);
        let mut pages = 0;
        loop {
            let result = poll("public-query-evaluation", pages).unwrap();
            if result["status"] == "complete" {
                println!("coverage: {}", result["coverage"]);
                break;
            }
            assert_ne!(result["status"], "failed", "{result}");
            if result.get("query").is_some() {
                println!(
                    "query={:?} reported={} retained={} error={}",
                    result["query"],
                    result["data"]["results_reported"],
                    result["data"]["results"].as_array().map_or(0, Vec::len),
                    result["error"].is_string()
                );
                if let Some(items) = result["data"]["results"].as_array() {
                    for item in items.iter().take(10) {
                        println!(
                            "candidate: {} {}",
                            item["title"].as_str().unwrap_or(""),
                            item["url"].as_str().unwrap_or("")
                        );
                    }
                }
                pages += 1;
            }
            assert!(Instant::now() < deadline, "public search did not finish");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn batch_visits_each_phrase_before_deeper_pages() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            for (query, page) in [("first", 1), ("second", 1), ("first", 2), ("second", 2)] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let n = stream.read(&mut request).unwrap();
                let request = std::str::from_utf8(&request[..n]).unwrap();
                assert!(request.contains(&format!("q={query}")), "{request}");
                assert!(request.contains(&format!("pageno={page}")), "{request}");
                let body = r#"{"results":[]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        start(
            "fair-pages".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["first".into(), "second".into()],
            2,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut pages = Vec::new();
        loop {
            let result = poll("fair-pages", pages.len()).unwrap();
            if result["status"] == "complete" {
                break;
            }
            if result.get("data").is_some() {
                pages.push((
                    result["query"].as_str().unwrap().to_owned(),
                    result["page"].as_u64().unwrap(),
                ));
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            pages,
            vec![
                ("first".into(), 1),
                ("second".into(), 1),
                ("first".into(), 2),
                ("second".into(), 2)
            ]
        );
        provider.join().unwrap();
    }

    #[test]
    fn searches_requested_pages_and_deduplicates_between_them() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            for page in 1..=2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let n = stream.read(&mut request).unwrap();
                let request = std::str::from_utf8(&request[..n]).unwrap();
                assert!(request.contains(&format!("pageno={page}")), "{request}");
                let body = if page == 1 {
                    r#"{"results":[{"title":"One","url":"https://example.org/a"}]}"#
                } else {
                    r#"{"results":[{"title":"One","url":"https://example.org/a"},{"title":"Two","url":"https://example.org/b"}]}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        start(
            "page-test".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["engineer".into()],
            2,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut results = Vec::new();
        loop {
            let result = poll("page-test", results.len()).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["processed"], 2);
                break;
            }
            if result.get("data").is_some() {
                results.push(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(results[0]["page"], 1);
        assert_eq!(results[1]["page"], 2);
        assert_eq!(results[1]["data"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(results[1]["data"]["results"][0]["title"], "Two");
        provider.join().unwrap();
    }
}
