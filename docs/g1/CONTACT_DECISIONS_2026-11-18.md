# Contact mapping decisions · 17–18 November 2026

The contact mapper now separates a request for further interpretation from a
protected answer, third-party detail, applicant decision or credential. A
follow-up receives only explicitly uncertain fields. It must return one binding
per offered field before any new proposal reaches the existing fact validator.
Known mappings and explicit review decisions retain their original status. A
failed or incomplete assessment leaves the uncertain field for manual review.

Verified section headings are supplied separately from the question where the
entire native prefix matches. The browser keeps the original scoped label for
identity, presentation and visual checks. Mapping receipts now include the
request and prompt digests, requested decoding settings, response and completion
metadata. Contact requests offer fact keys. The recorder keeps prompt hashes
and private reasoning lengths.

## Measured behavior

The first version separated uncertainty from the generic review decision.
The frozen single-field case still returned the generic review decision, kept
the field untouched and closed in 33.068 seconds. The native grammar then
required a specific review reason. With the same fixture and model profile,
the mapper selected the correct contact key on the first request. Native
readback and local vision verified the field and the expected repeat stop.
The isolated test passed in 75.895 seconds.

The initial eight-field regression mapped all four fields in the first batch.
It stopped after two native entries because visual verification disagreed on
the last-name field. The first image transcription had extra page-title
context. The second transcription reported a blank value despite a verified
native readback. The test retained both observations and stopped at
211.363 seconds.

An unchanged repeat verified all eight fields by native readback and local
vision, advanced through four sections and stopped at the expected repeated
section in 622.698 seconds. The isolated and in-sequence given-name requests
had identical body and prompt digests and both selected the correct key.
The initial and repeat last-name images were byte-identical and the native
value matched across runs. Both image transcriptions agreed with the value on
the repeat. The whole-fixture result is one pass in two attempts, with the
initial visual disagreement retained. The model selected contact keys
directly in the completed run. The deferred-field branch was exercised by the
focused regression tests.

The Windows workspace passed 1,470 tests with 620 opt-in tests ignored.
Warnings-denied Clippy, the Windows development build and formatting passed.
The focused regressions cover preserving valid mappings, keeping explicit
review decisions binding, leaving failed assessments unresolved and refusing
a follow-up response that names a field outside its offered scope.

The repeat sampled whole-host RAM 1,223 times, retained the latest 900 samples
and observed a minimum of 11.699 GiB available. The private model exited and
its listener had zero owners. The final host audit reported zero owned
survivors.

The private receipts are under `target/field-decision-ced922a6/`. All
qualification interactions occurred in fictional, isolated replays.
