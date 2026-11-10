//! Bounded, anonymous public-resource collection before offline candidate entry.
//!
//! Run on an owned worker, never the UI thread. Callers establish that the entry
//! and subsequent URLs are public and contain no candidate data. There is no
//! browser cookie/header/body input and no candidate-entry operation. Freezing
//! consumes the coordinator and its transport; the archive has no live fallback.
//! OS DNS cancellation still requires the caller's outer process lifetime.
use crate::rehearsal_capture::{write_new, Snapshot, SnapshotBuilder};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use waypoint_net::CapturedResponse;
use waypoint_policy::{Operation, PermissionRegistry};

#[cfg(all(test, windows))]
#[path = "capture_session_qualification.rs"]
mod qualification;

const ROBOTS_BYTES: usize = 512 * 1024;
const HEADER_ALLOWANCE: usize = 128 * 1024;

#[derive(Clone, Copy)]
pub struct CaptureBudget {
    pub resources: usize,
    pub requests: usize,
    pub body_bytes: usize,
    /// Includes robots and discarded redirect/error response bodies.
    pub received_bytes: usize,
    pub seconds: u64,
}
impl Default for CaptureBudget {
    fn default() -> Self {
        Self {
            resources: 96,
            requests: 160,
            body_bytes: 4 * 1024 * 1024,
            received_bytes: 32 * 1024 * 1024,
            seconds: 90,
        }
    }
}
impl CaptureBudget {
    fn valid(self) -> bool {
        (1..=512).contains(&self.resources)
            && (1..=1024).contains(&self.requests)
            && (1..=4 * 1024 * 1024).contains(&self.body_bytes)
            && (1..=60 * 1024 * 1024).contains(&self.received_bytes)
            && (1..=120).contains(&self.seconds)
    }
}

/// An explicit origin-to-source binding, not a source inferred from page text.
pub struct CaptureOrigin {
    pub origin: String,
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureGap {
    InvalidUrl,
    OriginNotDeclared,
    SourceNotAllowed,
    RobotsDenied,
    RobotsUnavailable,
    RedirectLoop,
    RedirectLimit,
    InvalidRedirect,
    BudgetExceeded,
    Cancelled,
    Network,
    ArchiveRejected,
    EntryRequired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptureOutcome {
    Recorded { status: u16, body_bytes: usize },
    AlreadyRecorded,
    NeedsReview { reason: CaptureGap },
}
#[derive(Serialize)]
pub struct CaptureRecord {
    pub url_sha256: String,
    pub outcome: CaptureOutcome,
}
#[derive(Serialize)]
pub struct CaptureReport {
    pub schema: u8,
    pub requests: usize,
    pub received_body_bytes: usize,
    pub recorded_resources: usize,
    pub records: Vec<CaptureRecord>,
    pub elapsed_ms: u128,
    pub omitted_proposals: usize,
    pub robots: Vec<RobotsRecord>,
    pub discovery: Option<crate::capture_discovery::DiscoverySummary>,
    pub coverage: &'static str,
    pub server_validation: &'static str,
}
#[derive(Clone, Serialize)]
pub struct RobotsRecord {
    pub origin: String,
    pub status: u16,
    pub response_url_sha256: String,
    pub body_file: String,
    pub body_sha256: String,
}
pub struct FrozenCapture {
    pub snapshot: Snapshot,
    pub report: CaptureReport,
}

trait Transport: Send {
    fn get(&mut self, url: &str, millis: u64, max_bytes: usize) -> Result<CapturedResponse, ()>;
}
struct PublicTransport;
impl Transport for PublicTransport {
    fn get(&mut self, url: &str, millis: u64, max_bytes: usize) -> Result<CapturedResponse, ()> {
        waypoint_net::get_asset_capped(url, millis, max_bytes).map_err(|_| ())
    }
}
enum Robots {
    Rules(String),
    Unavailable,
}

pub struct CaptureSession {
    builder: Option<SnapshotBuilder>,
    root: PathBuf,
    entry: String,
    entry_fragment: Option<String>,
    entry_recorded: bool,
    sources: BTreeMap<String, String>,
    registry: PermissionRegistry,
    transport: Box<dyn Transport>,
    robots: BTreeMap<String, Robots>,
    visited: BTreeSet<String>,
    recorded: BTreeSet<String>,
    records: Vec<CaptureRecord>,
    omitted: usize,
    robots_records: Vec<RobotsRecord>,
    sidecars: BTreeMap<String, String>,
    requests: usize,
    received: usize,
    budget: CaptureBudget,
    started: Instant,
    cancelled: Arc<AtomicBool>,
    failed: bool,
    discovery: Option<crate::capture_discovery::DiscoverySummary>,
}

fn resource_url(raw: &str) -> Result<url::Url, CaptureGap> {
    if raw.len() > 4096 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return Err(CaptureGap::InvalidUrl);
    }
    let mut url = url::Url::parse(raw).map_err(|_| CaptureGap::InvalidUrl)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.port_or_known_default() != Some(443)
        || waypoint_net::parse_url(url.as_str()).is_err()
    {
        return Err(CaptureGap::InvalidUrl);
    }
    url.set_fragment(None);
    Ok(url)
}
impl CaptureSession {
    /// Run only on an owned background worker. The template contributes its
    /// verified private engine; candidate content is never accepted here.
    #[cfg(windows)]
    pub fn discover_resources(
        &mut self,
        python: &Path,
        template: crate::rehearsal::RehearsalConfig,
        rounds: u8,
        settle_ms: u64,
    ) -> Result<crate::capture_discovery::DiscoverySummary, String> {
        if !(250..=3000).contains(&settle_ms) {
            return Err("Invalid discovery settling budget".into());
        }
        self.discover_with_probe(rounds, |snapshot, remaining| {
            let mut config = template.clone();
            config.seconds = remaining.clamp(5, 30);
            crate::capture_discovery::render(python, config, snapshot, settle_ms)
        })
    }
    #[cfg(any(windows, test))]
    fn discover_with_probe(
        &mut self,
        rounds: u8,
        mut render: impl FnMut(Snapshot, u64) -> Result<crate::capture_discovery::Probe, String>,
    ) -> Result<crate::capture_discovery::DiscoverySummary, String> {
        use crate::capture_discovery::{DiscoveryRound, DiscoveryStop, DiscoverySummary};
        if !(1..=4).contains(&rounds) || self.discovery.is_some() {
            return Err("Discovery needs one fresh bounded run of one to four rounds".into());
        }
        let mut summary = DiscoverySummary {
            rounds: vec![],
            stopped: DiscoveryStop::RoundLimit,
            coverage: "observed requests only; no guarantee of complete page or unseen steps",
        };
        if !self.entry_recorded && !matches!(self.capture_entry(), CaptureOutcome::Recorded { .. })
        {
            summary.stopped = DiscoveryStop::ReviewRequired;
        } else {
            let mut attempted = BTreeSet::new();
            let mut review_seen = false;
            for round in 0..rounds {
                if self.check_time().is_err() {
                    summary.stopped = DiscoveryStop::CancelledOrExpired;
                    break;
                }
                let mut checkpoint_name = self
                    .root
                    .file_name()
                    .ok_or("Capture directory has no name")?
                    .to_os_string();
                checkpoint_name.push(format!("-discovery-{:02}", round + 1));
                let destination = self
                    .root
                    .parent()
                    .ok_or("Capture directory has no parent")?
                    .join(checkpoint_name);
                let checkpoint = self
                    .builder
                    .as_ref()
                    .ok_or("Capture already frozen")?
                    .discovery_snapshot(&destination);
                let snapshot = match checkpoint {
                    Ok(s) => s,
                    Err(_) => {
                        self.failed = true;
                        summary.stopped = DiscoveryStop::CheckpointFailed;
                        break;
                    }
                };
                let remaining = self
                    .budget
                    .seconds
                    .saturating_sub(self.started.elapsed().as_secs());
                if remaining < 5 {
                    summary.stopped = DiscoveryStop::CancelledOrExpired;
                    break;
                }
                // render() must finish verified browser shutdown before returning.
                // Failure never falls through to a public fetch or unrestricted browser.
                let probe = match render(snapshot, remaining) {
                    Ok(p) if p.valid() => p,
                    _ => {
                        summary.stopped = DiscoveryStop::BrowserFailed;
                        break;
                    }
                };
                if self.check_time().is_err() {
                    summary.stopped = DiscoveryStop::CancelledOrExpired;
                    break;
                }
                let unsupported = probe.unsupported();
                let mut result = DiscoveryRound {
                    proposed: probe.urls.len(),
                    newly_recorded: 0,
                    review_outcomes: 0,
                    omitted_requests: probe.omitted_requests,
                    ineligible_requests: probe.ineligible_requests,
                    pending_commands: probe.pending_commands,
                    unsupported_targets: unsupported,
                    unreplayed_transports: probe.unreplayed_transports,
                };
                for url in &probe.urls {
                    if !attempted.insert(url.clone()) {
                        continue;
                    }
                    let before = self.recorded.len();
                    if matches!(self.capture(url), CaptureOutcome::NeedsReview { .. }) {
                        result.review_outcomes += 1;
                    }
                    result.newly_recorded += self.recorded.len().saturating_sub(before);
                }
                review_seen |= result.review_outcomes > 0
                    || result.omitted_requests > 0
                    || result.ineligible_requests > 0
                    || result.pending_commands > 0
                    || unsupported > 0
                    || result.unreplayed_transports > 0;
                let no_new = result.newly_recorded == 0;
                summary.rounds.push(result);
                if probe.urls.is_empty() {
                    summary.stopped = if review_seen {
                        DiscoveryStop::ReviewRequired
                    } else {
                        DiscoveryStop::NoNewRequests
                    };
                    break;
                }
                if no_new {
                    summary.stopped = DiscoveryStop::ReviewRequired;
                    break;
                }
            }
        }
        self.discovery = Some(summary.clone());
        Ok(summary)
    }
    pub fn start(
        root: &Path,
        entry: &str,
        origins: Vec<CaptureOrigin>,
        registry: PermissionRegistry,
        budget: CaptureBudget,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        Self::with_transport(
            root,
            entry,
            origins,
            registry,
            budget,
            cancelled,
            Box::new(PublicTransport),
        )
    }
    fn with_transport(
        root: &Path,
        entry: &str,
        origins: Vec<CaptureOrigin>,
        registry: PermissionRegistry,
        budget: CaptureBudget,
        cancelled: Arc<AtomicBool>,
        transport: Box<dyn Transport>,
    ) -> Result<Self, String> {
        if !budget.valid() || origins.is_empty() || origins.len() > 16 {
            return Err("Invalid capture budget or origin scope".into());
        }
        let entry_url = resource_url(entry).map_err(|_| "Invalid public capture entry")?;
        let mut sources = BTreeMap::new();
        for scope in origins {
            let url = resource_url(&format!("{}/", scope.origin))
                .map_err(|_| "Invalid capture origin")?;
            if url.origin().ascii_serialization() != scope.origin
                || scope.source.len() > 128
                || scope.source.is_empty()
                || sources.insert(scope.origin, scope.source).is_some()
            {
                return Err("Invalid or duplicate capture origin".into());
            }
        }
        if !sources.contains_key(&entry_url.origin().ascii_serialization()) {
            return Err("Capture entry origin was not declared".into());
        }
        let canonical_entry = url::Url::parse(entry).map_err(|_| "Invalid public capture entry")?;
        let builder = SnapshotBuilder::start(root, canonical_entry.as_str())?;
        let entry_fragment = url::Url::parse(entry)
            .ok()
            .and_then(|u| u.fragment().map(str::to_owned));
        Ok(Self {
            builder: Some(builder),
            root: root.to_owned(),
            entry: entry_url.to_string(),
            entry_fragment,
            entry_recorded: false,
            sources,
            registry,
            transport,
            robots: BTreeMap::new(),
            visited: BTreeSet::new(),
            recorded: BTreeSet::new(),
            records: vec![],
            omitted: 0,
            robots_records: vec![],
            sidecars: BTreeMap::new(),
            requests: 0,
            received: 0,
            budget,
            started: Instant::now(),
            cancelled,
            failed: false,
            discovery: None,
        })
    }
    fn check_time(&self) -> Result<(), CaptureGap> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(CaptureGap::Cancelled);
        }
        if self.started.elapsed() >= Duration::from_secs(self.budget.seconds) {
            return Err(CaptureGap::BudgetExceeded);
        }
        Ok(())
    }
    fn authorize(&self, url: &url::Url) -> Result<(), CaptureGap> {
        let source = self
            .sources
            .get(&url.origin().ascii_serialization())
            .ok_or(CaptureGap::OriginNotDeclared)?;
        self.registry
            .check_public_route(source, Operation::Read)
            .map_err(|_| CaptureGap::SourceNotAllowed)
    }
    fn get(&mut self, url: &url::Url, limit: usize) -> Result<CapturedResponse, CaptureGap> {
        self.check_time()?;
        self.authorize(url)?;
        if self.requests >= self.budget.requests || self.received >= self.budget.received_bytes {
            return Err(CaptureGap::BudgetExceeded);
        }
        let remaining =
            Duration::from_secs(self.budget.seconds).saturating_sub(self.started.elapsed());
        let millis = remaining.as_millis().clamp(1, 8_000) as u64;
        let body_limit = limit.min(self.budget.received_bytes - self.received);
        self.requests += 1;
        let response = self
            .transport
            .get(url.as_str(), millis, body_limit + HEADER_ALLOWANCE)
            .map_err(|_| CaptureGap::Network)?;
        if response.request_method != "GET" {
            return Err(CaptureGap::Network);
        }
        self.received = self.received.saturating_add(response.body.len());
        self.check_time()?;
        if response.body.len() > body_limit || self.received > self.budget.received_bytes {
            return Err(CaptureGap::BudgetExceeded);
        }
        Ok(response)
    }
    fn robots_allows(&mut self, url: &url::Url) -> Result<(), CaptureGap> {
        let origin = url.origin().ascii_serialization();
        self.authorize(url)?;
        if !self.robots.contains_key(&origin) {
            let mut next = resource_url(&format!("{origin}/robots.txt"))?;
            let mut seen = BTreeSet::new();
            let result = (|| {
                for depth in 0..=5 {
                    if !seen.insert(next.to_string()) {
                        return Err(CaptureGap::RedirectLoop);
                    }
                    let response = self.get(&next, ROBOTS_BYTES)?;
                    if is_redirect(response.status) {
                        if depth == 5 {
                            return Err(CaptureGap::RedirectLimit);
                        }
                        next = redirect(&next, &response)?;
                        continue;
                    }
                    let filename = format!("robots-{:02}.txt", self.robots_records.len() + 1);
                    let hash = write_new(&self.root, &filename, &response.body)
                        .map_err(|_| CaptureGap::ArchiveRejected)?;
                    self.sidecars.insert(filename.clone(), hash.clone());
                    self.robots_records.push(RobotsRecord {
                        origin: origin.clone(),
                        status: response.status,
                        response_url_sha256: waypoint_domain::sha256_hex(next.as_str().as_bytes()),
                        body_file: filename,
                        body_sha256: hash,
                    });
                    if response.framing == waypoint_net::BodyFraming::ConnectionClose {
                        return Err(CaptureGap::RobotsUnavailable);
                    }
                    return match response.status {
                        200..=299 if plain_text(&response) => String::from_utf8(response.body)
                            .map(Robots::Rules)
                            .map_err(|_| CaptureGap::RobotsUnavailable),
                        // Stricter handling of authentication/rate limits, rather than
                        // interpreting those operational walls as an absent policy.
                        404 | 410 => Ok(Robots::Rules(String::new())),
                        _ => Err(CaptureGap::RobotsUnavailable),
                    };
                }
                Err(CaptureGap::RedirectLimit)
            })();
            match result {
                Ok(rules) => {
                    self.robots.insert(origin.clone(), rules);
                }
                Err(error) => {
                    self.robots.insert(origin, Robots::Unavailable);
                    return Err(error);
                }
            }
        }
        match self.robots.get(&origin) {
            Some(Robots::Rules(body))
                if robotstxt::DefaultMatcher::default().one_agent_allowed_by_robots(
                    body,
                    "waypoint",
                    url.as_str(),
                ) =>
            {
                Ok(())
            }
            Some(Robots::Rules(_)) => Err(CaptureGap::RobotsDenied),
            _ => Err(CaptureGap::RobotsUnavailable),
        }
    }
    fn collect(&mut self, raw: &str) -> Result<CaptureOutcome, CaptureGap> {
        self.check_time()?;
        if self.failed {
            return Err(CaptureGap::ArchiveRejected);
        }
        let mut next = resource_url(raw)?;
        if self.recorded.contains(next.as_str()) {
            return Ok(CaptureOutcome::AlreadyRecorded);
        }
        if !self.entry_recorded && next.as_str() != self.entry {
            return Err(CaptureGap::EntryRequired);
        }
        let is_entry = !self.entry_recorded;
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let (status, body_bytes) = loop {
            if !seen.insert(next.to_string()) {
                return Err(CaptureGap::RedirectLoop);
            }
            if !self.visited.contains(next.as_str()) && self.visited.len() >= self.budget.resources
            {
                return Err(CaptureGap::BudgetExceeded);
            }
            self.visited.insert(next.to_string());
            self.robots_allows(&next)?;
            let response = self.get(&next, self.budget.body_bytes)?;
            let status = response.status;
            let size = response.body.len();
            let target = if is_redirect(status) {
                Some(redirect(&next, &response)?)
            } else {
                None
            };
            chain.push((next.to_string(), response));
            if let Some(target) = target {
                if chain.len() > 5 {
                    return Err(CaptureGap::RedirectLimit);
                }
                next = target;
            } else {
                break (status, size);
            }
        };
        let builder = self.builder.as_mut().ok_or(CaptureGap::ArchiveRejected)?;
        if is_entry {
            let mut entry = next.clone();
            entry.set_fragment(self.entry_fragment.as_deref());
            builder
                .retarget_entry(entry.as_str())
                .map_err(|_| CaptureGap::ArchiveRejected)?;
        }
        for (url, response) in chain {
            if self.recorded.contains(&url) {
                continue;
            }
            if builder.add(&url, response).is_err() {
                self.failed = true;
                return Err(CaptureGap::ArchiveRejected);
            }
            self.recorded.insert(url);
        }
        if is_entry {
            self.entry_recorded = true;
        }
        Ok(CaptureOutcome::Recorded { status, body_bytes })
    }
    pub fn capture(&mut self, url: &str) -> CaptureOutcome {
        // Keep outcomes bounded even if a caller repeatedly proposes denied URLs.
        if self.records.len() >= self.budget.resources {
            self.omitted = self.omitted.saturating_add(1);
            return CaptureOutcome::NeedsReview {
                reason: CaptureGap::BudgetExceeded,
            };
        }
        let outcome = self
            .collect(url)
            .unwrap_or_else(|reason| CaptureOutcome::NeedsReview { reason });
        if outcome
            == (CaptureOutcome::NeedsReview {
                reason: CaptureGap::ArchiveRejected,
            })
        {
            self.failed = true;
        }
        self.records.push(CaptureRecord {
            url_sha256: waypoint_domain::sha256_hex(url.as_bytes()),
            outcome,
        });
        outcome
    }
    pub fn capture_entry(&mut self) -> CaptureOutcome {
        self.capture(&self.entry.clone())
    }
    pub fn report(&self) -> CaptureReport {
        CaptureReport {
            schema: 1,
            requests: self.requests,
            received_body_bytes: self.received,
            recorded_resources: self.recorded.len(),
            records: self
                .records
                .iter()
                .map(|r| CaptureRecord {
                    url_sha256: r.url_sha256.clone(),
                    outcome: r.outcome,
                })
                .collect(),
            elapsed_ms: self.started.elapsed().as_millis(),
            omitted_proposals: self.omitted,
            robots: self.robots_records.clone(),
            discovery: self.discovery.clone(),
            coverage: "unassessed; missing resources and server-dependent steps require review",
            server_validation: "not_performed",
        }
    }
    /// Consumes every fetch-capable object before returning candidate-entry data.
    /// A frozen archive still does not prove page completeness or no-egress safety.
    pub fn freeze(mut self) -> Result<FrozenCapture, String> {
        let report = self.report();
        let bytes =
            serde_json::to_vec_pretty(&report).map_err(|_| "Cannot encode capture report")?;
        let hash = write_new(&self.root, "coordinator-report.json", &bytes)?;
        self.check_time()
            .map_err(|_| "Capture cancelled or expired; review the saved report")?;
        if self.failed || !self.entry_recorded {
            return Err("Capture entry is incomplete; review the saved report".into());
        }
        let mut snapshot = self
            .builder
            .take()
            .ok_or("Capture already frozen")?
            .finish()?;
        snapshot
            .files
            .insert("coordinator-report.json".into(), hash);
        snapshot.files.extend(self.sidecars);
        drop(self.transport);
        Ok(FrozenCapture { snapshot, report })
    }
}
fn plain_text(response: &CapturedResponse) -> bool {
    let headers: Vec<_> = response
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("content-type"))
        .collect();
    headers.len() == 1
        && std::str::from_utf8(&headers[0].value)
            .ok()
            .is_some_and(|s| {
                s.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("text/plain")
            })
}
fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}
fn redirect(base: &url::Url, response: &CapturedResponse) -> Result<url::Url, CaptureGap> {
    let mut locations = response
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("location"));
    let value = locations
        .next()
        .and_then(|h| std::str::from_utf8(&h.value).ok())
        .ok_or(CaptureGap::InvalidRedirect)?;
    if locations.next().is_some() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(CaptureGap::InvalidRedirect);
    }
    let target = base.join(value).map_err(|_| CaptureGap::InvalidRedirect)?;
    resource_url(target.as_str()).map_err(|_| CaptureGap::InvalidRedirect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    const ORIGIN: &str = "https://fixture.invalid";
    const ENTRY: &str = "https://fixture.invalid/form";
    struct Fake {
        replies: BTreeMap<String, VecDeque<CapturedResponse>>,
        calls: Arc<Mutex<Vec<String>>>,
        dropped: Arc<AtomicBool>,
    }
    impl Transport for Fake {
        fn get(&mut self, url: &str, millis: u64, max: usize) -> Result<CapturedResponse, ()> {
            assert!((1..=8000).contains(&millis));
            assert!(max <= 4 * 1024 * 1024 + HEADER_ALLOWANCE);
            self.calls.lock().unwrap().push(url.into());
            self.replies
                .get_mut(url)
                .and_then(VecDeque::pop_front)
                .ok_or(())
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }
    fn response(status: u16, body: &str, mime: &str) -> CapturedResponse {
        CapturedResponse {
            request_method: "GET".into(),
            status,
            body: body.as_bytes().to_vec(),
            headers: vec![waypoint_net::Header {
                name: "Content-Type".into(),
                value: mime.as_bytes().to_vec(),
            }],
            trailers: vec![],
            framing: waypoint_net::BodyFraming::ContentLength,
        }
    }
    fn robots(body: &str) -> CapturedResponse {
        response(200, body, "text/plain; charset=utf-8")
    }
    fn html() -> CapturedResponse {
        response(
            200,
            "<!doctype html><label>Name<input id=name></label>",
            "text/html",
        )
    }
    fn moved(target: &str) -> CapturedResponse {
        let mut r = response(302, "moved", "text/plain");
        r.headers.push(waypoint_net::Header {
            name: "Location".into(),
            value: target.as_bytes().to_vec(),
        });
        r
    }
    struct Harness {
        folder: tempfile::TempDir,
        session: CaptureSession,
        calls: Arc<Mutex<Vec<String>>>,
        dropped: Arc<AtomicBool>,
    }
    fn harness(
        rows: Vec<(&str, CapturedResponse)>,
        source: &str,
        budget: CaptureBudget,
    ) -> Harness {
        let folder = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(vec![]));
        let dropped = Arc::new(AtomicBool::new(false));
        let mut replies: BTreeMap<String, VecDeque<CapturedResponse>> = BTreeMap::new();
        for (url, response) in rows {
            replies.entry(url.into()).or_default().push_back(response);
        }
        let session = CaptureSession::with_transport(
            &folder.path().join("capture"),
            ENTRY,
            vec![CaptureOrigin {
                origin: ORIGIN.into(),
                source: source.into(),
            }],
            PermissionRegistry::from_json(waypoint_policy::INITIAL_REGISTRY_JSON).unwrap(),
            budget,
            Arc::new(AtomicBool::new(false)),
            Box::new(Fake {
                replies,
                calls: calls.clone(),
                dropped: dropped.clone(),
            }),
        )
        .unwrap();
        Harness {
            folder,
            session,
            calls,
            dropped,
        }
    }
    fn ready() -> Harness {
        harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (ENTRY, html()),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        )
    }
    fn gap(outcome: CaptureOutcome, reason: CaptureGap) {
        assert_eq!(outcome, CaptureOutcome::NeedsReview { reason });
    }

    #[test]
    fn capture_freezes_verified_resources_rules_and_receipt_without_transport() {
        let mut h = ready();
        assert!(matches!(
            h.session.capture_entry(),
            CaptureOutcome::Recorded { status: 200, .. }
        ));
        assert_eq!(h.session.capture_entry(), CaptureOutcome::AlreadyRecorded);
        let result = h.session.freeze().unwrap();
        assert!(h.dropped.load(Ordering::Acquire));
        assert_eq!(h.calls.lock().unwrap().len(), 2);
        assert_eq!(result.report.recorded_resources, 1);
        assert_eq!(result.report.requests, 2);
        assert_eq!(result.report.robots.len(), 1);
        for (file, hash) in result.snapshot.files {
            assert_eq!(
                waypoint_domain::sha256_hex(
                    &std::fs::read(result.snapshot.root.join(file)).unwrap()
                ),
                hash
            );
        }
        assert!(h
            .folder
            .path()
            .join("capture/coordinator-report.json")
            .is_file());
    }
    #[test]
    fn source_denial_precedes_even_robots_network_access() {
        for source in ["linkedin", "workday", "not-registered"] {
            let mut h = harness(vec![], source, CaptureBudget::default());
            gap(h.session.capture_entry(), CaptureGap::SourceNotAllowed);
            assert!(h.calls.lock().unwrap().is_empty());
            assert!(h.session.freeze().is_err());
            let value: serde_json::Value = serde_json::from_slice(
                &std::fs::read(h.folder.path().join("capture/coordinator-report.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(value["requests"], 0);
        }
    }
    #[test]
    fn robots_denial_is_enforced_on_every_resource_and_cached_only_in_this_run() {
        let mut h = harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    robots("User-agent: *\nDisallow: /private/*\nAllow: /private/open$"),
                ),
                (ENTRY, html()),
                (
                    "https://fixture.invalid/private/open",
                    response(200, "ok", "text/plain"),
                ),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        assert!(matches!(
            h.session.capture_entry(),
            CaptureOutcome::Recorded { .. }
        ));
        gap(
            h.session.capture("https://fixture.invalid/private/secret"),
            CaptureGap::RobotsDenied,
        );
        assert!(matches!(
            h.session.capture("https://fixture.invalid/private/open"),
            CaptureOutcome::Recorded { .. }
        ));
        assert_eq!(h.calls.lock().unwrap().len(), 3);
    }
    #[test]
    fn robots_html_auth_rate_limit_and_server_errors_are_not_permission() {
        for response in [
            response(200, "<html>challenge</html>", "text/html"),
            response(401, "login", "text/plain"),
            response(429, "wait", "text/plain"),
            response(503, "retry", "text/plain"),
        ] {
            let mut h = harness(
                vec![("https://fixture.invalid/robots.txt", response)],
                "user_supplied_url",
                CaptureBudget::default(),
            );
            gap(h.session.capture_entry(), CaptureGap::RobotsUnavailable);
            assert_eq!(h.calls.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn absent_robots_can_allow_but_unreachable_incomplete_or_invalid_utf8_cannot() {
        let mut invalid = robots("ok");
        invalid.body = vec![0xff];
        let mut incomplete = robots("");
        incomplete.framing = waypoint_net::BodyFraming::ConnectionClose;
        for r in [invalid, incomplete] {
            let mut h = harness(
                vec![("https://fixture.invalid/robots.txt", r)],
                "user_supplied_url",
                CaptureBudget::default(),
            );
            gap(h.session.capture_entry(), CaptureGap::RobotsUnavailable);
        }
        let mut absent = harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    response(404, "not found", "text/html"),
                ),
                (ENTRY, html()),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        assert!(matches!(
            absent.session.capture_entry(),
            CaptureOutcome::Recorded { .. }
        ));
        let mut down = harness(vec![], "user_supplied_url", CaptureBudget::default());
        gap(down.session.capture_entry(), CaptureGap::Network);
        assert_eq!(down.calls.lock().unwrap().len(), 1);
    }
    #[test]
    fn entry_redirects_are_retained_and_final_page_gets_the_entry_file() {
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, moved("/final")),
                ("https://fixture.invalid/final", html()),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        assert!(matches!(
            h.session.capture_entry(),
            CaptureOutcome::Recorded { status: 200, .. }
        ));
        let frozen = h.session.freeze().unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(frozen.snapshot.root.join("replay.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["entry_url"], "https://fixture.invalid/final");
        assert_eq!(manifest["resources"].as_array().unwrap().len(), 2);
        assert_eq!(manifest["resources"][1]["body_file"], "index.html");
    }
    #[test]
    fn foreign_redirects_are_not_contacted_without_an_explicit_origin_scope() {
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, moved("https://other.invalid/final")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(h.session.capture_entry(), CaptureGap::OriginNotDeclared);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
        assert!(h.session.freeze().is_err());
    }
    #[test]
    fn robots_redirects_also_require_scope_and_keep_the_original_origin_binding() {
        let mut denied = harness(
            vec![(
                "https://fixture.invalid/robots.txt",
                moved("https://other.invalid/rules"),
            )],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(
            denied.session.capture_entry(),
            CaptureGap::OriginNotDeclared,
        );
        assert_eq!(denied.calls.lock().unwrap().len(), 1);
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", moved("/rules")),
                (
                    "https://fixture.invalid/rules",
                    robots("Disallow: /\nUser-agent: *\nDisallow: /"),
                ),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(h.session.capture_entry(), CaptureGap::RobotsDenied);
        assert_eq!(h.session.report().robots[0].origin, ORIGIN);
    }
    #[test]
    fn redirect_loops_and_ambiguous_locations_stop_without_retries() {
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, moved("/form")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(h.session.capture_entry(), CaptureGap::RedirectLoop);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
        let mut r = moved("/one");
        r.headers.push(waypoint_net::Header {
            name: "location".into(),
            value: b"/two".to_vec(),
        });
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, r),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(h.session.capture_entry(), CaptureGap::InvalidRedirect);
    }
    #[test]
    fn request_body_time_and_proposal_budgets_remain_explicit() {
        let mut h = ready();
        h.session.budget.requests = 1;
        gap(h.session.capture_entry(), CaptureGap::BudgetExceeded);
        assert_eq!(h.calls.lock().unwrap().len(), 1);
        let mut h = ready();
        h.session.budget.body_bytes = 1;
        gap(h.session.capture_entry(), CaptureGap::BudgetExceeded);
        let mut h = ready();
        h.session.started = Instant::now() - Duration::from_secs(121);
        gap(h.session.capture_entry(), CaptureGap::BudgetExceeded);
        assert!(h.calls.lock().unwrap().is_empty());
        let mut h = ready();
        h.session.budget.resources = 1;
        gap(
            h.session.capture("https://fixture.invalid/other"),
            CaptureGap::EntryRequired,
        );
        gap(h.session.capture_entry(), CaptureGap::BudgetExceeded);
        assert_eq!(h.session.report().omitted_proposals, 1);
    }
    #[test]
    fn cancellation_prevents_fetch_and_freeze_but_retains_review_receipt() {
        let mut h = ready();
        h.session.cancelled.store(true, Ordering::Release);
        gap(h.session.capture_entry(), CaptureGap::Cancelled);
        assert!(h.session.freeze().is_err());
        assert!(h.calls.lock().unwrap().is_empty());
        assert!(h
            .folder
            .path()
            .join("capture/coordinator-report.json")
            .is_file());
        assert!(!h.folder.path().join("capture/replay.json").exists());
    }
    #[test]
    fn invalid_and_credentialed_urls_are_not_forwarded_to_transport() {
        for url in [
            "http://fixture.invalid/form",
            "https://name:secret@fixture.invalid/form",
            "https://fixture.invalid:444/form",
            "https://fixture.invalid/\nform",
            " https://fixture.invalid/form",
        ] {
            let mut h = ready();
            gap(h.session.capture(url), CaptureGap::InvalidUrl);
            assert!(h.calls.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn archive_rejection_never_produces_an_activatable_manifest() {
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, response(401, "login", "text/html")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(h.session.capture_entry(), CaptureGap::ArchiveRejected);
        assert!(h.session.freeze().is_err());
        assert!(!h.folder.path().join("capture/replay.json").exists());
    }

    #[test]
    fn normalizable_urls_share_browser_request_identity() {
        let mut h = ready();
        assert!(matches!(
            h.session
                .capture("HTTPS://FIXTURE.invalid/form#application"),
            CaptureOutcome::Recorded { .. }
        ));
        assert_eq!(
            h.session.capture("https://fixture.invalid/form#different"),
            CaptureOutcome::AlreadyRecorded
        );
        assert_eq!(h.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn approved_cross_origin_redirects_get_their_own_robots_check() {
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, moved("https://cdn.invalid/page")),
                (
                    "https://cdn.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                ("https://cdn.invalid/page", html()),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        h.session
            .sources
            .insert("https://cdn.invalid".into(), "user_supplied_url".into());
        assert!(matches!(
            h.session.capture_entry(),
            CaptureOutcome::Recorded { .. }
        ));
        assert_eq!(h.session.report().robots.len(), 2);
        assert_eq!(h.calls.lock().unwrap().len(), 4);
        assert!(h.session.freeze().is_ok());
    }

    #[test]
    fn redirect_limit_does_not_issue_the_next_hop() {
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, moved("/1")),
                ("https://fixture.invalid/1", moved("/2")),
                ("https://fixture.invalid/2", moved("/3")),
                ("https://fixture.invalid/3", moved("/4")),
                ("https://fixture.invalid/4", moved("/5")),
                ("https://fixture.invalid/5", moved("/6")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(h.session.capture_entry(), CaptureGap::RedirectLimit);
        assert_eq!(h.calls.lock().unwrap().len(), 7);
        assert!(!h.calls.lock().unwrap().iter().any(|u| u.ends_with("/6")));
        assert!(h.session.freeze().is_err());
    }

    fn probe(urls: Vec<&str>) -> crate::capture_discovery::Probe {
        serde_json::from_value(
            serde_json::json!({"schema":1,"mode":"resource_discovery","urls":urls,
            "omitted_requests":0,"ineligible_requests":0,"pending_commands":0,
            "unsupported_targets":{},"unreplayed_transports":0,"unmanaged_child_sessions":0}),
        )
        .unwrap()
    }
    #[test]
    fn discovery_collects_transitive_requests_then_verifies_without_new_requests() {
        use crate::capture_discovery::DiscoveryStop;
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, html()),
                (
                    "https://fixture.invalid/app.js",
                    response(200, "fixture", "text/javascript"),
                ),
                (
                    "https://fixture.invalid/data",
                    response(200, "{}", "application/json"),
                ),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        let mut round = 0;
        let result = h
            .session
            .discover_with_probe(4, |snapshot, _| {
                assert!(snapshot.root.is_dir());
                round += 1;
                Ok(probe(match round {
                    1 => vec!["https://fixture.invalid/app.js"],
                    2 => vec!["https://fixture.invalid/data"],
                    _ => vec![],
                }))
            })
            .unwrap();
        assert_eq!(result.stopped, DiscoveryStop::NoNewRequests);
        assert_eq!(result.rounds.len(), 3);
        assert_eq!(h.calls.lock().unwrap().len(), 4);
        let frozen = h.session.freeze().unwrap();
        let actual: BTreeSet<_> = std::fs::read_dir(&frozen.snapshot.root)
            .unwrap()
            .map(|p| p.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(actual, frozen.snapshot.files.keys().cloned().collect());
        assert!(h
            .folder
            .path()
            .join("capture-discovery-03/replay.json")
            .exists());
    }
    #[test]
    fn discovery_denials_and_browser_failures_never_retry_or_fall_back() {
        use crate::capture_discovery::DiscoveryStop;
        let mut h = ready();
        let mut rounds = 0;
        let report = h
            .session
            .discover_with_probe(4, |_, _| {
                rounds += 1;
                Ok(probe(vec!["https://undeclared.invalid/script"]))
            })
            .unwrap();
        assert_eq!(report.stopped, DiscoveryStop::ReviewRequired);
        assert_eq!(rounds, 1);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
        let mut h = ready();
        let report = h
            .session
            .discover_with_probe(4, |_, _| Err("fictional browser failure".into()))
            .unwrap();
        assert_eq!(report.stopped, DiscoveryStop::BrowserFailed);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
        assert!(h.session.freeze().unwrap().report.discovery.is_some());
    }
    #[test]
    fn round_limit_does_not_claim_the_new_archive_was_fully_observed() {
        use crate::capture_discovery::DiscoveryStop;
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, html()),
                (
                    "https://fixture.invalid/a.js",
                    response(200, "x", "text/javascript"),
                ),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        let report = h
            .session
            .discover_with_probe(1, |_, _| Ok(probe(vec!["https://fixture.invalid/a.js"])))
            .unwrap();
        assert_eq!(report.stopped, DiscoveryStop::RoundLimit);
        assert_eq!(report.rounds[0].newly_recorded, 1);
        assert!(h
            .session
            .discover_with_probe(1, |_, _| Ok(probe(vec![])))
            .is_err());
    }
    #[test]
    fn discovery_unknown_channels_overflow_and_pending_commands_stay_reviewable() {
        use crate::capture_discovery::DiscoveryStop;
        for field in [
            "omitted_requests",
            "ineligible_requests",
            "pending_commands",
            "unreplayed_transports",
            "unmanaged_child_sessions",
        ] {
            let mut h = ready();
            let report=h.session.discover_with_probe(2,|_,_| {
                let mut value=serde_json::to_value(serde_json::json!({"schema":1,"mode":"resource_discovery","urls":[],
                    "omitted_requests":0,"ineligible_requests":0,"pending_commands":0,
                    "unsupported_targets":{},"unreplayed_transports":0,"unmanaged_child_sessions":0})).unwrap();
                value[field]=serde_json::json!(1);Ok(serde_json::from_value(value).unwrap())
            }).unwrap();
            assert_eq!(report.stopped, DiscoveryStop::ReviewRequired, "{field}");
        }
    }
    #[test]
    fn cancellation_after_render_precedes_any_proposed_network_access() {
        use crate::capture_discovery::DiscoveryStop;
        let mut h = ready();
        let cancel = h.session.cancelled.clone();
        let report = h
            .session
            .discover_with_probe(4, |_, _| {
                cancel.store(true, Ordering::Release);
                Ok(probe(vec!["https://fixture.invalid/missing"]))
            })
            .unwrap();
        assert_eq!(report.stopped, DiscoveryStop::CancelledOrExpired);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
        assert!(h.session.freeze().is_err());
    }
    #[test]
    fn changed_checkpoint_body_prevents_rendering_or_final_activation() {
        use crate::capture_discovery::DiscoveryStop;
        let mut h = ready();
        h.session.capture_entry();
        std::fs::write(h.session.root.join("index.html"), b"changed").unwrap();
        let report = h
            .session
            .discover_with_probe(2, |_, _| panic!("changed checkpoint was rendered"))
            .unwrap();
        assert_eq!(report.stopped, DiscoveryStop::CheckpointFailed);
        assert!(h.session.freeze().is_err());
    }
    #[cfg(windows)]
    #[test]
    fn discovery_config_cannot_start_a_candidate_entry_driver() {
        let config:crate::rehearsal::RehearsalConfig=serde_json::from_value(serde_json::json!({
            "schema":3,"engine_root":std::env::temp_dir(),"engine_files":{},"content_root":std::env::temp_dir(),
            "content_files":{},"entry":"index.html","seconds":10,"replay_manifest":"replay.json"})).unwrap();
        let result =
            crate::CdpDriver::launch_rehearsal(Path::new("missing-fictional-python"), config);
        assert!(
            matches!(result,Err(e) if e.to_string().contains("cannot open a candidate-entry driver"))
        );
    }
}
