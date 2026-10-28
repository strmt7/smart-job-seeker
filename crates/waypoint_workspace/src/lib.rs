//! End-to-end orchestration (the wire between the verified components).
//!
//! Before this crate the platform had correct parts that were never connected:
//! connectors could parse, the store could persist, search could filter — but
//! nothing ran discovery → dedup → store → ranked search as one operation.
//! This is that operation, and it is where the product's honesty rules become
//! observable behaviour:
//!
//! - A source that policy forbids, a fetch that fails, a parser that drifts and
//!   a budget that is exhausted are each reported as *degraded coverage with a
//!   reason* — never as "0 jobs found".
//! - URLs arriving inside third-party payloads are re-validated before storage.
//! - Re-discovery never regresses an application the user already advanced.
//! - Strict constraint results and the excluded bucket are returned separately,
//!   so relaxing filters never contaminates the strict search (D3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use waypoint_connectors::{
    discovery_plan, is_public_host_url, parse_ashby, parse_greenhouse, parse_jobicy, parse_lever,
    RawPosting,
};
use waypoint_domain::{sha256_hex, ApplicationState, DomainError, JobIdentityId};
use waypoint_inference::ResourceBudgets;
use waypoint_policy::PermissionRegistry;
use waypoint_search::coverage::{CoverageReport, SourceFetchState, SourceStatus};
use waypoint_search::{
    check_constraints, dedup_decision, evidence_overlap, freshness, identity_key,
    CampaignConstraints, ConstraintOutcome, DedupDecision, Freshness,
};
use waypoint_store::{Store, StoreError, StoredJob};

/// A source-level failure reason, kept as data so the UI can explain itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchFailure {
    /// HTTP 401/403/404 — the board is private, gone, or requires auth.
    AccessDenied(String),
    /// Transport/protocol failure, timeout, TLS, DNS.
    Network(String),
}

pub trait Fetcher: Send + Sync {
    fn get(&self, url: &str) -> Result<String, FetchFailure>;
}

/// Real fetcher: HTTPS with OS-native TLS, DNS-rebinding guard, size cap.
#[derive(Debug, Clone)]
pub struct LiveFetcher {
    pub timeout_ms: u64,
}

impl Default for LiveFetcher {
    fn default() -> Self {
        Self { timeout_ms: 20_000 }
    }
}

impl Fetcher for LiveFetcher {
    fn get(&self, url: &str) -> Result<String, FetchFailure> {
        match waypoint_net::get(url, self.timeout_ms) {
            Ok(r) if (200..300).contains(&r.status) => Ok(r.text()),
            Ok(r) if matches!(r.status, 401 | 403 | 404) => {
                Err(FetchFailure::AccessDenied(format!("http {}", r.status)))
            }
            Ok(r) => Err(FetchFailure::Network(format!("http {}", r.status))),
            Err(e) => Err(FetchFailure::Network(e.to_string())),
        }
    }
}

/// Fixture fetcher for tests and offline demos: exact URL → body.
#[derive(Debug, Clone, Default)]
pub struct StaticFetcher {
    pub routes: HashMap<String, String>,
    /// URLs recorded as expected-but-absent, to prove degradation reporting.
    pub failures: HashMap<String, FetchFailure>,
}

impl StaticFetcher {
    pub fn route(mut self, url: &str, body: &str) -> Self {
        self.routes.insert(url.to_string(), body.to_string());
        self
    }

    pub fn failing(mut self, url: &str, failure: FetchFailure) -> Self {
        self.failures.insert(url.to_string(), failure);
        self
    }
}

impl Fetcher for StaticFetcher {
    fn get(&self, url: &str) -> Result<String, FetchFailure> {
        if let Some(f) = self.failures.get(url) {
            return Err(f.clone());
        }
        self.routes
            .get(url)
            .cloned()
            .ok_or_else(|| FetchFailure::Network(format!("no fixture route for {url}")))
    }
}

/// What to discover, after the registry has been consulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryTarget {
    pub source: String,
    pub board: String,
}

impl DiscoveryTarget {
    pub fn new(source: &str, board: &str) -> Self {
        Self {
            source: source.to_string(),
            board: board.to_string(),
        }
    }
}

/// Outcome of one discovery run. Every counter is a real measurement.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryOutcome {
    pub coverage: CoverageReport,
    pub fetched: usize,
    pub parsed: usize,
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    /// Postings dropped because their URL failed the public-host guard.
    pub rejected_urls: usize,
    /// Sources whose payload did not match the pinned adapter (T034 drift).
    pub drift: usize,
    /// Sources whose fetch was refused by the bounded-worker budget.
    pub budget_refused: usize,
}

impl DiscoveryOutcome {
    /// One-line truth for the UI, always naming a denominator.
    pub fn summary(&self) -> String {
        format!(
            "{} new, {} updated, {} unchanged from {} sources ({} fetched, {} parsed); {} degraded, {} drift, {} budget-refused, {} untrusted URLs dropped",
            self.inserted,
            self.updated,
            self.unchanged,
            self.coverage.sources.len(),
            self.fetched,
            self.parsed,
            self.coverage
                .sources
                .iter()
                .filter(|s| !matches!(s.state, SourceFetchState::Ok | SourceFetchState::Empty))
                .count(),
            self.drift,
            self.budget_refused,
            self.rejected_urls
        )
    }
}

/// Strict results and the excluded bucket, returned separately (D3).
#[derive(Debug, Clone, Default)]
pub struct RankedResults {
    /// Jobs passing every hard constraint, best first.
    pub strict: Vec<RankedJob>,
    /// Jobs failing at least one hard constraint, with the reasons, so the
    /// candidate can choose to relax a filter without rewriting it.
    pub excluded: Vec<ExcludedJob>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedJob {
    pub job: StoredJob,
    pub description_available: bool,
    /// Fraction of the candidate's skills evidenced in the posting text.
    pub score: f32,
    pub matched_skills: Vec<String>,
    pub freshness: Freshness,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExcludedJob {
    pub job: StoredJob,
    pub reasons: Vec<String>,
}

#[derive(Debug)]
pub enum WorkspaceError {
    Store(StoreError),
    Domain(DomainError),
    Io(std::io::Error),
    /// The requested application-state change is not a legal transition.
    IllegalTransition {
        from: ApplicationState,
        to: ApplicationState,
    },
    /// No such job identity in the workspace.
    NotFound(String),
    /// A fact must have text.
    EmptyFact,
    /// The referenced fact does not exist.
    UnknownFact(String),
    /// A packet must exist before it can be approved or exported.
    PacketRequired(String),
    /// The packet cited a fact that has since been corrected.
    PacketInvalidated(String),
    /// Approval was requested while spans remain unsupported.
    UnsupportedClaims(Vec<String>),
    /// An approval could not be created or spent.
    Approval(String),
    /// Export or document serialization failed.
    Export(String),
    /// The append-only submission journal refused a transition.
    Journal(String),
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::Store(e) => write!(f, "store: {e}"),
            WorkspaceError::Domain(e) => write!(f, "domain: {e}"),
            WorkspaceError::Io(e) => write!(f, "io: {e}"),
            WorkspaceError::IllegalTransition { from, to } => {
                write!(
                    f,
                    "illegal application-state transition: {from:?} -> {to:?}"
                )
            }
            WorkspaceError::NotFound(id) => write!(f, "no such job: {id}"),
            WorkspaceError::EmptyFact => write!(f, "a fact must have text"),
            WorkspaceError::UnknownFact(id) => write!(f, "no such fact: {id}"),
            WorkspaceError::PacketRequired(id) => {
                write!(f, "no prepared packet exists for {id}")
            }
            WorkspaceError::PacketInvalidated(id) => write!(
                f,
                "packet for {id} cites a fact that changed; re-prepare before approving"
            ),
            WorkspaceError::UnsupportedClaims(reasons) => write!(
                f,
                "cannot approve: {} unsupported item(s): {}",
                reasons.len(),
                reasons.join("; ")
            ),
            WorkspaceError::Approval(m) => write!(f, "approval: {m}"),
            WorkspaceError::Export(m) => write!(f, "export: {m}"),
            WorkspaceError::Journal(m) => write!(f, "submission journal refused: {m}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<StoreError> for WorkspaceError {
    fn from(e: StoreError) -> Self {
        WorkspaceError::Store(e)
    }
}

impl From<DomainError> for WorkspaceError {
    fn from(e: DomainError) -> Self {
        WorkspaceError::Domain(e)
    }
}

impl From<std::io::Error> for WorkspaceError {
    fn from(e: std::io::Error) -> Self {
        WorkspaceError::Io(e)
    }
}

/// Default freshness window for a job observation.
pub const STALE_AFTER_SECONDS: i64 = 72 * 3600;

pub struct Workspace {
    store: Store,
    registry: PermissionRegistry,
    budgets: ResourceBudgets,
    coverage: CoverageReport,
    last_outcome: Option<DiscoveryOutcome>,
    pub db_path: Option<PathBuf>,
    sealed_now: usize,
    database_encrypted: bool,
    legacy_plaintext: bool,
    _workspace_lock: Option<std::fs::File>,
}

#[cfg(feature = "sqlcipher")]
mod encrypted;

fn lock_workspace(dir: &Path) -> Result<std::fs::File, WorkspaceError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".workspace.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
        WorkspaceError::Export("Workspace is already open by another process".into())
    })?;
    Ok(file)
}

impl Workspace {
    /// Open (creating directories + migrating) a workspace on disk.
    ///
    /// The candidate's own material (facts and prepared documents) is sealed at
    /// rest with the best backend the platform offers — on Windows that is
    /// DPAPI, keyed to the logged-in user, so there is no password for anyone
    /// to manage or for us to store. If no backend exists, the workspace still
    /// works and says so plainly rather than implying protection it does not
    /// have. Any rows written before sealing was available are migrated here.
    pub fn open(dir: &Path) -> Result<Self, WorkspaceError> {
        std::fs::create_dir_all(dir)?;
        let workspace_lock = lock_workspace(dir)?;
        if dir.join("jobs.encrypted.sqlite3").exists() {
            return Err(WorkspaceError::Export(
                "An encrypted workspace already exists here; use a SQLCipher-enabled build".into(),
            ));
        }
        let db_path = dir.join("jobs.sqlite3");
        let mut store = Store::open(&db_path)?;
        let mut sealed_now = 0usize;
        if let Some(sealer) = waypoint_seal::platform_sealer() {
            store = store.with_sealer(sealer);
            sealed_now = store.seal_existing_rows()?;
        }
        Ok(Self {
            store,
            registry: default_registry(),
            budgets: ResourceBudgets::default(),
            coverage: CoverageReport::default(),
            last_outcome: None,
            db_path: Some(db_path),
            sealed_now,
            database_encrypted: false,
            legacy_plaintext: false,
            _workspace_lock: Some(workspace_lock),
        })
    }

    /// How the candidate's own material is protected at rest, in words they can
    /// act on. Never overstated: with no backend it says so.
    pub fn sealing_status(&self) -> String {
        if self.database_encrypted {
            return if self.legacy_plaintext {
                "Active database encrypted with SQLCipher; an older plaintext database remains in this folder.".into()
            } else {
                "Database encrypted with SQLCipher; key protected by Windows DPAPI".into()
            };
        }
        match self.store.sealing_kind() {
            Some(kind) => kind.label().to_string(),
            None => "NOT encrypted: no sealing backend is available on this platform".to_string(),
        }
    }

    /// How many rows were migrated to sealed storage the last time this
    /// workspace was opened (0 when nothing needed it).
    pub fn rows_sealed_on_open(&self) -> usize {
        self.sealed_now
    }

    /// In-memory workspace for tests and dry runs.
    pub fn in_memory() -> Result<Self, WorkspaceError> {
        Ok(Self {
            store: Store::in_memory()?,
            registry: default_registry(),
            budgets: ResourceBudgets::default(),
            coverage: CoverageReport::default(),
            last_outcome: None,
            db_path: None,
            sealed_now: 0,
            database_encrypted: false,
            legacy_plaintext: false,
            _workspace_lock: None,
        })
    }

    /// Override the bounded-worker budgets (used by the budget-refusal test).
    pub fn with_budgets(mut self, budgets: ResourceBudgets) -> Self {
        self.budgets = budgets;
        self
    }

    pub fn coverage(&self) -> &CoverageReport {
        &self.coverage
    }

    pub fn last_outcome(&self) -> Option<&DiscoveryOutcome> {
        self.last_outcome.as_ref()
    }

    pub fn jobs(&self) -> Result<Vec<StoredJob>, WorkspaceError> {
        Ok(self.store.all_jobs()?)
    }

    /// Store an applicant-supplied document using the workspace sealer.
    pub fn add_document(
        &self,
        id: &str,
        name: &str,
        data: &[u8],
        timestamp: i64,
    ) -> Result<(), WorkspaceError> {
        self.store.add_document(id, name, data, timestamp)?;
        Ok(())
    }

    pub fn documents(
        &self,
    ) -> Result<Vec<waypoint_store::documents::DocumentInfo>, WorkspaceError> {
        Ok(self.store.documents()?)
    }

    pub fn document_content(&self, id: &str) -> Result<Option<Vec<u8>>, WorkspaceError> {
        Ok(self.store.document_content(id)?)
    }

    pub fn add_documents_atomic(
        &self,
        documents: &[(&str, &str, &[u8], i64)],
    ) -> Result<(), WorkspaceError> {
        Ok(self.store.add_documents_atomic(documents)?)
    }

    pub fn candidate_profile(&self) -> Result<Option<String>, WorkspaceError> {
        Ok(self.store.candidate_profile()?)
    }

    pub fn save_candidate_profile(&self, content: &str) -> Result<(), WorkspaceError> {
        self.store.save_candidate_profile(content)?;
        Ok(())
    }

    pub fn save_fit_assessment(
        &self,
        id: &JobIdentityId,
        content: &[u8],
    ) -> Result<(), WorkspaceError> {
        Ok(self.store.save_fit_assessment(id.as_str(), content)?)
    }

    pub fn fit_assessment(&self, id: &JobIdentityId) -> Result<Option<Vec<u8>>, WorkspaceError> {
        Ok(self.store.fit_assessment(id.as_str())?)
    }

    /// Persist a consumed prefill record before any browser field writes.
    pub fn record_consumed_prefill_grant(
        &mut self,
        grant: &waypoint_store::prefill_grants::PrefillGrantRecord,
    ) -> Result<(), WorkspaceError> {
        self.store.record_consumed_prefill_grant(grant)?;
        Ok(())
    }

    pub fn mark_manual_review(
        &self,
        id: &JobIdentityId,
        reason: &str,
        timestamp: i64,
    ) -> Result<(), WorkspaceError> {
        if self.store.get_job(id)?.is_none() {
            return Err(WorkspaceError::NotFound(id.as_str().to_string()));
        }
        if reason.trim().is_empty() {
            return Err(WorkspaceError::Export(
                "Review reason must not be empty".into(),
            ));
        }
        self.store
            .mark_manual_review(id.as_str(), reason, timestamp)?;
        Ok(())
    }

    pub fn manual_reviews(
        &self,
    ) -> Result<Vec<waypoint_store::review::ManualReview>, WorkspaceError> {
        Ok(self.store.manual_reviews()?)
    }

    pub fn clear_manual_review(&self, id: &JobIdentityId) -> Result<(), WorkspaceError> {
        self.store.clear_manual_review(id.as_str())?;
        Ok(())
    }

    pub fn job(&self, id: &JobIdentityId) -> Result<Option<StoredJob>, WorkspaceError> {
        Ok(self.store.get_job(id)?)
    }

    pub fn job_description(&self, id: &JobIdentityId) -> Result<Option<String>, WorkspaceError> {
        Ok(self.store.job_description(id)?)
    }

    pub fn save_job_capture(
        &self,
        id: &JobIdentityId,
        metadata: &[u8],
        segments: &[Vec<u8>],
    ) -> Result<bool, WorkspaceError> {
        Ok(self
            .store
            .save_job_capture(id.as_str(), metadata, segments)?)
    }

    pub fn job_capture(
        &self,
        id: &JobIdentityId,
    ) -> Result<Option<waypoint_store::job_capture::JobCapture>, WorkspaceError> {
        Ok(self.store.job_capture(id.as_str())?)
    }

    pub fn save_gmail_credentials(&self, value: &[u8]) -> Result<(), WorkspaceError> {
        Ok(self.store.save_gmail_credentials(value)?)
    }

    pub fn can_store_gmail_credentials(&self) -> bool {
        self.store.can_store_gmail_credentials()
    }

    pub fn gmail_credentials(&self) -> Result<Option<Vec<u8>>, WorkspaceError> {
        Ok(self.store.gmail_credentials()?)
    }

    pub fn clear_gmail_credentials(&self) -> Result<(), WorkspaceError> {
        Ok(self.store.clear_gmail_credentials()?)
    }

    /// Reserve one conservative Jobicy fetch per hour across application restarts.
    pub fn reserve_jobicy_fetch(&mut self, timestamp: i64) -> Result<bool, WorkspaceError> {
        Ok(self.store.reserve_jobicy_fetch(timestamp)?)
    }

    pub fn job_count(&self) -> Result<u64, WorkspaceError> {
        Ok(self.store.job_count()?)
    }

    /// Resolve a known mirror URL back to its canonical job identity. The UI
    /// uses this so a pasted/duplicated posting URL finds the tracked job.
    pub fn resolve_alias(&self, url: &str) -> Result<Option<String>, WorkspaceError> {
        Ok(self.store.resolve_alias(url)?)
    }

    /// Track a job the candidate already knows about: their own research, a
    /// pasted URL, or a saved local page. This is real product surface (it is
    /// how "I found this myself" enters the pipeline, and how scripts import
    /// jobs), not a test hook.
    ///
    /// The URL is stored exactly as supplied — all *fetches* remain guarded by
    /// the permission registry and the network layer's public-host checks.
    pub fn import_job(&mut self, job: StoredJob) -> Result<(), WorkspaceError> {
        self.store.upsert_job(&job)?;
        Ok(())
    }

    /// Add a manually described posting and its description atomically.
    pub fn import_manual_job(
        &mut self,
        job: StoredJob,
        description: Option<&str>,
    ) -> Result<(), WorkspaceError> {
        self.store.insert_manual_job(&job, description)?;
        self.store.find_possible_duplicates(&job)?;
        Ok(())
    }

    pub fn possible_duplicates(
        &self,
    ) -> Result<Vec<waypoint_store::duplicates::PossibleDuplicate>, WorkspaceError> {
        Ok(self.store.possible_duplicates()?)
    }

    pub fn index_next_duplicate_job(&self, after: &str) -> Result<Option<String>, WorkspaceError> {
        Ok(self.store.index_next_duplicate_job(after)?)
    }

    pub fn duplicate_index_pending(&self) -> Result<bool, WorkspaceError> {
        Ok(self.store.duplicate_index_pending()?)
    }

    pub fn duplicate_coverage_counts(&self) -> Result<(u64, u64, u64, u64), WorkspaceError> {
        Ok(self.store.duplicate_coverage_counts()?)
    }

    pub fn duplicate_neighbors(
        &self,
        id: &JobIdentityId,
    ) -> Result<(Vec<StoredJob>, bool), WorkspaceError> {
        Ok(self.store.duplicate_neighbors(id.as_str())?)
    }

    pub fn add_ai_duplicates(
        &self,
        first: &JobIdentityId,
        matches: &[(String, String)],
    ) -> Result<usize, WorkspaceError> {
        Ok(self.store.add_ai_duplicates(first.as_str(), matches)?)
    }

    pub fn has_duplicate_hold(&self, id: &JobIdentityId) -> Result<bool, WorkspaceError> {
        Ok(self.store.has_duplicate_hold(id.as_str())?)
    }

    pub fn pending_duplicate_pair(
        &self,
        a: &JobIdentityId,
        b: &JobIdentityId,
    ) -> Result<bool, WorkspaceError> {
        Ok(self.store.pending_duplicate_pair(a.as_str(), b.as_str())?)
    }

    pub fn decide_duplicate(
        &self,
        a: &JobIdentityId,
        b: &JobIdentityId,
        same: bool,
    ) -> Result<bool, WorkspaceError> {
        Ok(self.store.decide_duplicate(a.as_str(), b.as_str(), same)?)
    }

    /// Drive an application-state change. Illegal transitions are refused with
    /// an error rather than silently applied — the state machine is a product
    /// invariant, not a UI suggestion.
    pub fn set_state(
        &mut self,
        id: &JobIdentityId,
        next: ApplicationState,
    ) -> Result<bool, WorkspaceError> {
        let mut job = self
            .store
            .get_job(id)?
            .ok_or_else(|| WorkspaceError::NotFound(id.as_str().to_string()))?;
        if job.state == next {
            return Ok(true);
        }
        if !job.state.may_transition_to(next) {
            return Err(WorkspaceError::IllegalTransition {
                from: job.state,
                to: next,
            });
        }
        job.state = next;
        self.store.upsert_job(&job)?;
        Ok(true)
    }

    /// The whole pipeline: permission gate → bounded fetch → parse → identity →
    /// dedup → persist → observation → alias. Degradation is always reported.
    pub fn discover(
        &mut self,
        targets: &[DiscoveryTarget],
        fetcher: &dyn Fetcher,
        now_unix: i64,
    ) -> DiscoveryOutcome {
        let mut outcome = DiscoveryOutcome::default();
        let mut scheduled = Scheduler::new(self.budgets);
        let mut sources: Vec<SourceStatus> = Vec::with_capacity(targets.len());

        for target in targets {
            let status = self.discover_one(target, fetcher, now_unix, &mut scheduled, &mut outcome);
            sources.push(status);
        }

        outcome.coverage = CoverageReport { sources };
        self.coverage = outcome.coverage.clone();
        self.last_outcome = Some(outcome.clone());
        outcome
    }

    fn discover_one(
        &mut self,
        target: &DiscoveryTarget,
        fetcher: &dyn Fetcher,
        now_unix: i64,
        scheduled: &mut Scheduler,
        outcome: &mut DiscoveryOutcome,
    ) -> SourceStatus {
        let checked_at = Some(now_unix.to_string());

        // 1. Permission gate. A forbidden source is reported, never skipped
        //    silently: "we did not search this because policy forbids it".
        let request = match discovery_plan(&self.registry, &target.source, &target.board) {
            Ok(r) => r,
            Err(e) => {
                return SourceStatus {
                    source: target.source.clone(),
                    state: SourceFetchState::Unsupported,
                    results: 0,
                    last_checked: checked_at,
                    detail: Some(format!("not permitted by the source registry: {e}")),
                }
            }
        };

        // 2. Bounded workers. Refusal is degradation, not an empty result.
        if !scheduled.acquire_fetch() {
            outcome.budget_refused += 1;
            return SourceStatus {
                source: target.source.clone(),
                state: SourceFetchState::NetworkError,
                results: 0,
                last_checked: checked_at,
                detail: Some("fetch refused: bounded-worker budget exhausted".into()),
            };
        }
        let body = fetcher.get(&request.url);
        scheduled.release_fetch();

        // 3. Fetch classification.
        let body = match body {
            Ok(b) => b,
            Err(FetchFailure::AccessDenied(m)) => {
                return SourceStatus {
                    source: target.source.clone(),
                    state: SourceFetchState::AccessDenied,
                    results: 0,
                    last_checked: checked_at,
                    detail: Some(m),
                }
            }
            Err(FetchFailure::Network(m)) => {
                return SourceStatus {
                    source: target.source.clone(),
                    state: SourceFetchState::NetworkError,
                    results: 0,
                    last_checked: checked_at,
                    detail: Some(m),
                }
            }
        };
        outcome.fetched += 1;

        // 4. Parse. A parse failure is adapter drift, not "no jobs today".
        let postings = parse_payload(&target.source, &target.board, &body);
        let postings = match postings {
            Ok(p) => p,
            Err(e) => {
                outcome.drift += 1;
                return SourceStatus {
                    source: target.source.clone(),
                    state: SourceFetchState::ParserDrift,
                    results: 0,
                    last_checked: checked_at,
                    detail: Some(e),
                };
            }
        };
        outcome.parsed += postings.len();
        let mut persisted = 0u64;
        let mut failure_detail: Option<String> = None;

        for posting in postings {
            match self.persist_posting(&posting, now_unix, outcome) {
                Ok(true) => persisted += 1,
                Ok(false) => {}
                Err(e) => failure_detail = Some(format!("store error: {e}")),
            }
        }

        SourceStatus {
            source: target.source.clone(),
            state: if failure_detail.is_some() {
                SourceFetchState::NetworkError
            } else if persisted == 0 {
                SourceFetchState::Empty
            } else {
                SourceFetchState::Ok
            },
            results: persisted,
            last_checked: checked_at,
            detail: failure_detail,
        }
    }

    /// Persist one parsed posting. Returns whether it is now visible as a job.
    fn persist_posting(
        &mut self,
        posting: &RawPosting,
        now_unix: i64,
        outcome: &mut DiscoveryOutcome,
    ) -> Result<bool, WorkspaceError> {
        // Never trust a URL that arrived inside a third-party payload.
        if !is_public_host_url(&posting.source_url) {
            outcome.rejected_urls += 1;
            return Ok(false);
        }
        let key = identity_key(
            &posting.employer_namespace,
            posting.requisition_id.as_deref(),
            &posting.source_url,
        );
        let id = match JobIdentityId::new(key) {
            Ok(id) => id,
            Err(_) => {
                outcome.rejected_urls += 1;
                return Ok(false);
            }
        };
        // Content hash covers the posting's *content* (with the source ref for
        // provenance), so "same requisition, changed title/location" is
        // detected as a new observation rather than a silent no-op.
        let content = format!(
            "{}|{}|{}|{}|{:?}",
            posting.title,
            posting.employer,
            posting.location,
            posting.content_hash_source,
            posting.description
        );
        let hash = sha256_hex(content.as_bytes());
        let now = now_unix.to_string();

        match self.store.get_job(&id)? {
            None => {
                let job = StoredJob {
                    id: id.clone(),
                    title: posting.title.clone(),
                    employer: posting.employer.clone(),
                    location: posting.location.clone(),
                    state: ApplicationState::Discovered,
                    canonical_url: Some(posting.source_url.clone()),
                    content_hash: Some(hash.clone()),
                    first_seen: Some(now.clone()),
                    last_seen: Some(now.clone()),
                    remote: posting.remote,
                    source: posting.source.clone(),
                    requisition_id: posting.requisition_id.clone(),
                    posted_at: posting.posted_at.clone(),
                };
                self.store.upsert_job(&job)?;
                self.store
                    .set_job_description(&id, posting.description.as_deref())?;
                self.store.find_possible_duplicates(&job)?;
                self.store.record_observation(
                    &id,
                    &now,
                    &format!("discover:{}", posting.source),
                    ApplicationState::Discovered,
                    &hash,
                )?;
                outcome.inserted += 1;
                Ok(true)
            }
            Some(existing) => {
                match dedup_decision(&existing.id, &id, existing.content_hash.as_deref(), &hash) {
                    DedupDecision::SameObservation => {
                        outcome.unchanged += 1;
                        // Repair a missing description if a previous import
                        // stopped after writing the job row but before its text.
                        self.store
                            .set_job_description(&id, posting.description.as_deref())?;
                        if let Some(url) = &existing.canonical_url {
                            if url != &posting.source_url {
                                self.store.add_alias(&id, &posting.source_url)?;
                            }
                        }
                    }
                    DedupDecision::NewObservation { .. } | DedupDecision::DifferentJob => {
                        outcome.updated += 1;
                        let mut merged = existing.clone();
                        // Re-discovery refreshes posting facts but must never
                        // regress an application state the user already drove.
                        merged.title = posting.title.clone();
                        merged.employer = posting.employer.clone();
                        merged.location = posting.location.clone();
                        merged.canonical_url = Some(posting.source_url.clone());
                        merged.content_hash = Some(hash.clone());
                        merged.last_seen = Some(now.clone());
                        merged.remote = posting.remote;
                        merged.source = posting.source.clone();
                        merged.requisition_id = posting.requisition_id.clone();
                        merged.posted_at = posting.posted_at.clone();
                        self.store.upsert_job(&merged)?;
                        self.store
                            .set_job_description(&id, posting.description.as_deref())?;
                        self.store.find_possible_duplicates(&merged)?;
                        self.store.record_observation(
                            &id,
                            &now,
                            &format!("discover:{}", posting.source),
                            merged.state,
                            &hash,
                        )?;
                        if let Some(url) = &existing.canonical_url {
                            if url != &posting.source_url {
                                self.store.add_alias(&id, &posting.source_url)?;
                            }
                        }
                    }
                }
                Ok(true)
            }
        }
    }

    /// Strict-first ranking with a separate excluded bucket (D3).
    pub fn ranked(
        &self,
        constraints: &CampaignConstraints,
        profile_skills: &[String],
        now_unix: i64,
    ) -> Result<RankedResults, WorkspaceError> {
        let mut results = RankedResults::default();
        for job in self.store.all_jobs()? {
            let outcomes = check_constraints(
                constraints,
                &[],  // adapters do not claim languages the payload never states
                None, // nor salaries it never states
                None, // nor sponsorship promises
                job.remote,
                &job.location,
            );
            let reasons: Vec<String> = outcomes
                .iter()
                .filter_map(|(name, o)| match o {
                    ConstraintOutcome::Fail(m) => Some(format!("{name}: {m}")),
                    _ => None,
                })
                .collect();
            let description = self.store.job_description(&job.id)?.unwrap_or_default();
            let text = format!(
                "{} {} {} {}",
                job.title, job.employer, job.location, description
            );
            let (score, matched_skills) = evidence_overlap(profile_skills, &text);
            let observed_at = job
                .last_seen
                .as_deref()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let fresh = freshness(observed_at, now_unix, STALE_AFTER_SECONDS);
            let ranked = RankedJob {
                job: job.clone(),
                description_available: !description.trim().is_empty(),
                score,
                matched_skills,
                freshness: fresh,
            };
            if reasons.is_empty() {
                results.strict.push(ranked);
            } else {
                results.excluded.push(ExcludedJob { job, reasons });
            }
        }
        results.strict.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.job.employer.cmp(&b.job.employer))
                .then_with(|| a.job.title.cmp(&b.job.title))
                .then_with(|| a.job.id.as_str().cmp(b.job.id.as_str()))
        });
        Ok(results)
    }
}

/// The registry is the shipped policy file — one source of truth.
pub fn default_registry() -> PermissionRegistry {
    PermissionRegistry::from_json(waypoint_policy::INITIAL_REGISTRY_JSON)
        .expect("shipped source registry must always parse")
}

fn parse_payload(source: &str, board: &str, body: &str) -> Result<Vec<RawPosting>, String> {
    let parsed = match source {
        "greenhouse" => parse_greenhouse(board, body),
        "lever" => parse_lever(board, body),
        "ashby" => parse_ashby(board, body),
        "jobicy" => parse_jobicy(body),
        other => return Err(format!("no adapter registered for source '{other}'")),
    };
    parsed.map_err(|e| e.to_string())
}

/// Bounded-worker accounting, kept local to a single discovery run.
struct Scheduler {
    global_fetch: u8,
    active: u8,
}

impl Scheduler {
    fn new(budgets: ResourceBudgets) -> Self {
        Self {
            global_fetch: budgets.global_fetch,
            active: 0,
        }
    }

    fn acquire_fetch(&mut self) -> bool {
        if self.active < self.global_fetch {
            self.active += 1;
            true
        } else {
            false
        }
    }

    fn release_fetch(&mut self) {
        self.active = self.active.saturating_sub(1);
    }
}

pub mod email_handoff;
pub mod submit;
pub mod work;

pub use submit::{
    live_form_schema_digest, plan_fill, BrowserDriver, DriverError, FillPlan, LiveForm, SiteAck,
    SubmitOutcome,
};
pub use work::{CorrectionReport, FactOrigin, PacketReadiness, PreparedPacket};

/// Test-only fixture helpers (kept out of the product surface).
#[cfg(test)]
impl Workspace {
    /// Insert a tracked job directly, for suites that exercise the work layer
    /// rather than discovery.
    pub(crate) fn test_insert_job(&mut self, id: &str, title: &str, employer: &str) {
        self.test_insert_job_at(id, title, employer, "1758900000");
    }

    /// Insert a job with an explicit last-observed timestamp (freshness tests).
    pub(crate) fn test_insert_job_at(
        &mut self,
        id: &str,
        title: &str,
        employer: &str,
        last_seen: &str,
    ) {
        let job = StoredJob {
            id: JobIdentityId::new(id).expect("valid test id"),
            title: title.into(),
            employer: employer.into(),
            location: "Remote".into(),
            state: ApplicationState::Discovered,
            canonical_url: Some(format!("https://boards.greenhouse.io/{id}")),
            content_hash: Some(sha256_hex(id.as_bytes())),
            first_seen: Some(last_seen.to_string()),
            last_seen: Some(last_seen.to_string()),
            remote: true,
            source: "greenhouse".into(),
            requisition_id: Some(id.rsplit("::").next().unwrap_or(id).to_string()),
            posted_at: None,
        };
        self.store.upsert_job(&job).expect("insert test job");
    }
}

#[cfg(test)]
mod submit_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod work_tests;
