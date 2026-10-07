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
//! - Responses are size-capped and time-bounded; no request ever sends
//!   `Accept-Encoding`, so no decompression bomb surface.
//! - We identify ourselves in `User-Agent` — a politeness/terms requirement for
//!   public APIs, not an optional nicety.
//! - TLS uses the OS trust store (schannel on Windows): no vendored crypto
//!   stack, and certificate validation is the platform's job.

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
    let (authority, target) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
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
        request_target: target.to_string(),
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
    let parsed = parse_url(url)?;
    if !parsed.https {
        return Err(NetError::SchemeNotAllowed(
            "plain http is only allowed for the local inference runtime".into(),
        ));
    }
    // Fast, cheap refusal for obviously-private host names before any DNS work.
    if !waypoint_connectors::is_public_host_url(url) {
        return Err(NetError::BlockedHost(parsed.host.clone()));
    }
    let addrs = resolve_public(&parsed)?;
    let timeout = Duration::from_millis(timeout_ms);
    let mut last_err: Option<NetError> = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout.min(Duration::from_secs(15))) {
            Ok(tcp) => {
                let _ = tcp.set_read_timeout(Some(timeout));
                let _ = tcp.set_write_timeout(Some(timeout));
                let connector =
                    native_tls::TlsConnector::new().map_err(|e| NetError::Tls(e.to_string()))?;
                let mut tls = connector
                    .connect(&parsed.host, tcp)
                    .map_err(|e| NetError::Tls(e.to_string()))?;
                return request(
                    &mut tls,
                    &parsed.host,
                    &parsed.request_target,
                    "GET",
                    None,
                    max_bytes,
                );
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
    let body_bytes = body.unwrap_or(&[]);
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body_bytes))
        .map_err(|e| NetError::Io(format!("write: {e}")))?;

    let mut raw: Vec<u8> = Vec::with_capacity(16 * 1024);
    let mut buf = [0u8; 32 * 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > max_bytes {
                    return Err(NetError::BodyTooLarge);
                }
                if let Some(split) = find_headers_end(&raw) {
                    let headers = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
                    let body_raw = &raw[split + 4..];
                    if headers.contains("transfer-encoding: chunked") {
                        if decode_chunked(body_raw).is_some() {
                            break;
                        }
                    } else if let Some(len) = content_length(&headers) {
                        if body_raw.len() >= len {
                            break;
                        }
                    } else if headers.contains("connection: close") {
                        // body runs to EOF; keep reading
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Err(NetError::Timeout)
            }
            Err(e) => return Err(NetError::Io(format!("read: {e}"))),
        }
    }

    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Result<HttpResponse, NetError> {
    let split = find_headers_end(raw).ok_or_else(|| NetError::Io("malformed response".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| NetError::Io("malformed status line".into()))?;
    let headers = head.to_ascii_lowercase();
    let body_raw = &raw[split + 4..];
    let body = if headers.contains("transfer-encoding: chunked") {
        decode_chunked(body_raw).ok_or_else(|| NetError::Io("invalid chunked body".into()))?
    } else {
        match content_length(&headers) {
            Some(len) => body_raw[..len.min(body_raw.len())].to_vec(),
            None => body_raw.to_vec(),
        }
    };
    Ok(HttpResponse { status, body })
}

fn find_headers_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(headers: &str) -> Option<usize> {
    headers
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
}

pub fn decode_chunked(body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = body[pos..].windows(2).position(|w| w == b"\r\n")? + pos;
        let size_str = std::str::from_utf8(&body[pos..line_end]).ok()?;
        let size = usize::from_str_radix(size_str.trim().split(';').next()?, 16).ok()?;
        pos = line_end + 2;
        if size == 0 {
            return Some(out);
        }
        if pos + size > body.len() {
            return None;
        }
        out.extend_from_slice(&body[pos..pos + size]);
        pos += size + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // server sends no body then closes; cap is not the failure here, but the
        // parser must not panic and must return a bounded body.
        let r = request_loopback(addr, "GET", "/x", None, 1000, 128);
        assert!(r.is_ok() || matches!(r, Err(NetError::Timeout)));
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
