# Repository overview — 17 November 2026

## Assessment

Smart Jobseeker has substantial working foundations and a large regression
suite. It remains an **application-assistance prototype**, not a release-ready
universal autonomous application product. The principal gap is integrated,
repeatable completion of unfamiliar workflows, rather than the absence of
individual controls or helper libraries.

The [implementation plan](IMPLEMENTATION_PLAN_2026-11-17.md) translates this
review into the existing R01–R14 work packages. The earlier handoff's product
requirements still apply, with the user's later local-web/tray, Qwen-default,
quality-first and headless-development decisions taking precedence. This is a
new current assessment; older dated evidence is retained, not rewritten.

There is no defensible release date or completion percentage yet. No release is
authorized. The evidence does not establish representative autonomous workflow
coverage, an error-free product, or safe interaction with arbitrary live employer
forms. Application and application-email submission remain prohibited in this
development run.

The subsequent [grounded-answer checkpoint](g1/GROUNDED_ANSWERS_2026-11-17.md)
verified native contact decisions and passed one of two authored field gates.
It also exposed an interrupted visual review in the mixed-materials case.
Attempt history now builds detailed rows after selecting the page, with
measured lower peak commit for a populated fictional archive. End-to-end
workflow, installer and real-form isolation gates remain open.

The [18 November mixed application checkpoint](g1/MIXED_APPLICATION_WORKFLOW_2026-11-18.md)
records saved-form journeys, qualification batching and search
execution with delayed collection. Its measurements cover the listed local
fixtures. Use the [execution limits plan](IMPLEMENTATION_PLAN_2026-11-17.md#r046--execution-limits-and-continuation)
to audit the remaining cutoffs and resume behavior before release.

## Source and evidence scope

- Local branch: `main`; review base `ec415d566d9410da774f56aa7eebecc46cf15c8f`
  plus the current Qwen/inference development batch. The commit containing this
  document identifies the final checkpoint; native reports separately bind their
  own exact source and binary hashes.
- Remote `main`, checked on 17 November:
  `90b5b1063af546e2544195394fec9b466f107060`. Hosted results for that source do
  not qualify the newer local work.
- Cargo metadata reports **16 workspace packages**. A tracked-source inventory
  of `apps/`, `crates/` and `tools/`, excluding vendored code and the historical
  handoff package, counted **671 source files**, including **405 test-named
  files**. Physical lines include inline tests: 183,233 Rust, 14,047 Python,
  7,884 JavaScript, 1,776 HTML, 1,528 PowerShell and 548 CSS. These are inventory
  figures, not implementation-completeness metrics.
- This review used current package manifests, source-bound semantic search,
  exact source reads, source/binary-bound local tests, native model/browser
  artifacts and GitHub API inventories. It is not a new exhaustive vulnerability
  audit, a line-by-line review of every source file, or a clean-host installation.
- Native/private evidence remains under ignored
  `target/qwen-default-6bjf38ki/`. No candidate documents, credentials, raw
  third-party CVs, model reasoning or private workspace data belong in Git.

## Current architecture

The product is a Rust workspace with a local web interface and Windows tray.
The encrypted workspace owns candidate/job state. Purpose-specific workers
handle documents, browsers, network work and inference. The current browser
actor uses native DOM/frame evidence, typed operations, screenshots and a local
vision model. Candidate-bearing development interactions use fictional/offline
replays; public employer pages remain read-only.

| Package | Present responsibility | Main integration or qualification gap |
| --- | --- | --- |
| `smart_job_core` | Earlier job/profile/repository/scoring service abstractions | Establish its remaining callers and retire redundant paths only after compatibility checks; do not maintain competing ranking authorities. |
| `waypoint_domain` | Facts, identities, amounts, approvals and application state contracts | Preserve these contracts through orchestration changes; complete cross-channel workflow evidence. |
| `waypoint_policy` | Source operation permissions and refusal semantics | Generic source onboarding, policy visibility and source-level recovery need complete discovery/UI acceptance. |
| `waypoint_inference` | Loopback adapter, capability probes, generation controls and budgets | One runtime-profile contract and lifecycle across every consumer; long-context accuracy and memory qualification. |
| `waypoint_store` | Versioned SQLite/SQLCipher storage, migrations, aliases, grants and journal | Clean-account migration/backup/restore and broad duplicate/application exclusion acceptance. |
| `waypoint_seal` | Windows DPAPI-backed secret/key envelopes | Recovery, relocation and user-account boundaries need installed-product qualification. |
| `waypoint_search` | Hard constraints, freshness, identity decisions and lexical evidence matching | Semantic fit calibration and near-duplicate evaluation beyond exact identity. |
| `waypoint_research` | Bounded, privacy-aware research planning and accounting | Stable free-provider operation and complete autonomous repair outcomes. |
| `waypoint_connectors` | Public ATS parsers and guarded discovery plans | Broad employer/custom-site discovery and faithful application-route classification. |
| `waypoint_net` | Guarded public network transport | Keep runtime purpose/source policy coherent across all adapters and specialized local/mail transports. |
| `waypoint_workspace` | Discovery/ranking/material/application orchestration and persistence | Complete candidate-to-final-review journeys, consistent cancellation, corrections and durable recovery. |
| `waypoint_materials` | Document structure, evidence checks, exports and correction propagation | Diverse actual templates, final PDF acceptance and measured ATS extraction behavior. |
| `waypoint_applications` | Typed mapping, preflight, journal, grants and uncertain-state rules | Universal navigation/final review and future explicitly authorized transport; existing contracts are not a shipped sender. |
| `waypoint_browser` | Native CDP, live state, controls, frames, captures and isolated replay | Complete unfamiliar workflows, transport isolation and adversarial lifecycle qualification. |
| `waypoint_lifecycle` | Proposal-based timeline, inbox, calendar, interviews, contacts, offers and sharing logic | Much of this is domain functionality; do not equate it with complete connected/UI workflows. |
| `waypoint_web` | Local HTTP control plane, tray, settings, tool provisioning and product integration | Consumer/runtime consistency, smaller modules, coherent task ownership, polished complete workflows. |

## What the evidence supports

### Windows shell, storage and setup

The local browser workspace, tray lifecycle, account-local paths, encrypted
default database and DPAPI key handling are implemented. Application-record
folders intentionally remain readable. Managed setup can provision private PDF,
browser, converter and Ollama dependencies, with inventories and notices; the
development build helper provisions its pinned OpenSSL SDK.

This is not proof of a clean Windows installation, every supported CPU/GPU
combination, offline repair, relocation, uninstall, update rollback or protection
against every host failure. Developer builds still require Rust/MSVC. Historical
native-GUI release documents describe a retired interface and must not be used
as current packaging evidence.

### Candidate information, documents and archives

Reviewed facts, contacts and structured career history have revision checks.
Corrections invalidate dependent work. PDF/DOCX text review, bounded template
changes, factual checks, conversion and visual-review mechanisms exist. Working
DOCX and final PDF are distinct artifacts. An uploaded document, converted PDF
or visible filename alone does not establish factual correctness or acceptance
by a website's upload widget.

Template planning uses the resident context to compare complete eligible
passages and job excerpts in successive requests. Coverage distinguishes
uneditable passages from work that a run did not reach. The editor still
checks the exact selected originals and reviews the exported PDF separately.

Attempt records already retain source/page material, selected PDFs, answer
proposals, screenshots, progress and source bindings. The complete required
packet—original ad, final tailored documents, all answers, every filled field,
final-review evidence and durable recovery—has not passed one universal,
independent end-to-end acceptance program.

### Discovery, fit and duplicate identity

The app has free-provider query planning, manual URLs, ATS connectors, link
ranking, source capture and route inspection. This is broader than a job-board
proxy, but current evidence does not demonstrate reliable, large-scale
candidate-led discovery across the general web. Earlier provider refusals and
HTTP 429 outcomes remain failures/degraded states, not successful coverage.

Provider responses are examined through the transport's byte boundary, and
the saved batch limit now depends on serialized history size. Durable
per-page storage, continuation across providers and labeled live route
coverage remain open.

Hard constraints keep unknowns explicit. The simple evidence score is lexical;
the separate fit task semantically selects facts and builds a source-bound
rubric. Required/preferred weighting, uncertainty ranges and repeat comparison
exist. A calibrated, repeatable overall match percentage across unfamiliar
occupations has not been established. Criterion extraction/selection can change
the denominator; source completeness must be measured before trusting a score.

Exact requisition/URL identity, aliases and changed observations are useful
foundations. They do not prove semantic recognition of slightly rewritten ads,
different board titles or reposts. Cross-channel, restart-safe duplicate
application exclusion remains a release blocker.

### Browser intelligence and actual completion

The browser can perform native typed entry, choices, attachments and bounded
section/dialog operations, with current-state and visual checks. Earlier
12B-model authored mixed-material evidence verified 9/9 known fields and left
two unknowns untouched. It stopped at an unrecognized review control. That was
field integration, not a complete workflow or representative performance.

The new default is the reviewed Qwen 3.8 27B recipe. Its installation, native
renderer/parser, explicit effort, metadata resolution and larger budgets are
wired into rehearsal. **Some other consumers still depend on the shared
resident-model path.** Whole-product default-model/lifecycle consistency needs
the unified runtime work in R01/R04.

The model-folder setting can retain a custom path, but selection still validates
against the shared daemon's inventory. Models present only in a custom store
need the unified local inventory/profile path; a stored path alone is not proof
that this installation scenario works.

The first integrated Qwen run with all visual operations deliberative reached
five verified fields and one entered/unverified field before its session
deadline. The later 16k transcription configuration stopped at the second field:
the independently required detail read used hyphens in its section-qualified
labels, while the earlier observation used em dashes. The guard retained that
disagreement. The completed case's declared field gate was **0/1**, with one
known field verified. Do not relabel it a successful application.

The follow-up escalates eligible native-associated visual detail to the
deliberative profile, retaining first observations and conflict/duplicate vetoes.
It verified **7/9 known fields** with both unknowns untouched, zero source-binding
failures and zero critical-entry flags, but still failed the field-case gate
**0/1**. The model marked the uniquely current employer ambiguous and omitted
the numeric question unit; the source/unit guards retained those refusals.
Navigation still stopped at `no_forward_control`. A structural visual
identity protocol remains preferable to a permanent dependence on model-written
separator punctuation. Merely accepting more spelling variants is insufficient.

Generic forward navigation still relies partly on a narrow label contract.
Final whole-form native/visual review and untouched held-out complete journeys
are open. Login, CAPTCHA and ambiguous/server-dependent states must remain
explicit manual walls, without losing the attempt or leaving workers running.

### Long context and host resources

The [256k assessment](g1/QWEN_256K_CONTEXT_2026-11-17.md) records a real native
Q8-cache allocation at 262,144 tokens, F16 failure, a guarded Q8 stop and a
successful Q8 repeat. The successful short request retained an eight-GiB sampled
host reserve. Its reported CPU allocation was about 8.7 GiB, above the current
application profile's six-GiB ceiling. Capacity, physical residency, cache
precision, full-window accuracy and product lifecycle are separate questions.

The adapter ceiling now permits 256k; the currently integrated rehearsal profile
uses 16k. Raising one constant would leave cache settings, memory admission,
shared daemons and caller budgets inconsistent. Integrate these through one
owned runtime profile before calling 256k a product default. Preserve the native
OOM peak-accounting and initial teardown discrepancy as unresolved observations.

## Quality and hosted state

The current coherent local validation passed **1,460 workspace tests / 618
ignored**, across 43 result groups, plus Clippy and the canonical debug build.
Worker/owned-lifecycle tests, package/runtime fixtures and relevant JavaScript
settings tests passed. These scopes do not establish live local-model accuracy,
browser universality or release readiness.

Rendered settings checks covered widths 1360, 480 and 320, revision conflicts,
selection persistence and returning to the Qwen default. Native action captures
were inspected. A mobile panel screenshot also includes the fixed bottom
navigation; selected screenshots are not proof that every menu, scroll state,
zoom, DPI or modal is accepted. The existing design contract explicitly retains
differences from generated references.

The GitHub connector did not expose the required alert inventory operations;
the authenticated CLI's read-only native API captures were used instead. On
17 November, the complete returned GitHub inventories contained six Dependabot
alerts, all automatically `fixed`; twelve code-scanning alerts, all `fixed` and
none dismissed; no secret-scanning alerts; and no open issues/PRs. The five
most recent listed main workflow runs were successful, but covered remote
`90b5…7060` or an older source. No findings were manually changed. This does not
certify other detectors or the newer local dependency graph.

## Main inefficiencies and root causes

1. **Field success is overrepresented relative to journey evidence.** Numerous
   detailed fixtures have improved correctness, but no representative complete
   workflow denominator exists. The next acceptance unit must be a full journey.
2. **Runtime settings are distributed.** Shared and private inference paths,
   vision, setup, context, cache and task budgets can disagree. A larger window
   or stronger model exposes this orchestration gap.
3. **Structural identities are represented as prose.** Model-formatted section
   strings and narrow button names can block otherwise valid perception.
   Replace them with typed, native-bound proposals and independent verification.
4. **Large source files combine concerns.** At the initial review, `main.rs`
   had 17,130 physical lines,
   `fit_task.rs` 10,441, browser `lib.rs` 8,793 and `form_preview.rs` 7,559,
   including substantial inline tests. Extract stable contracts and handlers.
   The current manifests contain no dependency on `smart_job_core`. Determine
   whether external consumers rely on its public library surface before
   changing its workspace membership or compatibility.
5. **Coverage and budgets are fragmented.** Per-call, browser, model, helper and
   controller limits have produced avoidable partial runs. Use one coherent
   execution budget and preserve omission counts rather than hiding truncation.
6. **Documentation is difficult to navigate.** Repeated historical checkpoints
   accumulate at the top of current-status documents. Keep one current overview
   and queue, with immutable dated evidence linked separately.
7. **Experiments need stronger observability.** Native vision completion/cost
   metadata is not retained consistently by the production path. OOM teardown
   and job-peak accounting need direct regressions, not inference from a later
   clean snapshot.
8. **Setup success is confused with installed-product readiness.** Private
   provisioning is valuable, but clean-account/VM lifecycle and recovery are
   still missing evidence. Fix those mechanisms rather than hardcoding this PC.

## Direction

Keep the domain, storage, source and authorization invariants. Refactor the
orchestration around typed observations, a shared runtime profile, a durable
workflow runner and one independent completion evaluator. Optimize the Qwen
default with actual matched/held-out evidence; retain generic model interfaces.
Develop broad discovery, documents, fit, identity and UI as parts of those
journeys, rather than disconnected demonstrations. The detailed plan specifies
the next executable steps and what would actually prove readiness.
