//! HTTP/1 response framing with retained headers for captured page resources.
use crate::NetError;
use std::{io::Read, ops::Range};

const MAX_HEADERS: usize = 128;
const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_CHUNKS: usize = 65536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyFraming {
    NoBody,
    ContentLength,
    Chunked,
    ConnectionClose,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedResponse {
    pub request_method: String,
    pub status: u16,
    /// Original field bytes, in received order. Transfer framing is decoded below.
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub trailers: Vec<Header>,
    pub framing: BodyFraming,
}
struct Head {
    offset: usize,
    status: u16,
    headers: Vec<Header>,
    framing: BodyFraming,
    length: Option<usize>,
}
fn invalid(reason: &'static str) -> NetError {
    NetError::InvalidResponse(reason)
}
fn head(raw: &[u8], method: &str) -> Result<Option<Head>, NetError> {
    let mut fields = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut response = httparse::Response::new(&mut fields);
    let offset = match response
        .parse(raw)
        .map_err(|_| invalid("invalid response headers"))?
    {
        httparse::Status::Partial => {
            if raw.len() > MAX_HEAD_BYTES {
                return Err(invalid("response headers exceed budget"));
            }
            return Ok(None);
        }
        httparse::Status::Complete(offset) => offset,
    };
    if offset > MAX_HEAD_BYTES {
        return Err(invalid("response headers exceed budget"));
    }
    let status = response
        .code
        .ok_or_else(|| invalid("missing response status"))?;
    if !(100..=599).contains(&status) || status == 101 || method == "CONNECT" {
        return Err(invalid("unsupported response protocol"));
    }
    let mut length = None;
    let mut transfer = None;
    let headers: Vec<_> = response
        .headers
        .iter()
        .map(|h| Header {
            name: h.name.into(),
            value: h.value.to_vec(),
        })
        .collect();
    for h in &headers {
        if h.name.eq_ignore_ascii_case("content-length") {
            let text =
                std::str::from_utf8(&h.value).map_err(|_| invalid("invalid content length"))?;
            for part in text.split(',') {
                let part = part.trim();
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid("invalid content length"));
                }
                let n = part
                    .parse::<usize>()
                    .map_err(|_| invalid("invalid content length"))?;
                if length.is_some_and(|old| old != n) {
                    return Err(invalid("conflicting content lengths"));
                }
                length = Some(n);
            }
        }
        if h.name.eq_ignore_ascii_case("transfer-encoding") {
            if transfer.is_some() {
                return Err(invalid("duplicate transfer encoding"));
            }
            let value =
                std::str::from_utf8(&h.value).map_err(|_| invalid("invalid transfer encoding"))?;
            if !value.trim().eq_ignore_ascii_case("chunked") {
                return Err(invalid("unsupported transfer encoding"));
            }
            transfer = Some(());
        }
    }
    if transfer.is_some() && length.is_some() {
        return Err(invalid("ambiguous response framing"));
    }
    let framing = if method == "HEAD" || status < 200 || matches!(status, 204 | 304) {
        BodyFraming::NoBody
    } else if transfer.is_some() {
        BodyFraming::Chunked
    } else if length.is_some() {
        BodyFraming::ContentLength
    } else {
        BodyFraming::ConnectionClose
    };
    Ok(Some(Head {
        offset,
        status,
        headers,
        framing,
        length,
    }))
}
#[derive(Default)]
struct Chunks {
    position: usize,
    size: Option<usize>,
    ranges: Vec<Range<usize>>,
    trailers: bool,
    trailer_fields: Vec<Header>,
}
impl Chunks {
    fn scan(&mut self, data: &[u8]) -> Result<Option<usize>, NetError> {
        loop {
            if self.trailers {
                let tail = &data[self.position..];
                if tail.starts_with(b"\r\n") {
                    return Ok(Some(self.position + 2));
                }
                let mut fields = [httparse::EMPTY_HEADER; MAX_HEADERS];
                match httparse::parse_headers(tail, &mut fields)
                    .map_err(|_| invalid("invalid response trailers"))?
                {
                    httparse::Status::Complete((end, fields)) => {
                        if end > MAX_HEAD_BYTES {
                            return Err(invalid("response trailers exceed budget"));
                        }
                        // Framing/authentication metadata cannot be supplied later.
                        if fields.iter().any(|h| {
                            [
                                "content-length",
                                "transfer-encoding",
                                "content-encoding",
                                "set-cookie",
                                "authorization",
                                "proxy-authorization",
                            ]
                            .iter()
                            .any(|n| h.name.eq_ignore_ascii_case(n))
                        }) {
                            return Err(invalid("unsupported response trailer"));
                        }
                        self.trailer_fields = fields
                            .iter()
                            .map(|h| Header {
                                name: h.name.into(),
                                value: h.value.to_vec(),
                            })
                            .collect();
                        return Ok(Some(self.position + end));
                    }
                    httparse::Status::Partial => {
                        if tail.len() > MAX_HEAD_BYTES {
                            return Err(invalid("response trailers exceed budget"));
                        }
                        return Ok(None);
                    }
                }
            }
            if self.size.is_none() {
                let tail = &data[self.position..];
                let (offset, size) = match httparse::parse_chunk_size(tail)
                    .map_err(|_| invalid("invalid chunk size"))?
                {
                    httparse::Status::Complete(v) => v,
                    httparse::Status::Partial => {
                        if tail.len() > 8192 {
                            return Err(invalid("chunk header exceeds budget"));
                        }
                        return Ok(None);
                    }
                };
                if offset > 8192 {
                    return Err(invalid("chunk header exceeds budget"));
                }
                let size = usize::try_from(size).map_err(|_| NetError::BodyTooLarge)?;
                self.position += offset;
                if size == 0 {
                    self.trailers = true;
                    continue;
                }
                if self.ranges.len() >= MAX_CHUNKS {
                    return Err(invalid("chunk count exceeds budget"));
                }
                self.size = Some(size);
            }
            let size = self.size.unwrap();
            let end = self
                .position
                .checked_add(size)
                .ok_or(NetError::BodyTooLarge)?;
            let after = end.checked_add(2).ok_or(NetError::BodyTooLarge)?;
            if after > data.len() {
                return Ok(None);
            }
            if &data[end..after] != b"\r\n" {
                return Err(invalid("invalid chunk delimiter"));
            }
            self.ranges.push(self.position..end);
            self.position = after;
            self.size = None;
        }
    }
    fn decode(&self, data: &[u8]) -> Vec<u8> {
        self.ranges
            .iter()
            .flat_map(|r| data[r.clone()].iter().copied())
            .collect()
    }
}

/// Read one final response; interim heads and all wire bytes share the same cap.
pub(crate) fn receive<S: Read>(
    stream: &mut S,
    method: &str,
    max_bytes: usize,
) -> Result<CapturedResponse, NetError> {
    let mut raw = Vec::new();
    let mut total = 0usize;
    let mut interim = 0;
    let mut parsed = None;
    let mut chunks = Chunks::default();
    let mut eof = false;
    loop {
        if parsed.is_none() {
            if let Some(value) = head(&raw, method)? {
                if value.status < 200 {
                    interim += 1;
                    if interim > 8 {
                        return Err(invalid("informational response budget exceeded"));
                    }
                    raw.drain(..value.offset);
                    continue;
                }
                if value.framing == BodyFraming::ContentLength
                    && value.length.unwrap()
                        > max_bytes.saturating_sub(total - raw.len() + value.offset)
                {
                    return Err(NetError::BodyTooLarge);
                }
                parsed = Some(value);
            }
        }
        if let Some(value) = &parsed {
            let body = &raw[value.offset..];
            let complete = match value.framing {
                BodyFraming::NoBody => Some(Vec::new()),
                BodyFraming::ContentLength => value
                    .length
                    .filter(|n| body.len() >= *n)
                    .map(|n| body[..n].to_vec()),
                BodyFraming::Chunked => chunks.scan(body)?.map(|_| chunks.decode(body)),
                BodyFraming::ConnectionClose => eof.then(|| body.to_vec()),
            };
            if let Some(body) = complete {
                let value = parsed.take().unwrap();
                return Ok(CapturedResponse {
                    request_method: method.to_owned(),
                    status: value.status,
                    headers: value.headers,
                    body,
                    trailers: chunks.trailer_fields,
                    framing: value.framing,
                });
            }
        }
        if eof {
            return Err(NetError::IncompleteResponse);
        }
        let mut buffer = [0u8; 32768];
        // One excess byte is sufficient to detect overflow, even on an open socket.
        let remaining = max_bytes
            .saturating_sub(total)
            .saturating_add(1)
            .min(buffer.len());
        let count = match stream.read(&mut buffer[..remaining]) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err(NetError::Timeout)
            }
            Err(e) => return Err(NetError::Io(format!("read: {e}"))),
        };
        if count == 0 {
            eof = true;
            continue;
        }
        total += count;
        if total > max_bytes {
            return Err(NetError::BodyTooLarge);
        }
        raw.extend_from_slice(&buffer[..count]);
    }
}

pub(crate) fn decode_chunks(body: &[u8]) -> Option<Vec<u8>> {
    let mut chunks = Chunks::default();
    chunks.scan(body).ok()??;
    Some(chunks.decode(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    struct Fragmented {
        bytes: std::io::Cursor<Vec<u8>>,
        fragment: usize,
    }
    impl Read for Fragmented {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let size = out.len().min(self.fragment);
            self.bytes.read(&mut out[..size])
        }
    }
    impl Write for Fragmented {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn parse(
        raw: &[u8],
        fragment: usize,
        method: &str,
        cap: usize,
    ) -> Result<CapturedResponse, NetError> {
        receive(
            &mut Fragmented {
                bytes: std::io::Cursor::new(raw.to_vec()),
                fragment,
            },
            method,
            cap,
        )
    }
    #[test]
    fn every_fragment_size_preserves_binary_body_and_ordered_header_bytes() {
        let raw=b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Type: image/png\r\nX-Test: first\r\nX-Test: \xff\r\n\r\n\x00\xffab";
        for fragment in 1..=raw.len() {
            let value = parse(raw, fragment, "GET", 4096).unwrap();
            assert_eq!(value.body, b"\x00\xffab");
            assert_eq!(value.headers[2].value, b"first");
            assert_eq!(value.headers[3].value, b"\xff");
            assert_eq!(value.framing, BodyFraming::ContentLength);
        }
    }
    #[test]
    fn declared_length_is_never_silently_truncated() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ntest";
        for end in 0..raw.len() {
            assert!(matches!(
                parse(&raw[..end], 1, "GET", 4096),
                Err(NetError::IncompleteResponse)
            ));
        }
        assert_eq!(parse(raw, 1, "GET", 4096).unwrap().body, b"test");
    }
    #[test]
    fn informational_responses_and_bodyless_statuses_obey_framing_precedence() {
        let raw=b"HTTP/1.1 103 Early Hints\r\nLink: </a.css>\r\n\r\nHTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        assert_eq!(parse(raw, 1, "GET", 4096).unwrap().body, b"ok");
        for status in [200, 204, 304] {
            let raw = format!("HTTP/1.1 {status} OK\r\nContent-Length: 999999999\r\n\r\n");
            let value = parse(
                raw.as_bytes(),
                1,
                if status == 200 { "HEAD" } else { "GET" },
                512,
            )
            .unwrap();
            assert!(value.body.is_empty());
            assert_eq!(value.framing, BodyFraming::NoBody);
        }
        let heads = b"HTTP/1.1 103 Early Hints\r\n\r\n".repeat(9);
        assert!(matches!(
            parse(&heads, 1, "GET", 4096),
            Err(NetError::InvalidResponse(_))
        ));
    }
    #[test]
    fn framing_fields_are_exact_and_ambiguity_is_refused() {
        for headers in [
            "Content-Length: 1\r\nContent-Length: 2",
            "Content-Length: 1, 2",
            "Content-Length: +1",
            "Content-Length: no",
            "Content-Length: 999999999999999999999999999",
            "Transfer-Encoding: gzip, chunked",
            "Transfer-Encoding: chunked\r\nContent-Length: 1",
            "Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked",
        ] {
            let raw = format!("HTTP/1.1 200 OK\r\n{headers}\r\n\r\nx");
            assert!(
                matches!(
                    parse(raw.as_bytes(), 1, "GET", 4096),
                    Err(NetError::InvalidResponse(_))
                ),
                "{headers}"
            );
        }
        let raw=b"HTTP/1.1 200 OK\r\nContent-Length: 1, 1\r\nContent-Length: 1\r\nX-Note: transfer-encoding: chunked\r\n\r\nx";
        assert_eq!(parse(raw, 1, "GET", 4096).unwrap().body, b"x");
    }
    #[test]
    fn chunked_fragments_trailers_and_truncation_are_explicit() {
        let raw=b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2;trace=yes\r\nab\r\n1\r\nc\r\n0\r\nX-Trace: done\r\n\r\n";
        for fragment in 1..=raw.len() {
            let value = parse(raw, fragment, "GET", 4096).unwrap();
            assert_eq!(value.body, b"abc");
            assert_eq!(value.trailers[0].value, b"done");
        }
        for end in 0..raw.len() {
            assert!(parse(&raw[..end], 1, "GET", 4096).is_err());
        }
        for body in [
            b"1\r\nxXX0\r\n\r\n".as_slice(),
            b"0\r\nContent-Length: 0\r\n\r\n",
        ] {
            let mut raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
            raw.extend(body);
            assert!(matches!(
                parse(&raw, 1, "GET", 4096),
                Err(NetError::InvalidResponse(_))
            ));
        }
    }
    #[test]
    fn wire_header_chunk_and_count_budgets_are_bounded() {
        assert!(matches!(
            parse(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n",
                1,
                "GET",
                128
            ),
            Err(NetError::BodyTooLarge)
        ));
        let raw = format!("HTTP/1.1 200 OK\r\nX-Large: {}", "a".repeat(MAX_HEAD_BYTES));
        assert!(matches!(
            parse(raw.as_bytes(), 32768, "GET", 128 * 1024),
            Err(NetError::InvalidResponse(_))
        ));
        let raw = b"HTTP/1.1 200 OK\r\n\r\nclose delimited";
        assert_eq!(
            parse(raw, 1, "GET", 4096).unwrap().framing,
            BodyFraming::ConnectionClose
        );
        assert!(matches!(
            parse(b"HTTP/1.1 101 Upgrade\r\n\r\n", 1, "GET", 4096),
            Err(NetError::InvalidResponse(_))
        ));
        let mut chunks = Chunks {
            ranges: vec![0..0; MAX_CHUNKS],
            ..Chunks::default()
        };
        assert!(matches!(
            chunks.scan(b"1\r\nx\r\n"),
            Err(NetError::InvalidResponse(_))
        ));
    }
}
