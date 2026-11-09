//! Experimental Windows-only, checksum-bound local browser rehearsal.
//!
//! This transport does not qualify a general submission boundary. It must not
//! be used to interact with real application forms until qualification closes
//! the remaining channels and lifecycle cases. No unrestricted fallback exists.
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RehearsalConfig {
    pub schema: u32,
    pub engine_root: PathBuf,
    pub engine_files: BTreeMap<String, String>,
    pub content_root: PathBuf,
    pub content_files: BTreeMap<String, String>,
    pub entry: String,
    pub seconds: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RehearsalReady {
    #[serde(rename = "type")]
    kind: String,
    pub schema: u32,
    pub appcontainer: bool,
    pub capabilities: u32,
    pub pid: u32,
    pub content_root: PathBuf,
    pub entry: String,
    pub profile_name: String,
    pub entry_url: String,
    pub job_memory_limit: u64,
}

impl RehearsalReady {
    pub fn validate(&self) -> Result<(), super::cdp::CdpError> {
        let name = self.profile_name.strip_prefix("SmartJobseeker.Rehearsal.");
        if self.kind != "ready"
            || self.schema != 1
            || !self.appcontainer
            || self.capabilities != 0
            || self.pid == 0
            || self.job_memory_limit != 2 * 1024 * 1024 * 1024
            || !self.content_root.is_absolute()
            || !self.entry_url.starts_with("file:///")
            || !name.is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(super::cdp::CdpError(
                "Invalid isolated-browser readiness evidence".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::cdp::CdpError;
    use processkit::{Command, OutputBufferPolicy, ProcessGroup, ProcessGroupOptions, StdioMode};
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Mutex,
    };
    use std::time::{Duration, Instant};
    use tokio_stream::StreamExt;

    const MAX_FRAME: usize = 16 * 1024 * 1024;
    // ProcessKit's fail-loud policy limits cumulative output, not its current
    // backlog. Keep a separate session ceiling and inspect it while streaming;
    // the consumer queue below independently limits outstanding data to 64 MiB.
    const MAX_SESSION_LINES: usize = 100_000;
    const MAX_SESSION_BYTES: usize = 256 * 1024 * 1024;
    static ACTIVE: AtomicBool = AtomicBool::new(false);

    struct Admission;
    impl Drop for Admission {
        fn drop(&mut self) {
            ACTIVE.store(false, Ordering::Release);
        }
    }

    enum OwnerCommand {
        Send(String, SyncSender<Result<(), String>>),
        Stop(SyncSender<Result<(), String>>),
    }

    pub struct RehearsalOwner {
        commands: Option<SyncSender<OwnerCommand>>,
        folder: PathBuf,
    }

    pub struct PipeTransport {
        commands: SyncSender<OwnerCommand>,
        events: Receiver<String>,
        failure: Arc<Mutex<Option<String>>>,
        queued_bytes: Arc<AtomicUsize>,
    }

    fn failure(state: &Arc<Mutex<Option<String>>>, message: impl Into<String>) {
        if let Ok(mut slot) = state.lock() {
            slot.get_or_insert(message.into());
        }
    }

    impl RehearsalOwner {
        pub fn start(
            python: &std::path::Path,
            config: RehearsalConfig,
        ) -> Result<(Self, PipeTransport, RehearsalReady), CdpError> {
            if ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(CdpError("Another isolated rehearsal is active".into()));
            }
            let admission = Admission;
            if !python.is_absolute()
                || !python.is_file()
                || config.schema != 1
                || !(5..=3600).contains(&config.seconds)
            {
                return Err(CdpError("Invalid isolated-browser setup".into()));
            }
            let encoded = serde_json::to_string(&config)
                .map_err(|_| CdpError("Invalid rehearsal configuration".into()))?;
            if encoded.len() > 1024 * 1024 {
                return Err(CdpError(
                    "Rehearsal configuration exceeds its budget".into(),
                ));
            }
            let folder = std::env::temp_dir().join(format!(
                "smart-jobseeker-rehearsal-worker-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| CdpError("Clock unavailable".into()))?
                    .as_nanos()
            ));
            std::fs::create_dir(&folder)
                .map_err(|e| CdpError(format!("Cannot prepare rehearsal worker: {e}")))?;
            let script = folder.join("rehearsal_worker.py");
            if let Err(error) = std::fs::write(&script, include_bytes!("rehearsal_worker.py")) {
                let _ = std::fs::remove_dir(&folder);
                return Err(CdpError(format!(
                    "Cannot prepare rehearsal worker: {error}"
                )));
            }
            let python = python.to_owned();
            let owned_folder = folder.clone();
            let lifetime = Duration::from_secs(config.seconds + 60);
            let (commands, command_rx) = mpsc::sync_channel(4);
            let (events, event_rx) = mpsc::sync_channel(256);
            let (ready, startup) = mpsc::sync_channel(1);
            let state = Arc::new(Mutex::new(None));
            let worker_state = state.clone();
            let queued_bytes = Arc::new(AtomicUsize::new(0));
            let worker_bytes = queued_bytes.clone();
            let spawn = std::thread::Builder::new()
                .name("smart-jobseeker-rehearsal-owner".into())
                .spawn(move || {
                    let _admission = admission;
                    let work = || -> Result<(), String> {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| e.to_string())?;
                        // ProcessKit constructs its line-stream timer eagerly,
                        // before the first poll inside block_on.
                        let _context = runtime.enter();
                        let group = ProcessGroup::with_options(
                            ProcessGroupOptions::default().max_memory(3 * 1024 * 1024 * 1024),
                        )
                        .map_err(|e| e.to_string())?;
                        let command = Command::new(python)
                            .args(["-I", "-B", "-u"])
                            .arg(script)
                            .create_no_window()
                            .keep_stdin_open()
                            .stderr(StdioMode::Null)
                            .timeout(lifetime)
                            .output_buffer(
                                OutputBufferPolicy::fail_loud(MAX_SESSION_LINES)
                                    .with_max_bytes(MAX_SESSION_BYTES),
                            );
                        let mut process = runtime
                            .block_on(group.start(&command))
                            .map_err(|e| e.to_string())?;
                        let mut input =
                            process.take_stdin().ok_or("Rehearsal stdin unavailable")?;
                        let mut lines = process.stdout_lines().map_err(|e| e.to_string())?;
                        runtime.block_on(async {
                            tokio::time::timeout(Duration::from_secs(2), input.write_line(&encoded))
                                .await
                                .map_err(|_| "Rehearsal setup write timed out")?
                                .map_err(|e| e.to_string())
                        })?;
                        let first = runtime.block_on(async {
                            tokio::time::timeout(Duration::from_secs(60), lines.next())
                                .await
                                .map_err(|_| "Rehearsal startup timed out")?
                                .ok_or("Rehearsal worker ended before readiness")
                        })?;
                        if first.len() > 8192 {
                            return Err("Invalid rehearsal readiness message".into());
                        }
                        let initial: serde_json::Value = serde_json::from_str(&first)
                            .map_err(|_| "Invalid rehearsal startup protocol")?;
                        if initial["type"] == "worker_error" {
                            return Err(format!(
                                "Rehearsal setup failed: {}",
                                initial["code"].as_str().unwrap_or("unknown")
                            ));
                        }
                        let evidence: RehearsalReady =
                            serde_json::from_value(initial).map_err(|_| {
                                "Rehearsal worker did not produce valid readiness evidence"
                            })?;
                        evidence.validate().map_err(|e| e.to_string())?;
                        if ready.send(Ok(evidence)).is_err() {
                            return Err("Rehearsal startup was abandoned".into());
                        }
                        let deadline = Instant::now() + lifetime;
                        let mut stopping: Option<(Instant, SyncSender<Result<(), String>>)> = None;
                        let mut clean = false;
                        loop {
                            if process.stdout_line_count() > MAX_SESSION_LINES
                                || process.stdout_bytes_seen() > MAX_SESSION_BYTES
                            {
                                return Err("Rehearsal session output budget exhausted".into());
                            }
                            if Instant::now() >= deadline {
                                return Err("Rehearsal session budget exhausted".into());
                            }
                            if stopping
                                .as_ref()
                                .is_some_and(|(deadline, _)| Instant::now() >= *deadline)
                            {
                                return Err("Rehearsal shutdown timed out".into());
                            }
                            match command_rx.try_recv() {
                                Ok(OwnerCommand::Send(message, reply)) => {
                                    let result = runtime.block_on(async {
                                        tokio::time::timeout(
                                            Duration::from_secs(2),
                                            input.write_line(&message),
                                        )
                                        .await
                                        .map_err(|_| {
                                            "Rehearsal command write timed out".to_string()
                                        })?
                                        .map_err(|e| e.to_string())
                                    });
                                    let failed = result.is_err();
                                    let _ = reply.send(result);
                                    if failed {
                                        return Err("Rehearsal command write failed".into());
                                    }
                                }
                                Ok(OwnerCommand::Stop(reply)) => {
                                    runtime.block_on(async {
                                        tokio::time::timeout(
                                            Duration::from_secs(1),
                                            input.write_line("{\"op\":\"close\"}"),
                                        )
                                        .await
                                        .map_err(|_| "Rehearsal close write timed out")?
                                        .map_err(|e| e.to_string())
                                    })?;
                                    stopping =
                                        Some((Instant::now() + Duration::from_secs(6), reply));
                                }
                                Err(TryRecvError::Disconnected) => {
                                    return Err("Rehearsal owner disconnected".into())
                                }
                                Err(TryRecvError::Empty) => (),
                            }
                            let line = runtime.block_on(async {
                                tokio::time::timeout(Duration::from_millis(10), lines.next()).await
                            });
                            match line {
                                Err(_) => (),
                                Ok(None) => break,
                                Ok(Some(line)) => {
                                    if line.len() > MAX_FRAME {
                                        return Err("Rehearsal response exceeds its budget".into());
                                    }
                                    // Browser DOM responses use the CDP parser's own
                                    // bounded deeper decoder, not serde's default
                                    // recursion limit in this forwarding layer.
                                    let control: Option<serde_json::Value> = if line
                                        .starts_with("{\"type\":")
                                    {
                                        Some(
                                            serde_json::from_str(&line)
                                                .map_err(|_| "Invalid rehearsal control message")?,
                                        )
                                    } else {
                                        None
                                    };
                                    match control.as_ref().and_then(|v| v["type"].as_str()) {
                                        Some("closed") => {
                                            clean = control
                                                .as_ref()
                                                .is_some_and(|v| v["cleanup_complete"] == true)
                                        }
                                        Some("worker_error") => {
                                            return Err(format!(
                                                "Rehearsal worker failed: {}",
                                                control
                                                    .as_ref()
                                                    .and_then(|v| v["code"].as_str())
                                                    .unwrap_or("unknown")
                                            ))
                                        }
                                        _ => {
                                            let size = line.len();
                                            if worker_bytes
                                                .try_update(
                                                    Ordering::AcqRel,
                                                    Ordering::Acquire,
                                                    |used| {
                                                        used.checked_add(size)
                                                            .filter(|n| *n <= 64 * 1024 * 1024)
                                                    },
                                                )
                                                .is_err()
                                            {
                                                return Err(
                                                    "Rehearsal event byte budget exhausted".into(),
                                                );
                                            }
                                            if events.try_send(line).is_err() {
                                                worker_bytes.fetch_sub(size, Ordering::AcqRel);
                                                return Err(
                                                    "Rehearsal event budget exhausted".into()
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let outcome = runtime.block_on(async {
                            tokio::time::timeout(Duration::from_secs(2), process.wait())
                                .await
                                .map_err(|_| "Rehearsal exit timed out")?
                                .map_err(|e| e.to_string())
                        })?;
                        if !clean || outcome.code() != Some(0) {
                            return Err("Rehearsal cleanup or worker exit was incomplete".into());
                        }
                        if let Some((_, reply)) = stopping {
                            let _ = reply.send(Ok(()));
                        } else {
                            failure(
                                &worker_state,
                                "Rehearsal browser ended without a close request",
                            );
                        }
                        Ok(())
                    };
                    if let Err(error) = work() {
                        failure(&worker_state, error.clone());
                        let _ = ready.try_send(Err(error));
                    }
                    // Only this newly created controller directory is removed here.
                    // The worker owns its separate AppContainer profile lifecycle.
                    let _ = std::fs::remove_file(owned_folder.join("rehearsal_worker.py"));
                    let _ = std::fs::remove_dir(&owned_folder);
                });
            if let Err(error) = spawn {
                let _ = std::fs::remove_file(folder.join("rehearsal_worker.py"));
                let _ = std::fs::remove_dir(&folder);
                return Err(CdpError(format!("Cannot start rehearsal owner: {error}")));
            }
            let mut owner = Self {
                commands: Some(commands.clone()),
                folder,
            };
            let evidence = match startup.recv_timeout(Duration::from_secs(65)) {
                Ok(Ok(evidence)) => evidence,
                Ok(Err(error)) => return Err(CdpError(error)),
                Err(_) => {
                    let _ = owner.shutdown();
                    return Err(CdpError("Rehearsal startup did not complete".into()));
                }
            };
            Ok((
                owner,
                PipeTransport {
                    commands,
                    events: event_rx,
                    failure: state,
                    queued_bytes,
                },
                evidence,
            ))
        }

        pub fn shutdown(&mut self) -> Result<(), CdpError> {
            let Some(commands) = self.commands.take() else {
                return Ok(());
            };
            let (reply, received) = mpsc::sync_channel(1);
            commands
                .try_send(OwnerCommand::Stop(reply))
                .map_err(|_| CdpError("Rehearsal owner is unavailable".into()))?;
            received
                .recv_timeout(Duration::from_secs(8))
                .map_err(|_| CdpError("Rehearsal cleanup was not confirmed".into()))?
                .map_err(CdpError)
        }
    }

    impl Drop for RehearsalOwner {
        fn drop(&mut self) {
            if let Err(error) = self.shutdown() {
                eprintln!("Isolated browser cleanup: {error}");
            }
            let _ = std::fs::remove_file(self.folder.join("rehearsal_worker.py"));
            let _ = std::fs::remove_dir(&self.folder);
        }
    }

    impl PipeTransport {
        fn check(&self) -> Result<(), CdpError> {
            match self.failure.lock() {
                Ok(state) => state
                    .as_ref()
                    .map_or(Ok(()), |error| Err(CdpError(error.clone()))),
                Err(_) => Err(CdpError("Rehearsal state unavailable".into())),
            }
        }
        pub fn send(&mut self, message: String) -> Result<(), CdpError> {
            self.check()?;
            if message.len() > MAX_FRAME {
                return Err(CdpError("Rehearsal command exceeds its budget".into()));
            }
            let (reply, response) = mpsc::sync_channel(1);
            self.commands
                .try_send(OwnerCommand::Send(message, reply))
                .map_err(|_| CdpError("Rehearsal command queue unavailable".into()))?;
            response
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| CdpError("Rehearsal command was not acknowledged".into()))?
                .map_err(CdpError)
        }
        pub fn receive(&mut self, timeout: Duration) -> Result<Option<String>, CdpError> {
            self.check()?;
            match self.events.recv_timeout(timeout) {
                Ok(value) => {
                    self.queued_bytes.fetch_sub(value.len(), Ordering::AcqRel);
                    self.check()?;
                    Ok(Some(value))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.check()?;
                    Ok(None)
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.check()?;
                    Err(CdpError("Rehearsal protocol connection closed".into()))
                }
            }
        }
    }
}

#[cfg(windows)]
pub(crate) use windows::{PipeTransport, RehearsalOwner};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_rejects_capabilities_and_unrelated_profile_identities() {
        let good = serde_json::json!({"type":"ready","schema":1,"appcontainer":true,"capabilities":0,
            "pid":1,"content_root":std::env::temp_dir(),"entry":"index.html","entry_url":"file:///fictional/index.html",
            "profile_name":"SmartJobseeker.Rehearsal.0123456789abcdef0123456789abcdef","job_memory_limit":2147483648u64});
        assert!(serde_json::from_value::<RehearsalReady>(good.clone())
            .unwrap()
            .validate()
            .is_ok());
        for (key, value) in [
            ("capabilities", serde_json::json!(1)),
            ("appcontainer", serde_json::json!(false)),
            ("profile_name", serde_json::json!("unrelated")),
            ("pid", serde_json::json!(0)),
        ] {
            let mut changed = good.clone();
            changed[key] = value;
            assert!(serde_json::from_value::<RehearsalReady>(changed)
                .unwrap()
                .validate()
                .is_err());
        }
    }
}
