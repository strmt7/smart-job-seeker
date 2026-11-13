# Visible viewport capture and session reuse

Date: 13 November 2026. This checkpoint follows `3aaeff9dcb79fb998c8e005d43099c07074cf35f` and continues R03. It does not authorize employer interactions or submission.

## Measured cause

A no-action saved-copy probe compared cold inspection, CSS-inspector preparation, a three-second review delay, three- and fifteen-second idle periods, and individual inspection operations. All six cases closed their isolated browsers, retained identical native document/form/dialog/value identities and made no clicks or entries. The native non-read request counter stayed 1 to 1; that existing request remained blocked. An extra missing image appeared specifically during the context screenshot, including after the fifteen-second idle. Node resolution, box-model reading, caret style application/restoration and a full-viewport screenshot did not produce it in the operation sweep.

A separate fictional responsive-background fixture reproduced the difference with fresh, zero-capability AppContainer sessions. Each of two scales (1 and 2) requested an unrecorded inactive narrow-screen background with `captureBeyondViewport:true`; both corresponding `false` cases had zero refusals. The earlier color/text clipping probe retained width 1280, device scale 1 and wide-screen colors afterward in all four cases. Unchanged final layout metrics therefore did not establish absence of screenshot-induced resource requests. These are measured browser behaviors, not a claim about Chromium internals or every browser version.

## Change and adversarial checks

Field context/detail screenshots now use `captureBeyondViewport:false`. Their existing native bounds already require the complete control to lie inside the visible viewport, with context constrained to the same viewport. Existing crop sizes, scale, caret restoration, stale evidence and replay/write barriers remain. The separate embedded-document capture contract is unchanged.

The permanent responsive-resource regression first exercises the production context and control capture paths, requiring zero missing resources. It then verifies the four raw capture cases above, including the exact refused fictional image identity. Context (220 by 81) and detail (219 by 42) images were visually inspected: the complete control text and surrounding background are present.

On the same saved employer archive, warmed and warmed-plus-three-second-review cases now reach the deliberately refusing visual callback without additional replay gaps (4 to 4). All values and native identities remain unchanged; no clicks or fields are entered. The cold case still stops before review on genuinely new inspector-related gaps (2 to 4). This is passive eligibility evidence, not a completed dialog action or model/workflow score.

The fixture also exposed a close-acknowledgement race: a subsequent session could receive `Another isolated rehearsal is active` after a successful Close. The owner now acknowledges success after its process-group scope, controller cleanup and admission release. Three immediate sequential sessions confirm released admission at return and retain refusal of a concurrent session. The existing consumer-failure recovery regression also passes. No arbitrary wait or concurrent-session allowance was added.

Replay refusal samples now include bounded first/last observation milliseconds from the worker's monotonic start; the report includes elapsed milliseconds. The 32-sample cap, URL hashing, request counters and decision semantics remain. A deterministic portable regression verifies repeated observations and backwards-clock refusal. Times from separate native/controller clocks are not treated as a shared absolute clock.

## Local-model continuation and validation

One changed-source run used the existing local `gemma4:12b-it-q4_K_M` model (digest `6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`) through the compiled application route. It independently proposed optional decline and its blind visual transcription matched the offered control. The resulting saved-copy screenshot shows the dialog gone. A newly blocked XHR POST appeared at worker-relative 52,140 ms, so the native closure witness was refused with `replay_gap`: zero fields entered, zero section advances and no workflow/held-out score. The model was not given the frozen passive-probe proposal as an answer key. No employer network operation was performed by the isolated browser.

The private runtime reported `session_closed:true` and native process exit (signaled, code 1); the task-owned tree drained and the shared runtime was unchanged. Across 258 whole-host RAM samples, minimum available memory was 21,250,514,944 bytes with no observation errors. The app job reached 9,104,822,272 committed bytes under its 10 GiB cap; that is aggregate commit, not exclusive physical RAM. Nine whole-adapter WDDM samples ranged from 15,656,497,152 to 15,956,586,496 used bytes of 16,977,985,536 total. The owned runtime API separately reported only 948,101,446 to 1,145,716,079 VRAM bytes. These differently scoped observations do not establish a universal 16 GiB deployment guarantee or an independently measured model peak.

Windows workspace tests passed: 1,357 passed, 609 explicitly ignored across 43 result groups. Final browser-unit checks passed (112 passed, 23 ignored, a workspace subset), portable replay 73 passed, Windows build, warnings-denied Clippy and formatting passed. Explicit native checks passed for responsive resources, immediate reuse, consumer-failure recovery, the seven-case dialog regression, visible-viewport rejection/scrolling and caret restoration/stability. The later passive-probe completion-count assertion is test-only; it received final browser compilation/unit checks after the full workspace and model run, without changing the measured production binary. GUI/window-resizing, unrelated packages and unchanged model cases were not redundantly requalified by this checkpoint.

Model evidence is `target/viewport-dialog-model-28blxk6o/`; its source/manifest/controller/binary identities and private native results remain intact. The scoped post-model host audit found zero matching processes, resident models, services, scheduled tasks, startup entries or errors, with 30,344,499,200 available RAM bytes (`target/development-host-74c48244e3ef4355b541a8684136e4f8.private.json`). This is teardown evidence, not a sustained leak test. Published baseline `3aaeff9` has live-verified successful CI, coverage and CodeQL runs; those runs do not attest to these newer changes.

## Private evidence

- Initial no-action native matrix: `target/passive-dialog-native-irrhl935/`.
- Timed six-case operation matrix: `target/passive-dialog-native-ta_j033r/`.
- Responsive raw reproduction: `target/responsive-resources-q5wg_n4m/`.
- Production capture regression/images: `target/responsive-fixed-ibdjff7_/`.
- Changed saved-copy passive checks: `target/passive-dialog-fixed-d049wode/`.
- Commands, logs and source-bound index/search receipts: `target/dialog-replay-deltas-0gh9_2nw/`.

The first passive probe had an insufficient 30-second owner lifetime and retained an unconfirmed Close. The first responsive resource probe exposed the admission race and retained its first image before the next launch failed. These failures remain private and intact. A subsequent scoped host audit found no matching survivors; neither failure was diagnosed as host RAM exhaustion. Source bytes and model/host results are recorded separately below when available.

## Remaining scope

Unsupported stylesheet Vary dimensions, missing server-dependent requests, requiredness, custom controls, independent answer/workflow truth, representative held-out evaluation and broader installation/model qualification remain open. No request counter is ignored or reset, no headers or responses are invented, and no live fallback, permission relaxation or application submission is introduced. R03 and the overall goal remain active.
