# Detailed implementation plan — 17 November 2026

## Purpose, authority and outcome

This plan continues the existing Smart Jobseeker development goal. It does not
create a smaller goal, authorize a release, authorize sending applications, or
authorize visible desktop interaction. The [current repository overview](REPO_OVERVIEW_2026-11-17.md)
is its factual starting point. The [active queue](ACTIVE_DEVELOPMENT_PLAN.json)
retains R01–R14 as stable work-package identifiers; the numbered steps below are
their detailed execution breakdown, not a competing backlog.

The required product is a Windows-first local web workspace with a tray owner,
free runtime tools, a Qwen default, generic model interfaces, broad internet job
discovery, understandable match scores, semantic duplicate exclusion, truthful
tailored PDFs and reliable application preparation/completion. Each attempted
application needs a recoverable archive. Settings, credentials, storage,
installation, status and recovery must work as a coherent product.

Success is a candidate starting from varied documents/preferences and reaching
a verified, complete final application review on supported unfamiliar workflows,
with correct manual outcomes for genuine walls. The eventual product's external
delivery path requires explicit user authority and durable uncertainty handling.
During this development, all application and application-email delivery stays
disabled. Real employer forms remain read-only until independently qualified
isolation permits the relevant interaction. A model or button label cannot
grant that permission.

Quality and measured complete-workflow coverage take priority over latency.
More time, context or reasoning is permitted when useful. None permits exhausted
host RAM, hidden omissions, invented qualifications, weakened verifiers or
unbounded workers. No finite evaluation supports a promise of zero bugs or
universal zero-risk interaction.

## Architectural destination

Keep the modular Rust workspace and the local web/tray product. Preserve stable
storage/protocol compatibility. Refactor the product integration through the
following contracts, introducing modules only where they remove a real duplicate
authority or clarify ownership.

| Contract | Required responsibility | What it must never do |
| --- | --- | --- |
| Runtime profile and inference lease | One effective model/version, renderer/parser, context, cache type, reasoning policy, budgets, owner and capability record | Silently choose an older model, claim unavailable vision, or alter a borrowed daemon's global settings |
| Candidate evidence snapshot | Immutable, revision-bound contacts, history, claims, selected documents and instructions | Treat a generated answer, retrieved advice or hiring requirement as a candidate fact |
| Native observation | Current document/frame/control identity, type, section path, value, geometry, visibility and barriers | Convert missing evidence into a successful action or reuse stale selectors/images |
| Visual observation | Blind transcription or interpretation with native image binding, raw output and completion metadata | Receive an expected answer as a visual hint or silently rewrite a mismatch into a pass |
| Typed action proposal | An intended operation against currently offered native targets and grounded values/documents | Authorize execution, invent a target, bypass a barrier or retry an uncertain external write |
| Workflow runner | Durable state, one coherent budget, progress detection, cancellation, checkpoints and recovery | Equate field accuracy with final review or a click with delivery |
| Research broker | Anonymous permitted public queries/reads, retained provenance and bounded repair advice | Send candidate PII, inherit candidate cookies, execute page instructions or authorize actions |
| Completion evaluator | Independent truth, explicit denominators, omissions, manual walls and final native/visual acceptance | Grade only successful fields, discard earlier failures, or substitute developer judgment for local-model observations |

All public model interfaces remain capability-based. The default recipe is
Qwen-specific; candidate truth, browser actions, documents, archives and policy
are not. Additional backends must satisfy the same contracts rather than mimic
an unsupported Qwen effort value or output parser.

### Integration decisions after reviewing the full workspace

Use one attempt identity from job selection through the application record.
Bind it to the observed role, the current candidate revision and the exact
materials. Discovery, fit, tailoring, form completion and email preparation can
run independently, but the attempt must have a single source for each decision
and one current outcome. Local writes can be resumed from validated checkpoints.
An uncertain external write requires reconciliation before any subsequent action.

Treat each model decision as a proposal with a distinct verification step. The
model interprets the question, its period and its scope. The application resolves
that interpretation against exact native controls and reviewed sources. For
example, an explicitly requested current role can match a single checked ongoing
record. Two ongoing records require further evidence about which role is meant.
When a scalar proposal omits a literal unit, recover it from the native question
only if the wording establishes one. Retain the original response and require
the same fact, unit, field constraints and independent semantic check used for
any other proposal.

Run focused reasoning after the ordinary checks so a recheck cannot consume the
budget for already proposed answers. Prioritize required unresolved questions.
Preserve first-pass abstentions and separate the reason for a failed recheck.
Choose completion and accuracy metrics for each stage before introducing more
branches or extending model budgets. Apply this policy to document repairs,
page transitions and experience reuse as those paths move to the shared runtime.

Distinguish observed progress, unresolved required work and final review in the
interface. An unsupported route should still yield useful artifacts and a clear
next manual action. A section transition counts only after native state and
visual evidence confirm it. Check the final review independently against the
whole attempt. Use permitted pages to evaluate reachability and isolated
fictional forms to measure interactions and unexpected transitions.

## Execution order and work admission

1. **Verify owned generation in the longer replay.** The
   [saved-page rehearsal](g1/PROGRESS_BASED_REHEARSAL_2026-11-17.md)
   now uses verified progress, cycle detection and session and evidence budgets.
   [Owned generation](g1/OWNED_RUNNER_BINDING_2026-11-17.md) now sends the
   preload options throughout the rehearsal. Run the retained fixture when
   RAM permits and retain native allocation, error and cleanup observations.
   Then rerun varied layouts with independent full-workflow truth. Qualify
   in-flight interruption with native request drain and fresh readback.
2. **R04.1 and R03.1: repair coverage and structural perception.** First account
   for every offered question and add bounded deliberative repair for grounded
   omissions. The current-record and quantity changes have scoped regression
   evidence and one native observation. Replace punctuation-sensitive
   model-written field paths with typed section/leaf observations and independent
   native resolution.
3. **R01.1 and R11.1: unify runtime profiles and admission.** Connect the proven
   Q8/256k configuration through an owned, resource-aware product runtime, then
   make every inference consumer use the selected/default profile consistently.
4. **R02.1: freeze independent complete-journey truth.** Build the smallest varied
   development and held-out journey sets that can distinguish a general solution
   from the current fixtures. Do this before tuning navigation or memory reuse.
5. **R04.2–R04.5 with R05 throughout:** generic navigation, conditional/repeated
   controls, final review and recovery. Each new action channel needs its
   isolation checks before candidate-bearing execution.
6. **R06–R08 and R10:** join real discovery, selected documents, fit, identity,
   answers and archives through the actual web UI. A pre-staged API fixture is
   not a substitute for that vertical journey.
7. **R09, R12, R11 and R14:** qualify experience reuse, finish consistent UI
   designs, run clean-Windows lifecycle acceptance, then assemble readiness
   evidence for the user's decision.

Work may advance on independent read-only/design tasks while one expensive
model run is live. Do not edit its bound subject/controller/inputs, run another
GPU load, rebuild its executable, or restart it after an observation timeout.
Serialize searches that share a CocoIndex namespace; independent repositories
and tools must retain their own locks, mirrors and registrations.

## R01 — Runtime, scheduling and control-plane ownership

**Existing source:** `private_inference.rs`, `resident_inference.rs`,
`owned_runtime.rs`, `model_start.rs`, `runtime_start.rs`, `http_server.rs`,
`generation_profile.rs` and `ollama.rs`.

### R01.1 — One effective runtime profile

1. Extend the current profile into a versioned configuration record containing
   model/weight/projector identities, runtime minimum/version, native
   renderer/parser, advertised capabilities, requested/effective context, KV
   type, attention mode, reasoning levels, task-specific sampling and memory
   estimates. Keep measured qualification identity alongside the configuration.
2. Make profile preparation and model-file verification shared mechanisms.
   Reuse the existing content-addressed cache; do not copy another large model
   merely because the next consumer is drafting instead of rehearsing.
3. Introduce one product inference owner/lease boundary. Document drafting,
   template edits, fit, search planning, form planning, vision and research must
   acquire that boundary rather than independently selecting a resident model.
4. Preserve explicit custom model choices. An unset choice means the current
   curated Qwen recipe. A missing, changed or incompatible model produces an
   actionable setup/repair state, never an implicit older-model fallback.
   Resolve custom-store metadata from that actual store, rather than requiring
   its model to appear in an unrelated shared daemon's inventory. Retain file,
   manifest and native capability verification before inference.
5. Distinguish an owned private runtime from a borrowed service. Apply cache,
   context and environment settings only to the owned instance. Refuse an
   incompatible borrowed configuration without killing or modifying it.
6. Bind each inference result to its actual profile and candidate/job revision.
   Revoke stale work when the selected profile changes. Do not revive approvals
   merely because the same model name appears again.

**Acceptance:** all named consumers report and use the same chosen recipe;
custom-model compatibility remains; a shared daemon with different settings is
left unchanged; no parallel GPU loads or unowned survivors are created.
**Checks:** native renderer/parser/thinking/tool/vision protocol tests; missing
weights/runtime, changed digest, borrowed-service mismatch and concurrent-lease
cases; one complete UI-driven workflow using the default rather than a manual
preload. Proposed new owner modules must replace existing duplicated behavior.

### R01.2 — Coherent execution budgets

1. Define one workflow budget with wall time, model calls/tokens, native actions,
   research work, repairs, output, disk and memory allowances. Each nested worker
   receives its remaining allowance and a cleanup margin.
2. Account for preparation, load, perception, deliberation, navigation and final
   review separately. Native-call and session deadlines must not contradict the
   outer controller. Keep original timeout causes in failure records.
3. Replace small demonstration limits that block ordinary complex applications
   with measured, progress-aware limits. Raise section/repeated-record capacity
   only with unique-target/no-progress/cycle checks and representative cases.
4. Keep source-completeness accounting. If a field batch, criterion list or
   document passage set cannot fit, split and resume it; record every omission.
   Do not silently shorten the task or weaken constraints to finish faster.
5. Allocate deliberation to semantic decisions and eligible uncertainty. Literal
   transcription has its own profile; high-effort rechecks retain first results
   and cannot override genuine duplicates or contradictions.
6. Remove the single recovery allowance shared by every entered field. Admit
   each eligible pre-request memory recovery according to the remaining workflow
   time and a fresh native value, image and runtime check. Exercise two separate
   interruptions in one workflow and retain both original observations.

**Acceptance:** a supported long workflow can finish within its declared budget;
exhaustion yields a recoverable partial attempt; Stop remains responsive during
native/model calls. Measure cancellation and cleanup latency, not only final
exit. Do not advertise a target as an achieved time bound.

### R01.3 — Memory, allocation failure and idle lifetime

1. Model admission must include context/cache/projector/working memory, not only
   the GGUF file size. Show the source and uncertainty of each estimate.
2. Sample native process commit, whole-host available RAM and whole-adapter
   memory as separate scopes. GPU API allocation cannot stand in for physical
   WDDM residency or kernel/driver memory.
3. Preserve a host reserve, aggregate job containment, bounded concurrency and
   one model owner. An idle lease expires; ownership-aware shutdown unloads only
   task-owned models and awaits actual native exit/listener removal.
4. Reproduce the 256k F16 failure in a bounded dedicated test: investigate peak
   accounting above the configured job ceiling and the initial conflicting
   teardown observations. Record the original anomaly unchanged. Do not infer
   the cause from a later clean host snapshot.
5. Exercise normal exit, timeout, cancellation, parent termination, load failure,
   broken native helper, abandoned UI and restart. Include an owned child that
   exits while its native process object remains inspectable.
6. Replace the single-adapter assumption with verified selection of the device
   the local runtime actually uses. Qualify integrated-plus-discrete Windows
   systems and explicit device overrides with native allocation observations.
7. Evaluate the current 16 GiB model-file gate separately from disk integrity
   and live resource admission. Qualify modest offload with larger supported
   models and several context sizes, including the 256k profile. Use measured
   total commit and host availability to define eligible configurations.

**Acceptance:** every lifecycle ends with independently checked owned-tree and
listener outcomes, or an explicit cleanup failure. Sustained memory qualification
requires repeated complete workflows and idle periods; one clean snapshot is
insufficient. Other applications, user sessions and services remain untouched.

## R02 — Independent evaluation and qualification data

**Existing source:** `tools/form_corpus.py`, existing owned qualification tools,
captured replay manifests and `local-model-qualification`.

1. Extend the current schema-4 corpus with independent workflow truth: required
   sections/records, expected conditional questions, exact selected document
   roles/hashes, permissible unknowns, final review state and manual-wall causes.
   Freeze truth before exposing a case to the model or changing production code.
2. Separate the actor-visible task from the oracle. Native field identity,
   screenshots and source facts may be visible; expected transcriptions, target
   answers and grading hints must remain outside the model prompt.
3. Partition by platform/layout family, candidate profile and document style,
   not just random fields from the same form. A new label on the old fixture is
   not a strong held-out case.
4. Start with a declared **development set and an untouched held-out set** spanning
   at least eight distinct platform/custom-layout families, repeated/conditional
   histories, file widgets, dialogs, frames and final review. The case counts
   are collection targets, not existing coverage. Expand until the denominator
   supports the scope claimed; do not stop at the easiest forms.
5. Include diverse private authorized/public-source CVs and cover letters:
   technical and nontechnical, junior/senior, career changes, sparse/long,
   PDF/DOCX/scanned, multilingual and non-Latin names. Keep real personal files
   and their provenance private. Public fixtures remain fictional.
6. For every trial record planned, admitted, attempted, completed, failed,
   cancelled and unattempted counts. Separate wrong entries, unsupported entries,
   missed answerable fields, correct abstentions, manual walls, resource/harness
   failures and complete final reviews.
7. Report the total attempted denominator and any separately eligible subset.
   Explain exclusions individually. Publish uncertainty intervals where useful;
   zero observed errors is not a zero-risk universal result.
8. Pair Qwen configurations on identical frozen inputs. Preserve all first
   attempts and repeats. Changing context, cache and perception policy together
   evaluates a configuration, not one isolated cause.
9. Use meaningful adversarial controls: same label in different sections,
   punctuation inside a real label, swapped same-size PDFs, conditional required
   questions, stale equal-schema documents, misleading buttons and changed
   values between native and visual checks.

**Acceptance:** the grader rejects plausible wrong/incomplete outcomes and can
independently distinguish field correctness from completed workflows. No model
promotion or learned-procedure promotion is based on a few authored probes.
Run focused affected cases first, then the frozen integration set once per
coherent change. Reuse unchanged exact-source evidence with its original scope.

## R03 — Perception, frames and structural field identity

**Existing source:** browser `captured_frame.rs`, native frame/geometry modules,
`visual_review.rs`, `recorded_preview.rs`, `form_preview.rs`.

### R03.1 / R04.1 — Typed visual observations

1. Replace the model's concatenated section/field label contract with a versioned
   observation containing an ordered visible section path, a leaf label,
   observed control kind and displayed value. Keep raw native/model observations
   and derive canonical identity explicitly.
2. Resolve that proposal against the current native control census, source,
   frame/document loader and section hierarchy. Require one unambiguous native
   match. Never fix an answer by copying the expected value into the observation.
3. Preserve punctuation and case inside real headings, labels and values.
   Structural formatting must not merge distinct native labels or reorder
   sections. Include negative cases that would pass an unsafe global dash or
   whitespace replacement.
4. Keep the original screenshot/crop and raw response. A native-associated detail
   is separate evidence. The local model must independently read it; developer
   inspection is diagnosis, not the required observation.
5. Preserve duplicate, conflict, clipping, coverage and stale-image vetoes.
   Escalate only eligible uncertainty within the declared budget. Do not sample
   repeatedly until a preferred answer appears.
6. Update the visual schema, prompts, comparison adapters, archived schema and
   grader together. Legacy evidence remains readable and visibly versioned;
   migration must not retrospectively change an old result.

The October 2026 W3C draft specifies a flat accessible name whose contributors
can include hidden referenced text. The DevTools Accessibility protocol offers
a DOM-bound partial tree and name-source metadata. Use those sources for
association and provenance. Keep the actual visible label and section path as
separate observations, including an explicit unavailable state for labels the
model cannot read in the image. Add paired cases for visible, hidden-referenced, duplicate
and replaced contributors to verify the distinction. [W3C AccName draft](https://www.w3.org/TR/2026/WD-accname-1.2-20261021/),
[DevTools protocol schema](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/json/browser_protocol.json).

**Acceptance:** the current separator failure is resolved by native-bound typed
identity, and adversarial repeated/literal-punctuation cases remain refused.
Matched and held-out local-Qwen results are required before calling the protocol
an improvement.

### R03.2 — Complete native/visual scope

1. Extend existing frame ownership and rendered-state checks to all supported
   main/child, same-origin, independently targeted and inherited documents.
2. Maintain current loader/backend identity, native hit testing, visibility,
   clipping and scope-completeness evidence. Unknown or inaccessible scopes stay
   visible as review barriers, including zero-field scopes.
3. Keep control-specific captures small and complete. Wider screenshots may
   introduce unrelated fields and reduce accuracy; larger is not inherently
   better. Qualify label detail, upload filename, select/radio and repeated-row
   captures independently.
4. Retain native completion metadata in the production path: request purpose,
   image/request/profile hashes, context, effort, cache, token counts, durations,
   stop reason and response digest. Preserve incomplete responses privately;
   public logs must not expose candidate material or hidden reasoning.

**Acceptance:** each action/verification is bound to a fresh complete scope and
actual local-model observation. Native identity alone never clears a scope or
visual barrier.

## R04 — Universal workflow completion

The event-driven relay reduced the same four-field native check from 99.8 to
38.9 seconds. The dense fixture now reaches the final review without a submit
action. Dialog admission uses distinct observations and verified section
progress. Independent terminal review and representative local-model journeys
are the next completion gates. [Rehearsal evidence](g1/REHEARSAL_PROGRESS_2026-11-17.md)

### R04.2 — Generic navigation intent

1. Give the actor a current native census of buttons, links, tabs, steps and
   dialogs, with type, label, state, source and target identity. Keep page text
   untrusted.
2. Ask for typed intent: move to the next section, review, return, save draft,
   final delivery, destructive action or unknown. The model nominates a current
   target; Rust policy decides whether that operation is admitted.
3. Replace the narrow forward-label mechanism with this contract. Do not fix
   the current fixture by adding its review-button wording to an allowlist.
4. Before action, recheck target identity, native state, verified required
   fields, replay/transport state and authority. After action, inspect the new
   document/scope and compare actual progress.
5. Detect cycles, unchanged pages, stale references and unexpected external
   targets. Stop with a useful pending-review reason and preserved archive when
   the next safe operation cannot be established.
6. Bind same-origin inline form fields, navigation controls and final review
   to the same current child document. The frozen inline-frame rehearsal now
   verifies both first-section fields and stops when the parent-document
   census misses the child's Continue button. Preserve separate frame leases,
   reject mixed or stale targets, and verify the unchanged inline fixture.

The saved-copy workflow now classifies native button intent from the current
control inventory. It rechecks the selected native identity, visual target and
resulting document. The mounted two-step Spanish fixture reached and verified
final review. The frozen history fixture still stops on an inconsistent visual
read of a neighboring control. Use native geometry to focus its detail image
and retain the original conflict evidence.
[Navigation qualification](g1/RECORDED_NAVIGATION_2026-11-18.md)

### R04.3 — Repeated, conditional and complex controls

1. Plan whole records for employment, education, qualifications and addresses;
   preserve chronology, supplied date precision and current/ongoing status.
2. Discover add/remove/reorder controls generically. Verify the resulting record
   identity and count after each admitted mutation. A repeated row must never
   receive another row's facts because its label is identical.
3. Support typed text/number/date/time, native/custom selection, searchable
   combos, multiselect, checkbox/radio groups, required conditional questions,
   file choosers and widget feedback through the shared tool boundary.
4. Reinspect when a value reveals new questions. Replan only against fresh native
   controls and the immutable candidate snapshot; invalidate stale proposals.
5. Handle validation errors by transcribing actual feedback, checking the
   requested format against known facts and proposing a bounded repair. Never
   invent salary, authorization, demographics, answers or file acceptance.
6. Audit every offered unanswered field and retain a proposal or a precise
   unsupported/unknown/conflict reason. Do not equate zero batch truncation with
   complete semantic coverage. Permit bounded high-effort repair of an invalid
   structured proposal using original question/evidence; never insert the oracle's
   missing unit or expected answer into a repair. Resolve a typed current-record
   intent against explicit native candidate flags before combining values from
   a whole record. Concurrent current roles must remain genuinely ambiguous.

Native month and year controls now accept checked history values in saved-copy
rehearsals, with scope, constraint, readback and localized month checks.
Continue with explicit ordering evidence for repeated record groups whose
section names and question wording are indistinguishable. Preserve a manual
review outcome whenever several records fit the same control.
[Qualification evidence](g1/HISTORY_PERIODS_2026-11-18.md)

### R04.4 — Independent final review

The [post-navigation checkpoint](g1/POST_NAVIGATION_REVIEW_2026-11-17.md)
records the terminal image as evidence for manual review and preserves a
read-only page when model admission fails. The five-step native fixture
exercised capture. Use the retained dense-form timing and refusal evidence to
qualify the verification steps below.

The [independent summary check](g1/FINAL_SUMMARY_VERIFICATION_2026-11-18.md)
now reconciles the current native summary, archived image and prepared
answers within one viewport. Extend that evidence across bounded image
regions, then qualify layouts that expose different review headings,
repeated records, attachment widgets and unavailable values.

[Summary detail images](g1/SUMMARY_DETAIL_IMAGES_2026-11-18.md) extend this
verification below the initial viewport of the main recorded document.
Separate heading and answer crops preserve exact scoped comparisons.
The [scrollable summary checkpoint](g1/SCROLLABLE_APPLICATION_REVIEW_2026-11-18.md)
extends this to nested scrollports and separately managed embedded frames.
The capture verifies the native answers and restores the original position
before releasing images. Same-origin navigation and broader layouts remain
acceptance work.

[Terminal attachment readback](g1/TERMINAL_ATTACHMENT_READBACK_2026-11-18.md)
binds retained native file inputs to the prepared PDFs after the summary
image is read. Held-out workflows must include both retained selections
and server-managed or detached attachments.

1. Enumerate every required section/control and relevant repeated record again.
   Establish which controls are currently applicable, hidden, unresolved or
   intentionally unanswered.
2. Read back all entered values natively, including formatted values and upload
   widget state. Bind exact submitted-file bytes to the prepared PDF manifests.
3. Have the local visual model inspect the complete final review and targeted
   details. Compare against source-backed intended answers independently.
4. Require agreement of source truth, native state and visual observations.
   Historical entry checkpoints cannot replace present final review.
5. Produce explicit `ready_for_user_review`, `pending_manual_review`,
   `resource_interrupted`, `stale_inputs` or `unsupported_workflow` outcomes.
   No state says submitted or delivered because fields were filled.

Implement the outcome check separately from navigation. Capture the current
summary's native roles, headings, labels and values with its document identity.
Use the hierarchy to associate repeated labels and records and bind that capture
to the screenshot the local model reviews. Compare the observed answers with
the immutable expected packet. BrowserGym's task validator and Playwright's
accessibility-tree assertions provide reference designs for these two checks.
[BrowserGym task validation](https://browsergym.readthedocs.io/latest/examples/create_custom_task.html)
· [Playwright accessibility snapshots](https://playwright.dev/docs/aria-snapshots)

Freeze acceptance cases for a complete summary, a missing required answer,
duplicate labels in different sections, a changed value and a replaced
document during review. Keep the fixture's independent truth outside the
model-visible prompt. Verification remains read-only and saves the evidence
for each mismatch or unresolved item.

### R04.5 — Recovery and useful autonomy

Persist stage boundaries and safe read-only resume points. A restart may
re-observe current state and regenerate stale proposals; it must not blindly
repeat mutations or any uncertain external write. Login, challenge, unsupported
widget and server-dependent situations become a review queue with the original
ad, artifacts, last observations and precise next issue. A bounded public
research repair loop may suggest a general technique, but cannot clear a failed
field or grant authority by itself.

**Acceptance:** complete unfamiliar source-to-final-review workflows, not only
field matches, pass independent truth. Include changed layouts and equivalent
wordings without production branches keyed to the test employer/ATS.

### R04.6 — Execution limits and continuation

1. Audit production limits by entry point. Record the resource or correctness
   property each bound protects and the exact outcome it triggers. Remove
   redundant deadlines, caller wait requirements and counts that silently drop
   eligible work. Qualify each removal through its normal entry point.
2. Keep request sizes bounded while continuing across all eligible work.
   Qualification planning batches questions in required-first order. Template
   planning now compares later passages and job excerpts in resident-context
   windows and reports coverage. Discovery examines complete capped responses
   and uses serialized history size for admission. Next, use durable page-level
   results and a common continuation contract for pending, verified and failed
   work.
3. Use a single task deadline and explicit cancellation across local inference,
   browser control and artifact writing. Estimate how many complete checked
   operations fit before reaching capacity. Checkpoint the last verified state
   when work needs another run. Re-observe current state before resuming.
4. Bind inference input and output budgets to the admitted native configuration
   and representative task measurements. Qualify 256k separately for factual
   retrieval, visual accuracy and host headroom. Keep incomplete native replies,
   allocation failures and visual discrepancies distinguishable in records.
5. Keep search execution independent of UI polling and add independently
   configured provider recovery with durable page cursors, backoff and provenance.
   Preserve source permissions and surface how much of the plan was attempted,
   retained and verified. Verify application routes separately from listing hits.
6. Replace premature progression stops with source-bound current-state evidence.
   Test partial review and continuation through conditional sections, repeated
   labels, custom controls and offscreen attachments. Require equivalent native
   and visual outcomes and intact isolation. Freeze unseen layouts and measure
   whole-workflow completion, review reasons and process-tree teardown.

## R05 — Independent candidate isolation and external-action authority

1. Maintain separate anonymous public-capture and candidate-bearing execution
   contexts. Do not share cookies, local storage, credentials, profiles or
   candidate-bearing URLs between research and public discovery.
2. Qualify transport/OS isolation for fetch, XHR, forms, navigation, Enter,
   uploads/autosave, beacon, images/links carrying data, redirects, popups,
   service/shared workers, frames, sockets and alternate protocols. Include
   already-established channels, not only new requests.
   Startup verifies the sandbox's Windows loopback policy and requires native
   observations from a disposable browser context before mounting source files
   or attachments. Carry the observation into the archived replay evidence.
   Qualify each remaining transport using a positive receiver and a measured
   restricted case.
3. Keep the model outside the authorization boundary. Page instructions, a
   familiar ATS, a learned procedure or a plausible button name cannot enable
   an operation blocked by policy or transport containment.
4. Keep fictional replay interaction and real read-only observation as distinct
   modes. Server-dependent behavior that cannot be faithfully replayed remains
   an explicit limitation; do not manufacture a successful server response.
5. Test stale/revoked grants, changed destinations/content, source corrections,
   duplicate journals and ambiguous completion. Only an explicit user gesture
   can create a one-use hash-bound grant in a future delivery path.
6. Retain the scripted-submit diagnostic and its failed-channel evidence. Do
   not exclude or rename it into a passing no-send test.

**Acceptance:** each newly admitted action channel has independent canary/native
evidence and complete owned teardown. Unsupported channels remain disabled.
No finite suite becomes a universal zero-risk claim. No actual application or
application-email send is part of development acceptance under current authority.

## R06 — Candidate evidence and truthful final documents

1. Import varied PDF/DOCX and supporting files through bounded workers, preserving
   originals and recording extraction/OCR coverage, page counts, encodings and
   unknown sections. Certificates and source images need a defined final-PDF
   path, without altering their original evidence.
2. Reconcile imported facts with reviewed profile/history. Maintain separate
   facts, evidence, preferences and instructions. Propagate corrections to
   answers, document proposals and pending attempts while submitted packets
   remain immutable.
3. Build a full source-backed requirement map for each role. Select relevant
   evidence without inventing achievements, qualifications, employer identity,
   dates, metrics or authorization.
4. Tailor through the existing document tools. Preserve layout where native
   editing is reliable; show a review wall when a PDF replacement cannot fit or
   a conversion changes meaning. A regenerated layout must remain traceable.
   Carry exact source offsets and compare shortlisted passages across context
   windows. Record the scope of each review and retain partial work when a
   model or document worker stops. Exercise long, multilingual and formatted
   inputs through the application UI.
   Automate ordinary local planning/editing/conversion in a staged per-job packet
   under the user's saved preferences; do not require manual passage selection
   for every change. Keep originals immutable and finalize only after source,
   text and visual checks, without converting preparation into send approval.
5. Run natural-writing/voice passes that improve clarity and specificity while
   preserving facts. Do not claim undetectable AI writing or manipulate a score
   by making unsupported candidate claims.
6. Convert every final attachment to PDF. Keep working files separately. A
   failed conversion or a site that refuses the required PDF is pending review,
   not permission to silently attach another format.
7. Check text extraction, reading order, Unicode, headings, dates, columns,
   tables, hidden/overlapping text, page breaks and source coverage. Explain ATS
   improvements as measured parser/relevance properties, not a hiring guarantee.
8. Render every final page and obtain independent local-model visual review.
   Repair within an explicit budget or preserve a review outcome. Bind final
   visual/text evidence and exact PDF hashes to the attempt.

**Acceptance:** varied real/private-source and fictional templates survive
import-to-tailoring-to-final-PDF-to-attachment, with correct refusal cases.
No third-party person's data enters Git or a real employer form. Evaluate licensed
external parser/scanner components before reuse; do not introduce a paid API or
an incompatible redistribution license merely for an ATS score.

## R07 — Broad internet discovery and usable application routes

1. Keep boards/ATS adapters as sources, not the architecture's universe. Generate
   diverse role synonyms, responsibilities, adjacent terms, languages,
   locations, employer types and direct-careers searches from non-PII candidate
   evidence and explicit campaign constraints.
2. Use free permitted providers and direct employer pages with bounded queues,
   host concurrency, retry/backoff, pagination, cache TTLs and durable resume.
   Thousands of URLs must not mean thousands of browser/model processes.
   Replace the single sealed history snapshot with sealed page records and a
   versioned cursor. Read bounded review windows while keeping the entire
   admitted run recoverable. Preserve exact URL provenance across retries.
3. Prefer cheap deterministic filtering and metadata before expensive local
   reasoning or browser rendering. Track query/source coverage and degradation;
   a provider refusal is not zero jobs or successful global coverage.
4. Parse and classify job pages, documents, career indexes, duplicate syndication,
   stale/closed positions, login/challenge walls and email-only applications.
   Retain original ads, source URLs, observed times and relevant structured data.
5. Follow and verify the actual application route read-only. Distinguish a live
   form/profile entry, a generic careers landing page, a broken redirect, an
   inaccessible page, an email route and an unsupported session requirement.
6. Feed resulting jobs through one identity/freshness/ranking pipeline before
   they appear as selectable application work. Recheck freshness before material
   preparation/final review without erasing original observations.
7. Exercise actual search from the product UI with varied private/fictional
   profiles and real public positions. Save failures and route classifications;
   do not substitute searches performed only by the development assistant.

**Acceptance:** labeled retrieval and route outcomes across varied regions and
occupations, provider failure recovery and direct-employer coverage are measured.
Never claim to exhaust the internet or silently weaken an empty campaign.

## R08 — Repeatable match scores and semantic duplicate exclusion

### R08.1 — Match quality

1. Freeze a versioned rubric with explicit requirement, responsibility, seniority,
   skills, domain, location/remote, compensation, language and authorization
   dimensions. Hard constraints remain separate and keep unknown distinct.
2. Extract a complete, source-bound criterion set using stable paragraph/range
   references and duplicate/overlap checks. Replace fixed small-list truncation
   with chunking and completeness accounting.
3. Use local semantic reasoning to evaluate evidence for each criterion. Rust
   validates citations, units, dates, alternatives, contradictions and unknowns,
   then computes the score deterministically from the frozen evaluated rubric.
4. Show reasons, uncertainty and coverage alongside the percentage. A score is
   not a probability of an offer. Store input/model/rubric hashes so unchanged
   assessments are repeatable and changed assumptions are visible.
5. Measure repeatability, paraphrase sensitivity and independently reviewed
   examples across occupations. Calibrate weights on development data and assess
   untouched cases. Report disagreement and intervals rather than inventing
   a precision level.

### R08.2 — Job identity and attempts

1. Store normalized employer/requisition/canonical-route facts and original
   observations separately. Preserve location, contract, department, dates,
   description and source provenance.
2. Retrieve a small candidate set using indexed blocking and semantic/lexical
   similarity; use the model for ambiguous pairs. Exact role-title equality is
   neither necessary nor sufficient.
3. Distinguish same job, changed observation/repost, related but distinct role,
   unknown and conflict. Include misleading same-company/same-title requisitions
   as false-merge negatives.
4. Keep merges reversible with reasons and history. Propagate aliases into
   attempt exclusion across boards, direct sites and email routes.
5. Atomically reserve work before preparation/execution. Concurrent selection,
   crash/restart or an uncertain external result must not create a duplicate
   application attempt or an automatic resend.

**Acceptance:** labeled duplicate/missed-duplicate and fit examples, meaningful
restart/concurrency cases, efficient retrieval at the intended personal-corpus
size, and no unreported scoring omissions. Do not call an exact-URL map semantic
deduplication or a lexical percentage an intelligent overall match.

## R09 — Validated experience and internet-assisted repair

1. Retain structured observations of failures, fixes, source/platform/control
   family, model/profile, document/DOM/image identities and actual outcomes.
   Current experience records are observations for review, not executable policy.
2. Retrieve narrowly relevant past cases. Separate candidate facts from generic
   procedures and anonymous public research. A lesson must not carry another
   candidate's value, account/session information or destination authority.
3. Make lessons scoped, versioned, expiring and revocable, with explicit negative
   evidence and conflict handling. Re-observe the current page before acting.
4. Validate proposed procedures on matched development and changed held-out
   layouts with and without memory. Promote only measured improvements without
   critical regressions; keep rollback immediate.
5. Let the local agent request bounded public research when current evidence is
   insufficient. Retain failures and provider budgets. Retrieved advice cannot
   supply missing candidate facts, clear a failed field or authorize an action.

**Acceptance:** measured improvement in complete workflows, not successful
replay of one memorized selector. Expired, wrong-family and malicious lessons
must be refused without suppressing current intelligence.

## R10 — Archives, email routes and lifecycle workflows

1. Make the per-job/per-attempt directory contract complete and versioned:
   original ad and provenance; originals/working material; final PDFs and hashes;
   all questions/answers/sources; before/after field captures; final review;
   model/native metadata; errors/manual-wall reasons; and the durable journal.
2. Write manifests/checkpoints atomically. Preserve immutable attempt history,
   correction annotations and incomplete artifacts. Recover from partial writes,
   crashes, missing files and disk pressure without falsely reporting success.
   History now paginates by bounded identity and ordering metadata and retrieves
   large saved reports on demand. The compiled route preserved an archived
   468,306-byte report and the renderer rejected stale detail responses.
   Measure peak memory with a populated history and qualify explicit incomplete
   outcomes when the manifest or source artifacts cannot be stored.
3. Build email-only application packets through the same facts/documents/identity
   pipeline. Parse addresses and requested subject/body/attachments from the ad,
   retaining uncertainties and exact destination bindings.
4. Consolidate Gmail/other supported mail configuration with DPAPI-protected
   credentials, connection status and appropriate account access. Revisit the
   authorized related-repository mechanism read-only before adopting more code;
   retain provenance/license review and current protocol tests.
5. Keep application-email sending disabled during this work. Future transport
   requires one-use user authority, an append-only write journal and uncertain
   delivery semantics. SMTP/API acceptance is not proof that a recruiter read
   or processed an application.
6. Exercise only the specifically authorized fixed-address self-test when a
   changed mail mechanism needs it; do not resend unchanged connectivity tests
   repeatedly. Never print/store app passwords or tokens in tracked artifacts.
7. Connect relevant lifecycle proposals to usable UI flows: inbox reconciliation,
   role-specific status, follow-up drafts, contacts, interview preparation and
   offer comparisons. Keep proposals distinct from external messages/events.

**Acceptance:** a failed or manual attempt still has a complete recoverable
record; email and web routes share duplicate exclusion; no ambiguous external
write is retried automatically. Lifecycle library methods alone do not complete
their product flows.

## R11 — Portable Windows installation and long-context integration

1. Package verified, licensed private runtimes for the supported Windows/CPU
   target. Discover capabilities, not this PC's absolute paths. Preserve explicit
   custom paths and leave unrelated installed applications/services unchanged.
2. Make first setup, missing-component repair, interrupted/resumed downloads,
   checksum failures, disk limits and rollback work through the web UI without
   console prompts or persistent ad-hoc services.
3. Use the current native 256k Q8 result as a capacity candidate, not a promotion.
   Wire attention/cache/context into the owned runtime profile and qualify its
   larger reported CPU allowance, actual host reserve and whole-workflow memory.
4. Compare effective 16k/32k/64k/128k/256k profiles where useful. Verify native
   context/cache acceptance and actual input token counts. Evaluate factual
   retrieval and completion over long inputs, not just a one-word request.
5. Keep requested and effective settings visible. If a requested profile cannot
   be admitted, give the actual resource/capability reason; do not silently use
   a smaller context or a different model.
6. Verify encrypted jobs/settings, secret envelopes, migrations, backups, restore,
   workspace relocation and account boundaries. Readable application archives
   remain intentional; temporary sensitive material needs an explicit lifecycle.
7. Test installation, normal use, repair, update/rollback and uninstall on clean
   Windows accounts/VMs with no preinstalled runtime assumptions. Do not make WSL,
   Docker, host Python or Node end-user requirements.
8. Review dependency/action versions and detector inventories before a candidate
   checkpoint. Fix underlying findings and rerun detectors; never dismiss,
   suppress, baseline away or manually resolve them.

**Acceptance:** the supported clean-host matrix installs and operates without
manual dependency rescue, existing host data remains intact, and resource-heavy
failure paths return to an independently verified clean owned state. Signing and
release/update trust decisions remain explicit user-controlled release work.

## R12 — Complete, consistent interface and interaction design

1. Inventory every top-level screen, settings category, dialog, subwindow, menu,
   empty/loading/degraded/error state and important interaction. Tie each control
   to actual backend behavior; remove fabricated status and inert options.
2. Run fresh original image-based design rounds for the whole workspace,
   documents/application review, settings/setup and archive/manual-review flows.
   Convert the chosen direction into measurable tokens and component behavior,
   not a loose visual suggestion. Retain earlier references as history.
3. Preserve Smart Jobseeker branding, the current shared visual system and
   centralized styles in `workspace-theme.css`. Keep appearance and pane sizing
   in `layout.js`; do not duplicate resizers or per-screen motion rules.
4. Use visible mouse-drag grips with correct cursor, bounds and feedback for
   resizable panes/dialogs. Define useful minimum/maximum sizes and recoverable
   persisted state. Do not require keyboard controls for resizing.
5. Keep interactions consistent: focus/hover/pressed/disabled/pending/error,
   drag/select/upload and save/cancel. Respect reduced motion and avoid delays
   or effects that obstruct work.
6. Validate populated and empty states, large job lists, long labels, mixed
   languages, all menus/dialogs and nested scrolling. Include narrow/mobile,
   wide/ultrawide and zoom/DPI/orientation changes where supported.
7. Exercise actual browser zoom and Windows scale separately from CSS geometry
   emulation. Inspect screenshots as well as overflow/hit-target measurements.
   Check fixed navigation, sticky headings and modal backdrops for occlusion.
8. Compare implemented screens against approved references and interaction
   contracts. Record remaining differences; do not claim pixel parity from
   geometry tests or a generated picture.

**Acceptance:** every visible control works, same-category components behave
consistently, all required resize/zoom states remain usable, and the actual UI
completes the core journey. Headless testing must not take focus or manipulate
the user's desktop.

## R13 — Maintainability, observability and efficient development

1. Extract stable API/worker/result contracts from the largest mixed-concern
   modules. Separate transport, domain decisions, profile assembly and handlers
   while preserving public routes/storage compatibility and proven invariants.
2. Determine callers of older `smart_job_core` paths before removing duplication.
   Avoid parallel scoring, inference, resource and UI mechanisms.
3. Centralize typed errors and progress records. Report failed capabilities,
   provider errors, omissions, native stop reasons and cleanup uncertainty.
   Keep full private evidence off the console and outside Git.
4. Keep one current overview, detailed plan and active queue. Date immutable
   evidence separately; stop repeatedly prepending history to every status page.
5. Use source-bound search, exact verification and bounded reads. Serialize a
   namespace's semantic queries. Retain first failures and do not reconstruct a
   run from selected successful excerpts.
6. Run focused regressions first and the required integration checks once per
   coherent change. Repeat only after relevant changes, failures or unresolved
   concerns. Test native behavior and adversarial outcomes rather than copying
   implementation constants into redundant assertions.
7. Preserve finding authority across every detector and mirror. Reopen prior
   agent-manual closures when supported and attributable; never reopen automatic
   fixes by undoing real code. Record inaccessible providers honestly.
8. Use substantial local commits on `main`, secret-check the staged scope, and
   push coherent tested checkpoints less frequently. No autonomous release,
   new branch, unsolicited service/autostart or delegated agent is introduced.

**Acceptance:** changed features have reviewable boundaries and evidence; task
handoffs identify source, active handles/resources, results and next work. A later
agent can resume without repeating research or mistaking plans for measurements.

## R14 — Readiness evidence and user decision

Build a requirement-to-evidence ledger for every explicit product requirement,
including default-Qwen intelligence, generic model contracts, broad discovery,
route reachability, calibrated fit, near-duplicates, templates/final PDFs,
complete controls/navigation/review, archives, email, learning, settings,
installation, encryption, resource ownership and full UI behavior.

For each requirement record the authoritative source and result as **proved,
contradicted, incomplete, indirect or missing**. Match scope: passing units do
not prove installed behavior; fields do not prove workflows; a short 256k
request does not prove 256k accuracy; remote CI does not prove an unpushed tree.

The readiness review requires:

- A current source/profile-bound independent journey report, including untouched
  cases, manual walls, incorrect/unsupported entries and all denominators.
- No unresolved critical correctness regression in the accepted scope; explicit
  limitations and confidence, without a universal bug-free or superiority claim.
- Current local checks and matching hosted checks for the submitted checkpoint,
  with native/live suites and exclusions distinguished.
- Clean-Windows installation/lifecycle, storage recovery and runtime ownership
  evidence for the supported target matrix.
- Actual interface acceptance against the design/interaction contract, including
  settings, dialogs, resize, zoom, long content and failure recovery.
- Immutable archive/journal, correction, duplicate and external-action authority
  evidence. Current no-send development restrictions remain enforced.
- Current detector inventories without manual closures/suppressions, dependency
  and license provenance, staged secret checks and no private candidate data.

Only when that ledger supports readiness should the agent report **ready for the
user's release decision**. The user decides whether and when to release. No
release, tag, publication or application sending follows from this plan.

## Research findings informing this direction

The [current Stagehand repository](https://github.com/browserbase/stagehand)
describes separating natural-language observation from deterministic browser
actions and recovering from changed pages. The useful design lesson is to keep
typed observations/actions and current-state repair distinct. Its documentation
does not qualify our local model, authorize a cached action or establish a
free product-wide backend for us.

[AgentLab](https://github.com/ServiceNow/AgentLab) documents benchmark development,
trace analysis and reproducibility controls across environments. Apply that
discipline to source, browser/runtime versions, task partitions and complete
outcomes. Do not import its default cloud/visible environment or count its
benchmark results as ours.

The [Ollama FAQ](https://docs.ollama.com/faq) documents KV-cache precision/memory
tradeoffs; the [Qwen model card](https://huggingface.co/Qwen/Qwen3.8-27B) documents
native context and recommended generation controls. Our actual 256k allocation
trials and retained task failures take precedence over an assumption that more
context or effort guarantees improvement.

Simplify's documentation describes continuous autofill on supported pages,
manual fallback for unsupported forms and a final review step. Use those as
workflow comparison points. Test against changing native layouts and keep
records usable after a failure. [Simplify Copilot documentation](https://help.simplify.jobs/en/articles/2415391-use-copilot-to-autofill-applications)

Bound rechecks by specific failed criteria and retain the first outcome. Use
structured model output for typed proposals and verify the meaning against local
evidence. [Building effective agents](https://www.anthropic.com/research/building-effective-agents), [Ollama structured outputs](https://docs.ollama.com/capabilities/structured-outputs)

These primary pages were read through bounded isolated captures on
17 November. Stagehand capture SHA-256:
`692f6d609dd44cc4a7db6fd315617ca83c8b3756619696a1ed4a0bce8b3c89b8`;
AgentLab capture:
`a7fd56a6f636846aa9343eaf329d0d48937f0d0fb5312077cddf991068f0bd3a`.
Simplify, Anthropic and Ollama structured-output captures respectively:
`944a5aa6b9ab520c2b94e48330f3a2677b9cabec902f8f5f6cf4b69666ae7ae5`,
`3ba5d890c6f91feca3db914a9e6fb5084afefbc76285c314c42c3c14009d91d5` and
`16da72be78168122a06b2ca37bcfbbfaf7ea3ba6da58b38772970bd0a5e87210`.
The current batch adopts architectural ideas, not their code. Any later code or
tool adoption requires exact-source/license review and free-runtime verification.
