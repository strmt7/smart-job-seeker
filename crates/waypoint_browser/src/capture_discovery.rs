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
    pub preload_projection_failures: usize,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Script,
    Stylesheet,
    Font,
    Image,
    Json,
    Manifest,
}

#[cfg(any(windows, test))]
#[derive(serde::Deserialize)]
pub(crate) struct Probe {
    schema: u8,
    mode: String,
    pub urls: Vec<String>,
    #[serde(default)]
    pub child_documents: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub assets: std::collections::BTreeMap<String, std::collections::BTreeSet<AssetKind>>,
    #[serde(default)]
    pub asset_request_origins: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub asset_origin_absent_stylesheets: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub entry_request_accept: Option<String>,
    #[serde(default)]
    pub entry_document_requests: u64,
    #[serde(default)]
    pub preload_projection_failures: usize,
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
            && self.assets.len() <= self.urls.len()
            && self.assets.iter().all(|(url, kinds)| {
                self.urls.contains(url) && !kinds.is_empty() && kinds.len() <= 6
            })
            && self.asset_request_origins.len() <= self.assets.len()
            && self.asset_request_origins.iter().all(|(url, origin)| {
                self.assets.contains_key(url)
                    && waypoint_net::PublicRequestOrigin::parse(origin).is_ok()
            })
            && self.asset_origin_absent_stylesheets.len() <= self.assets.len()
            && self.asset_origin_absent_stylesheets.iter().all(|url| {
                !self.asset_request_origins.contains_key(url)
                    && self.assets.get(url).is_some_and(|kinds| {
                        kinds.len() == 1 && kinds.contains(&AssetKind::Stylesheet)
                    })
            })
            && self.preload_projection_failures <= 512
            && self.entry_document_requests <= 128
            && self
                .entry_request_accept
                .as_ref()
                .is_none_or(|value| waypoint_net::PublicDocumentAccept::parse(value).is_ok())
            && self.pending_commands <= 128
            && self.unsupported_targets.len() <= 16
            && self
                .form_outline
                .as_ref()
                .is_none_or(PublicFormOutline::valid)
    }
}

#[cfg(windows)]
pub fn document_request_accept(
    python: &std::path::Path,
    config: crate::rehearsal::RehearsalConfig,
    root: &std::path::Path,
) -> Result<waypoint_net::PublicDocumentAccept, String> {
    // This entirely owned sentinel has no script, form, person or live source.
    // The same zero-capability renderer reveals its native navigation Accept;
    // no public URL is fetched to obtain that request preference.
    let url = "https://smart-jobseeker.invalid/document-request";
    let body = "<!doctype html><meta charset=utf-8><link rel=icon href=data:,><title>Local request check</title>";
    let raw = format!(
        "HTTP/1.1 200 Fixture\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let response =
        waypoint_net::read_asset_response(&mut std::io::Cursor::new(raw.into_bytes()), "GET", 4096)
            .map_err(|_| "Cannot create controlled request check")?;
    let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(root, url)?;
    builder.add(url, response)?;
    let snapshot = builder.finish()?;
    let engine_files_sha256 = waypoint_domain::sha256_hex(
        &serde_json::to_vec(&config.engine_files)
            .map_err(|_| "Cannot bind request-check engine")?,
    );
    let manifest_sha256 = snapshot
        .files
        .get(&snapshot.manifest)
        .cloned()
        .ok_or("Request-check manifest identity is unavailable")?;
    let probe = render(python, config, snapshot, 250)?;
    if probe.entry_document_requests != 1
        || probe.unsupported() != 0
        || probe.unreplayed_transports != 0
        || !probe.urls.is_empty()
        || probe.pending_commands != 0
    {
        return Err("Controlled document request observation is incomplete".into());
    }
    let accept = probe
        .entry_request_accept
        .ok_or("Controlled document Accept is unavailable")?;
    let value = waypoint_net::PublicDocumentAccept::parse(&accept)
        .map_err(|_| "Controlled document Accept is invalid")?;
    crate::rehearsal_capture::write_new(root, "request-observation.json", &serde_json::to_vec(&serde_json::json!({"schema":1,"source":"owned_scriptless_sentinel","entry_document_requests":1,"request_accept":accept,"engine_files_sha256":engine_files_sha256,"sentinel_manifest_sha256":manifest_sha256,"external_delivery":"not_performed"})).map_err(|_| "Cannot encode request observation")?)?;
    Ok(value)
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
    #[test]
    fn asset_observations_require_a_bounded_typed_subset_of_the_frontier() {
        let value = json!({"schema":1,"mode":"resource_discovery","urls":["https://cdn.fixture.invalid/a.js"],
            "assets":{"https://cdn.fixture.invalid/a.js":["script"]},"omitted_requests":0,"ineligible_requests":0,
            "pending_commands":0,"unsupported_targets":{},"unreplayed_transports":0,"unmanaged_child_sessions":0});
        assert!(serde_json::from_value::<Probe>(value.clone())
            .unwrap()
            .valid());
        for assets in [
            json!({"https://other.fixture.invalid/unobserved.js":["script"]}),
            json!({"https://cdn.fixture.invalid/a.js":[]}),
        ] {
            let mut candidate = value.clone();
            candidate["assets"] = assets;
            assert!(!serde_json::from_value::<Probe>(candidate).unwrap().valid());
        }
        let mut candidate = value;
        candidate["assets"]["https://cdn.fixture.invalid/a.js"] = json!(["document"]);
        assert!(serde_json::from_value::<Probe>(candidate).is_err());
    }
    #[test]
    fn absent_origin_style_proof_cannot_bind_an_unobserved_or_conflicting_kind() {
        let url = "https://cdn.fixture.invalid/style.css";
        let base = json!({"schema":1,"mode":"resource_discovery","urls":[url],
            "assets":{url:["stylesheet"]},"asset_origin_absent_stylesheets":[url],
            "omitted_requests":0,"ineligible_requests":0,"pending_commands":0,
            "unsupported_targets":{},"unreplayed_transports":0,"unmanaged_child_sessions":0});
        assert!(serde_json::from_value::<Probe>(base.clone())
            .unwrap()
            .valid());
        for kinds in [
            json!([]),
            json!(["script"]),
            json!(["stylesheet", "script"]),
        ] {
            let mut value = base.clone();
            value["assets"][url] = kinds;
            assert!(!serde_json::from_value::<Probe>(value).unwrap().valid());
        }
        let mut value = base.clone();
        value["asset_request_origins"] = json!({url:"https://fixture.invalid"});
        assert!(!serde_json::from_value::<Probe>(value).unwrap().valid());
        let mut value = base;
        value["asset_origin_absent_stylesheets"] = json!(["https://unobserved.invalid/style.css"]);
        assert!(!serde_json::from_value::<Probe>(value).unwrap().valid());
    }

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
