# Focused visual follow-up

Implemented and qualified on fictional forms on 2026-11-09. The local interface
can now follow a page review with up to three targeted visual checks. This extends
the earlier single-mismatch retry; it does not edit answers, approve filling,
advance a form or submit an application.

## How the workflow behaves

The initial review must have complete supported comparison/capture coverage,
current captures, saved evidence, unique field associations and no unconfirmed
live values. Only isolated transcription differences qualify. Ambiguous,
unobserved or incomplete fields keep the existing manual route. The planner uses
inspected fields and their available filled-field selectors, never employer names,
remembered selectors or platform-specific layouts.

For each planned field, the existing focus endpoint rechecks the schema, walls,
all completed values and attachments before obtaining a close-up. The local model
transcribes the image without receiving the expected answer. Its observation is
compared exactly and checked against the live typed value. A fresh disagreement
remains a review result; no answer is rewritten to agree with the model. Capture,
archive, page or value-check failure stops the remaining queue.

There are at most three attempts and a cooperative two-minute follow-up budget.
An in-flight vision request has its existing two-minute transport timeout, so
stopping or reaching the budget does not interrupt that request. **Stop remaining
close-ups** stops subsequent fields after the current check. Every visual task
also has a 240-poll client limit, preserving its recovery token if still running.
The work is sequential and retains the existing local-model resource admission.
No extra model is loaded by this coordinator.

The panel distinguishes attempted, observed, matched, unchecked and unresolved
fields. The initial results remain in an expandable section; close-ups have
separate labelled cards with optional raw model transcriptions. Scrolling makes
earlier captures historical. Matched close-ups never convert the whole form into
a current success or clear existing review flags. Evidence files and screenshots
remain in the application record under their original task identities.

## Stable captures and typed displays

An actual endpoint run exposed a false freshness warning: only a 17-pixel blinking
caret differed between images. The browser now uses its own inspector stylesheet
to suppress caret rendering during capture and then restores the empty rule. It
keeps a separate owned sheet per main-frame document, rebinds after navigation,
and attempts restoration after capture/application errors. Restoration failure is
reported; it is not silently accepted. No host caret or accessibility setting is
changed. Captures still use exact image hashes, without pixel tolerance, region
masking after capture, case folding or automatic acceptance of changed text.

An instrumented fictional fixture verified stable viewport, tile and field
captures across blink intervals; zero observed DOM mutations or focus/input/change
events; retained focus, value, schema and caret color; restoration after an invalid
capture; a new document after navigation; and different images for changed text.
This is scoped evidence. Carets inside separately styled shadow roots, frames or
plugins may remain visible. Other animations still cause conservative freshness
warnings. The method does not establish that arbitrary sites have no side effects.

Focused review accepts supported filled text, choice and native temporal controls.
File inputs still require the separate byte/attachment check. Native month, time
and local date-time comparisons can carry a server-created normalization record
binding the exact observed display to the canonical typed value. The UI accepts
that record only with the matching observation, expected value, supported type,
current capture and successful live check. Organization names and other ordinary
text retain exact case-sensitive comparison. Ambiguous date formats are not guessed.

Idempotent status polls now distinguish `saved`, `already_saved` and `failed`
freshness records. A previously saved check is not reported as a new archive
failure, and a failed write cannot qualify automatic follow-up.

## Qualification and limits

All interaction data is fictional; all browser work ran headlessly. No real
employer form, email application or submission was involved. Native-model and
rendered-interface evidence are separate checks, not a claim that the entire app
has been qualified together on real platforms.

The local `gemma4:12b-it-q4_K_M` run used contact mapping, whole-form checked-history
planning, the reviewed-fill coordinator and actual production visual endpoints.
Seven of ten entered values matched the first full-page visual/live check. Three
organization-name casing warnings were resolved through focused visual/live checks;
four restricted fields stayed blank. An extra native month close-up passed through
the formerly text-only target endpoint, retaining its canonical-value binding.
The original full-page warnings and all follow-up captures/results remain saved.

The final checks and source-bound private receipts are listed below after the
rendered qualification. These controlled observations do not establish broad
platform coverage, repeated-run reliability or a general no-submission boundary.
The isolated-driver regression checks capture compatibility only; it does not
renew or broaden the earlier network-isolation evidence. Source-captured public
research informed the method; no upstream implementation was copied.

Primary implementation references: [CDP CSS protocol](https://github.com/ChromeDevTools/devtools-protocol/blob/master/json/browser_protocol.json),
[Chromium inspector stylesheet implementation](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/inspector/inspector_css_agent.cc),
and [Playwright screenshot preparation](https://github.com/microsoft/playwright/blob/main/packages/playwright-core/src/server/screenshotter.ts).
Native API/stream bytes, digests and extraction results are retained privately in
`target/visual-caret-research/`. The inspected current Blink settings source had no
caret-blink setting; no speculative browser flag was introduced.

## Checkpoint evidence

- Managed web suite: **745 passed, 399 opt-in checks ignored**, in
  `target/visual-followup-web-suite.log`. Browser library: **29 passed, five ignored**,
  in `target/visual-followup-browser-suite.log`. Web and browser Clippy and formatting
  passed; dependencies did not change.
- **162 standalone JavaScript scripts passed**, including the new multi-mismatch,
  three-attempt/time limits, bounded poll/recovery, stop, detached panel, ambiguous
  label, failed archive, changed page, failed live value and typed-display cases:
  `target/visual-followup-js-final-contract.private.json`.
- Actual 12B model and production endpoint evidence:
  `target/history-pipeline-ctiJ5s/endpoint-qualification.private.json`, SHA-256
  `2478e64320f07087ba9f4f6be0c811e12fc8ee46b91fb6aec8117562ae52e818`.
  The native month response contains the observed `June 2020`, canonical `2020-06`,
  month type and successful live check. Its original and focused captures/results
  are saved under that run's fictional workspace application record.
- Instrumented capture regression:
  `target/caret-capture-66592-1794256765117153800/qualification.private.json`.
  It tests the normal browser transport and does not claim shadow/frame coverage.
- One isolated-driver fill/navigation/PDF/capture/cleanup regression passed in
  the private `target/visual-followup-isolated-*` run. Its receipt explicitly limits
  the claim to capture compatibility; network tests were not repeated.
- Final rendered-app evidence is appended below. Earlier resize failure
  `target/visual-followups-ui-mi97fw/` exposed the unbounded initial image; shared
  responsive image styling fixed it. The zoom harness also needed the same bounded
  layout-settling interval used by the existing native-zoom qualifier. Its failure
  remains under `target/visual-followups-ui-mxTbAx/`; no visibility assertion was
  weakened. Three visual rounds reduced repeated warnings and grouped the original
  review without deleting it.

The early native endpoint failure is retained in
`target/history-pipeline-NG7Oex/`: its saved images differ at the 17-pixel caret,
with the mismatch/freshness warning correctly preserved. Subsequent stable captures
passed the production review checks. These receipts describe their own measured
source snapshots; they do not certify universal correctness or a release.

Final rendered qualification: **18 grouped cases passed**, including five viewport
widths (320, 390, 768, 1440 and 1920 px) and twelve actual browser-zoom combinations
from 80–400%, with visible/unobstructed controls and no script errors. Narrow/wide
and zoom captures were inspected through the visual refinements. The final receipt
is `target/visual-followups-ui-JDtQFm/result.private.json`, SHA-256
`c1df036804266dd510ee56f124084484e22b277e14735051a5514d3b8c41b810`.
The captured executable SHA-256 is
`5f7a622c7f4aec9ce529dfdabadec683f27f46e9550891511aa30dc2d0db0b08`.
The UI uses fictional intercepted responses; real local-model endpoint evidence is
listed separately above. The source in the compiled app was checked against the
served JavaScript before rendering the new workflow.

Additional receipt digests: instrumented caret regression
`d3239078ab4152190a6a1b6bfdae0a879c4977270bbbc85ab2e88a190684c850`;
isolated capture regression
`a1022c61acf61a62decc5667d9500b36aac665f2048ffdcad98ec4987034000d`
at `target/visual-followup-isolated-9q32962u/result.private.json`.
