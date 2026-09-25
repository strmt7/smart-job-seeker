# Agent backlog

Every task is PLANNED. No task is marked implemented by this handoff. Gate order is G0 through G6. The conservative within-gate sequence can be parallelized only after contract/dependency review.

## T001 · G0 · Baseline and license inventory

Pin AI Job Hunter, AARG, openroles and selected dependencies; separate code/data/model terms.

Crates/ownership: research, legal inventory. Acceptance: Reproduce documented Prepared/Applied/Confirmed distinction and record exact version; no third-party code copied without allowed terms.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T002 · G0 · Source permission registry

Record permitted discover/read/fill/upload/submit/confirm operations and limitations per provider.

Crates/ownership: connectors, policy. Acceptance: Employer credential-only endpoints cannot be selected as candidate public submit routes.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T003 · G0 · Threat model and data map

Enumerate PII, egress, parser, browser, update and local IPC threats.

Crates/ownership: policy, security. Acceptance: Every external write and secret store has an owner and enforcement point.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T004 · G0 · Domain contracts

Define typed identities, revisions, states, grants, observations and amount units.

Crates/ownership: domain. Acceptance: Schema rejects unknown state, missing source, invalid currency basis and unbound approval.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T005 · G1 · Rust desktop shell

Create eframe/egui native navigation and typed view models.

Crates/ownership: desktop. Acceptance: No Node/Python/WSL runtime; 50k-row view virtualized; UI thread remains nonblocking.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T006 · G1 · Accessibility and input spike

Keyboard, Narrator/NVDA, IME, RTL sample and scaled multi-monitor behavior.

Crates/ownership: desktop. Acceptance: Focus order, semantic names, copy/select and 200% scaling pass observed tests; record unsupported cases.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T007 · G1 · Runtime capability handshake

Probe model metadata, tokenizer, reasoning control, schema output and cancellation.

Crates/ownership: inference. Acceptance: Unsupported reasoning settings fail qualification rather than silently disable thinking.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T008 · G1 · 16 GB AMD qualification

Pin model/backend/driver; measure VRAM, offload, latency and recall.

Crates/ownership: inference, benchmark. Acceptance: Target card tested with active UI/browser; aggregate peaks and context-position accuracy recorded.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T009 · G1 · Resource scheduler

Bound requests, model slot, browser session and blocking workers.

Crates/ownership: domain, runtime. Acceptance: Stress queue does not exceed budgets; cancellation does not lose drafts.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T010 · G2 · Encrypted store

Implement SQLite transactions, key storage and source/claim revision persistence.

Crates/ownership: store. Acceptance: Secrets absent from logs/config; encrypted file/backup/WAL handling tested.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T011 · G2 · Migrations and restore

Atomic migrations with preflight backups and rollback path.

Crates/ownership: store, export. Acceptance: Interrupted migration restores earlier consistent store in a clean account.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T012 · G2 · Profile evidence import

Import text/PDF/DOCX through restricted extraction; preserve source spans.

Crates/ownership: ingest, profile. Acceptance: Malicious archive/macros rejected; user attestation not mislabeled external verification.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T013 · G2 · Campaign constraints

Independent income/career/relocation campaigns with explicit hard/soft constraints.

Crates/ownership: profile, search. Acceptance: No cross-campaign answer leakage; unknown salary/sponsorship not silently passed.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T014 · G2 · ATS discovery adapters

Implement read-only permitted Greenhouse/Lever/Ashby sources.

Crates/ownership: connectors. Acceptance: Empty/error/blocked/stale states distinct; fixture fields and references preserved.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T015 · G2 · Safe network broker

Apply purpose/destination limits, redirect/IP checks, conditional cache and bounded bodies.

Crates/ownership: policy, research. Acceptance: SSRF/private-IP redirects and poisoned URLs fail without credential exposure.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T016 · G2 · Identity and dedup graph

Use requisition namespaces, aliases and conservative merge proposals.

Crates/ownership: domain, search. Acceptance: Same role multiple URLs retained as aliases; similar title different req stays separate; undo works.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T017 · G2 · Freshness observations

Track open/closed/unknown with time and method; recheck shortlist.

Crates/ownership: connectors, search. Acceptance: HTTP 200 alone never qualifies a vacancy; source outage preserves last-known stale data.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T018 · G2 · Lexical and semantic matching

Hard gates then lexical retrieval and bounded optional embedding/rerank.

Crates/ownership: search, inference. Acceptance: Near-miss test roles separated; scores not presented as interview probabilities.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T019 · G2 · Coverage dashboard

Show source/locale capability and scan failures with timestamps.

Crates/ownership: desktop, connectors. Acceptance: Failures never become silently relaxed filters or misleading empty success.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T020 · G3 · Research runs

Public source retrieval, contradictions and local synthesis with bounded passes.

Crates/ownership: research, inference. Acceptance: Candidate PII absent from general queries; answer citations resolve to captured evidence.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T021 · G3 · Semantic document tree

Resume/cover/answers reference source claims and lockable fields.

Crates/ownership: materials. Acceptance: Dates/title/ownership cannot change from a model proposal without explicit attestation.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T022 · G3 · Evidence validation

Validate claim IDs, quoted spans, numbers and user-approved semantic consistency.

Crates/ownership: materials, policy. Acceptance: Job requirements cannot become candidate evidence; unresolved claim blocks ready assurance.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T023 · G3 · Bounded drafting loop

Draft, deterministic checks, focused critique, two repair attempts, user review.

Crates/ownership: materials, inference. Acceptance: Best valid draft retained; no endless retry; failed constraint visible.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T024 · G3 · PDF and DOCX exporters

Render from semantic tree using qualified Rust pipelines and test fonts.

Crates/ownership: materials, export. Acceptance: Page targets, image ratio, text order and fact parity pass render comparisons.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T025 · G3 · Correction propagation

Invalidate downstream unsent revisions and grants when evidence changes.

Crates/ownership: profile, materials, applications. Acceptance: 100 correction fixtures affect exactly intended dependencies; submitted bytes remain immutable.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T026 · G3 · Answer library

Context-tagged answers with character counters and explicit reuse diff.

Crates/ownership: materials. Acceptance: Old authorization/salary answer cannot silently migrate to a new job.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T027 · G4 · Dedicated browser session

Visible Rust-controlled browser with isolated profile and private transport.

Crates/ownership: browser. Acceptance: No main-profile attachment or external CDP exposure; login/MFA challenge handoff works.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T028 · G4 · Typed form mapping

Read field labels/types and propose exact values without arbitrary JS execution.

Crates/ownership: browser, applications. Acceptance: Complex and ambiguous widgets pause; sensitive declarations need explicit user input.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T029 · G4 · One-use approval service

Bind job/destination/packet/profile/form/operation digest and expiry.

Crates/ownership: policy, applications. Acceptance: Replay/expiry/mutation/race cases reject; grant minted only by user gesture.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T030 · G4 · Submission journal

Persist intent and transition before approved external write.

Crates/ownership: applications, store. Acceptance: Fault at every boundary yields consistent local state; no Prepared=>Confirmed shortcut.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T031 · G4 · Receipt verification

Adapter-specific acknowledgement/identifier/email proof with redacted snapshot.

Crates/ownership: applications. Acceptance: Plausible success text on wrong origin not accepted; receipt source linked.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T032 · G4 · Uncertain recovery

Block ambiguous retries; reconcile through approved observation routes.

Crates/ownership: applications, browser. Acceptance: Crash-after-click scenario never auto-resubmits; user manual attestation separately labeled.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T033 · G4 · Manual fallback packet

Copy answers/download exact files and track external attestation.

Crates/ownership: desktop, export. Acceptance: Unsupported ATS remains usable without pretending automation or confirmation worked.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T034 · G4 · Adapter drift tests

Form fingerprints, custom questions and receipt fixtures with per-operation capabilities.

Crates/ownership: browser, connectors. Acceptance: Failed upload test disables upload claim without disabling permitted discovery.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T035 · G5 · Inbox consent and ingestion

OAuth/IMAP or selected-message import with minimal scopes and encrypted tokens.

Crates/ownership: mail. Acceptance: Unrelated mail excluded by policy; revoked grant stops new reads.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T036 · G5 · Timeline reconciliation

Match message evidence to requisition and propose lifecycle events.

Crates/ownership: mail, applications. Acceptance: One-role rejection does not reject other roles; low-confidence match awaits review.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T037 · G5 · Calendar coordination

Prepare timezone-aware events and availability checks before user-approved writes.

Crates/ownership: calendar. Acceptance: DST/overlap/recipient ambiguity handled; no invitation merely from a draft.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T038 · G5 · Relationship CRM

Candidate-owned contacts, provenance, opt-in followups and draft approvals.

Crates/ownership: contacts, desktop. Acceptance: No guessed email mass send; exact outbound text/recipient visible.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T039 · G5 · Interview practice

Role/evidence-based question and feedback sessions with optional local voice.

Crates/ownership: interview. Acceptance: Record consent; no fabricated experience/covert live assistance; resource budget remains visible.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T040 · G5 · Learning and public support

Source-backed gap exercises and official service links with unknown criteria.

Crates/ownership: research, profile. Acceptance: Tutorial completion not years of employment; program eligibility not decided by app.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T041 · G5 · Offer scenarios

Normalize units/currency/date while preserving original offers and assumptions.

Crates/ownership: offers. Acceptance: Gross/net/base/equity/installments distinguished; no unqualified legal/tax claims.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T042 · G5 · Outcome analytics

Explicit funnel denominators, response lag and variant provenance.

Crates/ownership: applications, analytics. Acceptance: Recent unresolved applications not counted rejected; observational results labeled.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T043 · G5 · Scoped review sharing

Candidate chooses exact redacted packet, expiry and reviewer permissions.

Crates/ownership: export, policy. Acceptance: Local core works without hosted sharing; no full vault disclosed by default.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T044 · G6 · Windows release packaging

Signed nonadmin installer, separate model downloads and manifest verification.

Crates/ownership: release. Acceptance: Clean account install/uninstall/upgrade tested; no mandatory development tools.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T045 · G6 · Update and rollback

Signed metadata, version checks, migration coupling and last-good version.

Crates/ownership: release, store. Acceptance: Tampered binary rejected; interrupted update leaves recoverable supported state.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T046 · G6 · Dependency and supply-chain audit

SBOM, license notices, vulnerability checks, pinned build inputs.

Crates/ownership: release, security. Acceptance: No code/data/model license conflation; critical findings resolved or release blocked.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T047 · G6 · Privacy and redacted support

Opt-in diagnostics, egress inspector, retention, export and key disposal.

Crates/ownership: policy, export. Acceptance: Canary PII stays out of telemetry; backup deletion behavior documented.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T048 · G6 · Comparative benchmark

Run blinded task suite against accessible current alternatives.

Crates/ownership: benchmark. Acceptance: Published denominators/version/tier/confidence limits; no untested missing-feature claims.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T049 · G6 · Usability pilot

Observe diverse consenting seekers on real permitted workflows.

Crates/ownership: research, UX. Acceptance: Task effort and failures recorded; no hiring-uplift claim from underpowered pilot.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.

## T050 · G6 · Release gate and limitations

Assemble test evidence, coverage matrix, support plan and known limitations.

Crates/ownership: all. Acceptance: Cannot release as enterprise-grade on screenshots or mocked happy paths alone.

Required evidence: Exact commit, commands, actual outputs, fixture IDs, screenshots/profile results where relevant.