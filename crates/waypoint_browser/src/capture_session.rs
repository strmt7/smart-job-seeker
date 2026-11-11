//! Bounded, anonymous public-resource collection before offline candidate entry.
//!
//! Run on an owned worker, never the UI thread. Callers establish that the entry
//! and subsequent URLs are public and contain no candidate data. There is no
//! browser cookie, arbitrary-header or body input and no candidate-entry operation.
//! A validated public Origin may come from an admitted document's observed asset GET. Freezing
//! consumes the coordinator and its transport; the archive has no live fallback.
//! OS DNS cancellation still requires the caller's outer process lifetime.
use crate::capture_discovery::AssetKind;
use crate::rehearsal_capture::{write_new, Snapshot, SnapshotBuilder, SnapshotScope};
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
const MAX_ORIGINS: usize = 16;
#[cfg(any(windows, test))]
const MAX_EMBEDDED_ORIGINS: usize = 2;
#[cfg(any(windows, test))]
const MAX_ASSET_ORIGINS: usize = 4;

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
            body_bytes: 8 * 1024 * 1024,
            received_bytes: 32 * 1024 * 1024,
            seconds: 90,
        }
    }
}
impl CaptureBudget {
    fn valid(self) -> bool {
        (1..=512).contains(&self.resources)
            && (1..=1024).contains(&self.requests)
            && (1..=8 * 1024 * 1024).contains(&self.body_bytes)
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
    PrivateUrl,
    UrlPrivacyUncertain,
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
    EmbeddedDocumentUnavailable,
    AssetUnavailable,
    RepresentationUnsupported,
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
    pub embedded_origins: Vec<EmbeddedOrigin>,
    pub asset_origins: Vec<AssetOrigin>,
    pub asset_origin_attempts: usize,
    pub asset_observations: Vec<AssetObservation>,
    pub omitted_asset_observations: usize,
    pub unavailable_robots_asset_origins: BTreeSet<String>,
    pub discovery: Option<crate::capture_discovery::DiscoverySummary>,
    pub coverage: &'static str,
    pub server_validation: &'static str,
    pub url_privacy: &'static str,
}
#[derive(Clone, Serialize)]
pub struct EmbeddedOrigin {
    origin: String,
    source: String,
    observed_url_sha256: String,
    discovery_round: u8,
}
#[derive(Clone, Serialize)]
pub struct AssetOrigin {
    origin: String,
    source: String,
    observed_url_sha256: String,
    discovery_round: u8,
    kinds: BTreeSet<AssetKind>,
    #[serde(skip)]
    request_origin: Option<waypoint_net::PublicRequestOrigin>,
    #[serde(skip)]
    origin_absent_stylesheet: bool,
}

#[derive(Clone, Serialize)]
pub struct AssetObservation {
    url_sha256: String,
    status: u16,
    body_bytes: usize,
    mime: Option<String>,
    varies_on: Vec<String>,
    vary_incomplete: bool,
    framing: &'static str,
}

struct ResourceScope<'a> {
    origin: &'a str,
    assets: Option<&'a BTreeSet<AssetKind>>,
    request_origin: Option<&'a waypoint_net::PublicRequestOrigin>,
    request_scope: SnapshotScope,
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
    fn get(
        &mut self,
        url: &str,
        millis: u64,
        max_bytes: usize,
        origin: Option<&waypoint_net::PublicRequestOrigin>,
    ) -> Result<CapturedResponse, ()>;
}
struct PublicTransport;
impl Transport for PublicTransport {
    fn get(
        &mut self,
        url: &str,
        millis: u64,
        max_bytes: usize,
        origin: Option<&waypoint_net::PublicRequestOrigin>,
    ) -> Result<CapturedResponse, ()> {
        match origin {
            Some(origin) => {
                waypoint_net::get_asset_capped_with_origin(url, millis, max_bytes, origin)
            }
            None => waypoint_net::get_asset_capped(url, millis, max_bytes),
        }
        .map_err(|_| ())
    }
}
enum Robots {
    Rules(String),
    MachineUnavailable403,
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
    embedded_origins: Vec<EmbeddedOrigin>,
    asset_origins: Vec<AssetOrigin>,
    asset_attempts: BTreeSet<String>,
    document_origins: BTreeSet<String>,
    asset_observations: Vec<AssetObservation>,
    omitted_asset_observations: usize,
    unavailable_robots_asset_origins: BTreeSet<String>,
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
    crate::capture_privacy::checked_reference(raw).map_err(privacy_gap)?;
    url.set_fragment(None);
    Ok(url)
}
fn privacy_gap(error: crate::capture_privacy::UrlPrivacyError) -> CaptureGap {
    match error {
        crate::capture_privacy::UrlPrivacyError::Invalid => CaptureGap::InvalidUrl,
        crate::capture_privacy::UrlPrivacyError::PrivateReference => CaptureGap::PrivateUrl,
        crate::capture_privacy::UrlPrivacyError::EncodingLimit => CaptureGap::UrlPrivacyUncertain,
    }
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
            form_outline: None,
            form_outline_snapshot_sha256: None,
            form_outline_round: None,
        };
        if !self.entry_recorded && !matches!(self.capture_entry(), CaptureOutcome::Recorded { .. })
        {
            summary.stopped = DiscoveryStop::ReviewRequired;
        } else {
            let mut attempted = BTreeMap::new();
            let mut attempted_absent_stylesheets = BTreeSet::new();
            let mut attempted_kinds: BTreeMap<String, BTreeSet<AssetKind>> = BTreeMap::new();
            let mut review_seen = false;
            for round in 0..rounds {
                summary.form_outline = None;
                summary.form_outline_snapshot_sha256 = None;
                summary.form_outline_round = None;
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
                    Ok(p)
                        if p.valid()
                            && p.asset_request_origins
                                .values()
                                .all(|origin| self.document_origins.contains(origin)) =>
                    {
                        p
                    }
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
                summary.form_outline = probe.form_outline.clone();
                summary.form_outline_snapshot_sha256 = probe.outline_snapshot_sha256.clone();
                summary.form_outline_round = summary.form_outline.as_ref().map(|_| round + 1);
                let mut result = DiscoveryRound {
                    proposed: probe.urls.len(),
                    newly_recorded: 0,
                    review_outcomes: 0,
                    omitted_requests: probe.omitted_requests,
                    ineligible_requests: probe.ineligible_requests,
                    pending_commands: probe.pending_commands,
                    unsupported_targets: unsupported,
                    unreplayed_transports: probe.unreplayed_transports,
                    preload_projection_failures: probe.preload_projection_failures,
                };
                let requests = probe.child_documents.iter().chain(
                    probe
                        .urls
                        .iter()
                        .filter(|url| !probe.child_documents.contains(*url)),
                );
                for url in requests {
                    let embedded = if probe.child_documents.contains(url) {
                        self.embedded_origin(url, round + 1)
                    } else {
                        None
                    };
                    let asset = probe.assets.get(url).and_then(|kinds| {
                        self.asset_origin(
                            url,
                            kinds,
                            round + 1,
                            probe.asset_request_origins.get(url).map(String::as_str),
                            probe.asset_origin_absent_stylesheets.contains(url),
                        )
                    });
                    if let Some(previous) = attempted.get(url) {
                        let new_binding = *previous
                            == (CaptureOutcome::NeedsReview {
                                reason: CaptureGap::OriginNotDeclared,
                            })
                            && (embedded.is_some()
                                || asset.is_some()
                                || resource_url(url).is_ok_and(|url| {
                                    self.sources
                                        .contains_key(&url.origin().ascii_serialization())
                                }));
                        let new_kind = *previous
                            == (CaptureOutcome::NeedsReview {
                                reason: CaptureGap::AssetUnavailable,
                            })
                            && asset.is_some()
                            && probe.assets.get(url).is_some_and(|kinds| {
                                attempted_kinds
                                    .get(url)
                                    .is_none_or(|previous| !kinds.is_subset(previous))
                            });
                        let new_absence = *previous
                            == CaptureOutcome::NeedsReview {
                                reason: CaptureGap::RepresentationUnsupported,
                            }
                            && probe.asset_origin_absent_stylesheets.contains(url)
                            && !attempted_absent_stylesheets.contains(url);
                        if !new_binding && !new_kind && !new_absence {
                            continue;
                        }
                    }
                    let before = self.recorded.len();
                    if asset
                        .as_ref()
                        .is_some_and(|scope| scope.origin_absent_stylesheet)
                    {
                        attempted_absent_stylesheets.insert(url.clone());
                    }
                    let outcome = if let Some(scope) = embedded {
                        self.capture_embedded(url, scope)
                    } else if let Some(scope) = asset {
                        self.capture_asset(url, scope)
                    } else if probe.asset_request_origins.contains_key(url)
                        || probe.asset_origin_absent_stylesheets.contains(url)
                    {
                        CaptureOutcome::NeedsReview {
                            reason: CaptureGap::OriginNotDeclared,
                        }
                    } else {
                        self.capture(url)
                    };
                    attempted.insert(url.clone(), outcome);
                    if let Some(kinds) = probe.assets.get(url) {
                        attempted_kinds
                            .entry(url.clone())
                            .or_default()
                            .extend(kinds.iter().copied());
                    }
                    if matches!(outcome, CaptureOutcome::NeedsReview { .. }) {
                        result.review_outcomes += 1;
                    }
                    result.newly_recorded += self.recorded.len().saturating_sub(before);
                }
                review_seen |= result.review_outcomes > 0
                    || result.preload_projection_failures > 0
                    || !self.unavailable_robots_asset_origins.is_empty()
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
    #[cfg(any(windows, test))]
    fn embedded_origin(&self, raw: &str, discovery_round: u8) -> Option<EmbeddedOrigin> {
        if self.sources.len() >= MAX_ORIGINS
            || self.embedded_origins.len() >= MAX_EMBEDDED_ORIGINS
            || self.check_time().is_err()
            || self.records.len() >= self.budget.resources
        {
            return None;
        }
        let url = resource_url(raw).ok()?;
        let origin = url.origin().ascii_serialization();
        if self.sources.contains_key(&origin) {
            return None;
        }
        // Only exact public ATS frontends with an existing Read policy. The
        // generic manual-URL source is never assigned to a discovered origin.
        let source = match url.host_str() {
            Some("boards.greenhouse.io" | "job-boards.greenhouse.io") => "greenhouse",
            Some("jobs.lever.co") => "lever",
            Some("jobs.ashbyhq.com") => "ashby",
            _ => return None,
        };
        self.registry
            .check_public_route(source, Operation::Read)
            .ok()?;
        Some(EmbeddedOrigin {
            origin,
            source: source.into(),
            observed_url_sha256: waypoint_domain::sha256_hex(raw.as_bytes()),
            discovery_round,
        })
    }
    #[cfg(any(windows, test))]
    fn capture_embedded(&mut self, url: &str, scope: EmbeddedOrigin) -> CaptureOutcome {
        self.sources
            .insert(scope.origin.clone(), scope.source.clone());
        let outcome = self.capture_scoped(
            url,
            Some(ResourceScope {
                origin: &scope.origin,
                assets: None,
                request_origin: None,
                request_scope: SnapshotScope::Ordinary,
            }),
        );
        if matches!(outcome, CaptureOutcome::Recorded { .. }) {
            self.embedded_origins.push(scope);
        } else {
            self.sources.remove(&scope.origin);
        }
        outcome
    }

    #[cfg(any(windows, test))]
    fn asset_origin(
        &mut self,
        raw: &str,
        kinds: &BTreeSet<AssetKind>,
        round: u8,
        request_origin: Option<&str>,
        origin_absent_stylesheet: bool,
    ) -> Option<AssetOrigin> {
        if origin_absent_stylesheet
            && (request_origin.is_some()
                || kinds.len() != 1
                || !kinds.contains(&AssetKind::Stylesheet))
        {
            return None;
        }
        let url = resource_url(raw).ok()?;
        let origin = url.origin().ascii_serialization();
        let request_origin = match request_origin {
            Some(value) if self.document_origins.contains(value) => {
                Some(waypoint_net::PublicRequestOrigin::parse(value).ok()?)
            }
            Some(_) => return None,
            None => None,
        };
        let declared = self.sources.contains_key(&origin);
        if !self.entry_recorded
            || kinds.is_empty()
            || self.check_time().is_err()
            || self.records.len() >= self.budget.resources
            || self.sources.len() >= MAX_ORIGINS
            || !declared
                && self.asset_attempts.len() >= MAX_ASSET_ORIGINS
                && !self.asset_attempts.contains(&origin)
        {
            return None;
        }
        let source = self
            .sources
            .get(&origin)
            .map(String::as_str)
            .unwrap_or_else(|| waypoint_policy::source_for_host(url.host_str().unwrap_or("")));
        self.registry
            .check_public_route(source, Operation::Read)
            .ok()?;
        if !declared {
            self.asset_attempts.insert(origin.clone());
        }
        Some(AssetOrigin {
            origin,
            source: source.into(),
            observed_url_sha256: waypoint_domain::sha256_hex(raw.as_bytes()),
            discovery_round: round,
            kinds: kinds.clone(),
            request_origin,
            origin_absent_stylesheet,
        })
    }

    #[cfg(any(windows, test))]
    fn capture_asset(&mut self, url: &str, scope: AssetOrigin) -> CaptureOutcome {
        // This binding permits only this observed asset and same-origin
        // redirects. It never promotes its host to an HTML/document origin.
        let temporary = !self.sources.contains_key(&scope.origin);
        if temporary {
            self.sources
                .insert(scope.origin.clone(), scope.source.clone());
        }
        let outcome = self.capture_scoped(
            url,
            Some(ResourceScope {
                origin: &scope.origin,
                assets: Some(&scope.kinds),
                request_origin: scope.request_origin.as_ref(),
                request_scope: if scope.origin_absent_stylesheet {
                    SnapshotScope::OriginAbsentStylesheet
                } else {
                    SnapshotScope::Ordinary
                },
            }),
        );
        if temporary {
            self.sources.remove(&scope.origin);
        }
        if matches!(outcome, CaptureOutcome::Recorded { .. }) {
            if let Some(previous) = self
                .asset_origins
                .iter_mut()
                .find(|previous| previous.origin == scope.origin)
            {
                previous.kinds.extend(scope.kinds);
            } else {
                self.asset_origins.push(scope);
            }
        }
        outcome
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
        if !budget.valid() || origins.is_empty() || origins.len() > MAX_ORIGINS {
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
            embedded_origins: vec![],
            asset_origins: vec![],
            asset_attempts: BTreeSet::new(),
            document_origins: BTreeSet::new(),
            asset_observations: vec![],
            omitted_asset_observations: 0,
            unavailable_robots_asset_origins: BTreeSet::new(),
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
    fn get(
        &mut self,
        url: &url::Url,
        limit: usize,
        origin: Option<&waypoint_net::PublicRequestOrigin>,
    ) -> Result<CapturedResponse, CaptureGap> {
        self.check_time()?;
        self.authorize(url)?;
        crate::capture_privacy::checked_reference(url.as_str()).map_err(privacy_gap)?;
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
            .get(url.as_str(), millis, body_limit + HEADER_ALLOWANCE, origin)
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
    fn robots_allows(&mut self, url: &url::Url, observed_asset: bool) -> Result<(), CaptureGap> {
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
                    let response = self.get(&next, ROBOTS_BYTES, None)?;
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
                        200..=299 if response_has_type(&response, &["text/plain"]) => {
                            String::from_utf8(response.body)
                                .map(Robots::Rules)
                                .map_err(|_| CaptureGap::RobotsUnavailable)
                        }
                        // Stricter handling of authentication/rate limits, rather than
                        // interpreting those operational walls as an absent policy.
                        404 | 410 => Ok(Robots::Rules(String::new())),
                        403 if machine_unavailable_robots(&response) => {
                            Ok(Robots::MachineUnavailable403)
                        }
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
            Some(Robots::MachineUnavailable403) if observed_asset => {
                // RFC 9309 permits retrieval after a 4xx unavailable robots
                // file. Limit this to typed, observed public assets, and keep
                // the reading rules explicitly unknown/reviewable.
                self.unavailable_robots_asset_origins.insert(origin);
                Ok(())
            }
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
    fn collect(
        &mut self,
        raw: &str,
        scope: Option<ResourceScope<'_>>,
    ) -> Result<CaptureOutcome, CaptureGap> {
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
            if scope
                .as_ref()
                .is_some_and(|scope| next.origin().ascii_serialization() != scope.origin)
            {
                return Err(
                    if scope.as_ref().is_some_and(|scope| scope.assets.is_some()) {
                        CaptureGap::AssetUnavailable
                    } else {
                        CaptureGap::EmbeddedDocumentUnavailable
                    },
                );
            }
            if !seen.insert(next.to_string()) {
                return Err(CaptureGap::RedirectLoop);
            }
            if !self.visited.contains(next.as_str()) && self.visited.len() >= self.budget.resources
            {
                return Err(CaptureGap::BudgetExceeded);
            }
            self.visited.insert(next.to_string());
            self.robots_allows(
                &next,
                scope.as_ref().is_some_and(|scope| scope.assets.is_some()),
            )?;
            let response = self.get(
                &next,
                self.budget.body_bytes,
                scope.as_ref().and_then(|scope| scope.request_origin),
            )?;
            let status = response.status;
            let size = response.body.len();
            if scope.as_ref().is_some_and(|scope| scope.assets.is_some()) {
                if self.asset_observations.len() < self.budget.resources {
                    self.asset_observations
                        .push(asset_observation(next.as_str(), &response));
                } else {
                    self.omitted_asset_observations =
                        self.omitted_asset_observations.saturating_add(1);
                }
            }
            let request_scope = scope
                .as_ref()
                .map_or(SnapshotScope::Ordinary, |scope| scope.request_scope);
            if crate::rehearsal_capture::snapshot_binding_scoped(&response, request_scope).is_err()
            {
                return Err(CaptureGap::RepresentationUnsupported);
            }
            let target = if is_redirect(status) {
                Some(redirect(&next, &response)?)
            } else {
                if let Some(scope) = scope.as_ref() {
                    let accepted = (200..=299).contains(&status)
                        && match scope.assets {
                            Some(kinds) => asset_response(&response, kinds, request_scope),
                            None => response_has_type(
                                &response,
                                &["text/html", "application/xhtml+xml"],
                            ),
                        };
                    if !accepted {
                        return Err(if scope.assets.is_some() {
                            CaptureGap::AssetUnavailable
                        } else {
                            CaptureGap::EmbeddedDocumentUnavailable
                        });
                    }
                }
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
            let document_origin =
                response_has_type(&response, &["text/html", "application/xhtml+xml"])
                    .then(|| {
                        url::Url::parse(&url)
                            .map(|u| u.origin().ascii_serialization())
                            .ok()
                    })
                    .flatten();
            if self.recorded.contains(&url) {
                continue;
            }
            let request_scope = scope
                .as_ref()
                .map_or(SnapshotScope::Ordinary, |scope| scope.request_scope);
            if builder.add_scoped(&url, response, request_scope).is_err() {
                self.failed = true;
                return Err(CaptureGap::ArchiveRejected);
            }
            if let Some(origin) = document_origin {
                self.document_origins.insert(origin);
            }
            self.recorded.insert(url);
        }
        if is_entry {
            self.entry_recorded = true;
        }
        Ok(CaptureOutcome::Recorded { status, body_bytes })
    }
    pub fn capture(&mut self, url: &str) -> CaptureOutcome {
        self.capture_scoped(url, None)
    }
    fn capture_scoped(&mut self, url: &str, scope: Option<ResourceScope<'_>>) -> CaptureOutcome {
        // Keep outcomes bounded even if a caller repeatedly proposes denied URLs.
        if self.records.len() >= self.budget.resources {
            self.omitted = self.omitted.saturating_add(1);
            return CaptureOutcome::NeedsReview {
                reason: CaptureGap::BudgetExceeded,
            };
        }
        let outcome = self
            .collect(url, scope)
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
            embedded_origins: self.embedded_origins.clone(),
            asset_origins: self.asset_origins.clone(),
            asset_origin_attempts: self.asset_attempts.len(),
            asset_observations: self.asset_observations.clone(),
            omitted_asset_observations: self.omitted_asset_observations,
            unavailable_robots_asset_origins: self.unavailable_robots_asset_origins.clone(),
            discovery: self.discovery.clone(),
            coverage: "unassessed; missing resources and server-dependent steps require review",
            server_validation: "not_performed",
            url_privacy: "known private keys and email-like references refused before transport; opaque identifiers unassessed",
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
fn response_mime(response: &CapturedResponse) -> Option<String> {
    let mut headers = response
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("content-type"));
    let field = headers.next()?;
    if headers.next().is_some() {
        return None;
    }
    let mime = std::str::from_utf8(&field.value)
        .ok()?
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase();
    let (kind, subtype) = mime.split_once('/')?;
    if mime.len() > 256
        || [kind, subtype].iter().any(|part| {
            part.is_empty()
                || *part == "*"
                || !part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&c))
        })
    {
        return None;
    }
    Some(mime)
}
fn response_has_type(response: &CapturedResponse, types: &[&str]) -> bool {
    response_mime(response)
        .is_some_and(|mime| types.iter().any(|kind| mime.eq_ignore_ascii_case(kind)))
}
fn machine_unavailable_robots(response: &CapturedResponse) -> bool {
    if response.status != 403
        || !response_has_type(response, &["application/xml", "text/xml"])
        || response.headers.iter().any(|h| {
            h.name.eq_ignore_ascii_case("www-authenticate")
                || h.name.eq_ignore_ascii_case("proxy-authenticate")
        })
    {
        return false;
    }
    // Public object stores commonly deny listing a missing robots object.
    // Recognize only the inert machine error, never an HTML/interactive wall.
    let Ok(body) = std::str::from_utf8(&response.body) else {
        return false;
    };
    let body = body.trim().to_ascii_lowercase();
    (body.starts_with("<?xml") || body.starts_with("<error>"))
        && body.contains("<error")
        && body.contains("<code>accessdenied</code>")
        && body.contains("</error>")
        && !["<!doctype", "<html", "<script", "captcha", "challenge"]
            .iter()
            .any(|marker| body.contains(marker))
}
fn asset_observation(url: &str, response: &CapturedResponse) -> AssetObservation {
    let mut fields = BTreeSet::new();
    let mut incomplete = false;
    let mut count = 0;
    for header in response
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("vary"))
    {
        count += 1;
        let Ok(value) = std::str::from_utf8(&header.value) else {
            incomplete = true;
            continue;
        };
        for field in value.split(',').map(str::trim) {
            if fields.len() >= 16
                || field.is_empty()
                || field.len() > 64
                || !field
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&c))
            {
                incomplete = true;
            } else {
                fields.insert(field.to_ascii_lowercase());
            }
        }
    }
    AssetObservation {
        url_sha256: waypoint_domain::sha256_hex(url.as_bytes()),
        status: response.status,
        body_bytes: response.body.len(),
        mime: response_mime(response),
        varies_on: fields.into_iter().collect(),
        vary_incomplete: incomplete || count > 1,
        framing: match response.framing {
            waypoint_net::BodyFraming::NoBody => "no_body",
            waypoint_net::BodyFraming::ContentLength => "content_length",
            waypoint_net::BodyFraming::Chunked => "chunked",
            waypoint_net::BodyFraming::ConnectionClose => "connection_close",
        },
    }
}
fn asset_response(
    response: &CapturedResponse,
    kinds: &BTreeSet<AssetKind>,
    scope: SnapshotScope,
) -> bool {
    // Use the archive's native request proof and variant contract. An absent
    // Asset context is never inferred from a response MIME alone.
    if crate::rehearsal_capture::snapshot_binding_scoped(response, scope).is_err() {
        return false;
    }
    let Some(mime) = response_mime(response) else {
        return false;
    };
    kinds.iter().any(|kind| match kind {
        AssetKind::Script => matches!(
            mime.as_str(),
            "text/javascript"
                | "application/javascript"
                | "text/ecmascript"
                | "application/ecmascript"
                | "application/x-javascript"
        ),
        AssetKind::Stylesheet => mime == "text/css",
        AssetKind::Font => {
            mime.starts_with("font/")
                || matches!(
                    mime.as_str(),
                    "application/font-woff"
                        | "application/font-sfnt"
                        | "application/vnd.ms-fontobject"
                        | "application/x-font-ttf"
                        | "application/octet-stream"
                )
        }
        AssetKind::Image => mime.starts_with("image/"),
        AssetKind::Json | AssetKind::Manifest => {
            mime == "application/json"
                || mime.starts_with("application/") && mime.ends_with("+json")
        }
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
    resource_url(target.as_str()).map_err(|reason| match reason {
        CaptureGap::PrivateUrl | CaptureGap::UrlPrivacyUncertain => reason,
        _ => CaptureGap::InvalidRedirect,
    })
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
        fn get(
            &mut self,
            url: &str,
            millis: u64,
            max: usize,
            origin: Option<&waypoint_net::PublicRequestOrigin>,
        ) -> Result<CapturedResponse, ()> {
            assert!((1..=8000).contains(&millis));
            assert!(max <= 8 * 1024 * 1024 + HEADER_ALLOWANCE);
            self.calls.lock().unwrap().push(url.into());
            let mut response = self
                .replies
                .get_mut(url)
                .and_then(VecDeque::pop_front)
                .ok_or(())?;
            if let Some(origin) = origin {
                response.request_origin = Some(origin.clone());
                response.request_header_names = Some(vec!["host".into(), "origin".into()]);
            }
            Ok(response)
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
            request_header_names: None,
            request_origin: None,
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
    fn observed_assets_are_typed_and_never_promote_their_origin_to_document_access() {
        const JS: &str = "https://cdn.fixture.invalid/app.js";
        const CSS: &str = "https://cdn.fixture.invalid/app.css";
        let mut h = harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (ENTRY, html()),
                (
                    "https://cdn.fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (
                    JS,
                    response(200, "console.log('fictional')", "text/javascript"),
                ),
                (CSS, response(200, "body{}", "text/css")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        h.session.capture_entry();
        for (url, kind) in [(JS, AssetKind::Script), (CSS, AssetKind::Stylesheet)] {
            let scope = h
                .session
                .asset_origin(url, &BTreeSet::from([kind]), 1, None, false)
                .unwrap();
            assert!(matches!(
                h.session.capture_asset(url, scope),
                CaptureOutcome::Recorded { .. }
            ));
            assert!(!h
                .session
                .sources
                .contains_key("https://cdn.fixture.invalid"));
        }
        let calls = h.calls.lock().unwrap().len();
        gap(
            h.session.capture("https://cdn.fixture.invalid/unobserved"),
            CaptureGap::OriginNotDeclared,
        );
        assert_eq!(h.calls.lock().unwrap().len(), calls);
        assert_eq!(h.session.report().asset_origin_attempts, 1);
        assert_eq!(h.session.report().asset_origins.len(), 1);
        assert_eq!(
            h.session.report().asset_origins[0].kinds,
            BTreeSet::from([AssetKind::Script, AssetKind::Stylesheet])
        );
        assert!(h.session.freeze().is_ok());
    }
    #[test]
    fn unsupported_optional_variants_stay_visible_without_poisoning_valid_resources() {
        const JSON: &str = "https://fixture.invalid/data";
        const CSS: &str = "https://fixture.invalid/app.css";
        let mut variant = response(200, "{}", "application/json");
        variant.headers.push(waypoint_net::Header {
            name: "Vary".into(),
            value: b"Origin".to_vec(),
        });
        let mut h = harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (ENTRY, html()),
                (JSON, variant),
                (CSS, response(200, "body{}", "text/css")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        h.session.capture_entry();
        let mut round = 0;
        let summary = h
            .session
            .discover_with_probe(3, |_, _| {
                round += 1;
                Ok(probe(if round == 1 { vec![JSON, CSS] } else { vec![] }))
            })
            .unwrap();
        assert_eq!(
            summary.stopped,
            crate::capture_discovery::DiscoveryStop::ReviewRequired
        );
        assert_eq!(h.session.report().recorded_resources, 2);
        assert!(h
            .session
            .report()
            .records
            .iter()
            .any(|record| record.outcome
                == CaptureOutcome::NeedsReview {
                    reason: CaptureGap::RepresentationUnsupported
                }));
        assert!(h.session.freeze().is_ok());
    }

    #[test]
    fn unavailable_machine_robots_is_asset_only_and_keeps_the_copy_in_review() {
        const CSS: &str = "https://cdn.fixture.invalid/app.css";
        let machine = || {
            response(
                403,
                "<?xml version=\"1.0\"?><Error><Code>AccessDenied</Code></Error>",
                "application/xml",
            )
        };
        let mut h = harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (ENTRY, html()),
                ("https://cdn.fixture.invalid/robots.txt", machine()),
                (CSS, response(200, "body{}", "text/css")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        h.session.capture_entry();
        let mut round = 0;
        let summary = h
            .session
            .discover_with_probe(3, |_, _| {
                round += 1;
                if round > 1 {
                    return Ok(probe(vec![]));
                }
                let mut value = probe(vec![CSS]);
                value
                    .assets
                    .insert(CSS.into(), BTreeSet::from([AssetKind::Stylesheet]));
                Ok(value)
            })
            .unwrap();
        assert_eq!(
            summary.stopped,
            crate::capture_discovery::DiscoveryStop::ReviewRequired
        );
        assert!(h
            .session
            .report()
            .unavailable_robots_asset_origins
            .contains("https://cdn.fixture.invalid"));
        assert_eq!(h.session.report().recorded_resources, 2);
        h.session.sources.insert(
            "https://cdn.fixture.invalid".into(),
            "user_supplied_url".into(),
        );
        gap(
            h.session.capture("https://cdn.fixture.invalid/document"),
            CaptureGap::RobotsUnavailable,
        );
        assert!(!h
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|url| url.ends_with("/document")));
        let mut entry = harness(
            vec![("https://fixture.invalid/robots.txt", machine())],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        gap(entry.session.capture_entry(), CaptureGap::RobotsUnavailable);
        assert_eq!(entry.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn authentication_rate_limits_interactive_and_unreachable_robots_do_not_use_asset_fallback() {
        const CSS: &str = "https://cdn.fixture.invalid/app.css";
        let mut authenticated = response(
            403,
            "<Error><Code>AccessDenied</Code></Error>",
            "application/xml",
        );
        authenticated.headers.push(waypoint_net::Header {
            name: "WWW-Authenticate".into(),
            value: b"Basic realm=fictional".to_vec(),
        });
        for unavailable in [
            authenticated,
            response(403, "<html>Human verification</html>", "text/html"),
            response(
                403,
                "<Error><Code>CaptchaRequired</Code></Error>",
                "application/xml",
            ),
            response(
                401,
                "<Error><Code>AccessDenied</Code></Error>",
                "application/xml",
            ),
            response(
                429,
                "<Error><Code>AccessDenied</Code></Error>",
                "application/xml",
            ),
            response(
                503,
                "<Error><Code>AccessDenied</Code></Error>",
                "application/xml",
            ),
        ] {
            let mut h = harness(
                vec![
                    (
                        "https://fixture.invalid/robots.txt",
                        robots("User-agent: *\nAllow: /"),
                    ),
                    (ENTRY, html()),
                    ("https://cdn.fixture.invalid/robots.txt", unavailable),
                ],
                "user_supplied_url",
                CaptureBudget::default(),
            );
            h.session.capture_entry();
            let scope = h
                .session
                .asset_origin(
                    CSS,
                    &BTreeSet::from([AssetKind::Stylesheet]),
                    1,
                    None,
                    false,
                )
                .unwrap();
            gap(
                h.session.capture_asset(CSS, scope),
                CaptureGap::RobotsUnavailable,
            );
            assert!(h
                .session
                .report()
                .unavailable_robots_asset_origins
                .is_empty());
            assert!(!h.calls.lock().unwrap().iter().any(|url| url == CSS));
        }
    }

    #[test]
    fn asset_observations_are_bounded_metadata_without_bodies_urls_or_header_values() {
        let mut value = response(200, "fictional-private-body", "text/css");
        value.headers.push(waypoint_net::Header {
            name: "Vary".into(),
            value: b"Origin, Accept-Encoding".to_vec(),
        });
        value.headers.push(waypoint_net::Header {
            name: "X-Fictional-Private".into(),
            value: b"fictional-private-header".to_vec(),
        });
        let observed = asset_observation("https://cdn.fixture.invalid/private-path", &value);
        assert_eq!(observed.status, 200);
        assert_eq!(observed.mime.as_deref(), Some("text/css"));
        assert_eq!(observed.varies_on, ["accept-encoding", "origin"]);
        assert!(!observed.vary_incomplete);
        let encoded = serde_json::to_string(&observed).unwrap();
        assert!(!encoded.contains("fictional-private") && !encoded.contains("private-path"));
        value.headers.push(waypoint_net::Header {
            name: "Vary".into(),
            value: (0..30)
                .map(|n| format!("x-{n}"))
                .collect::<Vec<_>>()
                .join(",")
                .into_bytes(),
        });
        let observed = asset_observation("https://cdn.fixture.invalid/a", &value);
        assert!(observed.vary_incomplete);
        assert_eq!(observed.varies_on.len(), 16);
    }

    #[test]
    fn asset_html_redirects_robots_and_policy_cannot_expand_the_scope() {
        const BAD: &str = "https://cdn.fixture.invalid/not-script";
        const MOVED: &str = "https://cdn.fixture.invalid/redirect";
        const DENIED: &str = "https://denied.fixture.invalid/app.js";
        let mut h = harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (ENTRY, html()),
                (
                    "https://cdn.fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (BAD, html()),
                (MOVED, moved("https://unobserved.fixture.invalid/app.js")),
                (
                    "https://denied.fixture.invalid/robots.txt",
                    robots("User-agent: *\nDisallow: /"),
                ),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        h.session.capture_entry();
        let kinds = BTreeSet::from([AssetKind::Script]);
        for url in [BAD, MOVED] {
            let scope = h.session.asset_origin(url, &kinds, 1, None, false).unwrap();
            gap(
                h.session.capture_asset(url, scope),
                CaptureGap::AssetUnavailable,
            );
        }
        let scope = h
            .session
            .asset_origin(DENIED, &kinds, 1, None, false)
            .unwrap();
        gap(
            h.session.capture_asset(DENIED, scope),
            CaptureGap::RobotsDenied,
        );
        assert!(h
            .session
            .asset_origin("https://WWW.LINKEDIN.COM./app.js", &kinds, 1, None, false)
            .is_none());
        assert!(!h
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|url| url.contains("unobserved") || url.contains("LINKEDIN") || url == DENIED));
        assert!(h.session.report().asset_origins.is_empty());
        assert!(h.session.freeze().is_ok());
    }

    #[test]
    fn asset_origin_budget_counts_admissions_before_fetch_and_keeps_old_scopes_usable() {
        let mut h = ready();
        let kinds = BTreeSet::from([AssetKind::Script]);
        assert!(h
            .session
            .asset_origin(
                "https://unobserved.fixture.invalid/app.js",
                &kinds,
                1,
                None,
                false
            )
            .is_none());
        h.session.capture_entry();
        for index in 0..MAX_ASSET_ORIGINS {
            assert!(h
                .session
                .asset_origin(
                    &format!("https://cdn{index}.fixture.invalid/app.js"),
                    &kinds,
                    1,
                    None,
                    false
                )
                .is_some());
        }
        assert!(h
            .session
            .asset_origin(
                "https://extra.fixture.invalid/app.js",
                &kinds,
                1,
                None,
                false
            )
            .is_none());
        assert!(h
            .session
            .asset_origin(
                "https://cdn0.fixture.invalid/other.js",
                &kinds,
                2,
                None,
                false
            )
            .is_some());
        assert_eq!(h.session.report().asset_origin_attempts, MAX_ASSET_ORIGINS);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn observed_asset_mime_types_are_not_interchangeable_or_origin_independent_by_assumption() {
        for (mime, kind, accepted) in [
            ("application/javascript", AssetKind::Script, true),
            ("text/plain", AssetKind::Script, false),
            ("text/css; charset=UTF-8", AssetKind::Stylesheet, true),
            ("application/json", AssetKind::Stylesheet, false),
            ("font/woff2", AssetKind::Font, true),
            ("image/svg+xml", AssetKind::Image, true),
            ("application/problem+json", AssetKind::Json, true),
            ("application/manifest+json", AssetKind::Manifest, true),
            ("text/html", AssetKind::Image, false),
            ("image/*", AssetKind::Image, false),
        ] {
            assert_eq!(
                asset_response(
                    &response(200, "fictional", mime),
                    &BTreeSet::from([kind]),
                    SnapshotScope::Ordinary
                ),
                accepted,
                "{mime}"
            );
        }
        let mut changed = response(200, "fictional", "text/css");
        changed.headers.push(waypoint_net::Header {
            name: "Vary".into(),
            value: b"Origin".to_vec(),
        });
        assert!(!asset_response(
            &changed,
            &BTreeSet::from([AssetKind::Stylesheet]),
            SnapshotScope::Ordinary
        ));
        changed.headers.pop();
        changed.headers.push(waypoint_net::Header {
            name: "Content-Type".into(),
            value: b"text/css".to_vec(),
        });
        assert!(!asset_response(
            &changed,
            &BTreeSet::from([AssetKind::Stylesheet]),
            SnapshotScope::Ordinary
        ));
    }

    #[test]
    fn fresh_asset_kind_evidence_can_repair_a_previous_mime_refusal_without_repeating_it() {
        const CSS: &str = "https://cdn.fixture.invalid/app.css";
        const BOOT: &str = "https://fixture.invalid/bootstrap.js";
        let mut h = harness(
            vec![
                (
                    "https://fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (ENTRY, html()),
                (
                    BOOT,
                    response(200, "/* fictional bootstrap */", "text/javascript"),
                ),
                (
                    "https://cdn.fixture.invalid/robots.txt",
                    robots("User-agent: *\nAllow: /"),
                ),
                (CSS, response(200, "body{}", "text/css")),
                (CSS, response(200, "body{}", "text/css")),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        h.session.capture_entry();
        let mut round = 0;
        let report = h
            .session
            .discover_with_probe(4, |_, _| {
                round += 1;
                if round > 2 {
                    return Ok(probe(vec![]));
                }
                let mut result = probe(if round == 1 {
                    vec![CSS, BOOT]
                } else {
                    vec![CSS]
                });
                result.assets.insert(
                    CSS.into(),
                    BTreeSet::from([if round == 1 {
                        AssetKind::Image
                    } else {
                        AssetKind::Stylesheet
                    }]),
                );
                Ok(result)
            })
            .unwrap();
        assert_eq!(report.rounds[0].review_outcomes, 1);
        assert_eq!(report.rounds[1].newly_recorded, 1);
        assert_eq!(
            h.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|url| url.as_str() == CSS)
                .count(),
            2
        );
        assert_eq!(
            h.session.report().asset_origins[0].kinds,
            BTreeSet::from([AssetKind::Stylesheet])
        );
        assert!(h.session.freeze().is_ok());
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
    fn private_references_are_refused_even_at_the_transport_boundary() {
        for raw in [
            "https://fixture.invalid/?access_token=fictional-private-marker",
            "https://fixture.invalid/avery%2540fixture.invalid",
            "https://fixture.invalid/?target=https%3A%2F%2Ffixture.invalid%2F%3Femail%3Dhidden",
        ] {
            let mut h = ready();
            gap(h.session.capture(raw), CaptureGap::PrivateUrl);
            assert_eq!(
                h.session
                    .get(&url::Url::parse(raw).unwrap(), 1024, None)
                    .unwrap_err(),
                CaptureGap::PrivateUrl
            );
            assert!(h.calls.lock().unwrap().is_empty());
            let report = serde_json::to_string(&h.session.report()).unwrap();
            assert!(!report.contains("fictional-private-marker"));
            assert!(!report.contains("avery"));
        }
    }
    #[test]
    fn private_resource_and_robots_redirects_stop_before_the_target_read() {
        for from_robots in [false, true] {
            let mut redirected = response(302, "public redirect", "text/plain");
            redirected.headers.push(waypoint_net::Header {
                name: "Location".into(),
                value: b"/flow?t%256fken=fictional-private-marker".to_vec(),
            });
            let rows = if from_robots {
                vec![("https://fixture.invalid/robots.txt", redirected)]
            } else {
                vec![
                    ("https://fixture.invalid/robots.txt", robots("")),
                    (ENTRY, redirected),
                ]
            };
            let mut h = harness(rows, "user_supplied_url", CaptureBudget::default());
            gap(h.session.capture_entry(), CaptureGap::PrivateUrl);
            assert_eq!(
                h.calls.lock().unwrap().len(),
                if from_robots { 1 } else { 2 }
            );
            assert!(h.session.freeze().is_err());
            assert!(!h.folder.path().join("capture/replay.json").exists());
        }
    }
    #[test]
    fn discovery_keeps_privacy_refusals_visible_without_fetching_them() {
        let mut h = ready();
        let summary = h
            .session
            .discover_with_probe(4, |_, _| {
                Ok(probe(vec![
                    "https://fixture.invalid/assets?user[email]=fictional-private-marker",
                    "https://fixture.invalid/bootstrap?session_id=fictional-private-marker",
                ]))
            })
            .unwrap();
        assert_eq!(
            summary.stopped,
            crate::capture_discovery::DiscoveryStop::ReviewRequired
        );
        assert_eq!(summary.rounds.len(), 1);
        assert_eq!(summary.rounds[0].review_outcomes, 2);
        assert_eq!(h.calls.lock().unwrap().len(), 2);
        let frozen = h.session.freeze().unwrap();
        assert_eq!(frozen.report.recorded_resources, 1);
        assert!(frozen.report.records.iter().skip(1).all(|r| r.outcome
            == CaptureOutcome::NeedsReview {
                reason: CaptureGap::PrivateUrl
            }));
        assert!(!serde_json::to_string(&frozen.report)
            .unwrap()
            .contains("fictional-private-marker"));
    }
    #[test]
    fn later_discovery_failure_does_not_reuse_an_earlier_control_inventory() {
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, html()),
                (
                    "https://fixture.invalid/app.js",
                    response(200, "// public asset", "text/javascript"),
                ),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        let mut calls = 0;
        let summary=h.session.discover_with_probe(3,|_,_|{
            calls+=1;
            if calls>1{return Err("browser unavailable".into());}
            let mut first=probe(vec!["https://fixture.invalid/app.js"]);
            first.form_outline=Some(serde_json::from_value(serde_json::json!({"schema":1,"scope":"root_frame_accessibility_tree","status":"observed","reason":null,"nodes":1,"controls":[],"omitted_controls":0,"unlabelled_controls":0,"omitted_texts":0})).unwrap());
            first.outline_snapshot_sha256=Some("a".repeat(64));
            Ok(first)
        }).unwrap();
        assert_eq!(
            summary.stopped,
            crate::capture_discovery::DiscoveryStop::BrowserFailed
        );
        assert!(summary.form_outline.is_none());
        assert!(summary.form_outline_snapshot_sha256.is_none());
        assert!(summary.form_outline_round.is_none());
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

    #[test]
    fn large_script_uses_bounded_resource_allowance_and_smaller_explicit_caps_still_stop() {
        const ASSET: &str = "https://cdn.fixture.invalid/app.js";
        let body = format!("/*{}*/", "x".repeat(4 * 1024 * 1024));
        for (cap, accepted) in [(4 * 1024 * 1024, false), (8 * 1024 * 1024, true)] {
            let mut h = harness(
                vec![
                    ("https://fixture.invalid/robots.txt", robots("")),
                    (ENTRY, html()),
                    ("https://cdn.fixture.invalid/robots.txt", robots("")),
                    (ASSET, response(200, &body, "text/javascript")),
                ],
                "user_supplied_url",
                CaptureBudget {
                    body_bytes: cap,
                    ..CaptureBudget::default()
                },
            );
            assert!(matches!(
                h.session.capture_entry(),
                CaptureOutcome::Recorded { .. }
            ));
            let scope = h
                .session
                .asset_origin(ASSET, &BTreeSet::from([AssetKind::Script]), 1, None, false)
                .unwrap();
            let outcome = h.session.capture_asset(ASSET, scope);
            if accepted {
                assert!(matches!(outcome, CaptureOutcome::Recorded { .. }));
            } else {
                assert_eq!(
                    outcome,
                    CaptureOutcome::NeedsReview {
                        reason: CaptureGap::BudgetExceeded
                    }
                );
            }
            assert!(h.session.freeze().is_ok());
        }
    }

    #[test]
    fn captured_asset_origin_is_bound_to_an_admitted_document_and_sent_to_transport() {
        const ASSET: &str = "https://cdn.fixture.invalid/bootstrap.json";
        let mut asset = response(200, "{}", "application/json");
        asset.headers.push(waypoint_net::Header {
            name: "Vary".into(),
            value: b"Origin, Access-Control-Request-Headers, Access-Control-Request-Method"
                .to_vec(),
        });
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, html()),
                ("https://cdn.fixture.invalid/robots.txt", robots("")),
                (ASSET, asset),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        assert!(matches!(
            h.session.capture_entry(),
            CaptureOutcome::Recorded { .. }
        ));
        let kinds = BTreeSet::from([AssetKind::Json]);
        assert!(h
            .session
            .asset_origin(ASSET, &kinds, 1, Some("https://unadmitted.invalid"), false)
            .is_none());
        let scope = h
            .session
            .asset_origin(ASSET, &kinds, 1, Some(ORIGIN), false)
            .unwrap();
        let outcome = h.session.capture_asset(ASSET, scope);
        assert!(
            matches!(outcome, CaptureOutcome::Recorded { .. }),
            "{outcome:?}"
        );
        let frozen = h.session.freeze().unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(frozen.snapshot.root.join("replay.json")).unwrap(),
        )
        .unwrap();
        let row = manifest["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["url"] == ASSET)
            .unwrap();
        assert_eq!(row["request_origin"], ORIGIN);
        assert_eq!(row["request_preflight_absent"], true);
        assert!(row.get("request_origin_absent").is_none());
    }

    #[test]
    fn newly_observed_style_absence_can_repair_an_unsupported_variant_once() {
        const CSS: &str = "https://fixture.invalid/style.css";
        const JS: &str = "https://fixture.invalid/app.js";
        for native_proof in [true, false] {
            let css = || {
                let mut value = response(200, "body{}", "text/css");
                value.headers.push(waypoint_net::Header {
                    name: "Vary".into(),
                    value: b"Origin".to_vec(),
                });
                if native_proof {
                    value.request_header_names = Some(vec!["host".into(), "accept".into()]);
                }
                value
            };
            let mut h = harness(
                vec![
                    ("https://fixture.invalid/robots.txt", robots("")),
                    (ENTRY, html()),
                    (CSS, css()),
                    (CSS, css()),
                    (JS, response(200, "void 0", "text/javascript")),
                ],
                "user_supplied_url",
                CaptureBudget::default(),
            );
            let mut round = 0;
            let summary = h
                .session
                .discover_with_probe(4, |_, _| {
                    round += 1;
                    let mut p = probe(if round == 1 { vec![CSS, JS] } else { vec![CSS] });
                    p.assets
                        .insert(CSS.into(), BTreeSet::from([AssetKind::Stylesheet]));
                    if round == 1 {
                        p.assets
                            .insert(JS.into(), BTreeSet::from([AssetKind::Script]));
                    } else {
                        p.asset_origin_absent_stylesheets.insert(CSS.into());
                    }
                    Ok(p)
                })
                .unwrap();
            assert_eq!(
                summary.stopped,
                crate::capture_discovery::DiscoveryStop::ReviewRequired
            );
            assert_eq!(
                h.calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|url| *url == CSS)
                    .count(),
                2
            );
            let frozen = h.session.freeze().unwrap();
            let manifest: serde_json::Value = serde_json::from_slice(
                &std::fs::read(frozen.snapshot.root.join("replay.json")).unwrap(),
            )
            .unwrap();
            let style = manifest["resources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["url"] == CSS);
            if native_proof {
                assert_eq!(style.unwrap()["request_origin_absent_stylesheet"], true);
            } else {
                assert!(style.is_none());
            }
        }
    }

    #[test]
    fn unadmitted_probe_origin_cannot_fall_back_to_an_originless_public_fetch() {
        const ASSET: &str = "https://fixture.invalid/bootstrap.json";
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, html()),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        let summary = h
            .session
            .discover_with_probe(1, |snapshot, _| {
                assert!(snapshot.root.is_dir());
                let mut p = probe(vec![ASSET]);
                p.assets
                    .insert(ASSET.into(), BTreeSet::from([AssetKind::Json]));
                p.asset_request_origins
                    .insert(ASSET.into(), "https://unadmitted.invalid".into());
                Ok(p)
            })
            .unwrap();
        assert!(matches!(
            summary.stopped,
            crate::capture_discovery::DiscoveryStop::BrowserFailed
        ));
        assert!(!h.calls.lock().unwrap().iter().any(|url| url == ASSET));
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
    fn only_observed_html_frames_add_policy_bound_origins() {
        const LEVER: &str = "https://jobs.lever.co/fictional/job/apply";
        const ASHBY: &str = "https://jobs.ashbyhq.com/fictional/job/application";
        const GREENHOUSE: &str = "https://boards.greenhouse.io/fictional/jobs/123";
        const EXTRA: &str = "https://job-boards.greenhouse.io/fictional/jobs/123";
        const PRIVATE: &str =
            "https://jobs.lever.co/fictional/job/apply?email=test%40fixture.invalid";
        const TRACKER: &str = "https://jobs.lever.co.fake.invalid/frame";
        const FIRST_ASSET: &str = "https://fixture.invalid/initial.js";
        const ASSET: &str = "https://jobs.lever.co/asset.css";
        const NO_SCOPE: &str = "https://jobs.ashbyhq.com/asset.css";
        let mut h = harness(
            vec![
                ("https://fixture.invalid/robots.txt", robots("")),
                (ENTRY, html()),
                (FIRST_ASSET, response(200, "void 0", "text/javascript")),
                ("https://jobs.lever.co/robots.txt", robots("")),
                (LEVER, html()),
                (ASSET, response(200, "body{}", "text/css")),
                ("https://jobs.ashbyhq.com/robots.txt", robots("")),
                (ASHBY, response(200, "not html", "text/plain")),
                ("https://boards.greenhouse.io/robots.txt", robots("")),
                (GREENHOUSE, html()),
            ],
            "user_supplied_url",
            CaptureBudget::default(),
        );
        let mut round = 0;
        h.session
            .discover_with_probe(4, |_, _| {
                round += 1;
                let mut report = match round {
                    1 => probe(vec![LEVER, ASHBY, PRIVATE, TRACKER, NO_SCOPE, FIRST_ASSET]),
                    2 => probe(vec![LEVER, ASSET, GREENHOUSE]),
                    3 => probe(vec![EXTRA]),
                    _ => probe(vec![]),
                };
                report.child_documents.extend(
                    match round {
                        1 => vec![ASHBY, PRIVATE, TRACKER],
                        2 => vec![LEVER, GREENHOUSE],
                        3 => vec![EXTRA],
                        _ => vec![],
                    }
                    .into_iter()
                    .map(str::to_owned),
                );
                Ok(report)
            })
            .unwrap();
        let report = h.session.report();
        assert_eq!(report.embedded_origins.len(), 2);
        assert!(report
            .embedded_origins
            .iter()
            .all(|scope| scope.discovery_round == 2));
        assert!(report.records.iter().any(|r| r.outcome
            == CaptureOutcome::NeedsReview {
                reason: CaptureGap::EmbeddedDocumentUnavailable,
            }));
        assert_eq!(
            *h.calls.lock().unwrap(),
            [
                "https://fixture.invalid/robots.txt",
                ENTRY,
                "https://jobs.ashbyhq.com/robots.txt",
                ASHBY,
                FIRST_ASSET,
                "https://boards.greenhouse.io/robots.txt",
                GREENHOUSE,
                "https://jobs.lever.co/robots.txt",
                LEVER,
                ASSET,
            ]
        );
    }
    #[test]
    fn embedded_origin_robots_and_document_redirects_remain_scoped() {
        const FRAME: &str = "https://jobs.lever.co/fictional/job/apply";
        for (rules, reason, count) in [
            ("User-agent: *\nDisallow: /", CaptureGap::RobotsDenied, 3),
            ("", CaptureGap::EmbeddedDocumentUnavailable, 4),
        ] {
            let mut h = harness(
                vec![
                    ("https://fixture.invalid/robots.txt", robots("")),
                    (ENTRY, html()),
                    ("https://jobs.lever.co/robots.txt", robots(rules)),
                    (FRAME, moved("https://fixture.invalid/other")),
                ],
                "user_supplied_url",
                CaptureBudget::default(),
            );
            h.session
                .discover_with_probe(1, |_, _| {
                    let mut report = probe(vec![FRAME]);
                    report.child_documents.insert(FRAME.into());
                    Ok(report)
                })
                .unwrap();
            let report = h.session.report();
            assert!(report.embedded_origins.is_empty());
            assert_eq!(
                report.records.last().unwrap().outcome,
                CaptureOutcome::NeedsReview { reason }
            );
            assert_eq!(h.calls.lock().unwrap().len(), count);
        }
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
