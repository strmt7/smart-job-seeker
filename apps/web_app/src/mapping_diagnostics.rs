//! Bounded private native-response evidence for saved-copy contact planning.
//! No prompt, context tokens, request headers or unknown runtime fields are kept.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
const RECORDS: usize = 16;
const RESPONSE_BYTES: usize = 8192;
const SERIALIZED_BYTES: usize = 64 * 1024;
#[derive(Default)]
struct State {
    records: Vec<Value>,
    bytes: usize,
    omitted: u64,
}
#[derive(Clone, Default)]
pub(crate) struct Capture(Arc<Mutex<State>>);
impl Capture {
    pub(crate) fn observe(&self, schema: &str, native: &Value) {
        let Ok(mut state) = self.0.lock() else { return };
        if state.records.len() == RECORDS {
            state.omitted = state.omitted.saturating_add(1);
            return;
        }
        let response = native["response"].as_str();
        let prefix = response.map(|s| {
            let mut end = s.len().min(RESPONSE_BYTES);
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            &s[..end]
        });
        let mut record = json!({"schema":schema.chars().take(96).collect::<String>(),"response":prefix,
            "response_bytes":response.map(str::len),"response_sha256":response.map(|s|waypoint_domain::sha256_hex(s.as_bytes())),
            "response_truncated":response.is_some_and(|s|s.len()>prefix.unwrap_or("").len()),
            "done":native["done"].as_bool(),"done_reason":native["done_reason"].as_str().map(|s|s.chars().take(64).collect::<String>()),
            "thinking_bytes":native["thinking"].as_str().map(str::len)});
        for key in [
            "eval_count",
            "prompt_eval_count",
            "total_duration",
            "load_duration",
            "eval_duration",
            "prompt_eval_duration",
        ] {
            record[key] = json!(native[key].as_u64());
        }
        let mut bytes = record.to_string().len();
        if bytes > SERIALIZED_BYTES - state.bytes {
            record["response"] = Value::Null;
            record["response_truncated"] = json!(response.is_some());
            record["response_omitted"] = json!("serialized_budget");
            bytes = record.to_string().len();
        }
        if bytes > SERIALIZED_BYTES - state.bytes {
            state.omitted = state.omitted.saturating_add(1);
            return;
        }
        state.bytes += bytes;
        state.records.push(record);
    }
    pub(crate) fn finish(&self) -> Value {
        match self.0.lock() {
            Ok(state) => {
                json!({"status":"retained","scope":"native_generation_without_prompt_or_context","records":state.records,"omitted_records":state.omitted,"record_limit":RECORDS,"response_byte_limit":RESPONSE_BYTES,"serialized_records_byte_limit":SERIALIZED_BYTES})
            }
            Err(_) => json!({"status":"unavailable","reason":"diagnostic_lock_unavailable"}),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_response_and_completion_are_retained_without_private_context() {
        let capture = Capture::default();
        let native = json!({"response":"{broken","done":true,"done_reason":"length","eval_count":512,"thinking":"private internal text","context":[1,2,3],"prompt":"private prompt","headers":{"secret":"must not retain"}});
        capture.observe("form_mapping", &native);
        let value = capture.finish();
        let row = &value["records"][0];
        assert_eq!(row["response"], "{broken");
        assert_eq!(row["done_reason"], "length");
        assert_eq!(row["eval_count"], 512);
        assert_eq!(
            row["response_sha256"],
            waypoint_domain::sha256_hex(b"{broken")
        );
        for secret in [
            "private internal text",
            "private prompt",
            "must not retain",
            "context",
        ] {
            assert!(!value["records"].to_string().contains(secret));
        }
    }
    #[test]
    fn diagnostic_bytes_records_and_utf8_are_bounded_without_changing_native_input() {
        let capture = Capture::default();
        let native = json!({"response":"é".repeat(12000),"done":false});
        let original = native.clone();
        for _ in 0..RECORDS + 3 {
            capture.observe("form_mapping", &native);
        }
        let result = capture.finish();
        let rows = result["records"].as_array().unwrap();
        assert!(!rows.is_empty() && rows.len() <= RECORDS);
        assert_eq!(
            rows.len() as u64 + result["omitted_records"].as_u64().unwrap(),
            (RECORDS + 3) as u64
        );
        assert_eq!(native, original);
        assert!(rows.iter().all(|r| r["response_truncated"] == true));
        assert!(rows
            .iter()
            .filter_map(|r| r["response"].as_str())
            .all(|s| s.len() <= RESPONSE_BYTES));
        assert!(rows.iter().map(|r| r.to_string().len()).sum::<usize>() <= SERIALIZED_BYTES);
        assert!(rows
            .iter()
            .any(|r| r["response_omitted"] == "serialized_budget"));
    }
}
