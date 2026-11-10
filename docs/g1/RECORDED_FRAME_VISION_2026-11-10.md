# Local visual review of recorded frame controls

Checkpoint: 2026-11-10, Windows x64. This qualifies a small fictional corpus
with the actual local model. It does not qualify the web workflow, arbitrary
employer forms or real candidate entry.

## Captures and verification

Focused captures now locate the leased child control, scroll it into view and
check its hit point in both child and parent. The screenshot is rendered in the
parent and clipped to the bound child viewport. Context includes nearby labels
without bringing in neighboring frames. A clipped, covered, moved or stale
field refuses capture. Scope and geometry are rechecked after the image; the
parent and child caret styles are restored. Ordinary page captures retain the
existing path. Full child-document coverage remains unavailable.

The native fixture produces a four-control overview, four distinct focused
images, a follow-up field image and a parent-only image after removing the
child. Each image is bound to the native run by its base64 digest. The two text
crops require exact nonempty DOM readback before and after capture. The model
controller refuses partial evidence, changed images, unrelated paths, repeated
crops, mislabeled crops or missing text-presence evidence before staging the
model. The native run also retained the six control/section criteria and
adversarial click/scope refusals from the prior checkpoint.

## Actual 12B results

`gemma4:12b-it-q4_K_M`, manifest SHA-256
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
Ollama 0.40.16, 4,096 context, closed JSON transcription, separate owned runtime.
The denominator is five visible values plus one required abstention on the
parent-only image. Native DOM verification is independent evidence, not a
visual success. Both text values were visible in the inspected overview; the
model returned empty strings for them.

| Method | Criteria met | Calls | Observation |
| --- | ---: | ---: | --- |
| Blind overview | 4/6 | 3 | Checkbox, select, follow-up and parent-only abstention passed. Both text values were reported empty. |
| Blind overview plus plain focused review | 5/6 | 5 | The textarea recovered. The short text input was still reported empty. |
| Blind overview plus DOM-presence-assisted focused review | 6/6 | 5 | Both missed text values recovered. Raw overview remained 4/6. |

The plain and presence-assisted runs used byte-identical overview, two selected
closeups, follow-up and parent-only images. The assist supplied only the known
field label and the fact that the control contained entered text. It did not
supply the expected characters. All five retained native prompts were checked
for absence of the two expected text strings. Only a parsed, unique difference
or missing observation can use a distinct focused capture; at most two such
calls are allowed for this corpus. The absent child never receives a cue.

The successful run generated 276 output tokens; its owned runtime invocation
lasted 19.115 seconds, excluding build/copy/setup. Across 205 whole-host samples,
minimum available physical RAM was 20,529,160,192 bytes. The controller used its
existing 10 GiB owned-job limit and private 512 MiB prompt-cache/one-checkpoint
settings. The native runner reported ROCm placement of all 49 main layers and
all five auxiliary layers on the RX 9070 XT; its fit projection was 7,693 MiB.
The sampled `/api/ps` VRAM field differed from native allocation logs; neither
is a device-wide peak measurement. A rocBLASLt Tensile data warning was retained
and requests still completed. No host runtime files or shared settings changed.
All three evaluated runs removed their private model copies/profiles and
recorded no cleanup errors. This is not a long-running leak guarantee.

## Retained evidence

- Native recorded captures: `target/replay-qualification-ndw67vog/result.private.json`,
  SHA-256 `e31155562f5b72db87a6d3758981d44d4bca422d79815e16079c09c0e65fa9f4`.
  Native report SHA-256
  `ee9fbe4b4cc101a9b2f1991b9b07067edf860290f343b78ac78f89e67dacd95c`.
  Native invocation 128.501 seconds; minimum sampled host availability
  28,176,076,800 bytes. No model was loaded in the browser run.
- `target/owned-answer-qualification-4gbak7qc/vision-frames.private.json` retains
  the 4/6 blind failure; `target/owned-answer-qualification-4842hbju/vision-frames-system.private.json`
  retains the 5/6 plain-focus failure. The earlier `8w5qjnr0` attempt stopped at
  test compilation; its endpoint receipt reference was corrected before model
  staging. These are distinct results and remain unchanged.
- Final `target/owned-answer-qualification-ru43ital/result.private.json`, SHA-256
  `a5fc59d39ff5a1823c5ff6d93b0ba781d70fec7647750fb586c608ff94ac1f3d`.
  Its `vision-frames-confirmed.private.json` SHA-256 is
  `c903733b1081efae2a2eba658ac25a1162c42ae580093f3796f86b113859baf1`.
  Five native request/response receipts, timing and teardown remain private.

For reproduction, first run `tools/qualify_replay.py --frame-controls` with an
explicit installed headless engine directory. Then run
`tools/qualify_owned_answers.py --suite vision-frames-confirmed --replay-result`
with the retained `result.private.json` and explicit local Ollama executable,
model store and manifest digest. `vision-frames` and `vision-frames-system`
retain the blind and unassisted modes; they do not relax failed criteria.

## Integration checks and teardown

The final source passed 1,243 ordinary workspace tests (585 opt-in cases
ignored across 43 targets), workspace Clippy, formatting and 56 tool contracts.
The managed build and compiled test binary were verified by the owned model
controller; the selected model test actually executed separately. The native
frame run passed on the current crop implementation. Ordinary browser controls
retain their path, so the previous 137-case fixture suite was not repeated for
this scoped capture/qualification change. Check details are in
`target/frame-vision-final-checks.private.json` and its immutable logs.

The final host audit found no matching development processes, services, tasks,
startup entries or resident models, with 29,853,978,624 physical bytes available:
`target/development-host-ad34b9834c3c4add9df42798a9f4ad59.private.json`.
The prior policy-blocked temporary profile cleanup remains unresolved; this
audit only describes observed liveness and host headroom.

## Integration still open

The presence cue is qualification-only. It must be connected to actual scoped
DOM readback, image freshness, explicit frame selection and attempt records
before it can be used by the web workflow. A fresh verified control is required
at the time of use; a historical screenshot or remembered model output cannot
authorize entry. More platforms, labels, layouts and ambiguous/changed controls
must be tested. No employer form was interacted with and no application,
application email or release was created.
