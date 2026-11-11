//! One owned, cancellable metadata read; no model execution or workspace writes.
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const PATH: &str = "/api/model-inventory";
const WAITERS: usize = 4;
const DEADLINE: Duration = Duration::from_secs(3);

pub fn handles(method: &str, path: &str) -> bool {
    method == "GET" && path == PATH
}

#[derive(Debug)]
pub struct Snapshot {
    pub tags: Value,
    pub inventory: Value,
}

pub fn read(address: SocketAddr, cancelled: &AtomicBool) -> Result<Snapshot, String> {
    validate_address(address)?;
    let started = Instant::now();
    let guard = || {
        if cancelled.load(Ordering::Acquire) {
            Err("Local model inventory read was stopped".into())
        } else if started.elapsed() >= DEADLINE {
            Err("Local model inventory timed out. Check the runtime and try again.".into())
        } else {
            Ok(())
        }
    };
    let (status, bytes) = waypoint_inference::ollama::http_request_with_guard(
        address,
        "GET",
        "/api/tags",
        None,
        DEADLINE.as_millis() as u64,
        &guard,
    )?;
    if status != 200 {
        return Err(format!("Ollama returned HTTP {status}"));
    }
    guard()?;
    let tags: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid local model inventory")?;
    let inventory = crate::installed_model_inventory(&tags)?;
    guard()?;
    Ok(Snapshot { tags, inventory })
}

fn validate_address(address: SocketAddr) -> Result<(), String> {
    if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) || address.port() == 0 {
        return Err("Model inventory requires a literal local runtime address".into());
    }
    Ok(())
}

struct Job {
    waiters: Vec<usize>,
    cancelled: Arc<AtomicBool>,
    worker: JoinHandle<Result<Snapshot, String>>,
}

pub struct Pending {
    address: SocketAddr,
    job: Option<Job>,
}

impl Pending {
    pub fn new(address: SocketAddr) -> Result<Self, String> {
        validate_address(address)?;
        Ok(Self { address, job: None })
    }

    pub fn request(&mut self, connection: usize) -> Result<(), String> {
        if connection == 0 {
            return Err("Inventory connection identity is unavailable".into());
        }
        if let Some(job) = self.job.as_mut() {
            if job.waiters.contains(&connection) {
                return Ok(());
            }
            if job.cancelled.load(Ordering::Acquire) || job.waiters.len() >= WAITERS {
                return Err("Local inventory checks are busy. Try again shortly.".into());
            }
            job.waiters.push(connection);
            return Ok(());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let address = self.address;
        let worker = std::thread::Builder::new()
            .name("local-model-inventory".into())
            .spawn(move || read(address, &flag))
            .map_err(|_| "The local inventory worker could not start")?;
        self.job = Some(Job {
            waiters: vec![connection],
            cancelled,
            worker,
        });
        Ok(())
    }

    pub fn retain(&mut self, active: impl Fn(usize) -> bool) {
        if let Some(job) = self.job.as_mut() {
            job.waiters.retain(|id| active(*id));
            if job.waiters.is_empty() {
                job.cancelled.store(true, Ordering::Release);
            }
        }
    }

    pub fn collect(&mut self) -> Option<(Vec<usize>, Result<Snapshot, String>)> {
        if !self.job.as_ref()?.worker.is_finished() {
            return None;
        }
        let job = self.job.take().expect("finished worker");
        let result = job
            .worker
            .join()
            .unwrap_or_else(|_| Err("Local model inventory stopped unexpectedly".into()));
        Some((job.waiters, result))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancelled.store(true, Ordering::Release);
            if job.worker.join().is_err() {
                eprintln!("Local inventory worker stopped unexpectedly during shutdown");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{HttpFixture, Response};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{atomic::AtomicUsize, mpsc};

    const MODELS: &str = r#"{"models":[{"name":"fictional:12b","size":8000000000,"digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","details":{"format":"gguf"}}]}"#;

    fn address(fixture: &HttpFixture) -> SocketAddr {
        fixture
            .origin
            .trim_start_matches("http://")
            .parse()
            .unwrap()
    }
    fn collect(pending: &mut Pending) -> (Vec<usize>, Result<Snapshot, String>) {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = pending.collect() {
                return result;
            }
            assert!(
                Instant::now() < end,
                "Inventory completion deadline exceeded"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[test]
    fn inventory_reads_require_literal_local_address_and_honor_cancellation() {
        for addr in ["127.0.0.2:1", "[::1]:1", "192.0.2.1:11434", "127.0.0.1:0"] {
            assert!(Pending::new(addr.parse().unwrap()).is_err());
        }
        assert!(read("127.0.0.1:1".parse().unwrap(), &AtomicBool::new(true))
            .unwrap_err()
            .contains("stopped"));
    }

    #[test]
    fn inventory_errors_are_not_an_empty_success() {
        for (status, body, expected) in [
            (503, "{}", "HTTP 503"),
            (200, "invalid", "Invalid local model inventory"),
            (200, r#"{"models":null}"#, "did not report"),
        ] {
            let fixture = HttpFixture::start_routes(vec![(
                "/api/tags",
                Response {
                    status,
                    location: None,
                    body: body.into(),
                },
            )]);
            let error = read(address(&fixture), &AtomicBool::new(false)).unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn one_inventory_read_serves_bounded_waiters_without_caching_later_reads() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let release = Arc::new(AtomicBool::new(false));
        let flag = release.clone();
        let (started, ready) = mpsc::channel();
        let fixture = HttpFixture::start_with_observer(MODELS, move |path| {
            assert_eq!(path, "/api/tags");
            count.fetch_add(1, Ordering::AcqRel);
            let _ = started.send(());
            let end = Instant::now() + Duration::from_secs(2);
            while !flag.load(Ordering::Acquire) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        });
        let _release = Release(release.clone());
        let mut pending = Pending::new(address(&fixture)).unwrap();
        pending.request(1).unwrap();
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        pending.request(1).unwrap();
        for id in 2..=WAITERS {
            pending.request(id).unwrap();
        }
        assert!(pending.request(WAITERS + 1).is_err());
        assert_eq!(calls.load(Ordering::Acquire), 1);
        release.store(true, Ordering::Release);
        let (waiters, result) = collect(&mut pending);
        assert_eq!(waiters, vec![1, 2, 3, 4]);
        assert_eq!(
            result.unwrap().inventory["models"][0]["name"],
            "fictional:12b"
        );
        pending.request(9).unwrap();
        assert_eq!(collect(&mut pending).0, vec![9]);
        assert_eq!(
            calls.load(Ordering::Acquire),
            2,
            "A completed observation is not a permanent cache"
        );
    }

    #[test]
    fn dropping_inventory_owner_cancels_joins_and_closes_the_reader() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        std::thread::scope(|scope| {
            let (started, ready) = mpsc::channel();
            let server = scope.spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(4);
                let mut stream = loop {
                    assert!(
                        Instant::now() < deadline,
                        "Runtime accept deadline exceeded"
                    );
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("Runtime accept failed: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                let mut notified = false;
                loop {
                    assert!(Instant::now() < deadline, "Cancelled reader did not close");
                    match stream.read(&mut buffer) {
                        Ok(0) => return notified,
                        Ok(n) => {
                            request.extend_from_slice(&buffer[..n]);
                            assert!(request.len() <= 8192);
                            if !notified && request.windows(4).any(|s| s == b"\r\n\r\n") {
                                assert!(request.starts_with(b"GET /api/tags "));
                                started.send(()).unwrap();
                                notified = true;
                            }
                        }
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::ConnectionAborted
                            ) =>
                        {
                            return notified
                        }
                        Err(error) => panic!("Runtime read failed: {error}"),
                    }
                }
            });
            let mut pending = Pending::new(addr).unwrap();
            pending.request(1).unwrap();
            ready.recv_timeout(Duration::from_secs(1)).unwrap();
            let start = Instant::now();
            drop(pending);
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "Shutdown waited for the full inventory deadline"
            );
            assert!(
                server.join().unwrap(),
                "Runtime never saw the inventory read"
            );
        });
    }

    struct Owner {
        stop: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.release.store(true, Ordering::Release);
            self.stop.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let result = worker.join();
                if !std::thread::panicking() {
                    assert!(result.is_ok());
                }
            }
        }
    }
    fn request(port: u16, path: &str) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream,"GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fictional-control-token\r\n\r\n").unwrap();
        stream
    }

    #[test]
    #[ignore = "isolated full owner loop and fictional local runtime; no model or browser"]
    fn deferred_inventory_keeps_real_control_requests_responsive() {
        let release = Arc::new(AtomicBool::new(false));
        let flag = release.clone();
        let (started, ready) = mpsc::channel();
        let fixture = HttpFixture::start_with_observer(MODELS, move |_| {
            let _ = started.send(());
            let end = Instant::now() + Duration::from_secs(5);
            while !flag.load(Ordering::Acquire) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        });
        let runtime = address(&fixture);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            crate::serve_loop_with_runtime(
                ws,
                listener,
                "fictional-control-token".into(),
                port,
                worker_stop,
                runtime,
            )
        });
        let _owner = Owner {
            stop,
            release: release.clone(),
            worker: Some(worker),
        };
        let mut inventories = vec![request(port, PATH)];
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 1..WAITERS {
            inventories.push(request(port, PATH));
        }
        inventories.push(request(port, PATH));
        for stream in &inventories {
            stream.set_nonblocking(true).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        // Concurrent arrivals have no guaranteed TCP ordering. Exactly one
        // overflow must be refused before the held metadata can complete.
        let (index, first) = loop {
            assert!(
                Instant::now() < deadline,
                "Inventory saturation did not return a response"
            );
            let mut found = None;
            for (i, stream) in inventories.iter_mut().enumerate() {
                let mut byte = [0];
                match stream.read(&mut byte) {
                    Ok(1) => {
                        found = Some((i, byte[0]));
                        break;
                    }
                    Ok(_) => panic!("Inventory connection closed without a refusal"),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(error) => panic!("Inventory read failed: {error}"),
                }
            }
            if let Some(found) = found {
                break found;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let mut refused = inventories.remove(index);
        refused.set_nonblocking(false).unwrap();
        let mut bytes = vec![first];
        refused.read_to_end(&mut bytes).unwrap();
        assert!(bytes.starts_with(b"HTTP/1.1 503"));
        for stream in &inventories {
            stream.set_nonblocking(false).unwrap();
        }
        let mut body = String::new();
        let start = Instant::now();
        let mut control = request(port, "/api/model-preferences");
        body.clear();
        control.read_to_string(&mut body).unwrap();
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "Control waited behind the inventory read"
        );
        assert!(body.starts_with("HTTP/1.1 200"));
        assert!(!release.load(Ordering::Acquire));
        inventories[0].set_nonblocking(true).unwrap();
        assert!(
            matches!(inventories[0].read(&mut [0;1]),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
        );
        inventories[0].set_nonblocking(false).unwrap();
        release.store(true, Ordering::Release);
        for mut inventory in inventories {
            body.clear();
            inventory.read_to_string(&mut body).unwrap();
            assert!(body.starts_with("HTTP/1.1 200"));
            assert!(body.contains("fictional:12b"));
        }
    }
}
