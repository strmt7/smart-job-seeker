# Visual inspection during recorded-form actions

The saved-copy rehearsal now requires local visual inspection before each
supported entry and forward action, and verifies the completed field before
continuing. The browser remains open during inference. A visual observation
cannot bypass native document, frame, field, attachment or request checks.
No real employer form was interacted with for this checkpoint.

## Method and measured scope

The existing owned-runtime controller ran the actual Windows renderer with
`gemma4:12b-it-q4_K_M`, manifest digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
at context 4096. Each run used a private model copy and runtime, a 10 GiB
process-tree memory limit and the existing host-headroom checks. The shared
Ollama service was left untouched. Native receipts bind source, executable,
fixture, model and output hashes. Retained files are private under `target/`.

| Fixture | Before-action matches | Completed-field matches | Native test time | Minimum sampled host RAM available |
| --- | ---: | ---: | ---: | ---: |
| Three sections: text, email, dropdown and two forward buttons | 5/5 | 3/3 | 146.93 s | 21,728,681,984 bytes |
| Text, email, dropdown and initially empty radio group | 4/4 | 4/4 | 145.08 s | 22,424,625,152 bytes |
| Text, dropdown, resume PDF and cover-letter PDF | 4/4 | 4/4 | 127.50 s | 22,955,229,184 bytes |

The section fixture also refused a field action and a forward action when
their visual callback returned an error; direct readback and form inspection
verified that those refused actions did not occur. The section run precedes
the subsequent radio-target helper refinement. The mixed fixture exercises
that refinement; it does not remeasure section navigation.

Section receipt: `target/owned-answer-qualification-9rq41zmp/result.private.json`,
SHA-256 `b12bcd65c96de652f3dc346272aa41f027029cad78bb3426ab0bfa6e13052625`.
Its native renderer result has SHA-256
`d7c2f9a97c4439ab0a74e04d3f658a72353c96aa25b553604d15bd5e122e288d`.

Mixed-control receipt: `target/owned-answer-qualification-o45mxc6z/result.private.json`,
SHA-256 `8c62b79829c1c444faff9dc18b9f37fb02d9a1651089905834808c119c80a886`.
Its native renderer result has SHA-256
`1df1f040c479be7a544bc74f71aab2ed5632d4d9432d539e234bf5bda0ef5fb3`.
PDF receipt: `target/owned-answer-qualification-rgwminij/result.private.json`,
SHA-256 `5bb2fb76432377245915e9cc4222de42750b9033914113db190f7682db374a74`.
Its native renderer result has SHA-256
`167a8349523ab772a0b2b5ea4575ec59774a0f9ecac2706f18b6dd0777a2804e`.
The PDF case retained the independent content-digest receipts for both
fictional attachments. All three controllers verified removal of their
private model copy and profile. Combined counts describe these three
fixtures only: 13/13 target observations and 11/11 completed-field
observations matched; they are not a population accuracy estimate.

The production preview component displayed the actual three-section result
in a headless loopback fixture at 1360, 480 and 320 pixels. Nine section/width
combinations loaded target images with their recorded hashes and had no
document-width overflow. Desktop and 320-pixel screenshots were inspected.
This is component rendering evidence, not a full-workbench or browser-zoom
qualification. `in-session-ui.private.json` beside the section receipt has
SHA-256 `03d725cc2ad0839945ee07ab75bdff950e68e76f430d87c85c1bdc18b5bc638a`.

The compiled preview API was separately exercised without a resident model.
It retained two page views, reported the required unavailable visual gate,
entered zero fields, advanced zero sections and left the job discovered.
Evidence: `target/recorded-sections-native-vhg6e4z8/gate-no-model.private.json`,
SHA-256 `88cbd9616846ec219ca4b804ca3b21d523278c4367730e5631871187850a8f7d`.
An earlier overlapping invocation was refused by the existing global
rehearsal lock; the successful repeat ran serially. An initial refusal-test
assertion incorrectly applied HTML required-field validation to an empty
field; it was corrected to use direct value inspection. Failed native logs
remain retained and are not counted as successful qualification.

## Limits

These are individual fictional saved-form runs, not estimates of accuracy,
repeatability or arbitrary employer-site coverage. Visual models can err;
unreadable, different or unavailable observations stop progression. The
recorded-page isolation, eight-field limit, two-transition limit, unsupported
controls and later-resource gaps remain. Candidate-data isolation and diverse
held-out real-source archives still need qualification before live filling.
Memory figures are sampled host availability, not application or GPU peaks
and not proof of long-running leak freedom. No release was created.

## Final ordinary checks and teardown

The final radio-refined workspace passed 1,248 tests with 587 explicitly
opt-in cases ignored across 43 targets. Clippy, formatting, diff whitespace
and the focused source-copy UI tests passed. Native reports:
`target/in-session-radio-final-checks.private.json` and
`target/in-session-js-verified.private.log`.

The final audit found zero matching development processes, scheduled tasks,
services, startup entries and resident models, with 31,821,078,528 available
physical bytes. The snapshot is
`target/development-host-6e25d960e4e743bfb6045df713d0878a.private.json`.
This confirms the observed teardown checkpoint, not long-running leak freedom.
