//! Render-only resource discovery; no candidate-entry driver or live fallback.
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicControl {
    pub number: usize,
    pub role: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub context: Vec<String>,
    pub context_incomplete: bool,
    pub required: Option<bool>,
    pub disabled: Option<bool>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicFormOutline {
    schema: u8,
    scope: String,
    pub status: String,
    reason: Option<String>,
    nodes: usize,
    pub controls: Vec<PublicControl>,
    omitted_controls: usize,
    unlabelled_controls: usize,
    omitted_texts: usize,
}
impl PublicFormOutline {
    pub fn valid(&self) -> bool {
        let text = |value: &str, limit| {
            !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
        };
        self.schema == 1
            && self.scope == "root_frame_accessibility_tree"
            && matches!(self.status.as_str(), "observed" | "partial" | "unavailable")
            && self.nodes <= 16384
            && self.controls.len() <= 256
            && self.omitted_controls <= 16384
            && self.unlabelled_controls <= 256
            && self.omitted_texts <= 32768
            && self.reason.as_ref().is_none_or(|s| text(s, 64))
            && (self.status != "unavailable" || self.controls.is_empty())
            && (self.status == "unavailable"
                || self.nodes > 0
                    && self.reason.is_none()
                    && self.unlabelled_controls
                        == self.controls.iter().filter(|c| c.label.is_none()).count())
            && (self.status != "observed"
                || self.omitted_controls == 0
                    && self.omitted_texts == 0
                    && self.unlabelled_controls == 0
                    && self.controls.iter().all(|c| !c.context_incomplete))
            && self.controls.iter().enumerate().all(|(index, c)| {
                c.number == index + 1
                    && matches!(
                        c.role.as_str(),
                        "textbox"
                            | "searchbox"
                            | "combobox"
                            | "listbox"
                            | "checkbox"
                            | "radio"
                            | "switch"
                            | "spinbutton"
                            | "slider"
                            | "button"
                            | "link"
                    )
                    && c.label.as_ref().is_none_or(|s| text(s, 400))
                    && c.description.as_ref().is_none_or(|s| text(s, 600))
                    && c.context.len() <= 3
                    && c.context.iter().all(|s| text(s, 400))
            })
            && serde_json::to_vec(&self.controls).is_ok_and(|bytes| bytes.len() <= 65536)
    }
}

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
    pub form_outline: Option<PublicFormOutline>,
    pub form_outline_snapshot_sha256: Option<String>,
    pub form_outline_round: Option<u8>,
}

#[cfg(any(windows, test))]
#[derive(serde::Deserialize)]
pub(crate) struct Probe {
    schema: u8,
    mode: String,
    pub urls: Vec<String>,
    #[serde(default)]
    pub child_documents: std::collections::BTreeSet<String>,
    pub omitted_requests: u64,
    pub ineligible_requests: u64,
    pub pending_commands: u64,
    unsupported_targets: std::collections::BTreeMap<String, u64>,
    pub unreplayed_transports: u64,
    unmanaged_child_sessions: u64,
    #[serde(default)]
    pub form_outline: Option<PublicFormOutline>,
    #[serde(skip_deserializing)]
    pub outline_snapshot_sha256: Option<String>,
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
            && self.child_documents.len() <= self.urls.len()
            && self
                .child_documents
                .iter()
                .all(|url| self.urls.contains(url))
            && self.pending_commands <= 128
            && self.unsupported_targets.len() <= 16
            && self
                .form_outline
                .as_ref()
                .is_none_or(PublicFormOutline::valid)
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
    let outline_snapshot_sha256 = waypoint_domain::sha256_hex(
        &serde_json::to_vec(&config.content_files)
            .map_err(|_| "Cannot bind public form outline")?,
    );
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
            cdp.call("SmartJobseeker.publicFormOutline", json!({}))
                .map_err(|e| e.to_string())?;
            let value = cdp
                .call_browser("SmartJobseeker.discoveryReport", json!({}))
                .map_err(|e| e.to_string())?;
            let mut report: Probe =
                serde_json::from_value(value).map_err(|_| "Invalid resource discovery report")?;
            if !report.valid() {
                return Err("Resource discovery report exceeded its contract".into());
            }
            report.outline_snapshot_sha256 = Some(outline_snapshot_sha256);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> serde_json::Value {
        json!({"schema":1,"scope":"root_frame_accessibility_tree","status":"observed","reason":null,"nodes":2,"controls":[{"number":1,"role":"textbox","label":"Name","description":null,"context":[],"context_incomplete":false,"required":null,"disabled":null}],"omitted_controls":0,"unlabelled_controls":0,"omitted_texts":0})
    }
    #[test]
    fn public_outline_accepts_explicit_unknown_flags_and_refuses_value_fields() {
        let value = fixture();
        let outline: PublicFormOutline = serde_json::from_value(value.clone()).unwrap();
        assert!(outline.valid());
        assert_eq!(outline.controls[0].required, None);
        let mut injected = value;
        injected["controls"][0]["value"] = json!("private");
        assert!(serde_json::from_value::<PublicFormOutline>(injected).is_err());
    }
    #[test]
    fn public_outline_bounds_and_missing_labels_cannot_claim_complete_observation() {
        let base = fixture();
        for (pointer, value) in [
            ("/nodes", json!(16385)),
            ("/controls/0/number", json!(0)),
            ("/controls/0/label", json!("a".repeat(401))),
            ("/omitted_controls", json!(1)),
            ("/controls/0/context_incomplete", json!(true)),
        ] {
            let mut candidate = base.clone();
            *candidate.pointer_mut(pointer).unwrap() = value;
            assert!(
                !serde_json::from_value::<PublicFormOutline>(candidate)
                    .unwrap()
                    .valid(),
                "{pointer}"
            );
        }
        let mut partial = base;
        partial["status"] = json!("partial");
        partial["controls"][0]["label"] = serde_json::Value::Null;
        partial["unlabelled_controls"] = json!(1);
        assert!(serde_json::from_value::<PublicFormOutline>(partial)
            .unwrap()
            .valid());
    }
}
