# Saved-upload diagnostics and visible review context

This advances the existing R03/R04 development work. It does not complete the
form engine, authorize live uploads or establish release readiness. Real forms
remain read-only. All file selection below used a previously archived page in
a zero-capability headless AppContainer and the fictional bundled résumé.

## What the native diagnosis establishes

Two private native protocol probes enabled runtime/log observation before
navigation, inspected the bound file control and selected the same staged PDF.
The native browser emitted one trusted input event and one trusted change event.
The control was connected, enabled and had an installed React change handler.
The selected FileList retained the expected filename, size and PDF type. These
are native actuator observations, not local-model or complete-workflow credit.

The saved page had already reported a failed uploader-setup fetch. Its handler
requires a successful server-backed upload before assigning a displayed
filename. After selection it catches an unavailable-uploader error and shows
that error below the chooser. The new full diagnostic screenshot exposes the
message; the earlier tight crop omitted it. Five unhandled runtime exceptions
were observed before entry, none newly observed after entry in the second
probe; the visible upload error was caught by the page itself. Absence of a new
unhandled exception therefore does not prove success.

The probes completed in 10.157 and 10.111 seconds respectively (native controller
elapsed values; outer containment includes small additional overhead). Both
owned process trees exited and the worker reported complete cleanup. No cached
non-read reply or unmanaged child session occurred. Private reports, runtime
messages and images remain under `target/widget-runtime-8beek94o/` and
`target/widget-runtime-vei3cvhq/`; native controller/source identities remain in those immutable reports.

This rules out a missing native input/change event in these two runs. It does
not prove that every dependency is understood, that the earlier Other-type
cached-page refusal caused this failure, or that replaying that request would
repair the upload. No server response is invented, no upload token is added to
an archive, and no replay origin/type rule is weakened.

The current official React decoders describe a server/client rendering mismatch
and recovery by client rendering ([418](https://react.dev/errors/418),
[423](https://react.dev/errors/423)). They provide context for the observed
warnings, not proof of the archived site's React version or permanent failure.
Both pages were robots-checked and successfully extracted through the isolated
provider, retaining native HTTP200 results/version/digests. The first attempt
used the internal research module directly and failed before network access;
using the documented tool entry point restored the reads. The failure remains
retained. Private receipts: `target/widget-diagnosis-qzchvj0z/`.

## Product changes

Native file-input context screenshots now spend the remaining existing
400-pixel vertical budget below the control after preserving heading space. The 720-by-400 CSS-pixel ceilings are unchanged. Main-page and embedded-frame
post-entry file context use the same geometry, clipped to the exact visible
page/child viewport. The existing embedded parent-capture path renders at scale
two, up to 1,440 by 800 raster pixels. Pre-entry target images and other control
crops retain their earlier geometry. A bounded crop still cannot prove every distant message is visible.

Each saved-copy entry check now records the delta of acknowledged replay
counters over its entry and native/visual review interval. Missing, wrong-mode,
malformed or regressing counters are explicitly unavailable; they do not become
zero. This is interval activity, not causality, server acceptance or proof that
all browser channels were observed. It never changes an entry's verification
status or grants authority. It contains no URL/header/body/form-value payload.

The record interface shows blocked requests alongside the affected check and
keeps the website response unverified. Unavailable activity is explicitly shown.
Attachment selection still requires separate native byte and visual checks;
visible errors and missing filenames remain review outcomes.

## Retained development regression

Expanding every control to the full height budget caused a measured regression:
the local12B model read only a neighboring last-name value and omitted a clearly
visible email. The independent case stopped at two verified controls out of six,
with one unverified email and three unreached controls. Both document-role
proposals were correct, three other answers stayed unknown, and no section
advanced. This failure is preserved in `target/semantic-docs-model-913vpir9/`;
it is not discarded or counted as a successful model run.

The correction scopes the expansion to file inputs proven by a fresh native
DOM description; there are no employer, selector or label aliases. Text, radio
and choice contexts retain the previous geometry. Post-entry file images keep bounded
heading space plus lower progress/validation context. A later run with wider
pre-entry file images verified the four contacts but refused the résumé before
selection: the model classified the visible upload options as buttons rather
than identifying a file chooser. That result is preserved in
`target/semantic-docs-model-a3dbrx4k/`. Pre-entry identity images now retain the
previous framing; post-entry feedback images use the wider budget. The additional
file-only run before that framing adjustment is retained in
`target/semantic-docs-model-ogx9332z/`, with four verified contacts, one unverified
native résumé selection and an unreached cover letter. This avoids forcing
additional neighboring inputs into already-qualified contact-field images.

## Qualification

Final Windows workspace validation passed **1,327 ordinary tests**, with
596 opt-in tests ignored. Warning-denied Clippy, canonical build, format checks,
90 agent-tool tests and source-copy UI contracts passed. After the phase split,
the complete workspace was rerun; there were no additional runtime edits.
The compiled application/source hashes still match the final model report.
No dependency graph changed.

The final known-source local12B run took 234.914 seconds, with four of six
answerable initial controls verified, one natively selected but visually
unverified résumé, and one unreached cover letter. Three answers remain unknown;
there are zero incorrect, unknown-answer, unexpected or ambiguous entries. Both
document-role proposals are correct. The field gate remains failed, workflow
not qualified, and no section advances. The wider saved feedback image now
contains the entire visible upload error, without treating it as a filename.
Raw model observations remain unchanged. This is development data only.

The owned model/browser tree drained, shared model state stayed unchanged, and
host samples retained at least 21,670,969,344 available bytes. Evidence:
`target/semantic-docs-model-t9u24lx0/`. This is a completed bounded run, not a
long-running leak test or a general model promotion.

The actual record view passes at 320, 480, 900 and 1,360 pixels with no horizontal
page overflow or browser errors; request notes and file-review controls remain
unobscured. Open inline images resize at 320, 480 and 1,360 pixels. The existing
image inspector also opens this real saved feedback at actual size, supports
mouse panning at narrow widths, and closes. Screenshots at 320 and 1,360 pixels
were inspected. The inspector report's close_restores_page flag names the Close
control/hidden-dialog check; body-overflow and focus restoration were not
independently asserted, and no broader restoration claim follows. Private UI
receipts: `target/upload-review-ui-3gxnuwg8/` and
`target/upload-review-ui-2ux4v7ft/`.

The compatible full-browser headless quick layout/zoom/drag suite passed 22
cases (`target/layout-check-run-grhv986_/`). An earlier packaged-headless-shell
attempt executed zero cases because Extensions.loadUnpacked is unavailable;
that failure remains under `target/layout-check-run-9u180stb/`. The packaged
shell passes the separate actual-record/image-resize checks. No visible browser
was used. This is not the full settings/accessibility/installation matrix.

The next development slice is generic reviewed-fact/action proposals (R04),
with source fidelity, held-out/full-workflow truth, installation and sustained
resource criteria still open. There is no release-readiness claim.


## GPU observation scope

Three whole-adapter Windows WDDM samples during the earlier file-only run
reported 14,076,952,576 used bytes out of 16,977,985,536 reported total bytes on
the Radeon RX9070XT. These are actual adapter observations, separate from the
runtime's allocation self-report. They are not per-process attribution, a
continuous peak, a cross-host fit guarantee or proof of spillover behavior.
Private native receipt: `target/widget-diagnosis-qzchvj0z/owned-model-gpu.private.json`.


The embedded fixture/controller retained three failed qualifications: one exact
request-list assumption omitted the newly visible unrelated image origin's
robots check; one public driver call used a raw selector instead of its native
scoped selector; one collector compared scale-two raster dimensions with CSS
limits. The fixture now explicitly denies the unrelated origin through robots,
keeps its pixel unfetched, uses the native scoped selector, and reports both CSS
limits and existing raster scale. No product permission or image-budget limit
was broadened. Original failures remain in
`target/capture-replay-qualification-ndveq5r_/`,
`target/capture-replay-qualification-6d9eqpn_/` and
`target/capture-replay-qualification-0ui8spar/`.


The final embedded qualification passed one selected native case in 26.319
seconds, including the controller. Its unchanged contact crop is 618 by 94
raster pixels; its file-feedback crop is 1,440 by 800 at the existing scale two.
Both saved images were visually inspected: the file choice and lower review
message are visible inside the child frame. No file was selected. Evidence:
`target/capture-replay-qualification-p2d32iri/`.


The final phase-specific model run retains 23 WDDM samples while its owned model
identity was live: 14,238,261,248 to 15,955,943,424 whole-adapter used bytes,
out of 16,977,985,536 reported total. A subsequent model-closed application/UI
sample reported 4,523,409,408 used bytes. Other GPU workloads were not controlled;
these observations cannot attribute every byte to this model or establish a
continuous physical peak, spillover behavior or every-host fit guarantee.


Final post-inspector host audit found zero matching development/browser
processes, resident shared models, services, scheduled tasks, startup entries or
audit errors, with 31,787,991,040 available RAM bytes. Individual owned model,
application and browser jobs also drained. Private source/weight caches remain
intentionally on disk; these observations are not a sustained leak qualification.
Audit: `target/development-host-2689aed9869547b3b5fc23f29a88e342.private.json`.
