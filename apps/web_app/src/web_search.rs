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
// Keep one bounded terminal result so a lost response can be read again.
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

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
    parse_excluding(payload, &std::collections::HashSet::new())
}

pub(crate) fn parse_excluding(
    payload: &str,
    excluded: &std::collections::HashSet<String>,
) -> Result<Value, String> {
    let data: Value =
        serde_json::from_str(payload).map_err(|_| "Provider returned invalid JSON")?;
    let results = data
        .get("results")
        .and_then(Value::as_array)
        .ok_or("Provider returned no results array")?;
    let mut links = Vec::new();
    let mut repeated = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut examined = 0;
    let mut previously_seen = 0;
    for item in results.iter().take(100) {
        examined += 1;
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
        if title.trim().is_empty() || !seen.insert(url.to_string()) {
            continue;
        }
        let snippet: String = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(1000)
            .collect();
        let mut engines = Vec::new();
        if let Some(values) = item.get("engines").and_then(Value::as_array) {
            for value in values.iter().take(8).filter_map(Value::as_str) {
                let cleaned: String = value.chars().filter(|c| !c.is_control()).take(80).collect();
                let cleaned = cleaned.trim().to_owned();
                if !cleaned.is_empty() && !engines.contains(&cleaned) {
                    engines.push(cleaned);
                }
            }
        }
        if engines.is_empty() {
            if let Some(value) = item.get("engine").and_then(Value::as_str) {
                let cleaned: String = value.chars().filter(|c| !c.is_control()).take(80).collect();
                let cleaned = cleaned.trim();
                if !cleaned.is_empty() {
                    engines.push(cleaned.to_owned());
                }
            }
        }
        let published_date: String = item
            .get("publishedDate")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect();
        if excluded.contains(url.as_str()) {
            previously_seen += 1;
            if repeated.len() < 10 {
                repeated.push(json!({"url":url.as_str(),"title":title,"engines":engines,"published_date":published_date.trim()}));
            }
            continue;
        }
        links.push(json!({"url":url.as_str(),"title":title,"snippet":snippet,"engines":engines,"published_date":published_date.trim()}));
        if links.len() == 100 {
            break;
        }
    }
    let unavailable = data
        .get("unresponsive_engines")
        .and_then(Value::as_array)
        .map(|v| v.len());
    Ok(
        json!({"results":links,"results_examined":examined,"results_reported":results.len(),"unresponsive_engines":unavailable,"limited":examined < results.len(),"previously_seen":previously_seen,"repeated_references":repeated,"repeated_references_omitted":previously_seen-repeated.len()}),
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

/// Find a running or unread search after a client loses the start response.
pub fn pending() -> Result<Option<String>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Web search unavailable")?
        .as_ref()
        .map(|task| task.id.clone()))
}

pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    let Some(task) = tasks.as_ref().filter(|task| task.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Web search unavailable")?
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone())
            .ok_or("Web search not found".into());
    };
    match task.rx.try_recv() {
        Ok(result) => {
            let terminal = match result {
                Ok(data) => json!({"status":"complete","data":data}),
                Err(error) => json!({"status":"failed","error":error}),
            };
            *COMPLETED.lock().map_err(|_| "Web search unavailable")? =
                Some((id.to_owned(), terminal.clone()));
            *tasks = None;
            Ok(terminal)
        }
        Err(mpsc::TryRecvError::Empty) => Ok(json!({"status":"running"})),
        Err(mpsc::TryRecvError::Disconnected) => {
            let terminal = json!({"status":"failed","error":"Web search stopped unexpectedly"});
            *COMPLETED.lock().map_err(|_| "Web search unavailable")? =
                Some((id.to_owned(), terminal.clone()));
            *tasks = None;
            Ok(terminal)
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
    fn provider_metadata_is_optional_bounded_and_cleaned() {
        let data = parse(&json!({"results":[
            {"title":"Role", "url":"https://example.org/one", "engines":["search\nengine", "search\nengine", 4], "publishedDate":"2026-10-28\n"},
            {"title":"Other", "url":"https://example.org/two", "engine":"fallback", "publishedDate":null},
            {"title":"Plain", "url":"https://example.org/three"}
        ]}).to_string()).unwrap();
        assert_eq!(data["results"][0]["engines"], json!(["searchengine"]));
        assert_eq!(data["results"][0]["published_date"], "2026-10-28");
        assert_eq!(data["results"][1]["engines"], json!(["fallback"]));
        assert_eq!(data["results"][2]["engines"], json!([]));
        assert_eq!(data["results"][2]["published_date"], "");
        assert!(data["unresponsive_engines"].is_null());
        assert_eq!(data["results_reported"], 3);
        let healthy = parse(&json!({"results":[],"unresponsive_engines":[]}).to_string()).unwrap();
        assert_eq!(healthy["unresponsive_engines"], 0);
        let degraded =
            parse(&json!({"results":[],"unresponsive_engines":[["engine","timeout"]]}).to_string())
                .unwrap();
        assert_eq!(degraded["unresponsive_engines"], 1);
        let engines: Vec<String> = (0..10).map(|i| format!("{i}{}", "x".repeat(100))).collect();
        let long = parse(&json!({"results":[{"title":"Role", "url":"https://example.org/long", "engines":engines, "publishedDate":"x".repeat(100)}]}).to_string()).unwrap();
        let engines = long["results"][0]["engines"].as_array().unwrap();
        assert_eq!(engines.len(), 8);
        assert_eq!(engines[0].as_str().unwrap().chars().count(), 80);
        assert_eq!(
            long["results"][0]["published_date"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            80
        );
    }

    #[test]
    fn result_limits_and_duplicate_validation_reflect_actual_work() {
        let duplicate = parse(
            &json!({"results":[
                {"title":"\u{0001}", "url":"https://example.org/job"},
                {"title":"Fictional role", "url":"https://example.org/job"}
            ]})
            .to_string(),
        )
        .unwrap();
        assert_eq!(duplicate["results"][0]["title"], "Fictional role");
        assert_eq!(duplicate["results_examined"], 2);
        assert_eq!(duplicate["limited"], false);

        let valid: Vec<Value> = (0..100)
            .map(|i| json!({"title":"Role","url":format!("https://example.org/{i}")}))
            .collect();
        let complete_page = parse(&json!({"results":valid}).to_string()).unwrap();
        assert_eq!(complete_page["results"].as_array().unwrap().len(), 100);
        assert_eq!(complete_page["results_examined"], 100);
        assert_eq!(complete_page["limited"], false);

        let valid: Vec<Value> = (0..101)
            .map(|i| json!({"title":"Role","url":format!("https://example.org/{i}")}))
            .collect();
        let limited = parse(&json!({"results":valid}).to_string()).unwrap();
        assert_eq!(limited["results"].as_array().unwrap().len(), 100);
        assert_eq!(limited["results_examined"], 100);
        assert_eq!(limited["limited"], true);

        let invalid = vec![json!({"title":"No URL"}); 35];
        let complete = parse(&json!({"results":invalid}).to_string()).unwrap();
        assert_eq!(complete["results_examined"], 35);
        assert_eq!(complete["limited"], false);
        let invalid = vec![json!({"title":"No URL"}); 120];
        let bounded = parse(&json!({"results":invalid}).to_string()).unwrap();
        assert_eq!(bounded["results_examined"], 100);
        assert_eq!(bounded["limited"], true);
    }

    #[test]
    fn repeated_urls_keep_bounded_search_provenance_without_new_candidates() {
        let excluded: std::collections::HashSet<String> = (0..15)
            .map(|index| format!("https://example.org/job/{index}"))
            .collect();
        let rows: Vec<Value> = (0..15)
            .map(|index| {
                json!({
                    "title":format!("Fictional role {index}"),
                    "url":format!("https://example.org/job/{index}"),
                    "engines":["fictional engine"],
                    "publishedDate":"2026-10-30"
                })
            })
            .collect();
        let result = parse_excluding(&json!({"results":rows}).to_string(), &excluded).unwrap();
        assert_eq!(result["results"].as_array().unwrap().len(), 0);
        assert_eq!(result["previously_seen"], 15);
        assert_eq!(result["repeated_references"].as_array().unwrap().len(), 10);
        assert_eq!(result["repeated_references_omitted"], 5);
        assert_eq!(
            result["repeated_references"][0]["engines"],
            json!(["fictional engine"])
        );
        assert_eq!(
            result["repeated_references"][0]["url"],
            "https://example.org/job/0"
        );
    }

    #[test]
    fn local_provider_search_runs_in_background_and_returns_candidates() {
        let _lock = crate::web_search_batch::TEST_LOCK.lock().unwrap();
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
        assert_eq!(pending().unwrap().as_deref(), Some("fixture-search"));
        let api_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api_port = api_listener.local_addr().unwrap().port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", api_port)).unwrap();
        let body = "{}";
        write!(client, "POST /api/web-search/pending HTTP/1.1\r\nHost: 127.0.0.1:{api_port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = api_listener.accept().unwrap();
        let mut workspace = waypoint_workspace::Workspace::in_memory().unwrap();
        crate::serve(
            &mut server,
            &mut workspace,
            &mut None,
            "fixture-token",
            api_port,
        )
        .unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["task_id"], "fixture-search");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let result = poll("fixture-search").unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["data"]["results"][0]["title"], "Fictional role");
                assert_eq!(poll("fixture-search").unwrap(), result);
                assert!(!busy());
                assert!(pending().unwrap().is_none());
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        provider.join().unwrap();
        let (tx, rx) = mpsc::channel();
        drop(tx);
        *TASK.lock().unwrap() = Some(Task {
            id: "stopped-fixture".into(),
            rx,
        });
        let failed = poll("stopped-fixture").unwrap();
        assert_eq!(failed["status"], "failed");
        assert_eq!(poll("stopped-fixture").unwrap(), failed);
        assert!(poll("fixture-search").is_err());
        assert!(!busy());
    }
}
