# Recorded page previews

In a saved job’s **Details and records**, open **Application records** and select
**Create recorded preview** beside a saved page copy. The background task uses
the same progress and cancellation controls as page copying. The resulting
record offers a view selector, observed field labels, review messages and a
saved image for each available view. Selecting an image opens the existing
zoomable viewer.

The app checks that the source copy completed and belongs to the current job URL.
The worker checks the manifest against the saved inventory and starts the
checksum-bound replay browser. It saves the page and up to four directly captured
embedded frames. The frame caption uses the frame element’s label or title when
available. The worker checks the document and form schema after capture, keeps
missing resources and frame failures visible, and saves each image with a digest
in a new attempt record. The viewer checks that digest when loading an image.

This route reviews public archived content. It accepts a job and saved-record
identity, and uses only recorded resources. It preserves the job’s application
state. The [collector](SOURCE_COPY_WORKFLOW.md) can also preserve eligible
embedded origins. Frames outside the saved archive can appear unavailable.

## Offline autofill rehearsal

Choose **Rehearse autofill** beside a saved page copy. The task uses a built-in
fictional profile and the same exact-label and standard autocomplete mapping
used by the ordinary form review. It checks up to four eligible controls in
the saved page and available frames. These include text fields, native
dropdowns, radio groups and PDF inputs identified uniquely as a résumé or
cover letter. Choices need one matching displayed option. Sensitive,
ambiguous and unsupported controls remain unfilled. The job snapshot is
saved with the rehearsal, while the prepared candidate draft stays in its
existing location.

For each field, the driver checks the current document, frame lease and form
schema, respects existing values and browser constraints, and uses the browser
driver's typed commands. Exact visible DOM readback is required before and
after the focused image. Radio captures center the checked option and include
surrounding labels where the viewport permits. After the last input in that
view, it checks the completed values, document identity and observed request
activity again.
A failed action stops further entry and the result retains earlier observations.

Rehearsal stages two named copies of a fictional one-page PDF in the new
record's `rehearsal-attachments` directory. The saved source archive stays
immutable. The browser receives a verified private copy and exercises only
an empty file input whose declared formats accept the PDF. An isolated
browser script world checks the selected file's name, size and actual bytes
against its admitted digest. The record shows an attachment receipt only
after the final in-view recheck.

The browser closes before optional visual review. When an eligible local
vision model is already loaded, it receives the focused image. Text controls
also get a field label and independently verified text-presence cue. Choices
are transcribed from the selected state in the image. File input review reads
the displayed filename. Expected values stay in the comparison code. The
model's identity is checked before and after inference. Application records
distinguish a visual match, different or unreadable transcription,
unavailable vision, a failed image review and failed DOM verification. A crop
error is recorded for that field. Review continues while the same local model
remains eligible. The result retains its name and digest, and each saved image
has a digest and can be opened with the shared viewer.

This exercise covers the saved responses and supported controls. It makes no
live application request and creates no approval. Real candidate data, other
attachment types, custom choice widgets, later steps and server validation
remain outside this rehearsal.

### Rehearsal qualification

The compiled headless Windows UI opened a fictional saved frame, filled first
name and email, and retained two independently verified field images with
matching history and screenshot digests. An unsupported required question
remained in the outline. A separate fixture cleared the first answer and made
an uncaptured POST during the second input. Replay blocked that request, the
first answer was flagged as changed, and entry stopped. Both jobs remained
discovered.

The field image fit the 318-pixel panel at a 1,360-pixel viewport and the
381-pixel panel at a 480-pixel viewport. Both screenshots were inspected.
The complete two-case invocation took 96.811 seconds and sampled at least
24,548,900,864 bytes of available host RAM. Private evidence is in
`target/recorded-fill-native-iwpb2uzk/`, with
`qualification.private.json` SHA-256
`c3f4654163e364abc0bc652cef00abb0b24087a96a31731fcabf1b02593ab3e8`.
That run exercised the unavailable-vision state. The connected model check
below exercises the same saved-copy renderer with the resident vision path.

The mixed-control Windows fixture verified four fields in a 440-pixel
embedded frame. It checked native dropdown and radio selections alongside two
text fields, with focused images linked from the record. An earlier attempt
exposed a 400-pixel overview limit that skipped this visible frame. Larger
bounded element captures now admit the frame while keeping the viewport and
image byte limits. The final run rendered the selected radio image at 1,360
and 480 pixels. One intermediate invocation failed the resize assertion.
The final capture and render passed both viewport checks, and a separate
history render also fit at 320 pixels. A second fixture cleared the dropdown and attempted a POST
during a radio change. Replay blocked the request and the rehearsal stopped.
Both jobs remained discovered. The two-case run took 137.525 seconds and
sampled at least 27,708,227,584 available physical bytes.
`target/recorded-choice-native-9rph2hvv/qualification.private.json` has
SHA-256 `c35f5b75ac318aff87a1dd7833440c1e3ef0ea90809e8028df438a59ec2f4453`.

The attachment fixture retained the two sample PDFs, verified their digests
and opened the focused file image at 1,360 and 480 pixels. The source archive
retained its original inventory. A second fixture replaced the résumé after
attachment with different bytes under the same name and size. Readback
refused it and stopped, preserving the earlier verified dropdown.
Both jobs remained discovered. The two-case invocation took 115.069 seconds
and sampled at least 28,560,060,416 available physical bytes. The evidence
file `target/recorded-upload-native-63fbw2mg/qualification.private.json`
has SHA-256 `879277ac2bccc251d65564c9f1015cf8424a011f6de88ef0e31187b9156198d0`.
The final compiled history view displayed both filenames and fit the field
image at 1,360, 480 and 320 pixels.

### Connected local vision

The native Windows qualification ran the saved-copy renderer against an owned
Ollama 0.40.16 instance on a private loopback port. It used
`gemma4:12b-it-q4_K_M` at 4,096 context. The focused **First name** and
**Email** captures both matched the verified values. The image digests matched
the earlier compiled UI captures. The renderer closed the browser before the
two visual requests. The retained record binds the matches to the model digest.

The contained run took 60.655 seconds and sampled at least 22,739,935,232
bytes of available host RAM. Native output reported all 49 main layers and all
five auxiliary layers on the GPU, with a 7,693 MiB fit projection and the
retained rocBLASLt data warning. The projection is distinct from a device-wide
peak. The private runtime and model copy were removed. Evidence is in
`target/owned-answer-qualification-xxx3rri0/`, with
`result.private.json` SHA-256
`f1fe5b1d85bb6d3aa6a8d3b8974c5625f209224f954de364e60e04eb46bfdcd6`.

The opt-in `vision-saved-rehearsal` suite in
`tools/qualify_owned_answers.py` accepts a private JSON fixture under
`target/` through `--recorded-rehearsal-fixture`. Supply `source` with
the saved attempt directory, job and attempt IDs, and `rehearse: true`.
Supply `python`, a `RehearsalConfig` under `config` with the verified
headless engine inventory, and `manifest_sha256` for the saved replay.
Use a fictional copy of `https://example.org/fictional-rehearsal` with the
two eligible text fields in its recorded frame. The `mixed_controls` fixture
flag additionally expects a **Country** dropdown and **City** radio group.
The separate `upload_controls` flag expects **First name**, **Country**,
**Resume** and **Cover letter**.
The suite verifies the supplied source, config and current model.

The final mixed run used the same model and context. It matched all four
values, including both native selections. All four image digests matched the
compiled UI run. The invocation took 99.505 seconds and sampled at least
20,365,971,456 bytes of available host RAM. The owned process tree, private
profile and model copy were removed.
`target/owned-answer-qualification-2fjmzgrb/result.private.json` has SHA-256
`2208202dd4976780d26a9f5ab7d5a003d70e8a3ecd5a6979fd08f62d7227b6b3`.
This fixture leaves broader form coverage and model accuracy unmeasured.

The owned attachment run matched all four fields, including both displayed
PDF filenames. It reproduced all four compiled fixture image digests.
The invocation took 80.164 seconds and sampled at least 24,483,835,904
bytes of available host RAM. The model copy and private profile were
removed. `target/owned-answer-qualification-jseusjvn/result.private.json`
has SHA-256 `ba846d95dc2c6807929f0b8f3b3b38e30948b6cacfe013258c5900bde5fb70a9`.
This visual result corroborates the filenames. The isolated DOM checks
establish the attachment bytes within the recorded form.

## Compiled Windows evidence, 2026-11-11

An owned app accepted a fictional saved archive with a parent page and a separate
frame, ran the normal preview endpoint and saved two images. The frame’s required
field appeared only in that frame’s outline. The history result and screenshot
API returned the recorded metadata and matching PNG digests. The private
`target/recorded-preview-qualification-uzbdlwz8/result.private.json` records the
API journey and drained process tree. Minimum sampled host availability was
28,728,381,440 bytes.

The compiled headless UI then opened the record, ran **Create recorded preview**,
selected **Qualifications** and loaded the frame image. The 1,360-pixel and
480-pixel viewport checks measured panel widths of 318 and 381 pixels, with the
image fitting each. The inspected screenshots and executable fingerprint are in
`target/recorded-preview-qualification-uzbdlwz8/`. Its `rendered.private.json`
binds the final result. The owned invocation took 40.418 seconds. The replay
reported four recorded responses and zero blocked requests for this fixture.

The initial rendered fixture omitted a required history timestamp. The next run
exposed a refresh race that discarded the open preview during completion. The
fixture now includes the timestamp, and the client waits for completion refresh
before showing the preview and preserves the rendered result for the same
attempt. A focused UI regression also checks that a response for a previous
selection cannot replace the current image and that mismatched digests refuse
rendering.

The Rust workspace passed 1,244 tests with 585 opt-in cases ignored across 43
targets. Clippy, formatting, source-copy, layout and image-viewer checks passed.
An existing worker contract test caught an extra null field on ordinary copy
requests. The optional field is now omitted when absent, preserving that wire
format, and both request variants reject candidate or credential fields. The
final UI refinement passed the focused script, managed build and compiled
browser checks. Logs are indexed by
`target/recorded-preview-verified-checks.private.json`.

The final host audit found zero matching development processes, services,
scheduled tasks, startup entries and resident models, with 33,285,652,480
physical bytes available. Automatic review blocked deletion of the copied test
runtimes and returned only “blocked by policy”. They remain in the private
qualification folder.
