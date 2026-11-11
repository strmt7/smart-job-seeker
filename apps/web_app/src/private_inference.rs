//! A selected model and its workspace-owned cache, carried to the isolated worker.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selection {
    pub choice: crate::model_preferences::Choice,
    pub cache_root: PathBuf,
}

fn record_task(attempt: &std::path::Path) -> Option<&str> {
    let name = attempt.file_name()?.to_str()?;
    match name.split_once('-') {
        Some((prefix, task))
            if prefix.len() == 16 && prefix.bytes().all(|c| c.is_ascii_hexdigit()) =>
        {
            Some(task)
        }
        _ => Some(name),
    }
}

fn namespace(attempt: &std::path::Path, task: &str) -> Result<PathBuf, String> {
    if task.is_empty()
        || task.len() > 128
        || !task
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        || record_task(attempt) != Some(task)
    {
        return Err("Private model namespace does not match its task".into());
    }
    Ok(attempt.join("model-runtime"))
}

pub(crate) fn create_namespace(attempt: &std::path::Path, task: &str) -> Result<(), String> {
    use std::io::Write;
    let path = namespace(attempt, task)?;
    let parent = attempt
        .canonicalize()
        .map_err(|_| "Model record folder is unavailable")?;
    std::fs::create_dir(&path).map_err(|_| "Cannot create a new private model namespace")?;
    if path
        .canonicalize()
        .map_err(|_| "Private model path is unavailable")?
        .parent()
        != Some(parent.as_path())
    {
        return Err("Private model namespace escaped its record".into());
    }
    let result = (|| -> Result<(), String> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join("owner.json"))
            .map_err(|_| "Cannot bind private model namespace")?;
        let bytes = serde_json::to_vec(&serde_json::json!({"schema":1,"task_id":task}))
            .map_err(|_| "Cannot encode model ownership")?;
        file.write_all(&bytes)
            .map_err(|_| "Cannot persist model namespace ownership".into())
    })();
    if result.is_err() {
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| "Unstarted namespace could not be inspected")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Unstarted namespace path changed; files were retained".into());
            }
        }
        if metadata.file_type().is_symlink()
            || path
                .canonicalize()
                .map_err(|_| "Unstarted namespace path changed")?
                .parent()
                != Some(parent.as_path())
        {
            return Err("Unstarted namespace path changed; files were retained".into());
        }
        std::fs::remove_dir_all(&path).map_err(|_| "Unstarted model namespace cleanup failed")?;
    }
    result
}

fn checked_namespace(attempt: &std::path::Path, task: &str) -> Result<PathBuf, String> {
    use std::io::Read;
    let path = namespace(attempt, task)?;
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| "Private model namespace is unavailable")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Private namespace is a reparse point".into());
        }
    }
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || path
            .canonicalize()
            .map_err(|_| "Private model path is unavailable")?
            .parent()
            != Some(
                attempt
                    .canonicalize()
                    .map_err(|_| "Model record folder is unavailable")?
                    .as_path(),
            )
    {
        return Err("Private model namespace path changed".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path.join("owner.json"))
        .map_err(|_| "Private model owner marker is unavailable")?
        .take(1025)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read model ownership")?;
    if bytes.len() > 1024
        || serde_json::from_slice::<serde_json::Value>(&bytes).ok()
            != Some(serde_json::json!({"schema":1,"task_id":task}))
    {
        return Err("Private model ownership changed; files were retained".into());
    }
    Ok(path)
}

/// Caller must first confirm that its worker process group has drained.
pub(crate) fn cleanup_namespace(attempt: &std::path::Path, task: &str) -> Result<(), String> {
    let path = checked_namespace(attempt, task)?;
    std::fs::remove_dir_all(path)
        .map_err(|_| "Private model profile files could not be removed".into())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::{owned_runtime::OwnedRuntime, visual_review::RequestGuard};
    use processkit::{Command, OutputBufferPolicy, ProcessGroup, ProcessGroupOptions, Stdin};
    use serde_json::{json, Value};
    use std::{
        net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
        path::Path,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    use waypoint_inference::ollama::{http_request, http_request_with_guard};

    const DEFAULT: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 11434);

    fn helper(python: &Path, script: &Path, body: &Value, seconds: u64) -> Result<Value, String> {
        let bytes = serde_json::to_vec(body).map_err(|_| "Cannot prepare model worker input")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let value = runtime.block_on(async {
            let group = ProcessGroup::with_options(
                ProcessGroupOptions::default().max_memory(512 * 1024 * 1024),
            )
            .map_err(|e| e.to_string())?;
            let command = Command::new(python)
                .args(["-I", "-B"])
                .arg(script)
                .stdin(Stdin::from_bytes(bytes))
                .create_no_window()
                .timeout(Duration::from_secs(seconds))
                .output_buffer(
                    OutputBufferPolicy::fail_loud(usize::MAX).with_max_bytes(256 * 1024),
                );
            let output = group
                .start(&command)
                .await
                .map_err(|e| e.to_string())?
                .output_bytes()
                .await
                .map_err(|e| e.to_string())?;
            if output.timed_out() || output.truncated() {
                return Err("Local model worker exceeded its bounds".to_owned());
            }
            serde_json::from_slice::<Value>(output.stdout())
                .map_err(|_| "Invalid local model worker response".to_owned())
        })?;
        if value["ok"] != true {
            return Err(value["error"]
                .as_str()
                .unwrap_or("Local model worker failed")
                .to_owned());
        }
        Ok(value["result"].clone())
    }

    fn idle_shared_runtime_at(
        python: &Path,
        script: &Path,
        addr: SocketAddr,
    ) -> Result<(), String> {
        match http_request(addr, "GET", "/api/ps", None, 1000) {
            Ok((200, bytes)) => {
                let value: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| "The shared model state is unavailable")?;
                match value["models"].as_array() {
                    Some(models) if models.is_empty() => Ok(()),
                    Some(_) => Err(
                        "Another model is using the shared runtime; it was left unchanged".into(),
                    ),
                    None => Err("The shared model state is unavailable".into()),
                }
            }
            Err(_) => {
                let endpoint = helper(
                    python,
                    script,
                    &json!({"action":"endpoint","port":addr.port()}),
                    4,
                )?;
                if endpoint["owners"].as_array().is_some_and(Vec::is_empty) {
                    Ok(())
                } else {
                    Err("The shared runtime is listening but could not be verified; automatic loading stopped".into())
                }
            }
            _ => Err("The shared runtime did not report a usable model state".into()),
        }
    }

    fn get(addr: SocketAddr, path: &str) -> Result<Value, String> {
        let (code, bytes) = http_request(addr, "GET", path, None, 3000)?;
        if code != 200 {
            return Err("The private model runtime returned an error".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid private runtime response".into())
    }

    #[test]
    #[ignore = "uses the explicitly provided local Python worker; no model or shared-service mutation"]
    fn shared_runtime_probe_distinguishes_absent_from_unresponsive_listener() {
        let fixture = PathBuf::from(std::env::var_os("WAYPOINT_AUTO_MODEL_FIXTURE").unwrap());
        assert!(std::fs::metadata(&fixture).unwrap().len() <= 128 * 1024);
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        let python = Path::new(fixture["python"].as_str().unwrap());
        let root = tempfile::Builder::new()
            .prefix("shared-idle-probe-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        let script = root.path().join("probe.py");
        std::fs::write(&script, include_bytes!("local_model_worker.py")).unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(idle_shared_runtime_at(python, &script, addr)
            .unwrap_err()
            .contains("listening"));
        drop(listener);
        idle_shared_runtime_at(python, &script, addr).unwrap();
    }

    pub(crate) struct Session {
        pub addr: SocketAddr,
        pub guard: Option<RequestGuard>,
        pub metadata: Value,
        owner: Option<Arc<OwnedRuntime>>,
        profile: Option<tempfile::TempDir>,
        parent: PathBuf,
        inspector: Option<(PathBuf, PathBuf, u32, u64)>,
        artifact: Option<PathBuf>,
        cleanup_result: Option<Result<(), String>>,
    }

    impl Session {
        pub(crate) fn admit(
            selection: &Selection,
            python: &Path,
            screenshots: &Path,
        ) -> Result<Self, String> {
            selection.choice.validate()?;
            crate::check_inference_memory()?;
            if let Ok(Some(identity)) = crate::resident_model_identity() {
                if !selection.choice.matches(&identity) {
                    return Err("The loaded model differs from the saved rehearsal selection; it was left unchanged".into());
                }
                return Ok(Self {
                    addr: DEFAULT,
                    guard: None,
                    metadata: json!({"mode":"borrowed","model":identity.name,"digest":identity.digest}),
                    owner: None,
                    profile: None,
                    parent: PathBuf::new(),
                    inspector: None,
                    artifact: None,
                    cleanup_result: None,
                });
            }
            let attempt = screenshots
                .parent()
                .ok_or("Model runtime record is unavailable")?
                .canonicalize()
                .map_err(|_| "Model runtime record is unavailable")?;
            let task = super::record_task(&attempt).ok_or("Model runtime task is unavailable")?;
            let parent = super::checked_namespace(&attempt, task)?
                .canonicalize()
                .map_err(|_| "Private model namespace is unavailable")?;
            let profile = tempfile::Builder::new()
                .prefix("local-model-runtime-")
                .tempdir_in(&parent)
                .map_err(|_| "Cannot create private model runtime folder")?;
            let mut session = Self {
                addr: DEFAULT,
                guard: None,
                metadata: Value::Null,
                owner: None,
                profile: Some(profile),
                parent,
                inspector: None,
                artifact: Some(attempt.join("model-session.json")),
                cleanup_result: None,
            };
            let result = session.initialize(selection, python);
            if let Err(error) = result {
                return match session.stop() {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(format!("{error}; model cleanup failed: {cleanup}")),
                };
            }
            Ok(session)
        }

        fn initialize(&mut self, selection: &Selection, python: &Path) -> Result<(), String> {
            let choice = &selection.choice;
            let profile = self
                .profile
                .as_ref()
                .ok_or("Private model profile is unavailable")?
                .path()
                .to_owned();
            let script = profile.join("model-worker.py");
            std::fs::write(&script, include_bytes!("local_model_worker.py"))
                .map_err(|_| "Cannot stage local model worker")?;
            idle_shared_runtime_at(python, &script, DEFAULT)?;
            if !selection.cache_root.is_absolute() {
                return Err("Private model cache must be absolute".into());
            }
            std::fs::create_dir_all(&selection.cache_root)
                .map_err(|_| "Cannot create private model cache")?;
            let prepared = helper(
                python,
                &script,
                &json!({"action":"prepare","choice":choice,"cache_root":selection.cache_root}),
                610,
            )?;
            if prepared["name"] != choice.name || prepared["digest"] != choice.digest {
                return Err("Prepared model identity changed".into());
            }
            let models = PathBuf::from(
                prepared["models"]
                    .as_str()
                    .ok_or("Prepared model location is unavailable")?,
            );
            let expected = selection
                .cache_root
                .canonicalize()
                .map_err(|_| "Private cache is unavailable")?
                .join(&choice.digest)
                .join("models");
            if models
                .canonicalize()
                .map_err(|_| "Prepared model folder is unavailable")?
                != expected
                    .canonicalize()
                    .map_err(|_| "Expected model cache is unavailable")?
            {
                return Err("Prepared model escaped its private cache".into());
            }
            idle_shared_runtime_at(python, &script, DEFAULT)?;
            let mut system =
                sysinfo::System::new().map_err(|_| "Cannot sample model startup memory")?;
            system.refresh_memory();
            crate::model_start::resource_gate(
                choice.size_bytes,
                system.available_memory(),
                &crate::gpu_resources::sample(),
            )?;
            let memory = crate::owned_runtime::memory_budget(
                system.available_memory(),
                system.total_memory(),
            )?;
            let binary = crate::runtime_start::installed_binary()
                .map(Ok)
                .unwrap_or_else(|| {
                    crate::runtime_setup::ensure(crate::runtime_setup::Tool::Ollama)
                })?;
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .map_err(|_| "Cannot reserve a private model port")?;
            self.addr = listener
                .local_addr()
                .map_err(|_| "Cannot inspect private model port")?;
            drop(listener);
            let mut command = crate::runtime_start::runtime_command(&binary, self.addr)
                .env("OLLAMA_MODELS", &models)
                .env("OLLAMA_NOPRUNE", "1")
                .env("OLLAMA_MAX_QUEUE", "1")
                .env("OLLAMA_CONTEXT_LENGTH", "4096")
                .env("OLLAMA_KEEP_ALIVE", "5m")
                .env("LLAMA_ARG_CACHE_RAM", "512")
                .env("LLAMA_ARG_CTX_CHECKPOINTS", "1");
            for (key, name) in [
                ("USERPROFILE", "profile"),
                ("LOCALAPPDATA", "local"),
                ("APPDATA", "roaming"),
                ("TEMP", "temp"),
                ("TMP", "temp"),
            ] {
                let path = profile.join(name);
                std::fs::create_dir_all(&path).map_err(|_| "Cannot stage private model profile")?;
                command = command.env(key, path);
            }
            for key in [
                "OLLAMA_API_KEY",
                "OLLAMA_CLOUD_API_KEY",
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                command = command.env_remove(key);
            }
            let owner = Arc::new(OwnedRuntime::spawn(command, memory)?);
            let pid = owner.identity().pid();
            self.inspector = Some((
                python.to_owned(),
                script.clone(),
                pid,
                owner
                    .identity()
                    .start_time()
                    .ok_or("Private creation identity is unavailable")?,
            ));
            let weak = Arc::downgrade(&owner);
            self.owner = Some(owner);
            let deadline = Instant::now() + Duration::from_secs(25);
            let version = loop {
                if !self
                    .owner
                    .as_ref()
                    .ok_or("Private runtime ownership is unavailable")?
                    .running()?
                {
                    return Err("Private model runtime exited during startup".into());
                }
                let listening = helper(
                    python,
                    &script,
                    &json!({"action":"listener","pid":pid,"port":self.addr.port()}),
                    4,
                )
                .is_ok();
                let reported = if listening {
                    get(self.addr, "/api/version").ok().and_then(|value| {
                        value["version"]
                            .as_str()
                            .filter(|version| !version.is_empty() && version.len() <= 100)
                            .map(str::to_owned)
                    })
                } else {
                    None
                };
                if let Some(version) = reported {
                    break version;
                }
                if Instant::now() >= deadline {
                    return Err("The private model listener could not be verified".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            };
            let addr = self.addr;
            let python = python.to_owned();
            let last = Mutex::new(Instant::now() - Duration::from_secs(2));
            let guard: RequestGuard = Arc::new(move || {
                let owner = weak.upgrade().ok_or("Private model owner was released")?;
                if !owner.running()? {
                    return Err("Private model process exited".into());
                }
                let mut checked = last
                    .lock()
                    .map_err(|_| "Private model inspection is unavailable")?;
                if checked.elapsed() >= Duration::from_secs(1) {
                    helper(
                        &python,
                        &script,
                        &json!({"action":"listener","pid":pid,"port":addr.port()}),
                        4,
                    )?;
                    idle_shared_runtime_at(&python, &script, DEFAULT)?;
                    *checked = Instant::now();
                }
                crate::check_inference_memory()
            });
            self.guard = Some(guard.clone());
            guard()?;
            let binding =
                crate::model_start::startup_binding(&get(addr, "/api/tags")?, &choice.name)?;
            if binding != (choice.size_bytes, choice.digest.clone()) {
                return Err("Private model inventory differs from the saved selection".into());
            }
            let body=serde_json::to_vec(&json!({"model":choice.name,"prompt":"","stream":false,"keep_alive":"5m","options":{"num_ctx":4096,"num_gpu":999,"num_thread":2,"num_batch":128}})).map_err(|_| "Cannot prepare model startup")?;
            let (code, response) = http_request_with_guard(
                addr,
                "POST",
                "/api/generate",
                Some(&body),
                120_000,
                guard.as_ref(),
            )?;
            let response: Value = serde_json::from_slice(&response)
                .map_err(|_| "Invalid private model startup response")?;
            if code != 200 || response["done"] != true {
                return Err("Private model startup was not confirmed".into());
            }
            guard()?;
            let identity = crate::resident_model_identity_at(addr)?
                .ok_or("The private model did not meet the runtime budgets")?;
            if !choice.matches(&identity) {
                return Err("Private resident identity differs from the saved model".into());
            }
            let resident = crate::resident_model_status_at(addr)?;
            let owner = self
                .owner
                .as_ref()
                .ok_or("Private model owner was released")?;
            self.metadata = json!({"mode":"private","model":identity.name,"digest":identity.digest,"context_length":4096,"cache_reused":prepared["reused"],"cache_repaired":prepared["retained_cache"].is_string(),"runtime_version":version,"reported_vram_bytes":resident["reported_vram_bytes"],"reported_cpu_bytes":resident["reported_cpu_bytes"],"pid":pid,"port":addr.port(),"creation_token":owner.identity().start_time(),"parent_pid":owner.identity().ppid(),"process_memory_limit":memory});
            self.metadata["session_state"] = json!("ready");
            self.persist_metadata()?;
            Ok(())
        }

        fn persist_metadata(&self) -> Result<(), String> {
            use std::io::Write;
            let Some(path) = &self.artifact else {
                return Ok(());
            };
            let parent = path
                .parent()
                .ok_or("Model evidence folder is unavailable")?;
            let mut file = tempfile::NamedTempFile::new_in(parent)
                .map_err(|_| "Cannot stage model session evidence")?;
            let bytes = serde_json::to_vec_pretty(&self.metadata)
                .map_err(|_| "Cannot encode model session evidence")?;
            file.write_all(&bytes)
                .and_then(|_| file.flush())
                .map_err(|_| "Cannot write model session evidence")?;
            file.persist(path)
                .map_err(|_| "Cannot persist model session evidence")?;
            Ok(())
        }

        pub(crate) fn stop(&mut self) -> Result<(), String> {
            if let Some(result) = &self.cleanup_result {
                return result.clone();
            }
            self.guard = None;
            let stopped = if let Some(owner) = self.owner.take() {
                match Arc::try_unwrap(owner) {
                    Ok(mut owner) => owner.stop(),
                    Err(_) => Err("Private model still has an active request".into()),
                }
            } else {
                Ok(())
            };
            let inspected = if let Some((python, script, pid, created)) = self.inspector.take() {
                helper(
                    &python,
                    &script,
                    &json!({"action":"process","pid":pid,"created":created}),
                    5,
                )
                .and_then(|state| {
                    if state["exited"] != true {
                        return Err("The private model process has not exited".to_owned());
                    }
                    self.metadata["process_exit"] = state;
                    let deadline=Instant::now()+Duration::from_secs(5);
                    loop {
                        let endpoint=helper(&python,&script,&json!({"action":"endpoint","port":self.addr.port()}),4)?;
                        let empty=endpoint["owners"].as_array().is_some_and(Vec::is_empty);
                        self.metadata["endpoint_after_stop"]=endpoint;
                        if empty { break; }
                        if Instant::now()>=deadline { return Err("Private model listener remained after teardown".to_owned()); }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    self.metadata["connect_after_stop"]=match TcpStream::connect_timeout(&self.addr,Duration::from_millis(250)) {
                        Ok(_)=>json!({"connected":true}),
                        Err(error)=>json!({"connected":false,"kind":format!("{:?}",error.kind()),"os_error":error.raw_os_error()}),
                    };
                    Ok(())
                })
            } else {
                Ok(())
            };
            let stopped = match (stopped, inspected) {
                (Err(stop), Err(inspect)) => Err(format!("{stop}; {inspect}")),
                (result, other) => result.and(other),
            };
            let cleaned = (|| -> Result<(), String> {
                if let Some(profile) = self.profile.take() {
                    let verified = (|| -> Result<(), String> {
                        let metadata = std::fs::symlink_metadata(profile.path())
                            .map_err(|_| "Private model profile could not be inspected")?;
                        use std::os::windows::fs::MetadataExt;
                        if metadata.file_attributes() & 0x400 != 0
                            || profile
                                .path()
                                .canonicalize()
                                .map_err(|_| "Private model profile path changed")?
                                .parent()
                                != Some(self.parent.as_path())
                        {
                            return Err("Private profile cleanup path changed".into());
                        }
                        Ok(())
                    })();
                    if let Err(error) = verified {
                        let _retained = profile.keep();
                        return Err(format!("{error}; files were retained"));
                    }
                    profile
                        .close()
                        .map_err(|_| "Private model profile could not be removed".into())
                } else {
                    Ok(())
                }
            })();
            let result = match (stopped, cleaned) {
                (Err(stop), Err(cleanup)) => Err(format!("{stop}; {cleanup}")),
                (result, other) => result.and(other),
            };
            if self.artifact.is_some() {
                self.metadata["session_state"] = json!(if result.is_ok() {
                    "closed"
                } else {
                    "cleanup_unverified"
                });
            }
            let evidence = self.persist_metadata();
            let result = match (result, evidence) {
                (Err(error), Err(evidence)) => Err(format!("{error}; {evidence}")),
                (result, other) => result.and(other),
            };
            self.cleanup_result = Some(result.clone());
            result
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            if self.cleanup_result.is_none() {
                if let Err(error) = self.stop() {
                    eprintln!("Private model cleanup failed: {error}");
                }
            }
        }
    }
}

#[cfg(windows)]
pub(crate) use windows::Session;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_cleanup_preserves_other_files_and_refuses_changed_ownership() {
        let folder = tempfile::Builder::new()
            .prefix("model-namespace-test-")
            .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        let attempt = folder.path().join("attempt");
        std::fs::create_dir(&attempt).unwrap();
        let outside = folder.path().join("outside.txt");
        std::fs::write(&outside, b"preserve").unwrap();
        create_namespace(&attempt, "attempt").unwrap();
        let namespace = attempt.join("model-runtime");
        std::fs::write(namespace.join("profile"), b"owned temporary data").unwrap();
        assert!(cleanup_namespace(&attempt, "wrong").is_err());
        std::fs::write(namespace.join("owner.json"), b"{}").unwrap();
        assert!(cleanup_namespace(&attempt, "attempt").is_err());
        assert!(namespace.join("profile").is_file());
        std::fs::write(
            namespace.join("owner.json"),
            br#"{"schema":1,"task_id":"attempt"}"#,
        )
        .unwrap();
        cleanup_namespace(&attempt, "attempt").unwrap();
        assert!(!namespace.exists());
        assert_eq!(std::fs::read(outside).unwrap(), b"preserve");
    }

    #[test]
    fn model_namespace_uses_the_application_record_task_not_the_job_prefix() {
        let folder = tempfile::tempdir().unwrap();
        let task = "b".repeat(32);
        let attempt = folder.path().join(format!("0123456789abcdef-{task}"));
        std::fs::create_dir(&attempt).unwrap();
        create_namespace(&attempt, &task).unwrap();
        assert_eq!(record_task(&attempt), Some(task.as_str()));
        assert!(cleanup_namespace(&attempt, &"c".repeat(32)).is_err());
        cleanup_namespace(&attempt, &task).unwrap();
        let other = folder.path().join(format!("not-a-job-prefix-{task}"));
        assert!(namespace(&other, &task).is_err());
    }
}
