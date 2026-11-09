# Smart Jobseeker completion plan

Status: active development, updated 2026-11-09. This plan translates the requested
product into implementation and evidence gates. It supplements, rather than
replaces, the repository's handoff and `AGENTS.md` invariants. There is no
defensible completion percentage or delivery date yet: the unmeasured work is
not uniform in difficulty. “Ready” requires the acceptance evidence below, not
just implemented components or a passing unit suite. The [capability coverage
map](CAPABILITY_COVERAGE.md) adds the current comparison and connected workflow
acceptance gates. Do not create a release as part of this plan.

## Operating boundaries

- Development and qualification must not submit a real application or email an
  application. A future external submission requires a separate, explicit user
  authorization and the existing one-use, destination- and content-bound grant.
- Development and test automation must run headlessly without taking focus or
  interacting with the user's desktop. User-operated product interfaces remain
  available; do not open them for agent testing unless the user later requests it.
- Use free local software and services. Preserve source permissions, licensing,
  provenance and personal-data boundaries. Do not put real CVs, contact details,
  credentials or application contents into the repository.
- Keep network, browser, inference, conversion and indexing work bounded and
  cancellable; measure peak host memory and GPU memory. Do not kill or unload a
  model or process belonging to another user or application.
- Unknowns, incomplete inspections and ambiguous external writes remain visible;
  no inferred success or automatic retry after uncertain delivery. Challenges,
  authentication and unsupported controls require a recoverable manual route.

## Current baseline and gaps

- The Windows local web app and tray exist. The prototype provides board and
  general-web discovery, duplicate review, encrypted candidate storage,
  local-model assistance, reviewed form interactions, documents and attempt
  records. See `README.md`, `apps/web_app/README.md` and the dated entries in
  `IMPLEMENTATION_STATUS.md` for the precise scope of observations.
- The app explicitly does not send applications. Individual real employer
  pages have been inspected, but end-to-end form completion across many real
  platforms, visual verification accuracy, and live local-model reliability
  are not established. Do not generalize a single fixture or site to all forms.
- Clean-Windows installation, browser and dependency provisioning, update and
  recovery across machines are not established. The existing prototype bundle
  is unsigned and is not a distribution-ready package.
- Document processing and tailoring exist in stages; generic template fidelity,
  scanned documents, factual correctness, ATS extraction, visual quality and
  mandatory final PDF behavior require end-to-end evidence. Match scores are
  estimates and have not been calibrated as probabilities or hiring outcomes.

## Current priorities and evidence needed

The prototype is not release-ready, and the remaining work cannot be reduced to
a defensible percentage or date. The following order reflects current evidence;
each item remains open until its stated evidence is recorded against the exact
build and configuration.

1. **Restore a trustworthy integration baseline.** The latest `89407eb8` hosted
   browser and build checks passed, but its audit now reports
   `RUSTSEC-2026-0459` for the locked SQLite binding. Review and repair both the
   plain SQLite and SQLCipher paths: the nominal patched binding version still
   pins an affected SQLCipher source. Verify the real engine versions, encrypted
   reopen/migration behavior and inventory after a dependency change; do not
   suppress the finding or infer safety from package versions alone.
   Confirm the current hosted
   test and coverage results after the local fixture fixes. Investigate the
   hosted browser termination failure using the bounded survivor diagnostics;
   do not assume a local pass explains it. Resolve the three denied unmaintained
   dependency notices through an appropriate dependency or architecture change,
   not an unexplained suppression. Re-run the affected checks on the final SHA.
   The separate native GUI packages have now been retired in favor of the
   requested web/tray entrypoint. Local audit passed on the resulting 353-package
   graph without ignores, and hosted supply-chain checks passed for `12fc1504`.
   Its actual browser log reports 133 passes and three failures: late-field
   review-state timing, WebSocket fixture navigation and disconnect-probe baseline
   evaluation. The previous single-failure description was incomplete. Local
   repairs now cover fixture ownership/parsing, startup-independent fixture
   lifetime and document readiness, while loading stability has deterministic
   unit coverage and a trace. The later `10996953` hosted run passed all five CI
   jobs, including 136 browser cases and 296 layout cases. It predates the local
   fixture repairs; confirmation of those repairs is still required. Neither
   result establishes a submission-isolation boundary.
2. **Measure the local application agent.** Run a controlled, versioned corpus
   of unseen forms and a diverse set of permitted live vacancies through
   discovery, inspection, reviewed filling and final-step handoff. Measure
   incorrect and missed fields, navigation outcomes, manual-review reasons,
   repeated-run variation and peak resources. Include Workable and other shared
   platforms without tailoring the evaluation to them. Never submit an
   application during these checks. The current single-site and fixture
   observations do not establish majority coverage or visual reliability.
   A reviewed Lever privacy-notice control was refused because its click could
   submit or navigate. Investigate general, bounded consent handling with
   adversarial local fixtures and intercepted external writes before considering
   any broader action; uncertain controls remain in manual review.
   A fictional loopback diagnostic also demonstrated that a `type=button`
   click can invoke a scripted form submission. The DOM type guard alone does
   not prevent this. Treat protection against script-triggered writes across
   clicks, typing, uploads, frames, navigation and other browser channels as a
   release blocker; prove it with adversarial fixtures before relying on it.
   The current synchronous CDP client only observes network events while it is
   processing commands or pumping events. A possible design needs a continuously
   serviced interception layer, installed before interaction, that covers related
   targets and fails closed on disconnect. CDP Fetch request-stage interception
   can pause requests before transmission. A headless loopback test rejected
   requests in two manually attached targets; automatic attachment remains
   unproven. In a headless loopback experiment on this host, auto-attachment
   paused a dedicated worker, but its session rejected `Fetch.enable` as an
   unknown command. The worker was not resumed and no fixture request arrived.
   In a separate loopback test, enabling Fetch on the parent intercepted one
   dedicated worker GET after resume; this does not establish coverage of
   nested workers, service workers, other channels, or disconnection.
   This is a blocker for assuming target-level Fetch covers workers; another
   independently verified network isolation boundary is needed. Target
   auto-attachment has to be handled recursively for related frames and workers. A headless fictional
   loopback test showed that a scripted form GET reaches the server while the
   click returns success; GET is not necessarily read-only. Do not assume Fetch
   covers existing WebSocket connections, browser-level traffic, service
   workers or every external side effect. A fictional loopback POST paused
   by Fetch was delivered after its CDP
   connection was dropped while the browser remained running. Interception
   alone therefore does not fail closed on disconnect on this host.
   Interception failure, asynchronous actions and teardown need separate tests.
   Current runner and local disconnect observations differ, so the probe now
   records version-bound results instead of assuming delivery always occurs.
   The observations cannot qualify a no-submission boundary. A Windows
   zero-capability AppContainer feasibility probe blocked one loopback HTTP
   request and rendered a local fictional page, but inherited-pipe Chromium
   control remains unresolved. Investigate that compatibility before integrating
   a rehearsal mode; never relax network capabilities or silently run outside
   the container to make it start. A subsequent private headless-shell probe did
   complete pipe control, fictional field readback and screenshot capture inside
   a checked zero-capability root token, with zero observed loopback requests
   from four scripted mechanisms. It required a private readable engine copy,
   separate stderr/protocol handles and document readiness checks. Qualify a
   current engine, descendant tokens, additional channels, parent/control
   failure and resource cleanup before integrating the production transport.
   This remains a feasibility result; real-site interaction is not enabled. Network-disabled replay would also need a
   bounded way to capture/replay permitted assets and explicit handling of
   server-dependent validation and missing steps. No live-form interaction is
   authorized by these partial results.
3. **Prove document and matching quality.** Exercise PDF and DOCX inputs from
   varied, privately retained sources through extraction, factual tailoring,
   PDF conversion and page-by-page review. Compare ATS-readable text and visual
   layout against the originals, with explicit failures and held-out examples.
   Evaluate fit estimates against labeled cases and repeat runs; the recent
   lower-bound retrieval change still lacks a live-model qualification.
4. **Complete the dry-run journey and records.** Verify deduplication, source
   capture, edited PDFs, answers, screenshots, recovery and review state as one
   end-to-end workflow, including changed pages and interrupted sessions.
   Email application delivery and real application submission remain disabled
   in this prototype; any future delivery mechanism needs explicit one-use
   authorization and separate evidence, not a test submission.
5. **Qualify a Windows distribution.** Test a complete per-user package on
   clean supported Windows installations with no assumed developer tools,
   missing browser, offline dependencies, different GPU/RAM, upgrades and
   rollback. Verify dependency integrity, credentials, encrypted settings and
   job records, backup/recovery and resource bounds. The current unsigned bundle
   and fixture installer checks do not prove clean-host installation.
6. **Close experience and accessibility gaps.** Verify the actual rendered
   flows, visual consistency, keyboard and screen-reader operation, DPI and
   narrow layouts; resolve findings with representative users.
   The current shared theme, mouse-drag panels, Appearance settings and bounded
   license dialog passed a 295-case compiled-app Chrome layout matrix and a
   16-case focused Edge pass. These fictional-data checks cover selected
   geometry and interactions, not the entire settings/application journey or
   native Windows DPI. Use the retained design reference and actual screenshots
   for further refinements; no pixel-parity claim is established. Complete privacy,
   licensing and release documentation against the exact build. Do not publish
   a release as part of this work.

## Workstreams and acceptance gates

1. **Representative evaluation first.** Maintain a versioned fictional corpus
   of diverse applications and documents, including multistep flows, embedded
   frames, custom controls, validation errors, complex layouts and changed
   pages. Add legally sourced, privately retained real-world observations and
   real vacancy checks without candidate data. Record fixtures, model digest,
   prompts/configuration, hardware, date, outcomes and denominators; reserve
   held-out platforms and documents to detect overfitting. Include repeat runs
   and user-corrected failures. Do not report target numbers as measured results.
2. **Discovery and freshness.** Extend broad search planning, free providers,
   pagination, source-specific adapters and application-link verification under
   bounded budgets and source policies. Retain original ad and source evidence;
   distinguish an absent result, stale observation, changed role, inaccessible
   page and verified application route. Deduplicate with reviewable evidence
   across differing titles, employers and sources, without automatic destructive
   merging. Gate on measured recall and precision against a labeled held-out
   set, including expired and ambiguous links and failures.
3. **Profile, documents and matching.** Ingest varied PDF/DOCX and supporting
   documents with provenance, OCR where needed, corrections and private storage.
   Evaluate semantic matching against labeled qualifications and preferences;
   report evidence and unknowns, test repeatability and calibration, and never
   equate a score with hiring probability. Preserve candidate facts and template
   intent during job-specific CV and cover-letter editing, perform style and
   factual checks, compare rendered pages and extractable text, and generate
   reviewed final PDFs. Gate on diverse held-out documents, accessibility/ATS
   extraction checks and independently inspected visual and factual outcomes.
4. **Application agent.** Build a bounded observe–plan–act–verify loop using
   DOM, accessibility and local vision, with explicit control for navigation,
   fields, files, dependent widgets, frames and multi-step forms. Recheck live
   values and page changes, preserve prior work, recover safely from errors,
   and route unsupported, uncertain, authentication or challenge cases to the
   user with enough context to resume. Evaluate both model decisions and browser
   behavior against held-out forms and real sites up to the final step, never
   submit in qualification. Record field accuracy, completion, failure modes,
   coverage, time and resource peaks, including false-success rates.
5. **Learning without overfitting.** Store privacy-controlled, reviewable
   observations of platform patterns, attempted actions and outcomes. Retrieve
   bounded, context-matched evidence as advisory input; never turn a prior
   success into permission or bypass current-page checks. Evaluate learning
   against held-out platforms, conflicting and stale lessons, and a no-lesson
   baseline; keep it only where results justify it.
6. **Applications and audit records.** Bind selected jobs, source evidence,
   final PDFs, reviewed answers, screenshots and operation outcomes to each
   attempt; clearly record incomplete captures and uncertain delivery. Keep
   submitted packets immutable and propagate factual corrections to pending
   artifacts and approvals. Support email drafts, account setup, attachment and
   delivery reconciliation without sending in development. Gate on crash,
   retry, duplicate, reconciliation and artifact-integrity scenarios.
7. **Windows deployment and resources.** Build a reversible per-user setup with
   pinned and verified free dependencies, model setup, browser strategy,
   encrypted database and credential storage, data-folder choice, diagnostics,
   backup/restore and upgrade/rollback. Test clean supported Windows machines
   with different hardware and missing prerequisites, offline/degraded cases,
   low disk and memory, normal user privileges and restart recovery. Bound CPU,
   memory, GPU and process counts; report unavailable features honestly. Do not
   publish or create a release while completing these checks.
8. **Product experience.** Refine the web interface iteratively around discovery,
   fit, document review, queue, application progress, exceptions, settings and
   recovery. Test keyboard, screen reader, DPI, narrow widths and clear status;
   compare rendered screens with their intended designs. Preserve user control
   and make difficult cases actionable rather than hiding them.
9. **Integration readiness.** Run change-appropriate unit, integration,
   browser, real-site read-only, local-model and clean-host checks. Reconcile
   results against these gates, record failures and unresolved risks, and verify
   exact source and dependency identity. Declare ready only when the requested
   workflows work together with measured coverage and documented limitations;
   perfection or universal site compatibility cannot be inferred from tests.

## Execution order

Start with representative form and model evaluations, address the highest-impact
observed failures, and grow the held-out set alongside implementation. In
parallel over successive iterations, complete document quality, broad discovery
and duplicate handling, then exercise the whole dry-run journey. Qualify clean
Windows installation and the full integration only after the underlying flows
are stable. Keep `IMPLEMENTATION_STATUS.md` as dated evidence; update this plan
when the actual product or acceptance criteria change, without treating a
planned item as a completed measurement.
