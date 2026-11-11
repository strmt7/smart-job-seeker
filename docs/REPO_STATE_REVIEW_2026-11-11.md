# Repository review - 11 November 2026

Reviewed source: `4acb9a955bcb961ad7a27cb9538e77bb0e03a547` on `main`.
This is a source-and-evidence review, not an exhaustive code/security audit or
release approval. The existing Smart Jobseeker development goal remains active.
The [execution plan](COMPLETION_PLAN_2026-11-11.md) and its
[machine-readable queue](ACTIVE_DEVELOPMENT_PLAN.json) carry the next work.

Subsequent implementation: the [R01 I/O checkpoint](g1/CONTROL_PLANE_IO_2026-11-11.md)
repairs the reproduced socket stall. Blocking-handler and broader readiness
work remain open. The review below retains its original source snapshot.

## Overall assessment

The repository is a substantial Windows/local-first prototype with connected
workflows, not merely a mock interface. Its strongest foundations are explicit
unknowns, source-bound candidate facts, corrections, application records, local
inference, encrypted production storage and owned-process teardown. The largest
remaining gap is dependable, generic form completion by the shipped local model
across unfamiliar employer layouts. Document, search, installation and interface
breadth also need broader end-to-end qualification.

There is no defensible overall completion percentage. Implemented mechanisms,
fixture success, a successful whole-product journey and operational readiness
are different milestones. More buttons or unit tests alone will not close the
gap. No real applications or application emails have been sent for this review.

## Capability map

| Area | Present in current source/evidence | Open work |
| --- | --- | --- |
| Windows product shell | Local authenticated loopback web UI, tray lifecycle, configurable per-user storage, managed runtime provisioning and repair | Clean non-admin Windows qualification, updates/rollback/uninstall and complete installed-product lifecycle |
| Candidate workspace | Supporting-file ingestion, reviewed facts, structured career history, corrections and artifact invalidation | Diverse imports/OCR and multilingual/ambiguous histories; complete provenance through every downstream feature |
| Discovery | ATS connectors plus general-web search planning, varied local-model queries, bounded background research and application-route checks | Search recall, provider degradation, pagination and stale-link classification on a labeled, varied corpus; it cannot literally enumerate the entire internet |
| Deduplication | Canonical identity, aliases, semantic comparison/review and incremental background indexing | Labeled cross-source near-duplicates, distinct requisitions, reposts and restart-safe application exclusion at larger scale |
| Job fit | Source-backed semantic fact selection, rubric-based assessments, stored input/model identity and repeatability comparison | Independent calibration, sensitivity and stability measurements; scores remain fit estimates, not hiring probabilities |
| Documents | Template workflows, local-model drafting/revision, factual checks, PDF conversion, rendered review and attempt records | Broad PDF/DOCX/template fidelity, scans, long documents, factual consistency, independent extraction and mandatory final-PDF acceptance |
| Form assistance | Native DOM/frame/shadow traversal, typed controls, choices, uploads, model advice, visual review and state-change detection | Connect and qualify generic multistep/repeated/custom-control decisions across unseen saved real forms; reviewed native features do not automatically constitute a universal autonomous agent |
| Real saved-form rehearsal | Compiled route, private local model, zero-capability offline browser, per-attempt screenshots and target observations | Latest real saved form completed three fictional fields, then stopped before Phone; Workable/Ashby copies remain incomplete/review cases |
| Learning | Sealed, revisioned and revocable form-experience records; bounded context-aware retrieval and stale-dependency checks | Automatic lesson proposals, validation/promotion, no-memory comparisons and held-out transfer without regression |
| Email/lifecycle | Credential handling, fixed Gmail self-test, inbox/record mechanisms and application-related preparation | Current full email workflow qualification, ambiguous delivery/reconciliation and duplicate prevention; application sending remains disabled during development |
| Privacy/recovery | SQLCipher production Windows opens, DPAPI-sealed material/keys, journals, backups and correction/grant invariants | Cross-machine recovery, plaintext/degraded builds, cancellation/crash/migration qualification and complete artifact export/deletion acceptance |
| Interface | Shared theme/layout, pane dragging, image viewer, settings, records and task progress | Full menu/state consistency, usability, actual browser zoom/DPI, accessibility, all resizing extremes and clean-host behavior |

Current source anchors include `apps/web_app/src/{main,search_planner,fit_task,
candidate_task,form_preview,recorded_preview,visual_review}.rs`,
`crates/waypoint_browser/src/{lib,captured_frame}.rs`,
`crates/waypoint_workspace/src/form_experience.rs` and the evidence links below.

## Problems and inefficiencies

### 1. Control requests can block each other - reproduced, high priority

`main.rs::serve_loop` accepts and serves a connection inline. `serve` performs
blocking socket reads with a five-second per-read timeout. A harmless incomplete
loopback request delayed a separate valid settings read by **4.910 seconds**;
the baseline was **0.040 seconds**, and recovery was **0.201 seconds**. This is
one controlled reproduction, not a latency distribution. It can impede status,
stop and other control requests. A per-read timeout also does not establish a
total request deadline. Write-side, queue and slow-consumer behavior need review.

The owned fresh-workspace probe created no browser, model or public request and
drained its process tree. Private evidence:
`target/control-latency-90tt2x5t/result.private.json`.

### 2. Form coverage remains the critical product gap

The latest real archive exposed nine supported controls. Four target-image
reviews were attempted: three matched, three entries passed native and visual
checks, and the fourth was correctly refused because its label was absent from
the crop. No section advanced. This is evidence of partial functionality and
correct abstention, not majority-platform coverage. The rehearsal deliberately
uses a fixed fictional profile; it does not establish a generic candidate-aware
autonomous planner. Some sophisticated native features are qualified separately
from this connected saved-copy route.

Incomplete rendering and request-variant proof affect what the local model can
see. The saved Ashby API path is robots-disallowed; a different transport is not
a solution. Avoid converting a rendering defect into a permissive visual match.

### 3. Evidence and planning have drifted

The legacy T001-T050 backlog still marks all 50 tasks `PLANNED`; it is a design
contract, not an accurate progress tracker. The main completion document still
said private model admission was unwired. The older limitations register said
there was no frame traversal, no CV ingestion and only plaintext postings, despite
newer implementations. Historical records should remain available, but they
must not compete with the current execution queue or masquerade as live status.
This review adds explicit precedence and current qualifications.

### 4. Large orchestration surfaces raise change/review cost

At the reviewed source, `main.rs` has 17,013 lines, with its main test module
starting at 9,864; `fit_task.rs` has 10,247 lines, with tests starting at 3,810;
`form_preview.rs` has 7,407 lines, with tests starting at 1,340.
`waypoint_browser/src/lib.rs` has 8,063 lines. These counts include tests and are
not proof of poor runtime performance. They identify expensive review surfaces.
Separate protocol framing, routing, workflow ownership and visual/context
capture when those mechanisms change, preserving contracts and regressions.
Avoid a repository-wide rewrite or a framework migration for appearance alone.

### 5. Development artifacts need a retention strategy

A bounded 20-second traversal counted 207,146 entries and approximately 42.66 GiB
of apparent file sizes before stopping. This is an **incomplete sample**, not
total allocated disk use or a hardlink-adjusted total. Several old runtime
qualification directories alone account for multiple GiB each. Repeated runtime
and model copies can waste disk and setup time. Reuse verified content-addressed
inputs, retain exact failure/evidence manifests, and propose dry-run cleanup of
proven task-owned disposable copies. Do not delete source evidence or unrelated
applications. Disk accumulation is distinct from resident-memory leakage.

### 6. Validation breadth and efficiency need explicit denominators

The latest local workspace run passed **1,286 tests**, with **591 ignored**.
Three focused native capture/barrier/privacy/discovery tests passed separately.
Nine scoped component width/CSS-zoom combinations passed and screenshots were
inspected. None establishes universal model accuracy, whole-workbench scaling,
clean-host installation or sustained leak freedom. Keep failed cases, but avoid
rerunning unchanged suites or model cases after documentation-only changes.
Use source/configuration/input identity to select relevant existing harnesses.

### 7. Packaging, dependency and detector claims must remain bounded

The previously pushed `201d418c` has successful hosted CI/coverage/CodeQL runs;
the newer local `4acb9a9` did not yet have hosted evidence at this review snapshot.
Live paginated GitHub inventories returned 11 automatically fixed code-scanning
alerts and six fixed Dependabot alerts, with no open or dismissed entries in
either returned inventory. This does not cover inaccessible providers or prove
all dependencies are latest. Do not manually alter finding states. Clean-host,
signing/update and final release gates remain open.

## Resources and evidence scope

The latest owned-resource checkpoint observed zero matching development
processes, services, scheduled tasks, startup entries or resident Ollama models,
with **28,263,559,168 bytes** (about 26.32 GiB) of host RAM available. It is a
checkpoint, not a long-running leak proof. The shared Ollama service was left
unchanged. No desktop or visible browser interaction was used.

Relevant evidence: [independent field review](g1/INDEPENDENT_FIELD_REVIEW_2026-11-11.md),
[private model admission](g1/PRIVATE_MODEL_ADMISSION_2026-11-11.md),
[document journey](DOCUMENT_PDF_JOURNEY.md),
[template tailoring](TEMPLATE_TAILORING.md),
[learning limits](FORM_EXPERIENCE_RESEARCH.md),
[capability coverage](CAPABILITY_COVERAGE.md) and
[the interface contract](design/2026-11-smart-jobseeker/README.md).
Full private audit receipts are under `target/repo-state-plan/`.
