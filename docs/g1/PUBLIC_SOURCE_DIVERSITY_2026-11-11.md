# Public source-copy diversity - 11 November 2026

The compiled Windows app saved three anonymous employer-page copies through its
normal headless web interface. This repaired shared collection failures; it did
**not** establish general application-completion coverage. No live employer form
was filled, clicked or uploaded to. No application or application email was sent.
Real page bytes and screenshots remain private under ignored `target/`.

## Changes

- Native GET records the complete header names actually written. A response-only
  stream carries no evidence about absent request headers. HTML varying on Origin
  requires native proof of absence and replays only for document requests without
  Origin. Unsupported optional variants remain review items without poisoning
  unrelated valid resources.
- Observed anonymous scripts, styles, fonts, images, JSON and manifests can use
  up to four supporting origins. Each URL requires source Read permission,
  privacy checks, robots evaluation, bounded transport and matching response
  MIME. Same-origin redirects retain that scope. An asset origin never gains
  document access. Restricted provider hosts retain their shared source policy.
- Explicit head declarations for JSON fetch preloads can explain observed
  browser requests classified as Other. Declarations alone cause no fetch.
  Unknown encodings and ambiguous declarations remain reviewable.
- A narrow XML AccessDenied response for a supporting origin's robots file can
  be treated as unavailable under RFC 9309's 4xx discretion. This applies only
  to typed, observed assets and always marks the copy for review. Primary and
  child documents, authentication/rate-limit responses, interactive walls,
  explicit robots denials and unreachable rules retain their existing stops.
- Bounded diagnostics retain URL digests, status, MIME, body size, framing and
  Vary field names, excluding bodies, raw URLs and arbitrary header values.
  The interface explains unsupported variants and unknown reading rules.

Browser headers, cookies, credentials and form bodies are not forwarded.
External requests remain anonymous GET only. Replay has no online fallback;
application-write permissions are unchanged.

## Measured results

Baseline: `7a8577e0c925f5c65617f06e50d73d90ebfefb41`. Private receipts retain
changed tracked-file digests, native crawl captures, source fences, task records
and failed attempts. Final compiled binary SHA-256:
`dadbeaf5e2030e4a271849d18a02801b31a90ac565152df582b36ef2910b0f02`.

| Real page family | Baseline archive | Final archive | Preview controls | Final served / blocked |
| --- | --- | --- | --- | --- |
| Workable | Refused variant | Saved; review required | 0 | 3 / 5 |
| Ashby | Saved; incomplete | Saved; review required | 0 | 10 / 0 |
| Greenhouse | Refused variant | Saved; review required | 9 | 1 / 16 |

These are three pages, not representative platform coverage. Controls are
observed preview controls, not filled fields. Screenshots still showed incomplete
rendering. Zero blocked requests does not prove scripts ran or a form was usable.

The final controller completed in 99.214 seconds. Source/binary fences passed
and its owned process tree drained. Whole-host RAM sampled every 500 ms remained
at or above 25.30 GiB available. This is sampled headroom, not peak process/GPU
usage or a memory-leak proof. No model was loaded for this qualification.

Validation: workspace tests passed with 1,273 passed and 590 ignored across 43
test-result groups; Clippy with warnings denied, formatting, 45 Python replay
tests, one focused source-copy UI test, Cargo audit with warnings denied and
Cargo deny licenses/bans/sources passed. Ignored live/model tests are outside
the passing workspace denominator.

## Next gate

Real CDN responses exposed unsupported Origin/preflight-dependent variants.
The Ashby bootstrap manifest also lacked CORS response headers when fetched
without browser Origin. Native replay preserved that response; downloading it
did not make it usable by a browser fetch. An exploratory private Playwright
route was not accepted as production evidence because its fulfillment behavior
can change CORS headers.

Capture a narrowly validated public request context, bind responses to the
context actually sent, and match it during offline replay. Do not manufacture
CORS permission, remove Vary checks, treat missing evidence as absence or count
a spinner as completion. Multiple variants, implicit browser headers, preload
and credential modes, and redirects need adversarial coverage. Then repeat real
saved-page comparison and the local model's own fictional-entry/visual checks.

## Research basis

Official pages were read through the isolated, robots-checked development
crawler. Native successful results, extracted text and hashes are private.
Variant handling follows [RFC 9111 section 4.1](https://www.rfc-editor.org/rfc/rfc9111.html#section-4.1).
Origin handling uses the [Fetch Standard's Origin-header algorithm](https://fetch.spec.whatwg.org/#append-a-request-origin-header).
Limited unavailable-robots handling follows [RFC 9309 section 2.3.1.3](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.3.1.3).
No stealth, challenge solver or CAPTCHA bypass was used.
