//! Retain a bounded Windows process tree until explicit shutdown or owner exit.
use processkit::{Command, MemberInfo, ProcessGroup, ProcessGroupOptions, RunningProcess};
use std::{
    sync::mpsc::{self, Sender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

const GIB: u64 = 1024 * 1024 * 1024;

fn host_headroom(free: u64, total: u64) -> Result<(), String> {
    if total == 0 || free > total || free < 3 * GIB {
        return Err("The owned runtime stopped to preserve host RAM".into());
    }
    Ok(())
}

pub(crate) fn memory_budget(free: u64, total: u64) -> Result<u64, String> {
    host_headroom(free, total)?;
    if free < 5 * GIB {
        return Err("Runtime startup needs at least 5 GiB free RAM".into());
    }
    Ok((free - 3 * GIB).min(20 * GIB))
}

enum Request {
    Check(Sender<Result<bool, String>>),
    Stop,
}

fn root_running(group: &ProcessGroup, identity: &MemberInfo) -> Result<bool, String> {
    if !group
        .members()
        .map_err(|e| e.to_string())?
        .contains(&identity.pid())
    {
        return Ok(false);
    }
    processkit::process_is_alive(identity.pid(), identity.start_time()).map_err(|e| e.to_string())
}

pub(crate) struct OwnedRuntime {
    commands: Option<Sender<Request>>,
    worker: Option<JoinHandle<Result<(), String>>>,
    identity: MemberInfo,
}

fn drain(
    runtime: &tokio::runtime::Runtime,
    group: ProcessGroup,
    process: RunningProcess,
) -> Result<(), String> {
    let killed = group.kill_all().map_err(|e| e.to_string());
    let waited = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), process.wait())
            .await
            .map_err(|_| "Runtime exit was not confirmed".to_owned())?
            .map_err(|e| e.to_string())
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let drained = loop {
        match group.members() {
            Ok(members) if members.is_empty() => break Ok(()),
            Ok(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(_) => break Err("Runtime descendants did not exit".to_owned()),
            Err(error) => break Err(error.to_string()),
        }
    };
    // Even a failed observation must close the kill-on-close job handle.
    drop(group);
    killed.and(waited.map(|_| ())).and(drained)
}

impl OwnedRuntime {
    pub(crate) fn spawn(command: Command, memory_limit: u64) -> Result<Self, String> {
        Self::spawn_with_deadline(command, memory_limit, Duration::from_secs(10), || {})
    }

    fn spawn_with_deadline(
        command: Command,
        memory_limit: u64,
        ready_timeout: Duration,
        before_handoff: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let expected = std::path::PathBuf::from(command.program());
        if !expected.is_absolute() || !expected.is_file() || memory_limit == 0 {
            return Err("Runtime executable or memory budget is invalid".into());
        }
        let (commands, receive) = mpsc::channel();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("smart-jobseeker-runtime-owner".into())
            .spawn(move || {
                let setup = || -> Result<_, String> {
                    let system =
                        sysinfo::System::new().map_err(|_| "Cannot monitor runtime memory")?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    let group = ProcessGroup::with_options(
                        ProcessGroupOptions::default().max_memory(memory_limit),
                    )
                    .map_err(|e| e.to_string())?;
                    let process = runtime.block_on(async {
                        tokio::time::timeout(Duration::from_secs(5), group.start(&command))
                            .await
                            .map_err(|_| "Runtime process creation timed out".to_owned())?
                            .map_err(|e| e.to_string())
                    })?;
                    Ok((runtime, group, process, system))
                };
                let (runtime, group, process, mut system) = match setup() {
                    Ok(owned) => owned,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return Ok(());
                    }
                };
                let identify = || -> Result<MemberInfo, String> {
                    let pid = process.pid().ok_or("Runtime PID is unavailable")?;
                    let identity = group
                        .members_info()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|member| member.pid() == pid)
                        .ok_or("Runtime containment identity is unavailable")?;
                    if identity.start_time().is_none()
                        || identity.ppid() != Some(std::process::id())
                        || identity.exe_name().is_none_or(|name| {
                            expected.file_name().is_none_or(|file| {
                                !name.eq_ignore_ascii_case(&file.to_string_lossy())
                            })
                        })
                    {
                        return Err("Runtime creation identity could not be verified".into());
                    }
                    Ok(identity)
                };
                let identity = match identify() {
                    Ok(identity) => identity,
                    Err(error) => {
                        let result = drain(&runtime, group, process);
                        let message = match &result {
                            Ok(()) => error,
                            Err(cleanup) => format!("{error}; cleanup failed: {cleanup}"),
                        };
                        let _ = ready.send(Err(message));
                        return result;
                    }
                };
                before_handoff();
                if ready.send(Ok(identity.clone())).is_err() {
                    return drain(&runtime, group, process);
                }
                let mut sampled = Instant::now();
                let reason = loop {
                    match receive.recv_timeout(Duration::from_secs(1)) {
                        Ok(Request::Stop) => return drain(&runtime, group, process),
                        Ok(Request::Check(reply)) => {
                            let state = root_running(&group, &identity);
                            let running = state.as_ref().is_ok_and(|alive| *alive);
                            let _ = reply.send(state);
                            if !running {
                                break "Runtime root exited or its identity changed";
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            break "Runtime owner was released"
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if sampled.elapsed() >= Duration::from_secs(1) {
                        system.refresh_memory();
                        sampled = Instant::now();
                        if host_headroom(system.available_memory(), system.total_memory()).is_err()
                        {
                            break "The owned runtime stopped to preserve host RAM";
                        }
                    }
                    if !root_running(&group, &identity).unwrap_or(false) {
                        break "Runtime root exited or its identity changed";
                    }
                };
                let result = drain(&runtime, group, process);
                if let Err(error) = &result {
                    eprintln!("{reason}; cleanup failed: {error}");
                }
                result
            })
            .map_err(|e| e.to_string())?;
        match started.recv_timeout(ready_timeout) {
            Ok(Ok(identity)) => Ok(Self {
                commands: Some(commands),
                worker: Some(worker),
                identity,
            }),
            result => {
                drop(commands);
                worker
                    .join()
                    .map_err(|_| "Runtime startup owner failed during cleanup")??;
                Err(match result {
                    Ok(Err(error)) => error,
                    _ => "Runtime ownership setup timed out".into(),
                })
            }
        }
    }

    pub(crate) fn identity(&self) -> &MemberInfo {
        &self.identity
    }

    pub(crate) fn running(&self) -> Result<bool, String> {
        let (send, receive) = mpsc::channel();
        self.commands
            .as_ref()
            .ok_or("Runtime owner was released")?
            .send(Request::Check(send))
            .map_err(|_| "Runtime ownership worker stopped")?;
        receive
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "Runtime state could not be verified")?
    }

    pub(crate) fn stop(&mut self) -> Result<(), String> {
        if let Some(commands) = self.commands.take() {
            // A disconnected receiver may mean the root already exited. The
            // joined worker result proves whether its tree actually drained.
            let _ = commands.send(Request::Stop);
            drop(commands);
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "Runtime ownership worker failed during teardown")??;
        }
        Ok(())
    }
}

impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("Runtime cleanup failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};

    fn folder() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("runtime-owner-test-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap()
    }

    fn command(folder: &Path, role: &str) -> Command {
        use processkit::{Stdin, StdioMode};
        Command::new(std::env::current_exe().unwrap())
            .args([
                "owned_runtime::tests::child_fixture",
                "--ignored",
                "--exact",
                "--nocapture",
            ])
            .env("SJS_RUNTIME_FIXTURE", folder)
            .env("SJS_RUNTIME_FIXTURE_ROLE", role)
            .stdin(Stdin::empty())
            .stdout(StdioMode::Null)
            .stderr(StdioMode::Null)
            .create_no_window()
    }

    fn wait_file(path: &Path) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !path.is_file() {
            assert!(
                Instant::now() < deadline,
                "Fixture did not create {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn identity(pid: u32) -> Value {
        let info = processkit::process_info(pid).unwrap().unwrap();
        assert!(info.start_time().is_some());
        json!({"pid":pid,"start":info.start_time(),"parent":info.ppid(),"executable":info.exe_name()})
    }

    struct FixtureProcess(std::process::Child);
    impl Drop for FixtureProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn fixture_process(folder: &Path, role: &str) -> FixtureProcess {
        use std::os::windows::process::CommandExt;
        FixtureProcess(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "owned_runtime::tests::child_fixture",
                    "--ignored",
                    "--exact",
                ])
                .env("SJS_RUNTIME_FIXTURE", folder)
                .env("SJS_RUNTIME_FIXTURE_ROLE", role)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap(),
        )
    }

    fn gone(member: &Value) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while processkit::process_is_alive(
            member["pid"].as_u64().unwrap() as u32,
            member["start"].as_u64(),
        )
        .unwrap()
        {
            assert!(
                Instant::now() < deadline,
                "Owned fixture survived teardown: {member}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn startup_budget_preserves_host_headroom() {
        assert_eq!(memory_budget(5 * GIB, 16 * GIB).unwrap(), 2 * GIB);
        assert_eq!(memory_budget(64 * GIB, 64 * GIB).unwrap(), 20 * GIB);
        for (free, total) in [(0, 0), (4 * GIB, 16 * GIB), (17 * GIB, 16 * GIB)] {
            assert!(memory_budget(free, total).is_err());
        }
    }

    #[test]
    fn explicit_stop_drains_descendants_and_is_idempotent() {
        let folder = folder();
        let mut owner = OwnedRuntime::spawn(command(folder.path(), "root"), GIB).unwrap();
        let members = wait_file(&folder.path().join("tree.json"));
        assert_eq!(members["root"]["pid"], owner.identity().pid());
        assert!(owner.running().unwrap());
        owner.stop().unwrap();
        owner.stop().unwrap();
        gone(&members["root"]);
        gone(&members["child"]);
    }

    #[test]
    fn abandoned_handoff_drains_the_started_tree() {
        let folder = folder();
        let observed = folder.path().join("tree.json");
        let wait = observed.clone();
        let result = OwnedRuntime::spawn_with_deadline(
            command(folder.path(), "root"),
            GIB,
            Duration::from_millis(1),
            move || {
                wait_file(&wait);
            },
        );
        assert!(result.is_err());
        let members = wait_file(&observed);
        gone(&members["root"]);
        gone(&members["child"]);
    }

    #[test]
    fn root_exit_does_not_leave_its_child_running() {
        let folder = folder();
        let mut owner = OwnedRuntime::spawn(command(folder.path(), "root"), GIB).unwrap();
        let members = wait_file(&folder.path().join("tree.json"));
        std::fs::write(folder.path().join("exit-root"), b"exit").unwrap();
        gone(&members["root"]);
        gone(&members["child"]);
        owner.stop().unwrap();
    }

    #[test]
    fn stop_after_observed_root_exit_uses_the_owner_teardown_result() {
        let folder = folder();
        let mut owner = OwnedRuntime::spawn(command(folder.path(), "root"), GIB).unwrap();
        let members = wait_file(&folder.path().join("tree.json"));
        std::fs::write(folder.path().join("exit-root"), b"exit").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while owner.running().unwrap_or(false) {
            assert!(Instant::now() < deadline, "Fixture root did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        owner.stop().unwrap();
        gone(&members["root"]);
        gone(&members["child"]);
    }

    #[test]
    fn owner_process_exit_closes_its_job_and_preserves_an_unrelated_process() {
        let folder = folder();
        let mut owner = fixture_process(folder.path(), "owner");
        let owner_identity = identity(owner.0.id());
        let unrelated = fixture_process(folder.path(), "child");
        let unrelated_identity = identity(unrelated.0.id());
        wait_file(&folder.path().join("owner.json"));
        let members = wait_file(&folder.path().join("tree.json"));
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        drop(owner);
        gone(&owner_identity);
        gone(&members["root"]);
        gone(&members["child"]);
        assert!(processkit::process_is_alive(
            unrelated.0.id(),
            unrelated_identity["start"].as_u64()
        )
        .unwrap());
        drop(unrelated);
        gone(&unrelated_identity);
    }

    #[test]
    #[ignore = "owned subprocess fixture; run only with an explicit private fixture directory"]
    fn child_fixture() {
        let folder = PathBuf::from(
            std::env::var_os("SJS_RUNTIME_FIXTURE").expect("Private fixture directory required"),
        );
        let target = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert!(folder.canonicalize().unwrap().starts_with(target));
        assert!(folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("runtime-owner-test-"));
        let role = std::env::var("SJS_RUNTIME_FIXTURE_ROLE").unwrap();
        if role == "owner" {
            let _owned = OwnedRuntime::spawn(command(&folder, "root"), GIB).unwrap();
            wait_file(&folder.join("tree.json"));
            std::fs::write(folder.join("owner.json"), b"{}").unwrap();
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        } else if role == "root" {
            use std::os::windows::process::CommandExt;
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "owned_runtime::tests::child_fixture",
                    "--ignored",
                    "--exact",
                ])
                .env("SJS_RUNTIME_FIXTURE_ROLE", "child")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap();
            let data = json!({"root":identity(std::process::id()),"child":identity(child.id())});
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            std::fs::write(folder.join("tree.tmp"), serde_json::to_vec(&data).unwrap()).unwrap();
            std::fs::rename(folder.join("tree.tmp"), folder.join("tree.json")).unwrap();
            while !folder.join("exit-root").is_file() {
                std::thread::sleep(Duration::from_millis(25));
            }
            // Deliberately leave the child to the production job owner.
        } else {
            assert_eq!(role, "child");
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
