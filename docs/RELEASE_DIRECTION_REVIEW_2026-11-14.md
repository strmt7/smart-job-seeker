# Product direction and release readiness — 2026-11-14

Smart Jobseeker remains a prototype, not a release candidate. Substantial
components exist, but a varied, independently scored, complete candidate journey
has not passed. A completion percentage or delivery date would be unsupported.
The remaining work includes integration and demonstrated capability, not just
polish. The user alone decides whether and when to release.

This review updates execution priorities within the existing
[completion plan](COMPLETION_PLAN_2026-11-11.md) and
[active queue](ACTIVE_DEVELOPMENT_PLAN.json). It does not replace their acceptance
criteria or mark an unfinished package complete. Source reviewed: local main
`e8f932e719b8426546192ffdac43de851816c044` plus the conditional-form working changes.

## What exists and what is still unproven

| Area | Existing foundations | Remaining release evidence/work |
| --- | --- | --- |
| Local application | Web interface, Windows tray, authenticated loopback control plane, workspace settings | Complete installed-user journeys, all slow-handler cancellation, abandoned-session expiry, sustained lifecycle measurements |
| Candidate materials | Imports, reviewed facts, templates, document workers, local PDF review and correction propagation | Diverse real-source PDF/DOCX fidelity, supported tailoring without invented history, final PDF conversion and ATS readability across layouts |
| Discovery | Free broad query planning/providers, public ATS adapters, route inspection and source-policy gates | Repeatable candidate-led searches beyond boards, stale-result cleanup, reachable application routes and honest coverage on a varied source set |
| Fit and identity | Constraint unknowns, scored criteria, semantic fit mechanisms and near-duplicate identity | Human-annotated equivalent/different jobs, repeated score stability, constraint and recall calibration; no duplicate attempts across aliases |
| Form completion | Native DOM/frame/shadow inspection, typed controls, choices/files, local visual checks, scoped change detection | Candidate-aware planning across conditional/repeated sections, custom controls, uploads and final review; complete local-model-scored workflows |
| Experience | Scoped retrieval and correction/revocation mechanisms | Automatically proposed lessons validated against current state, held-out transfer and paired no-memory regression checks |
| Records and email | Attempt archives, source advertisements, document/answer evidence, email preparation and explicit fixed self-test mechanisms | One complete source-to-final-review record, restart reconciliation and fictional dispatch recovery; no real application sends during development |
| Storage and installation | Encrypted-store/DPAPI mechanisms, private dependency provisioning and platform checks | Clean supported Windows install/update/rollback/recovery matrix; no reliance on this development host |
| Interface | Central theme/interaction styles, mouse-drag sizing and existing responsive checks | Functional screens/settings across complete journeys; screenshot inspection at narrow widths, resize/zoom and interaction-state coverage |

These are source capabilities and outstanding acceptance work, not claims that
every mechanism is finished. A queue entry marked planned does not establish
that its underlying code is absent. Passing fixture tests is not proof of
employer-site completion or employer ATS acceptance.

## Why progress has been difficult

Recent work overemphasized native geometry/identity fixtures relative to usable
whole journeys. Those repairs address real errors and remain necessary, but
more isolated passing cases cannot substitute for connecting the product.
The execution balance now shifts toward R04 and complete journeys, retaining R02
independent truth and R01 ownership/cancellation work as gates.

Three concrete problems explain the gap better than a blanket claim that local
models cannot do this:

1. **The planner underuses available reasoning.** Form key decisions use strict
   structured generation with thinking disabled. This avoids a measured runtime
   failure where thinking plus a schema exhausted tokens without a final answer.
   Plain reasoning is currently exercised by a probe; a probe is not an actor.
   Reasoning and constrained decisions should be separate bounded requests.
2. **Observations, facts and workflows are not fully joined.** Correct field
   geometry does not answer whose history is requested, whether a country means
   address or citizenship, whether an upload widget accepted bytes, or whether
   a later question invalidates an earlier decision. The actor needs explicit
   question intent, fact provenance and fresh observations after transitions.
3. **Developer research is not runtime autonomy.** Public research/discovery
   mechanisms exist, but the form actor does not yet have a complete, evaluated
   tool loop for researching a platform issue and testing the proposed solution.
   Giving Codex internet tools during development does not fill this gap.

## What other products and frameworks actually establish

The research retained native captures of 17 public pages: 15 successful,
substantive captures and two preserved 404s, with corrected primary sources for
both failed references. These are November 13–14 observations, not an independent
benchmark of the products. No competitor code was copied or dependency adopted.

| Primary source | Observed mechanism | Implication for this product |
| --- | --- | --- |
| [Simplify Copilot instructions](https://help.simplify.jobs/en/help/articles/2415391-use-copilot-to-autofill-applications) | Profile autofill, per-page completion/review, document selection; continuous multi-page assistance. Its separate Autopilot can submit selected applications. | Integrate profile, documents, review and tracking; do not claim all competitors rely only on manual submission. |
| [Simplify unsupported forms](https://help.simplify.jobs/en/help/articles/8717287-autofill-not-supported) and [Autopilot failures](https://help.simplify.jobs/en/help/articles/4455305-autopilot-credits-and-failed-applications) | Unsupported pages, missing answers and verification can require manual work or fail an automated attempt. | Explicit pending-review states are part of a professional product; never manufacture unknown candidate answers to inflate completion. |
| [Huntr](https://huntr.co/) and [autofill](https://huntr.co/product/job-application-autofill) | Connected tracker, profile, resume/letter tools and extension autofill; missing information can need manual completion. | Optimize the entire candidate journey. Vendor reliability adjectives are not measured universal coverage. |
| [LoopCV](https://www.loopcv.pro/) and [agent description](https://www.loopcv.pro/ai-job-application-agent) | Markets continuous discovery, matching, tailored materials and automated applications/email. | Continuous orchestration matters. Marketing does not establish internet-wide recall, local 16 GB feasibility or correctness. |
| [Browser Use](https://github.com/browser-use/browser-use) | Model-driven browser/REPL/CDP tooling; local Ollama is supported. Cloud and local browser modes are distinct. | Evaluate mature primitives. Local inference is feasible in principle; an unrestricted model-written Python REPL is not our isolation boundary. MIT source is not a reason to adopt cloud defaults or personal browser profiles. |
| [Skyvern](https://github.com/Skyvern-AI/skyvern) and [product](https://www.skyvern.com/) | Vision/model workflow automation using browser tooling. Repository core is AGPL-3.0; hosted features are separate. | Learn from visual workflow architecture. Do not incorporate incompatible source or count managed challenge handling as a free local capability. |
| [Playwright actionability](https://playwright.dev/docs/actionability) | Readiness checks include unique targets, visibility, stability, event reception, enabled/editable state as applicable. | Prefer mature interaction semantics where compatible. Readiness alone does not establish a correct answer or prevent a scripted external write. |
| [BrowserGym](https://github.com/ServiceNow/BrowserGym) | Common browser-agent evaluation environments spanning multiple task suites. | Evaluate complete tasks and held-out families with independent scorers; isolated field counts are insufficient. |
| [Ollama vision](https://docs.ollama.com/capabilities/vision) and [thinking](https://docs.ollama.com/capabilities/thinking) | Image/text inputs and separate thinking/final response channels; thinking controls depend on model/runtime metadata. | Use local visual inference and completed final assessments, measure actual outcomes, and preserve incomplete response metadata. API support does not prove intelligence or reliability. |
| [Windows application certification tooling](https://learn.microsoft.com/en-us/windows/uwp/debug-test-perf/windows-app-certification-kit) | Official installation/performance/platform validation tooling. | Qualify supported clean Windows hosts; this reference alone neither selects a packaging architecture nor establishes installed readiness. |

Our differentiating target is measurable correctness, candidate privacy and
recoverable full workflows with local inference. Superiority remains a target
until matched-task comparisons demonstrate it. Resume keyword/format scores
must not be advertised as an employer's actual ATS ranking or a hiring promise.

## Local inference with online autonomy

The intended architecture is **local inference with an internet-connected tool
layer**. It is not an offline-only actor. Existing source-permission, public
network and privacy mechanisms remain the network authority.

- The local model can propose anonymous queries about platform behavior,
  supported controls, public documentation and technical failures. A typed tool
  broker validates purpose, source permissions, robots, PII exclusion, request
  count, bytes, time and cancellation before any request.
- Public research and candidate-bearing form interaction run in separate
  contexts. Research access never grants network access to the inert rehearsal
  browser or authorizes an application, account creation, email or upload.
- Retrieved content is untrusted advice. Preserve URL, time, digest and failure;
  test the proposed action against current native/visual state. It cannot create
  candidate facts, clear a wall, override an approval, or supply CAPTCHA/login
  bypass. An unavailable source stays unavailable.
- Lessons store the failure, validated correction, scope/platform/version,
  supporting evidence, expiry and revocation. Retrieve a few relevant lessons,
  revalidate them, and retain an ablation path. Do not replay old selectors or
  let one successful layout dominate new observations.
- Keep requests bounded and local inference serial/resource-admitted. No paid
  API or silent online-model fallback is required or permitted.

## Implementation sequence and exit evidence

1. **R04/R02: improve the actual actor on frozen failures.** Complete the native
   conditional transition slice; preserve the current model's initial abstention.
   Add plain reasoning before a new typed decision when it abstains, retaining
   original responses and unknowns. Separate question subject/intent from
   approved fact resolution. Score the unchanged source/profile before claiming
   an improvement; do not turn expected answers into prompt hints.
2. **R04/R05: connect runtime public research and candidate-aware state.** Use
   existing network/policy machinery behind bounded tool requests. Exercise a
   public technical-knowledge failure/recovery and a refused PII-bearing query.
   Prove that retrieved advice cannot authorize candidate writes. Maintain the
   independent no-send boundary and cancellation/ownership tests.
3. **R02–R10: complete integrated journeys.** Start with varied candidate inputs,
   discovery, route verification, dedup/fit, tailored PDFs, application planning,
   inert saved-form completion and the final attempted-application archive. Use
   several distinct profiles and layouts, then the existing representative
   corpus floor (48 scenarios/12 saved pages) and frozen held-out families.
   Add repeated histories, conditional questions, files, sections and review.
   Unknown required facts correctly prevent completion credit. Record employer
   server-dependent behavior as untested; never fabricate responses to pass.
4. **R06–R10: generalize from measured failures.** Expand diverse PDF/DOCX
   extraction/layout/ATS checks, semantic fit stability and duplicate calibration,
   validated transferable lessons, email preparation and restart recovery.
   Compare assisted and unassisted outcomes on the same frozen tasks.
5. **R01/R11–R14: installed product acceptance.** Complete functional UI/settings
   and visual resize/zoom checks, clean Windows dependency/install/update/rollback
   tests, encrypted recovery and sustained idle/cancellation resource checks.
   Bind exact-source local and hosted checks before assembling the release
   decision package. Do not publish it as a release.

At every checkpoint report answerable fields reached/declared, exact/native and
local visual correctness, wrong/unverified/unknown entries, complete workflows,
review reasons, held-out scope, elapsed time, model/context identity, peak owned
commit, host headroom and verified cleanup. A successful development case is not
model promotion. Keep every unfinished acceptance criterion visible.

## Current development failure retained

The conditional native matrix has ten cases: two positive main/inline workflows
and eight refusal cases. All ten browsers close. Positive cases supply four
native readbacks and four additive transitions; an old plan is refused after a
transition. Some refusal cases enter a fictional value before detecting the
inconsistency; they are not zero-interaction cases.

On the unchanged fictional conditional profile, three local Gemma 4 12B attempts
each verified **0/2** declared answerable fields: the initial Country question
was marked for manual review, so the later question was never reached.
Formatting and contact-key definitions did not fix it. The unknown citizenship
question was also unreached: zero entries is not evidence of correct abstention
on that question. This is a mapping failure, not a measured visual failure.
The [conditional continuation](g1/CONDITIONAL_FORM_CONTINUATION_2026-11-14.md)
records the subsequent bounded reasoning, complete native label context and
structural label-comparison repairs. The unchanged development case now verifies
2/2 supported facts and leaves its reached required unknown untouched in two
actual local-model runs at separately bound binaries. This does not complete a workflow, qualify held-out
families or establish release readiness.
