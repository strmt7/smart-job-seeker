//! Bounded nonblocking transport. Workspace mutations stay on their owner thread.
use mio::{Events, Interest, Poll, Token};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener};
use std::time::{Duration, Instant};

const CONNECTIONS: usize = 8;
const BUFFERS: usize = 96 * 1024 * 1024;
const RESPONSE: usize = 32 * 1024 * 1024;
const IO_QUOTA: usize = 256 * 1024;
const ACCEPT_QUOTA: usize = 16;
const IO_CALL_QUOTA: usize = 128;
const HEADER_TIME: Duration = Duration::from_secs(5);
const REQUEST_TIME: Duration = Duration::from_secs(30);
const WRITE_TIME: Duration = Duration::from_secs(15);

pub struct Access {
    authority: String,
    token: String,
    public_get: fn(&str) -> bool,
}

impl Access {
    pub fn new(port: u16, token: String, public_get: fn(&str) -> bool) -> Self {
        Self {
            authority: format!("127.0.0.1:{port}"),
            token,
            public_get,
        }
    }
}

pub struct Request {
    pub method: String,
    pub path: String,
    pub filename: String,
    pub body: Vec<u8>,
}

struct Frame {
    header: usize,
    length: usize,
    method: String,
    path: String,
    filename: String,
}

fn parse(bytes: &[u8], access: &Access) -> Result<Option<Frame>, String> {
    // Do not parse an unfinished header repeatedly as each body chunk arrives.
    let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return if bytes.len() >= crate::MAX_REQUEST {
            Err("request too large".into())
        } else {
            Ok(None)
        };
    };
    let end = end + 4;
    if end > crate::MAX_REQUEST {
        return Err("request too large".into());
    }
    let mut headers = [httparse::EMPTY_HEADER; 128];
    let mut parsed = httparse::Request::new(&mut headers);
    if !parsed
        .parse(&bytes[..end])
        .map_err(|_| "invalid headers")?
        .is_complete()
    {
        return Err("incomplete request".into());
    }
    let method = parsed.method.ok_or("missing method")?;
    let path = parsed.path.ok_or("missing path")?;
    if !path.starts_with('/') || path.starts_with("//") {
        return Err("invalid path".into());
    }
    let mut host = None;
    let mut token = None;
    let mut origin = None;
    let mut filename = None;
    let mut length = None;
    for header in parsed.headers.iter() {
        let value = std::str::from_utf8(header.value)
            .map_err(|_| "invalid header value")?
            .trim();
        let slot = match header.name.to_ascii_lowercase().as_str() {
            "host" => Some(&mut host),
            "x-workspace-token" => Some(&mut token),
            "origin" => Some(&mut origin),
            "x-document-name" => Some(&mut filename),
            "content-length" => Some(&mut length),
            "transfer-encoding" | "expect" => return Err("unsupported request framing".into()),
            _ => None,
        };
        if let Some(slot) = slot {
            if slot.replace(value).is_some() {
                return Err("duplicate request header".into());
            }
        }
    }
    if host != Some(access.authority.as_str()) {
        return Err("invalid host".into());
    }
    let public = method == "GET" && (access.public_get)(path);
    if !public
        && (token != Some(access.token.as_str())
            || origin.is_some_and(|value| {
                !value.is_empty() && value != format!("http://{}", access.authority)
            }))
    {
        return Err("unauthorized".into());
    }
    let length = match length {
        Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
            value.parse::<usize>().map_err(|_| "invalid length")?
        }
        None => 0,
        _ => return Err("invalid length".into()),
    };
    let limit = if method == "POST" && path == "/api/documents/upload" {
        crate::MAX_DOCUMENT
    } else if method == "POST" && path == "/api/form/fill" {
        crate::MAX_FORM_FILL_REQUEST
    } else {
        crate::MAX_REQUEST - end
    };
    if length > limit {
        return Err("request too large".into());
    }
    Ok(Some(Frame {
        header: end,
        length,
        method: method.into(),
        path: path.into(),
        filename: filename.unwrap_or_default().into(),
    }))
}

#[derive(Default)]
pub struct Response {
    bytes: Vec<u8>,
    failed: bool,
}

impl Write for Response {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed || bytes.len() > RESPONSE.saturating_sub(self.bytes.len()) {
            self.failed = true;
            return Err(io::Error::other("Response exceeds the transport budget"));
        }
        if self.bytes.try_reserve_exact(bytes.len()).is_err() {
            self.failed = true;
            return Err(io::Error::other("Response buffer unavailable"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Response {
    fn error(status: &str, message: &str) -> Self {
        let mut response = Self::default();
        crate::json_reply(&mut response, status, serde_json::json!({"error":message}));
        response
    }
    fn finish(self) -> Vec<u8> {
        if self.failed || self.bytes.is_empty() {
            Self::error(
                "500 Internal Server Error",
                "The response could not be prepared within its transport budget",
            )
            .bytes
        } else {
            self.bytes
        }
    }
}

enum State {
    Reading {
        bytes: Vec<u8>,
        frame: Option<Frame>,
    },
    Ready(Request),
    Handling,
    Writing {
        bytes: Vec<u8>,
        offset: usize,
        since: Instant,
    },
}

struct Connection {
    socket: mio::net::TcpStream,
    accepted: Instant,
    state: State,
    readable: bool,
    writable: bool,
}

impl Connection {
    fn memory(&self) -> usize {
        match &self.state {
            State::Reading { bytes, .. } | State::Writing { bytes, .. } => bytes.capacity(),
            State::Ready(request) => request.body.capacity(),
            State::Handling => 0,
        }
    }
}

pub struct Server {
    listener: mio::net::TcpListener,
    poll: Poll,
    events: Events,
    connections: BTreeMap<usize, Connection>,
    access: Access,
    next_id: usize,
    runnable: bool,
    accept_pending: bool,
}

impl Server {
    pub fn new(listener: TcpListener, access: Access) -> io::Result<Self> {
        listener.set_nonblocking(true)?;
        let mut listener = mio::net::TcpListener::from_std(listener);
        let poll = Poll::new()?;
        poll.registry()
            .register(&mut listener, Token(0), Interest::READABLE)?;
        Ok(Self {
            listener,
            poll,
            events: Events::with_capacity(32),
            connections: BTreeMap::new(),
            access,
            next_id: 1,
            runnable: false,
            accept_pending: false,
        })
    }

    fn memory(&self) -> usize {
        self.connections.values().map(Connection::memory).sum()
    }

    fn close(&mut self, id: usize) {
        if let Some(mut connection) = self.connections.remove(&id) {
            if matches!(
                connection.state,
                State::Reading { .. } | State::Writing { .. }
            ) {
                let _ = self.poll.registry().deregister(&mut connection.socket);
            }
            let _ = connection.socket.shutdown(Shutdown::Both);
        }
    }

    fn accept(&mut self) -> io::Result<()> {
        for _ in 0..ACCEPT_QUOTA {
            let (mut socket, _) = match self.listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    self.accept_pending = false;
                    return Ok(());
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            };
            if self.connections.len() >= CONNECTIONS {
                // Best-effort refusal is nonblocking and never consumes another
                // connection slot. A disconnected/partial client is not retried.
                let _ = socket.write(
                    &Response::error(
                        "503 Service Unavailable",
                        "The local connection limit was reached; try again shortly",
                    )
                    .finish(),
                );
                let _ = socket.shutdown(Shutdown::Both);
                continue;
            }
            let id = self.next_id;
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Connection identity exhausted"))?;
            socket.set_nodelay(true)?;
            self.poll
                .registry()
                .register(&mut socket, Token(id), Interest::READABLE)?;
            self.connections.insert(
                id,
                Connection {
                    socket,
                    accepted: Instant::now(),
                    state: State::Reading {
                        bytes: Vec::new(),
                        frame: None,
                    },
                    readable: true,
                    writable: false,
                },
            );
        }
        // A readiness edge need not repeat after a quota; explicitly revisit it.
        self.runnable = true;
        self.accept_pending = true;
        Ok(())
    }

    pub fn poll(&mut self) -> io::Result<()> {
        let timeout = if self.runnable {
            Duration::ZERO
        } else {
            Duration::from_millis(100)
        };
        self.runnable = false;
        match self.poll.poll(&mut self.events, Some(timeout)) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(()),
            Err(error) => return Err(error),
        }
        let mut accept = false;
        for event in &self.events {
            if event.token() == Token(0) {
                accept = true;
                continue;
            }
            if let Some(connection) = self.connections.get_mut(&event.token().0) {
                connection.readable |=
                    event.is_readable() || event.is_read_closed() || event.is_error();
                connection.writable |=
                    event.is_writable() || event.is_write_closed() || event.is_error();
            }
        }
        // Always attempt accept after quota exhaustion as well as on readiness.
        if accept || self.accept_pending || self.connections.len() < CONNECTIONS {
            self.accept()?;
        }
        let ids: Vec<_> = self.connections.keys().copied().collect();
        for id in ids {
            let mut memory = self.memory();
            let connection = self.connections.get_mut(&id).expect("collected identity");
            let expired = match &connection.state {
                State::Reading { frame, .. } => {
                    connection.accepted.elapsed()
                        >= if frame.is_some() {
                            REQUEST_TIME
                        } else {
                            HEADER_TIME
                        }
                }
                State::Ready(_) => connection.accepted.elapsed() >= REQUEST_TIME,
                State::Writing { since, .. } => since.elapsed() >= WRITE_TIME,
                State::Handling => connection.accepted.elapsed() >= REQUEST_TIME,
            };
            if expired {
                self.close(id);
                continue;
            }
            let mut quota = IO_QUOTA;
            let mut calls = 0;
            let mut remove = false;
            while quota > 0 && calls < IO_CALL_QUOTA {
                calls += 1;
                match &mut connection.state {
                    State::Reading { bytes, frame } if connection.readable => {
                        let limit = frame
                            .as_ref()
                            .map_or(crate::MAX_REQUEST, |head| head.header + head.length);
                        let room = BUFFERS.saturating_sub(memory);
                        let count = 8192
                            .min(quota)
                            .min(limit.saturating_sub(bytes.len()))
                            .min(room.saturating_add(bytes.capacity() - bytes.len()));
                        if count == 0 {
                            break;
                        }
                        let mut chunk = [0; 8192];
                        match connection.socket.read(&mut chunk[..count]) {
                            Ok(0) => {
                                remove = true;
                                break;
                            }
                            Ok(n) => {
                                let previous = bytes.capacity();
                                let target = bytes
                                    .capacity()
                                    .saturating_mul(2)
                                    .max(bytes.len() + n)
                                    .min(limit)
                                    .min(bytes.capacity().saturating_add(room));
                                if bytes.try_reserve_exact(target - bytes.len()).is_err() {
                                    remove = true;
                                    break;
                                }
                                bytes.extend_from_slice(&chunk[..n]);
                                memory += bytes.capacity() - previous;
                                quota -= n;
                                if frame.is_none() {
                                    match parse(bytes, &self.access) {
                                        Ok(parsed) => *frame = parsed,
                                        Err(error) => {
                                            connection.state = State::Writing {
                                                bytes: Response::error("400 Bad Request", &error)
                                                    .finish(),
                                                offset: 0,
                                                since: Instant::now(),
                                            };
                                            connection.writable = true;
                                            self.poll.registry().reregister(
                                                &mut connection.socket,
                                                Token(id),
                                                Interest::WRITABLE,
                                            )?;
                                            break;
                                        }
                                    }
                                }
                                if let Some(head) = frame {
                                    if bytes.len() >= head.header + head.length {
                                        let head = frame.take().expect("complete frame");
                                        let mut body = std::mem::take(bytes);
                                        body.truncate(head.header + head.length);
                                        body.drain(..head.header);
                                        // One request per connection. Stop read
                                        // readiness while queued, including any
                                        // pipelined bytes a caller supplied.
                                        self.poll.registry().deregister(&mut connection.socket)?;
                                        connection.state = State::Ready(Request {
                                            method: head.method,
                                            path: head.path,
                                            filename: head.filename,
                                            body,
                                        });
                                        break;
                                    }
                                }
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                connection.readable = false;
                                break;
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                            Err(_) => {
                                remove = true;
                                break;
                            }
                        }
                    }
                    State::Writing { bytes, offset, .. } if connection.writable => {
                        let count = quota.min(bytes.len() - *offset);
                        match connection.socket.write(&bytes[*offset..*offset + count]) {
                            Ok(0) => {
                                remove = true;
                                break;
                            }
                            Ok(n) => {
                                *offset += n;
                                quota -= n;
                                if *offset == bytes.len() {
                                    remove = true;
                                    break;
                                }
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                connection.writable = false;
                                break;
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                            Err(_) => {
                                remove = true;
                                break;
                            }
                        }
                    }
                    _ => break,
                }
            }
            if remove {
                self.close(id);
            } else if quota == 0 || calls == IO_CALL_QUOTA {
                self.runnable = true;
            }
        }
        Ok(())
    }

    pub fn next_request(&mut self) -> Option<(usize, Request)> {
        // Reserve room for a maximum bounded response before mutating state.
        if self.memory() > BUFFERS - RESPONSE {
            return None;
        }
        for (id, connection) in &mut self.connections {
            if matches!(connection.state, State::Ready(_)) {
                let State::Ready(request) =
                    std::mem::replace(&mut connection.state, State::Handling)
                else {
                    unreachable!()
                };
                self.runnable = true;
                return Some((*id, request));
            }
        }
        None
    }

    pub fn is_handling(&self, id: usize) -> bool {
        self.connections
            .get(&id)
            .is_some_and(|connection| matches!(connection.state, State::Handling))
    }

    pub fn respond(&mut self, id: usize, response: Response) -> io::Result<()> {
        let connection = self
            .connections
            .get_mut(&id)
            .ok_or_else(|| io::Error::other("Response connection is no longer active"))?;
        if !matches!(connection.state, State::Handling) {
            return Err(io::Error::other(
                "Response connection is not awaiting a handler",
            ));
        }
        connection.state = State::Writing {
            bytes: response.finish(),
            offset: 0,
            since: Instant::now(),
        };
        connection.writable = true;
        if let Err(error) =
            self.poll
                .registry()
                .register(&mut connection.socket, Token(id), Interest::WRITABLE)
        {
            self.close(id);
            return Err(error);
        }
        self.runnable = true;
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        while let Some(id) = self.connections.keys().next().copied() {
            self.close(id);
        }
        let _ = self.poll.registry().deregister(&mut self.listener);
    }
}

/// Route regressions use the production framer with bounded blocking test I/O.
#[cfg(test)]
pub fn read_test_request(
    socket: &mut std::net::TcpStream,
    access: &Access,
) -> Result<Request, String> {
    socket.set_nonblocking(false).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + HEADER_TIME;
    let mut bytes = Vec::new();
    let mut frame = None;
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or("request deadline exceeded")?;
        socket
            .set_read_timeout(Some(left))
            .map_err(|e| e.to_string())?;
        let mut chunk = [0; 4096];
        let n = socket.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("incomplete request".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if frame.is_none() {
            frame = parse(&bytes, access)?;
        }
        if let Some(head) = frame.as_ref() {
            if bytes.len() >= head.header + head.length {
                let head = frame.take().expect("complete frame");
                bytes.truncate(head.header + head.length);
                bytes.drain(..head.header);
                return Ok(Request {
                    method: head.method,
                    path: head.path,
                    filename: head.filename,
                    body: bytes,
                });
            }
        }
    }
}

#[cfg(test)]
pub fn write_test_response(
    socket: &mut std::net::TcpStream,
    response: Response,
) -> Result<(), String> {
    let bytes = response.finish();
    let deadline = Instant::now() + WRITE_TIME;
    let mut offset = 0;
    while offset < bytes.len() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or("response deadline exceeded")?;
        socket
            .set_write_timeout(Some(left))
            .map_err(|e| e.to_string())?;
        let n = socket.write(&bytes[offset..]).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("incomplete response".into());
        }
        offset += n;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    fn access(port: u16) -> Access {
        Access::new(port, "fictional-test-token".into(), |path| path == "/")
    }
    fn head(extra: &str) -> Vec<u8> {
        format!("POST /api/example HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nX-Workspace-Token: fictional-test-token\r\n{extra}\r\n").into_bytes()
    }
    fn request(port: u16) -> Vec<u8> {
        format!("GET /api/example HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fictional-test-token\r\n\r\n").into_bytes()
    }
    fn server() -> (Server, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        (Server::new(listener, access(port)).unwrap(), port)
    }
    fn exchange(server: &mut Server, client: &mut TcpStream, text: &str) -> Vec<u8> {
        client.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut result = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "Exchange deadline exceeded");
            server.poll().unwrap();
            while let Some((id, _)) = server.next_request() {
                let mut response = Response::default();
                crate::reply(&mut response, "200 OK", "text/plain", text);
                server.respond(id, response).unwrap();
            }
            let mut bytes = [0; 8192];
            loop {
                match client.read(&mut bytes) {
                    Ok(0) => return result,
                    Ok(n) => result.extend_from_slice(&bytes[..n]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("Client read failed: {error}"),
                }
            }
        }
    }

    #[test]
    fn framing_rejects_conflicting_headers_and_unsupported_body_transports() {
        for extra in [
            "Content-Length: 0\r\ncontent-length: 0\r\n",
            "Host: 127.0.0.1:8080\r\n",
            "X-Workspace-Token: fictional-test-token\r\n",
            "Origin: a\r\nOrigin: b\r\n",
            "X-Document-Name: a\r\nx-document-name: b\r\n",
            "Transfer-Encoding: chunked\r\n",
            "Transfer-Encoding: identity\r\n",
            "Expect: 100-continue\r\n",
            "Content-Length: +1\r\n",
            "Content-Length: -1\r\n",
            "Content-Length: 1, 1\r\n",
            "Content-Length: 184467440737095516160\r\n",
        ] {
            assert!(parse(&head(extra), &access(8080)).is_err(), "{extra:?}");
        }
        assert!(parse(&head("Content-Length: 0\r\n"), &access(8080))
            .unwrap()
            .is_some());
        assert!(parse(b"POST / HTTP/1.1\r\n", &access(8080))
            .unwrap()
            .is_none());
        assert!(parse(&vec![b'x'; crate::MAX_REQUEST], &access(8080)).is_err());
    }

    #[test]
    fn framing_keeps_host_auth_origin_and_endpoint_specific_limits() {
        let valid = String::from_utf8(head("Content-Length: 0\r\n")).unwrap();
        for invalid in [
            valid.replace("127.0.0.1:8080", "localhost:8080"),
            valid.replace("fictional-test-token", "different-token"),
            valid.replace("/api/example", "http://example.org/"),
            valid.replace("/api/example", "//example.org/"),
            valid.replace("Content-Length: 0", "Origin: https://example.org"),
        ] {
            assert!(parse(invalid.as_bytes(), &access(8080)).is_err());
        }
        let ordinary = head(&format!("Content-Length: {}\r\n", crate::MAX_REQUEST));
        assert!(parse(&ordinary, &access(8080)).is_err());
        let upload = String::from_utf8(head(&format!(
            "Content-Length: {}\r\n",
            crate::MAX_DOCUMENT
        )))
        .unwrap()
        .replace("/api/example", "/api/documents/upload");
        assert!(parse(upload.as_bytes(), &access(8080)).unwrap().is_some());
        let over = upload.replace(
            &format!("Content-Length: {}", crate::MAX_DOCUMENT),
            &format!("Content-Length: {}", crate::MAX_DOCUMENT + 1),
        );
        assert!(parse(over.as_bytes(), &access(8080)).is_err());
        let public = b"GET / HTTP/1.1\r\nHost: 127.0.0.1:8080\r\n\r\n";
        assert!(parse(public, &access(8080)).unwrap().is_some());
    }

    #[test]
    fn incomplete_header_does_not_block_another_request() {
        let (mut server, port) = server();
        let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
        slow.write_all(b"GET / HTTP/1.1\r\nHost:").unwrap();
        server.poll().unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(&request(port)).unwrap();
        let result = exchange(&mut server, &mut client, "available");
        assert!(result.ends_with(b"available"));
        assert_eq!(
            server.connections.len(),
            1,
            "The incomplete client remains isolated"
        );
    }

    #[test]
    fn abandoned_connections_do_not_stop_accepting_valid_requests() {
        let (mut server, port) = server();
        for _ in 0..4 {
            drop(TcpStream::connect(("127.0.0.1", port)).unwrap());
        }
        server.poll().unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(&request(port)).unwrap();
        assert!(exchange(&mut server, &mut client, "available").ends_with(b"available"));
    }

    #[test]
    fn incomplete_body_does_not_block_another_request_or_reset_its_deadline() {
        let (mut server, port) = server();
        let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let bytes = String::from_utf8(request(port))
            .unwrap()
            .replace("GET", "POST")
            .replace("\r\n\r\n", "\r\nContent-Length: 10\r\n\r\nx");
        slow.write_all(bytes.as_bytes()).unwrap();
        server.poll().unwrap();
        assert!(matches!(
            server.connections.values().next().unwrap().state,
            State::Reading { frame: Some(_), .. }
        ));
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(&request(port)).unwrap();
        assert!(exchange(&mut server, &mut client, "available").ends_with(b"available"));
        let original = Instant::now() - REQUEST_TIME - Duration::from_secs(1);
        server.connections.values_mut().next().unwrap().accepted = original;
        slow.write_all(b"y").unwrap();
        server.poll().unwrap();
        assert!(
            server.connections.is_empty(),
            "Trickled body bytes cannot extend the absolute deadline"
        );
    }

    #[test]
    fn saturation_refuses_without_dispatch_and_drop_closes_owned_sockets() {
        let (mut server, port) = server();
        let mut clients = Vec::new();
        for _ in 0..CONNECTIONS {
            let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            clients.push(client);
            server.poll().unwrap();
        }
        let mut refused = TcpStream::connect(("127.0.0.1", port)).unwrap();
        server.poll().unwrap();
        refused
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut bytes = Vec::new();
        refused.read_to_end(&mut bytes).unwrap();
        assert!(bytes.starts_with(b"HTTP/1.1 503"));
        assert!(server.next_request().is_none());
        assert_eq!(server.connections.len(), CONNECTIONS);
        drop(server);
        for mut client in clients {
            client
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
        }
    }

    #[test]
    fn byte_quota_preserves_readiness_for_large_responses() {
        let (mut server, port) = server();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(&request(port)).unwrap();
        let body = "x".repeat(IO_QUOTA * 3 + 29);
        let response = exchange(&mut server, &mut client, &body);
        let end = response.windows(4).position(|s| s == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(&response[end..], body.as_bytes());
        assert!(server.connections.is_empty());
    }

    #[test]
    fn slow_reader_does_not_block_another_request_and_write_deadline_closes_it() {
        let (mut server, port) = server();
        let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
        slow.write_all(&request(port)).unwrap();
        server.poll().unwrap();
        let (id, _) = server.next_request().unwrap();
        let mut response = Response::default();
        crate::reply(
            &mut response,
            "200 OK",
            "text/plain",
            &"x".repeat(RESPONSE / 2),
        );
        server.respond(id, response).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(&request(port)).unwrap();
        assert!(exchange(&mut server, &mut client, "responsive").ends_with(b"responsive"));
        assert!(server.memory() <= BUFFERS);
        let connection = server
            .connections
            .get_mut(&id)
            .expect("Unread large response remains pending");
        let State::Writing { since, .. } = &mut connection.state else {
            panic!("Expected a pending writer")
        };
        *since = Instant::now() - WRITE_TIME - Duration::from_secs(1);
        server.poll().unwrap();
        assert!(!server.connections.contains_key(&id));
    }

    #[test]
    fn upload_larger_than_io_quota_is_received_exactly_without_losing_readiness() {
        let (mut server, port) = server();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.set_nonblocking(true).unwrap();
        let body = vec![b'q'; IO_QUOTA * 3 + 71];
        let head = format!("POST /api/documents/upload HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fictional-test-token\r\nX-Document-Name: fictional.pdf\r\nContent-Length: {}\r\n\r\n", body.len());
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(&body);
        let mut offset = 0;
        let deadline = Instant::now() + Duration::from_secs(3);
        let received = loop {
            assert!(Instant::now() < deadline);
            if offset < bytes.len() {
                match client.write(&bytes[offset..]) {
                    Ok(n) => offset += n,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                    Err(error) => panic!("Upload failed: {error}"),
                }
            }
            server.poll().unwrap();
            assert!(server.memory() <= BUFFERS);
            if let Some((_, request)) = server.next_request() {
                break request;
            }
        };
        assert_eq!(received.body, body);
        assert_eq!(received.filename, "fictional.pdf");
        assert_eq!(received.path, "/api/documents/upload");
    }

    #[test]
    fn response_overflow_never_publishes_a_truncated_success() {
        let mut response = Response::default();
        crate::reply(&mut response, "200 OK", "text/plain", "prefix");
        response.write_all(&vec![b'x'; RESPONSE]).unwrap_err();
        let bytes = response.finish();
        assert!(bytes.starts_with(b"HTTP/1.1 500"));
        assert!(!bytes.windows(6).any(|s| s == b"prefix"));
    }

    #[test]
    fn deferred_handling_expires_and_late_completion_cannot_send() {
        let (mut server, port) = server();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(&request(port)).unwrap();
        server.poll().unwrap();
        let (id, _) = server.next_request().unwrap();
        assert!(server.is_handling(id));
        server.connections.get_mut(&id).unwrap().accepted =
            Instant::now() - REQUEST_TIME - Duration::from_secs(1);
        server.poll().unwrap();
        assert!(!server.is_handling(id));
        assert!(server.connections.is_empty());
        assert!(server
            .respond(id, Response::error("200 OK", "late"))
            .is_err());
    }

    #[test]
    fn stale_response_identity_cannot_replace_a_new_connection() {
        let (mut server, port) = server();
        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        server.poll().unwrap();
        let old = *server.connections.keys().next().unwrap();
        server.close(old);
        drop(client);
        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        server.poll().unwrap();
        let current = *server.connections.keys().next().unwrap();
        assert_ne!(old, current);
        assert!(server
            .respond(old, Response::error("200 OK", "stale"))
            .is_err());
        assert!(server
            .respond(current, Response::error("200 OK", "unrequested"))
            .is_err());
        assert!(matches!(
            server.connections[&current].state,
            State::Reading { .. }
        ));
        drop(client);
    }
}
