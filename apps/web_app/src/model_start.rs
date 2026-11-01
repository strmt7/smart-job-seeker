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
const MAX_MODEL_BYTES: u64 = 10 * GIB;

#[cfg(any(windows, test))]
fn resource_gate(size: u64, system_available: u64, gpu: &Value) -> Result<(), String> {
    if size == 0 || size > MAX_MODEL_BYTES {
        return Err("Model file size is unavailable or exceeds the 10 GiB startup limit".into());
    }
    let needed_system = size.checked_add(8 * GIB).ok_or("Invalid model size")?;
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
        .and_then(|n| n.checked_add(2 * GIB))
        .ok_or("Invalid model size")?;
    if free < needed_gpu {
        return Err(format!(
            "Not enough observed GPU memory for the conservative startup estimate: {} MiB free, at least {} MiB required. This is a snapshot, not a reservation.",
            free / (1024 * 1024), needed_gpu.div_ceil(1024 * 1024)
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn installed_size(tags: &Value, model: &str) -> Result<u64, String> {
    let models = tags["models"]
        .as_array()
        .filter(|items| items.len() <= 1000)
        .ok_or("Installed model inventory is unavailable or too large")?;
    let matching: Vec<_> = models
        .iter()
        .filter(|item| item["name"].as_str() == Some(model))
        .collect();
    if matching.len() != 1 {
        return Err("The selected model is not uniquely present in the installed inventory".into());
    }
    if model.to_ascii_lowercase().contains("cloud")
        || matching[0]["details"]["format"].as_str() != Some("gguf")
    {
        return Err("Only installed local GGUF models are supported for startup".into());
    }
    matching[0]["size"]
        .as_u64()
        .filter(|size| *size >= GIB && *size <= MAX_MODEL_BYTES)
        .ok_or_else(|| {
            "The installed model size is outside the supported 1–10 GiB range or unavailable".into()
        })
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
    let empty = |ps: &Value| ps["models"].as_array().is_some_and(Vec::is_empty);
    if !empty(&get("/api/ps")?) {
        return Err("Another model is already loaded; it was left unchanged".into());
    }
    let size = installed_size(&get("/api/tags")?, model)?;
    let mut system = sysinfo::System::new().map_err(|_| "System memory could not be sampled")?;
    system.refresh_memory();
    resource_gate(
        size,
        system.available_memory(),
        &crate::gpu_resources::sample(),
    )?;
    if !empty(&get("/api/ps")?) {
        return Err("A model became resident during startup checks; it was left unchanged".into());
    }
    let request = serde_json::to_vec(&json!({"model":model,"prompt":"","stream":false,"keep_alive":"10m","options":{"num_ctx":4096,"num_gpu":999,"num_thread":2,"num_batch":128}})).map_err(|_| "Could not prepare local model request")?;
    let (status, bytes) = http_request(addr, "POST", "/api/generate", Some(&request), 120_000)
        .map_err(|_| "Model startup did not return; check the model status before trying again")?;
    if status != 200 {
        return Err(format!("Local model startup failed (HTTP {status}); check the model status before trying again"));
    }
    let response: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid model startup response; check status before trying again")?;
    if response["done"] != true {
        return Err("Model startup was not confirmed; check status before trying again".into());
    }
    let state = crate::model_diagnostics(&get("/api/ps")?)?;
    if state["model"].as_str() != Some(model) {
        return Err("The requested model is not reported as the single fully GPU-resident model; inspect local model status".into());
    }
    Ok(
        json!({"model":model,"reported_vram_bytes":state["reported_vram_bytes"],"context_length":state["context_length"]}),
    )
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
    fn startup_refuses_missing_or_insufficient_resources() {
        let gpu = json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":16*GIB,"used_bytes":5*GIB}]});
        assert!(resource_gate(6 * GIB, 20 * GIB, &gpu).is_ok());
        assert!(resource_gate(0, 20 * GIB, &gpu).is_err());
        assert!(resource_gate(9 * GIB, 20 * GIB, &gpu).is_err());
        let spacious = json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":16*GIB,"used_bytes":GIB}]});
        assert!(resource_gate(9 * GIB, 20 * GIB, &spacious).is_ok());
        assert!(resource_gate(10 * GIB, 20 * GIB, &spacious).is_ok());
        assert!(resource_gate(10 * GIB + 1, 20 * GIB, &spacious).is_err());
        assert!(resource_gate(9 * GIB, 16 * GIB, &spacious).is_err());
        let system_error = resource_gate(6 * GIB, 13 * GIB, &gpu).unwrap_err();
        assert!(
            system_error.contains("13312 MiB observed")
                && system_error.contains("14336 MiB required")
        );
        let gpu_error = resource_gate(8 * GIB, 20 * GIB, &gpu).unwrap_err();
        assert!(gpu_error.contains("11264 MiB free") && gpu_error.contains("11879 MiB required"));
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
            json!({"models":[{"name":"fixture","size":10*GIB+1,"details":{"format":"gguf"}}]});
        assert!(installed_size(&too_large, "fixture").is_err());
        assert!(installed_size(&one, "other").is_err());
        assert!(installed_size(
            &json!({"models":[{"name":"fixture-cloud","size":GIB,"details":{"format":"gguf"}}]}),
            "fixture-cloud"
        )
        .is_err());
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
