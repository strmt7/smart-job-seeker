//! Render-only resource discovery; no candidate-entry driver or live fallback.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStop {
    NoNewRequests,
    ReviewRequired,
    RoundLimit,
    CancelledOrExpired,
    BrowserFailed,
    CheckpointFailed,
}
#[derive(Clone, Serialize)]
pub struct DiscoveryRound {
    pub proposed: usize,
    pub newly_recorded: usize,
    pub review_outcomes: usize,
    pub omitted_requests: u64,
    pub ineligible_requests: u64,
    pub pending_commands: u64,
    pub unsupported_targets: u64,
    pub unreplayed_transports: u64,
}
#[derive(Clone, Serialize)]
pub struct DiscoverySummary {
    pub rounds: Vec<DiscoveryRound>,
    pub stopped: DiscoveryStop,
    pub coverage: &'static str,
}

#[cfg(any(windows, test))]
#[derive(serde::Deserialize)]
pub(crate) struct Probe {
    schema: u8,
    mode: String,
    pub urls: Vec<String>,
    pub omitted_requests: u64,
    pub ineligible_requests: u64,
    pub pending_commands: u64,
    unsupported_targets: std::collections::BTreeMap<String, u64>,
    pub unreplayed_transports: u64,
    unmanaged_child_sessions: u64,
}
#[cfg(any(windows, test))]
impl Probe {
    pub fn unsupported(&self) -> u64 {
        self.unsupported_targets
            .values()
            .fold(self.unmanaged_child_sessions, |a, b| a.saturating_add(*b))
    }
    pub(crate) fn valid(&self) -> bool {
        self.schema == 1
            && self.mode == "resource_discovery"
            && self.urls.len() <= 128
            && self.urls.iter().all(|u| u.len() <= 4096)
            && self.urls.iter().map(String::len).sum::<usize>() <= 128 * 1024
            && self.pending_commands <= 128
            && self.unsupported_targets.len() <= 16
    }
}

#[cfg(windows)]
pub(crate) fn render(
    python: &std::path::Path,
    mut config: crate::rehearsal::RehearsalConfig,
    snapshot: crate::rehearsal_capture::Snapshot,
    settle_ms: u64,
) -> Result<Probe, String> {
    use crate::{cdp::Cdp, rehearsal::RehearsalOwner};
    use serde_json::json;
    use std::time::{Duration, Instant};
    if !(250..=3000).contains(&settle_ms) {
        return Err("Invalid discovery settling budget".into());
    }
    config.schema = 3;
    config.seconds = config.seconds.clamp(5, 30);
    config.content_root = snapshot.root;
    config.content_files = snapshot.files;
    config.entry = snapshot.entry;
    config.replay_manifest = Some(snapshot.manifest);
    let (mut owner, pipe, ready) =
        RehearsalOwner::start(python, config).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut cdp = Cdp::new_rehearsal(pipe).map_err(|e| e.to_string())?;
        let result = (|| {
            cdp.call("Page.enable", json!({}))
                .map_err(|e| e.to_string())?;
            cdp.call("Network.enable", json!({}))
                .map_err(|e| e.to_string())?;
            let navigation = cdp
                .call("Page.navigate", json!({"url":ready.entry_url}))
                .map_err(|e| e.to_string())?;
            if navigation
                .get("errorText")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| !s.is_empty())
                || navigation["isDownload"] == true
            {
                return Err("Discovery entry navigation failed".into());
            }
            let deadline = Instant::now() + Duration::from_millis(settle_ms);
            while Instant::now() < deadline {
                cdp.pump_checked(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(100)),
                )
                .map_err(|e| e.to_string())?;
            }
            let value = cdp
                .call_browser("SmartJobseeker.discoveryReport", json!({}))
                .map_err(|e| e.to_string())?;
            let report: Probe =
                serde_json::from_value(value).map_err(|_| "Invalid resource discovery report")?;
            if !report.valid() {
                return Err("Resource discovery report exceeded its contract".into());
            }
            Ok(report)
        })();
        // The entire discovery browser must close before any proposed URL can
        // reach the native fetch coordinator. A cleanup error aborts discovery.
        let closed = owner.shutdown().map_err(|e| e.to_string());
        drop(cdp);
        closed?;
        result
    })();
    if result.is_err() {
        let _ = owner.shutdown();
    }
    result
}
