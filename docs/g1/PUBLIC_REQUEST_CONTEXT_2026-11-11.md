# Public request-context binding - 11 November 2026

The public asset collector now preserves one browser-observed public Origin,
bound to an already captured document. This repaired the saved Ashby page's
bootstrap rendering. Its actual application fields still require unavailable
data: the offline page shows an error and remains a review case. No live form
was filled, clicked or uploaded to; no application or email was sent.

## Mechanism

Only an anonymous, bodyless asset GET in the owned discovery context can propose
Origin. It must be a canonical public HTTPS origin, match an admitted document,
and pass independent native validation. A missing, unadmitted or conflicting
context cannot fall back to an originless collection request. Conflicting
proposals share the fixed 128-entry frontier budget and are removed from capture.
Neither URLs nor Origin proposals appear in the ordinary replay report.

The native API accepts a dedicated Origin type, not arbitrary browser headers.
It writes that one header on a bodyless asset GET and records the exact value
alongside complete outgoing header names. Robots requests have no Origin.
Target privacy, source Read permission, DNS/address checks, response budgets and
same-origin asset redirects remain in force. Cookies, authorization, form bodies,
preflight requests and application writes remain excluded.

The manifest binds a positive Origin even if the server omitted Vary. Replay
requires an exact matching Origin and rejects duplicate or malformed headers.
An Origin-varying response without Origin still needs native absence evidence
and is replayable only as a document. Unsupported asset modes remain reviewable.
Each URL still has one recorded variant; this is not a general browser cache.

The two Access-Control-Request headers can be marked absent only with complete
native GET-header evidence. Replay checks that absence and the GET identity.
Missing/forged bindings and explicit headers are refused. The
[Fetch Standard](https://fetch.spec.whatwg.org/#cors-preflight-fetch) defines those
forbidden request headers and OPTIONS preflights. No CORS response permission is
manufactured: the server's response headers are preserved.

The per-response archive allowance is now 8 MiB, after a real 4,274,145-byte
JavaScript bundle exceeded the former 4 MiB allowance. The collector's default
32 MiB aggregate body allowance, finite request/round/time budgets and browser
process memory bounds are unchanged. Replay request bodies remain capped at
4 MiB; increasing response capacity did not increase that limit.

Legacy explicit-default-port and IPv6 documents retain prior offline replay
behavior. Their origins are not currently projected for the new CORS collector.

## Evidence

Base commit: `0d00785`. Private receipts retain changed-source hashes, immutable
attempts, native response metadata, manifests and screenshots. Raw employer
material remains ignored; no real person or candidate profile was used.

| Ashby saved-copy stage | Resources recorded | Preview served / blocked | Controls | Observed result |
| --- | --- | --- | --- | --- |
| Before request context | 9 | 10 / 0 | 0 | Spinner |
| Origin only, unsupported preflight variant | 5 | 6 / 4 | 0 | Incomplete |
| Bound Origin and absent preflight headers, 4 MiB | 10 | 11 / 2 | 0 | Bundle exceeded resource budget |
| Bound context, 8 MiB | 11 | 12 / 2 | 0 | Styled application screen with an offline data error |

The final focused Ashby controller completed in 37.456 seconds. Source/binary
fences passed and its owned process tree drained. Sampled available host RAM
remained above 29.60 GiB. The measured binary SHA-256 was
`91342c713228bba9231addf8191a45062a96629e9c1b3d08c8931bc582385288`,
before an additive legacy-origin compatibility correction. The two other real
page families were compared in the earlier three-page request-context run;
their previews still exposed 0 and 9 controls. They were not repeatedly fetched
for the subsequent bundle-limit change.

The Rust/resource-change batch passed 1,279 workspace tests, with 590 ignored
across 43 result groups, plus Clippy and build checks. The final portable replay
suite passed 51 tests, including legacy-origin compatibility. A separate native
Windows capture/privacy qualification passed its one explicitly selected test,
including transport disposal before fictional entry and resource teardown.
No local model was loaded or evaluated in these source-collection checks.

The screenshot and refused-request counts establish an offline stop, not that
the live job is closed or that those requests would be safe to send. Classify
their methods and purposes before considering any new transport capability.
Do not relax the no-submission boundary to obtain missing application data.
Also qualify no-CORS assets whose Origin absence remains unproven, then repeat
usable-form and local-model evaluation. General application coverage remains open.
