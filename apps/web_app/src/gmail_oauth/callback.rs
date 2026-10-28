// Adapted from rejection-rejector commit 48c672b29cf098ff5da10d324de3e87766a2f483. See docs/REJECTION_REJECTOR_LICENSE.md.
//! Bounded, inert HTTP parsing for the temporary loopback OAuth listener.
use anyhow::{ensure, Context, Result};
use std::{
    io::{ErrorKind, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use zeroize::Zeroizing;

const MAX_HEADER_BYTES: usize = 8192;

/// The caller must bound each blocking read (the TCP caller uses 250 ms).
/// A per-connection absolute deadline prevents a slow stream from renewing a
/// per-read timeout indefinitely. Never accept an incomplete header block.
///
/// Inputs: `reader` — callback connection stream; `cancelled` — cancellation
/// flag checked on every chunk; `deadline` — absolute per-connection deadline.
/// Output: `Result<Zeroizing<Vec<u8>>>` — the complete header block; errors on
/// cancellation, deadline, or an incomplete block.
pub(super) fn read_headers(
    reader: &mut impl Read,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    let mut chunk = Zeroizing::new([0u8; 1024]);
    loop {
        ensure!(!cancelled.load(Ordering::SeqCst), "Authorization cancelled");
        ensure!(
            Instant::now() < deadline,
            "OAuth callback deadline exceeded"
        );
        ensure!(
            bytes.len() < MAX_HEADER_BYTES,
            "OAuth callback headers are too large"
        );
        let remaining = (MAX_HEADER_BYTES - bytes.len()).min(chunk.len());
        match reader.read(&mut chunk[..remaining]) {
            Ok(0) => anyhow::bail!("Incomplete OAuth callback request"),
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
            Err(error) => ensure!(
                matches!(
                    error.kind(),
                    ErrorKind::Interrupted | ErrorKind::TimedOut | ErrorKind::WouldBlock
                ),
                "Cannot read OAuth callback request"
            ),
        }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            bytes.truncate(end + 4);
            return Ok(bytes);
        }
    }
}

/// Parse only a GET with a unique literal-loopback Host and no request body.
///
/// Inputs: `headers` — complete header block; `port` — loopback port the
/// callback listener bound. Output: `Result<&str>` — the request target of the
/// validated `GET`; anything else is rejected.
pub(super) fn request_target(headers: &[u8], port: u16) -> Result<&str> {
    ensure!(
        headers.len() <= MAX_HEADER_BYTES && headers.ends_with(b"\r\n\r\n"),
        "Invalid OAuth callback header block"
    );
    let text = std::str::from_utf8(headers).context("Invalid OAuth callback encoding")?;
    let mut lines = text[..text.len() - 4].lines();
    let first = lines
        .next()
        .context("OAuth callback request line is missing")?;
    let parts: Vec<_> = first.split(' ').collect();
    ensure!(
        parts.len() == 3
            && parts[0] == "GET"
            && matches!(parts[2], "HTTP/1.1" | "HTTP/1.0")
            && parts[1].starts_with('/')
            && !parts[1].starts_with("//")
            && !parts[1].contains('#')
            && !parts[1].chars().any(char::is_control),
        "Invalid OAuth callback request line"
    );
    let expected_host = format!("127.0.0.1:{port}");
    let mut hosts = 0;
    let mut lengths = 0;
    for line in lines {
        ensure!(
            !line.starts_with([' ', '\t']) && !line.contains(['\r', '\n']),
            "Invalid OAuth callback header"
        );
        let (name, value) = line
            .split_once(':')
            .context("Invalid OAuth callback header")?;
        ensure!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid OAuth callback header name"
        );
        let value = value.trim();
        if name.eq_ignore_ascii_case("Host") {
            hosts += 1;
            ensure!(value == expected_host, "Invalid OAuth callback Host");
        }
        if name.eq_ignore_ascii_case("Content-Length") {
            lengths += 1;
            ensure!(value == "0", "OAuth callback request bodies are forbidden");
        }
        ensure!(
            !name.eq_ignore_ascii_case("Transfer-Encoding"),
            "OAuth callback transfer encodings are forbidden"
        );
    }
    ensure!(
        hosts == 1 && lengths <= 1,
        "Ambiguous OAuth callback headers"
    );
    Ok(parts[1])
}
