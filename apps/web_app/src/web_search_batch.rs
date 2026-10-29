//! Sequential, user-approved web-search phrases. Results are unverified candidates.
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    time::Duration,
};
struct Task {
    id: String,
    rx: Receiver<Message>,
    cancel: Arc<AtomicBool>,
    total: usize,
    processed: usize,
    pending: Option<Value>,
    history: Vec<Value>,
}
enum Message {
    Query(String, u8, Result<Value, String>),
    Done(bool),
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);
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
                Some((id, result)) => json!({"active":false,"last":{"task_id":id,"result":result}}),
                None => json!({"active":false}),
            }
        }
    })
}
fn send(tx: &SyncSender<Message>, mut message: Message, cancel: &AtomicBool) -> bool {
    loop {
        match tx.try_send(message) {
            Ok(()) => return true,
            Err(TrySendError::Full(m)) => {
                message = m;
                if cancel.load(Ordering::Acquire) {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
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
    let total = requests.len();
    *task = Some(Task {
        id,
        rx,
        cancel,
        total,
        processed: 0,
        pending: None,
        history: Vec::new(),
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
                if let Some(items) = data["results"].as_array_mut() {
                    items.retain(|item| {
                        item["url"].as_str().is_some_and(|url| {
                            if seen.contains(url) {
                                return false;
                            }
                            if seen.len() >= 600 {
                                limit_reached = true;
                                return false;
                            }
                            let inserted = seen.insert(url.to_owned());
                            if seen.len() >= 600 {
                                limit_reached = true;
                            }
                            inserted
                        })
                    });
                }
                data
            });
            if !send(&tx, Message::Query(query, page, result), &stop) {
                return;
            }
            if limit_reached {
                break;
            }
        }
        let _ = send(&tx, Message::Done(limit_reached), &AtomicBool::new(false));
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
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone())
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
            let response = json!({"status":"complete","processed":processed,"total":total,"cancelled":cancelled,"limit_reached":limit_reached,"history":task.history});
            *COMPLETED.lock().map_err(|_| "Web search unavailable")? =
                Some((id.to_owned(), response.clone()));
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
            let response = if cancelled {
                json!({"status":"complete","processed":processed,"total":total,"cancelled":true,"history":task.history})
            } else {
                json!({"status":"failed","processed":processed,"total":total,"error":"Batch search stopped before completion","history":task.history})
            };
            *COMPLETED.lock().map_err(|_| "Web search unavailable")? =
                Some((id.to_owned(), response.clone()));
            *tasks = None;
            Ok(response)
        }
    }
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
                let mut results: Vec<Value> = (0..30)
                    .map(|i| json!({"title":"Repeated", "url":format!("https://example.org/{i}")}))
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
        assert_eq!(batches[0]["data"]["results"].as_array().unwrap().len(), 30);
        assert_eq!(batches[1]["data"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(batches[1]["data"]["results"][0]["title"], "New result");
        assert_eq!(batches[1]["data"]["previously_seen"], 30);
        assert_eq!(batches[1]["data"]["results_examined"], 31);
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
