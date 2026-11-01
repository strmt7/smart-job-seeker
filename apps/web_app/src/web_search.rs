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
    if is_wiby(&url) {
        let mut search = url.join("json/").map_err(|_| "Invalid provider path")?;
        search.query_pairs_mut().append_pair("q", query.trim());
        return Ok(search);
    }
    let mut search = url.join("search").map_err(|_| "Invalid provider path")?;
    search
        .query_pairs_mut()
        .append_pair("q", query.trim())
        .append_pair("format", "json");
    Ok(search)
}

fn is_wiby(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("wiby.me")
        && url.port().is_none()
        && url.path() == "/"
}

fn is_wiby_request(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("wiby.me")
        && url.port().is_none()
        && url.path() == "/json/"
}

pub(crate) fn page_parameter(url: &Url) -> &'static str {
    if is_wiby_request(url) {
        "p"
    } else {
        "pageno"
    }
}

pub(crate) fn parse_provider(
    payload: &str,
    url: &Url,
    excluded: &std::collections::HashSet<String>,
) -> Result<Value, String> {
    if !is_wiby_request(url) {
        return parse_excluding(payload, excluded);
    }
    let data: Value =
        serde_json::from_str(payload).map_err(|_| "Provider returned invalid JSON")?;
    let entries = data
        .as_array()
        .ok_or("Provider returned no results array")?;
    if entries.len() > 500 {
        return Err("Provider returned too many results".into());
    }
    let results: Vec<Value> = entries.iter().map(|item| json!({"url":item.get("URL"),"title":item.get("Title"),"content":item.get("Snippet").or_else(|| item.get("Description")),"engine":"Wiby"})).collect();
    let mut parsed = parse_excluding(&json!({"results":results}).to_string(), excluded)?;
    parsed["provider"] = json!("wiby");
    Ok(parsed)
}

pub(crate) struct FetchError {
    pub message: String,
    pub stop_batch: bool,
}

impl From<String> for FetchError {
    fn from(message: String) -> Self {
        Self {
            message,
            stop_batch: false,
        }
    }
}

pub(crate) fn fetch(url: &Url) -> Result<String, FetchError> {
    let response = if url.scheme() == "http" {
        let port = url
            .port_or_known_default()
            .ok_or_else(|| FetchError::from("Missing local provider port".to_owned()))?;
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
    .map_err(|e| FetchError::from(e.to_string()))?;
    if response.status == 429 {
        return Err(FetchError {
            message: "Search provider rate limited this request (HTTP 429)".into(),
            stop_batch: true,
        });
    }
    if response.status == 403 {
        return Err(FetchError {
            message: "Provider refused JSON search (HTTP 403)".into(),
            stop_batch: true,
        });
    }
    if !(200..300).contains(&response.status) {
        return Err(FetchError::from(format!(
            "Search provider returned HTTP {}",
            response.status
        )));
    }
    String::from_utf8(response.body)
        .map_err(|_| FetchError::from("Provider returned non-UTF-8 output".to_owned()))
}

#[cfg(test)]
pub(crate) fn parse(payload: &str) -> Result<Value, String> {
    parse_excluding(payload, &std::collections::HashSet::new())
}

fn merge_result_provenance(
    entry: &mut Value,
    title: &str,
    title_missing: bool,
    snippet: &str,
    engines: &[String],
    engines_limited: bool,
    date: &str,
) {
    if entry["title_missing"] == true && !title_missing {
        entry["title"] = json!(title);
        entry["title_missing"] = json!(false);
    }
    if entry
        .get("snippet")
        .and_then(Value::as_str)
        .is_some_and(|current| current.trim().is_empty())
        && !snippet.trim().is_empty()
    {
        entry["snippet"] = json!(snippet);
    }
    let mut truncated = engines_limited || entry["engines_limited"] == true;
    if let Some(existing) = entry["engines"].as_array_mut() {
        for engine in engines {
            if !existing
                .iter()
                .any(|item| item.as_str() == Some(engine.as_str()))
            {
                if existing.len() < 8 {
                    existing.push(json!(engine));
                } else {
                    truncated = true;
                }
            }
        }
    }
    entry["engines_limited"] = json!(truncated);
    let previous_date = entry["published_date"].as_str().unwrap_or("");
    if previous_date.is_empty() && !date.is_empty() {
        entry["published_date"] = json!(date);
    } else if !date.is_empty() && date != previous_date {
        if let Some(dates) = entry["alternative_published_dates"].as_array_mut() {
            if !dates.iter().any(|item| item.as_str() == Some(date)) {
                if dates.len() < 4 {
                    dates.push(json!(date));
                } else {
                    entry["dates_limited"] = json!(true);
                }
            }
        }
    }
}

pub(crate) fn parse_excluding(
    payload: &str,
    excluded: &std::collections::HashSet<String>,
) -> Result<Value, String> {
    let prefix: String = payload
        .trim_start_matches('\u{feff}')
        .trim_start()
        .chars()
        .take(32)
        .collect::<String>()
        .to_ascii_lowercase();
    if prefix.starts_with("<!doctype html") || prefix.starts_with("<html") {
        return Err("Provider returned an HTML page instead of JSON; check whether its JSON search API is enabled or choose another provider".into());
    }
    let data: Value =
        serde_json::from_str(payload).map_err(|_| "Provider returned invalid JSON")?;
    let results = data
        .get("results")
        .and_then(Value::as_array)
        .ok_or("Provider returned no results array")?;
    let mut links: Vec<Value> = Vec::new();
    let mut repeated: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut examined = 0;
    let mut previously_seen = 0;
    let mut within_page_duplicates = 0usize;
    // A provider can repeat earlier pages before returning new results. Scan a
    // larger bounded prefix while still retaining at most 100 new links.
    for item in results.iter().take(500) {
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
        let cleaned_title: String = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        let title_missing = cleaned_title.trim().is_empty();
        let title = if title_missing {
            format!(
                "Untitled result from {}",
                url.host_str().unwrap_or("unknown host")
            )
        } else {
            cleaned_title
        };
        let snippet: String = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(1000)
            .collect();
        let mut engines = Vec::new();
        let mut engines_limited = false;
        if let Some(values) = item.get("engines").and_then(Value::as_array) {
            engines_limited = values.len() > 32;
            for value in values.iter().take(32).filter_map(Value::as_str) {
                let cleaned: String = value.chars().filter(|c| !c.is_control()).take(80).collect();
                let cleaned = cleaned.trim().to_owned();
                if !cleaned.is_empty() && !engines.contains(&cleaned) {
                    if engines.len() < 8 {
                        engines.push(cleaned);
                    } else {
                        engines_limited = true;
                    }
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
        if !seen.insert(url.to_string()) {
            within_page_duplicates += 1;
            if let Some(earlier) = links
                .iter_mut()
                .chain(repeated.iter_mut())
                .find(|entry| entry["url"] == url.as_str())
            {
                merge_result_provenance(
                    earlier,
                    &title,
                    title_missing,
                    &snippet,
                    &engines,
                    engines_limited,
                    published_date.trim(),
                );
            }
            continue;
        }
        if excluded.contains(url.as_str()) {
            previously_seen += 1;
            if repeated.len() < 10 {
                repeated.push(json!({"url":url.as_str(),"title":title,"title_missing":title_missing,"engines":engines,"engines_limited":engines_limited,"published_date":published_date.trim(),"alternative_published_dates":[],"dates_limited":false}));
            }
            continue;
        }
        links.push(json!({"url":url.as_str(),"title":title,"title_missing":title_missing,"snippet":snippet,"engines":engines,"engines_limited":engines_limited,"published_date":published_date.trim(),"alternative_published_dates":[],"dates_limited":false}));
        if links.len() == 100 {
            break;
        }
    }
    let unavailable = data.get("unresponsive_engines").and_then(Value::as_array);
    let mut unavailable_names = Vec::new();
    if let Some(entries) = unavailable {
        // Provider output is untrusted. Keep only bounded labels, not error
        // messages or URLs, and report when the list was not fully named.
        for entry in entries.iter().take(50) {
            let raw = entry
                .as_str()
                .or_else(|| {
                    entry
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(Value::as_str)
                })
                .or_else(|| entry.get("engine").and_then(Value::as_str));
            if let Some(raw) = raw {
                let name: String = raw.chars().filter(|c| !c.is_control()).take(80).collect();
                let name = name.trim();
                if !name.is_empty()
                    && !unavailable_names.iter().any(|prior: &String| prior == name)
                    && unavailable_names.len() < 8
                {
                    unavailable_names.push(name.to_owned());
                }
            }
        }
    }
    Ok(
        json!({"results":links,"results_examined":examined,"results_reported":results.len(),"unresponsive_engines":unavailable.map(Vec::len),"unresponsive_engine_names":unavailable_names,"limited":examined < results.len(),"results_rejected":examined-links.len()-previously_seen-within_page_duplicates,"within_page_duplicates":within_page_duplicates,"previously_seen":previously_seen,"repeated_references":repeated,"repeated_references_omitted":previously_seen-repeated.len()}),
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
        let result = std::panic::catch_unwind(|| {
            fetch(&url)
                .map_err(|error| error.message)
                .and_then(|body| parse_provider(&body, &url, &std::collections::HashSet::new()))
        })
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
    #[ignore = "requires live Wiby network access"]
    fn live_wiby_search_returns_unverified_candidates() {
        let url = request_url("https://wiby.me/", "software engineer jobs").unwrap();
        let payload = fetch(&url).map_err(|error| error.message).unwrap();
        let result = parse_provider(&payload, &url, &std::collections::HashSet::new()).unwrap();
        assert_eq!(result["provider"], "wiby");
        let results = result["results"].as_array().unwrap();
        assert!(!results.is_empty());
        println!(
            "provider results={}, usable https candidates={}",
            result["results_reported"],
            results.len()
        );
    }
    #[test]
    fn wiby_search_normalizes_results_and_uses_its_page_parameter() {
        let url = request_url("https://wiby.me/", "software engineer jobs").unwrap();
        assert_eq!(url.path(), "/json/");
        assert_eq!(page_parameter(&url), "p");
        let raw = json!([
            {"URL":"https://example.org/careers", "Title":"Fictional opening", "Snippet":"Apply here"},
            {"URL":"http://example.org/old", "Title":"Untrusted HTTP"},
            {"URL":"https://127.0.0.1/private", "Title":"Private host"}
        ]).to_string();
        let result = parse_provider(&raw, &url, &std::collections::HashSet::new()).unwrap();
        assert_eq!(result["provider"], "wiby");
        assert_eq!(result["results"].as_array().unwrap().len(), 1);
        assert_eq!(result["results_rejected"], 2);
        assert_eq!(result["results"][0]["engines"][0], "Wiby");
        assert!(parse_provider("{}", &url, &std::collections::HashSet::new()).is_err());
        assert_eq!(
            page_parameter(&request_url("https://example.org/", "role").unwrap()),
            "pageno"
        );
    }
    #[test]
    fn html_provider_response_reports_actionable_failure_without_page_contents() {
        let error = parse(" \n<!DOCTYPE html><html><body>private provider response</body></html>")
            .unwrap_err();
        assert!(error.contains("HTML page instead of JSON"));
        assert!(!error.contains("private provider response"));
        assert_eq!(
            parse("not json").unwrap_err(),
            "Provider returned invalid JSON"
        );
    }
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
    fn result_coverage_distinguishes_rejected_and_repeated_entries() {
        let payload = json!({"results":[
            {"url":"https://example.org/new","title":"Role"},
            {"url":"https://example.org/new","title":"Duplicate"},
            {"url":"https://example.org/previous","title":"Earlier"},
            {"url":"http://example.org/job","title":"Insecure"},
            {"url":"https://example.org/no-title"},
            {"title":"Missing URL"}
        ]})
        .to_string();
        let excluded = ["https://example.org/previous".to_owned()]
            .into_iter()
            .collect();
        let result = parse_excluding(&payload, &excluded).unwrap();
        assert_eq!(result["results_examined"], 6);
        assert_eq!(result["results"].as_array().unwrap().len(), 2);
        assert_eq!(result["results"][1]["title_missing"], true);
        assert_eq!(
            result["results"][1]["title"],
            "Untitled result from example.org"
        );
        assert_eq!(result["within_page_duplicates"], 1);
        assert_eq!(result["previously_seen"], 1);
        assert_eq!(result["results_rejected"], 2);
    }

    #[test]
    fn provider_engine_failures_retain_bounded_names_without_error_messages() {
        let data = parse(&json!({"results":[],"unresponsive_engines":[["engine-one","private detail"], ["engine-one","duplicate"], {"engine":"engine\ntwo"}, [42,"invalid"], "engine-three"]}).to_string()).unwrap();
        assert_eq!(data["unresponsive_engines"], 5);
        assert_eq!(
            data["unresponsive_engine_names"],
            json!(["engine-one", "enginetwo", "engine-three"])
        );
        assert!(!data.to_string().contains("private detail"));
        let failures: Vec<Value> = (0..12)
            .map(|i| json!([format!("engine-{i}"), "error"]))
            .collect();
        let limited =
            parse(&json!({"results":[],"unresponsive_engines":failures}).to_string()).unwrap();
        assert_eq!(limited["unresponsive_engines"], 12);
        assert_eq!(
            limited["unresponsive_engine_names"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        assert_eq!(
            parse(&json!({"results":[]}).to_string()).unwrap()["unresponsive_engines"],
            Value::Null
        );
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
        assert_eq!(duplicate["results"][0]["title_missing"], false);
        assert_eq!(duplicate["within_page_duplicates"], 1);
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
        let invalid = vec![json!({"title":"No URL"}); 501];
        let bounded = parse(&json!({"results":invalid}).to_string()).unwrap();
        assert_eq!(bounded["results_examined"], 500);
        assert_eq!(bounded["limited"], true);
    }

    #[test]
    fn identical_urls_keep_bounded_engine_and_conflicting_date_evidence() {
        let rows = json!([
            {"url":"https://example.org/job", "title":"", "engines":["alpha"], "publishedDate":"2026-10-01"},
            {"url":"https://example.org/job", "title":"Fictional role", "content":"Fictional description", "engines":["alpha","beta","gamma","delta","epsilon","zeta","eta","theta","iota"], "publishedDate":"2026-10-02"},
            {"url":"https://example.org/job", "engines":["beta"], "publishedDate":"2026-10-02"}
        ]);
        let data = parse(&json!({"results":rows}).to_string()).unwrap();
        let item = &data["results"][0];
        assert_eq!(data["within_page_duplicates"], 2);
        assert_eq!(data["results_rejected"], 0);
        assert_eq!(item["title"], "Fictional role");
        assert_eq!(item["snippet"], "Fictional description");
        assert_eq!(item["engines"].as_array().unwrap().len(), 8);
        assert_eq!(item["engines_limited"], true);
        assert_eq!(item["published_date"], "2026-10-01");
        assert_eq!(item["alternative_published_dates"], json!(["2026-10-02"]));
        let excluded = std::collections::HashSet::from(["https://example.org/job".to_owned()]);
        let repeated = parse_excluding(&json!({"results":rows}).to_string(), &excluded).unwrap();
        assert_eq!(repeated["previously_seen"], 1);
        assert_eq!(repeated["within_page_duplicates"], 2);
        assert_eq!(
            repeated["repeated_references"][0]["title"],
            "Fictional role"
        );
        assert_eq!(
            repeated["repeated_references"][0]["alternative_published_dates"],
            json!(["2026-10-02"])
        );
        let many: Vec<_> = (1..=7).map(|day| json!({"url":"https://example.org/job","publishedDate":format!("2026-10-{day:02}")})).collect();
        let bounded = parse(&json!({"results":many}).to_string()).unwrap();
        assert_eq!(
            bounded["results"][0]["alternative_published_dates"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(bounded["results"][0]["dates_limited"], true);
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
            let body = r#"{"results":[{"title":"Fictional role","url":"https://example.org/role","content":"A fictional listing","engines":["one"],"publishedDate":"2026-10-01"},{"title":"Fictional role","url":"https://example.org/role","engines":["two"],"publishedDate":"2026-10-02"}]}"#;
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
                assert_eq!(
                    result["data"]["results"][0]["engines"],
                    json!(["one", "two"])
                );
                assert_eq!(
                    result["data"]["results"][0]["alternative_published_dates"],
                    json!(["2026-10-02"])
                );
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
