# Typed anonymous asset capture — 2026-11-13

This continues local `main` from `52462835c658ca33f15bd20cd9dcdab3f3167ebc`.
The product goal and R03 remain active. Public collection stayed read-only;
interaction qualifications used fictional bytes inside a zero-capability
AppContainer. No application, application email or release was sent or published.
Public-source captures and screenshots remain private under ignored `target/`.

## Mechanism and boundaries

Absent-Origin proof now covers observed script, stylesheet, font, image, JSON
and manifest asset categories. The same MIME predicate serves the archive writer
and coordinator. A category comes from the controlled browser's observed request,
not a response's MIME alone. It requires an anonymous GET, native request-header
evidence, no Origin, credentials, body or preflight metadata, and compatible MIME.
Unsupported `Vary` fields remain unsupported. No HTTP headers were overridden.

New typed bindings use manifest version 3; versions 1 and 2 remain readable and
retain their existing matching rules. Legacy stylesheet binding remains supported.
Replay checks the recorded asset category, method, body and headers before serving
the selected immutable body. Wrong types, changed bodies, unknown contexts and
ambiguous candidates have no fallback. Different proved categories can be disjoint
variants of one URL; there are still at most eight candidates per request identity.

Discovery metadata is versioned and validated before asset transport. Each
anonymous category must match its observed category set; invalid combinations,
unadmitted Origins and conflicts are refused. Actual request contexts share the
existing 128-proposal and 128 KiB context-metadata limits. The coordinator uses
those contexts for deduplication; a separate stylesheet retry exception is no
longer needed. Session time, byte, resource and request limits remain unchanged.

The existing JSON-preload path recognizes Chromium's `Other` type only for an
explicit anonymous fetch-preload declaration in an archived document. Arbitrary
`Other` requests cannot borrow an asset response. Only successful GET HTML
documents contribute document Origins and preload declarations; an asset redirect
with an HTML body cannot supply either. POST replies are not manufactured.

The [Fetch Origin algorithm](https://fetch.spec.whatwg.org/#append-a-request-origin-header)
adds Origin conditionally. Absence alone does not prove an anonymous request; the
additional native method/header/category checks remain necessary. The
[Fetch Metadata caching guidance](https://www.w3.org/TR/fetch-metadata/#vary)
also requires respecting metadata-dependent variants. This change does not solve
the remaining `Sec-Fetch-*` representation. No third-party implementation was copied.

## Native and actual-source results

The new native fixture independently collects a script, SVG image and JSON
question response, all with `Vary: Origin`. Three discovery passes produce four
resources from six simulated HTTP requests. The collector is consumed before
final offline rendering. Chromium decodes the image at its expected 16×12 natural
size, executes the captured script, and exposes the JSON-provided required question.
Two native controls are visible, with zero refused requests and zero entries.
The screenshot was inspected. Font and manifest contracts have unit coverage;
this fixture does not establish font decoding or native manifest-install behavior.

A fresh permitted development source now records **38 representations for 37
URLs**, from **43 requests** and **2,618,319 response-body bytes**, in seven passes.
The collector's elapsed observation is **34,477 ms**, within its existing budget.
It writes manifest version 3 and retains the three image responses previously
rejected. The final screenshot visibly restores the employer logo and background.
The previous capture had 35 representations for 34 URLs, also from 43 requests;
previously downloaded but unsupported images now remain in the archive. Different
capture times and bytes do not establish a speed or bandwidth benchmark.

Final inventory still has ten controls and three review walls. Missing-response
refusals decrease from nine to three; one document-context refusal remains. The
remaining samples are two stylesheet observations, one XHR POST observation and
one `Other` document-context observation. The copy remains review-required.
Typography, server-dependent behavior, requiredness and broader workflow fidelity
are not complete. No new model/GPU run, independent answer score, held-out pass or
whole-workflow pass is claimed.

## Validation and resource scope

- Portable replay: **72 passed**, covering category/MIME/schema incompatibility,
  credentials and body metadata, independent body selection, changes after capture,
  document/preload provenance, ambiguity and proposal budgets.
- Focused Rust capture tests: **78 passed, 13 opt-in tests ignored**.
- Workspace: **1,357 passed, 605 opt-in tests ignored**, across 43 test/doc-test
  results. This precedes an equivalent schema-range spelling correction and a
  test-fixture move cleanup; focused capture tests, final Clippy, native fixture,
  format check and canonical Windows build pass afterward.
- The earlier three inspector/variant native checks, deep dependency chain and
  embedded-frame context checks pass. This is scoped regression evidence, not
  renewed qualification of every browser transport or installed-product flow.

An initial compile error exposed a remaining boolean use after the typed context
conversion; it is fixed. The earlier stylesheet-versus-script tombstone assertion
now distinguishes two proved disjoint categories while retaining an unknown-type
tombstone check. Clippy's schema-range finding was fixed without suppression.
Earlier failure logs remain retained; no detector findings were manually closed.

Private evidence roots:

- `target/anonymous-assets-d34j4vu7/`: research, source fingerprints and test logs.
- `target/capture-replay-qualification-huzfrf9o/`: final native fixture, receipt
  SHA-256 `250c4caeb871a985e023a95bfc5b1352f10c45b81978e84ab3ffbfc4d2452c84`.
- `target/asset-source-capture-yukaxyd6/`: compiled read-only collection, original
  advertisement, archive and offline inventory; receipt SHA-256
  `312dc7a42f227ac894933f67b64174a9e217e78e3d6579daf8692f62d4014a47`.

The reviewed isolated Crawl4AI provider retained successful HTTP-200 substantive
extractions with no native errors. Fetch receipt SHA-256:
`2f33072ee819dd9d42ca6dc8e455ebd515c05a093eb1992944189f13d015e3db`;
Fetch Metadata receipt SHA-256:
`7e3ee7251390dd2429518d4f445a0e6d8366d164013353fda875e71f8b2b32d9`.
No paid or external-model API was used.

Heavy work ran sequentially in owned 6 GiB jobs. The source app additionally had
an owned 3 GiB child job and records its process tree drained. Its sampled minimum
available host RAM was **29,611,384,832 bytes**; the final native fixture sampled
at least **31,237,750,784 bytes**. Final audit observes zero matching processes,
resident shared models, services, scheduled tasks or startup entries, no errors,
and **31,325,339,648 available RAM bytes**. These are scoped teardown and sampled
headroom observations, not exclusive peaks or proof of long-term leak freedom.
