//! Resource-gated loading of one already-installed local Ollama model.
use serde_json::{json, Value};
use std::sync::Mutex;

struct Task {
    id: String,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);
#[cfg(any(windows, test))]
const GIB: u64 = 1024 * 1024 * 1024;
#[cfg(any(windows, test))]
const MAX_MODEL_BYTES: u64 = 16 * GIB;

#[cfg(any(windows, test))]
pub(crate) fn resource_gate(size: u64, system_available: u64, gpu: &Value) -> Result<(), String> {
    if size == 0 || size > MAX_MODEL_BYTES {
        return Err("Model file size is unavailable or exceeds the 16 GiB startup limit".into());
    }
    // Leave additional headroom for other applications after allowing a full
    // model-sized host allocation. This remains a snapshot, not a reservation.
    let needed_system = size.checked_add(10 * GIB).ok_or("Invalid model size")?;
    if system_available < needed_system {
        return Err(format!(
            "Not enough available system memory for the conservative startup estimate: {} MiB observed, at least {} MiB required. This is a snapshot, not a reservation.",
            system_available / (1024 * 1024), needed_system.div_ceil(1024 * 1024)
        ));
    }
    if gpu["state"] != "sampled" || gpu["reported_adapters"].as_u64() != Some(1) {
        return Err("Startup needs one GPU with complete memory observations".into());
    }
    let adapters = gpu["adapters"]
        .as_array()
        .filter(|items| items.len() == 1)
        .ok_or("GPU memory is unavailable")?;
    let total = adapters[0]["total_bytes"]
        .as_u64()
        .ok_or("GPU capacity is unavailable")?;
    let used = adapters[0]["used_bytes"]
        .as_u64()
        .ok_or("GPU usage is unavailable")?;
    let free = total
        .checked_sub(used)
        .ok_or("GPU memory observation is inconsistent")?;
    let needed_gpu = size
        .checked_mul(6)
        .and_then(|n| n.checked_add(4))
        .map(|n| n / 5)
        // Estimated overhead and at most 2 GiB of model offload. The runtime
        // decides the actual split, which is checked after loading.
        .and_then(|n| n.checked_add(GIB / 2))
        .map(|n| n.saturating_sub(2 * GIB).max(GIB))
        .ok_or("Invalid model size")?;
    if free < needed_gpu {
        return Err(format!(
            "Not enough observed GPU memory for the conservative startup estimate: {} MiB free, at least {} MiB required. This is a snapshot, not a reservation.",
            free / (1024 * 1024), needed_gpu.div_ceil(1024 * 1024)
        ));
    }
    Ok(())
}

pub(crate) fn local_inventory_entry<'a>(tags: &'a Value, model: &str) -> Result<&'a Value, String> {
    let models = tags["models"]
        .as_array()
        .filter(|items| items.len() <= 1000)
        .ok_or("Installed model inventory is unavailable or too large")?;
    let mut matches = models
        .iter()
        .filter(|item| item["name"].as_str() == Some(model));
    let entry = matches
        .next()
        .ok_or("The selected model is not uniquely present in the installed inventory")?;
    if matches.next().is_some() {
        return Err("The selected model is not uniquely present in the installed inventory".into());
    }
    let remote_marker = |name: &str| match entry.get(name) {
        None | Some(Value::Null) => false,
        Some(Value::String(value)) => !value.trim().is_empty(),
        Some(_) => true,
    };
    if model.to_ascii_lowercase().contains("cloud")
        || entry["details"]["format"].as_str() != Some("gguf")
        || remote_marker("remote_model")
        || remote_marker("remote_host")
    {
        return Err(
            "Only installed local GGUF models without remote forwarding are supported".into(),
        );
    }
    Ok(entry)
}

#[cfg(any(windows, test))]
pub(crate) fn installed_size(tags: &Value, model: &str) -> Result<u64, String> {
    local_inventory_entry(tags, model)?["size"]
        .as_u64()
        .filter(|size| *size >= GIB && *size <= MAX_MODEL_BYTES)
        .ok_or_else(|| {
            "The installed model size is outside the supported 1–16 GiB range or unavailable".into()
        })
}

#[cfg(any(windows, test))]
fn startup_binding(tags: &Value, model: &str) -> Result<(u64, String), String> {
    let size = installed_size(tags, model)?;
    let digest = local_inventory_entry(tags, model)?["digest"]
        .as_str()
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("The installed model digest is unavailable")?;
    Ok((size, digest.to_owned()))
}

#[cfg(any(windows, test))]
fn check_startup_binding(
    tags: &Value,
    model: &str,
    expected: &(u64, String),
) -> Result<(), String> {
    if startup_binding(tags, model)? != *expected {
        return Err("The selected model changed after the resource check; start again".into());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn existing_model(ps: &Value, requested: &str) -> Result<Option<Value>, String> {
    let models = ps["models"]
        .as_array()
        .ok_or("Local runtime did not report resident models")?;
    if models.is_empty() {
        return Ok(None);
    }
    let state = crate::model_diagnostics(ps)?;
    if state["model"].as_str() == Some(requested) && state["context_length"].as_u64() == Some(4096)
    {
        return Ok(Some(
            json!({"model":requested,"reported_vram_bytes":state["reported_vram_bytes"],"reported_cpu_bytes":state["reported_cpu_bytes"],"context_length":4096,"already_running":true}),
        ));
    }
    Err("A model is already loaded with a different identity, context, or resource state; it was left unchanged".into())
}

#[cfg(any(windows, test))]
fn verify_started_model(ps: &Value, requested: &str) -> Result<Value, String> {
    let state = crate::model_diagnostics(ps).map_err(|_| {
        "Model startup could not be verified; a model may remain loaded. Check local model status before retrying".to_owned()
    })?;
    if state["model"].as_str() != Some(requested) || state["context_length"].as_u64() != Some(4096)
    {
        return Err("The requested model was not reported as the single model within the GPU and system-memory budgets with the requested 4096 context; a model may remain loaded. Check local model status before retrying".into());
    }
    Ok(
        json!({"model":requested,"reported_vram_bytes":state["reported_vram_bytes"],"reported_cpu_bytes":state["reported_cpu_bytes"],"context_length":4096,"already_running":false}),
    )
}

#[cfg(any(windows, test))]
fn guarded_start_request(
    addr: std::net::SocketAddr,
    request: &[u8],
    guard: &(dyn Fn() -> Result<(), String> + Send + Sync),
) -> Result<(u16, Vec<u8>), String> {
    waypoint_inference::ollama::http_request_with_guard(
        addr,
        "POST",
        "/api/generate",
        Some(request),
        120_000,
        guard,
    )
    .map_err(|_| "Model startup was interrupted or did not return; a model may still be loading or resident. Check local model status before retrying".into())
}

#[cfg(windows)]
fn run(model: &str) -> Result<Value, String> {
    use waypoint_inference::ollama::http_request;
    let addr = "127.0.0.1:11434"
        .parse()
        .map_err(|_| "Invalid local runtime address")?;
    let get = |path: &str| -> Result<Value, String> {
        let (status, bytes) = http_request(addr, "GET", path, None, 3000)?;
        if status != 200 {
            return Err(format!("Local runtime returned HTTP {status}"));
        }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid local runtime response".into())
    };
    let ps = get("/api/ps")?;
    let tags = get("/api/tags")?;
    if let Some(existing) = existing_model(&ps, model)? {
        crate::verify_local_model_binding(&ps, &tags, model)?;
        return Ok(existing);
    }
    let binding = startup_binding(&tags, model)?;
    let mut system = sysinfo::System::new().map_err(|_| "System memory could not be sampled")?;
    system.refresh_memory();
    resource_gate(
        binding.0,
        system.available_memory(),
        &crate::gpu_resources::sample(),
    )?;
    let ps = get("/api/ps")?;
    if let Some(existing) = existing_model(&ps, model)? {
        crate::verify_local_model_binding(&ps, &get("/api/tags")?, model)?;
        return Ok(existing);
    }
    // Recheck the alias after the resource sample, immediately before startup.
    check_startup_binding(&get("/api/tags")?, model, &binding)?;
    let request = serde_json::to_vec(&json!({"model":model,"prompt":"","stream":false,"keep_alive":"10m","options":{"num_ctx":4096,"num_gpu":999,"num_thread":2,"num_batch":128}})).map_err(|_| "Could not prepare local model request")?;
    let (status, bytes) = guarded_start_request(addr, &request, &crate::check_inference_memory)?;
    if status != 200 {
        return Err(format!("Local model startup failed (HTTP {status}); check the model status before trying again"));
    }
    let response: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid model startup response; check status before trying again")?;
    if response["done"] != true {
        return Err("Model startup was not confirmed; check status before trying again".into());
    }
    let status = get("/api/ps").map_err(|_| "Model startup could not be verified; a model may remain loaded. Check local model status before retrying")?;
    crate::verify_local_model_binding(&status, &get("/api/tags")?, model)?;
    if status["models"][0]["digest"].as_str() != Some(binding.1.as_str()) {
        return Err("The started model differs from the resource-checked model; a model may remain loaded. Check local model status before retrying".into());
    }
    verify_started_model(&status, model)
}
#[cfg(not(windows))]
fn run(_model: &str) -> Result<Value, String> {
    Err("Resource-gated model startup is supported on Windows only".into())
}

pub fn start(id: String, model: String) -> Result<(), String> {
    if model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
        return Err("Invalid model name".into());
    }
    let mut task = TASK.lock().map_err(|_| "Model startup unavailable")?;
    if task.is_some() {
        return Err("Model startup is running or awaiting its result".into());
    }
    let lease = crate::doc_suggest::lease()?;
    *task = Some(Task {
        id: id.clone(),
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let _lease = lease;
        let result = std::panic::catch_unwind(|| run(&model)).unwrap_or_else(|_| {
            Err("Model startup stopped unexpectedly; check status before retrying".into())
        });
        if let Ok(mut tasks) = TASK.lock() {
            if let Some(task) = tasks.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}

pub fn active() -> Result<Option<String>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Model startup unavailable")?
        .as_ref()
        .map(|task| task.id.clone()))
}

pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Model startup unavailable")?;
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Model startup unavailable")?
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| "Model startup not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    let value = match result {
        Ok(data) => json!({"status":"complete","model":data}),
        Err(error) => json!({"status":"failed","error":error}),
    };
    *COMPLETED.lock().map_err(|_| "Model startup unavailable")? = Some((id.into(), value.clone()));
    *tasks = None;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_binding_rejects_changed_size_or_digest() {
        let digest = "a".repeat(64);
        let mut tags = json!({"models":[{"name":"fixture","size":6*GIB,"digest":digest,"details":{"format":"gguf"}}]});
        let binding = startup_binding(&tags, "fixture").unwrap();
        assert!(check_startup_binding(&tags, "fixture", &binding).is_ok());
        tags["models"][0]["size"] = json!(7 * GIB);
        assert!(check_startup_binding(&tags, "fixture", &binding).is_err());
        tags["models"][0]["size"] = json!(6 * GIB);
        tags["models"][0]["digest"] = json!("b".repeat(64));
        assert!(check_startup_binding(&tags, "fixture", &binding).is_err());
        tags["models"][0]["digest"] = json!("invalid");
        assert!(startup_binding(&tags, "fixture").is_err());
    }
    #[test]
    fn startup_request_stops_on_memory_guard_and_reports_uncertain_state() {
        use std::io::Read;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 2048];
            assert!(stream.read(&mut bytes).unwrap() > 0);
            tx.send(()).unwrap();
            while stream.read(&mut bytes).unwrap() != 0 {}
        });
        let pressure = Arc::new(AtomicBool::new(false));
        let worker_pressure = pressure.clone();
        let client = std::thread::spawn(move || {
            guarded_start_request(addr, b"{}", &|| {
                if worker_pressure.load(Ordering::Relaxed) {
                    Err("simulated pressure".into())
                } else {
                    Ok(())
                }
            })
        });
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        pressure.store(true, Ordering::Relaxed);
        let error = client.join().unwrap().unwrap_err();
        assert!(error.contains("may still be loading or resident"));
        server.join().unwrap();
    }

    #[test]
    fn existing_model_is_reused_only_when_identity_context_and_memory_match() {
        let valid = json!({"models":[{"name":"fixture","size":6*GIB,"size_vram":6*GIB,"context_length":4096}]});
        let result = existing_model(&valid, "fixture").unwrap().unwrap();
        assert_eq!(result["already_running"], true);
        assert_eq!(result["context_length"], 4096);
        let mut limited_offload = valid.clone();
        limited_offload["models"][0]["size_vram"] = json!(5 * GIB);
        assert_eq!(
            existing_model(&limited_offload, "fixture")
                .unwrap()
                .unwrap()["reported_cpu_bytes"],
            GIB
        );
        assert_eq!(
            verify_started_model(&limited_offload, "fixture").unwrap()["reported_cpu_bytes"],
            GIB
        );
        assert!(existing_model(&valid, "other").is_err());
        let mut wrong_context = valid.clone();
        wrong_context["models"][0]["context_length"] = json!(8192);
        assert!(existing_model(&wrong_context, "fixture").is_err());
        let mut partial_gpu = valid.clone();
        partial_gpu["models"][0]["size_vram"] = json!(GIB);
        assert!(existing_model(&partial_gpu, "fixture").is_err());
        assert!(existing_model(&json!({"models":[]}), "fixture")
            .unwrap()
            .is_none());
        assert!(existing_model(&json!({}), "fixture").is_err());
    }
    #[test]
    fn new_start_must_report_requested_context_and_safe_residency() {
        let valid = json!({"models":[{"name":"fixture","size":6*GIB,"size_vram":6*GIB,"context_length":4096}]});
        assert_eq!(
            verify_started_model(&valid, "fixture").unwrap()["context_length"],
            4096
        );
        let mut wrong_context = valid.clone();
        wrong_context["models"][0]["context_length"] = json!(8192);
        for status in [wrong_context, json!({"models":[]}), json!({})] {
            let error = verify_started_model(&status, "fixture").unwrap_err();
            assert!(error.contains("may remain loaded"));
        }
        let mut partial = valid;
        partial["models"][0]["size_vram"] = json!(GIB);
        assert!(verify_started_model(&partial, "fixture").is_err());
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires a compatible resident local model; does not load or unload it"]
    fn live_existing_requested_model_is_reused() {
        let model = crate::resident_model()
            .unwrap()
            .expect("eligible resident model");
        let result = run(&model).unwrap();
        assert_eq!(result["model"], model);
        assert_eq!(result["already_running"], true);
    }
    #[test]
    fn startup_refuses_missing_or_insufficient_resources() {
        let gpu = json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":16*GIB,"used_bytes":5*GIB}]});
        assert!(resource_gate(6 * GIB, 20 * GIB, &gpu).is_ok());
        assert!(resource_gate(0, 20 * GIB, &gpu).is_err());
        assert!(resource_gate(9 * GIB, 20 * GIB, &gpu).is_ok());
        let spacious = json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":16*GIB,"used_bytes":GIB}]});
        assert!(resource_gate(9 * GIB, 20 * GIB, &spacious).is_ok());
        assert!(resource_gate(10 * GIB, 20 * GIB, &spacious).is_ok());
        assert!(resource_gate(10 * GIB + GIB / 2, 21 * GIB, &spacious).is_ok());
        assert!(resource_gate(11 * GIB, 20 * GIB, &spacious).is_err());
        let larger_gpu = json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":24*GIB,"used_bytes":GIB}]});
        let at_free = |free| json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":16*GIB,"used_bytes":16*GIB-free}]});
        assert!(resource_gate(12 * GIB, 22 * GIB, &larger_gpu).is_ok());
        assert!(resource_gate(12 * GIB + 1, 21 * GIB, &larger_gpu).is_err());
        assert!(resource_gate(13 * GIB, 23 * GIB, &at_free(16 * GIB)).is_ok());
        assert!(resource_gate(16 * GIB, 26 * GIB, &at_free(16 * GIB)).is_err());
        let model_1285 = 12 * GIB + 85 * GIB / 100;
        assert!(resource_gate(model_1285, 23 * GIB, &at_free(16 * GIB)).is_ok());
        assert!(resource_gate(model_1285, 20 * GIB, &at_free(16 * GIB)).is_err());
        assert!(resource_gate(9 * GIB, 16 * GIB, &spacious).is_err());
        let system_error = resource_gate(6 * GIB, 13 * GIB, &gpu).unwrap_err();
        assert!(
            system_error.contains("13312 MiB observed")
                && system_error.contains("16384 MiB required")
        );
        assert!(resource_gate(6 * GIB, 16 * GIB - 1, &gpu).is_err());
        assert!(resource_gate(6 * GIB, 16 * GIB, &gpu).is_ok());
        let gpu_error = resource_gate(11 * GIB, 21 * GIB, &gpu).unwrap_err();
        assert!(gpu_error.contains("11264 MiB free") && gpu_error.contains("MiB required"));
        let required_gpu = (6 * GIB * 6).div_ceil(5) + GIB / 2 - 2 * GIB;
        let at_boundary = |free| json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":16*GIB,"used_bytes":16*GIB-free}]});
        assert!(resource_gate(6 * GIB, 20 * GIB, &at_boundary(required_gpu)).is_ok());
        assert!(resource_gate(6 * GIB, 20 * GIB, &at_boundary(required_gpu - 1)).is_err());
        assert!(resource_gate(6 * GIB, 20 * GIB, &json!({"state":"unavailable"})).is_err());
        assert!(resource_gate(
            6 * GIB,
            20 * GIB,
            &json!({"state":"sampled","reported_adapters":2,"adapters":[]})
        )
        .is_err());
        assert!(resource_gate(6*GIB, 20*GIB, &json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":10,"used_bytes":11}]})).is_err());
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "loads one installed model with partial GPU offload after resource preflight"]
    fn live_partial_gpu_model_is_usable_with_memory_guard() {
        use waypoint_inference::Inference;
        let addr = "127.0.0.1:11434".parse().unwrap();
        let (status, ps) =
            waypoint_inference::ollama::http_request(addr, "GET", "/api/ps", None, 3000).unwrap();
        assert_eq!(status, 200);
        assert!(serde_json::from_slice::<Value>(&ps).unwrap()["models"]
            .as_array()
            .unwrap()
            .is_empty());
        let (_, tags) =
            waypoint_inference::ollama::http_request(addr, "GET", "/api/tags", None, 3000).unwrap();
        let tags: Value = serde_json::from_slice(&tags).unwrap();
        let name = "qwen3.5:9b";
        let size = installed_size(&tags, name).unwrap();
        let mut system = sysinfo::System::new().unwrap();
        system.refresh_memory();
        resource_gate(
            size,
            system.available_memory(),
            &crate::gpu_resources::sample(),
        )
        .unwrap();
        struct Unload;
        impl Drop for Unload {
            fn drop(&mut self) {
                let request = json!({"model":"qwen3.5:9b","keep_alive":0}).to_string();
                let _ = waypoint_inference::ollama::http_request(
                    "127.0.0.1:11434".parse().unwrap(),
                    "POST",
                    "/api/generate",
                    Some(request.as_bytes()),
                    10_000,
                );
            }
        }
        let _unload = Unload;
        let request = json!({"model":name,"prompt":"","stream":false,"keep_alive":"2m","options":{"num_ctx":4096,"num_gpu":30,"num_thread":2,"num_batch":128}}).to_string();
        assert_eq!(
            guarded_start_request(addr, request.as_bytes(), &crate::check_inference_memory)
                .unwrap()
                .0,
            200
        );
        let (_, ps) =
            waypoint_inference::ollama::http_request(addr, "GET", "/api/ps", None, 3000).unwrap();
        let ps: Value = serde_json::from_slice(&ps).unwrap();
        let (gpu, cpu) =
            waypoint_inference::ollama::bounded_resident_allocation(&ps["models"][0]).unwrap();
        println!("Reported model allocation: GPU {gpu} bytes, system memory {cpu} bytes");
        assert!(cpu > 0, "runtime did not report partial offload");
        assert_eq!(crate::resident_model().unwrap().as_deref(), Some(name));
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", name)
            .unwrap()
            .with_resident_context();
        let mut llm = crate::resident_inference::ResidentInference::new(
            llm,
            name.into(),
            crate::resident_model_identity,
        );
        let result = llm
            .generate_structured("test", "Return JSON with ok set to true.", 32)
            .unwrap()
            .0;
        assert_eq!(result["ok"], true);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires no resident model, adequate memory and an installed local model"]
    fn live_start_respects_resource_gate_and_returns_gpu_resident_model() {
        let model =
            std::env::var("WAYPOINT_MODEL_START_TEST").expect("set WAYPOINT_MODEL_START_TEST");
        start("fictional-start-test".into(), model.clone()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(140);
        loop {
            let result = poll("fictional-start-test").unwrap();
            if result["status"] != "running" {
                assert_eq!(result["status"], "complete", "{result}");
                assert_eq!(result["model"]["model"], model);
                assert_eq!(result["model"]["context_length"], 4096);
                assert_eq!(poll("fictional-start-test").unwrap(), result);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "model startup timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires a resident local model; does not load or unload it"]
    fn live_start_refuses_to_replace_resident_model() {
        let before = crate::resident_model_status().unwrap();
        assert!(
            before["model"].is_string(),
            "a model must already be resident"
        );
        let error = run("another-fictional-model").unwrap_err();
        assert!(error.contains("already loaded"));
        assert_eq!(crate::resident_model_status().unwrap(), before);
    }
    #[test]
    fn startup_accepts_only_one_sized_installed_model() {
        let one = json!({"models":[{"name":"fixture","size":6*GIB,"details":{"format":"gguf"}}]});
        assert_eq!(installed_size(&one, "fixture").unwrap(), 6 * GIB);
        let larger =
            json!({"models":[{"name":"fixture","size":9*GIB,"details":{"format":"gguf"}}]});
        assert_eq!(installed_size(&larger, "fixture").unwrap(), 9 * GIB);
        let too_large =
            json!({"models":[{"name":"fixture","size":16*GIB+1,"details":{"format":"gguf"}}]});
        assert!(installed_size(&too_large, "fixture").is_err());
        assert!(installed_size(&one, "other").is_err());
        assert!(installed_size(
            &json!({"models":[{"name":"fixture-cloud","size":GIB,"details":{"format":"gguf"}}]}),
            "fixture-cloud"
        )
        .is_err());
        for marker in [
            json!({"remote_model":"provider/model"}),
            json!({"remote_host":"https://example.invalid"}),
            json!({"remote_host":true}),
        ] {
            let mut tags = one.clone();
            for (key, value) in marker.as_object().unwrap() {
                tags["models"][0][key] = value.clone();
            }
            assert!(installed_size(&tags, "fixture").is_err());
        }
        assert!(installed_size(
            &json!({"models":[{"name":"remote-alias","size":GIB,"details":{"format":""}}]}),
            "remote-alias"
        )
        .is_err());
        assert!(installed_size(
            &json!({"models":[{"name":"fixture","size":382,"details":{"format":"gguf"}}]}),
            "fixture"
        )
        .is_err());
        assert!(
            installed_size(&json!({"models":[{"name":"fixture","size":0}]}), "fixture").is_err()
        );
        assert!(installed_size(
            &json!({"models":[{"name":"fixture","size":1},{"name":"fixture","size":1}]}),
            "fixture"
        )
        .is_err());
    }
}
