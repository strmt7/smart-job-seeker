//! Build an offline replay archive from already-obtained public resource bytes.
//! No transport, browser action, permission decision, cookies or live fallback.
use serde_json::{json, Value};
#[cfg(any(windows, test))]
use std::io::Read;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use waypoint_net::{BodyFraming, CapturedResponse};

const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_TOTAL: usize = 64 * 1024 * 1024;

pub struct SnapshotBuilder {
    root: PathBuf,
    entry_url: String,
    resources: Vec<Value>,
    files: BTreeMap<String, String>,
    urls: BTreeSet<String>,
    total: usize,
    warnings: BTreeSet<&'static str>,
    failed: bool,
}
pub struct Snapshot {
    pub root: PathBuf,
    pub files: BTreeMap<String, String>,
    pub entry: String,
    pub manifest: String,
    pub warnings: Vec<String>,
}
fn canonical(raw: &str, entry: bool) -> Result<String, String> {
    if raw.len() > 4096 || raw.chars().any(|c| c.is_control()) {
        return Err("Invalid snapshot URL".into());
    }
    waypoint_net::parse_url(raw).map_err(|_| "Invalid snapshot URL")?;
    let url = url::Url::parse(raw).map_err(|_| "Invalid snapshot URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !entry && url.fragment().is_some()
    {
        return Err("Snapshot resources require credential-free HTTPS URLs".into());
    }
    if url.as_str() != raw {
        return Err("Snapshot URL is not browser-canonical".into());
    }
    Ok(url.to_string())
}
/// Native request context needed to match a supported response variant.
#[derive(Default)]
pub(super) struct SnapshotRequestBinding {
    origin: bool,
    preflight: bool,
    stylesheet: bool,
    accept: bool,
    requested_with_absent: bool,
}
#[derive(Clone, Copy, Default)]
pub(super) enum SnapshotScope {
    #[default]
    Ordinary,
    OriginAbsentStylesheet,
}
pub(super) fn snapshot_origin_binding(
    response: &CapturedResponse,
) -> Result<SnapshotRequestBinding, String> {
    snapshot_binding_scoped(response, SnapshotScope::Ordinary)
}
pub(super) fn snapshot_binding_scoped(
    response: &CapturedResponse,
    scope: SnapshotScope,
) -> Result<SnapshotRequestBinding, String> {
    let names = response.request_header_names.as_ref().and_then(|names| {
        let mut unique = BTreeSet::new();
        (!names.is_empty()
            && names.len() <= 128
            && names.iter().all(|name| {
                !name.is_empty()
                    && name.len() <= 256
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&c))
                    && unique.insert(name.to_ascii_lowercase())
            })
            && unique.contains("host"))
        .then_some(unique)
    });
    let origin_absent = response.request_origin.is_none()
        && names
            .as_ref()
            .is_some_and(|names| !names.contains("origin"));
    let origin_present = response.request_origin.is_some()
        && names.as_ref().is_some_and(|names| names.contains("origin"));
    if response.request_origin.is_some() && (!origin_present || response.request_method != "GET") {
        return Err("Recorded request Origin lacks native GET header evidence".into());
    }
    let preflight_absent = response.request_method == "GET"
        && names.as_ref().is_some_and(|names| {
            !names.contains("access-control-request-headers")
                && !names.contains("access-control-request-method")
        });
    let stylesheet = matches!(scope, SnapshotScope::OriginAbsentStylesheet);
    let mime: Vec<_> = response
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("content-type"))
        .collect();
    let html = mime.len() == 1
        && std::str::from_utf8(&mime[0].value).is_ok_and(|value| {
            matches!(
                value
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
                    .as_str(),
                "text/html" | "application/xhtml+xml"
            )
        });
    let redirect = matches!(response.status, 301 | 302 | 303 | 307 | 308);
    let anonymous_navigation = !stylesheet
        && response.request_method == "GET"
        && origin_absent
        && preflight_absent
        && (html || redirect)
        && names.as_ref().is_some_and(|names| {
            !names.iter().any(|name| {
                matches!(
                    name.as_str(),
                    "cookie"
                        | "authorization"
                        | "proxy-authorization"
                        | "content-length"
                        | "transfer-encoding"
                        | "content-type"
                )
            })
        });
    if stylesheet {
        let mime: Vec<_> = response
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("content-type"))
            .collect();
        let css = mime.len() == 1
            && std::str::from_utf8(&mime[0].value).is_ok_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/css"))
            });
        let anonymous = names.as_ref().is_some_and(|names| {
            !names.iter().any(|name| {
                matches!(
                    name.as_str(),
                    "cookie" | "authorization" | "proxy-authorization"
                )
            })
        });
        if response.request_method != "GET"
            || !origin_absent
            || !preflight_absent
            || !anonymous
            || !(css || matches!(response.status, 301 | 302 | 303 | 307 | 308))
        {
            return Err("Absent-Origin stylesheet requires observed style scope and native anonymous GET evidence".into());
        }
    }
    if response.headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case("connection")
            && std::str::from_utf8(&h.value).is_ok_and(|value| {
                value
                    .split(',')
                    .any(|v| v.trim().eq_ignore_ascii_case("vary"))
            })
    }) {
        return Ok(SnapshotRequestBinding {
            origin: stylesheet,
            stylesheet,
            ..Default::default()
        });
    }
    let mut binding = SnapshotRequestBinding {
        origin: stylesheet,
        stylesheet,
        ..Default::default()
    };
    for header in response
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("vary"))
    {
        let value = std::str::from_utf8(&header.value)
            .map_err(|_| "Header-dependent snapshot representation needs review")?;
        for field in value.split(',').map(str::trim) {
            if field.eq_ignore_ascii_case("accept-encoding") {
                continue;
            }
            if field.eq_ignore_ascii_case("accept")
                && anonymous_navigation
                && response
                    .request_accept
                    .as_ref()
                    .is_some_and(|value| waypoint_net::PublicDocumentAccept::parse(value).is_ok())
                && names.as_ref().is_some_and(|names| names.contains("accept"))
            {
                binding.accept = true;
                binding.origin = true;
                continue;
            }
            if field.eq_ignore_ascii_case("x-requested-with")
                && anonymous_navigation
                && names
                    .as_ref()
                    .is_some_and(|names| !names.contains("x-requested-with"))
            {
                binding.requested_with_absent = true;
                binding.origin = true;
                continue;
            }
            if field.eq_ignore_ascii_case("origin") && (origin_absent || origin_present) {
                binding.origin = true;
            } else if preflight_absent
                && (field.eq_ignore_ascii_case("access-control-request-headers")
                    || field.eq_ignore_ascii_case("access-control-request-method"))
            {
                binding.preflight = true;
            } else {
                return Err("Header-dependent snapshot representation needs review".into());
            }
        }
    }
    if binding.origin
        && !origin_present
        && !stylesheet
        && (response.request_method != "GET"
            || !(html || redirect && (binding.accept || binding.requested_with_absent)))
    {
        return Err("Absent Origin snapshots require HTML navigation resources".into());
    }
    Ok(binding)
}
pub(super) fn write_new(root: &Path, name: &str, bytes: &[u8]) -> Result<String, String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .map_err(|_| "Cannot create snapshot resource")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot save snapshot resource")?;
    Ok(waypoint_domain::sha256_hex(bytes))
}
// Existing binding predicates must be provably disjoint. Unbound responses
// cannot be made more specific retrospectively or chosen by list order.
fn variants_disjoint(left: &Value, right: &Value) -> bool {
    let origin = |item: &Value| {
        item["request_origin"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| (item["request_origin_absent"] == true).then(String::new))
    };
    let kind = |item: &Value| {
        if item["request_origin_absent_stylesheet"] == true {
            Some("Stylesheet")
        } else if item["request_origin_absent"] == true {
            Some("Document")
        } else {
            None
        }
    };
    origin(left).zip(origin(right)).is_some_and(|(a, b)| a != b)
        || kind(left).zip(kind(right)).is_some_and(|(a, b)| a != b)
        || left["request_accept"]
            .as_str()
            .zip(right["request_accept"].as_str())
            .is_some_and(|(a, b)| a != b)
}

impl SnapshotBuilder {
    /// An immutable rendering checkpoint while the anonymous collector remains
    /// active. Only the render-only worker may consume these checkpoints.
    #[cfg(any(windows, test))]
    pub(super) fn discovery_snapshot(&self, destination: &Path) -> Result<Snapshot, String> {
        if self.failed || !self.files.contains_key("index.html") {
            return Err("Cannot discover resources from an incomplete entry".into());
        }
        fs::create_dir(destination).map_err(|_| "Discovery requires a new checkpoint directory")?;
        for (name, expected) in &self.files {
            let source = self.root.join(name);
            let metadata =
                fs::symlink_metadata(&source).map_err(|_| "Cannot inspect checkpoint resource")?;
            if !metadata.file_type().is_file() || metadata.len() > MAX_BODY as u64 {
                return Err("Checkpoint source is not a bounded regular resource".into());
            }
            let mut bytes = Vec::new();
            fs::File::open(source)
                .map_err(|_| "Cannot read checkpoint resource")?
                .take(MAX_BODY as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read checkpoint resource")?;
            if bytes.len() > MAX_BODY || waypoint_domain::sha256_hex(&bytes) != *expected {
                return Err("Checkpoint source changed".into());
            }
            write_new(destination, name, &bytes)?;
        }
        Self {
            root: destination.to_owned(),
            entry_url: self.entry_url.clone(),
            resources: self.resources.clone(),
            files: self.files.clone(),
            urls: self.urls.clone(),
            total: self.total,
            warnings: self.warnings.clone(),
            failed: false,
        }
        .finish()
    }
    /// Resolve the initial public redirect chain before recording any resources.
    pub(super) fn retarget_entry(&mut self, entry_url: &str) -> Result<(), String> {
        if self.failed || !self.resources.is_empty() {
            return Err("Snapshot entry is already fixed".into());
        }
        self.entry_url = canonical(entry_url, true)?;
        Ok(())
    }
    pub fn start(root: &Path, entry_url: &str) -> Result<Self, String> {
        let entry_url = canonical(entry_url, true)?;
        fs::create_dir(root).map_err(|_| "Snapshot requires a new destination directory")?;
        let mut files = BTreeMap::new();
        let bytes=serde_json::to_vec_pretty(&json!({"schema":1,"state":"started","note":"Only a valid replay.json and final capture-report.json establish archive completion. No source permission or server validation is implied."})).map_err(|_|"Cannot encode capture state")?;
        files.insert(
            "capture-started.json".into(),
            write_new(root, "capture-started.json", &bytes)?,
        );
        Ok(Self {
            root: root.to_owned(),
            entry_url,
            resources: Vec::new(),
            files,
            urls: BTreeSet::new(),
            total: 0,
            warnings: BTreeSet::new(),
            failed: false,
        })
    }
    /// Only GET representations with complete framing enter this archive format.
    /// Callers must separately establish source permissions and keep capture
    /// completely separate from candidate entry and authenticated sessions.
    pub fn add(&mut self, resource_url: &str, response: CapturedResponse) -> Result<(), String> {
        self.add_scoped(resource_url, response, SnapshotScope::Ordinary)
    }
    pub(super) fn add_scoped(
        &mut self,
        resource_url: &str,
        response: CapturedResponse,
        scope: SnapshotScope,
    ) -> Result<(), String> {
        if self.failed {
            return Err("Snapshot has failed; start a new capture".into());
        }
        let result = self.add_response(resource_url, response, scope);
        if let Err(error) = &result {
            self.failed = true;
            let _ = write_new(
                &self.root,
                "capture-failure.json",
                &serde_json::to_vec_pretty(
                    &json!({"schema":1,"state":"incomplete","reason":error}),
                )
                .map_err(|_| "Cannot encode capture failure")?,
            );
        }
        result
    }
    fn add_response(
        &mut self,
        resource_url: &str,
        response: CapturedResponse,
        scope: SnapshotScope,
    ) -> Result<(), String> {
        if response.request_method != "GET" {
            return Err("Only GET representations can enter this snapshot".into());
        }
        let url = canonical(resource_url, false)?;
        if self.resources.len() >= 512
            || response.body.len() > MAX_BODY
            || self.total.saturating_add(response.body.len()) > MAX_TOTAL
        {
            return Err("Snapshot resource budget exceeded".into());
        }
        if response.framing == BodyFraming::ConnectionClose {
            return Err("Close-delimited resource completeness is unknown".into());
        }
        if !response.trailers.is_empty() {
            return Err("Response trailers need separate review".into());
        }
        if !(200..=599).contains(&response.status) || matches!(response.status, 206 | 304) {
            return Err("Partial or unsupported snapshot response".into());
        }
        if matches!(response.status, 204 | 205) && !response.body.is_empty() {
            return Err("Bodyless snapshot response contains data".into());
        }
        let mut names = BTreeSet::new();
        let mut headers: Vec<Value> = Vec::new();
        let mut connection_names = BTreeSet::new();
        let mut ignored_cookie = false;
        let binding = match scope {
            SnapshotScope::Ordinary => snapshot_origin_binding(&response)?,
            _ => snapshot_binding_scoped(&response, scope)?,
        };
        for h in &response.headers {
            if h.name.eq_ignore_ascii_case("connection") {
                let value = std::str::from_utf8(&h.value)
                    .map_err(|_| "Invalid connection response header")?;
                connection_names.extend(value.split(',').map(|v| v.trim().to_ascii_lowercase()));
            }
        }
        let request_accept = response.request_accept.clone();
        let request_origin = response
            .request_origin
            .as_ref()
            .map(|origin| origin.as_str().to_owned());
        for h in response.headers {
            let name = h.name.to_ascii_lowercase();
            if matches!(name.as_str(), "set-cookie" | "set-cookie2") {
                ignored_cookie = true;
                continue;
            }
            if matches!(
                name.as_str(),
                "authorization" | "proxy-authorization" | "www-authenticate" | "proxy-authenticate"
            ) {
                return Err("Authenticated snapshot response is unsupported".into());
            }
            if matches!(
                name.as_str(),
                "content-length"
                    | "content-encoding"
                    | "transfer-encoding"
                    | "connection"
                    | "keep-alive"
                    | "proxy-connection"
                    | "te"
                    | "trailer"
                    | "upgrade"
            ) || connection_names.contains(&name)
            {
                if name == "content-encoding" && !h.value.eq_ignore_ascii_case(b"identity") {
                    return Err("Encoded snapshot body is unsupported".into());
                }
                continue;
            }
            let value = std::str::from_utf8(&h.value)
                .map_err(|_| "Non-UTF8 snapshot header needs review")?;
            if value.len() > 8192
                || value.chars().any(|c| c.is_control())
                || h.name.is_empty()
                || h.name.len() > 256
                || !h
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&c))
            {
                return Err("Invalid or duplicate snapshot response header".into());
            }
            if !names.insert(name.clone()) {
                // Vary is a list field. Retain every selecting dimension when
                // serializing repeated field lines; never discard a variant.
                if name != "vary" {
                    return Err("Invalid or duplicate snapshot response header".into());
                }
                let previous = headers
                    .iter_mut()
                    .find(|header| {
                        header["name"]
                            .as_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case("vary"))
                    })
                    .ok_or("Missing snapshot Vary header")?;
                let combined = format!(
                    "{}, {value}",
                    previous["value"]
                        .as_str()
                        .ok_or("Invalid snapshot Vary header")?
                );
                if combined.len() > 8192 {
                    return Err("Snapshot Vary header budget exceeded".into());
                }
                previous["value"] = json!(combined);
                continue;
            }
            headers.push(json!({"name":h.name,"value":value}));
        }
        if headers.len() > 127 {
            return Err("Snapshot header budget exceeded".into());
        }
        let mut entry = url::Url::parse(&self.entry_url).map_err(|_| "Invalid snapshot entry")?;
        entry.set_fragment(None);
        let first = url == entry.as_str() && !self.files.contains_key("index.html");
        if first
            && (!(200..300).contains(&response.status)
                || !headers.iter().any(|h| {
                    h["name"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("content-type"))
                        && h["value"].as_str().is_some_and(|s| {
                            matches!(
                                s.split(';')
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_ascii_lowercase()
                                    .as_str(),
                                "text/html" | "application/xhtml+xml"
                            )
                        })
                }))
        {
            return Err("Snapshot entry is not a successful HTML representation".into());
        }
        let mut resource = json!({"url":url,"method":"GET","request_body_sha256":waypoint_domain::sha256_hex(b""),"status":response.status,"headers":headers});
        if let Some(origin) = request_origin {
            resource["request_origin"] = json!(origin);
        } else if binding.origin {
            resource["request_origin_absent"] = json!(true);
        }
        if binding.stylesheet {
            resource["request_origin_absent_stylesheet"] = json!(true);
        }
        if binding.preflight {
            resource["request_preflight_absent"] = json!(true);
        }
        if binding.accept {
            resource["request_accept"] = json!(request_accept);
        }
        if binding.requested_with_absent {
            resource["request_x_requested_with_absent"] = json!(true);
        }
        let previous: Vec<_> = self
            .resources
            .iter()
            .filter(|r| r["url"] == resource["url"])
            .collect();
        if previous.len() >= 8 || previous.iter().any(|r| !variants_disjoint(r, &resource)) {
            return Err(
                "Snapshot request identity is ambiguous or exceeds its variant budget".into(),
            );
        }
        let filename = if first {
            "index.html".into()
        } else {
            format!("resource-{:04}.bin", self.resources.len() + 1)
        };
        let hash = write_new(&self.root, &filename, &response.body)?;
        self.total += response.body.len();
        self.files.insert(filename.clone(), hash.clone());
        self.urls.insert(url.clone());
        resource["body_file"] = json!(filename);
        resource["body_sha256"] = json!(hash);
        self.resources.push(resource);
        if ignored_cookie {
            self.warnings.insert("response_cookies_not_replayed");
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<Snapshot, String> {
        if self.failed {
            return Err("Failed snapshot cannot be activated".into());
        }
        if !self.files.contains_key("index.html") {
            return Err("Snapshot entry has not been captured".into());
        }
        let manifest = json!({"schema":if self.resources.len()>self.urls.len() {2}else{1},"body_representation":"decoded_http","entry_url":self.entry_url,"resources":self.resources});
        let bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|_| "Cannot encode replay manifest")?;
        if bytes.len() > 1024 * 1024 {
            return Err("Replay manifest budget exceeded".into());
        }
        let report = json!({"schema":1,"state":"archive_written","resources":self.resources.len(),"body_bytes":self.total,"warnings":self.warnings,"server_validation":"not_performed","source_permission":"must_be_established_by_capture_caller","note":"Only supplied GET representations are recorded; missing steps and cookie-dependent behavior remain unknown."});
        let report_bytes =
            serde_json::to_vec_pretty(&report).map_err(|_| "Cannot encode capture report")?;
        let started = fs::metadata(self.root.join("capture-started.json"))
            .map_err(|_| "Cannot inspect capture state")?
            .len();
        if (self.total as u64)
            .saturating_add(bytes.len() as u64)
            .saturating_add(report_bytes.len() as u64)
            .saturating_add(started)
            > MAX_TOTAL as u64
        {
            return Err("Complete snapshot including metadata exceeds the content budget".into());
        }
        self.files.insert(
            "replay.json".into(),
            write_new(&self.root, "replay.json", &bytes)?,
        );
        self.files.insert(
            "capture-report.json".into(),
            write_new(&self.root, "capture-report.json", &report_bytes)?,
        );

        Ok(Snapshot {
            root: self.root,
            files: self.files,
            entry: "index.html".into(),
            manifest: "replay.json".into(),
            warnings: self.warnings.into_iter().map(str::to_owned).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(body: &str, mime: &str, extra: &str) -> CapturedResponse {
        let raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{extra}\r\n{body}",
            body.len()
        );
        waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(raw.into_bytes()),
            "GET",
            8 * 1024 * 1024,
        )
        .unwrap()
    }
    fn builder(root: &Path) -> SnapshotBuilder {
        SnapshotBuilder::start(&root.join("capture"), "https://fixture.invalid/form#apply").unwrap()
    }
    fn origin_style(origin: Option<&str>) -> CapturedResponse {
        let mut value = response("input{color:green}", "text/css", "Vary: Origin\r\n");
        value.request_header_names = Some(vec!["host".into(), "accept".into()]);
        value.request_accept = Some("*/*".into());
        if let Some(origin) = origin {
            value.request_origin = Some(waypoint_net::PublicRequestOrigin::parse(origin).unwrap());
            value
                .request_header_names
                .as_mut()
                .unwrap()
                .push("origin".into());
        }
        value
    }
    #[test]
    fn versioned_variants_require_independent_disjoint_binding_before_writing() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        capture
            .add(
                "https://fixture.invalid/form",
                response("<!doctype html>Fixture", "text/html", ""),
            )
            .unwrap();
        let url = "https://cdn.fixture.invalid/shared.css";
        capture
            .add(url, origin_style(Some("https://fixture.invalid")))
            .unwrap();
        capture
            .add_scoped(
                url,
                origin_style(None),
                SnapshotScope::OriginAbsentStylesheet,
            )
            .unwrap();
        let snapshot = capture.finish().unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(snapshot.root.join(snapshot.manifest)).unwrap())
                .unwrap();
        assert_eq!(manifest["schema"], 2);
        assert_eq!(manifest["resources"].as_array().unwrap().len(), 3);
        assert_ne!(
            manifest["resources"][1]["body_file"],
            manifest["resources"][2]["body_file"]
        );
        assert_eq!(
            manifest["resources"][1]["request_origin"],
            "https://fixture.invalid"
        );
        assert_eq!(
            manifest["resources"][2]["request_origin_absent_stylesheet"],
            true
        );
        for duplicate in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let mut capture = builder(temp.path());
            capture
                .add(
                    "https://fixture.invalid/form",
                    response("Fixture", "text/html", ""),
                )
                .unwrap();
            capture
                .add(url, origin_style(Some("https://fixture.invalid")))
                .unwrap();
            let value = if duplicate {
                origin_style(Some("https://fixture.invalid"))
            } else {
                response("input{}", "text/css", "")
            };
            assert!(capture.add(url, value).is_err());
            assert!(!capture.root.join("resource-0003.bin").exists());
            assert!(capture.finish().is_err());
        }
    }
    #[test]
    fn representation_budget_counts_same_url_variants() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        capture
            .add(
                "https://fixture.invalid/form",
                response("Fixture", "text/html", ""),
            )
            .unwrap();
        for index in 0..8 {
            capture
                .add(
                    "https://cdn.fixture.invalid/shared.css",
                    origin_style(Some(&format!("https://site{index}.invalid"))),
                )
                .unwrap();
        }
        assert!(capture
            .add(
                "https://cdn.fixture.invalid/shared.css",
                origin_style(Some("https://overflow.invalid"))
            )
            .is_err());
        assert!(capture.finish().is_err());
    }

    #[test]
    fn anonymous_navigation_variants_retain_native_request_proof_and_all_vary_lines() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        let mut value = response(
            "<label>Fictional</label>",
            "text/html",
            "Vary: Accept-Encoding\r\nVary: Accept, X-Requested-With, Origin\r\n",
        );
        value.request_header_names =
            Some(vec!["host".into(), "accept".into(), "user-agent".into()]);
        value.request_accept = Some("*/*".into());
        capture.add("https://fixture.invalid/form", value).unwrap();
        let snapshot = capture.finish().unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(snapshot.root.join(snapshot.manifest)).unwrap())
                .unwrap();
        let resource = &manifest["resources"][0];
        assert_eq!(resource["request_accept"], "*/*");
        assert_eq!(resource["request_x_requested_with_absent"], true);
        assert_eq!(resource["request_origin_absent"], true);
        let vary: Vec<_> = resource["headers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|header| {
                header["name"]
                    .as_str()
                    .unwrap()
                    .eq_ignore_ascii_case("vary")
            })
            .collect();
        assert_eq!(vary.len(), 1);
        assert_eq!(
            vary[0]["value"],
            "Accept-Encoding, Accept, X-Requested-With, Origin"
        );
    }

    #[test]
    fn navigation_variant_refuses_unproved_credentialed_body_or_non_document_scope() {
        let mut original = response(
            "<p>Fictional</p>",
            "text/html",
            "Vary: Accept, X-Requested-With, Origin\r\n",
        );
        original.request_header_names = Some(vec!["host".into(), "accept".into()]);
        original.request_accept = Some("*/*".into());
        assert!(snapshot_origin_binding(&original).is_ok());
        for accept in [
            None,
            Some("not a media range"),
            Some(" */*"),
            Some("*/*\r\nX: x"),
        ] {
            let mut invalid = original.clone();
            invalid.request_accept = accept.map(str::to_owned);
            assert!(snapshot_origin_binding(&invalid).is_err());
        }
        for names in [
            None,
            Some(vec!["host"]),
            Some(vec!["accept"]),
            Some(vec!["host", "accept", "Accept"]),
            Some(vec!["host", "bad name", "accept"]),
        ] {
            let mut invalid = original.clone();
            invalid.request_header_names =
                names.map(|names| names.into_iter().map(str::to_owned).collect());
            assert!(snapshot_origin_binding(&invalid).is_err());
        }
        for header in [
            "cookie",
            "authorization",
            "proxy-authorization",
            "origin",
            "content-length",
            "transfer-encoding",
            "content-type",
            "access-control-request-method",
            "access-control-request-headers",
            "x-requested-with",
        ] {
            let mut invalid = original.clone();
            invalid
                .request_header_names
                .as_mut()
                .unwrap()
                .push(header.into());
            assert!(snapshot_origin_binding(&invalid).is_err(), "{header}");
        }
        for mime in ["application/json", "text/css", "text/plain"] {
            let mut invalid = original.clone();
            invalid.headers[0].value = mime.as_bytes().to_vec();
            assert!(snapshot_origin_binding(&invalid).is_err());
        }
        let mut invalid = original.clone();
        invalid.headers.push(invalid.headers[0].clone());
        assert!(snapshot_origin_binding(&invalid).is_err());
        for method in ["HEAD", "POST", "OPTIONS"] {
            let mut invalid = original.clone();
            invalid.request_method = method.into();
            assert!(snapshot_origin_binding(&invalid).is_err());
        }
        assert!(snapshot_binding_scoped(&original, SnapshotScope::OriginAbsentStylesheet).is_err());
        for vary in ["Accept, Cookie", "Accept, *", "Accept, Accept-Language"] {
            let mut invalid = original.clone();
            invalid
                .headers
                .iter_mut()
                .find(|header| header.name.eq_ignore_ascii_case("vary"))
                .unwrap()
                .value = vary.as_bytes().to_vec();
            assert!(snapshot_origin_binding(&invalid).is_err());
        }
        let mut redirect = original;
        redirect.status = 302;
        redirect.headers[0].value = b"text/plain".to_vec();
        assert!(snapshot_origin_binding(&redirect).is_ok());
    }
    #[test]
    fn snapshot_preflight_and_positive_origin_require_complete_native_get_proof() {
        let mut value = response("void 0", "text/javascript", "");
        value.headers.push(waypoint_net::Header {
            name: "Vary".into(),
            value: b"Origin, Access-Control-Request-Headers, Access-Control-Request-Method"
                .to_vec(),
        });
        value.request_origin =
            Some(waypoint_net::PublicRequestOrigin::parse("https://example.org").unwrap());
        value.request_header_names = Some(vec!["host".into(), "origin".into()]);
        let proof = snapshot_origin_binding(&value).unwrap();
        assert!(proof.origin && proof.preflight);
        for names in [
            None,
            Some(vec!["host"]),
            Some(vec!["host", "origin", "access-control-request-method"]),
            Some(vec!["host", "origin", "access-control-request-headers"]),
            Some(vec!["host", "origin", "Origin"]),
        ] {
            value.request_header_names =
                names.map(|names| names.into_iter().map(str::to_owned).collect());
            assert!(snapshot_origin_binding(&value).is_err());
        }
        value.request_header_names = Some(vec!["host".into(), "origin".into()]);
        value.request_method = "OPTIONS".into();
        assert!(snapshot_origin_binding(&value).is_err());
    }

    #[test]
    fn snapshot_origin_variation_requires_recorded_absence_and_binds_navigation() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        let mut value = response(
            "<h1>Fictional page</h1>",
            "text/html",
            "Vary: Accept-Encoding, Origin\r\n",
        );
        assert!(value.request_header_names.is_none());
        value.request_header_names =
            Some(vec!["host".into(), "accept".into(), "user-agent".into()]);
        capture.add("https://fixture.invalid/form", value).unwrap();
        let snapshot = capture.finish().unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(snapshot.root.join("replay.json")).unwrap()).unwrap();
        assert_eq!(manifest["resources"][0]["request_origin_absent"], true);
    }

    #[test]
    fn snapshot_origin_unknown_present_or_invalid_proof_and_other_variants_stay_refused() {
        for names in [
            None,
            Some(vec![]),
            Some(vec!["host", "Origin"]),
            Some(vec!["host", "accept", "Accept"]),
            Some(vec!["host", "bad name"]),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut capture = builder(temp.path());
            let mut value = response("x", "text/html", "Vary: Origin\r\n");
            value.request_header_names =
                names.map(|names| names.into_iter().map(str::to_owned).collect());
            assert!(capture
                .add("https://fixture.invalid/form", value)
                .unwrap_err()
                .contains("Header-dependent"));
            assert!(capture.finish().is_err());
        }
        for (mime, vary) in [
            ("text/html", "*"),
            ("text/html", "Origin, Accept-Language"),
            ("application/json", "Origin"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut capture = builder(temp.path());
            let mut value = response("x", mime, &format!("Vary: {vary}\r\n"));
            value.request_header_names = Some(vec!["host".into()]);
            assert!(capture.add("https://fixture.invalid/form", value).is_err());
        }
    }
    #[test]
    fn observed_stylesheet_scope_persists_absence_even_without_vary() {
        for vary in ["", "Vary: Accept-Encoding, Origin\r\n"] {
            let temp = tempfile::tempdir().unwrap();
            let mut capture = builder(temp.path());
            capture
                .add(
                    "https://fixture.invalid/form",
                    response("<h1>Fixture</h1>", "text/html", ""),
                )
                .unwrap();
            let mut css = response("body{color:navy}", "text/css; charset=utf-8", vary);
            css.request_header_names = Some(vec!["host".into(), "accept".into()]);
            if !vary.is_empty() {
                assert!(snapshot_origin_binding(&css).is_err());
            }
            capture
                .add_scoped(
                    "https://fixture.invalid/style.css",
                    css,
                    SnapshotScope::OriginAbsentStylesheet,
                )
                .unwrap();
            let snapshot = capture.finish().unwrap();
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(snapshot.root.join("replay.json")).unwrap())
                    .unwrap();
            let style = &manifest["resources"][1];
            assert_eq!(style["request_origin_absent"], true);
            assert_eq!(style["request_origin_absent_stylesheet"], true);
            assert!(style.get("request_origin").is_none());
            assert!(manifest["resources"][0]
                .get("request_origin_absent_stylesheet")
                .is_none());
        }
    }

    #[test]
    fn stylesheet_scope_never_infers_anonymous_get_proof_from_css_mime() {
        let scope = SnapshotScope::OriginAbsentStylesheet;
        for names in [
            None,
            Some(vec![]),
            Some(vec!["accept"]),
            Some(vec!["host", "Origin"]),
            Some(vec!["host", "Accept", "accept"]),
            Some(vec!["host", "bad name"]),
            Some(vec!["host", "cookie"]),
            Some(vec!["host", "authorization"]),
            Some(vec!["host", "proxy-authorization"]),
            Some(vec!["host", "access-control-request-method"]),
            Some(vec!["host", "access-control-request-headers"]),
        ] {
            let mut css = response("body{}", "text/css", "Vary: Origin\r\n");
            css.request_header_names =
                names.map(|names| names.into_iter().map(str::to_owned).collect());
            assert!(snapshot_binding_scoped(&css, scope).is_err());
        }
        for (mime, vary, method, origin) in [
            ("application/json", "Origin", "GET", false),
            ("text/css", "*", "GET", false),
            ("text/css", "Origin, Accept-Language", "GET", false),
            ("text/css", "Origin", "POST", false),
            ("text/css", "Origin", "GET", true),
        ] {
            let mut css = response("body{}", mime, &format!("Vary: {vary}\r\n"));
            css.request_header_names = Some(vec!["host".into()]);
            css.request_method = method.into();
            if origin {
                css.request_origin = Some(
                    waypoint_net::PublicRequestOrigin::parse("https://fixture.invalid").unwrap(),
                );
                css.request_header_names
                    .as_mut()
                    .unwrap()
                    .push("origin".into());
            }
            assert!(snapshot_binding_scoped(&css, scope).is_err());
        }
        let mut redirect = response("", "text/plain", "Vary: Origin\r\n");
        redirect.status = 302;
        redirect.request_header_names = Some(vec!["host".into()]);
        let binding = snapshot_binding_scoped(&redirect, scope).unwrap();
        assert!(binding.origin && binding.stylesheet);
        assert!(snapshot_origin_binding(&redirect).is_err());
    }

    #[test]
    fn snapshot_stream_bytes_create_bound_manifest_without_cookies_or_hop_headers() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        capture.add("https://fixture.invalid/form",response("<!doctype html><label>Fictional form</label>","text/html","Set-Cookie: fictional=discarded\r\nConnection: X-Trace\r\nX-Trace: hop only\r\n")).unwrap();
        capture
            .add(
                "https://fixture.invalid/app.js",
                response(
                    "console.log('fictional');",
                    "text/javascript",
                    "Vary: Accept-Encoding\r\n",
                ),
            )
            .unwrap();
        let snapshot = capture.finish().unwrap();
        assert_eq!(snapshot.warnings, ["response_cookies_not_replayed"]);
        for (name, digest) in &snapshot.files {
            assert_eq!(
                &waypoint_domain::sha256_hex(&fs::read(snapshot.root.join(name)).unwrap()),
                digest
            );
        }
        let text = fs::read_to_string(snapshot.root.join(snapshot.manifest)).unwrap();
        assert!(
            !text.contains("discarded")
                && !text.contains("hop only")
                && !text.contains("Content-Length")
        );
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["resources"].as_array().unwrap().len(), 2);
        assert_eq!(value["resources"][0]["body_file"], "index.html");
    }
    #[test]
    fn snapshot_failure_is_retained_and_cannot_be_ignored_to_activate_partial_capture() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        capture
            .add(
                "https://fixture.invalid/form",
                response("<html>Fictional</html>", "text/html", ""),
            )
            .unwrap();
        assert!(capture
            .add(
                "https://fixture.invalid/form",
                response("changed", "text/html", "")
            )
            .is_err());
        assert!(capture.root.join("capture-failure.json").is_file());
        assert!(capture
            .add(
                "https://fixture.invalid/other",
                response("other", "text/plain", "")
            )
            .is_err());
        assert!(capture.finish().is_err());
        assert!(!temp.path().join("capture/replay.json").exists());
    }
    #[test]
    fn snapshot_refuses_cookie_dependent_variants_authentication_encoding_and_uncertain_framing() {
        for extra in [
            "Vary: Cookie\r\n",
            "WWW-Authenticate: Bearer\r\n",
            "Content-Type: text/html\r\n",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut capture = builder(temp.path());
            assert!(capture
                .add(
                    "https://fixture.invalid/form",
                    response("<html>Fictional</html>", "text/html", extra)
                )
                .is_err());
            assert!(capture.finish().is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        let mut value = response("x", "text/html", "");
        value.framing = BodyFraming::ConnectionClose;
        assert!(capture
            .add("https://fixture.invalid/form", value)
            .unwrap_err()
            .contains("unknown"));
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        let mut value = response("x", "text/html", "");
        value.headers.push(waypoint_net::Header {
            name: "Content-Encoding".into(),
            value: b"gzip".to_vec(),
        });
        assert!(capture
            .add("https://fixture.invalid/form", value)
            .unwrap_err()
            .contains("Encoded"));
    }
    #[test]
    fn snapshot_entry_urls_paths_existing_files_and_budgets_are_not_guessed() {
        let temp = tempfile::tempdir().unwrap();
        for url in [
            "http://fixture.invalid/",
            "https://user:fictional@fixture.invalid/",
            "https://fixture.invalid:444/form",
            "https://Fixture.invalid/form",
        ] {
            assert!(SnapshotBuilder::start(&temp.path().join("refused"), url).is_err());
        }
        let capture = builder(temp.path());
        assert!(SnapshotBuilder::start(&capture.root, "https://fixture.invalid/form").is_err());
        assert!(capture.finish().is_err());
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        capture.total = MAX_TOTAL;
        assert!(capture
            .add(
                "https://fixture.invalid/form",
                response("x", "text/html", "")
            )
            .unwrap_err()
            .contains("budget"));
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        let mut value = response("x", "text/html", "");
        value.body = vec![b'x'; MAX_BODY + 1];
        assert!(capture
            .add("https://fixture.invalid/form", value)
            .unwrap_err()
            .contains("budget"));
    }
    #[test]
    fn snapshot_final_budget_includes_manifests_and_state_files() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        capture
            .add(
                "https://fixture.invalid/form",
                response("<html>Fictional</html>", "text/html", ""),
            )
            .unwrap();
        capture.total = MAX_TOTAL - 1;
        assert!(capture
            .finish()
            .err()
            .unwrap()
            .contains("including metadata"));
        assert!(!temp.path().join("capture/replay.json").exists());
    }
    #[test]
    fn snapshot_head_observation_cannot_be_relabelled_as_a_get_body() {
        let temp = tempfile::tempdir().unwrap();
        let mut capture = builder(temp.path());
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 1000\r\n\r\n";
        let response =
            waypoint_net::read_asset_response(&mut std::io::Cursor::new(raw), "HEAD", 4096)
                .unwrap();
        assert!(response.body.is_empty());
        assert_eq!(response.request_method, "HEAD");
        assert!(capture
            .add("https://fixture.invalid/form", response)
            .unwrap_err()
            .contains("Only GET"));
        assert!(capture.finish().is_err());
    }
}
