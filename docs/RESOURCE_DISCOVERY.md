# Browser-observed resource discovery

Checkpoint: 2026-11-10. The public-resource coordinator can now discover anonymous
GET resources requested while rendering its captured entry. It makes bounded
passes through scripts, stylesheets and subsequent requests without an explicit
asset list. The component remains experimental. A [background source-copy workflow](SOURCE_COPY_WORKFLOW.md)
now connects it to job details and application records. This is not whole-site
crawling or application completion.

## Separate collection and entry modes

Worker configuration schema 3 means render-only discovery. It uses the existing
zero-capability AppContainer, verified private engine, immutable response archive
and continuously serviced interception. The worker only accepts a small protocol
allowlist: creating a blank target, attaching targets, enabling page/network events,
navigating to the archive's exact entry, reading version/counters and requesting
the discovery report. Extra parameters and other commands are refused. Typing,
pointer input, uploads, arbitrary script, document mutation, extra headers and
navigation to another URL cannot be requested by its host client. The ordinary
candidate-entry driver rejects schema 3 before starting a worker.

Missing requests still fail inside the contained browser; the discovery broker
does not fetch anything. Its bounded frontier includes only missing bodyless GET
URLs, without forwarding cookies, authentication, headers or request bodies.
The limit is 128 URLs and 128 KiB of URL text. Ineligible and omitted requests,
pending controller commands, unsupported targets and unreplayed transports remain
visible. Normal schema-2 entry/replay reports still contain no URLs and cannot
request this discovery report.

The coordinator closes the discovery browser and requires successful shutdown
before consuming any proposals. Every proposed URL then goes through the same
declared-origin, source-policy, robots, size and time checks as other resources.
Denied proposals are not retried repeatedly. Public, candidate-free URL provenance
and trusted origin/source bindings remain caller requirements; a page or model
cannot authorize a source.

Each round renders an immutable checkpoint. Source hashes are rechecked before
copying, changed/nonregular files are refused, and the current archive stays
separate from its rendering checkpoints. This preserves the worker's exact-tree
inventory check when the final archive is later used for candidate entry.
Checkpoints use sibling directories under the caller's selected parent, with
new-directory creation and no replacement. Four rounds retain at most four
64-MiB checkpoint trees; engine-copy and disk-admission checks also remain active.

One run permits one to four rounds and a 250–3,000 ms observation interval per
render. It also retains the enclosing capture deadline, request/body budgets and
cancellation. Discovery cannot be repeated indefinitely on the same coordinator.
After the observed graph is collected, consuming `freeze` drops the fetch
transport. Candidate entry starts in a separate schema-2 worker using only the
frozen files.

## Outcomes and limits

`no_new_requests` only means that the final bounded render proposed nothing new.
It does not establish page completeness. Delayed requests, unopened sections,
server-dependent POST questions, authentication, cookie/header-dependent behavior,
worker/transport gaps and missing origins can still need review. Other explicit
stops cover round limits, review outcomes, cancellation/expiry, browser/controller
failure and invalid checkpoints. These are saved in the coordinator report.
Checkpoint failure cannot activate a final archive. A browser discovery failure
can retain an otherwise valid partial archive with its failure recorded for review.

No real application form was interacted with during this development. Remaining
[isolation gates](ISOLATED_REHEARSAL.md), local-model verification, broader public
capture qualification and captured-form journey integration remain open. This does not
qualify an unrestricted no-egress boundary or enable live-form work.

## Verification

Portable checks exercise the actual discovery broker's command allowlist,
anonymous GET classification, frontier bounds and privacy of ordinary entry
reports. Coordinator checks exercise transitive collection, policy denial,
failure/cancellation, round limits, incomplete observations and changed files.
They run without public networking.

The native fictional fixture starts with HTML and a redirect. Its first render
discovers a script and stylesheet. The next render discovers JSON requested by
the script and a stylesheet imported by CSS. A third render observes no new
requests. The collector receives no list of those four asset URLs. Robots and
all six resource representations require seven fixture reads. Each discovery
browser closes before collection resumes. After freeze, a separate contained
browser serves five recorded replies, verifies fictional name/email entry and a
saved next section, refuses commit and closes. The screenshot is inspected.

The final selected-source/engine-frozen receipt is retained under
`target/capture-replay-qualification-2_gx2u5g/`. One native test ran with zero
ignored cases, taking 52.886 seconds including Cargo startup and concurrent
workspace compilation. The final whole-host sampling recorded at least
20,145,364,992 free bytes (18.762 GiB) over 106 nominal 500 ms samples. This is
not an exclusive application/GPU peak or a reservation. The final screenshot
matches the directly inspected rendering byte-for-byte; its digest is
`0aab9440c4178651125f84e8e161bb435d755d15242eb06a00dd8ea6e036250a`.
The native result digest is
`2704e2a76c9b96c5d4055497f4556e851dab8e0087c30a9de1f44a875e58516d`.

Final local verification passed 1,179 workspace tests with 576 opt-in cases
ignored, across 43 reported targets including empty documentation targets.
Twenty-three coordinator checks, 24 replay/broker checks, 14 worker checks,
browser Clippy, formatting and the managed SQLCipher app build passed. The
dependency graph and web interface were unchanged; their unrelated broad
qualifications were not repeated. The local batch has no new hosted result.

The first native attempt was refused by the owner's previous schema preflight
before a browser started. Its retained failure led to updating that preflight
for the same explicit render-only mode already enforced by the worker. No guard
was disabled and no fallback was introduced.

Primary protocol research used a settled, marker-checked native Crawl4AI capture
of the official [Page domain](https://chromedevtools.github.io/devtools-protocol/tot/Page/).
The retained result was HTTP 200 with substantive `Page.navigate` documentation.
No external application implementation or dependency was added.

```powershell
py -3.13 -X utf8 -B tools/qualify_capture_replay.py --discover --engine <absolute-private-headless-shell-directory>
```

The existing qualifier checks actual test denominators, freezes selected source
and engine inventories, uses an owned four-GiB Windows job with host-RAM
admission, samples whole-host RAM, and keeps private logs/screenshots under
ignored `target/`. No visible desktop interaction, application, email or release
is involved.
