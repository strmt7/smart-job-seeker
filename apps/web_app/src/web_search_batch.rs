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
    processed: usize,
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
    let mut requests = Vec::new();
    for query in queries {
        let query = query.trim().to_owned();
        if !seen.insert(query.to_lowercase()) {
            return Err("Search phrases must be distinct".into());
        }
        let url = crate::web_search::request_url(&provider, &query)?;
        for page in 1..=pages {
            let mut page_url = url.clone();
            page_url
                .query_pairs_mut()
                .append_pair("pageno", &page.to_string());
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
    *task = Some(Task {
        id,
        rx,
        cancel,
        total,
        processed: 0,
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
            let result = std::panic::catch_unwind(|| {
                crate::web_search::fetch(&url)
                    .and_then(|body| crate::web_search::parse_excluding(&body, &seen))
            })
            .unwrap_or_else(|_| Err("Search stopped unexpectedly".into()))
            .map(|mut data| {
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
            if limit_reached {
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
    #[test]
    fn background_collection_retains_pages_for_later_client_recovery() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        *TASK.lock().unwrap() = Some(Task {
            id: "background-fixture".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 2,
            processed: 0,
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
            processed: 0,
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
            processed: 0,
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
            processed: 0,
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
    fn results_are_replayed_until_acknowledged_and_completion_is_recoverable() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::sync_channel(2);
        *TASK.lock().unwrap() = Some(Task {
            id: "replay".into(),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            total: 1,
            processed: 0,
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
            processed: 0,
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
            processed: 0,
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
