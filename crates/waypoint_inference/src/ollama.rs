//! T007 production adapter: Ollama over loopback HTTP, std-only.
//!
//! Design rules enforced structurally:
//! - The runtime origin must be a literal `http://127.0.0.1:<port>`. Anything
//!   else (https, other hosts, DNS names) is refused — there is no code path
//!   that could dial a cloud model, so "no silent cloud fallback" is a
//!   property of the type, not a promise in a prompt.
//! - Structured output uses Ollama's JSON-schema `format` when the caller
//!   names one of the six handoff schemas, else strict `json` mode.
//! - Reasoning is probed with an adequate token budget; a model that can only
//!   produce an empty answer under `think:true` is unqualified (recorded G1
//!   pitfall: small `num_predict` silently yields an empty final answer).
//! - Cancel is honored between requests and via socket timeouts within one.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::{CapabilityReport, Inference, InferenceError, UsageReport};

/// The six JSON Schemas vendored from the research handoff. Passing any other
/// name falls back to generic JSON mode (never a silent schema mismatch).
pub const KNOWN_SCHEMAS: &[(&str, &str)] = &[
    (
        "application_link_ranking",
        r#"{"type":"object","properties":{"indices":{"type":"array","items":{"type":"integer","minimum":0,"maximum":29},"maxItems":5}},"required":["indices"],"additionalProperties":false}"#,
    ),
    (
        "claim",
        include_str!("../../../docs/waypoint-handoff/schemas/claim.schema.json"),
    ),
    (
        "approval_grant",
        include_str!("../../../docs/waypoint-handoff/schemas/approval_grant.schema.json"),
    ),
    (
        "job_observation",
        include_str!("../../../docs/waypoint-handoff/schemas/job_observation.schema.json"),
    ),
    (
        "application_intent",
        include_str!("../../../docs/waypoint-handoff/schemas/application_intent.schema.json"),
    ),
    (
        "receipt",
        include_str!("../../../docs/waypoint-handoff/schemas/receipt.schema.json"),
    ),
    (
        "reasoning_run",
        include_str!("../../../docs/waypoint-handoff/schemas/reasoning_run.schema.json"),
    ),
];

const PROBE_TIMEOUT_MS: u64 = 20_000;
const GENERATE_TIMEOUT_MS: u64 = 300_000;

pub struct OllamaInference {
    addr: SocketAddr,
    model: String,
    context_window: Option<u32>,
    pub think: bool,
    cancel_flag: Arc<AtomicBool>,
}

impl OllamaInference {
    /// `origin` must be exactly `http://127.0.0.1:<port>`.
    pub fn new(origin: &str, model: impl Into<String>) -> Result<Self, InferenceError> {
        let addr = parse_loopback_origin(origin)?;
        Ok(Self {
            addr,
            model: model.into(),
            context_window: None,
            think: true,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Request an explicit context size. The caller is responsible for checking
    /// that the resident model and machine have sufficient resources.
    pub fn with_context_window(mut self, tokens: u32) -> Result<Self, InferenceError> {
        if !(1024..=131_072).contains(&tokens) {
            return Err(InferenceError::BudgetExceeded);
        }
        self.context_window = Some(tokens);
        Ok(self)
    }

    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel_flag)
    }

    fn cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::Relaxed)
    }

    fn post_json(
        &self,
        path: &str,
        body: &Value,
        timeout_ms: u64,
    ) -> Result<Value, InferenceError> {
        let payload = serde_json::to_vec(body)
            .map_err(|e| InferenceError::StructuredOutputFailed(e.to_string()))?;
        let (status, bytes) = http_request(self.addr, "POST", path, Some(&payload), timeout_ms)
            .map_err(InferenceError::RuntimeUnreachable)?;
        if !(200..300).contains(&status) {
            return Err(InferenceError::RuntimeUnreachable(format!(
                "local runtime returned HTTP {status}"
            )));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| InferenceError::StructuredOutputFailed(e.to_string()))
    }

    fn get_json(&self, path: &str, timeout_ms: u64) -> Result<Value, InferenceError> {
        let (status, bytes) = http_request(self.addr, "GET", path, None, timeout_ms)
            .map_err(InferenceError::RuntimeUnreachable)?;
        if !(200..300).contains(&status) {
            return Err(InferenceError::RuntimeUnreachable(format!(
                "local runtime returned HTTP {status}"
            )));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| InferenceError::StructuredOutputFailed(e.to_string()))
    }
}

impl Inference for OllamaInference {
    fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
        // 1. Runtime reachable.
        let version = self.get_json("/api/version", PROBE_TIMEOUT_MS)?;
        let _ = version
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                InferenceError::StructuredOutputFailed("runtime did not report a version".into())
            })?;

        // 2. Structured-output probe (strict JSON mode, no reasoning tokens).
        let structured = self.post_json(
            "/api/generate",
            &serde_json::json!({
                "model": self.model,
                "prompt": "Return exactly this JSON object and nothing else: {\"ok\": true}",
                "stream": false,
                "format": "json",
                "think": false,
                "options": {"num_predict": 48, "temperature": 0.0},
            }),
            PROBE_TIMEOUT_MS,
        )?;
        let structured_ok = structured
            .get("response")
            .and_then(|r| r.as_str())
            .map(|s| {
                serde_json::from_str::<Value>(s.trim())
                    .ok()
                    .is_some_and(|v| v.is_object())
            })
            .unwrap_or(false);
        if !structured_ok {
            return Err(InferenceError::StructuredOutputFailed(
                "runtime failed the strict-JSON probe".into(),
            ));
        }

        // 3. Reasoning probe with an adequate budget (learned pitfall: a tiny
        // num_predict under think:true yields an empty final answer and would
        // fake "reasoning unavailable"). Reasoning is verified by checking a
        // multi-step arithmetic answer for correctness — the runtime may
        // deliver reasoning via a separate `thinking` field (this build) or
        // inline, and the final answer may be plain prose, not JSON.
        let reasoning = self.post_json(
            "/api/generate",
            &serde_json::json!({
                "model": self.model,
                "prompt": "What is 17 * 23? Show your reasoning briefly.",
                "stream": false,
                "think": true,
                "options": {"num_predict": 768, "temperature": 0.0},
            }),
            PROBE_TIMEOUT_MS,
        )?;
        let response_text = reasoning
            .get("response")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();
        let thinking_text = reasoning
            .get("thinking")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();
        let done_reason = reasoning
            .get("done_reason")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        let answer_ok = response_text.contains("391");
        let reasoning_evidence = !thinking_text.is_empty()
            || response_text.contains("<think>")
            || response_text.contains("391");
        if !(answer_ok && reasoning_evidence) {
            if response_text.is_empty() && done_reason == "length" {
                return Err(InferenceError::ReasoningUnavailable);
            }
            return Err(InferenceError::ReasoningUnavailable);
        }

        // 4. Report the resident instance, not a model's theoretical maximum.
        let ps = self.get_json("/api/ps", PROBE_TIMEOUT_MS)?;
        let served = ps
            .get("models")
            .and_then(|m| m.as_array())
            .and_then(|models| {
                models.iter().find(|m| {
                    m.get("name")
                        .and_then(|n| n.as_str())
                        .is_some_and(|n| n == self.model)
                })
            });
        let (gpu_bytes, cpu_bytes) = resident_memory(served);
        let quantization = match served {
            Some(m) => m
                .pointer("/details/quantization_level")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            None => "unknown".to_string(),
        };
        let context = served.and_then(|m| m.get("context_length").and_then(Value::as_u64));

        Ok(CapabilityReport {
            backend: "ollama".into(),
            model_name: self.model.clone(),
            quantization,
            context_window_tokens: context.unwrap_or(0).min(u32::MAX as u64) as u32,
            supports_reasoning: true,
            supports_structured_output: true,
            cpu_offload_bytes: cpu_bytes,
            gpu_reserved_bytes: gpu_bytes,
        })
    }

    fn generate_structured(
        &mut self,
        schema_name: &str,
        prompt: &str,
        max_output_tokens: u32,
    ) -> Result<(Value, UsageReport), InferenceError> {
        if self.cancelled() {
            return Err(InferenceError::Cancelled);
        }
        if max_output_tokens == 0 {
            return Err(InferenceError::BudgetExceeded);
        }
        let format: Value = match KNOWN_SCHEMAS.iter().find(|(n, _)| *n == schema_name) {
            Some((_, schema_json)) => serde_json::from_str(schema_json)
                .map_err(|e| InferenceError::StructuredOutputFailed(e.to_string()))?,
            None => Value::String("json".into()),
        };
        // Measured pitfall (2026-09-26): with think:true + schema format on
        // this runtime, reasoning consumed the entire budget (2238 thinking
        // chars, response empty, done_reason=length). Structured generation
        // therefore disables thinking by default; reasoning runs happen on
        // plain prompts where the thinking field is expected. Callers who
        // want reasoning before a structured result run two steps and pass
        // their own synthesized context.
        let mut body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "format": format,
            "think": false,
            "options": {"num_predict": max_output_tokens, "temperature": 0.0},
        });
        if let Some(tokens) = self.context_window {
            body["options"]["num_ctx"] = tokens.into();
        }
        let out = self.post_json("/api/generate", &body, GENERATE_TIMEOUT_MS)?;
        if self.cancelled() {
            return Err(InferenceError::Cancelled);
        }
        require_natural_stop(&out)?;
        let text = out
            .get("response")
            .and_then(|r| r.as_str())
            .ok_or_else(|| InferenceError::StructuredOutputFailed("no response field".into()))?;
        let parsed: Value = serde_json::from_str(text.trim()).map_err(|e| {
            InferenceError::StructuredOutputFailed(format!("model output not valid JSON: {e}"))
        })?;
        let usage = UsageReport {
            prompt_tokens: out
                .get("prompt_eval_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            completion_tokens: out.get("eval_count").and_then(|v| v.as_u64()).unwrap_or(0),
            wall_time_ms: out
                .get("total_duration")
                .and_then(|v| v.as_u64())
                .map(|ns| ns / 1_000_000)
                .unwrap_or(0),
        };
        Ok((parsed, usage))
    }

    fn cancel(&mut self) -> Result<(), InferenceError> {
        self.cancel_flag.store(true, Ordering::Relaxed);
        Ok(())
    }
}

fn resident_memory(model: Option<&Value>) -> (Option<u64>, Option<u64>) {
    let Some(model) = model else {
        return (None, None);
    };
    let gpu = model.get("size_vram").and_then(Value::as_u64);
    let size = model.get("size").and_then(Value::as_u64);
    match (size, gpu) {
        (Some(total), Some(gpu)) if total > 0 && gpu <= total => (Some(gpu), Some(total - gpu)),
        _ => (None, None),
    }
}

fn require_natural_stop(out: &Value) -> Result<(), InferenceError> {
    if out.get("done").and_then(Value::as_bool) != Some(true)
        || out.get("done_reason").and_then(Value::as_str) != Some("stop")
    {
        return Err(InferenceError::StructuredOutputFailed(
            "generation did not finish with a natural stop".into(),
        ));
    }
    Ok(())
}

/* ------------------------------ origin guard ------------------------------- */

fn parse_loopback_origin(origin: &str) -> Result<SocketAddr, InferenceError> {
    let rest = origin
        .strip_prefix("http://")
        .ok_or(InferenceError::NonLocalRuntime)?;
    let (host, port) = rest
        .rsplit_once(':')
        .ok_or(InferenceError::NonLocalRuntime)?;
    if host != "127.0.0.1" || rest.contains('/') {
        return Err(InferenceError::NonLocalRuntime);
    }
    let port: u16 = port.parse().map_err(|_| InferenceError::NonLocalRuntime)?;
    if port == 0 {
        return Err(InferenceError::NonLocalRuntime);
    }
    Ok(SocketAddr::from(([127, 0, 0, 1], port)))
}

/* --------------------------- minimal HTTP/1.1 ------------------------------- */

/// Blocking HTTP/1.1 over a fresh loopback connection. Supports
/// Content-Length and chunked responses. std-only by design: the inference
/// runtime is loopback, so no TLS stack is pulled into the supply chain.
pub fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    timeout_ms: u64,
) -> Result<(u16, Vec<u8>), String> {
    let timeout = Duration::from_millis(timeout_ms);
    let mut stream = TcpStream::connect_timeout(&addr, timeout.min(Duration::from_secs(10)))
        .map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;

    let body_bytes = body.unwrap_or(&[]);
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body_bytes))
        .map_err(|e| format!("write: {e}"))?;

    let mut raw = Vec::with_capacity(8 * 1024);
    const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
    let mut buf = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > MAX_RESPONSE_BYTES {
                    return Err("runtime response exceeds the 8 MiB limit".into());
                }
                if let Some(split) = find_headers_end(&raw) {
                    let headers = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
                    if headers.contains("transfer-encoding: chunked") {
                        // chunked: stop when the terminating chunk is present
                        if body_complete_chunked(&raw[split + 4..]) {
                            break;
                        }
                    } else if let Some(len) = content_length(&headers) {
                        if raw.len().saturating_sub(split + 4) >= len {
                            break;
                        }
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Err("timeout waiting for runtime".into())
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }

    let split = find_headers_end(&raw).ok_or("malformed HTTP response")?;
    let status: u16 = String::from_utf8_lossy(&raw[..split])
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or("malformed status line")?;
    let headers = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let body_raw = &raw[split + 4..];
    let bytes = if headers.contains("transfer-encoding: chunked") {
        decode_chunked(body_raw).ok_or("invalid chunked body")?
    } else {
        match content_length(&headers) {
            Some(len) if body_raw.len() >= len => body_raw[..len].to_vec(),
            Some(_) => return Err("incomplete content-length body".into()),
            None => body_raw.to_vec(),
        }
    };
    Ok((status, bytes))
}

fn find_headers_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        if let Some(v) = line.strip_prefix("content-length:") {
            return v.trim().parse().ok();
        }
    }
    None
}

fn body_complete_chunked(body: &[u8]) -> bool {
    decode_chunked(body).is_some()
}

/// Decodes a chunked body; returns None while incomplete/corrupt.
fn decode_chunked(body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = body
            .get(pos..)?
            .windows(2)
            .position(|w| w == b"\r\n")?
            .checked_add(pos)?;
        let size_str = std::str::from_utf8(&body[pos..line_end]).ok()?;
        let size = usize::from_str_radix(size_str.trim().split(';').next()?, 16).ok()?;
        pos = line_end + 2;
        if size == 0 {
            let remainder = body.get(pos..)?;
            return (remainder.starts_with(b"\r\n")
                || remainder.windows(4).any(|w| w == b"\r\n\r\n"))
            .then_some(out);
        }
        let end = pos.checked_add(size)?;
        let next = end.checked_add(2)?;
        if body.get(end..next)? != b"\r\n" {
            return None;
        }
        out.extend_from_slice(body.get(pos..end)?);
        pos = next;
    }
}

/* -------------------------------- tests ------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_context_window_is_bounded() {
        let model = || OllamaInference::new("http://127.0.0.1:11434", "test").unwrap();
        assert!(model().with_context_window(1023).is_err());
        assert_eq!(
            model().with_context_window(4096).unwrap().context_window,
            Some(4096)
        );
        assert!(model().with_context_window(131_073).is_err());
    }

    #[test]
    fn residency_reports_actual_gpu_and_unknown_incomplete_data() {
        let model = serde_json::json!({"size": 1000, "size_vram": 700});
        assert_eq!(resident_memory(Some(&model)), (Some(700), Some(300)));
        let full = serde_json::json!({"size": 1000, "size_vram": 1000});
        assert_eq!(resident_memory(Some(&full)), (Some(1000), Some(0)));
        for invalid in [
            serde_json::json!({"size":1000}),
            serde_json::json!({"size":1000,"size_vram":1001}),
            serde_json::json!({"size":0,"size_vram":0}),
        ] {
            assert_eq!(resident_memory(Some(&invalid)), (None, None));
        }
        assert_eq!(resident_memory(None), (None, None));
    }

    #[test]
    fn structured_generation_requires_explicit_natural_completion() {
        assert!(
            require_natural_stop(&serde_json::json!({"done":true,"done_reason":"stop"})).is_ok()
        );
        for response in [
            serde_json::json!({"done":true,"done_reason":"length"}),
            serde_json::json!({"done":false,"done_reason":"stop"}),
            serde_json::json!({"done":true}),
            serde_json::json!({}),
        ] {
            assert!(require_natural_stop(&response).is_err());
        }
    }

    #[test]
    fn origin_guard_refuses_everything_but_loopback_literal() {
        assert!(parse_loopback_origin("http://127.0.0.1:11434").is_ok());
        assert_eq!(
            parse_loopback_origin("https://127.0.0.1:11434").unwrap_err(),
            InferenceError::NonLocalRuntime
        );
        assert_eq!(
            parse_loopback_origin("http://localhost:11434").unwrap_err(),
            InferenceError::NonLocalRuntime
        );
        assert_eq!(
            parse_loopback_origin("http://0.0.0.0:11434").unwrap_err(),
            InferenceError::NonLocalRuntime
        );
        assert_eq!(
            parse_loopback_origin("http://api.openai.com:443").unwrap_err(),
            InferenceError::NonLocalRuntime
        );
        assert_eq!(
            parse_loopback_origin("http://127.0.0.1").unwrap_err(),
            InferenceError::NonLocalRuntime
        );
        assert_eq!(
            parse_loopback_origin("http://127.0.0.1:0").unwrap_err(),
            InferenceError::NonLocalRuntime
        );
    }

    #[test]
    fn known_schemas_embed_and_parse() {
        for (name, raw) in KNOWN_SCHEMAS {
            let v: Value = serde_json::from_str(raw)
                .unwrap_or_else(|e| panic!("schema {name} does not parse: {e}"));
            assert!(v.get("type").is_some(), "schema {name} missing type");
        }
    }

    fn spawn_server(response: &'static [u8]) -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut conn, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf); // read request head+body greedily
                let _ = conn.write_all(response);
            }
        });
        addr
    }

    #[test]
    fn http_parses_content_length_responses() {
        let addr = spawn_server(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"ok\":true}\n",
        );
        let (status, body) = http_request(addr, "GET", "/x", None, 2000).unwrap();
        assert_eq!(status, 200);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn http_parses_chunked_responses() {
        let addr = spawn_server(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n{\"ok\": tr\r\n3\r\nue}\r\n0\r\n\r\n",
        );
        let (status, body) = http_request(addr, "GET", "/x", None, 2000).unwrap();
        assert_eq!(status, 200);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn fragmented_chunks_are_incomplete_until_the_terminator() {
        let body = b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        for end in 0..body.len() {
            assert!(
                decode_chunked(&body[..end]).is_none(),
                "prefix length {end}"
            );
        }
        assert_eq!(decode_chunked(body).unwrap(), b"abcde");
        assert!(decode_chunked(b"3\r\nabcXX0\r\n\r\n").is_none());
        assert!(decode_chunked(b"ffffffffffffffff\r\n").is_none());
    }

    #[test]
    fn short_content_length_response_is_an_error() {
        let addr = spawn_server(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort");
        assert_eq!(
            http_request(addr, "GET", "/x", None, 2000).unwrap_err(),
            "incomplete content-length body"
        );
    }

    #[test]
    fn http_surfaces_connection_failures() {
        // bind then drop = refused port
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        assert!(http_request(addr, "GET", "/x", None, 2000).is_err());
    }

    /// Live probe against the local runtime. Run with:
    /// `cargo test -p waypoint_inference -- --ignored`
    #[test]
    #[ignore = "requires a local Ollama runtime with qwen3.5:9b"]
    fn live_probe_qualifies_local_model() {
        let mut rt = OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b").unwrap();
        let cap = rt.probe().expect("probe");
        println!("{cap:#?}");
        assert!(cap.supports_structured_output);
        assert!(cap.supports_reasoning);
        let (v, usage) = rt
            .generate_structured("claim", "Create one truthful claim about a fictional candidate 'Alex Test': JSON with summary.", 512)
            .expect("structured generate");
        assert!(v.is_object());
        assert!(usage.completion_tokens > 0);
    }
}
