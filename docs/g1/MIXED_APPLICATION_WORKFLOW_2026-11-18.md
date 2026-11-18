# Mixed application workflow · 18 November 2026

Two fictional saved applications were frozen before inference. Each had seven
answerable fields, two distinct PDFs and an optional field without a supported
answer. The development case combined conditional location, a repeated email
question and a lower-page review row. The second case was reserved to check a
different layout after the repair. The browser ran the production replay actor
with private Ollama 0.40.16 and Qwen 3.8 27B at an effective 16,384-token context.
The answer key and attachment digests were held outside the inference inputs.

## Matched development trials

| Trial | Verified entries | Verified review rows | Retained PDFs at review | Seconds |
| --- | ---: | ---: | ---: | ---: |
| Initial supervisor | 3 of 7 | Review unreached | Review unreached | 277.590 |
| 16 GiB aggregate test budget with native logs | 6 of 7 | Review unreached | Review unreached | 382.978 |
| Structured file target | 7 of 7 | 6 of 7 | 1 of 2 | 485.368 |
| Separate review heading and answer row | 7 of 7 | 7 of 7 | 2 of 2 | 867.228 |

The initial city entry matched natively. Ollama returned `done=false` and an
empty response during visual verification. The later run completed that read.
Its native aggregate job peak was 14,399,815,680 bytes, about 10 MiB above
the initial job ceiling. The initial run did not capture the native process log
or peak, so the cause of its incomplete response is unresolved. The later
test used a 6 GiB whole-host reserve and sampled at least 10.25 GiB available.

The cover-letter target showed a section heading and a separately labeled
chooser. The first visual reader treated the heading as a text field and
correctly stopped entry on disagreement. The revised reader reports ordered
headings, the visible leaf label and the control type separately. The paired
trial verified both files, reached the final review and correctly withheld a
pass because the model conflated the section heading with the field label in
the lower-page review crop. The browser had revalidated all seven native rows.

The development repair separated a section heading from its answer row when
the native heading and label shared prefix words. The local model read the
separate images without the expected text. The exact scoped comparison,
native revalidation and retained file-byte checks still applied.

The final development run passed all seven review assertions. Both original
PDF digests matched the terminal selections, the unsupported optional field
remained untouched, and the browser revalidated the current native summary.
Aggregate job peak was 14,484,635,648 bytes. Minimum sampled available host
RAM was 11,318,894,592 bytes. The owned model process exited and the replay
record reported zero blocked or cached write responses.

Observed runtime varied substantially. Two file-target detail requests had
the same images, prompts and options as the prior run and kept native `xhigh`
effort. Their combined generation count rose from 372 to 4,452 tokens and
their combined native duration rose from about 35 to 399 seconds. The new
separate summary detail reads took about four seconds combined. These timings
locate the observed increase and leave its underlying runtime cause open.

## Initially held-out layout

The first run verified all seven entries and reached the native final summary
in 578.062 seconds. Six review rows and one retained PDF passed. The model
correctly transcribed two document rows from a crop intended for the cover
letter. The heading and cover-letter regions enclosed the résumé row, so the
single-row detail verifier withheld a final pass. The original model result,
image and stop remain the held-out evidence.

Grouped detail capture now reads each native heading separately from the
selected answer row across layouts. The overview remains the first independent
read. Ungrouped rows still use one detail image. The targeted geometry test
checks both forms of capture and preserves the exact answer comparison.

The standard private launcher reran the same frozen application in 641.304
seconds. Both lower-page detail reads verified the scoped answers from separate
heading and row images. All seven entries and all seven review rows matched,
both original PDFs were retained byte-for-byte, and the optional field stayed
untouched. The run revalidated the native summary and reported no external
delivery or cached write responses. The job peaked at 14,500,388,864 bytes
with a sampled minimum of 11,145,687,040 bytes available on the host. This
is a paired repair of the original held-out failure. A fresh unseen layout
is required to measure generalization after the repair.

## Limits and rendered records

Qualification planning schedules all eligible questions in required-first
batches. The workflow deadline applies to each native model request. The
planner retains each batch and explicitly records work it could not offer.
A bounded web search can queue its planned page results and completion while
collection is delayed. The loopback test confirmed requests for all four pages
before the first client poll.

The private launcher preserves the fixture's independent journey checks and
validates the manifest through the selected local model store. The first
launcher attempt failed its former shared-inventory preflight because that
runtime did not list the separately installed model. The same local manifest
resolved with the exact selected digest and size. The launcher now admits its
test job against live host memory, supports explicit overrides, and includes
setup and cleanup in the outer deadline. The 35 native vision receipts in the
final development run used 357,981 bytes. A byte budget replaces the earlier
32-response cutoff.

The workspace passed 1,477 tests with 621 ignored before the final crop
change. Three application summary tests, the browser geometry regression and
warnings-denied Clippy passed afterward. The private launcher, owned process
lifecycle and qualification controller tests passed. The relevant UI script
and 70 compiled-interface checks passed, including the combined qualification
count at widths from 320 to 1,920 pixels and with 200% browser zoom. The
390-pixel and 200% views were inspected directly.

Private evidence is under `target/mixed-workflow-34e09602/`,
`target/private-rehearsal-qualification-b_jrgsiz/` and
`target/layout-check-run-q0ittcu6/`. The test scope covers saved applications,
a loopback search provider and an isolated interface. Live server validation,
representative employer workflows and release qualification remain open.
