# Independent request representations — 2026-11-13

This continues local `main` from `3da8464`. The product goal and R03 remain
active. Public collection was read-only; native interaction qualifications used
fictional bytes in a zero-capability AppContainer. No application, application
email, release or desktop interaction was performed. Private source captures,
native reports and screenshots stay under ignored `target/`.

## Mechanism

An archive can now retain independently obtained responses for the same URL,
method and request-body digest when their existing request predicates are
provably disjoint. Explicit Origin values, proven Origin absence, supported
resource-type constraints and exact supported document Accept values determine
that separation. Overlapping or unbound representations are refused before a
new body file is written. Response order never chooses a representation.

The writer emits manifest version 2 only when multiple representations of a URL
exist. Version 1 remains readable and still rejects duplicate identities. Each
representation has its own immutable body file and digest. Version 2 permits at
most eight candidates per request identity, within the unchanged 512-resource,
8 MiB per-body, 64 MiB aggregate and 1 MiB manifest bounds. Replay requires
exactly one matching candidate and verifies that selected body's digest;
missing, ambiguous or changed responses have no fallback.

Native discovery carries observed request contexts separately. Explicit request
Origins must belong to admitted archived documents; absence needs the existing
anonymous Stylesheet GET proof. Credentials, body metadata, malformed headers,
unknown Origins and ambiguous document/asset contexts remain ineligible. The
128-proposal ceiling counts representations, not just distinct URLs, with an
additional 128 KiB context-metadata bound. The coordinator deduplicates actual
request contexts across rounds instead of marking an entire URL as already
collected. Reports distinguish recorded representations from distinct URLs.
Existing permission, robots, MIME, Vary, framing and transport limits remain.

[RFC 9111 section 4.1](https://www.rfc-editor.org/rfc/rfc9111.html#section-4.1)
requires stored responses to match their nominated request fields, including
the distinction between present and absent fields. Our bounded offline archive
supports only its explicitly implemented predicates; it is not a general HTTP
cache. Unlike a general cache, it refuses overlapping candidates rather than
selecting by recency or preference. No third-party implementation was copied.

## Native and public-source observations

The new fictional native scenario requests one stylesheet URL both with an
explicit Origin and without Origin. The collector independently obtains both
responses, writes version 2, then closes the live transport before the final
offline browser. Two discovery rounds record three representations for two URLs
from five simulated transport requests. Native computed styles independently
verify the first body's border `rgb(1, 2, 3)` and the second body's background
`rgb(4, 5, 6)`. One field is visible; zero fields are entered. Its screenshot was
inspected. This proves that specific native variant path, not arbitrary forms.

The earlier six-script dependency chain still renders its field after seven
rounds, and the embedded-frame context qualification still passes. Their
screenshots and bounded native reports remain separate from the new case.

A fresh capture of the existing permitted development employer source records
34 representations for 34 URLs from 42 requests and 2,367,700 response-body
bytes. It stops review-required after six rounds; collector elapsed observation
is 31,958 ms. Its manifest remains version 1: the final offline inventory still
observes an absent-Origin stylesheet mismatch that this collection did not
retain as a second representation. Ten native controls and three review walls
remain. Delayed POST, non-document entry requests and missing resources remain
refused. Different timestamps and bytes prevent a controlled speed comparison.
This real source has not gained a fidelity or workflow pass from this change.

No local-model run was repeated on that unchanged incomplete input. There is no
new model/GPU, held-out, complete-workflow or release-readiness claim. The next
step is to distinguish discovery versus final-inventory timing and native
request contexts without guessing headers or relaxing unknown variants. Other
requiredness, custom-control and representative-workflow gaps remain open.

## Evidence and resource scope

Focused Rust capture checks pass **55 tests**, with 9 ignored live tests.
Portable replay checks pass **66 tests**. The bounded workspace run passes
**1,354 tests**, with **602 ignored** across 43 executed test/doc-test results.
Initial Clippy found an overly complex type in the new native fixture. A named
request-key alias fixed it without a suppression; the final Clippy and format
checks pass. The full workspace result precedes that test-only alias; the new
native scenario is rerun afterward. The canonical Windows app build passes.
Earlier failed logs remain intact.

Private evidence roots:

- `target/representation-variants-zl51lz43/`: source tests, validation logs,
  research and compact checkpoint metrics.
- `target/capture-replay-qualification-4eq7n229/`: independent native variants.
- `target/capture-replay-qualification-blxbqoz9/`: deep-discovery regression.
- `target/capture-replay-qualification-2wdek6z7/`: embedded-context regression.
- `target/variant-source-capture-66awfkk3/`: new read-only source and offline
  inventory, original advertisement and application record.

The native variant receipt SHA-256 is
`03c9ae631af61d1780dbd9bfc2a910690118896b3403c06dda5aa9a6effe6447`.
The public capture receipt SHA-256 is
`4a5a0fabf6af22878f76a650b059e8b8374e8f3783ce4d261b2e23df3b9118dd`.
The RFC capture SHA-256 is
`900ec8db90beb0eb875567af4d473e0897deb53f06174a45cfb867e4fb90f4f3`;
the reviewed isolated Crawl4AI provider `0.9.5+flitzzip.portable2` retained native
HTTP 200, successful extraction, substantive text and no native error.

Native/build validation uses owned 6 GiB jobs and bounded sequential execution.
The public source app additionally has a 3 GiB owned child job. Its 134 nominal
500 ms whole-host RAM samples show at least 24,748,490,752 available bytes
(23.049 GiB); the initial native variant case sampled at least 30,531,833,856 bytes.
These are sampled host headroom observations, not exclusive application/GPU
peaks or proof of long-term leak freedom. Source collection records its owned
process tree drained. No shared model was loaded for this batch.

Final checkpoint audit observes zero matching owned processes, resident models,
services, scheduled tasks or startup entries, with no audit errors and
31,096,270,848 bytes available host RAM. This is a scoped ownership snapshot,
not a claim that unrelated host processes are absent.
