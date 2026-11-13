# Observe stylesheet inspection before freezing — 2026-11-13

This continues `main` from `14bb31ad19b13e87c3cab98ebbf3a629e270ed60`.
That exact pushed commit's CI, coverage and CodeQL runs passed. This checkpoint
has separate local evidence below. The product goal and R03 remain active.
Development stayed headless. Public collection was read-only; all browser
interaction used fictional fixtures or isolated saved copies. No application,
application email or release was sent or published.

## Native cause and repair

The extra stylesheet request was caused by inspection, rather than proven to
need a longer settling delay. A native fictional page has one cross-origin
stylesheet link with `crossorigin=anonymous`. Its original page requests are
served without refusals. Enabling the CSS inspector afterward produces a
Stylesheet GET with absent Origin; the archive containing only the explicit
Origin representation correctly refuses it. Creating an inspector stylesheet
afterward adds no further refusal in this measured case. The browser is closed
before the phase assertions, and the reports are retained privately.

Discovery now enables DOM inspection before navigation and CSS inspection after
the current page's load event, within the existing settling window. The
activation phase matches final screenshot preparation rather than enabling CSS
inspection before navigation.
The broker permits only empty-parameter `DOM.enable` and `CSS.enable` in this
discovery path. CSS creation/edits, script evaluation, input, upload and arbitrary
navigation remain refused. No request header is guessed or overridden.

If the load event is not observed in time, the native report records
`stylesheet_inspection_pending`; discovery stays review-required even with no
new proposals. Capture details show the shared warning, “Some styling could not
be inspected. Review this copy.” The session time, byte, resource, request and
discovery-round limits remain unchanged. Incompatible response variants still
cannot substitute for one another.

The [primary CDP CSS protocol](https://chromedevtools.github.io/devtools-protocol/tot/CSS/#method-enable)
defines CSS-agent activation and requires waiting for its response. The helper
waits for that acknowledgment. The additional request and its context are
native observations for the qualified engine, not a protocol guarantee for all
browser versions. No third-party implementation was copied.

## Actual collection and rendered results

The inspector-only native fixture now independently collects both request
contexts in two passes: three representations, two URLs, five simulated HTTP
requests and one visible field. The original stylesheet retains its explicit
white background and border; the separately served inspector-source body does
not become a second applied stylesheet. Final inspection has zero refusals and
zero entries. The original two-link variant fixture also passes. A third native
test preserves the causal observation before collection is repaired.

A fresh permitted employer-page collection now records **35 representations for
34 URLs**, **43 requests**, **2,618,357 response-body bytes**, six passes and
**30,257 ms** collector elapsed observation. It writes manifest version 2. Final
offline inventory no longer reports the stylesheet Origin mismatch; its remaining
refusals are `document_variant_not_recorded: 1` and `response_not_recorded: 9`.
All six rounds report stylesheet inspection complete. Ten controls and three
review walls remain. The screenshot still shows missing imagery and other
fidelity limitations. Unknown responses, a delayed POST and unsupported request
contexts are not manufactured or ignored. This is not a complete workflow or
held-out pass. Capture times and response bytes differ, so this is not a speed
benchmark against the previous collection.

Local validation passes:

- Workspace: **1,354 passed, 604 opt-in tests ignored**, across 43 test/doc-test
  results; Clippy, format checks and the canonical Windows build pass.
- Portable replay: **66 passed**, including the expanded discovery-command
  refusals. Pending stylesheet inspection remains review-required in Rust.
- Three native inspector/variant cases and the earlier deep-discovery and
  embedded-frame qualifications pass.
- The source-copy client check covers true, false, absent and malformed pending
  flags. The actual component and production styles render without horizontal
  overflow at 1360, 800, 640 and 420 px, with no page errors. Wide and narrow
  screenshots were inspected. This is component evidence with fictional API
  responses, not a full installed-app resizing or zoom qualification.

Initial qualification failures exposed fixture directory reuse and incorrect
expectations about pass count and default input background. Distinct fixture
destinations and explicit owned background styling correct the fixture contract;
both independently captured responses, final refusal counts and field-entry
assertions remain required. Failure logs are retained. No detector finding was
manually closed or suppressed.

## Local-model continuation

One changed-input check used the fresh saved source, fictional candidate data,
the verified existing cache and local `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
context 4096. It proposed declining non-necessary cookies, then correctly stopped
before the action because new unreplayed requests appeared. No fields were
entered, no sections advanced and no complete workflow or independent model
accuracy score was established. The final replay report still has one refused
document-context request and 17 missing-response refusals. Successful controller
completion is not a successful application workflow.

The 114.581-second contained run reused the model cache, closed its private model
session and app process tree, and verified the shared runtime unchanged. Native
aggregate app-job peak committed memory was **8,870,383,616 bytes**, inside its
10 GiB job and the controller's 12 GiB outer job. Across 226 nominal 500 ms samples,
available host RAM stayed at least **21,587,955,712 bytes**. Six identity-bound
runtime samples reported only approximately 0.95–1.15 billion VRAM bytes; a
concurrent whole-adapter WDDM sample reported **15,485,628,416 used bytes** on a
**16,977,985,536-byte** adapter. These measure different scopes and do not prove
process GPU usage or a continuous peak. Runtime self-report must not substitute
for physical GPU qualification; this is not a general 16 GiB deployment claim.

The final host audit observes zero matching development processes, resident
shared models, services, scheduled tasks or startup entries, no audit errors,
and **30,349,139,968 available RAM bytes**. The observed shutdown and snapshot
do not establish long-term leak freedom.

## Private evidence

Evidence stays outside Git under:

- `target/late-request-context-n5a_b3qh/`: native diagnosis, research, client
  rendering, source/test fingerprints and validation logs.
- `target/capture-replay-qualification-a6ous7sr/`: three native inspector/variant
  cases; receipt SHA-256
  `9a0b2c4fdaf084c12d59d075f2c1d3ba657aec0ca0360c15858ce074f3b70426`.
- `target/inspector-source-capture-y8p3v7vl/`: compiled read-only capture and
  offline inventory, original advertisement and application record; receipt
  SHA-256 `80e9d10f23d177c0f51999e87b3adb1adb787373003b3aea05ec280d3b2adda4`.
- `target/inspector-dialog-model-vet9uyk3/`: changed-input local-model check;
  receipt SHA-256
  `596218a5e669d5df96203f9cf0280b21a8a8f13b81b5ee51a45ba9dbca185991`.

The reviewed isolated Crawl4AI provider retained native HTTP 200, successful
substantive extraction and no native error for the protocol page. Its receipt
SHA-256 is `f33adbe4e813e9b1a4aa4b98a82b3e7f7ad22cdf653d0883ef8b7be540dc665a`.
Native/build work runs sequentially in owned 6 GiB jobs. The public collector app
has an additional owned 3 GiB child job and records its tree drained. Its sampled
minimum available host RAM is **29,731,926,016 bytes**; the three-case native
qualification sampled at least **30,816,677,888 bytes**. These are sampled host
headroom observations, not process/GPU peaks or long-term leak proof.
