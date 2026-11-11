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

const MAX_BODY: usize = 4 * 1024 * 1024;
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
/// Return the binding needed by a supported response variant, without writing
/// or poisoning a snapshot when an optional public resource is unsupported.
pub(super) fn snapshot_origin_binding(response: &CapturedResponse) -> Result<bool, String> {
    if response.headers.iter().any(|h| {
        h.name.eq_ignore_ascii_case("connection")
            && std::str::from_utf8(&h.value).is_ok_and(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("vary"))
            })
    }) {
        return Ok(false);
    }
    let origin_absent = response.request_header_names.as_ref().is_some_and(|names| {
        let mut unique = BTreeSet::new();
        !names.is_empty()
            && names.len() <= 128
            && names.iter().all(|name| {
                !name.is_empty()
                    && name.len() <= 256
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&c))
                    && unique.insert(name.to_ascii_lowercase())
            })
            && unique.contains("host")
            && !unique.contains("origin")
    });
    let mut origin = false;
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
            if field.eq_ignore_ascii_case("origin") && origin_absent {
                origin = true;
            } else {
                return Err("Header-dependent snapshot representation needs review".into());
            }
        }
    }
    if origin {
        let headers: Vec<_> = response
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("content-type"))
            .collect();
        if response.request_method != "GET"
            || headers.len() != 1
            || !std::str::from_utf8(&headers[0].value).is_ok_and(|value| {
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
            })
        {
            return Err("Origin-dependent snapshots require HTML navigation resources".into());
        }
    }
    Ok(origin)
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
        if self.failed {
            return Err("Snapshot has failed; start a new capture".into());
        }
        let result = self.add_response(resource_url, response);
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
    ) -> Result<(), String> {
        if response.request_method != "GET" {
            return Err("Only GET representations can enter this snapshot".into());
        }
        let url = canonical(resource_url, false)?;
        if self.urls.contains(&url) {
            return Err("Snapshot request identity is ambiguous".into());
        }
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
        let mut headers = Vec::new();
        let mut connection_names = BTreeSet::new();
        let mut ignored_cookie = false;
        let vary_origin = snapshot_origin_binding(&response)?;
        for h in &response.headers {
            if h.name.eq_ignore_ascii_case("connection") {
                let value = std::str::from_utf8(&h.value)
                    .map_err(|_| "Invalid connection response header")?;
                connection_names.extend(value.split(',').map(|v| v.trim().to_ascii_lowercase()));
            }
        }
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
            if !names.insert(name.clone())
                || value.len() > 8192
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
            headers.push(json!({"name":h.name,"value":value}));
        }
        if headers.len() > 127 {
            return Err("Snapshot header budget exceeded".into());
        }
        let mut entry = url::Url::parse(&self.entry_url).map_err(|_| "Invalid snapshot entry")?;
        entry.set_fragment(None);
        let first = url == entry.as_str();
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
        let filename = if first {
            "index.html".into()
        } else {
            format!("resource-{:04}.bin", self.resources.len() + 1)
        };
        let hash = write_new(&self.root, &filename, &response.body)?;
        self.total += response.body.len();
        self.files.insert(filename.clone(), hash.clone());
        self.urls.insert(url.clone());
        let mut resource = json!({"url":url,"method":"GET","request_body_sha256":waypoint_domain::sha256_hex(b""),"status":response.status,"headers":headers,"body_file":filename,"body_sha256":hash});
        if vary_origin {
            resource["request_origin_absent"] = json!(true);
        }
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
        let manifest = json!({"schema":1,"body_representation":"decoded_http","entry_url":self.entry_url,"resources":self.resources});
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
