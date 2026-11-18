//! Sequential, user-approved web-search phrases. Results are unverified candidates.
use crate::web_search::{FetchError, ProviderStop, SearchStart};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use waypoint_workspace::{WebSearchWriter, Workspace};

const CURSOR_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
struct SearchPage {
    query: String,
    page: u8,
}

#[derive(Serialize, Deserialize)]
struct SearchPlan {
    version: u8,
    plan: Vec<SearchPage>,
    provider: String,
    prefer_fresh: bool,
    retry_source_task: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome {
    Complete {
        cancelled: bool,
        limit_reached: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_stop: Option<ProviderStop>,
    },
    Failed {
        error: String,
        fetched_unreported_pages: usize,
    },
}

struct Task {
    id: String,
    rx: Receiver<Message>,
    cancel: Arc<AtomicBool>,
    total: usize,
    plan: Vec<(String, u8)>,
    provider: Option<String>,
    processed: usize,
    prefer_fresh: bool,
    pending: Option<Value>,
    history: Vec<Value>,
    prior_history: Vec<Value>,
    timed_out: Arc<AtomicBool>,
    unreported: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
enum Message {
    Query(String, u8, Result<Value, String>),
    Done(Outcome, bool),
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

// Leave room in the store's 12 MiB sealed record for the plan, coverage,
// failures and the last page receipt. Short entries need not consume the
// same share of this budget as long descriptions or multiple engine reports.
const SAVED_PAGE_BUDGET: usize = 8 * 1024 * 1024;
const PAGE_ENVELOPE_BYTES: usize = 256;

fn saved_search(ws: &Workspace) -> Result<Option<(String, Value)>, String> {
    let Some(run) = ws
        .latest_web_search_run()
        .map_err(|error| error.to_string())?
    else {
        return ws
            .latest_web_search()
            .map_err(|error| error.to_string())?
            .map(|(id, bytes)| {
                let saved: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| "Saved web search history is invalid")?;
                if !matches!(saved["status"].as_str(), Some("complete" | "failed"))
                    || !saved["history"].is_array()
                {
                    return Err("Saved web search history is invalid".into());
                }
                Ok((id, saved))
            })
            .transpose();
    };
    let plan: SearchPlan =
        serde_json::from_slice(&run.plan).map_err(|_| "Saved search plan is invalid")?;
    if plan.version != CURSOR_VERSION || run.planned != plan.plan.len() {
        return Err("Saved search cursor uses an unsupported plan".into());
    }
    let mut history = Vec::with_capacity(run.pages.len());
    for (index, (bytes, expected)) in run.pages.iter().zip(&plan.plan).enumerate() {
        let page: Value =
            serde_json::from_slice(bytes).map_err(|_| "Saved search page is invalid")?;
        if page["query"] != expected.query
            || page["page"] != expected.page
            || page["processed"] != index + 1
            || page["total"] != run.planned
            || (page["error"].is_string() == page["data"]["results"].is_array())
        {
            return Err("Saved search page does not match its plan".into());
        }
        history.push(page);
    }
    let prior_history = run
        .prior_pages
        .iter()
        .map(|bytes| serde_json::from_slice::<Value>(bytes))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Saved earlier search results are invalid")?;
    let recovered_partial = run.terminal.is_none();
    let outcome = match run.terminal {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Saved search outcome is invalid")?,
        None => Outcome::Failed {
            error: "The app stopped before this search finished; another provider request may have been in progress".into(),
            fetched_unreported_pages: 0,
        },
    };
    let mut result = terminal_result(&outcome, run.planned, &history, true);
    result["history"] = json!(history);
    result["prior_history"] = json!(prior_history);
    result["plan"] = json!(plan.plan);
    result["provider"] = json!(plan.provider);
    result["prefer_fresh"] = json!(plan.prefer_fresh);
    result["retry_source_task"] = json!(plan.retry_source_task);
    if recovered_partial {
        result["recovered_partial"] = json!(true);
    }
    Ok(Some((run.task_id, result)))
}

/// Load only successfully collected pages from the user-sealed previous
/// search. A retry must bind to the exact saved task, not arbitrary client data.
pub fn previous_successful_history(
    ws: &Workspace,
    source_task: &str,
) -> Result<Vec<Value>, String> {
    let (id, saved) = saved_search(ws)?.ok_or("The earlier search history is unavailable")?;
    if id != source_task {
        return Err(
            "The earlier search changed; review the current results before retrying".into(),
        );
    }
    let history = saved["history"]
        .as_array()
        .ok_or("Saved web search history is invalid")?;
    let prior = match saved.get("prior_history") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(items)) => items.as_slice(),
        _ => return Err("Saved earlier search results are invalid".into()),
    };
    let mut groups = Vec::new();
    for group in prior.iter().chain(history) {
        if group.get("error").is_some() {
            continue;
        }
        group["data"]["results"]
            .as_array()
            .ok_or("Saved search page is invalid")?;
        groups.push(group.clone());
    }
    if serde_json::to_vec(&groups)
        .map_err(|_| "Could not serialize earlier search results")?
        .len()
        > SAVED_PAGE_BUDGET
    {
        return Err("Earlier search results exceed the retry storage limit".into());
    }
    Ok(groups)
}
pub fn busy() -> bool {
    TASK.lock().map(|t| t.is_some()).unwrap_or(true)
}
pub fn pending() -> Result<Value, String> {
    let tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    Ok(match tasks.as_ref() {
        Some(task) => {
            json!({"active":true,"task_id":task.id,"history":task.history,"prior_history":task.prior_history})
        }
        None => {
            let completed = COMPLETED.lock().map_err(|_| "Web search unavailable")?;
            match completed.as_ref() {
                Some((id, result)) => {
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

fn protected_provider_call(
    operation: impl FnOnce() -> Result<Value, FetchError> + std::panic::UnwindSafe,
) -> Result<Value, FetchError> {
    std::panic::catch_unwind(operation).unwrap_or_else(|_| {
        Err(FetchError::unusable_response(
            "Search stopped unexpectedly".into(),
        ))
    })
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

fn page_receipt(
    query: &str,
    page: u8,
    processed: usize,
    total: usize,
    result: &Result<Value, String>,
) -> Value {
    let mut receipt = json!({
        "status":"running","processed":processed,"total":total,"query":query,"page":page
    });
    match result {
        Ok(data) => receipt["data"] = data.clone(),
        Err(error) => receipt["error"] = json!(error),
    }
    receipt
}

fn terminal_result(outcome: &Outcome, total: usize, history: &[Value], saved: bool) -> Value {
    let mut result = json!(outcome);
    let mut report = coverage(total, history);
    if let Outcome::Failed {
        fetched_unreported_pages,
        ..
    } = outcome
    {
        let attempted = history.len().saturating_add(*fetched_unreported_pages);
        report["fetched_unreported_pages"] = json!(fetched_unreported_pages);
        report["attempted_pages"] = json!(attempted);
        report["unattempted_pages"] = json!(total.saturating_sub(attempted));
    }
    result["processed"] = json!(history.len());
    result["total"] = json!(total);
    result["history_saved"] = json!(saved);
    result["coverage"] = report;
    if saved {
        result["cursor"] = json!({"version":CURSOR_VERSION,"committed_pages":history.len()});
    }
    result
}

// Comparison key only: preserve the original URL in the result. Remove a
// small set of standard tracking parameters; all other query parameters and
// fragments remain significant because sites may use them to identify roles.
fn tracking_comparison_key(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return raw.to_owned();
    };
    let retained: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(name, _)| {
            !matches!(
                name.to_ascii_lowercase().as_str(),
                "utm_source"
                    | "utm_medium"
                    | "utm_campaign"
                    | "utm_term"
                    | "utm_content"
                    | "gclid"
                    | "fbclid"
            )
        })
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    if !retained.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in retained {
            pairs.append_pair(&name, &value);
        }
    }
    url.into()
}

#[derive(Default)]
struct BatchSeen {
    originals: HashSet<String>,
    comparison_keys: HashMap<String, String>,
    saved_bytes: usize,
}

fn seen_from_prior(history: &[Value]) -> Result<BatchSeen, String> {
    let saved_bytes = serde_json::to_vec(history)
        .map_err(|_| "Could not serialize earlier search results")?
        .len();
    if saved_bytes > SAVED_PAGE_BUDGET {
        return Err("Earlier search results exceed the search history size limit".into());
    }
    let mut seen = BatchSeen {
        saved_bytes,
        ..BatchSeen::default()
    };
    for group in history {
        let results = group["data"]["results"]
            .as_array()
            .ok_or("Earlier search page is invalid")?;
        for result in results {
            let url = result["url"]
                .as_str()
                .filter(|url| {
                    url.len() <= 2048
                        && waypoint_connectors::is_public_host_url(url)
                        && url::Url::parse(url).is_ok_and(|parsed| {
                            parsed.scheme() == "https"
                                && parsed.username().is_empty()
                                && parsed.password().is_none()
                        })
                })
                .ok_or("Earlier search result URL is invalid")?;
            if seen.originals.insert(url.to_owned()) {
                seen.comparison_keys
                    .entry(tracking_comparison_key(url))
                    .or_insert_with(|| url.to_owned());
            }
        }
    }
    Ok(seen)
}

fn retain_batch_budget(data: &mut Value, query: &str, seen: &mut BatchSeen) -> bool {
    let mut omitted = 0usize;
    let mut repeated = 0usize;
    let mut potentially_related = 0usize;
    if let Some(Value::Array(mut items)) = data.get_mut("results").map(Value::take) {
        data["results"] = json!([]);
        let page_bytes =
            data.to_string().len() + json!(query).to_string().len() + PAGE_ENVELOPE_BYTES;
        seen.saved_bytes += page_bytes;
        items.retain_mut(|item| {
            let Some(url) = item["url"].as_str().map(str::to_owned) else {
                return false;
            };
            if seen.originals.contains(&url) {
                repeated += 1;
                return false;
            }
            let comparison = tracking_comparison_key(&url);
            let variant_of = seen.comparison_keys.get(&comparison);
            if let Some(first) = variant_of {
                item["possible_tracking_variant"] = json!(true);
                item["possible_tracking_variant_of"] = json!(first);
            }
            let bytes = item.to_string().len() + 1;
            if bytes > SAVED_PAGE_BUDGET.saturating_sub(seen.saved_bytes) {
                omitted += 1;
                return false;
            }
            seen.saved_bytes += bytes;
            if variant_of.is_some() {
                potentially_related += 1;
            } else {
                seen.comparison_keys.insert(comparison, url.clone());
            }
            seen.originals.insert(url)
        });
        data["results"] = Value::Array(items);
    }
    if repeated > 0 {
        data["exact_repeated_omitted"] = json!(repeated);
    }
    if potentially_related > 0 {
        data["possible_tracking_variants"] = json!(potentially_related);
    }
    if omitted > 0 {
        data["limited"] = json!(true);
        data["batch_limit_omitted"] = json!(omitted);
    }
    omitted > 0 || seen.saved_bytes >= SAVED_PAGE_BUDGET
}

#[cfg(test)]
pub fn start(id: String, provider: String, queries: Vec<String>, pages: u8) -> Result<(), String> {
    start_with_freshness(None, id, provider, queries, pages, false).map(|_| ())
}

pub fn start_with_freshness(
    ws: Option<&Workspace>,
    id: String,
    provider: String,
    queries: Vec<String>,
    pages: u8,
    prefer_fresh: bool,
) -> Result<SearchStart, String> {
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
        let url = crate::web_search::request_url_with_freshness(&provider, &query, prefer_fresh)?;
        phrases.push((query, url));
    }
    // Cover every phrase's first page before spending the shared candidate
    // budget on deeper pages of any one phrase.
    let mut requests = Vec::new();
    for page in 1..=pages {
        for (query, url) in &phrases {
            let page_url = crate::web_search::page_url(url, page);
            requests.push((query.clone(), page, page_url));
        }
    }
    start_requests(ws, id, provider, requests, Vec::new(), prefer_fresh, None)
}

/// Start only explicitly requested query/page pairs, for a user-reviewed retry.
#[cfg(test)]
pub fn start_selected(
    id: String,
    provider: String,
    pairs: Vec<(String, u8)>,
) -> Result<(), String> {
    start_selected_with_history(id, provider, pairs, Vec::new())
}

#[cfg(test)]
pub fn start_selected_with_history(
    id: String,
    provider: String,
    pairs: Vec<(String, u8)>,
    prior_history: Vec<Value>,
) -> Result<(), String> {
    start_selected_with_freshness(None, id, provider, pairs, prior_history, false, None).map(|_| ())
}

pub fn start_selected_with_freshness(
    ws: Option<&Workspace>,
    id: String,
    provider: String,
    pairs: Vec<(String, u8)>,
    prior_history: Vec<Value>,
    prefer_fresh: bool,
    retry_source_task: Option<&str>,
) -> Result<SearchStart, String> {
    if crate::web_search::busy() {
        return Err("A web search is active".into());
    }
    if pairs.is_empty() || pairs.len() > 50 {
        return Err("Choose 1 to 50 search pages".into());
    }
    let mut seen = HashSet::new();
    let mut phrases = HashSet::new();
    let mut requests = Vec::new();
    for (query, page) in pairs {
        let query = query.trim().to_owned();
        if !(1..=5).contains(&page) || !seen.insert((query.to_lowercase(), page)) {
            return Err("Search pages must be distinct and numbered 1 to 5".into());
        }
        phrases.insert(query.to_lowercase());
        if phrases.len() > 10 {
            return Err("Choose at most 10 distinct search phrases".into());
        }
        let url = crate::web_search::request_url_with_freshness(&provider, &query, prefer_fresh)?;
        requests.push((query, page, crate::web_search::page_url(&url, page)));
    }
    start_requests(
        ws,
        id,
        provider,
        requests,
        prior_history,
        prefer_fresh,
        retry_source_task,
    )
}

fn start_requests(
    ws: Option<&Workspace>,
    id: String,
    provider: String,
    requests: Vec<(String, u8, url::Url)>,
    prior_history: Vec<Value>,
    prefer_fresh: bool,
    retry_source_task: Option<&str>,
) -> Result<SearchStart, String> {
    let mut task = TASK.lock().map_err(|_| "Web search unavailable")?;
    if task.is_some() {
        return Err("A batch search is running or awaiting its result".into());
    }
    let (_, _, first_url) = requests.first().ok_or("Choose a search page")?;
    if let Some(pause) = crate::web_search::provider_pause(ws, first_url)? {
        return Ok(SearchStart::Deferred(pause));
    }
    let initial_seen = seen_from_prior(&prior_history)?;
    if initial_seen.saved_bytes >= SAVED_PAGE_BUDGET {
        return Err("Earlier results fill the search history size limit; save or review them before starting a new search".into());
    }
    // Every planned page can enqueue at most one result. Leave one slot for
    // completion so collecting pages is independent of the HTTP/UI loop.
    // Both entry points bound the plan at 50 pages and size-limit the history.
    let total = requests.len();
    let plan: Vec<_> = requests
        .iter()
        .map(|(query, page, _)| (query.clone(), *page))
        .collect();
    let provider = provider.trim().to_owned();
    let journal = if let Some(ws) = ws {
        let metadata = SearchPlan {
            version: CURSOR_VERSION,
            plan: plan
                .iter()
                .map(|(query, page)| SearchPage {
                    query: query.clone(),
                    page: *page,
                })
                .collect(),
            provider: provider.clone(),
            prefer_fresh,
            retry_source_task: retry_source_task.map(str::to_owned),
        };
        let header = serde_json::to_vec(&metadata).map_err(|error| error.to_string())?;
        let prior = prior_history
            .iter()
            .map(serde_json::to_vec)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        ws.begin_web_search_run(&id, total, &header, &prior)
            .map_err(|error| format!("Could not save the search plan: {error}"))?
    } else {
        None
    };
    let durable = journal.is_some();
    let (tx, rx) = mpsc::sync_channel(total + 1);
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let timed_out = Arc::new(AtomicBool::new(false));
    let unreported = Arc::new(AtomicBool::new(false));
    let worker_timeout = timed_out.clone();
    let worker_unreported = unreported.clone();
    let worker = std::thread::Builder::new()
        .name("web-search".into())
        .spawn(move || {
            run_worker(
                requests,
                initial_seen,
                journal,
                tx,
                stop,
                worker_timeout,
                worker_unreported,
            );
        })
        .map_err(|error| format!("Could not start web search worker: {error}"))?;
    *task = Some(Task {
        id,
        rx,
        cancel,
        total,
        plan,
        provider: (provider.len() <= 2048).then_some(provider),
        processed: 0,
        prefer_fresh,
        pending: None,
        history: Vec::new(),
        prior_history,
        timed_out,
        unreported,
        worker: Some(worker),
    });
    Ok(SearchStart::Started {
        checkpoint_saved: Some(durable),
    })
}

fn run_worker(
    requests: Vec<(String, u8, url::Url)>,
    mut seen: BatchSeen,
    journal: Option<WebSearchWriter>,
    tx: SyncSender<Message>,
    stop: Arc<AtomicBool>,
    timed_out: Arc<AtomicBool>,
    unreported: Arc<AtomicBool>,
) {
    let total = requests.len();
    let mut saved_pages = 0;
    let mut limit_reached = false;
    let mut failure = None;
    let mut provider_stop = None;
    for (index, (query, page, url)) in requests.into_iter().enumerate() {
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
        let result = protected_provider_call(|| {
            crate::web_search::fetch(&url).and_then(|body| {
                crate::web_search::parse_provider(&body, &url, &seen.originals)
                    .map_err(FetchError::unusable_response)
            })
        });
        let mut stop_batch = false;
        let result = match result {
            Ok(mut data) => {
                limit_reached = retain_batch_budget(&mut data, &query, &mut seen);
                Ok(data)
            }
            Err(error) => {
                stop_batch = error.stop_batch;
                provider_stop = error.provider_stop;
                if let Err(storage_error) = crate::web_search::save_provider_pause(
                    journal.as_ref(),
                    &url,
                    provider_stop.as_ref(),
                ) {
                    failure = Some(Outcome::Failed {
                        error: format!("{}. {storage_error}", error.message),
                        fetched_unreported_pages: 1,
                    });
                    break;
                }
                Err(error.message)
            }
        };
        if let Some(journal) = &journal {
            let committed =
                serde_json::to_vec(&page_receipt(&query, page, index + 1, total, &result))
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| {
                        journal
                            .append(index, &bytes)
                            .map_err(|error| error.to_string())
                    });
            if let Err(error) = committed {
                failure = Some(Outcome::Failed {
                    error: format!("Could not save a fetched search page: {error}"),
                    fetched_unreported_pages: 1,
                });
                break;
            }
            saved_pages += 1;
        }
        if !send(
            &tx,
            Message::Query(query, page, result),
            &stop,
            &timed_out,
            Duration::from_secs(30),
        ) {
            unreported.store(journal.is_none(), Ordering::Release);
            break;
        }
        if limit_reached || stop_batch {
            break;
        }
    }
    let mut outcome = failure.unwrap_or(Outcome::Complete {
        cancelled: stop.load(Ordering::Acquire),
        limit_reached,
        provider_stop,
    });
    let mut saved = false;
    if let Some(journal) = journal {
        let result = serde_json::to_vec(&outcome)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                journal
                    .finish(saved_pages, &bytes)
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => saved = true,
            Err(error) => {
                let (message, fetched_unreported_pages) = match outcome {
                    Outcome::Failed {
                        error: first,
                        fetched_unreported_pages,
                    } => (
                        format!("{first}. Could not save the search outcome: {error}"),
                        fetched_unreported_pages,
                    ),
                    Outcome::Complete { .. } => {
                        (format!("Could not save the search outcome: {error}"), 0)
                    }
                };
                outcome = Outcome::Failed {
                    error: message,
                    fetched_unreported_pages,
                };
            }
        }
    }
    let _ = send(
        &tx,
        Message::Done(outcome, saved),
        &stop,
        &timed_out,
        Duration::from_secs(30),
    );
}
fn poll_task(id: &str, acknowledged: usize) -> Result<Option<Value>, String> {
    let mut tasks = TASK.lock().map_err(|_| "Web search unavailable")?;
    let Some(task) = tasks.as_mut().filter(|t| t.id == id) else {
        return Ok(COMPLETED
            .lock()
            .map_err(|_| "Web search unavailable")?
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone()));
    };
    if acknowledged > task.processed {
        return Err("Invalid batch acknowledgement".into());
    }
    if acknowledged
        < task
            .processed
            .saturating_sub(usize::from(task.pending.is_some()))
    {
        return Ok(Some(json!({"status":"running","processed":task.processed,
            "total":task.total,"history":task.history})));
    }
    if let Some(pending) = &task.pending {
        if acknowledged < task.processed {
            return Ok(Some(pending.clone()));
        }
        task.pending = None;
    }
    let (mut outcome, mut saved) = match task.rx.try_recv() {
        Ok(Message::Query(query, page, result)) => {
            task.processed += 1;
            let response = page_receipt(&query, page, task.processed, task.total, &result);
            task.pending = Some(response.clone());
            task.history.push(response.clone());
            return Ok(Some(response));
        }
        Ok(Message::Done(outcome, saved)) => (outcome, saved),
        Err(TryRecvError::Empty) => {
            return Ok(Some(json!({"status":"running","processed":task.processed,
                "total":task.total})));
        }
        Err(TryRecvError::Disconnected) => {
            if task.cancel.load(Ordering::Acquire) {
                (
                    Outcome::Complete {
                        cancelled: true,
                        limit_reached: false,
                        provider_stop: None,
                    },
                    false,
                )
            } else {
                let error = if task.timed_out.load(Ordering::Acquire) {
                    "Batch search stopped because results were not collected within 30 seconds"
                } else {
                    "Batch search stopped before completion"
                };
                (
                    Outcome::Failed {
                        error: error.into(),
                        fetched_unreported_pages: usize::from(
                            task.unreported.load(Ordering::Acquire),
                        ),
                    },
                    false,
                )
            }
        }
    };
    let task = tasks.take().expect("matched search task");
    if task.worker.is_some_and(|worker| worker.join().is_err()) {
        outcome = Outcome::Failed {
            error: "Batch search worker terminated unexpectedly".into(),
            fetched_unreported_pages: 0,
        };
        saved = false;
    }
    let mut response = terminal_result(&outcome, task.total, &task.history, saved);
    response["history"] = json!(task.history);
    response["prior_history"] = json!(task.prior_history);
    response["plan"] = json!(task
        .plan
        .iter()
        .map(|(query, page)| json!({"query":query,"page":page}))
        .collect::<Vec<_>>());
    response["provider"] = json!(task.provider);
    response["prefer_fresh"] = json!(task.prefer_fresh);
    *COMPLETED.lock().map_err(|_| "Web search unavailable")? =
        Some((id.to_owned(), response.clone()));
    Ok(Some(response))
}

#[cfg(test)]
pub fn poll(id: &str, acknowledged: usize) -> Result<Value, String> {
    poll_task(id, acknowledged)?.ok_or_else(|| "Batch web search not found".into())
}

pub fn status(ws: &Workspace, id: &str, acknowledged: usize) -> Result<Value, String> {
    if let Some(result) = poll_task(id, acknowledged)? {
        return Ok(result);
    }
    saved_search(ws)?
        .filter(|(task_id, _)| task_id == id)
        .map(|(_, result)| result)
        .ok_or_else(|| "Batch web search not found".into())
}

/// Advance client delivery without requiring an open browser tab.
/// The worker has already committed the page independently.
pub fn collect_background() -> Result<(), String> {
    let task = TASK
        .lock()
        .map_err(|_| "Web search unavailable")?
        .as_ref()
        .map(|task| (task.id.clone(), task.processed));
    if let Some((id, processed)) = task {
        poll_task(&id, processed)?;
    }
    Ok(())
}

pub fn pending_with_saved(ws: &Workspace) -> Result<Value, String> {
    let mut current = pending()?;
    if current["active"] == false && current.get("last").is_none() {
        if let Some((id, result)) = saved_search(ws)? {
            current["last"] = json!({"task_id":id,"result":result});
        }
    }
    Ok(current)
}

/// The worker owns the final checkpoint. Drain its connection and lock on exit.
pub fn shutdown() -> Result<(), String> {
    let task = TASK.lock().map_err(|_| "Web search unavailable")?.take();
    if let Some(task) = task {
        task.cancel.store(true, Ordering::Release);
        drop(task.rx);
        if task.worker.is_some_and(|worker| worker.join().is_err()) {
            return Err("Batch search worker terminated unexpectedly".into());
        }
    }
    Ok(())
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

    fn done(limit_reached: bool) -> Message {
        Message::Done(
            Outcome::Complete {
                cancelled: false,
                limit_reached,
                provider_stop: None,
            },
            false,
        )
    }

    #[test]
    fn provider_panic_stops_batch_and_preserves_failure_accounting() {
        let error = protected_provider_call(|| panic!("fictional provider failure")).unwrap_err();
        assert!(error.stop_batch);
        assert_eq!(error.message, "Search stopped unexpectedly");
        let report = coverage(4, &[json!({"error":error.message})]);
        assert_eq!(report["failed_pages"], 1);
        assert_eq!(report["unattempted_pages"], 3);
        let error = protected_provider_call(|| Err("temporary".to_owned().into())).unwrap_err();
        assert!(!error.stop_batch);
    }

    #[test]
    fn retry_keeps_unique_urls_beyond_the_old_batch_count() {
        let _lock = TEST_LOCK.lock().unwrap();
        let prior_results: Vec<_> = (0..599)
            .map(|i| json!({"url":format!("https://example.org/job/{i}")}))
            .collect();
        let prior = vec![json!({"data":{"results":prior_results}})];
        let mut seen = seen_from_prior(&prior).unwrap();
        assert_eq!(seen.originals.len(), 599);
        let mut data = json!({"results":[
            {"url":"https://example.org/job/0"},
            {"url":"https://example.org/new"},
            {"url":"https://example.org/over-limit"}
        ]});
        assert!(!retain_batch_budget(&mut data, "role", &mut seen));
        assert_eq!(data["results"].as_array().unwrap().len(), 2);
        assert_eq!(data["results"][0]["url"], "https://example.org/new");
        assert_eq!(data["exact_repeated_omitted"], 1);
        assert!(data["batch_limit_omitted"].is_null());
        assert_eq!(seen.originals.len(), 601);
        let prior = vec![json!({"data":{"results":[{"url":"https://example.org/job/0"}]}})];
        let mut seen = seen_from_prior(&prior).unwrap();
        let mut page = crate::web_search::parse_excluding(
            r#"{"results":[{"url":"https://example.org/job/0","title":"Repeated"},{"url":"https://example.org/new","title":"New"}]}"#,
            &seen.originals,
        ).unwrap();
        assert!(!retain_batch_budget(&mut page, "role", &mut seen));
        assert_eq!(page["results"][0]["url"], "https://example.org/new");
        assert_eq!(
            page["repeated_references"][0]["url"],
            "https://example.org/job/0"
        );
        assert!(
            seen_from_prior(&[json!({"data":{"results":[{"url":"http://127.0.0.1/"}]}})]).is_err()
        );
    }
    use std::io::{Read, Write};
    use std::time::Instant;
    #[cfg(windows)]
    fn search_test_dir() -> tempfile::TempDir {
        let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        tempfile::tempdir_in(target).unwrap()
    }

    #[cfg(windows)]
    fn search_workspace(path: &std::path::Path) -> Workspace {
        #[cfg(feature = "sqlcipher")]
        {
            Workspace::open_encrypted(path).unwrap()
        }
        #[cfg(not(feature = "sqlcipher"))]
        {
            Workspace::open(path).unwrap()
        }
    }

    #[cfg(windows)]
    #[test]
    fn legacy_history_is_recovered_after_workspace_reopens() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = search_test_dir();
        let expected = json!({
            "status":"complete","history":[{"query":"fictional","page":1,
                "data":{"results":[{"url":"https://example.test/job"}]}}]
        });
        {
            let ws = search_workspace(dir.path());
            ws.save_latest_web_search("fixture-restart", &serde_json::to_vec(&expected).unwrap())
                .unwrap();
        }
        *COMPLETED.lock().unwrap() = None;
        let ws = search_workspace(dir.path());
        let recovered = pending_with_saved(&ws).unwrap();
        assert_eq!(recovered["last"]["task_id"], "fixture-restart");
        assert_eq!(recovered["last"]["result"], expected);
    }

    #[cfg(windows)]
    #[test]
    fn interrupted_batch_recovers_committed_pages_and_retries_without_client_polling() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = search_test_dir();
        let earlier: Vec<_> = (0..50)
            .map(|index| {
                json!({
                    "query":format!("earlier {index}"),"page":1,
                    "data":{"results":[{"url":format!("https://example.test/earlier/{index}")}]}
                })
            })
            .collect();
        {
            let ws = search_workspace(dir.path());
            ws.save_latest_web_search("older", br#"{"status":"complete","history":[]}"#)
                .unwrap();
            let metadata = json!({
                "version":CURSOR_VERSION, "provider":"https://freeserp.ai/",
                "prefer_fresh":true,"retry_source_task":"older",
                "plan":[{"query":"fictional","page":2},{"query":"other","page":3}]
            });
            let writer = ws
                .begin_web_search_run(
                    "active-fixture",
                    2,
                    &serde_json::to_vec(&metadata).unwrap(),
                    &earlier
                        .iter()
                        .map(|page| serde_json::to_vec(page).unwrap())
                        .collect::<Vec<_>>(),
                )
                .unwrap()
                .unwrap();
            let initial = saved_search(&ws).unwrap().unwrap().1;
            assert_eq!(initial["cursor"]["committed_pages"], 0);
            assert_eq!(initial["provider"], "https://freeserp.ai/");
            assert_eq!(initial["history"], json!([]));
            let page = page_receipt(
                "fictional",
                2,
                1,
                2,
                &Ok(json!({
                    "results":[{"url":"https://example.test/job"}]
                })),
            );
            writer
                .append(0, &serde_json::to_vec(&page).unwrap())
                .unwrap();
        }
        *COMPLETED.lock().unwrap() = None;
        let ws = search_workspace(dir.path());
        let recovered = pending_with_saved(&ws).unwrap();
        let result = &recovered["last"]["result"];
        assert_eq!(recovered["last"]["task_id"], "active-fixture");
        assert_eq!(result["status"], "failed");
        assert_eq!(result["recovered_partial"], true);
        assert_eq!(result["cursor"], json!({"version":1,"committed_pages":1}));
        assert_eq!(result["coverage"]["unattempted_pages"], 1);
        assert_eq!(result["prefer_fresh"], true);
        assert_eq!(result["retry_source_task"], "older");
        assert_eq!(result["history"][0]["page"], 2);
        assert_eq!(status(&ws, "active-fixture", 0).unwrap(), *result);
        let prior = previous_successful_history(&ws, "active-fixture").unwrap();
        assert_eq!(prior.len(), 51);
        assert_eq!(prior[..50], earlier);
        assert_eq!(prior[50]["page"], 2);
        assert!(previous_successful_history(&ws, "older").is_err());

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let n = stream.read(&mut request).unwrap();
            let request = std::str::from_utf8(&request[..n]).unwrap();
            assert!(request.contains("q=other") && request.contains("pageno=3"));
            let body = r#"{"results":[{"url":"https://example.test/next","title":"Fictional"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        assert!(matches!(
            start_selected_with_freshness(
                Some(&ws),
                "retry-fixture".into(),
                format!("http://127.0.0.1:{port}/"),
                vec![("other".into(), 3)],
                prior,
                false,
                Some("active-fixture"),
            )
            .unwrap(),
            SearchStart::Started {
                checkpoint_saved: Some(true)
            }
        ));
        provider.join().unwrap();
        let worker = TASK.lock().unwrap().take().unwrap();
        // Keep the receiver alive while waiting. No polling or collection advances the ledger.
        worker.worker.unwrap().join().unwrap();
        drop(ws);
        let reopened = search_workspace(dir.path());
        let saved = pending_with_saved(&reopened).unwrap();
        let final_result = &saved["last"]["result"];
        assert_eq!(final_result["status"], "complete");
        assert_eq!(final_result["history_saved"], true);
        assert_eq!(final_result["cursor"]["committed_pages"], 1);
        assert_eq!(final_result["prior_history"].as_array().unwrap().len(), 51);
        assert_eq!(final_result["history"][0]["page"], 3);
        assert_eq!(
            final_result["history"][0]["data"]["results"][0]["url"],
            "https://example.test/next"
        );
        assert_eq!(
            final_result["prior_history"][50]["data"]["results"][0]["url"],
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
            prefer_fresh: false,
            worker: None,
            pending: None,
            history: Vec::new(),
            prior_history: Vec::new(),
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
        tx.send(done(false)).unwrap();
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
            prefer_fresh: false,
            worker: None,
            pending: None,
            history: Vec::new(),
            prior_history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        let (finished, result) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let sent = send(
                &tx,
                done(false),
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
            prefer_fresh: false,
            worker: None,
            pending: None,
            history: Vec::new(),
            prior_history: Vec::new(),
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
    fn search_history_size_reports_links_that_cannot_be_saved() {
        let mut seen = BatchSeen {
            saved_bytes: SAVED_PAGE_BUDGET - 4096,
            ..Default::default()
        };
        let mut page = json!({"results":[
            {"url":"https://example.org/one"},
            {"url":"https://example.org/large","snippet":"🗺".repeat(1000)},
            {"url":"https://example.org/two"}
        ],"limited":false});
        assert!(retain_batch_budget(&mut page, "role", &mut seen));
        assert_eq!(seen.originals.len(), 2);
        assert!(!seen.originals.contains("https://example.org/large"));
        assert_eq!(page["results"].as_array().unwrap().len(), 2);
        assert_eq!(page["batch_limit_omitted"], 1);
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
            prefer_fresh: false,
            worker: None,
            pending: None,
            history: Vec::new(),
            prior_history: Vec::new(),
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
        tx.send(done(true)).unwrap();
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
                assert_eq!(result["provider"], format!("http://127.0.0.1:{port}/"));
                assert_eq!(
                    result["plan"],
                    json!([
                        {"query":"first role","page":1}, {"query":"second role","page":1},
                        {"query":"first role","page":2}, {"query":"second role","page":2}
                    ])
                );
                assert!(result["history"][0]["error"].is_string());
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
    fn unusable_provider_response_stops_remaining_pages() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = listener.try_clone().unwrap();
        let provider = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = "<html>Search is unavailable</html>";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        start(
            "invalid-provider".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["first role".into(), "second role".into()],
            2,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut acknowledged = 0;
        loop {
            let result = poll("invalid-provider", acknowledged).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["coverage"]["attempted_pages"], 1);
                assert_eq!(result["coverage"]["failed_pages"], 1);
                assert_eq!(result["coverage"]["unattempted_pages"], 3);
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
            prefer_fresh: false,
            worker: None,
            pending: None,
            history: Vec::new(),
            prior_history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        tx.send(Message::Query("role".into(), 1, Ok(json!({"results":[]}))))
            .unwrap();
        tx.send(done(false)).unwrap();
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
            prefer_fresh: false,
            worker: None,
            pending: None,
            history: Vec::new(),
            prior_history: Vec::new(),
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
            prefer_fresh: false,
            worker: None,
            pending: None,
            history: Vec::new(),
            prior_history: Vec::new(),
            timed_out: Arc::new(AtomicBool::new(false)),
            unreported: Arc::new(AtomicBool::new(false)),
        });
        tx.send(Message::Query("role".into(), 1, Ok(json!({"results":[]}))))
            .unwrap();
        tx.send(Message::Query("role".into(), 2, Ok(json!({"results":[]}))))
            .unwrap();
        tx.send(done(false)).unwrap();
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
    fn batch_retains_tracking_variants_without_losing_job_identity() {
        let mut seen = BatchSeen::default();
        let mut page = json!({"results":[
            {"url":"https://example.org/job?id=1&utm_source=first"},
            {"url":"https://example.org/job?id=1&utm_source=second"},
            {"url":"https://example.org/job?id=2&utm_source=second"},
            {"url":"https://example.org/job?id=1#other-role"},
            {"url":"https://example.org/job?id=1&ref=other"}
        ]});
        assert!(!retain_batch_budget(&mut page, "role", &mut seen));
        assert_eq!(page["results"].as_array().unwrap().len(), 5);
        assert_eq!(
            page["results"][0]["url"],
            "https://example.org/job?id=1&utm_source=first"
        );
        assert_eq!(page["possible_tracking_variants"], 1);
        assert_eq!(page["results"][1]["possible_tracking_variant"], true);
        assert_eq!(
            page["results"][1]["possible_tracking_variant_of"],
            "https://example.org/job?id=1&utm_source=first"
        );
        assert!(page["results"][2]["possible_tracking_variant"].is_null());
    }

    #[test]
    fn tracking_variants_survive_provider_pages_and_batch_recovery() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            for suffix in ["first", "second"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let received = stream.read(&mut request).unwrap();
                assert!(received > 0);
                let body = json!({"results":[{"title":"Fictional role","url":format!("https://example.org/job?id=1&utm_source={suffix}")}]}).to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        start(
            "tracking-recovery".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["one".into(), "two".into()],
            1,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut pages = Vec::new();
        loop {
            let result = poll("tracking-recovery", pages.len()).unwrap();
            if result["status"] == "complete" {
                let history = result["history"].as_array().unwrap();
                assert_eq!(history.len(), 2);
                assert_eq!(
                    history[1]["data"]["results"][0]["possible_tracking_variant_of"],
                    "https://example.org/job?id=1&utm_source=first"
                );
                break;
            }
            if result.get("data").is_some() {
                pages.push(result);
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(pages.len(), 2);
        assert_eq!(
            pages[1]["data"]["results"][0]["possible_tracking_variant"],
            true
        );
        assert_eq!(pages[1]["data"]["possible_tracking_variants"], 1);
        provider.join().unwrap();
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
    #[ignore = "sends fictional search phrases to the public FreeSerp API"]
    fn live_freeserp_batch_searches_multiple_phrases_and_pages() {
        let _lock = TEST_LOCK.lock().unwrap();
        start(
            "freeserp-batch-evaluation".into(),
            "https://freeserp.ai/".into(),
            vec![
                std::env::var("WAYPOINT_SEARCH_FIXTURE_QUERY_1")
                    .unwrap_or_else(|_| "microscopy technician careers".into()),
                std::env::var("WAYPOINT_SEARCH_FIXTURE_QUERY_2")
                    .unwrap_or_else(|_| "payroll accountant careers".into()),
            ],
            2,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut pages = 0;
        let mut retained = 0;
        let mut errors = Vec::new();
        loop {
            let result = poll("freeserp-batch-evaluation", pages).unwrap();
            if result["status"] == "complete" {
                println!(
                    "coverage: {}; retained: {retained}; errors: {errors:?}",
                    result["coverage"]
                );
                assert_eq!(result["coverage"]["succeeded_pages"], 4);
                assert_eq!(result["coverage"]["failed_pages"], 0);
                break;
            }
            assert_ne!(result["status"], "failed", "{result}");
            if result.get("query").is_some() {
                if let Some(error) = result["error"].as_str() {
                    errors.push(error.to_owned());
                } else {
                    assert_eq!(result["data"]["provider"], "freeserp");
                    let count = result["data"]["results"].as_array().unwrap().len();
                    retained += count;
                    println!(
                        "query={} page={} retained={count}",
                        result["query"], result["page"]
                    );
                    for candidate in result["data"]["results"].as_array().unwrap().iter().take(3) {
                        println!(
                            "candidate: {} {}",
                            candidate["title"].as_str().unwrap_or(""),
                            candidate["url"].as_str().unwrap_or("")
                        );
                    }
                }
                pages += 1;
            }
            assert!(Instant::now() < deadline, "public search did not finish");
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(pages, 4);
        assert!(retained > 0, "no candidates were returned");
    }

    #[test]
    #[ignore = "sends two non-personal search phrases to the public FreeSerp API"]
    fn live_freeserp_recency_search_multiple_languages() {
        let _lock = TEST_LOCK.lock().unwrap();
        start_with_freshness(
            None,
            "recency-evaluation".into(),
            "https://freeserp.ai/".into(),
            vec![
                "bioinformatics jobs Berlin".into(),
                "analista financiero empleo Bogota".into(),
            ],
            1,
            true,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut pages = 0;
        let mut retained = 0;
        loop {
            let result = poll("recency-evaluation", pages).unwrap();
            if result["status"] == "complete" {
                println!("coverage: {}; retained: {retained}", result["coverage"]);
                assert_eq!(result["coverage"]["succeeded_pages"], 2);
                assert_eq!(result["coverage"]["failed_pages"], 0);
                break;
            }
            assert_ne!(result["status"], "failed", "{result}");
            if result.get("query").is_some() {
                assert!(result.get("error").is_none(), "{result}");
                assert_eq!(result["data"]["provider"], "freeserp");
                assert_eq!(result["data"]["freshness_preferred"], true);
                retained += result["data"]["results"].as_array().unwrap().len();
                pages += 1;
            }
            assert!(Instant::now() < deadline, "public search did not finish");
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(pages, 2);
        assert!(retained > 0, "no candidates were returned");
    }

    #[test]
    fn batch_visits_each_phrase_before_deeper_pages() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (finished, responses) = mpsc::channel();
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
            finished.send(()).unwrap();
        });
        start(
            "fair-pages".into(),
            format!("http://127.0.0.1:{port}/"),
            vec!["first".into(), "second".into()],
            2,
        )
        .unwrap();
        let collected_without_polling = responses.recv_timeout(Duration::from_secs(8)).is_ok();
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
        assert!(
            collected_without_polling,
            "client polling stalled the search worker"
        );
    }

    #[test]
    fn selected_retry_requests_only_the_chosen_pages() {
        let _lock = TEST_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let provider = std::thread::spawn(move || {
            for (query, page) in [("first", 2), ("second", 1)] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let n = stream.read(&mut request).unwrap();
                let request = std::str::from_utf8(&request[..n]).unwrap();
                assert!(request.contains(&format!("q={query}")));
                assert!(request.contains(&format!("pageno={page}")));
                let body = r#"{"results":[]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            listener.set_nonblocking(true).unwrap();
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        });
        let url = format!("http://127.0.0.1:{port}/");
        assert!(start_selected("invalid".into(), url.clone(), vec![("first".into(), 0)]).is_err());
        assert!(start_selected(
            "invalid".into(),
            url.clone(),
            vec![("first".into(), 1), ("FIRST".into(), 1)]
        )
        .is_err());
        start_selected(
            "selected-pages".into(),
            url,
            vec![("first".into(), 2), ("second".into(), 1)],
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut acknowledged = 0;
        loop {
            let result = poll("selected-pages", acknowledged).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["coverage"]["succeeded_pages"], 2);
                assert_eq!(
                    result["plan"],
                    json!([{"query":"first","page":2},{"query":"second","page":1}])
                );
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
