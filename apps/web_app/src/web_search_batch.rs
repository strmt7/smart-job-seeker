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
}
enum Message {
    Query(String, Result<Value, String>),
    Done,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
pub fn busy() -> bool {
    TASK.lock().map(|t| t.is_some()).unwrap_or(true)
}
fn send(tx: &SyncSender<Message>, mut message: Message, cancel: &AtomicBool) -> bool {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        match tx.try_send(message) {
            Ok(()) => return true,
            Err(TrySendError::Full(m)) => {
                message = m;
                if cancel.load(Ordering::Acquire) || Instant::now() >= until {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}
pub fn start(id: String, provider: String, queries: Vec<String>) -> Result<(), String> {
    if crate::web_search::busy() {
        return Err("A web search is active".into());
    }
    if queries.is_empty() || queries.len() > 10 {
        return Err("Enter 1 to 10 search phrases".into());
    }
    let mut seen = HashSet::new();
    let mut requests = Vec::new();
    for query in queries {
        let query = query.trim().to_owned();
        if !seen.insert(query.to_lowercase()) {
            return Err("Search phrases must be distinct".into());
        }
        let url = crate::web_search::request_url(&provider, &query)?;
        requests.push((query, url));
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
    });
    drop(task);
    std::thread::spawn(move || {
        let mut seen = HashSet::new();
        for (index, (query, url)) in requests.into_iter().enumerate() {
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
                crate::web_search::fetch(&url).and_then(|body| crate::web_search::parse(&body))
            })
            .unwrap_or_else(|_| Err("Search stopped unexpectedly".into()))
            .map(|mut data| {
                if let Some(items) = data["results"].as_array_mut() {
                    items.retain(|item| {
                        item["url"]
                            .as_str()
                            .is_some_and(|url| seen.insert(url.to_owned()))
                    });
                }
                data
            });
            if !send(&tx, Message::Query(query, result), &stop) {
                return;
            }
        }
        let _ = send(&tx, Message::Done, &AtomicBool::new(false));
    });
    Ok(())
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|t| t.id == id)
        .ok_or("Batch web search not found")?;
    match task.rx.try_recv() {
        Ok(Message::Query(query, result)) => {
            task.processed += 1;
            Ok(match result {
                Ok(data) => {
                    json!({"status":"running","processed":task.processed,"total":task.total,"query":query,"data":data})
                }
                Err(error) => {
                    json!({"status":"running","processed":task.processed,"total":task.total,"query":query,"error":error})
                }
            })
        }
        Ok(Message::Done) => {
            let (processed, total, cancelled) = (
                task.processed,
                task.total,
                task.cancel.load(Ordering::Acquire),
            );
            *tasks = None;
            Ok(
                json!({"status":"complete","processed":processed,"total":total,"cancelled":cancelled}),
            )
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
            *tasks = None;
            if cancelled {
                Ok(
                    json!({"status":"complete","processed":processed,"total":total,"cancelled":true}),
                )
            } else {
                Ok(json!({"status":"failed","error":"Batch search stopped before completion"}))
            }
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
    #[test]
    fn searches_sequentially_deduplicates_and_reports_each_query() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            for n in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let len = stream.read(&mut request).unwrap();
                let request = std::str::from_utf8(&request[..len]).unwrap();
                assert!(request.contains(if n == 0 { "first" } else { "second" }));
                let body = if n == 0 {
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
            "test-batch".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["first".into(), "second".into()],
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut batches = Vec::new();
        loop {
            let result = poll("test-batch").unwrap();
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
        assert_eq!(batches[0]["data"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(batches[1]["data"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(batches[1]["data"]["results"][0]["title"], "Two");
        provider.join().unwrap();
    }
}
