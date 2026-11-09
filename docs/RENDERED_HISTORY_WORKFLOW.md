# Rendered history-to-review workflow

Measured on Windows on 2026-11-09 with the installed local
`gemma4:12b-it-q4_K_M` model and Chromium 157.0.8103.39. These are two
fictional-form observations, not a general accuracy or release-readiness result.

## Connected path

The headless qualifier serves the production interface and API handlers against
a temporary workspace. The initial job and inspected session are seeded from
owned file fixtures; the contact mapper actually runs locally. Discovery and the
initial Inspect form task lifecycle are outside this qualification.

The rendered interface requests blank-field history advice, displays eight
unchecked source-linked proposals, selects them, applies them to the editors and
deliberately invokes reviewed filling. The real driver enters two contact values
and eight checked history components. Automatic whole-page visual review and
eligible close-ups use the actual local model and production archive/status
endpoints. API responses are not intercepted or substituted. Expected fixture
values are independent literals rather than production-parser output.

| Case | Filled and visually/live matched at inspection | Restricted blanks | Focused checks | Duration | Minimum sampled free host RAM |
|---|---:|---:|---:|---:|---:|
| Repeated sections, baseline profile | 10/10 | 4 | 3 | 61.198 s | 9.563 GiB |
| Flat layout, different profile | 10/10 | 4 | 0 | 45.019 s | 9.910 GiB |

Both cases made exactly one fill request, retained the eight history source
links, and reported no client errors, external UI requests or refused server
operations. No application or email was submitted. The ten matches belong to
their individual captures; they do not establish simultaneous whole-form
correctness. Four unanswered restricted questions remain manual.

## Problems found and corrected

A real run paused before history generation with 10,125 MiB available against
the old continuation requirement of 10,240 MiB. No stale owned browser or resident
test model was found after cleanup; unrelated workloads were left running.
Startup already reserved nine GiB beyond a full model-sized host allocation.
Continuation now uses one quarter of physical RAM, bounded to four through eight
GiB, so startup admission cannot require less reserve than continuation. Invalid
samples and pressure still refuse inference; model/GPU/startup limits are
unchanged. This is admission control, not a RAM reservation or a guarantee against
other workloads exhausting memory.

History advice now exposes bounded, text-only worker diagnostics, including an
empty-result resource pause. Successful targeted captures no longer display the
full-page coverage warning: their scope remains explicitly one field. Incomplete
whole-page reviews still warn and cannot start the automatic follow-up queue.

## Reproduction and evidence

Use explicit existing absolute executables:

```powershell
py -3.13 -X utf8 -B tools/qualify_history_ui.py --node <node.exe> --browser <chrome.exe> --profile baseline
py -3.13 -X utf8 -B tools/qualify_history_ui.py --node <node.exe> --browser <chrome.exe> --profile varied --layout flat
```

The wrapper places the build/test/Node/browser tree in a four-GiB Windows job
before resuming it, with a 420-second deadline. Ollama is outside that job and
uses the existing resource gates and single-model ownership checks. Only a model
loaded by the fixture is unloaded. Host RAM is sampled every 500 ms: 121 samples
in the repeated case and 89 in the flat case, with no sampler errors. These are
whole-host observations, not exclusive app/GPU peaks or reserved memory.

Private evidence remains under ignored `target/`, including request/response
receipts, final DOM readback, screenshots at 390 and 1440 pixels, compiled-source
digests and browser identity:

- `history-ui-run-bmodih93/result.private.json`, SHA-256
  `e49ba895c51a5afbbce2bf80d5f733477b5667725f9d49420117582d04fc4081`;
  native/client evidence in `history-rendered-19qEtZ`.
- `history-ui-run-xc9e4arp/result.private.json`, SHA-256
  `00b03558d810fc7a045b0fb83789cbefb247f12b0772b853bfd364188d8a3d3d`;
  native/client evidence in `history-rendered-4CzJoB`.

Earlier harness failures and the resource-pause run are retained rather than
counted as successful qualifications. The managed encrypted web suite passed
746 tests with 400 opt-in tests ignored; Clippy and the managed build passed.
The standalone JavaScript suite passed 162 scripts. Broad live-site no-write
qualification, unseen-form model accuracy, clean-Windows installation and
document fidelity remain open. Narrow-screen capture magnification is also a
remaining usability improvement.
