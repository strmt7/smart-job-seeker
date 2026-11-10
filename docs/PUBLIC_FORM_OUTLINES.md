# Questions and controls in saved pages

Checkpoint: 2026-11-10. **Save page copy → Capture details → Page controls**
now exposes labels, descriptions, group context and reported required/disabled
states from the anonymous rendering checkpoint. Application records retain the
same inventory after reload. Controls remain plain text; a label such as
“Submit application” cannot become an action button in this view.

## Mechanism and limits

The render-only broker accepts one empty `SmartJobseeker.publicFormOutline`
request on its managed page session. It privately calls the documented
[Chromium accessibility-tree API](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/),
then emits a filtered inventory. It does not retain `AXNode.value`, checked-choice
properties, link destinations, selectors or browser node handles, and it drops
raw accessibility events.
Labels/descriptions remain untrusted page content and can themselves contain
private or misleading text. This is not a general privacy/provenance guarantee.

Typing, pointer input, uploads, arbitrary scripts, extra headers and navigation
away from the recorded entry remain refused by discovery. Raw accessibility
commands, extra parameters, repeated outline requests and the outline operation
in candidate-entry mode are also refused. This adds no model call or dependency.

Accepted trees have at most 16,384 nodes, 256 inspected controls and 65,536 serialized
control bytes. Whole labels/descriptions over 400/600 UTF-8 bytes are omitted;
up to three named group ancestors supply question context. Unknown flags stay
null. Missing labels, incomplete ancestry and omitted data produce a partial
observation. Invalid trees and failed reads stay unavailable. Inputs are shown
before buttons/links; numbering is an observation order, never a reusable selector.

The root-frame tree can miss embedded forms, hidden sections, unsupported widgets
and later questions. Observation does not establish application readiness,
correctness, visual verification or server acceptance. The inventory carries the
capture round and a hash of that immutable checkpoint's file inventory. It may
precede resources fetched later in the same pass; it is not the final form schema.
A failed later pass clears earlier outline/binding data rather than presenting
an older observation as current. Candidate entry still needs fresh inspection
and the remaining [isolation gates](ISOLATED_REHEARSAL.md).

The interface initially renders twelve controls, with bounded show-all/show-fewer
buttons. History validates the closed JSON contract and refuses value-bearing or
malformed metadata. Older copies explicitly show that controls were not assessed.

## Evidence

- Twenty-nine portable broker checks pass, including value/handle exclusion,
  protocol refusal, read errors/events, bounds, conflicting flags and ancestry.
  Rust checks cover closed metadata, unknown flags and stale-pass invalidation.
- A native fictional discovery/freeze/entry case observed the required “Full name”
  field, its “Your details” context and Continue button. Its prefilled marker was
  absent from the inventory. Three private targets stayed refused. Only after
  consuming the collector did a separate contained browser fill/read back name
  and email, refuse commit and close. Its screenshot was inspected. Receipt:
  `target/capture-replay-qualification-huuugo99/result.private.json`; native result
  SHA-256 `e72e8c3c5a7d1c94b9278d45d601f6287ae436e5ade07eda2c692135e4b25120`.
- The actual compiled UI copied the public RFC reference, recovered after reload
  and retained cancellation and control history. It recorded 176 controls as a
  **partial** observation. Three workflow checks and nine actual-zoom/width cases
  passed, including expansion/collapse, 320 px and 400% zoom. All three final
  surface screenshots were inspected. The refinement removes irrelevant required
  labels from links/buttons while preserving unknown input requirements.
  Final receipt: `target/source-copy-ui-cz2kragn/result.private.json`; rendered
  SHA-256 `1a66e40520f65d5d28551aee5fc8bc01eb19cceae26d5b763e54816dfc53dcfa`.
  This invocation took 29.875 seconds; 64 whole-host samples retained at least
  26,060,779,520 bytes (24.271 GiB) available RAM. These are host observations,
  not a reservation, application peak or long-running leak measurement.
- The ordinary workspace passed 1,228 tests with 580 opt-in cases ignored across
  43 reported targets; Clippy, formatting and the managed build passed. All 168
  JavaScript checks passed; the two affected controls checks passed again after
  the display refinement. Unchanged model/document matrices were not repeated.

No model, real employer form or candidate material was used in the UI run; the
reference contains links, not an application. It does not qualify application
identification or majority form coverage. No application, email or release was
created. Native captures, workspaces and screenshots remain outside Git.

Protocol research was captured with the isolated provider
`0.9.5+flitzzip.portable2`, HTTP 200, on 2026-11-10 at
`target/research/accessibility-outline.native.json`; native capture SHA-256
`0526b8edf0a71eed0c81e424dee42bfa83df454ddd15ed8fcfb334132feca11a`.
Only the published protocol was used; no third-party implementation was copied.

Reproduce with explicitly selected private test runtimes:

```powershell
py -3.13 -X utf8 -B tools/qualify_capture_replay.py --engine <absolute-headless-shell-directory> --privacy
./apps/web_app/scripts/build-windows.ps1
py -3.13 -X utf8 -B tools/qualify_source_copy.py --runtime-control <absolute-test-owned-runtime-directory-in-target> --browser <absolute-Chromium-executable>
```
