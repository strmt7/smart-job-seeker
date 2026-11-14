# Native file-label detail views - 14 November 2026

Baseline: `42704c9d787caafba02a5ad46fc00fbfc21b8246`.
This addresses the retained PDF-only missing visual observation. It adds a
separate native-bound detail image, not an expected answer, label hint or
application capability. The original image, first observation and review
barriers remain intact. All development interactions use fictional offline
fixtures in headless browsers; no employer form is changed or application sent.

## Root cause and implementation

The previous PDF-only case selects the correct CV bytes but transcribes only the
neighboring Country control in both passes. The original upload-feedback image
contains several controls. A file label can also be a block whose element box
spans most of the viewport; that empty layout space exceeds the existing detail
budget even when its visible text and chooser fit comfortably.

Main-page file controls now obtain a separate detail image from the existing
native-associated context mechanism. Only this fixed native file caller opts into
text-range geometry for labels. Native `labels`, unique `aria-labelledby`
references and enclosing group/legend associations identify the source regions.
Actual rendered text rectangles and the complete file-control rectangle determine
the crop. No model-proposed label, filename, answer, company-specific selector or
DOM mutation enters that query. Geometry is checked again after the capture.

The query is bounded by existing source/reference/ancestor/image limits plus
512 walked nodes, 8,192 label characters and 128 text rectangles. Hidden, clipped,
distant, generated, ambiguous or wrong-type context refuses the detail. Generated
content cannot be silently dropped from a text-range view. Its original primary
image and any refusal remain available; a detail refusal does not manufacture a
whole-form success or a new observation. Text/select control crops keep their
original element geometry. Embedded controls retain their previous behavior.

The existing target and after-entry comparers require native-associated detail
when present, preserve initial observations, compare exact values, reject
contradictory overlapping observations, and bound missing-target repair. Native
document/schema/control identity, file-byte verification, sensitivity and review
gates still run. A tight image does not establish PDF validity, candidate facts,
per-page document quality, widget acceptance or server validation. Selection is
not approval or send authority.

## Native verification

Private evidence: `target/upload-vision-onepz29x/`.
The first test build exposed that the new fixture called a private method from a
sibling module. The test was moved to the owning module without widening the
production interface; that compiler failure is retained. The subsequent explicit
native selection passes two tests: the existing four-case text/select context
matrix and a new twelve-case file matrix. Every contained browser closes.

The file matrix admits wide block labels, wrapped labels, referenced labels,
multiline text and accented nested text. It refuses hidden labels, clipped text,
distant labels, generated content, hidden children, duplicate label references
and an actual text input passed as a file. It checks current native form equality,
unchanged original context dimensions, associated bounds, size limits and actual
retained PNG bytes. The wide-label case proves that the old element-box detail
exceeds its budget while the new native text-bound view fits. Native readiness
requires AppContainer isolation with zero capabilities. These are read-only
image-geometry cases, not local-model accuracy or application completion.

Wide, multiline and accented native captures were inspected directly. Their
visible label and native chooser are complete without the Country neighbor.
The current ordinary Windows workspace passes **1,420 tests**, with **617 opt-in
tests ignored**, across 43 result groups. Warnings-denied Clippy, JavaScript syntax
and formatting checks pass. The published baseline's hosted CI and coverage are
also verified green, separately from this working-tree change.

The complete fixture-only native browser regression passes **139/139**, with
the four independently gated/public probes explicitly excluded as documented in
AGENTS.md. The canonical SQLCipher debug build passes. Four frozen local-model
cases are recorded below after completion. Earlier failed observations and their
original denominators stay retained. No held-out or universal accuracy claim is
made, and all overall release acceptance criteria remain open.

## Local-model result

Private controller: `target/upload-vision-onepz29x/model-ckut2p7o/`.
All four authored layouts and source document hashes freeze before the first
inference. Actual compiled HTTP routes, private owned inference, an independently
frozen schema-3 value/control oracle, native file-byte checks and before/after
image bindings are used. Each upload role has distinct PDF bytes. These are
known development cases, not representative held-out data.

| Case | Verified / answerable | Unknown inputs entered / unknown inputs |
| --- | --- | --- |
| Saved contacts and PDFs (regression) | 5/5 | 0/1 |
| PDFs only (retained failure) | 2/2 | 0/4 |
| Referenced labels with a document group | 2/2 | 0/4 |
| Wrapped labels | 2/2 | 0/4 |

All **4/4 development field gates pass**, totaling **11/11 answerable fields**
and **0/13 unknown inputs entered**. Every case has zero incorrect values,
unverified performed entries, unexpected entries, ambiguous matches or unbound
entry identities. The two new layouts are frozen separately rather than counted
as repeats of the original page. Model decisions assign the selected CV and
cover letter to their respective controls and leave the unavailable professional
certificate unassigned. PDF-only cases do not substitute contact values.

The recovered PDF-only case retains the **byte-identical original upload image**
from the earlier failed run: SHA-256
`71058d59f2a0f4f6c894fa6ba399a2d5714f1fb4f730789a295767033ecb40f6`.
Its initial transcription still reports only `Country: Choose`. The separate
native-bound detail has SHA-256
`18588a06cc1e5ad4236b85d8412fb74d1017f51a1986dd0671c82929ac55abf2`;
the blind second observation reads `Upload your resume: resume.pdf` without
receiving that expected label or value. Both observations and image identities
remain preserved. The actual post-entry detail was inspected directly; that
inspection is not substituted for the recorded local-model observation.
Earlier failures stay immutable and are not relabeled as successful runs.

The model is `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
context 4,096, owned runtime 0.40.16. The four-case controller takes 536.813 seconds.
Native aggregate app-job peak commit is 9,356,009,472 bytes; minimum sampled
whole-host available RAM is 21,955,944,448 bytes. Runtime API and whole-adapter GPU
samples remain separate scopes; neither establishes universal GPU residency or
sustained leak freedom. All four private model sessions close, shared model
inventory remains unchanged, and the containing app tree drains. Native
source/binary/controller identities and full records remain private.

No whole-workflow gate is claimed: these six-control fixtures intentionally
contain unresolved required inputs and no application delivery endpoint. Final
workflow review, repeated histories, varied widgets, embedded file details,
representative held-out cases, final factual PDF/ATS review and the remaining
installation/lifecycle/release gates are still open. Candidate PDF-only visual
recovery no longer blocks the next reviewed-facts/history integration tranche.

The final scoped host audit
`target/development-host-d1de2e6c5f864aa0aba566b8c30a916f.private.json` observes
zero matching repository/profile processes, resident Ollama models, services,
scheduled tasks, startup items or errors, with 29,939,560,448 available RAM bytes.
The existing private fixture cache is retained on disk for verified reuse. No
unrelated workload was terminated. This is teardown evidence, not a long-running
memory-leak test. Frontend and dependency source are unchanged; their previous
identity-bound evidence is reused within that scope.
