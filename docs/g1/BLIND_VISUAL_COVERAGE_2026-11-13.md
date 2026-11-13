# Native detail views and blind visual coverage

This local checkpoint follows `70b76fe4adb27d732658abe4bdbab1ded2f283bd`.
It improves contained rehearsal coverage; it is not release approval, held-out
qualification or proof of universal form completion. All interactions were
headless, with fictional facts and saved copies. No live employer form changed
and no application was sent.

## Change and retained experiment

The local 12B model omitted a visible French email control and reported its
preceding focused field. Adding systematic full-image instructions alone did
not recover the target: both raw observations still named the preceding field.
That failed experiment remains immutable.

The original image and first-pass prompt now remain the fast path. Only a truly
absent target admits one supplementary blind observation; ambiguity, an explicit
difference, exhausted observation capacity or a failure does not. A tighter
native-bound detail image is available for main-page text/select controls. It
is stored independently, with its own digest. Embedded, radio, upload and section
images retain their existing behavior. Both model observations remain recorded;
contradictory overlapping labels veto entry. Native document/schema/value/request
checks still run before action. See the [method and research limits](../BLIND_VISUAL_COVERAGE.md).

## Measured local-model results

All rows are known development inputs. Their frozen control/value truth was not
changed to make the model pass. Both final runs used the private local
`gemma4:12b-it-q4_K_M`, context 4,096, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.

| Case | Verified / answerable | Unverified performed entries | Unknown answers entered |
| --- | --- | --- | --- |
| French baseline, earlier checkpoint | 2/7 | 0 | 0/2 |
| French same-image prompt-only recheck | 2/7 | 0 | 0/2 |
| French native-detail coverage | **4/7** | 1 | 0/2 |
| Earlier saved Greenhouse family regression | **4/6** | 1 | 0/3 |

The detail view recovered the French email target; email and city then passed
independent native, image and expected-answer checks. The first email observation
still omits it. Its second observation reads `Courriel du candidat` as a text
control, without receiving that label in its prompt. The original image digest
is unchanged from the failed case; the additional detail image is separate.

Country was then selected correctly: native readback and the screenshot show
Canada. Its post-entry visual transcription instead reports the preceding city.
It remains unverified, and execution stops. Phone is unreached and the answerable
residence radio remains withheld. Both unknown salary/preference questions stay
untouched. There are zero incorrect values, unexpected entries or ambiguity
counts; these zeros do not turn the three unfinished answerable controls into
successes. The field gate fails and workflow remains `not_qualified`.

The earlier source family retains four verified contacts, one unverified résumé
selection, one unreached cover letter and three unknowns. Its pre-entry targets
need no coverage recheck. The saved uploader still lacks server-dependent setup;
no displayed filename or server acceptance is proven. Neither case is held-out,
and no workflow or broad quality gate passes.

## Resources, rendering and validation

Both runs reused previously verified workspace model caches. The French run took
178.340 seconds, with native aggregate app-job peak commit 9,362,907,136 bytes
and minimum sampled available host RAM 26,415,128,576 bytes. The earlier-family
run took 188.146 seconds, peak commit 9,332,137,984 bytes and minimum available
host RAM 26,214,047,744 bytes. These are distinct memory scopes, not per-process
GPU allocations or continuous peaks. API/WDDM samples remain private; no GPU-fit
claim is inferred from either alone. Both private runtimes and app trees drained,
listeners had no owners afterward, and shared model state was unchanged.

- Ordinary workspace: **1,336 passed, 598 opt-in tests ignored**.
- Warning-denied Clippy, canonical Windows build and format check pass.
- Four coverage regressions verify the two-call bound, no target/answer hints,
  preserved first observations, fast paths, ambiguity, limits, conflicts,
  malformed responses and ownership/model failures.
- Native detail-bound regression and one headless fixture test pass. Hidden and
  off-screen controls refuse capture; the form stays unchanged. Fixture images
  measure 720×234 and 720×102 pixels. The detail image was visually inspected.
- Source-copy UI contracts pass. Actual compiled record views pass at 320, 480,
  900 and 1360 px, with no horizontal overflow or page errors. The saved detail
  opens through the hash-checked endpoint. Actual-size inspection, mouse panning
  and closing pass at narrow/wide widths; the 320 px panned label was inspected.

Intermediate build/native fixture failures are retained. A caret invalidated a
pixel-identity test assumption; that test now compares image geometry and native
state. A 3 GiB debug build job hit an allocation failure; a RAM-admitted 6 GiB
job passed. Neither failure was hidden or counted as model error. No product
resource guard was lowered and no runtime dependency was added.

The final app SHA-256 is
`79503bff5f5b35b8ede5ef165fdb1963d27b8c4f39f704aa549c40ed790beeaa`.
Core source/app digests match both final model reports. Full native reports,
research receipts and source bindings remain under ignored private paths:

- `target/visual-coverage-2x385_b7/` — source/index/Crawl4AI, commands and native images.
- `target/semantic-profile-model-r0drwmxf/` — retained prompt-only failure.
- `target/semantic-profile-model-39uzlyy_/` — final French truth/result.
- `target/semantic-docs-model-67d6ju36/` — frozen previous-family regression.
- `target/profile-review-ui-zcyicfmq/` — final actual record/detail-inspector checks.

These changes have no hosted validation. Earlier hosted results apply to their
own commits only. The overall product goal remains active.

## Next work

Extend the same evidence-preserving coverage mechanism to missing post-entry
value observations, starting with the retained country selection. Keep the
original transcription and exact/native telephone comparison rules; never put
the expected answer in an observation prompt or retry away a mismatch. Also
retain radio over-abstention, server-dependent uploads, R02 held-out/full-workflow
truth, R01 lifecycle/admission work and every other unfinished product gate.
