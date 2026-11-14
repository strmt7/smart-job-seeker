# Reviewed qualification answers and atomic corrections — 14 November 2026

Baseline: `4ea8a3959c6f0e0b373336f33ef384affe5a4625`, followed by this
checkpoint's qualification/correction changes. This is partial development
qualification, not employer-site, complete-workflow, held-out or release acceptance.

## Connected behavior

The shared offline controls offer **Use reviewed qualifications**, initially off.
Only a snapshot digest enters the capture-worker request. Version-4 candidate
snapshots retain selected active reviewed claims, IDs, source kinds, revisions,
text digests, writing instructions and the full active-facts digest. Existing
versions remain readable. Unreviewed/inferred/disputed/unknown claims cannot
become usable qualifications through this projection. At most 32 eligible facts
are retained from the existing bounded reviewed pool; omissions and oversize
counts remain visible. Invalid profile/instruction types are refused. Facts-only
mode adds no demonstration contacts, history or attachments.

The actor reuses `answer_task` selection, drafting, natural wording and factual
review. It separately checks whether the final text answers the actual question.
Entry requires completed supported evidence, no numeric uncertainty, current
sources, field constraints and an accepted question check. Proposed values and
source IDs are re-resolved before the existing native and independent local-model
before/after image checks. Four offered text questions and 24 model calls per
planning pass are explicit bounds; unavailable/unanswered/excluded inputs remain
in review. This does not support every control or arbitrarily long answers.

Plans, accepted answers and available rejected-draft/audit observations are
separate from actual entry results. Immutable checksum-bound files live under
`answers/facts-plan-NNN.json` and `answers/facts-entries-NNN.json`; source snapshots,
original source and screenshots remain in the attempt folder. Raw generation
receipts have the existing explicit record/byte limits and omission accounting.
Local technical-research guards include qualification values without adding them
to anonymous queries or granting the rehearsal browser network authority.

## Corrections and transaction boundaries

The profile UI now offers an editable qualification disclosure with Save/Cancel,
source comparison and inline errors. The authenticated correction route compares
the exact active text the user reviewed. An immediate SQLite transaction contains
source replacement, revision advancement, dependent-packet invalidation, approval
expiry and eligible job-state correction. A shared state-transition helper retains
the existing domain checks. Error/panic rollback and nested-transaction refusal
are tested; no migration was changed. Submitted records remain untouched.

A facts revision is independent of the contact/profile digest. Changing facts
cancels a bound rehearsal, retaining its task until teardown is observed. Cached
status and archive views show current/stale/unavailable facts without rewriting
old results. The compiled UI qualification saves a fictional correction once,
then refuses a stale editor's second submission. No automatic retry occurs.
Crash/power-loss recovery and arbitrary concurrent-host behavior remain separate
acceptance work; these scoped tests do not establish them.

## Local-model result and retained failures

Two frozen fictional layouts change question wording/order, the language fact
(Rust to Python) and experience fact (4 to 7 years). Exact expected values and
required source IDs freeze before the corresponding model run; the catalog is
audited separately afterward. Inputs are authored inert replay fixtures with
explicit synthetic HTTP metadata, not observations of real employer responses.

The first owned `gemma4:12b-it-q4_K_M` run completed its controller but passed
**0/2 field cases**: both performed entries violated an explicit short-value
format. The initial drafts were correct short values; the natural-wording request
omitted the original question and expanded them. A question check sometimes
accepted the expansion. Native/visual agreement alone did not prove correctness.
These original responses, results and images remain retained.

The shared drafting/wording requests now preserve safe question-format constraints
and give the wording pass the original question. The question audit explicitly
checks format as well as factual meaning. The same frozen cases were repeated,
without changing their expected values:

| Final development result | Measured |
| --- | --- |
| Independently verified answerable fields | **2/4** |
| Whole declared field cases passed | **0/2** |
| Unknown answers entered | **0/4** |
| Incorrect / unverified / unexpected / unbound performed entries | **0 / 0 / 0 / 0** |
| Required source-ID/digest mismatches | **0** |
| Numeric answers still withheld | **2** |

Rust/Python are now entered in the requested format and visually verified.
Bare numeric answers remain blocked by the existing numeric/unit support checks;
invalid source indexes during a repair are also rejected. Those guards were not
weakened. Typed source-unit/question-unit binding is the next implementation
priority. Neither case, generic qualification completion, paragraph quality nor
whole-workflow readiness is declared successful.

Model digest: `6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
context 4,096. The final controller completed in 209.006 seconds. Aggregate app
peak commit: 9,364,402,176 bytes under its unchanged 10 GiB job limit. Minimum
sampled host available RAM: 18,551,709,696 bytes. Owned model exit after a live
facts correction and `pending=false` are observed; the shared runtime stays
unchanged and the tree drains. Earlier archived results become stale without
changed bytes. This is runtime-live invalidation, not a cancellation-latency,
whole-adapter GPU-residency or sustained-leak measurement.

## Checks and interface inspection

Windows workspace: **1,442 passed, 617 opt-in tests ignored, 43 result groups**.
A final profile/instruction-type guard then passed all three focused qualification
tests. Warnings-denied Clippy, formatting and the canonical SQLCipher debug build
pass. All **173 JavaScript test scripts** pass; the subsequently strengthened
source-copy link test also passes. No dependency graph changed.

All **61 rendered checks** pass, including qualification provenance, stale-source
notices, correction controls at 320/390/768/1440/1920 pixels, 200% zoom and the
compiled correction interaction. The first geometry pass left a browser-default
editor; actual screenshot inspection caught it. Shared theme typography, a full-
width field, vertical label layout and spaced actions replace it. New computed-
style assertions and inspected 390-pixel/200% captures confirm that refinement.
The earlier captures remain retained; geometry alone was not treated as design
acceptance. App exit, context closure and browser disconnection are observed.

The scoped final local audit records zero matching processes, services, scheduled
tasks, startup entries, resident shared models and inventory errors, with
29,346,385,920 bytes available RAM. A clean snapshot is not proof of sustained
leak freedom. Private evidence: `target/candidate-facts-s39iwmr6/`; no personal
materials, credentials, model caches or runtime logs are tracked. No real employer
form interaction, application email, application delivery or release occurred.
