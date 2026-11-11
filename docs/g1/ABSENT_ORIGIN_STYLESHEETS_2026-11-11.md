# Observed absent-Origin stylesheet replay - 11 November 2026

This R03 checkpoint restores a missing class of eligible stylesheet captures.
It does not establish general application completion, unseen-site accuracy or
complete rendering. R02/R03/R04 and the full product goal remain open.

## Mechanism and boundaries

The saved Greenhouse page had a successful CSS response, but `Vary: Origin`
made its representation unsupported: ordinary capture only bound absent Origin
for HTML navigation. A fresh headless Chromium 157.0.8103.38 probe observed a
bodyless ordinary Stylesheet GET without Origin, while `crossorigin=anonymous`
Stylesheet GET included Origin. The native paused headers did **not** include
Sec-Fetch-Mode; no such proof is claimed.

The collector now accepts an internal absent-Origin stylesheet scope only for
a native observed GET Stylesheet with explicit valid nonempty anonymous header
map. Independently, the HTTP response must retain complete valid request-header
name evidence: GET, no Origin, auth, cookies or preflight headers, with CSS MIME
(or a scoped redirect followed by eligible CSS). An ordinary CSS response does
not acquire this privilege from MIME alone. Conflicting request contexts remove
the proposal instead of selecting a convenient representation.

Saved entries bind both `request_origin_absent` and
`request_origin_absent_stylesheet`. Replay requires the same native bodyless GET
Stylesheet context, including for responses without Vary. Documents, Fetch/XHR,
scripts, CORS/credentialed requests, malformed or absent header evidence and
unsupported variants cannot reuse it. New absence proof may retry a formerly
unsupported variant once; repeated failures stay reviewable. Source permission,
robots, URL privacy, origin/redirect scope and resource/process budgets remain.

This adds no live browser fallback, cookie/header forwarding, candidate entry
permission, challenge handling or application-delivery capability.

## Native fixture and source collection

The existing native capture/replay qualifier now includes root and imported CSS
with `Vary: Accept-Encoding, Origin`. Fictional HTTP transport explicitly models
anonymous sent headers; actual Chromium independently supplies the native request
type and absence observation. The frozen manifest must bind both stylesheets.
Computed layout checks require 18px body text, block labels and 16px input margin;
the final screenshot was also inspected. The single selected native case passed.
Minimum sampled available host RAM was 24,809,775,104 bytes. This is whole-host
headroom, not exclusive process peak or a sustained leak test.

A **new** compiled-app read-only capture of the same public employer page saved
16 resources, including two bound absent-Origin stylesheets of 36,091 and 17,551
bytes. Old manifests and reports were not edited. The contained offline preview
served 13 requests, blocked 12 and exposed nine native controls. The screenshot
shows the restored typography and layout; banner/logo images are still missing.
Eight completion-review obligations remain, including country/location, work
authorization, sponsorship, experience and education custom controls. Those are
not completed controls or proof the whole application can advance.

The read-only capture/preview controller completed in 49.947 seconds and drained
its owned process tree. Minimum sampled available host RAM was 28,152,283,136
bytes. No candidate data or model was used in that capture/preview check.

## Local-model observation on the repaired archive

The compiled app then rehearsed the new archive **offline**, using only the
bundled fictional profile and local `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`, context
4,096. First Name, Last Name and Email passed native readback and local visual
review; their actual screenshots were inspected. All four before-entry target
checks, including **Phone**, visually matched. The former missing-label stop is
therefore resolved on this development case without changing the crop or gate.

Phone entry then failed the existing native exact-value/validity check. Its
status remains `entry_unverified`, the run halted and no section advanced. The
before-entry image shows the adjacent Country choice is blank. This observation
does not prove whether formatting, a country dependency or another input mutation
caused the failure. The failed after-entry value/image is not retained by this
error path yet: improve that generic evidence path and qualify masked/dependent
controls rather than assuming punctuation normalization or overriding validity.
The local model's successful target identification is not successful Phone entry.

This controller completed in 189.845 seconds including model admission and
teardown. The private runtime reported its session closed, the owned process
tree drained and the shared Ollama service remained unchanged and idle. Minimum
sampled available host RAM was 18,863,636,480 bytes. This is a development
observation, not held-out accuracy, a model throughput benchmark or complete-form
truth. No new expected-value oracle or corpus case is implied. Private evidence:
`target/public-style-model-czuntszm/`. The controller's `complete` flag describes
collection/teardown; it does not override the failed field or establish workflow
completion.

## Validation and retained evidence

- Replay Python: 54 passed, including new request-context, malformed manifest and
  discovery-conflict regressions.
- Browser library: 98 passed / 12 ignored at the focused checkpoint.
- Managed workspace: 1,312 passed / zero failed / 594 ignored.
- Warning-denied Clippy, canonical Windows build and formatting passed.
- Native fictional discovery/replay: one explicitly selected case passed; it is
  separate from the ignored workspace denominator.

Private receipts: `target/no-cors-assets/`,
`target/capture-replay-qualification-x13orxk7/`, and
`target/public-style-render-oq0641gh/`. Their manifests retain actual source,
engine, app, runtime and output identities. An initial native qualifier invocation
supplied the engine executable where an engine directory was required and was
refused before launch; the corrected directory invocation passed. The unsupported
semantic wildcard-filter attempt and successful restored search are retained.
No application or application email was sent, and no visible window was opened.

Primary protocol references were read through retained native Crawl4AI captures:
[Fetch Origin header algorithm](https://fetch.spec.whatwg.org/#origin-header) and
[CDP Fetch.requestPaused](https://chromedevtools.github.io/devtools-protocol/tot/Fetch/#event-requestPaused).
These standards explain the request distinction, not measured employer coverage.
