# Post-entry visual value coverage

This checkpoint follows `ce22050fc736dc6b74fa40a771014b4add568417`.
It reuses the [blind coverage resolver](../BLIND_VISUAL_COVERAGE.md) for missing
post-entry value observations. It is not release approval or broad form-quality
qualification. Interactions were headless with fictional facts and saved copies;
no live employer form changed and no application was sent.

## Implementation

The original value pass and primary image remain unchanged. Main-page text and
select captures also retain a separate post-entry detail view. Only a genuinely
absent target admits one supplementary blind value read. Its prompt excludes
the native label, expected answer and earlier observations. Matches need no
extra inference; differences, ambiguity, limits, failures and contradictory
overlapping values cannot be retried away.

The controller retains both reviews and uses the existing exact/native telephone
comparison rules. It never replaces observed text with the intended answer.
The native driver verifies document/schema/value/request state around capture
and after review. Unsaved required image evidence prevents verification. Both
phases reuse the same capture/resolution mechanisms; the record interface uses
one shared detail-image/check renderer. The new post-entry images have independent
paths and digests; original images and failed observations remain intact.

## Actual local-model results

Both runs use the private local `gemma4:12b-it-q4_K_M`, context 4,096, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.
Their frozen truth, profile, inputs and development partitions are unchanged.

| Case | Verified / answerable | Unknown entered | Incorrect values | Performed but unverified |
| --- | --- | --- | --- | --- |
| French previous checkpoint | 4/7 | 0/2 | 0 | 1 |
| French post-entry coverage | **6/7** | **0/2** | **0** | **0** |
| Earlier saved Greenhouse family | **4/6** | **0/3** | **0** | **1** |

The French case now verifies first/last name, email, city, selected country and
phone. The country first pass still reports only the preceding city. Its
supplementary observation reads `Pays de résidence: Canada` from the post-entry
detail image, without receiving those strings in its prompt. Native state,
raw transcription and independent expected-answer checks agree. Phone is now
reached and verified. The answerable residence radio remains withheld; both
unknown salary/contact-preference answers stay untouched. Its field gate still
fails and workflow remains `not_qualified`.

The earlier family retains four verified contacts, one unverified résumé
selection and an unreached cover letter. The server-dependent uploader setup
remains unavailable. No displayed filename, server acceptance or completed
workflow is established. Neither case is held-out, and these denominators cover
their declared initial controls rather than every possible site state.

The French run took 222.152 seconds, with app-job peak commit 9,363,197,952 bytes
and minimum sampled available host RAM 20,339,499,008 bytes. The earlier-family
run took 187.726 seconds, peak commit 9,339,326,464 bytes and minimum available
host RAM 21,414,567,936 bytes. Both reused verified model caches. Private runtime,
listener and app-tree teardown passed; shared model state stayed unchanged.
These are distinct memory scopes, not continuous GPU peaks or per-process
allocation claims. The production host reserve was not lowered.

## Validation and binding

- Ordinary workspace: **1,338 passed; 598 opt-in tests ignored**.
- Warning-denied Clippy, canonical Windows build and format check pass.
- Six focused coverage regressions pass, including exact-case refusal, preserved
  first/second values, ambiguity/limits/conflicts/failures, no answer hints, and
  telephone equivalence only under explicit native scope. Missing country
  prefixes remain refused.
- Packaged headless failed-entry and embedded-context qualifications each pass
  their one selected native scenario. Neither uses a model or live employer.
- Source-copy UI tests exercise the post-entry detail endpoint and review states.
  Actual compiled records pass at 320, 480, 900 and 1360 px, with no horizontal
  overflow or page errors. The selected country image opens through its
  hash-checked endpoint. Actual-size zoom, mouse pan and close pass at 320/1360 px;
  the narrow selected-country view was visually inspected.

The app SHA-256 is
`d16cb8bf94ae1c7326a92fb89c2756bcc70bb7f387509294a0e410902f226e0e`.
Core source/app digests match both model reports. Native evidence, hashes and
command logs remain under ignored private paths:

- `target/value-coverage-yuqxh46c/` — checkpoint, source/index receipts and checks.
- `target/semantic-profile-model-nvhxrmna/` — frozen French model result.
- `target/semantic-docs-model-xkz3s0cy/` — earlier-family regression.
- `target/capture-replay-qualification-ptvo5k6q/` — native failed-entry scenario.
- `target/capture-replay-qualification-qj6m7az3/` — native embedded-context scenario.
- `target/value-review-ui-vcakwiq2/` — actual record/detail-inspector checks.

At preparation time, no hosted result for this batch is claimed. Hosted results
must bind the exact published commit; no release is implied.

## Next qualification

The shared before/after visual coverage slice now has passing scoped evidence.
Broaden R02 qualification to the frozen unseen source family and additional
eligible inputs under the existing permission, containment and corpus protocol.
Freeze source/model/config and independent truth before inference; preserve every
excluded or unavailable case and never tune a case while calling it held-out.
Full-workflow truth remains a separate criterion. R04 radio over-abstention,
arbitrary facts/histories, server-dependent uploads and all other product gates
remain open. This checkpoint does not complete the overall goal.
