//! Real network layer for Waypoint (T014/T015 continuation, gate G2).
//!
//! Why this crate exists: connectors previously relied on the `curl` binary in
//! tests, so the product had no HTTPS client. This is the single HTTP client
//! for the whole platform.
//!
//! Security properties (structural, tested):
//! - HTTPS only for non-loopback hosts. Loopback plain HTTP is permitted solely
//!   for the local inference runtime.
//! - Every host is resolved first and **every resolved address** must be public.
//!   This defeats DNS-rebinding: a name that first resolves to a public IP and
//!   later to 127.0.0.1 / 10.x / link-local / IPv6 unique-local is refused.
//! - Responses are wire-size capped. Public socket/TLS reads and writes check
//!   a request deadline; the OS DNS resolver remains synchronous. Asset reads
//!   request identity encoding and refuse encoded bodies; no decompression runs.
//! - We identify ourselves in `User-Agent` — a politeness/terms requirement for
//!   public APIs, not an optional nicety.
//! - TLS uses the OS trust store (schannel on Windows): no vendored crypto
//!   stack, and certificate validation is the platform's job.

mod http;
pub use http::{BodyFraming, CapturedResponse, Header};

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const USER_AGENT: &str = concat!(
    "waypoint/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/strmt7/smart-job-seeker; local-first job search client)"
);

/// Hard ceiling for any single response body (defensive: public job feeds are
/// JSON lists; 32 MiB is far above any real board and far below memory risk).
pub const DEFAULT_MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    BadUrl(String),
    /// Host resolves to a non-public address, or the host literal is private.
    BlockedHost(String),
    /// Scheme/host combination not permitted (e.g. plain http to the internet).
    SchemeNotAllowed(String),
    Tls(String),
    Io(String),
    Timeout,
    /// Response exceeded the configured byte cap.
    BodyTooLarge,
    HttpStatus(u16),
    IncompleteResponse,
    InvalidResponse(&'static str),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::BadUrl(u) => write!(f, "malformed url: {u}"),
            NetError::BlockedHost(h) => write!(f, "host is not a public address: {h}"),
            NetError::SchemeNotAllowed(s) => write!(f, "scheme not allowed: {s}"),
            NetError::Tls(m) => write!(f, "tls: {m}"),
            NetError::Io(m) => write!(f, "io: {m}"),
            NetError::Timeout => write!(f, "request timed out"),
            NetError::BodyTooLarge => write!(f, "response body exceeded the size cap"),
            NetError::HttpStatus(s) => write!(f, "unexpected http status {s}"),
            NetError::IncompleteResponse => {
                write!(f, "HTTP response ended before its declared body completed")
            }
            NetError::InvalidResponse(reason) => write!(f, "invalid HTTP response: {reason}"),
        }
    }
}

impl std::error::Error for NetError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub https: bool,
    pub host: String,
    pub port: u16,
    /// Path + query, always starting with '/'.
    pub request_target: String,
}

pub fn parse_url(url: &str) -> Result<ParsedUrl, NetError> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| NetError::BadUrl(url.to_string()))?;
    let https = match scheme {
        "https" => true,
        "http" => false,
        other => return Err(NetError::SchemeNotAllowed(other.to_string())),
    };
    if url.len() > 8192 || !url.is_ascii() || url.bytes().any(|c| c <= 32 || c == 127) {
        return Err(NetError::BadUrl("invalid URL bytes".into()));
    }
    let (authority, suffix) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let suffix = suffix.split('#').next().unwrap_or("");
    let target = if suffix.starts_with('/') {
        suffix.to_owned()
    } else {
        format!("/{suffix}")
    };
    if authority.is_empty() {
        return Err(NetError::BadUrl(url.to_string()));
    }
    // Reject userinfo outright: credentials must never travel in URLs.
    if authority.contains('@') {
        return Err(NetError::BadUrl(url.to_string()));
    }
    let (host, port, bracketed) = if let Some(rest) = authority.strip_prefix('[') {
        // Bracketed IPv6 literal: [::1]:11434 or [2606:4700::1111]
        let (h, tail) = rest
            .split_once(']')
            .ok_or_else(|| NetError::BadUrl(url.to_string()))?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| NetError::BadUrl(url.to_string()))?,
            None if tail.is_empty() => {
                if https {
                    443
                } else {
                    80
                }
            }
            None => return Err(NetError::BadUrl(url.to_string())),
        };
        (h.to_string(), port, true)
    } else {
        let (h, port) = match authority.rsplit_once(':') {
            Some((h, p)) => {
                let port: u16 = p.parse().map_err(|_| NetError::BadUrl(url.to_string()))?;
                (h.to_string(), port)
            }
            None => (authority.to_string(), if https { 443 } else { 80 }),
        };
        (h, port, false)
    };
    if host.is_empty()
        || host.contains(|c: char| c.is_whitespace())
        // A colon left in an *unbracketed* host means a malformed IPv6 literal.
        || (!bracketed && host.contains(':'))
    {
        return Err(NetError::BadUrl(url.to_string()));
    }
    Ok(ParsedUrl {
        https,
        host,
        port,
        request_target: target,
    })
}

/// Addresses that must never be reachable through a discovery fetch.
pub fn is_disallowed_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.octets()[0] == 0
                // 100.64.0.0/10 carrier-grade NAT
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
                // 192.0.0.0/24 IETF protocol assignments
                || (v4.octets()[0] == 192 && v4.octets()[1] == 0 && v4.octets()[2] == 0)
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 unique local
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10 link local
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped: re-check the embedded v4 address
                || v6.to_ipv4_mapped().map(|v4| is_disallowed_ip(&std::net::IpAddr::V4(v4))).unwrap_or(false)
        }
    }
}

/// Resolves and validates every address for a URL before any connection.
pub fn resolve_public(url: &ParsedUrl) -> Result<Vec<SocketAddr>, NetError> {
    let addrs: Vec<SocketAddr> = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(|e| NetError::Io(format!("dns {}: {e}", url.host)))?
        .collect();
    if addrs.is_empty() {
        return Err(NetError::Io(format!("dns {}: no addresses", url.host)));
    }
    for a in &addrs {
        if is_disallowed_ip(&a.ip()) {
            return Err(NetError::BlockedHost(format!(
                "{} resolved to non-public address {}",
                url.host,
                a.ip()
            )));
        }
    }
    Ok(addrs)
}

/// The single entry point used by connectors and research.
pub fn get(url: &str, timeout_ms: u64) -> Result<HttpResponse, NetError> {
    get_capped(url, timeout_ms, DEFAULT_MAX_BYTES)
}

pub fn get_capped(url: &str, timeout_ms: u64, max_bytes: usize) -> Result<HttpResponse, NetError> {
    let response = get_response(url, timeout_ms, max_bytes, false)?;
    Ok(HttpResponse {
        status: response.status,
        body: response.body,
    })
}

/// Fetch one public resource with original headers and explicit body framing.
/// No redirects, cookies, authorization, browser headers or compression negotiation.
/// Callers must check source permissions/robots and candidate-data exclusion first.
pub fn get_asset_capped(
    url: &str,
    timeout_ms: u64,
    max_bytes: usize,
) -> Result<CapturedResponse, NetError> {
    if max_bytes == 0 || max_bytes > DEFAULT_MAX_BYTES || timeout_ms == 0 || timeout_ms > 60_000 {
        return Err(NetError::InvalidResponse("invalid asset capture budget"));
    }
    validate_asset_response(get_response(url, timeout_ms, max_bytes, true)?)
}

/// Parse an already obtained response stream without opening any connection.
/// Input is bounded on the handle; this does not authorize its source or fetch it.
pub fn read_asset_response<S: Read>(
    stream: &mut S,
    method: &str,
    max_bytes: usize,
) -> Result<CapturedResponse, NetError> {
    if !matches!(method, "GET" | "HEAD") || max_bytes == 0 || max_bytes > DEFAULT_MAX_BYTES {
        return Err(NetError::InvalidResponse(
            "invalid asset response budget or method",
        ));
    }
    validate_asset_response(http::receive(stream, method, max_bytes)?)
}
fn validate_asset_response(response: CapturedResponse) -> Result<CapturedResponse, NetError> {
    if response.headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case("content-encoding")
            && !h.value.eq_ignore_ascii_case(b"identity")
    }) {
        return Err(NetError::InvalidResponse(
            "encoded asset body is unsupported",
        ));
    }
    Ok(response)
}

#[derive(Debug)]
struct DeadlineStream<S> {
    stream: S,
    timer: TcpStream,
    deadline: std::time::Instant,
}
impl<S> DeadlineStream<S> {
    fn remaining(&self) -> std::io::Result<Duration> {
        self.deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "request deadline exceeded")
            })
    }
}
impl<S: Read> Read for DeadlineStream<S> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.timer.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(buffer)
    }
}
impl<S: Write> Write for DeadlineStream<S> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.timer.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}
fn get_response(
    url: &str,
    timeout_ms: u64,
    max_bytes: usize,
    asset: bool,
) -> Result<CapturedResponse, NetError> {
    let parsed = parse_url(url)?;
    if !parsed.https {
        return Err(NetError::SchemeNotAllowed(
            "plain http is only allowed for the local inference runtime".into(),
        ));
    }
    if !waypoint_connectors::is_public_host_url(url) {
        return Err(NetError::BlockedHost(parsed.host.clone()));
    }
    if asset && parsed.port != 443 {
        return Err(NetError::SchemeNotAllowed(
            "asset capture requires default HTTPS port".into(),
        ));
    }
    let timeout = Duration::from_millis(timeout_ms);
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .ok_or(NetError::Timeout)?;
    // OS DNS lookup is synchronous. The deadline is checked immediately after it;
    // this does not claim cancellation of a stalled OS resolver.
    let addrs = resolve_public(&parsed)?;
    let mut last_err = None;
    for addr in addrs {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|v| !v.is_zero())
            .ok_or(NetError::Timeout)?;
        match TcpStream::connect_timeout(&addr, remaining.min(Duration::from_secs(15))) {
            Ok(tcp) => {
                tcp.set_read_timeout(Some(remaining))
                    .map_err(|e| NetError::Io(e.to_string()))?;
                tcp.set_write_timeout(Some(remaining))
                    .map_err(|e| NetError::Io(e.to_string()))?;
                let timer = tcp.try_clone().map_err(|e| NetError::Io(e.to_string()))?;
                let connector =
                    native_tls::TlsConnector::new().map_err(|e| NetError::Tls(e.to_string()))?;
                let timed = DeadlineStream {
                    stream: tcp,
                    timer,
                    deadline,
                };
                let mut stream = connector
                    .connect(&parsed.host, timed)
                    .map_err(|e| NetError::Tls(e.to_string()))?;
                let host = if parsed.host.contains(':') {
                    format!("[{}]", parsed.host)
                } else {
                    parsed.host.clone()
                };
                let host = if parsed.port == 443 {
                    host
                } else {
                    format!("{host}:{}", parsed.port)
                };
                let request_header_names = write_request(
                    &mut stream,
                    &host,
                    &parsed.request_target,
                    "GET",
                    None,
                    asset,
                )?;
                let mut response = http::receive(&mut stream, "GET", max_bytes)?;
                response.request_header_names = Some(request_header_names);
                return Ok(response);
            }
            Err(e) => last_err = Some(NetError::Io(format!("connect {addr}: {e}"))),
        }
    }
    Err(last_err.unwrap_or(NetError::Timeout))
}

/// Plain-HTTP request to an already-validated loopback address (local runtime).
pub fn request_loopback(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    timeout_ms: u64,
    max_bytes: usize,
) -> Result<HttpResponse, NetError> {
    if !addr.ip().is_loopback() {
        return Err(NetError::BlockedHost(addr.ip().to_string()));
    }
    let timeout = Duration::from_millis(timeout_ms);
    let mut stream = TcpStream::connect_timeout(&addr, timeout.min(Duration::from_secs(10)))
        .map_err(|e| NetError::Io(format!("connect {addr}: {e}")))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    request(
        &mut stream,
        &addr.to_string(),
        path,
        method,
        body,
        max_bytes,
    )
}

/// Frame + parse one HTTP/1.1 exchange over any stream (plain or TLS).
pub fn request<S: Read + Write>(
    stream: &mut S,
    host: &str,
    path: &str,
    method: &str,
    body: Option<&[u8]>,
    max_bytes: usize,
) -> Result<HttpResponse, NetError> {
    write_request(stream, host, path, method, body, false)?;
    let response = http::receive(stream, method, max_bytes)?;
    Ok(HttpResponse {
        status: response.status,
        body: response.body,
    })
}

fn write_request<S: Write>(
    stream: &mut S,
    host: &str,
    path: &str,
    method: &str,
    body: Option<&[u8]>,
    asset: bool,
) -> Result<Vec<String>, NetError> {
    if host.is_empty()
        || !host.is_ascii()
        || host.bytes().any(|c| c <= 32 || c == 127)
        || !path.starts_with('/')
        || !path.is_ascii()
        || path.bytes().any(|c| c <= 32 || c == 127)
        || path.contains('#')
        || method.is_empty()
        || method.len() > 32
        || !method.bytes().all(|c| c.is_ascii_uppercase())
    {
        return Err(NetError::BadUrl("invalid request target or method".into()));
    }
    let bytes = body.unwrap_or(&[]);
    let head = if asset {
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n")
    } else {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len())
    };
    let mut fields = [httparse::EMPTY_HEADER; 16];
    let mut parsed = httparse::Request::new(&mut fields);
    if !matches!(
        parsed.parse(head.as_bytes()),
        Ok(httparse::Status::Complete(_))
    ) {
        return Err(NetError::InvalidResponse(
            "invalid outgoing request headers",
        ));
    }
    let names = parsed
        .headers
        .iter()
        .map(|h| h.name.to_ascii_lowercase())
        .collect();
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(bytes))
        .map_err(|e| {
            if matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) {
                NetError::Timeout
            } else {
                NetError::Io(format!("write: {e}"))
            }
        })?;
    Ok(names)
}

pub fn decode_chunked(body: &[u8]) -> Option<Vec<u8>> {
    http::decode_chunks(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_stream_requires_complete_delimiters_at_every_boundary() {
        let body = b"4\r\nWiki\r\n5;example=1\r\npedia\r\n0\r\n\r\n";
        for end in 0..body.len() {
            assert_eq!(decode_chunked(&body[..end]), None, "partial boundary {end}");
        }
        assert_eq!(decode_chunked(body).as_deref(), Some(&b"Wikipedia"[..]));
        assert!(decode_chunked(b"4\r\nWikiXX0\r\n\r\n").is_none());
        assert!(decode_chunked(b"ffffffffffffffff\r\n").is_none());
        assert_eq!(
            decode_chunked(b"1\r\na\r\n0\r\nX-Trace: yes\r\n\r\n").as_deref(),
            Some(&b"a"[..])
        );
    }

    #[test]
    fn url_parsing_defaults_and_rejections() {
        let u = parse_url("https://boards-api.greenhouse.io/v1/boards/vercel/jobs").unwrap();
        assert!(u.https);
        assert_eq!(u.host, "boards-api.greenhouse.io");
        assert_eq!(u.port, 443);
        assert_eq!(u.request_target, "/v1/boards/vercel/jobs");

        let u = parse_url("https://api.lever.co").unwrap();
        assert_eq!(u.request_target, "/");

        let u = parse_url("https://api.ashbyhq.com:8443/x?y=1").unwrap();
        assert_eq!(u.port, 8443);
        assert_eq!(u.request_target, "/x?y=1");

        assert!(matches!(
            parse_url("ftp://x/y"),
            Err(NetError::SchemeNotAllowed(_))
        ));
        assert!(matches!(parse_url("https://"), Err(NetError::BadUrl(_))));
        assert!(matches!(
            parse_url("https://user:pw@host/x"),
            Err(NetError::BadUrl(_))
        ));
        assert!(matches!(
            parse_url("https://host:notaport/x"),
            Err(NetError::BadUrl(_))
        ));
    }

    #[test]
    fn plain_http_to_the_internet_is_refused() {
        assert!(matches!(
            get("http://example.com/jobs", 2000),
            Err(NetError::SchemeNotAllowed(_))
        ));
    }

    #[test]
    fn url_parsing_handles_bracketed_ipv6() {
        let u = parse_url("https://[2606:4700::1111]/board/jobs").unwrap();
        assert_eq!(u.host, "2606:4700::1111");
        assert_eq!(u.port, 443);
        assert_eq!(u.request_target, "/board/jobs");

        let u = parse_url("https://[::1]:11434/api").unwrap();
        assert_eq!(u.host, "::1");
        assert_eq!(u.port, 11434);

        assert!(matches!(
            parse_url("https://[::1/x"),
            Err(NetError::BadUrl(_))
        ));
        assert!(matches!(
            parse_url("https://2606:4700::1111/x"),
            Err(NetError::BadUrl(_))
        ));
    }

    #[test]
    fn private_and_loopback_targets_are_blocked() {
        assert!(matches!(
            get("https://localhost/x", 2000),
            Err(NetError::BlockedHost(_))
        ));
        assert!(matches!(
            get("https://127.0.0.1/x", 2000),
            Err(NetError::BlockedHost(_))
        ));
        assert!(matches!(
            get("https://10.0.0.5/x", 2000),
            Err(NetError::BlockedHost(_))
        ));
        assert!(matches!(
            get("https://192.168.1.10/x", 2000),
            Err(NetError::BlockedHost(_))
        ));
        assert!(matches!(
            get("https://[::1]/x", 2000),
            Err(NetError::BlockedHost(_))
        ));
    }

    #[test]
    fn disallowed_ip_classifier_covers_the_dangerous_ranges() {
        let bad = [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.1.1",
            "192.0.0.5",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ];
        for ip in bad {
            let parsed: std::net::IpAddr = ip.parse().unwrap();
            assert!(is_disallowed_ip(&parsed), "{ip} must be disallowed");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700::1111", "104.16.0.1"] {
            let parsed: std::net::IpAddr = ip.parse().unwrap();
            assert!(!is_disallowed_ip(&parsed), "{ip} must be allowed");
        }
    }

    #[test]
    fn loopback_helper_refuses_non_loopback() {
        let addr: SocketAddr = "10.0.0.1:11434".parse().unwrap();
        assert!(matches!(
            request_loopback(addr, "GET", "/api/version", None, 500, 1024),
            Err(NetError::BlockedHost(_))
        ));
    }

    #[test]
    fn query_only_urls_fragments_and_request_injection_are_handled_before_writing() {
        assert_eq!(
            parse_url("https://example.org?job=123#apply")
                .unwrap()
                .request_target,
            "/?job=123"
        );
        assert_eq!(
            parse_url("https://example.org/job#apply")
                .unwrap()
                .request_target,
            "/job"
        );
        assert!(parse_url("https://example.org/job\r\nX-Fictional: injected").is_err());
        for (host, path, method) in [
            ("example.org\r\nX-Fictional: x", "/", "GET"),
            ("example.org", "/job\r\nX-Fictional: x", "GET"),
            ("example.org", "/", "GET\r\nPOST"),
        ] {
            let mut written = Vec::new();
            assert!(write_request(&mut written, host, path, method, None, false).is_err());
            assert!(written.is_empty());
        }
        let mut written = Vec::new();
        let names = write_request(&mut written, "example.org", "/job", "GET", None, true).unwrap();
        assert_eq!(
            names,
            [
                "host",
                "user-agent",
                "accept",
                "accept-encoding",
                "connection"
            ]
        );
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("Accept: */*\r\n"));
        assert!(text.contains("Accept-Encoding: identity\r\n"));
        assert!(!text.to_ascii_lowercase().contains("\r\norigin:"));
        let mut response = std::io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        assert!(read_asset_response(&mut response, "GET", 1024)
            .unwrap()
            .request_header_names
            .is_none());
        assert!(
            !text.contains("Cookie:")
                && !text.contains("Authorization:")
                && !text.contains("Content-Type:")
        );
    }

    fn spawn_server(response: &'static [u8]) -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut conn, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let _ = conn.read(&mut buf);
                let _ = conn.write_all(response);
            }
        });
        addr
    }

    #[test]
    fn parses_content_length_and_chunked_over_a_real_socket() {
        let addr = spawn_server(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}");
        let r = request_loopback(addr, "GET", "/x", None, 2000, 4096).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.text(), "{\"ok\":true}");

        let addr = spawn_server(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n{\"ok\": tr\r\n3\r\nue}\r\n0\r\n\r\n",
        );
        let r = request_loopback(addr, "GET", "/x", None, 2000, 4096).unwrap();
        assert_eq!(r.text(), "{\"ok\": true}");
    }

    #[test]
    fn body_cap_is_enforced() {
        let addr = spawn_server(b"HTTP/1.1 200 OK\r\nContent-Length: 5000\r\n\r\n");
        // A declared body larger than the wire cap is refused before allocation.
        let r = request_loopback(addr, "GET", "/x", None, 1000, 128);
        assert!(matches!(r, Err(NetError::BodyTooLarge)));
    }

    /// Live test: real HTTPS fetch of a public ATS board (network required).
    /// `cargo test -p waypoint_net -- --ignored`
    #[test]
    #[ignore = "requires network access"]
    fn live_https_fetch_of_public_board() {
        let r = get(
            "https://boards-api.greenhouse.io/v1/boards/vercel/jobs",
            20_000,
        )
        .expect("live fetch");
        assert_eq!(r.status, 200);
        let v: serde_json::Value = serde_json::from_slice(&r.body).expect("json");
        let jobs = v["jobs"].as_array().expect("jobs array");
        assert!(!jobs.is_empty());
        println!("live: {} postings fetched over TLS", jobs.len());
    }
}
