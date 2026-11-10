# Public resource collection before offline entry

Checkpoint: 2026-11-10. `waypoint_browser::capture_session` connects source policy,
anonymous resource GETs and the existing snapshot builder. It is a development
component, not yet an interface feature or a complete browser-driven collector.
It does not enable real application interaction or delivery.

The caller supplies a public, candidate-free entry URL and explicit mappings
from canonical HTTPS origins to source-registry entries. The caller must establish
those mappings and the URLs' public provenance; page text and model proposals
cannot authorize a source. Every resource and redirect target must belong to
the declared scope and pass the registry's public `Read` operation. Unknown,
forbidden and review-required sources do not pass. The transport accepts no
cookies, authorization, browser headers, request bodies or candidate material.

Robots rules are checked for each resource, with a cache limited to this short
capture run. The parser reuses `robotstxt 0.3.0`, an Apache-2.0 Rust port of Google's
parser, with no transitive dependencies. Its published archive was checked
against the registry checksum. [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html)
informed the behavior: bounded redirects, original-origin rule binding, UTF-8
rules and at least 500 KiB of parsing capacity. This implementation accepts
404/410 as absent rules, but treats authentication, rate limits, other errors,
HTML challenges, invalid UTF-8 and uncertain close-delimited bodies as needing
review. A 200 HTML page is not an empty robots policy. Rules and their hashes
are saved with response status and a fingerprint of the actual response URL.

Resource redirects retain their original representations and resolve to the
successful final entry. Every redirect, including robots redirects, is checked
against scope and source permission. Loops, ambiguous Location fields and more
than five redirects stop explicitly. Browser-equivalent URLs are canonicalized
and fragments stay outside request identity; the entry fragment is retained.

Defaults allow 96 resource proposals, 160 HTTP attempts, four MiB per resource,
32 MiB of received bodies including robots and discarded responses, and 90
seconds. Hard limits remain explicit. Denied proposals, omitted proposals and
partial coverage are reported, not converted into successful capture. Each
request uses the existing native network client, checked public addresses and
a remaining deadline capped at eight seconds. This is blocking worker code;
OS DNS resolution still needs an outer owned-process lifetime. The native
qualification uses the existing four-GiB Windows job and host-memory admission.

`freeze` consumes the coordinator and drops its private transport before returning
the archive. The returned value contains only snapshot files and a report.
No fetch method or live fallback is provided by that value. Cancelled, expired
or invalid entry captures retain an explicit report and cannot freeze as usable
archives. Callers must retain failed staging and invoke finalization on failed
operations; forced termination can leave only the existing incomplete marker.
Archive completion does not mean the page is complete or server-validated.

## Verification scope

Sixteen focused checks exercise authorization before network access, rules on
every resource, cross-origin redirects, redirect limits/loops, absent/unreachable
rules, HTML/authentication/rate-limit responses, canonical request identity,
budgets, cancellation, archive failure and transport disposal. No external
network runs in these checks.

An opt-in native check sends fictional HTTP responses through the actual parser,
policy/robots coordinator, redirect handling and frozen archive, then uses the
existing AppContainer browser for fictional name/email entry and a recorded
next-section request. Six anonymous fixture reads precede freezing; the transport
is dropped before entry. Four cached replies are served during the rehearsal,
readback succeeds, a screenshot is saved, commit is refused and normal close
completes. This is not a renewed qualification of every egress/broker channel.

The final wrapper receipt is retained in
`target/capture-replay-qualification-lg6336i0/result.private.json`, with all 19
selected source fingerprints and the engine inventory unchanged. One native
test ran, with zero ignored cases, in 32.517 seconds including Cargo startup.
Sixty-five nominal 500 ms whole-host samples recorded at least 19,239,874,560
free bytes (17.919 GiB); this is not an exclusive application/GPU peak or memory
reservation. The final PNG was inspected and matches the earlier inspected
rendering byte-for-byte. Its digest is
`0aab9440c4178651125f84e8e161bb435d755d15242eb06a00dd8ea6e036250a`.
Native result digest:
`bd5035ca70cade6e906fe2e67009a8d2c0866c4f60d3f978023d2ca052075e0b`.

Two separate native public-read checks captured the RFC page and a public Lever
listing, preserving 82,367 and 797,400 HTML bytes respectively. Each required
one robots GET and one page GET; both robots responses were 200. No browser or
page script was used in these public captures. They do not establish application
form completeness, local-model quality, server validation or delivery behavior.

Final local workspace checks passed 1,172 tests, with 575 opt-in cases ignored
across 43 reported targets including empty documentation targets. Browser Clippy,
formatting, the managed SQLCipher application build, license-inventory checks,
and dependency advisory/license/source checks passed. No existing dependency
version changed; the lock graph adds only the pinned robots parser. The preceding
`7ab0fee2` hosted checkpoint passed all CI jobs, coverage and CodeQL, including
137 browser and 296 layout cases. Those hosted results do not verify this new
local batch, and the unchanged full native browser/layout suites were not repeated.

Primary-source research has retained native Crawl4AI captures. The first CDP
read returned an incompletely rendered homepage shell and was rejected as
wrong-scope evidence. A second settled capture with the `fulfillRequest` marker
retained substantive [Fetch documentation](https://chromedevtools.github.io/devtools-protocol/tot/Fetch/).
The RFC and public listing captures were also checked for substantive content.
No third-party application implementation was copied.

## Remaining integration

The subsequent [browser-observed discovery](RESOURCE_DISCOVERY.md) collects
transitive anonymous GETs through bounded render-only rounds. It has fictional
native evidence, not a public whole-site qualification. Related worker/transport lifecycles, server-dependent POST
questions, authenticated pages, content variants and missing resources remain
explicit gaps. The [isolation qualification gates](ISOLATED_REHEARSAL.md),
candidate-data provenance checks and captured-form journey integration remain
prerequisites for candidate entry. A [bounded background source-copy task](SOURCE_COPY_WORKFLOW.md)
now exposes read-only collection in job details. No live-form
interaction is authorized by this checkpoint.

```powershell
py -3.13 -X utf8 -B tools/qualify_capture_replay.py --coordinator --engine <absolute-private-headless-shell-directory>
$env:WAYPOINT_CAPTURE_PUBLIC_URL = 'https://www.rfc-editor.org/rfc/rfc9309.html'
cargo test -p waypoint_browser --test capture_public_live -- --ignored --nocapture --test-threads=1
```

The public check accepts only its two reviewed reference/listing URLs and never
targets an application endpoint. All captures, screenshots and receipts remain
under ignored `target/`. No release is created.
