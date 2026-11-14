# Selected candidate PDFs - 14 November 2026

Baseline: `e94eb8e9919bd22295692456f39d1ffb4d115701`.
This connects explicitly selected saved PDFs to the network-isolated saved-page
actor. It does not qualify employer-site interaction, document truth/layout,
complete candidate histories, a complete application journey or release readiness.

## Inputs and records

The shared saved-page controls offer an optional **Candidate PDFs** disclosure.
Nothing is selected automatically. Each row identifies a saved PDF and an explicit
purpose: one CV, one cover letter, or supporting documents with a short declared
purpose. Search narrows long document lists, retaining selected items. Removal and
validation use the same controls at narrow widths and browser zoom. Preview-only
requests ignore rehearsal inputs. The server rejects candidate inputs without an
offline rehearsal, unsupported roles/fields, duplicate IDs/primary roles, invalid
IDs, excessive counts and unsupported supporting purposes.

The authenticated server obtains immutable document IDs from the encrypted store,
checks individual metadata before reading content, and stages sequentially. Limits
are 16 PDFs, 10 MiB each and 64 MiB in total. DOCX/other formats must first use the
existing PDF conversion/export journey. A PDF filename/signature check is an input
filter, not proof that a PDF parses correctly, is ATS-compatible, has sound layout
or contains true candidate facts.

Each attempt retains the selected bytes in `materials/rehearsal-documents/`, with
neutral upload names (`resume.pdf`, `cover-letter.pdf`, `supporting-N.pdf`). The
immutable version-2 `screenshots/rehearsal-candidate.json` binds source document
IDs, explicit roles/purposes, size, actual byte digests, job identity and the
separately selected contact projection. Public capture-worker requests carry only
the snapshot digest, not candidate values, document IDs, paths or PDF bytes.
Version-1 contact-only snapshots remain readable.

The offline renderer checks the exact snapshot schema/digest, catalog bounds,
regular local paths, job binding, file size/signature and actual PDF digest before
model/browser admission. No arbitrary incoming attachment configuration survives.
The browser's existing checksum-bound attachment inventory and native selected-file
verification remain required, along with independent before/after visual checks.
A changed file, uncertain target, unsupported widget, page feedback or incomplete
observation stays in review. Native byte identity alone never proves widget or
server acceptance.

The existing local-model purpose matcher receives opaque document aliases and
explicit purposes, rather than filesystem paths or inferred filename meanings.
Its decisions resolve only to that snapshot's verified files. Contact matching's
manual disposition of a file field no longer vetoes a separate document proposal;
native sensitivity/credential exclusions and the document actor's review decisions
still apply. No new browser action or network authority is introduced.

Selecting only PDFs supplies an empty contact profile. Selecting saved contacts
additionally snapshots only the existing allowed contact keys. Neither candidate
mode substitutes bundled fictional contact values or documents. Entry records keep
candidate-document provenance and selected byte digests separately from the
actual native/visual observations. Selection does not establish document approval.
Corrections still require the broader fact/document invalidation journey.

## Verification

Windows workspace: **1,420 passed / 616 explicitly ignored / 43 native groups**.
Clippy with warnings denied, the canonical debug SQLCipher build and formatting
checks pass. All **171 UI scripts** pass. These checks do not include the opted-out
live model/browser tests. No dependency graph or native browser-core change is
part of this batch; earlier 139-case browser and dependency/license evidence keeps
its original exact-source scope.

The new regressions cover immutable selected byte copies, source/role identity,
distinct CV/cover hashes, contact versus document-only input, cross-job refusal,
tampered file/catalog/path/alias/size/digest, missing/wrong-format/oversized files,
duplicate/unknown input and UI filtering/selection/removal/request behavior. Store
metadata lookup is keyed, including the existing sealed-name read path, rather
than scanning all saved documents or allocating every document's bytes.

Private evidence stays under `target/candidate-pdfs-y02koclu/`.

The first completed two-case local12B run is retained in
`native-3zhounj8/result.private.json`. Both cases correctly propose the CV and
cover-letter destinations and abstain on the unknown certificate. The contact
case verifies 3/5 answerable fields; the PDF-only case verifies 0/2. Each selects
the correct native CV bytes but lacks its independent visual observation and
halts before the cover letter. Each therefore has one unverified entry, zero
incorrect values/unknown/unexpected entries, and **fails** its field gate: 0/2
cases pass. This is not successful document or workflow qualification.
The inspected original image clearly contains the selected filename and several
neighboring controls; the local model transcribes only the Country neighbor.
Both original image bytes and raw observations are retained.

The after-entry review previously returned immediately when no supplementary
native detail image existed. It now applies the same bounded missing-only blind
coverage gate to the original immutable image. It supplies no expected label,
filename or answer; the original observation, comparisons and image identity
remain retained. One coverage pass inspects all visible controls and explicitly
copies displayed attached filenames. Existing contradiction, duplicate-label,
observation-limit, native identity and overlapping-value checks remain in force.
Mismatches never become another guess, and native-associated detail remains
required when present. A new adversarial regression verifies correct filename,
wrong document filename and conflicting neighbor observations separately.

The production recheck continuation is retained in
`native-5vok44rg/result.private.json`. The saved-contacts case now verifies **5/5**
answerable fields, including distinct native CV and cover-letter bytes plus
independent visual transcriptions; the unknown certificate remains untouched.
There are zero incorrect, unverified, unexpected or unbound performed entries in
that case. The PDF-only case still transcribes only the neighboring Country
selector in both blind passes. It selects the correct native CV bytes but remains
visually unverified, halts before the cover letter, and fails its field gate.
Final scope is therefore **1/2 development field cases**, not two successes or
complete-workflow qualification. All four unresolved contacts/certificate inputs
in the PDF-only case remain untouched. This failure is a priority for the next
generic native/visual robustness tranche; do not tune by supplying expected
answers, discard its initial observations or relabel it a successful upload.

Both final model sessions close, shared model inventory stays unchanged and the
owned process tree drains. Native aggregate peak commit is 9,280,262,144 bytes;
minimum sampled whole-host available memory is 26,589,384,704 bytes. The model is
`gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
context 4,096, owned runtime 0.40.16. Source/binary/controller identities and the
before-inference fixed inputs are retained privately. Subsequent UI-only wrapping
and capture-controller changes have their own compiled/rendered evidence; they
are not silently relabeled as a new model run.

The first 44-case rendered geometry/mouse/zoom run returns success, but independent
inspection finds a large unpainted region in its 200% picker screenshot. Its
original report/images and a separate failing visual-review receipt are retained
in `target/visual-followups-ui-LE3YrE/` and
`target/candidate-pdfs-y02koclu/ui-first-review.private.json`. Geometry and simple
pixel diversity do not prove a complete screenshot. The capture helper now awaits
font readiness and two animation frames after zoom/scroll before native capture.
Rendered requalification and direct image inspection are recorded separately.

The first native attempt stops before model startup: 14,657 MiB available versus
its conservative 16,867 MiB estimate. Its app-job peak committed memory is
167,768,064 bytes and its tree drains. Sampled whole-host available memory drops
to 12,953,567,232 bytes; that is not attributed to this app. A subsequent host
audit observes zero repository-matching processes, services, tasks, startup items
or resident Ollama models, and 22,122,627,072 available bytes. Another
`llama-server` is independently visible in the host process inventory; it is not
claimed as task-owned or stopped. The first failure is retained and supplies no
model-quality evidence. Retry uses the first run's verified, private fixture
workspace/cache; it never changes the user's workspace or shared model inventory.

The first completed native run drains both model sessions, leaves shared model
inventory unchanged and drains the containing tree. Native peak aggregate commit
is 9,274,302,464 bytes; sampled whole-host available RAM stays at or above
24,978,829,312 bytes. The runtime's API memory figures and sampled whole-adapter
telemetry are separate scopes, not proof of GPU residency or sustained leak
freedom. These are two development fixtures, not held-out or cross-ATS evidence.

## Rendered continuation and remaining scope

The repeated **44-case** compiled-UI run passes in
`target/visual-followups-ui-Kcxxna/`, wrapper
`target/layout-check-run-k9zqzvc0/`. It exercises 320/390/768/1,440/1,920 px
viewports, independently constrained 240/280/360/520 px picker widths, actual
80/100/125/150/200/400% browser zoom, mouse toggles, explicit purpose/PDF choices,
search preserving selection, a supporting-purpose text field and row removal.
Rows wrap according to available pane space, including a narrow pane at a wide
viewport. The original 390 px/1,440 px images and the repeated 200% image were
inspected directly. The repeated 200% image is fully painted where the earlier
capture was incomplete. That supports the timing correction for this capture;
it does not establish absence of every possible rendering defect. The prior
failed image/review stays intact, and a separate final-review receipt records
the continuation. Native app/context/browser teardown checks pass.

R04 remains open: the retained PDF-only missing visual observation, varied upload
widgets/contexts, reviewed candidate history/facts, repeated sections, final
native/visual workflow review and held-out end-to-end qualification. R06 still
requires diverse factual template/tailoring/ATS/per-page final PDF acceptance.
Anonymous research schema/finalization and useful remediation remain separately
unqualified. Selection is not document approval or application-send permission.
No release or real application interaction/delivery occurred.

Final focused regression (after its test-name clarification) passes 1/1; final
warnings-denied all-target Clippy, build, formatting and staged secret scan pass.
The staged Gitleaks 8.30.1 report has zero findings; no private runtime/model/log,
profile, source archive or credentials are staged. No dependency graph changed.
The final CSS whitespace cleanup removes one trailing blank line after the
rendered run; executable rendering semantics are unchanged.

Final host audit `target/development-host-8f661f7d393b43bda89646f2d209fc0a.private.json`
observes zero repository/profile-matching processes, resident Ollama models,
services, scheduled tasks, startup entries or errors, with 37,373,149,184 available
RAM bytes. An earlier audit matched its own outer command shell; exact PID lookup
found that shell exited, and the repeat invocation avoids that argument match.
No unrelated workload was killed. This checkpoint proves scoped teardown, not
sustained absence of every memory leak.
