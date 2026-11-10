//! Background public-page copies. No candidate-entry driver or delivery path.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use waypoint_domain::JobIdentityId;
use waypoint_workspace::Workspace;

pub const WORKER_FLAG: &str = "--job-source-worker";
const CANCELLED: &str = "Page capture was cancelled; partial files were retained";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    task_id: String,
    url: String,
    source: String,
    destination: PathBuf,
}
struct Task {
    id: String,
    job: JobIdentityId,
    workspace: PathBuf,
    source_url: String,
    record: crate::attempt_record::AttemptRecord,
    root: PathBuf,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Value, String>>,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, PathBuf, Value, Instant)>> = Mutex::new(None);

fn checked_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn busy() -> bool {
    TASK.lock().map(|t| t.is_some()).unwrap_or(true)
}
fn public_url(raw: &str, profile: &Value) -> Result<(String, String), String> {
    let url = url::Url::parse(raw).map_err(|_| "Invalid source page URL")?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || !waypoint_connectors::is_public_host_url(url.as_str())
    {
        return Err("Source page needs a public HTTPS URL".into());
    }
    let decoded = waypoint_browser::capture_privacy::checked_reference(url.as_str())
        .map_err(|_| "Source page URL is private, authenticated or needs review")?;
    if crate::search_planner::contains_protected(
        &decoded,
        &crate::search_planner::protected_identifiers(profile),
    ) {
        return Err(
            "Source page URL contains personal information; use a public job listing".into(),
        );
    }
    let source =
        crate::manual_job::source(url.host_str().ok_or("Source page has no host")?).to_owned();
    waypoint_workspace::default_registry()
        .check_public_route(&source, waypoint_policy::Operation::Read)
        .map_err(|e| e.to_string())?;
    Ok((url.to_string(), source))
}
pub fn start(ws: &Workspace, job: &JobIdentityId, id: String) -> Result<(), String> {
    if !checked_id(&id) {
        return Err("Invalid page-copy identity".into());
    }
    let mut active = TASK.lock().map_err(|_| "Page capture unavailable")?;
    if active.is_some() {
        return Err("Another page copy is running".into());
    }
    let workspace = ws
        .db_path
        .clone()
        .ok_or("Page copies require a saved workspace")?;
    let selected = ws
        .job(job)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    waypoint_workspace::default_registry()
        .check_public_route(&selected.source, waypoint_policy::Operation::Read)
        .map_err(|e| e.to_string())?;
    let profile: Value = ws
        .candidate_profile()
        .map_err(|e| e.to_string())?
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|_| "Stored profile is invalid")?
        .unwrap_or_else(|| json!({}));
    let (url, source) = public_url(
        selected
            .canonical_url
            .as_deref()
            .ok_or("Job has no source URL")?,
        &profile,
    )?;
    let mut record = crate::attempt_record::AttemptRecord::start(ws, job, &id, crate::now())?
        .ok_or("Page copies require a saved workspace")?;
    let root = record.source_capture_directory();
    std::fs::create_dir(&root).map_err(|_| "Cannot create page-copy folder")?;
    record.event(json!({"type":"source_capture_started","time":crate::now(),"task_id":id,"scope":"anonymous_public_page_copy"}))?;
    let request = Request {
        task_id: id.clone(),
        url: url.clone(),
        source,
        destination: root.clone(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let child_cancel = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("job-page-copy".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_request(request, child_cancel)
            }))
            .unwrap_or_else(|_| Err("Page capture stopped unexpectedly".into()));
            let _ = tx.send(result);
        })
        .map_err(|_| "Cannot start page capture")?;
    drop(spawned);
    *active = Some(Task {
        id,
        job: job.clone(),
        workspace,
        source_url: url,
        record,
        root,
        cancel,
        rx,
        result: None,
    });
    Ok(())
}
pub fn pending(ws: &Workspace) -> Result<Value, String> {
    let active = TASK.lock().map_err(|_| "Page capture unavailable")?;
    if let Some(task) = active
        .as_ref()
        .filter(|t| ws.db_path.as_ref() == Some(&t.workspace))
    {
        return Ok(json!({"active":true,"task_id":task.id,"job_id":task.job.as_str()}));
    }
    let completed = COMPLETED.lock().map_err(|_| "Page capture unavailable")?;
    if let Some((_, path, value, at)) = completed.as_ref().filter(|(_, path, _, at)| {
        ws.db_path.as_ref() == Some(path) && at.elapsed() < Duration::from_secs(600)
    }) {
        let _ = (path, at);
        return Ok(json!({"active":false,"last":with_current_source(ws,value.clone())}));
    }
    Ok(json!({"active":false}))
}
pub fn cancel(ws: &Workspace, id: &str) -> Result<Value, String> {
    let active = TASK.lock().map_err(|_| "Page capture unavailable")?;
    let task = active
        .as_ref()
        .filter(|t| t.id == id && ws.db_path.as_ref() == Some(&t.workspace))
        .ok_or("Page copy not found")?;
    task.cancel.store(true, Ordering::Release);
    Ok(json!({"ok":true}))
}
fn progress(root: &Path, id: &str) -> Value {
    use std::io::Read;
    for phase in ["saved", "resources", "reading", "tools"] {
        if let Ok(file) = std::fs::File::open(root.join(format!("phase-{phase}.json"))) {
            let mut bytes = Vec::new();
            if file.take(4097).read_to_end(&mut bytes).is_ok() && bytes.len() <= 4096 {
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                    if value["task_id"] == id && value["phase"] == phase {
                        return json!(phase);
                    }
                }
            }
        }
    }
    json!("starting")
}
pub fn poll(ws: &Workspace, id: &str) -> Result<Value, String> {
    let mut active = TASK.lock().map_err(|_| "Page capture unavailable")?;
    let Some(task) = active
        .as_mut()
        .filter(|t| t.id == id && ws.db_path.as_ref() == Some(&t.workspace))
    else {
        let completed = COMPLETED.lock().map_err(|_| "Page capture unavailable")?;
        return completed
            .as_ref()
            .filter(|(key, path, _, at)| {
                key == id
                    && ws.db_path.as_ref() == Some(path)
                    && at.elapsed() < Duration::from_secs(600)
            })
            .map(|(_, _, result, _)| with_current_source(ws, result.clone()))
            .ok_or("Page copy not found".into());
    };
    let result = if let Some(result) = task.result.as_ref() {
        result.clone()
    } else {
        match task.rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => {
                return Ok(
                    json!({"status":"running","task_id":id,"job_id":task.job.as_str(),"phase":progress(&task.root,id),"cancelling":task.cancel.load(Ordering::Acquire)}),
                )
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Page capture stopped unexpectedly".into())
            }
            Ok(result) => result,
        }
    };
    task.result = Some(result.clone());
    let value = finalize(ws, task, result)?;
    *COMPLETED.lock().map_err(|_| "Page capture unavailable")? = Some((
        id.to_owned(),
        task.workspace.clone(),
        value.clone(),
        Instant::now(),
    ));
    *active = None;
    Ok(value)
}

fn with_current_source(ws: &Workspace, mut value: Value) -> Value {
    let matches = (|| {
        let job = JobIdentityId::new(value["job_id"].as_str()?).ok()?;
        let raw = ws.job(&job).ok()??.canonical_url?;
        let url = url::Url::parse(&raw).ok()?;
        Some(
            value["source_url_sha256"].as_str()?
                == waypoint_domain::sha256_hex(url.as_str().as_bytes()),
        )
    })();
    value["current_source"] = json!(matches);
    value
}

fn finalize(
    ws: &Workspace,
    task: &mut Task,
    result: Result<Value, String>,
) -> Result<Value, String> {
    let id = task.id.clone();
    let current = ws
        .job(&task.job)
        .map_err(|e| e.to_string())?
        .and_then(|j| j.canonical_url)
        .and_then(|raw| url::Url::parse(&raw).ok())
        .is_some_and(|url| url.as_str() == task.source_url);
    let mut value = match result {
        Ok(result) => {
            json!({"status":"complete","task_id":id,"job_id":task.job.as_str(),"attempt_id":id,"current_source":current,"result":result})
        }
        Err(error) => {
            json!({"status":if error==CANCELLED{"cancelled"}else{"failed"},"task_id":id,"job_id":task.job.as_str(),"attempt_id":id,"error":error})
        }
    };
    value["source_url_sha256"] = json!(waypoint_domain::sha256_hex(task.source_url.as_bytes()));
    let event=task.record.event(json!({"type":"source_capture_result","time":crate::now(),"task_id":id,"status":value["status"],"current_source":current,"result":value["result"]}));
    let summary_saved = task.record.source_capture_result(&value).is_ok();
    let status_saved = task.record.status("manual_review", crate::now()).is_ok();
    let saved = event.is_ok() && summary_saved && status_saved;
    value["record_saved"] = json!(saved);
    if value["status"] != "complete"
        || value["result"]["needs_review"] == true
        || !current
        || !saved
    {
        value["review_flag_saved"] = json!(ws
            .mark_manual_review(&task.job, "Saved page copy needs review", crate::now())
            .is_ok());
    }
    Ok(value)
}

#[cfg(any(windows, test))]
fn host_reserve(free: u64, total: u64) -> Result<(), String> {
    if total == 0 || free > total || free < 3 * 1024u64.pow(3) {
        return Err(
            "Page capture stopped to preserve host RAM; partial files were retained".into(),
        );
    }
    Ok(())
}
#[cfg(any(windows, test))]
fn memory_limit(free: u64, total: u64) -> Result<u64, String> {
    let gib = 1024u64.pow(3);
    if total == 0 || free > total || free < 5 * gib {
        return Err(
            "Page capture needs at least 5 GiB free RAM to keep a 3 GiB host reserve".into(),
        );
    }
    Ok(free.saturating_sub(3 * gib).min(4 * gib))
}

#[cfg(windows)]
fn run_request(request: Request, cancel: Arc<AtomicBool>) -> Result<Value, String> {
    use processkit::{Command, OutputBufferPolicy, ProcessGroup, ProcessGroupOptions, Stdin};
    let mut sys = sysinfo::System::new().map_err(|_| "Cannot sample memory for page capture")?;
    sys.refresh_memory();
    let memory = memory_limit(sys.available_memory(), sys.total_memory())?;
    let input = serde_json::to_vec(&request).map_err(|_| "Cannot prepare page capture")?;
    let exe = std::env::current_exe().map_err(|_| "Cannot locate the application worker")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot start page capture runtime")?;
    runtime.block_on(async {
        let group = ProcessGroup::with_options(ProcessGroupOptions::default().max_memory(memory))
            .map_err(|_| "Cannot contain page capture")?;
        let command = Command::new(exe)
            .args([WORKER_FLAG])
            .create_no_window()
            .stdin(Stdin::from_bytes(input))
            .timeout(Duration::from_secs(1500))
            .output_buffer(OutputBufferPolicy::fail_loud(usize::MAX).with_max_bytes(256 * 1024));
        let process = group
            .start(&command)
            .await
            .map_err(|_| "Cannot start page capture worker")?;
        let future = process.output_bytes();
        tokio::pin!(future);
        let mut memory_checked = Instant::now();
        let output = loop {
            let reserve_error = if memory_checked.elapsed() >= Duration::from_millis(500) {
                sys.refresh_memory();
                memory_checked = Instant::now();
                host_reserve(sys.available_memory(), sys.total_memory()).err()
            } else {
                None
            };
            let stop_reason = if cancel.load(Ordering::Acquire) {
                Some(CANCELLED.to_owned())
            } else {
                reserve_error
            };
            if let Some(reason) = stop_reason {
                group
                    .kill_all()
                    .map_err(|_| "Could not stop the owned capture process")?;
                let _stopped = tokio::time::timeout(Duration::from_secs(5), future.as_mut())
                    .await
                    .map_err(|_| "Capture shutdown is unconfirmed; partial files were retained")?
                    .map_err(|_| "Capture shutdown failed")?;
                return Err(reason);
            }
            if let Ok(result) =
                tokio::time::timeout(Duration::from_millis(150), future.as_mut()).await
            {
                break result.map_err(|_| "Page capture worker failed")?;
            }
        };
        if output.timed_out() {
            return Err("Page capture timed out; partial files were retained".into());
        }
        if output.truncated() {
            return Err("Page capture worker could not finish; partial files were retained".into());
        }
        let value: Value =
            serde_json::from_slice(output.stdout()).map_err(|_| "Invalid page capture result")?;
        if value["task_id"] != request.task_id {
            return Err("Page capture result does not match the request".into());
        }
        if !output.is_success() {
            return Err(value["error"]
                .as_str()
                .unwrap_or("Page capture worker failed")
                .chars()
                .take(1000)
                .collect());
        }
        Ok(value)
    })
}
#[cfg(not(windows))]
fn run_request(_: Request, _: Arc<AtomicBool>) -> Result<Value, String> {
    Err("Background page capture requires Windows".into())
}

#[cfg(windows)]
fn phase(request: &Request, name: &str) -> Result<(), String> {
    let bytes = serde_json::to_vec(&json!({"task_id":request.task_id,"phase":name}))
        .map_err(|_| "Cannot encode capture progress")?;
    std::fs::write(
        request.destination.join(format!("phase-{name}.json")),
        bytes,
    )
    .map_err(|_| "Cannot save capture progress".into())
}
#[cfg(windows)]
fn inventory(root: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::os::windows::fs::MetadataExt;
    let mut pending = vec![root.to_owned()];
    let mut files = std::collections::BTreeMap::new();
    let mut total = 0u64;
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).map_err(|_| "Cannot inspect the background browser")? {
            let path = entry.map_err(|_| "Cannot inspect browser files")?.path();
            let meta =
                std::fs::symlink_metadata(&path).map_err(|_| "Cannot inspect browser files")?;
            if meta.file_attributes() & 0x400 != 0 {
                return Err("Browser files contain a reparse point".into());
            }
            if meta.is_dir() {
                pending.push(path);
                if pending.len() > 4096 {
                    return Err("Browser directory limit exceeded".into());
                }
                continue;
            }
            if !meta.is_file() {
                return Err("Unexpected background browser file".into());
            }
            total = total.saturating_add(meta.len());
            if total > 600 * 1024 * 1024 || files.len() >= 4096 {
                return Err("Background browser budget exceeded".into());
            }
            let name = path
                .strip_prefix(root)
                .map_err(|_| "Invalid browser path")?
                .to_str()
                .ok_or("Browser path is not supported")?
                .replace('\\', "/");
            let mut input = std::fs::File::open(path).map_err(|_| "Cannot read browser files")?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let n = input
                    .read(&mut buffer)
                    .map_err(|_| "Cannot read browser files")?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            files.insert(
                name,
                hash.finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            );
        }
    }
    Ok(files)
}
#[cfg(windows)]
fn worker(request: Request) -> Result<Value, String> {
    use waypoint_browser::{
        capture_session::{CaptureBudget, CaptureOrigin, CaptureOutcome, CaptureSession},
        rehearsal::RehearsalConfig,
    };
    if !checked_id(&request.task_id)
        || !request.destination.is_absolute()
        || !request.destination.is_dir()
    {
        return Err("Invalid capture worker request".into());
    }
    let (canonical, source) = public_url(&request.url, &json!({}))?;
    if canonical != request.url || source != request.source {
        return Err("Capture source does not match its public URL".into());
    }
    let url = url::Url::parse(&canonical).map_err(|_| "Invalid page URL")?;
    phase(&request, "tools")?;
    let python = crate::runtime_setup::ensure_pdf_tools()?;
    let engine = crate::runtime_setup::ensure(crate::runtime_setup::Tool::Browser)?;
    let engine_root = engine
        .parent()
        .ok_or("Background browser is unavailable")?
        .to_owned();
    let engine_files = inventory(&engine_root)?;
    let archive = request.destination.join("archive");
    let mut capture = CaptureSession::start(
        &archive,
        &request.url,
        vec![CaptureOrigin {
            origin: url.origin().ascii_serialization(),
            source: request.source.clone(),
        }],
        waypoint_workspace::default_registry(),
        CaptureBudget::default(),
        Arc::new(AtomicBool::new(false)),
    )?;
    phase(&request, "reading")?;
    let entry = capture.capture_entry();
    if matches!(entry, CaptureOutcome::Recorded { .. }) {
        phase(&request, "resources")?;
        let config = RehearsalConfig {
            schema: 3,
            engine_root,
            engine_files,
            content_root: archive.clone(),
            content_files: Default::default(),
            entry: "index.html".into(),
            seconds: 30,
            replay_manifest: None,
        };
        capture.discover_resources(&python, config, 4, 1500)?;
    }
    let report =
        serde_json::to_value(capture.report()).map_err(|_| "Cannot encode capture result")?;
    let frozen = capture.freeze();
    let state = report["discovery"]["stopped"]
        .as_str()
        .unwrap_or("not_started");
    let needs_review = frozen.is_err()
        || state != "no_new_requests"
        || report["records"]
            .as_array()
            .is_some_and(|r| r.iter().any(|v| v["outcome"]["state"] == "needs_review"));
    if let Ok(archive) = frozen.as_ref() {
        let mut file = tempfile::NamedTempFile::new_in(&request.destination)
            .map_err(|_| "Cannot save capture inventory")?;
        serde_json::to_writer_pretty(
            &mut file,
            &json!({"schema":1,"task_id":request.task_id,"files":archive.snapshot.files}),
        )
        .map_err(|_| "Cannot save capture inventory")?;
        std::io::Write::flush(&mut file).map_err(|_| "Cannot save capture inventory")?;
        file.persist_noclobber(request.destination.join("inventory.json"))
            .map_err(|_| "Cannot preserve capture inventory")?;
    }
    phase(&request, "saved")?;
    Ok(
        json!({"task_id":request.task_id,"needs_review":needs_review,"archive_written":frozen.is_ok(),"report":report}),
    )
}
pub fn worker_main() -> i32 {
    use std::io::Read;
    let mut bytes = Vec::new();
    let read = std::io::stdin().take(65537).read_to_end(&mut bytes);
    let mut identity = String::new();
    let result = if read.is_err() || bytes.len() > 65536 {
        Err("Invalid capture worker input".to_owned())
    } else {
        serde_json::from_slice::<Request>(&bytes)
            .map_err(|_| "Invalid capture worker input".to_owned())
            .and_then(|request| {
                identity = request.task_id.clone();
                #[cfg(windows)]
                {
                    worker(request)
                }
                #[cfg(not(windows))]
                {
                    let _ = request;
                    Err("Page capture requires Windows".into())
                }
            })
    };
    match result {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(error) => {
            let detail: String = error
                .chars()
                .take(1000)
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            println!("{}", json!({"task_id":identity,"error":detail}));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "0123456789abcdef0123456789abcdef";
    #[test]
    fn outbound_source_urls_refuse_identifiers_authentication_and_forbidden_sources() {
        let profile = json!({"first_name":"Avery","last_name":"Invented","email":"avery@fixture.invalid","phone":"+49 555 0199"});
        for raw in [
            "https://example.org/avery-invented/jobs",
            "https://example.org/?q=avery%40fixture.invalid",
            "https://example.org/?access_token=private",
            "https://example.org/?signature=private",
            "https://example.org/?code=private",
            "https://example.org/?id_token=private",
            "https://example.org/?session_id=private",
            "https://example.org/?user[email]=hidden",
            "https://example.org/avery%2520invented/jobs",
            "https://example.org/?next=https%3A%2F%2Fother.invalid%2F%3Ftoken%3Dprivate",
            "http://example.org/jobs",
            "https://user:private@example.org/jobs",
            "https://127.0.0.1/jobs",
            "https://www.linkedin.com/jobs/view/1",
        ] {
            assert!(public_url(raw, &profile).is_err(), "{raw}");
        }
        let (url, source) = public_url("https://example.org/jobs?jobId=42", &profile).unwrap();
        assert_eq!(url, "https://example.org/jobs?jobId=42");
        assert_eq!(source, "user_supplied_url");
    }
    #[test]
    fn child_request_has_no_candidate_or_credential_fields() {
        let valid = json!({"task_id":ID,"url":"https://example.org/jobs","source":"user_supplied_url","destination":std::env::temp_dir()});
        let request: Request = serde_json::from_value(valid.clone()).unwrap();
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
        for field in [
            "profile",
            "email",
            "password",
            "cookies",
            "headers",
            "body",
            "browser_script",
        ] {
            let mut bad = valid.clone();
            bad[field] = json!("fictional private value");
            assert!(serde_json::from_value::<Request>(bad).is_err());
        }
    }
    #[test]
    fn memory_admission_preserves_reserve_and_caps_the_worker() {
        let gib = 1024u64.pow(3);
        assert_eq!(memory_limit(5 * gib, 8 * gib).unwrap(), 2 * gib);
        assert_eq!(memory_limit(6 * gib, 8 * gib).unwrap(), 3 * gib);
        assert_eq!(memory_limit(20 * gib, 64 * gib).unwrap(), 4 * gib);
        for (free, total) in [(4 * gib, 8 * gib), (9 * gib, 8 * gib), (5 * gib, 0)] {
            assert!(memory_limit(free, total).is_err());
        }
        assert!(host_reserve(3 * gib, 8 * gib).is_ok());
        assert!(host_reserve(3 * gib - 1, 8 * gib).is_err());
        assert!(host_reserve(9 * gib, 8 * gib).is_err());
        assert!(host_reserve(0, 0).is_err());
    }
    #[test]
    fn progress_is_bounded_and_bound_to_its_task() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("phase-reading.json"),
            json!({"task_id":ID,"phase":"reading"}).to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("phase-saved.json"),
            json!({"task_id":"another-task","phase":"saved"}).to_string(),
        )
        .unwrap();
        std::fs::write(dir.path().join("phase-resources.json"), vec![b'x'; 8192]).unwrap();
        assert_eq!(progress(dir.path(), ID), json!("reading"));
    }
    fn fixture() -> (tempfile::TempDir, Workspace, Task) {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added=crate::manual_job::add(&mut ws,&json!({"url":"https://example.org/fictional","title":"Fictional role","employer":"Fictional organization","description":"Fictional source description for capture."}),1).unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let record = crate::attempt_record::AttemptRecord::start(&ws, &job, ID, 2)
            .unwrap()
            .unwrap();
        let root = record.source_capture_directory();
        std::fs::create_dir(&root).unwrap();
        let (_tx, rx) = mpsc::channel();
        let task = Task {
            id: ID.into(),
            job,
            workspace: ws.db_path.clone().unwrap(),
            source_url: "https://example.org/fictional".into(),
            record,
            root,
            cancel: Arc::new(AtomicBool::new(false)),
            rx,
            result: None,
        };
        (dir, ws, task)
    }
    fn completed() -> Value {
        json!({"task_id":ID,"archive_written":true,"needs_review":false,"report":{"recorded_resources":1,"requests":2}})
    }
    #[test]
    fn completion_saves_summary_and_original_job_without_applying() {
        let (_dir, ws, mut task) = fixture();
        let value = finalize(&ws, &mut task, Ok(completed())).unwrap();
        assert_eq!(value["status"], "complete");
        assert_eq!(value["record_saved"], true);
        assert_eq!(value["current_source"], true);
        let records = crate::attempt_record::list(&ws, &task.job).unwrap();
        assert_eq!(records["attempts"][0]["source_page_copy"]["resources"], 1);
        assert_eq!(records["attempts"][0]["status"], "manual_review");
        assert_eq!(
            ws.job(&task.job).unwrap().unwrap().state,
            waypoint_domain::ApplicationState::Discovered
        );
        assert!(
            std::fs::read_to_string(task.record.test_directory().join("original-job.txt"))
                .unwrap()
                .contains("Fictional source description for capture.")
        );
    }
    #[test]
    fn public_control_history_is_durable_and_rejects_value_bearing_metadata() {
        for injected in [false, true] {
            let (_dir, ws, mut task) = fixture();
            let mut result = completed();
            let mut outline = json!({"schema":1,"scope":"root_frame_accessibility_tree","status":"observed","reason":null,"nodes":3,"omitted_controls":0,"unlabelled_controls":0,"omitted_texts":0,"controls":[{"number":1,"role":"textbox","label":"Full name","description":null,"context":["Your details"],"context_incomplete":false,"required":true,"disabled":null}]});
            if injected {
                outline["controls"][0]["value"] = json!("private-value-must-not-be-projected");
            }
            result["report"]["discovery"] = json!({"form_outline":outline});
            finalize(&ws, &mut task, Ok(result)).unwrap();
            let records = crate::attempt_record::list(&ws, &task.job).unwrap();
            let saved = &records["attempts"][0]["source_page_copy"]["form_outline"];
            if injected {
                assert_eq!(saved["status"], "unavailable");
            } else {
                assert_eq!(saved["controls"][0]["label"], "Full name");
                assert!(saved["controls"][0]["disabled"].is_null());
            }
            assert!(!saved
                .to_string()
                .contains("private-value-must-not-be-projected"));
        }
    }
    #[test]
    fn cached_results_recheck_source_identity_instead_of_trusting_cached_flags() {
        let (_dir, ws, mut task) = fixture();
        let mut value = finalize(&ws, &mut task, Ok(completed())).unwrap();
        value["current_source"] = json!(false);
        assert_eq!(
            with_current_source(&ws, value.clone())["current_source"],
            true
        );
        value["source_url_sha256"] = json!("another-source");
        value["current_source"] = json!(true);
        assert_eq!(
            with_current_source(&ws, value.clone())["current_source"],
            false
        );
        value.as_object_mut().unwrap().remove("source_url_sha256");
        assert!(with_current_source(&ws, value)["current_source"].is_null());
    }
    #[test]
    fn cancellation_does_not_hide_an_unconfirmed_shutdown() {
        for (error, status) in [
            (CANCELLED, "cancelled"),
            ("Capture shutdown is unconfirmed", "failed"),
        ] {
            let (_dir, ws, mut task) = fixture();
            task.cancel.store(true, Ordering::Release);
            let value = finalize(&ws, &mut task, Err(error.into())).unwrap();
            assert_eq!(value["status"], status);
            assert!(!ws.manual_review_reasons(&task.job).unwrap().is_empty());
        }
    }
    #[test]
    fn summary_write_failure_is_reported_and_does_not_leave_an_active_record() {
        let (_dir, ws, mut task) = fixture();
        std::fs::create_dir(
            task.record
                .test_directory()
                .join("source-page-summary.json"),
        )
        .unwrap();
        let value = finalize(&ws, &mut task, Ok(completed())).unwrap();
        assert_eq!(value["record_saved"], false);
        assert_eq!(
            crate::attempt_record::list(&ws, &task.job).unwrap()["attempts"][0]["status"],
            "manual_review"
        );
        assert!(!ws.manual_review_reasons(&task.job).unwrap().is_empty());
    }
}
