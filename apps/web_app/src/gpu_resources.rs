//! Read-only GPU memory observations. A sample does not reserve memory.
use serde_json::{json, Value};

#[cfg(any(windows, test))]
fn report(adapters: Vec<Value>, reported: usize) -> Value {
    let complete = reported == adapters.len()
        && !adapters.is_empty()
        && adapters.iter().all(|adapter| {
            let total = adapter["total_bytes"].as_u64();
            let used = adapter["used_bytes"].as_u64();
            matches!((total, used), (Some(total), Some(used)) if total > 0 && used <= total)
        });
    let state = if adapters.is_empty() {
        "unavailable"
    } else if complete {
        "sampled"
    } else {
        "incomplete"
    };
    json!({"state":state,"reported_adapters":reported,"adapters":adapters})
}

#[cfg(windows)]
pub fn sample() -> Value {
    use gpuviewer_core::{wddm::WddmBackend, GpuBackend};
    let Ok(mut backend) = WddmBackend::init() else {
        return report(vec![], 0);
    };
    let devices = backend.devices();
    let reported = devices.len();
    let mut adapters = Vec::new();
    for device in devices.iter().take(16) {
        let info = backend.static_info(device).ok();
        let dynamic = backend.refresh_dynamic(device).ok();
        let name = info
            .as_ref()
            .map(|value| value.name.chars().take(200).collect::<String>());
        adapters.push(json!({"name":name,"total_bytes":info.and_then(|value| value.mem_total_bytes),"used_bytes":dynamic.and_then(|value| value.mem_used_bytes)}));
    }
    report(adapters, reported)
}

#[cfg(not(windows))]
pub fn sample() -> Value {
    json!({"state":"unsupported","reported_adapters":0,"adapters":[]})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_inconsistent_memory_is_not_reported_complete() {
        assert_eq!(report(vec![], 0)["state"], "unavailable");
        assert_eq!(
            report(vec![json!({"total_bytes":100,"used_bytes":40})], 1)["state"],
            "sampled"
        );
        assert_eq!(
            report(vec![json!({"total_bytes":100,"used_bytes":null})], 1)["state"],
            "incomplete"
        );
        assert_eq!(
            report(vec![json!({"total_bytes":100,"used_bytes":101})], 1)["state"],
            "incomplete"
        );
        assert_eq!(
            report(vec![json!({"total_bytes":100,"used_bytes":40})], 2)["state"],
            "incomplete"
        );
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires a Windows host with GPU performance counters"]
    fn windows_gpu_sample_has_observed_memory() {
        let result = sample();
        assert_eq!(
            result["state"], "sampled",
            "GPU memory unavailable: {result}"
        );
        let adapters = result["adapters"].as_array().unwrap();
        assert!(adapters
            .iter()
            .any(|adapter| adapter["total_bytes"].as_u64().unwrap_or(0) > 0));
        if let Ok(expected) = std::env::var("WAYPOINT_GPU_EXPECTED_USED_BYTES") {
            let expected: u64 = expected.parse().unwrap();
            assert_eq!(adapters.len(), 1, "comparison requires a single adapter");
            let used = adapters[0]["used_bytes"].as_u64().unwrap();
            assert!(
                used.abs_diff(expected) <= 1024 * 1024 * 1024,
                "independent GPU samples differ by more than 1 GiB"
            );
        }
    }
}
