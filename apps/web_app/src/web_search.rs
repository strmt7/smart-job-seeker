//! Bounded, user-initiated SearXNG-compatible search. Results are candidates,
//! not verified jobs, and are never opened or submitted by this module.
use serde_json::{json, Value};
use std::sync::{mpsc, Mutex};
use url::Url;

struct Task {
    id: String,
    rx: mpsc::Receiver<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

pub(crate) fn request_url(instance: &str, query: &str) -> Result<Url, String> {
    if query.trim().is_empty() || query.len() > 500 || query.chars().any(char::is_control) {
        return Err("Enter a search query of at most 500 bytes".into());
    }
    let mut url = Url::parse(instance.trim()).map_err(|_| "Enter a valid search provider URL")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Provider URL must not contain credentials, query or fragment".into());
    }
    let local = url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if !local && (url.scheme() != "https" || !waypoint_connectors::is_public_host_url(url.as_str()))
    {
        return Err("Use HTTPS or an explicit http://127.0.0.1 local provider".into());
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    let mut search = url.join("search").map_err(|_| "Invalid provider path")?;
    search
        .query_pairs_mut()
        .append_pair("q", query.trim())
        .append_pair("format", "json");
    Ok(search)
}

pub(crate) fn fetch(url: &Url) -> Result<String, String> {
    let response = if url.scheme() == "http" {
        let port = url
            .port_or_known_default()
            .ok_or("Missing local provider port")?;
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let target = format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        waypoint_net::request_loopback(address, "GET", &target, None, 15000, 2 * 1024 * 1024)
    } else {
        waypoint_net::get_capped(url.as_str(), 15000, 2 * 1024 * 1024)
    }
    .map_err(|e| e.to_string())?;
    if response.status == 403 {
        return Err(
            "Provider refused JSON search; its administrator may need to enable JSON output".into(),
        );
    }
    if !(200..300).contains(&response.status) {
        return Err(format!("Search provider returned HTTP {}", response.status));
    }
    String::from_utf8(response.body).map_err(|_| "Provider returned non-UTF-8 output".into())
}

pub(crate) fn parse(payload: &str) -> Result<Value, String> {
    let data: Value =
        serde_json::from_str(payload).map_err(|_| "Provider returned invalid JSON")?;
    let results = data
        .get("results")
        .and_then(Value::as_array)
        .ok_or("Provider returned no results array")?;
    let mut links = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in results.iter().take(100) {
        let Some(raw) = item.get("url").and_then(Value::as_str) else {
            continue;
        };
        if raw.len() > 2048 {
            continue;
        }
        let Ok(url) = Url::parse(raw) else { continue };
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || !waypoint_connectors::is_public_host_url(url.as_str())
            || !seen.insert(url.to_string())
        {
            continue;
        }
        let Some(title) = item
            .get("title")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        else {
            continue;
        };
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        let snippet: String = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(1000)
            .collect();
        links.push(json!({"url":url.as_str(),"title":title,"snippet":snippet}));
        if links.len() == 30 {
            break;
        }
    }
    let unavailable = data
        .get("unresponsive_engines")
        .and_then(Value::as_array)
        .map(|v| v.len())
        .unwrap_or(0);
    Ok(
        json!({"results":links,"results_examined":results.len().min(100),"unresponsive_engines":unavailable,"limited":results.len()>30}),
    )
}

pub fn start(id: String, instance: String, query: String) -> Result<(), String> {
    if crate::web_search_batch::busy() {
        return Err("A batch web search is active".into());
    }
    let url = request_url(&instance, &query)?;
    let mut tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    if tasks.is_some() {
        return Err("A web search is running or awaiting its result".into());
    }
    let (tx, rx) = mpsc::channel();
    *tasks = Some(Task { id, rx });
    drop(tasks);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| fetch(&url).and_then(|body| parse(&body)))
            .unwrap_or_else(|_| Err("Web search stopped unexpectedly".into()));
        let _ = tx.send(result);
    });
    Ok(())
}

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    let task = tasks
        .as_ref()
        .filter(|task| task.id == id)
        .ok_or("Web search not found")?;
    match task.rx.try_recv() {
        Ok(result) => {
            *tasks = None;
            match result {
                Ok(data) => Ok(json!({"status":"complete","data":data})),
                Err(error) => Ok(json!({"status":"failed","error":error})),
            }
        }
        Err(mpsc::TryRecvError::Empty) => Ok(json!({"status":"running"})),
        Err(mpsc::TryRecvError::Disconnected) => {
            *tasks = None;
            Err("Web search stopped unexpectedly".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_and_results_are_bounded_and_untrusted_urls_rejected() {
        let url = request_url("http://127.0.0.1:8080/", "\"engineer\" remote").unwrap();
        assert_eq!(url.path(), "/search");
        assert!(url.as_str().contains("format=json"));
        assert!(request_url("http://192.168.1.2:8080/", "engineer").is_err());
        assert!(request_url("https://user:secret@example.org/", "engineer").is_err());
        let data=parse(&json!({"results":[{"title":"Role","url":"https://example.org/job","content":"Fictional"},{"title":"private","url":"https://127.0.0.1/job"},{"title":"duplicate","url":"https://example.org/job"}]}).to_string()).unwrap();
        assert_eq!(data["results"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn local_provider_search_runs_in_background_and_returns_candidates() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let n = socket.read(&mut request).unwrap();
            let text = std::str::from_utf8(&request[..n]).unwrap();
            assert!(text.starts_with("GET /search?"));
            assert!(text.contains("format=json"));
            let body = r#"{"results":[{"title":"Fictional role","url":"https://example.org/role","content":"A fictional listing"}]}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        start(
            "fixture-search".into(),
            format!("http://127.0.0.1:{port}/"),
            "materials engineer".into(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let result = poll("fixture-search").unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["data"]["results"][0]["title"], "Fictional role");
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        provider.join().unwrap();
    }
}
