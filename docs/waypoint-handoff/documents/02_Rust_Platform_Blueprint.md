# Waypoint — Rust implementation blueprint

Version 1.0 • Research cut-off: 25 September 2026 • Status: implementation specification, not a built application

## 1. Decision and product contract

Build a candidate-owned Windows job-search workspace whose application logic and desktop UI are written in Rust. The product must join discovery, evaluation, evidence-backed materials, supervised applying, follow-up, networking, interviews and offer decisions without turning any of those steps into an unsupported claim of success. A mandatory local reasoning model and permissioned internet retrieval are core dependencies of AI-assisted decision workflows. A cloud model must never be required to complete the core workflow.

The ambition is to outperform the best alternatives on measured task outcomes, factual consistency, accessibility, effort and resource use. No evidence in this survey establishes universal market superiority or exclusive ownership of an idea. Treat “world’s best” as a benchmark program, not launch copy. Native/local AI, evidence-grounded drafting, receipts, job freshness and networking already have substantial prior art. [S63–S85]

The strongest near-neighbor is AI Job Hunter, not a generic spreadsheet tracker. AARG is a particularly relevant Rust document-workflow comparator. Reuse compatible components and learn from existing implementations; do not recreate ordinary functionality merely to increase our feature count. Because a pure-Rust native UI is now a requirement, the primary recommendation is a modular new native shell with selectively reused, attributed components—not a blind fork of a React application. [S63–S66]

### Non-negotiable acceptance boundaries

All owned production business logic, orchestration, parsers we write, local service endpoints and native desktop UI must be Rust. The renderer must not be an Electron or React application. Third-party engines may contain C/C++ or platform code; document this distinction explicitly. A llama.cpp inference process and a Chromium browser are dependencies, not evidence that our application logic is non-Rust. A genuinely all-Rust inference stack remains optional until it matches required hardware correctness and performance. [S39 S95–S97]

Do not require Docker, WSL, Python, Node.js or a cloud account for the end-user core application. Installer-managed model/runtime downloads are explicit, checksum-verified and resumable. A downloaded LLM can be several gigabytes; never bundle it silently in a deceptively small app-size figure. Internet search may use a configured service, but public company/ATS discovery and manual URL research must remain useful without a paid search API.

Every externally consequential action requires a fresh, explicit approval bound to its exact destination and contents. Filling fields is not submitting. A generated document is not an application. A clicked button is not confirmed delivery. A timeout after clicking is uncertainty, not failure. Never fabricate experience, work authorization, education, salary history, answers or application status.

The product may work offline for stored jobs, profile editing, local generation and exports. It must not label a current opening or current employer fact as freshly verified without a successful external check. When a current-decision workflow cannot research online, it produces a clearly stale/unverified draft and blocks that workflow’s “ready” assurance—not the user’s ability to manually inspect or export their own data.

## 2. Users, jobs to be done and breadth

The primary owner is an individual job seeker. The architecture must support multiple career campaigns without blending their constraints: an urgent income campaign, a specialist career campaign, international relocation and an academic/research campaign can coexist. The first qualification corpus should cover technical specialists, general office roles, skilled trades, career changers and early-career candidates. Expand language and geographic claims only when tested.

An optional coach or trusted reviewer receives a narrow, expiring view chosen by the candidate. Do not turn the product into employer-side automated ranking of people. Candidate job relevance is not a judgment of human worth. Protected attributes should not be inferred from names, photos or writing. Any requested accessibility/accommodation or work-authorization data is separately consented and never silently exported into other contexts.

The complete journey includes: importing evidence; setting constraints; discovering and reconciling leads; checking freshness, language, salary and authorization signals; researching an employer; comparing the role to source-backed experience; building documents and portal answers; reviewing and applying; recovering uncertain submissions; syncing responses; preparing interviews; maintaining relationships; comparing offers; learning from outcomes; and securely exporting or deleting the workspace.

A portfolio should include first-party URLs, documents, optional candidate-authorized GitHub work and public publications. It must distinguish sole ownership from team contribution and research experience from industry employment. Concurrent dates must not be double-counted. Sensitive profile details do not belong in a public sample repository; fixtures must use fictional people and employers.

## 3. Competitive thesis and differentiators to test

### D1 — Consistency across the whole application, including corrections

Existing evidence-grounded generation is not new. Our proposed stronger contract is a dependency graph connecting each verified or user-attested profile claim to every resume sentence, cover-letter statement, saved answer and pending application payload. Correcting a date, instrument, responsibility or authorship statement identifies affected artifacts, invalidates stale approvals and offers a specific diff. It must not change an already submitted artifact; it creates a new revision and optionally drafts a correction for user review. [S15 S63–S66 S84]

A valid differentiator would be demonstrated cross-artifact correction and approval invalidation on adversarial fixtures, not merely a “no hallucinations” prompt. Whether competitors already provide an equivalent end-to-end guarantee is unresolved. Recheck their current code and workflows before making a comparative claim.

### D2 — A legible, recoverable application transaction

The system has separate prepared, approved, attempted, uncertain, confirmed and externally attested states. Persist intended destination and content before interacting with a form. Bind approval to a content hash and revision. After a browser crash or uncertain response, reconcile with allowed receipt evidence before any retry. Capture a timestamped receipt package without collecting unrelated browser data. Existing products already offer receipts, history and confirmation tracking; the target advantage is a tested invariant across crashes and changed forms. [S64 S74 S82 S85]

Exactly-once submission cannot be guaranteed against arbitrary third-party websites that do not support idempotency. Promise neither that nor “guaranteed delivery.” Achievable targets are: never automatically retry an ambiguous external write; never claim confirmed without qualifying evidence; and never reuse an approval after its content changes.

### D3 — Source-aware opportunity identity and constraint reasoning

Connect syndicated URLs, employer requisition IDs, material reposts and previous applications into a reversible identity graph. A different URL must not automatically imply a new opportunity; a matching title must not automatically imply a duplicate. Show the evidence behind a merge, support undo and preserve employer/role identities across location changes. Existing dedup and repost detection are acknowledged baselines. [S63 S67 S70]

Evaluate explicit hard constraints separately from ranking. Show language-required versus language-of-ad, sponsorship-stated versus sponsor-history, base salary versus total compensation, exact experience requirement versus preferred experience. Unknown is not pass and not fail. Offer a user-visible relaxed-search branch, never silently relax constraints to fill an empty screen. No legal eligibility verdict is produced. [S25–S27 S72 S89]

### D4 — Hardware-qualified private reasoning, not a local-mode checkbox

The shipped local workflow must be evaluated end to end on Windows with a 16 GB discrete GPU, including the user’s AMD RX 9070 XT class. Verify reasoning controls, tool-call correctness, context recall and aggregate VRAM under simultaneous desktop and browser activity. Record the model, quantization hash, backend, driver, prompt size and resource measurements in each qualification report. Private prompts must remain local even when research URLs are fetched externally. [S36–S43 S95–S100]

### D5 — A measured source-coverage and effort frontier

The app tells the user what it searched, which sources failed, which are unsupported and how many candidates were filtered out for explicit reasons. It allocates expensive reasoning to plausible roles and uncertain facts, not every scraped page. Track active user effort and valid completed applications rather than raw sends. Coverage claims must specify a denominator—known employers, source/locale combinations, or benchmark postings—not “the whole internet.” [S33–S35 S67 S80 S88]

These five targets form an integrated quality proposition. None is certified as globally unique by this research. Their value must survive direct baseline tests, user observation and current competitor rechecks.

## 4. Rust architecture and process boundaries

### Architecture decision ADR-001: native desktop, modular monolith

Use eframe/egui with wgpu as the first native-UI spike. Keep rendering separate from domain actions through typed messages and immutable view models. A small Tokio runtime handles async networking and background queues; blocking extraction, indexing and document rendering run off the UI thread in bounded worker pools. The desktop shell owns navigation and accessibility semantics, never database or network logic directly. [S95]

egui’s availability does not guarantee Windows-native visual behavior, rich document editing, screen-reader completeness or low idle cost. The first gate must prove keyboard navigation, Narrator/NVDA, text selection, CJK input, 125/150/200% DPI, multiple monitors and long-list virtualization. If the spike fails, evaluate another Rust-native framework or a Rust-authored Windows UI backend. A JavaScript frontend is not an automatic fallback: that would change the user’s requirement.

Use a modular monolith, not a distributed cluster. Candidate crate boundaries: waypoint-domain; waypoint-store; waypoint-profile; waypoint-ingest; waypoint-connectors; waypoint-search; waypoint-policy; waypoint-research; waypoint-inference; waypoint-materials; waypoint-browser; waypoint-applications; waypoint-mail; waypoint-calendar; waypoint-interview; waypoint-offers; waypoint-export; waypoint-desktop; waypoint-test-support; waypoint-cli. Share types through domain contracts and stable serialization rather than cyclic cross-crate dependencies.

The GUI, orchestration and stores can start in one process. Untrusted file parsing and browser sessions belong in restricted child processes. Inference is a managed process with a narrow local interface and no internet capability. Network access is brokered by purpose. The browser necessarily contacts employer sites, but its automation bridge must be private and bound to the current candidate workflow.

### ADR-002: local store and migration safety

Use SQLite with transactional migrations and an explicitly qualified encryption layer. SQLCipher is a candidate, with its own license/build requirements; selecting it is not proof that files, backups and temporary exports are protected. The Rust storage layer owns transactions, encryption-key retrieval, schema version and retention. Use Windows-protected credential storage for secrets; never store OAuth tokens or browser cookies in config files. [S53]

Use a single writer task with bounded requests and independent read connections as appropriate. Use indexed identifiers, normalized URLs and FTS for baseline lexical search. Store original structured fields, normalized fields and provenance separately. Add vector indexing only when corpus size and quality tests justify the complexity. On a personal corpus, a compact exact vector scan may beat a second database operationally; measure before introducing one.

Write migration backups before destructive changes. A failed migration leaves the prior store intact and the app in recovery mode, never half-open. Backup manifests include schema, app and encryption version, hashes and expected row counts. Restore must be exercised in clean Windows accounts. Secure deletion on SSDs is not guaranteed by overwriting a file; rely on encryption, key disposal and documented limitations.

### ADR-003: inference boundary

Use a pinned llama.cpp build as the first qualified backend, supervised by Rust. For AMD Windows, test Vulkan first while comparing a supported alternative only when available for the exact GPU. The application must not accept a server bound to all interfaces without explicit warning and approval; internal runtimes use private pipes or authenticated loopback with restrictive bindings. Never rely on “localhost” alone as authentication. [S39–S43 S99 S100]

The domain only sees a trait-like inference interface: capability probe, tokenize, estimate budget, generate structured output, cancel and report actual usage. Model-specific chat templates and reasoning controls live in adapter metadata. Do not embed obsolete /think commands or one runtime’s flags into all prompts. A successful startup probe tests structured output, an allowed synthetic tool request, a bounded reasoning-enabled request and a nonempty final answer.

A Rust-native inference engine such as mistral.rs is a future substitution option, not an ideological requirement to trade away AMD correctness or speed. Foreign inference kernels, GPU drivers, Windows and browser engines remain listed in the software bill of materials. [S97]

### ADR-004: browser and internet separation

Use Rust CDP orchestration through chromiumoxide or a qualified equivalent. Launch a dedicated, visible browser profile under a restricted workflow account/context. Do not automatically attach to the candidate’s everyday browser or inspect unrelated tabs. Never expose CDP to the LAN, disable browser sandboxing, bypass challenges or steal authentication state. Login, MFA, CAPTCHA and ambiguous consent dialogs are user handoffs. [S46 S96]

Prefer permitted documented public ATS listing interfaces over browser scraping. The app may read Greenhouse, Lever and Ashby public postings, but employer-side application API credentials are not available to a normal candidate. Do not invent a public submission integration. Use the employer-hosted application flow, a documented partner integration when authorized, or an explicit manual handoff. [S25–S27]

No LinkedIn automated scraping/applying connector is enabled merely because a public GitHub bot exists. Source-code licensing and platform permission are separate questions. Build source permission decisions into an adapter registry and show unavailable routes honestly. Site terms, geography and robots restrictions must be rechecked; robots permission alone is not a legal license. [S12 S14 S28]

## 5. Data contracts and invariants

Core entities: Workspace, Campaign, CandidateProfileRevision, EvidenceItem, Claim, ClaimDependency, JobIdentity, JobObservation, Requirement, Employer, ResearchArtifact, DocumentRevision, AnswerRevision, ApplicationIntent, ApprovalGrant, BrowserSession, Receipt, TimelineEvent, Contact, MessageDraft, InterviewSession, OfferScenario, ConnectorStatus and ResourceSample.

EvidenceItem contains an immutable ID, source kind, acquisition method, source URL or local attachment reference, captured/observed time, content hash, consent scope, retention policy and classification. Claims reference evidence IDs and spans; source-backed means traceable, not automatically true. Claim truth status is separate: user-attested, externally supported, disputed, inferred or unknown. Do not mark a claim externally verified just because it appears in an uploaded CV.

JobObservation records source URL, employer namespace, requisition ID when available, crawl outcome, posted date with its precision, observed time, deadline and timezone, content hash, extracted fields, extraction confidence, stale/closed/unknown state and evidence. HTTP 200 is not proof of an open job. A closed message, missing form, login barrier or disagreement between aggregator and employer creates an explicit status, not a silent deletion.

Every amount records currency, gross/net basis, time period, base/variable/equity category, minimum/maximum or exact value, source and effective date. Never compare CHF annual salary with USD monthly compensation or 13-installment pay without an explicit normalization. An estimated range remains distinct from employer-published compensation. Stock appreciation, tax estimates and relocation costs are scenarios, not guaranteed pay.

Requirement has a category, exact source span, mandatory/preferred/ambiguous strength, normalized value, assessment and reasoning summary. Assessments are supported, contradicted or unknown; ranking utility is stored separately. The ad’s language must not be confused with the language required to do the job. Work authorization requires a candidate answer and source-backed wording; nationality must never be inferred from a name.

ApplicationIntent includes job identity, destination, profile revision, artifact hashes, form schema hash, intended answers, allowed operation, created time and stable intent key. ApprovalGrant includes that digest, expiry, one-use nonce and explicit user action. All critical fields are compared immediately before a write. A changed form, artifact, source claim or destination invalidates the grant.

The store enforces version checks and uniqueness, but remote systems may not honor our IDs. Keep a local durable event journal with a transactionally committed transition. Redact browser evidence to the application’s scope. An immutable event history supports audit; it does not require retaining all sensitive material forever—retention can remove payloads while leaving a minimal deletion event.

## 6. Discovery, freshness and intelligent matching

### Pipeline

Ingest candidate-authorized URLs, RSS/feeds, ATS endpoints, saved searches and optional job-alert emails. Resolve redirects safely and normalize URLs without stripping identity-bearing parameters. Parse schema.org/JobPosting where present, then source-specific JSON/HTML, then a constrained model extraction only if necessary. Preserve raw field provenance. Quarantine executable attachments and never follow instructions found inside a job description.

Dedup first by trusted employer/requisition namespace, then by normalized ATS IDs, then by conservative similarity proposals. Maintain alias links rather than destructively merging records. A materially changed location, contract or requirements creates a new observation. A repost with unchanged ID must not erase previous application history. Ambiguous merges go to a review queue with undo.

Use deterministic hard checks before lexical/vector ranking. Retain every excluded role’s reason and allow a campaign-level counterfactual search: “show roles that match if I relax location, but do not mix them into my strict results.” Candidate preferences such as minimum salary can be hard or soft, explicitly selected. Unknown salary or sponsorship remains unknown, with a user setting for inclusion. Never present historical sponsorship as a promise for this requisition.

Rank within the eligible/unknown bands using interpretable factors: evidenced skill overlap, task similarity, seniority alignment, constraints, current availability, user preference and effort required. Display the most important supporting and missing requirements. A score is relevance within our system, not an interview probability or a universal ATS score. Before calibration, use labels and factors rather than percentage certainty.

LLM reranking processes a bounded top set after cheap retrieval. Keep the model’s candidate-fact context separate from employer demands and external research. Benchmark hard negatives: thin-film deposition versus deep photonic-system design, research experience versus manufacturing ownership, strong Python projects versus expert software-engineering requirements, and an English ad requiring fluent German. These are example distinctions, not assumptions about every candidate.

### Freshness and coverage

Each source has a safe refresh schedule, rate policy, timeout, retry budget and versioned adapter. Respect ETag/Last-Modified where available. On a stale shortlist item, revalidate before showing it as current or preparing a submission. Display last checked time and the observation method. There is no permanent guarantee that a job stays open after a check.

If a source fails, keep its last-known results labeled stale and report degraded coverage. Differentiate empty results, network error, access denied, unsupported locale and parser drift. Do not change user filters to conceal a failure. Use worker subprocess timeouts when an underlying library can hang; do not assume its own timeout always protects the queue. [S33–S35]

The free core discovery tier uses direct company/ATS sources and user URLs. Optional search providers, SearXNG instances or paid aggregators may extend coverage, but none implies unlimited free search or perfect reliability. Publish connector capability tables by operation: discover, inspect, prefill, upload, submit, confirm. Testing one operation does not certify the others. [S25–S27 S54 S67]

## 7. Mandatory research and reasoning, efficiently

Every AI-assisted recommendation, employer brief, tailoring plan and application answer uses a bounded local reasoning pass. Ordinary deterministic operations—sorting a table, converting currency with a supplied rate, checking a hash—do not waste LLM tokens. Reasoning is a model capability with a verified runtime control, not a UI animation. If the selected runtime cannot enable it reliably, mark the model unqualified for core AI tasks.

Internet research is mandatory for current external facts used in an AI decision. Retrieval and inference are distinct stages. The network broker fetches public evidence; the local model synthesizes it. Candidate PII never goes into a general web query without specific disclosure. Search for an employer/role fact, not for the candidate’s whole CV. Offline candidate-only drafting remains a clearly labeled preliminary mode.

A research run has a plan with unresolved questions, source hierarchy, search/fetch budget and stopping criteria. Prefer employer/ATS pages for the role, official employer product pages for company facts and official authorities for rules. Search results are leads, not final evidence. Capture citation spans, timestamps and contradictory observations. Do not expand research indefinitely to manufacture certainty.

Default run policy, to benchmark and tune: at most four query formulations, eight fetched pages and two verification passes for a normal role; allow an explicitly approved deeper run. Batch and deduplicate requests. Cache public research by canonical company/role and content hash with domain-specific expiry. Never cache a candidate-specific inference under only an employer name.

A model response must conform to a typed schema. Validate JSON, enumerations, evidence references, number/date consistency, requested length and policy constraints deterministically. A second model pass may critique a risky answer but is not independent evidence. Self-agreement does not prove truth. Stop after two repair attempts; expose the unresolved issue rather than an endless “thinking” spinner.

Display a concise decision rationale, supporting sources, uncertainty and completed tool steps. Raw hidden reasoning is not required as a product guarantee. Do not confuse token streams with trustworthy justification. Model internals or verbose scratchpads are not evidence of external facts, and logs must not accumulate candidate secrets unnecessarily.

### Model selection and memory qualification

Provisional default: Qwen3.5-9B with a qualified GGUF quantization and a pinned llama.cpp backend. Compare Q4_K_M and Q5 variants using our own tasks; choose the smaller only if grounding, extraction and form-answer accuracy remain acceptable. The official card and quantizer sizes support candidate selection, not a claim that this app has been benchmarked. [S36 S38–S41]

Compare Gemma 4 12B Unified as a current alternative if its terms, backend and task performance pass. Keep Qwen3-14B as a size/quality reference. Do not call either the absolute best model without current measurements. Gemma’s open weights and model-specific terms must not be conflated with an OSI application license. [S37 S98]

Use one resident reasoning model and one generation slot on the 16 GB target. Shortlist extraction/assessment begins at an 8K–16K task window; complex synthesis can qualify 32K. A 128K profile is a separate stress/long-context target, not a default requirement or a promised fit. KV/cache behavior is model- and backend-specific, particularly for hybrid architectures. Measure it; do not reuse an attention-only formula blindly. [S36 S43 S90 S91]

Qualification includes a browser with an active form, the native UI, display allocation and inference workspace. Record dedicated and shared GPU memory and CPU offload. Passing “fully GPU-resident inference” means zero inference offload in backend reports and no hidden shared-memory paging under sustained tests; deterministic CPU parsing is allowed. An out-of-memory event reduces workload size or offers another qualified model with explicit disclosure. Never silently switch to a paid cloud model.

## 8. Document and answer studio

Use a canonical semantic document tree containing headings, roles, bullets, text spans, hyperlinks, image references and evidence links. The LLM proposes changes to this tree, not arbitrary OOXML or runnable markup. Deterministic constraints preserve locked professional titles, employers, dates, page targets and original image aspect ratios. The user can lock exact wording and approve sentence-level changes.

PDF output can use a restricted Typst pipeline; DOCX output can use a qualified Rust OOXML/docx-rs pipeline. Both render from the same semantic facts, not one loosely translated from the other. The templates may differ visually. Editable Word pagination cannot be guaranteed across all font installations/renderers; record the tested renderer and font set. Never claim full fidelity without a rendered check. [S66 S102 S103]

Quality gates check page count, text overflow, minimum readable font size, section order, paragraph orphaning, links, hidden text, text extraction order and image proportions. Two-page CVs and one-page variants are supported as explicit templates, not by deleting inconvenient facts. Highlight changes and show which base document/profile revision was used. A formatting error blocks “export validated,” not the ability to save the draft.

Candidate truth checks distinguish paraphrase from new claims. A numeric achievement, certification, tool skill or ownership assertion needs candidate evidence or a new explicit attestation. A sentence copied from a job ad is not evidence that the candidate did it. Quote matching only establishes traceability; semantic entailment and human confirmation remain separate gates. [S63–S66]

Build application packets: CV, cover letter, short motivations, long-form answers, writing samples and a submission manifest. Answer libraries are tagged by question meaning, employer/job context, date and constraints. Reuse offers a suggestion with provenance; it never silently copies an old salary expectation or authorization answer into a new employer’s form.

Character limits must support the portal’s actual counting rule where known: Unicode code points, UTF-16 units or bytes. Display the chosen counter. Avoid fabricated “human-written” or “undetectable AI” guarantees. Prioritize natural specificity, brevity and correct facts instead.

## 9. Supervised application engine and recovery

### State machine

Draft and Prepared are local artifacts. Approved is a one-use permission for a specific payload. Submitting means an external write may have begun. Confirmed means qualifying receipt evidence was captured. Uncertain means the write may have succeeded but we lack sufficient evidence. ExternalAttested means the user says an application was made outside the app, and the source of that status remains visible. Rejected, Interview and Offer are later outcome events supported by user or message evidence.

Allowed core path: Discovered → Shortlisted → Prepared → Approved → Submitting → Confirmed or Uncertain. Before any submission, a canceled or expired approval returns to Prepared. After an external write may have occurred, cancellation is not a rollback: enter Uncertain unless confirmation is known. A local validation error before any write may safely return to Prepared with evidence of no transmission.

Preflight rechecks job identity/freshness, form schema, required fields, file names/hashes, target origin, constraints and approval digest. Store the intent and consume the nonce transactionally, then permit the narrowly scoped browser command. The model cannot issue arbitrary browser scripts, shell commands or outbound POSTs. It may propose a typed field mapping; the policy layer decides whether the action is allowed.

Prefill itself is an information-disclosure boundary: a live employer page can observe personal data as it is entered. Require a separate explicit prefill grant before populating fields, followed by a distinct final-submission grant. Final approval cannot retrospectively authorize earlier disclosure. Present a split view showing form fields and exact proposed values. Sensitive declarations, legal attestations, demographic questions, salary history, authorization, availability and conflicts of interest require explicit candidate answers. A model must not guess. Skip optional demographic items only under a user-selected policy; never silently invent responses.

### Receipt evidence and uncertainty

Receipt evidence may include a confirmation page with a stable application identifier, an employer acknowledgement email or a permitted API result. Capture the exact page/response excerpt and time; verification criteria are adapter-specific. A screenshot is useful but not intrinsically authoritative. A marketing page that says “success” is not enough unless it is in the expected trusted flow.

If the browser crashes, network times out or receipt parsing fails after the write boundary, freeze automatic retries. Offer scoped reconciliation: inspect the employer account manually, check authorized inbox messages, or use a documented status route. A user may mark externally confirmed or choose a deliberate retry after reviewing the duplicate risk. Record that decision; do not guarantee the absence of duplicates.

A stored approval is invalid when the profile, packet, form, destination or relevant unresolved contradiction changes. Reopen review with a concise diff. The app can recover local work automatically after a crash; it cannot undo an employer-side submission. There is no hidden “apply to 100 jobs” mode that bypasses per-application review.

### Browser coverage policy

Start with deterministic test fixtures for Greenhouse-, Lever- and Ashby-like forms, then verify allowed real hosted forms manually without submission. Later adapters cover Workday, iCIMS, SmartRecruiters, SuccessFactors and regional boards only after a permission/maintenance review. “Generic autofill” remains best-effort, not a declaration that every ATS is supported. File upload, custom widgets, multistep routing and receipt parsing each have separate test rows.

Unsupported pages preserve a ready packet and provide copy buttons/manual handoff. They must not degrade into unbounded computer-use improvisation. Login/password reset and CAPTCHA are always visible user handoffs. Never purchase challenges, rotate accounts or evade a site block.

## 10. Inbox, calendar, relationships and interviews

Email connectors are opt-in and purpose-limited. Begin with selected message imports and explicit folder/label scopes. Gmail read scopes may trigger verification/security obligations; do not hide this behind a “connect everything” button. OAuth desktop flows require browser-based authorization and appropriate PKCE/redirect handling. An IMAP option still processes sensitive mail and needs careful credential storage. [S51 S63 S74]

Extract employer, requisition, date, event and source snippet into a proposed timeline update. Match deterministically where IDs exist; otherwise rank candidates and request review. A rejection for one role must not reject all roles at the company. Calendar scheduling checks timezone, existing availability and attendee/recipient identity before an authorized write. No email or invitation is sent just because an LLM drafted it.

Contact CRM records how a relationship is known, explicit public or candidate-supplied contact routes, conversation context and follow-up consent. Recommend specific, respectful introductions or followups, not invented email addresses or bulk unsolicited outreach. Weak-tie evidence supports considering networks; it does not prescribe mass messaging. [S77–S80 S87]

Interview preparation uses the role’s actual requirements, candidate evidence and a current company brief. Practice can cover behavioral, technical, portfolio and salary conversations. Keep transcripts local by default; recording or microphone use is explicit. Do not implement covert real-time assistance intended to misrepresent the candidate’s unaided performance. The product helps preparation and permitted accommodations, not deception.

Learning paths map genuine gaps to small verifiable activities and candidate-owned portfolio evidence. A completed tutorial is not equivalent to years of professional experience. Public services, apprenticeships and paid retraining can be surfaced with clear eligibility criteria and official source links. Offer human help when a rule or language barrier cannot be resolved; no automated benefits or visa verdict. [S72 S89]

## 11. Offers and outcome learning

Offer comparison decomposes base, fixed installments, bonus, equity vesting, benefits, relocation, commute, work authorization dependencies and candidate priorities. Preserve original currencies and dated exchange-rate sources. Show uncertainty bands and editable assumptions. Do not conflate estimated net income with a payroll quote or historical sponsor filings with current legal permission.

A negotiation coach drafts evidence-backed questions and alternatives from the actual offer and candidate preferences. No fabricated competing offer. Legal or tax matters route to official information and qualified advice where needed, with an explicit jurisdiction/date. The application does not decide whether someone is legally eligible to immigrate or work.

Analytics distinguish opportunities seen, shortlisted, prepared, submitted-confirmed, externally attested, responses, interviews and offers. Display denominators and censoring: recent applications have had less time to respond. No response is not necessarily rejection. Late responses revise the event view without rewriting history.

CV variant experiments must avoid duplicate applications to the same requisition. Randomize only between otherwise comparable permissible cases, with user consent. Observational correlations should remain labeled as such. A response difference after changing CV wording may reflect role selection, timing or network effects. Do not label a dashboard “causal AI optimization” without a suitable study. [S80 S86]

## 12. Privacy, security and enterprise readiness

Threats include malicious job pages and PDFs, prompt injection, rogue connectors, credential theft, cross-campaign data leakage, local IPC attacks, hostile update packages, symlink/path traversal, archive bombs, sensitive logs and an employer form changing after approval. Use an explicit threat model with attacker capabilities; “Rust is memory safe” is not a sufficient security argument.

All external content is untrusted data. Reading a page never grants its instructions authority. Separate read tools from write tools; models cannot mint approval grants. Enforce permissions in Rust before tool execution. Maintain allowlisted destinations, maximum body sizes, decompression limits, redirect limits and timeouts. Deny private/link-local addresses in untrusted URL fetches, recheck every redirect and pin resolved destinations to resist rebinding. [S47 S94]

Use attachment-type sniffing, macros disabled, no automatic execution and restricted renderer/parser workers. Restrict file-system access to a staging directory and candidate-selected paths. Do not commit real CVs, cookies, OAuth tokens, browser profiles or private email fixtures to GitHub. Security tests include secret canaries, malicious evidence text and poisoned job descriptions.

Default telemetry is off. Local diagnostics show precisely what an optional report contains. Crash reporting requires separate opt-in and redaction. Update checks are disclosed independently of analytics. Network inspector shows purpose, destination and data category, with a global pause that does not pretend to cancel already submitted remote work.

An enterprise-quality release requires signed packages, verifiable update metadata, rollback, SBOM, dependency/license audit, incident response, support bundle redaction, restore testing, reproducible build instructions, accessible UI, bounded resource use and migration compatibility. It is not equivalent to adding Kubernetes or calling an unreviewed codebase SOC 2 compliant. Business/team support may add policy profiles and coach collaboration later; core privacy and security are never premium-only features.

## 13. Efficiency budgets and measurement protocol

All figures below are proposed release targets, NOT measurements achieved by this handoff. Record hardware, driver, OS build, model/runtime hash, test data, cold/warm state and percentile distribution. Measure application, inference and browser processes separately and as an aggregate. Do not improve a reported metric by shifting cost into an excluded child process.

Proposed native shell targets: cold interactive startup p95 under 2 seconds on the reference SSD system excluding optional model loading; local input-to-visible-feedback p95 under 50 ms; inactive UI idle CPU below 1% of one logical core averaged over five minutes; UI resident memory under 250 MiB on the baseline dataset excluding separately reported model/browser; responsive list/search interaction p95 under 200 ms over 50,000 indexed jobs. These budgets need revision after the first measured spike.

Interaction rendering aims for p95 frame time at or below 16.7 ms on the reference machine, but idle screens must not repaint at 60 Hz. Coalesce streamed tokens into small periodic UI updates and stop animated effects in reduced-motion mode. Virtualize long lists, decode thumbnails lazily and free hidden large previews. No decorative always-running GPU animation.

Reference inference profile: one resident model, one generation slot and aggregate dedicated GPU memory normally no more than 14 GiB on a 16 GiB card, with dynamic adjustment to the actual Windows memory budget. This is a target for headroom, not an exact allocation forecast. Measure long-prefill peaks, KV/cache growth, sustained generation, browser rendering, sleep/resume and ten consecutive runs. Report time-to-first-token, tokens/second, whole-task latency, energy where obtainable and accuracy together.

Bound default concurrency: four public fetch tasks globally, no more than two per origin unless the source policy is stricter, one browser application session and one local reasoning generation. Cache and conditional requests take precedence over more threads. Queue cancellation removes pending work and stops child processes safely. A canceled submission after the external-write boundary becomes uncertain, not automatically retried.

Use streaming JSON/HTML extraction, capped response sizes, incremental indexing, content-hash caches, memoized tokenization and compact retrieval contexts. Batch low-risk deterministic validations. Embeddings may run on CPU or in a separate serial GPU slot; label the resource mode. Do not load several large models concurrently in the belief that more agents automatically improve quality.

The benchmark must compare at least the chosen local Qwen configuration, the qualified 12B alternative when feasible, a no-LLM lexical baseline and current competing app workflows on matched tasks. If a larger model gives negligible quality gain at much higher latency, ship the smaller qualified option. If no candidate meets the safety/quality target, narrow the automated task and retain human review instead of claiming an enterprise-grade autonomous agent.

## 14. Evaluation and superiority program

Create a rights-cleared, versioned benchmark corpus. Proposed starting scope: 50 synthetic or explicitly consented candidate profiles across at least five occupational families; 5,000 dated job observations; 300 local ATS-form variants; 200 adversarial document/web fixtures; and 100 cross-artifact correction scenarios. These are design targets, not a dataset already collected. Use separate development and held-out employer/template/time splits.

Human raters assess relevance, unsupported claims, source entailment and answer completeness with blinded product labels. Resolve disagreements and publish rubric/inter-rater statistics. Evaluate multiple seeds/model settings where stochastic generation matters. Do not let the same model act as the only judge of its own writing or compare products on cherry-picked demos.

Primary metrics: useful eligible opportunities at k; hard-constraint false-pass rate; false duplicate suppression; stale-open false positives; unsupported candidate claims per packet; document render/extraction defects; correct field mapping; verified end-state completion; accidental duplicate or unauthorized writes; recovery correctness; active candidate time; and resource budgets. Publish denominators, confidence intervals and failure examples.

Safety gates: zero unauthorized sends in the release adversarial suite; zero automatic ambiguous retries; zero silent cloud fallback; all sensitive fields explicitly sourced/attested; all receipts traceable; corrected evidence invalidates affected approvals. Passing a finite test suite is necessary but not a proof that real-world failures are impossible. Keep monitoring and rollback.

Baseline protocol: AI Job Hunter and AARG for local evidence/materials; openroles and JobSync for discovery; CareerPulse and permitted supervised workflows for applying; Simplify/Huntr/Teal/Jobright for broad UX; Scout for receipt experience; LoopCV for outcome analytics. Compare only actually accessible features, versions and tiers, and mark untestable cells unknown. Reproduce documented claims before alleging a defect. [S01 S02 S63–S82]

Only after task-level gates, run an opt-in pilot with diverse real job seekers. A sample of 30–50 is a usability pilot, not necessarily adequate to prove hiring uplift. Conduct a proper power calculation and pre-register a larger study before claiming improved interview or hiring rates. Record active effort, satisfaction and safety immediately; hiring outcomes require longer observation and control for eligibility and market conditions.

## 15. Phased delivery with explicit gates

G0 — Research and contract lock. Pin upstream code/data/model licenses and revisions; reproduce the closest competitor’s core flows; finalize user roles, sensitive-data policy and source permissions. Deliver the capability matrix and architecture decision records. Exit only when no critical dependency has an unresolved redistribution blocker.

G1 — Native Rust and hardware qualification. Build an empty desktop shell, one accessible long list, a document preview and a managed inference probe. Measure startup, idle CPU/RAM, GPU peaks, reasoning controls and long-context recall. Demonstrate Narrator/keyboard/IME/DPI basics. Do not build the whole product on an unqualified renderer or runtime.

G2 — Evidence and discovery foundation. Implement encrypted store, migrations, profile revisions, evidence graph, campaigns, direct public ATS discovery, freshness observations, reversible dedup and hard constraints. A user can obtain a truthful shortlist and inspect every reason without any applying code enabled.

G3 — Materials and coherent packet. Implement semantic document model, deterministic facts checks, evidence-linked edits, PDF/DOCX generation, template locks and correction propagation. A prepared packet is exportable, traceable and not counted as submitted. Pass render and text-extraction tests across supported templates.

G4 — Supervised application vertical slice. Add dedicated browser sessions, scoped field mapping, one-use approval, receipt capture and uncertain-state recovery against local fixtures first. Enable a small permitted real-source set only after preflight, drift and crash tests. No hidden mass-apply mode.

G5 — Full lifecycle. Add authorized inbox proposals, calendar support, relationship CRM, interview practice, offer scenarios and outcome analytics. Preserve campaign boundaries and provenance across every connector. Add human-help paths and translation only with qualified locales.

G6 — Release hardening and comparative validation. Complete signing/updating, restore/fault tests, telemetry opt-in, accessibility review, license/SBOM/security gates and baseline comparisons. Conduct pilot observation and revise. Publish precise supported operations and known limitations; keep the superiority claim conditional on evidence.

Do not attach a promised calendar date to these gates without staffing, hardware and source-access information. Parallelize independent research or documentation work, but serialize changes to central contracts and never let several agents independently invent incompatible domain models.

## 16. Agent operating protocol

Read AGENTS.md, this plan, the source register, gap ledger, screen catalogue, contracts and the current task checkpoint before modifying code. Each task has an owner, affected crates, prerequisites, acceptance cases, evidence and a review role. Implement one vertical slice at a time. Do not quietly drop safety states or support boundaries to finish a feature.

For every slice: write a failing behavioral test; implement minimal Rust code; run formatting, clippy, unit/integration/property tests; review input boundaries and permission checks; render the UI and document exports; profile; update the known-limitations register; then obtain a separate review. A second agent is a reviewer, not independent empirical evidence. Keep machine-readable commands and actual outputs.

Use checkpoint files containing completed task IDs, exact commit, test results, known failures and next step. Never say a task is finished because a file exists or a mocked screenshot looks convincing. Do not label a pipeline supported until its fixture and allowed manual validation pass. Do not create fake credentials, “sample” API keys that resemble real secrets, or tests that send live applications.

The provided package is a specification and design handoff. It contains no production Windows binary, trained model, verified runtime benchmark or promise of hiring. Generated visual examples use fictional data. The implementation agent must replace mock view models with actual domain data only after passing each gate.
