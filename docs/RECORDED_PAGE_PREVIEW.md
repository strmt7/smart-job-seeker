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
used by the ordinary form review. It checks up to four eligible text controls
in the saved page and available frames. Sensitive, ambiguous and unsupported
controls remain unfilled. The job snapshot is saved with the rehearsal, while
the prepared candidate draft stays in its existing location.

For each field, the driver checks the current document, frame lease and form
schema, respects existing values and browser constraints, and uses native input
events. Exact visible DOM readback is required before and after the focused
image. After the last input in that view, it checks the completed values again.
A failed action stops further entry and the result retains earlier observations.

The browser closes before optional visual review. When an eligible local
vision model is already loaded, it receives the focused image, field label and
verified text-presence cue. The expected characters stay in the comparison
code. The model's identity is checked before and after inference. Application
records distinguish a visual match, different or unreadable transcription,
unavailable vision and failed DOM verification. Each image has a digest and
can be opened with the shared viewer.

This exercise covers the saved responses and supported controls. It makes no
live application request and creates no approval. Real candidate data, choices,
attachments, later steps and server validation remain outside this rehearsal.

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
The run exercised the unavailable-vision state. The
[prior local model measurements](g1/RECORDED_FRAME_VISION_2026-11-10.md)
used different captures and do not qualify model accuracy on this route.

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
