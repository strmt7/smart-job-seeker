//! Bounded fictional HTTP fixtures. Idle speculative connections must not block
//! navigation, and dropping a fixture joins every thread, including on panic.
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

type RequestObserver = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

pub struct HttpFixture {
    pub origin: String,
    posts: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl HttpFixture {
    pub fn start(body: impl Into<String>) -> Self {
        Self::start_inner(body.into(), None)
    }

    pub fn start_observing(
        observer: impl Fn(&str) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self::start_inner(String::new(), Some(Arc::new(observer)))
    }

    fn start_inner(body: String, observer: Option<RequestObserver>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let body = Arc::new(body);
        assert!(body.len() <= 64 * 1024, "fixture response too large");
        let stop = Arc::new(AtomicBool::new(false));
        let posts = Arc::new(AtomicUsize::new(0));
        let (worker_stop, worker_posts) = (stop.clone(), posts.clone());
        let worker = thread::spawn(move || {
            let mut clients: Vec<JoinHandle<io::Result<()>>> = vec![];
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut failure = None;
            while !worker_stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                let mut index = 0;
                while index < clients.len() {
                    if clients[index].is_finished() {
                        if let Err(error) = join(clients.swap_remove(index)) {
                            failure.get_or_insert(error);
                        }
                    } else {
                        index += 1;
                    }
                }
                if failure.is_some() {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        if clients.len() >= 16 {
                            failure = Some(io::Error::other("fixture connection budget exhausted"));
                            break;
                        }
                        let (stop, posts, body) =
                            (worker_stop.clone(), worker_posts.clone(), body.clone());
                        let observer = observer.clone();
                        clients.push(thread::spawn(move || {
                            serve(stream, &stop, &posts, &body, observer.as_ref())
                        }));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            if Instant::now() >= deadline {
                failure.get_or_insert(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture lifetime budget exhausted",
                ));
            }
            worker_stop.store(true, Ordering::Relaxed);
            for client in clients {
                if let Err(error) = join(client) {
                    failure.get_or_insert(error);
                }
            }
            failure.map_or(Ok(()), Err)
        });
        Self {
            origin,
            stop,
            posts,
            worker: Some(worker),
        }
    }

    pub fn posts(&self) -> usize {
        self.posts.load(Ordering::Relaxed)
    }

    pub fn finish(mut self) {
        self.shutdown().expect("fictional HTTP fixture failed");
    }

    fn shutdown(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().map_or(Ok(()), join)
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("fictional HTTP fixture cleanup failed: {error}");
        }
    }
}

fn join(worker: JoinHandle<io::Result<()>>) -> io::Result<()> {
    worker
        .join()
        .map_err(|_| io::Error::other("fixture worker panicked"))?
}

fn serve(
    mut stream: TcpStream,
    stop: &AtomicBool,
    posts: &AtomicUsize,
    body: &str,
    observer: Option<&RequestObserver>,
) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = vec![];
    let mut buffer = [0u8; 2048];
    let mut expected = None;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return if bytes.is_empty() {
                Ok(()) // An idle preconnection is not an HTTP request.
            } else {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "incomplete fixture request",
                ))
            };
        }
        match stream.read(&mut buffer) {
            Ok(0) if bytes.is_empty() => return Ok(()),
            Ok(0) => return Err(io::Error::other("truncated fixture request")),
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
        if bytes.len() > 16 * 1024 {
            return Err(io::Error::other("fixture request too large"));
        }
        if expected.is_none() {
            if let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let header = std::str::from_utf8(&bytes[..end])
                    .map_err(|_| io::Error::other("invalid fixture headers"))?;
                let mut length = None;
                for line in header.lines().skip(1) {
                    let (name, value) = line
                        .split_once(':')
                        .ok_or_else(|| io::Error::other("invalid fixture header"))?;
                    if name.eq_ignore_ascii_case("transfer-encoding") {
                        return Err(io::Error::other("unsupported fixture transfer encoding"));
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        if length.is_some() {
                            return Err(io::Error::other("duplicate fixture content length"));
                        }
                        length = Some(
                            value
                                .trim()
                                .parse::<usize>()
                                .map_err(|_| io::Error::other("invalid fixture content length"))?,
                        );
                    }
                }
                let length = length.unwrap_or(0);
                if length > 8192 {
                    return Err(io::Error::other("fixture body too large"));
                }
                expected = Some(end + 4 + length);
            }
        }
        if expected.is_some_and(|length| bytes.len() >= length) {
            break;
        }
    }
    let is_post = bytes.starts_with(b"POST ");
    if let Some(observer) = observer {
        let line_end = bytes
            .windows(2)
            .position(|v| v == b"\r\n")
            .ok_or_else(|| io::Error::other("missing fixture request line"))?;
        let line = std::str::from_utf8(&bytes[..line_end])
            .map_err(|_| io::Error::other("invalid fixture request line"))?;
        let path = line
            .split_whitespace()
            .nth(1)
            .filter(|path| path.len() <= 256)
            .ok_or_else(|| io::Error::other("missing or oversized fixture request path"))?;
        observer(path)?;
    }
    if is_post {
        posts.fetch_add(1, Ordering::Relaxed);
    }
    if observer.is_some() {
        // Observed channels measure arrival; 204 terminates SSE without retries.
        stream.write_all(b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Private-Network: true\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    } else {
        let response = if is_post { "" } else { body };
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response)
    }
}
