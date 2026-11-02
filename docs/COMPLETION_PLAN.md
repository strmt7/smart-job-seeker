# Waypoint completion plan

Status: active development, 2026-11-02. This plan translates the requested
product into implementation and evidence gates. It supplements, rather than
replaces, the repository's handoff and `AGENTS.md` invariants. There is no
defensible completion percentage or delivery date yet: the unmeasured work is
not uniform in difficulty. “Ready” requires the acceptance evidence below, not
just implemented components or a passing unit suite. Do not create a release
as part of this plan.

## Operating boundaries

- Development and qualification must not submit a real application or email an
  application. A future external submission requires a separate, explicit user
  authorization and the existing one-use, destination- and content-bound grant.
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
