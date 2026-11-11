//! Explicitly isolated owner-loop qualification; fictional runtime, no model.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct RuntimeHold {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    ready: mpsc::Receiver<()>,
    worker: Option<JoinHandle<bool>>,
}
impl RuntimeHold {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (sender, ready) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut stream = loop {
                if flag.load(Ordering::Acquire) {
                    return false;
                }
                assert!(Instant::now() < deadline, "Runtime was never contacted");
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("Runtime accept: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let mut request = Vec::new();
            let mut notified = false;
            loop {
                if flag.load(Ordering::Acquire) {
                    return false;
                }
                assert!(Instant::now() < deadline, "Fit client did not disconnect");
                let mut bytes = [0; 1024];
                match stream.read(&mut bytes) {
                    Ok(0) => return notified,
                    Ok(n) => {
                        assert!(!notified, "Unexpected additional runtime data");
                        request.extend_from_slice(&bytes[..n]);
                        assert!(request.len() <= 8192);
                        if request.windows(4).any(|b| b == b"\r\n\r\n") {
                            assert!(request.starts_with(b"GET /api/ps "));
                            sender.send(()).unwrap();
                            notified = true;
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        return notified
                    }
                    Err(e) => panic!("Runtime read: {e}"),
                }
            }
        });
        Self {
            address,
            stop,
            ready,
            worker: Some(worker),
        }
    }
    fn finish(&mut self) {
        assert!(
            self.worker.take().unwrap().join().unwrap(),
            "No observed client disconnect"
        );
    }
}
impl Drop for RuntimeHold {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                assert!(result.is_ok());
            }
        }
    }
}
struct Owner {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Owner {
    fn finish(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                assert!(result.is_ok());
            }
        }
    }
}
fn request(port: u16, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let method = if body.is_some() { "POST" } else { "GET" };
    let body = body.map_or_else(String::new, |value| value.to_string());
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fictional-fit-control\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.take(65536).read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        serde_json::from_str(body).unwrap(),
    )
}

#[test]
#[ignore = "isolated APPDATA, full owner loop and held fictional runtime; no model/browser"]
fn fit_preflight_remains_responsive_and_drains_on_cancel_or_shutdown() {
    assert_eq!(
        std::env::var("WAYPOINT_FIT_TEST_ISOLATED").as_deref(),
        Ok("1"),
        "Run with isolated APPDATA and WAYPOINT_FIT_TEST_ISOLATED=1"
    );
    for cancel in [true, false] {
        let mut runtime = RuntimeHold::start();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        ws.save_candidate_profile(&json!({"instructions":"Fictional Rust role"}).to_string())
            .unwrap();
        let job = crate::manual_job::add(&mut ws, &json!({"url":"https://example.org/fictional-fit","title":"Engineer","employer":"Fictional"}), 1).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let address = runtime.address;
        let worker = std::thread::spawn(move || {
            crate::serve_loop_with_runtime(
                ws,
                listener,
                "fictional-fit-control".into(),
                port,
                flag,
                address,
            )
        });
        let mut owner = Owner {
            stop,
            worker: Some(worker),
        };
        let started = Instant::now();
        let (code, result) = request(port, "/api/fit/start", Some(json!({"id":job["id"]})));
        assert_eq!(code, 200, "{result}");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "Start blocked on model preflight"
        );
        runtime.ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let id = result["task_id"].as_str().unwrap();
        let started = Instant::now();
        assert_eq!(request(port, "/api/model-preferences", None).0, 200);
        assert_eq!(
            request(port, "/api/fit/status", Some(json!({"task_id":id}))).1["phase"],
            "checking_model"
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "Settings/status blocked behind model preflight"
        );
        if cancel {
            assert_ne!(
                request(port, "/api/fit/cancel", Some(json!({"task_id":"other"}))).0,
                200
            );
            assert_eq!(
                request(port, "/api/fit/status", Some(json!({"task_id":id}))).1["phase"],
                "checking_model"
            );
            let started = Instant::now();
            assert_eq!(
                request(port, "/api/fit/cancel", Some(json!({"task_id":id}))).1["status"],
                "stopping"
            );
            let terminal = loop {
                let (code, value) = request(port, "/api/fit/status", Some(json!({"task_id":id})));
                assert_eq!(code, 200);
                if value["status"] != "running" {
                    break value;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(2),
                    "Cancellation did not drain"
                );
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(terminal["status"], "stopped");
            assert!(terminal["assessment"].is_null());
            assert_eq!(
                request(port, "/api/fit/cancel", Some(json!({"task_id":id}))).1["status"],
                "finished"
            );
            assert_eq!(
                request(port, "/api/fit/pending", Some(json!({}))).1["active"],
                false
            );
        }
        let started = Instant::now();
        owner.finish();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "Owner shutdown did not drain fit worker"
        );
        runtime.finish();
        assert_eq!(crate::fit_task::pending().unwrap()["active"], false);
    }
}

#[test]
#[ignore = "isolated APPDATA, full owner loop and held fictional runtime; no browser launch"]
fn form_preflight_remains_responsive_and_drains_before_browser_launch() {
    assert_eq!(
        std::env::var("WAYPOINT_FIT_TEST_ISOLATED").as_deref(),
        Ok("1")
    );
    for cancel in [true, false] {
        let mut runtime = RuntimeHold::start();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let job=crate::manual_job::add(&mut ws,&json!({"url":"https://boards.greenhouse.io/fictional-control/jobs/12345","title":"Fictional Engineer","employer":"Fictional"}),1).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let address = runtime.address;
        let worker = std::thread::spawn(move || {
            crate::serve_loop_with_runtime(
                ws,
                listener,
                "fictional-fit-control".into(),
                port,
                flag,
                address,
            )
        });
        let mut owner = Owner {
            stop,
            worker: Some(worker),
        };
        let (code, value) = request(port, "/api/shortlist", Some(json!({"id":job["id"]})));
        assert_eq!(code, 200, "{value}");
        let started = Instant::now();
        let (code, value) = request(
            port,
            "/api/form/inspect-start",
            Some(json!({"id":job["id"],"visible":false})),
        );
        assert_eq!(code, 200, "{value}");
        assert!(started.elapsed() < Duration::from_millis(500));
        let id = value["task_id"].as_str().unwrap();
        runtime.ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let started = Instant::now();
        assert_eq!(request(port, "/api/model-preferences", None).0, 200);
        assert_eq!(
            request(
                port,
                "/api/form/inspect-status",
                Some(json!({"task_id":id}))
            )
            .1["phase"],
            "checking_model"
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        if cancel {
            assert_ne!(
                request(
                    port,
                    "/api/form/inspect-cancel",
                    Some(json!({"task_id":"other"}))
                )
                .0,
                200
            );
            let started = Instant::now();
            assert_eq!(
                request(
                    port,
                    "/api/form/inspect-cancel",
                    Some(json!({"task_id":id}))
                )
                .1["status"],
                "stopping"
            );
            let terminal = loop {
                let (code, value) = request(
                    port,
                    "/api/form/inspect-status",
                    Some(json!({"task_id":id})),
                );
                assert_eq!(code, 200);
                if value["status"] != "running" {
                    break value;
                }
                assert!(started.elapsed() < Duration::from_secs(2));
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(terminal["status"], "failed");
            assert!(
                terminal["error"]
                    .as_str()
                    .unwrap()
                    .starts_with("Form inspection was stopped"),
                "{terminal}"
            );
            assert!(terminal["preview"].is_null());
            assert!(!crate::form_task::busy());
        }
        let started = Instant::now();
        owner.finish();
        assert!(started.elapsed() < Duration::from_secs(2));
        runtime.finish();
        assert!(!crate::form_task::busy());
    }
}
