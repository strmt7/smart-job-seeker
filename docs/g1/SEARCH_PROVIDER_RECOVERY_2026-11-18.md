# Search provider recovery — 18 November 2026

Quick and batch web search share a retry schedule for each provider origin.
The HTTP client reads a single valid `Retry-After` field from the final
response, accepting delay seconds and HTTP dates. HTTP 429 and 503 pause that
origin. An absent or malformed hint uses a 60-second default. Access refusals
remain a separate review outcome. The app never starts a retry automatically.
[HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html#section-10.2.3),
[HTTP 429](https://www.rfc-editor.org/rfc/rfc6585.html#section-4).

Version 51 retains active pauses in the encrypted workspace without replacing
search receipts. A path change on the same origin keeps the schedule. A
different origin has independent admission. A deferred search leaves the
prepared pages and saved results in place. A quick search releases its worker
when complete even if its browser tab closes. An unread result remains
available for the next client.

## Controlled Windows qualification

The compiled app saved a candidate on page one, received HTTP 429 on page two
and retained the pause across restart. Quick and batch admission both deferred
the same origin. The second provider returned 403 without a pause, followed
by 503 with an absolute retry date. An unread quick result released the shared
worker and remained recoverable. After the first pause expired, an explicit
retry requested only pages two and three and preserved the first page. The
second provider stayed paused. The six fixture responses were 200, 429,
403, 503, 200 and 200.

The headless app showed the retry notice and kept the two prepared pages at
1440 × 900 and 860 × 820. Its rendered result held one earlier candidate.
The browser and app jobs drained, and the final temporary browser profile
was removed. An earlier rendered run hit a Windows sharing violation during
profile cleanup. Ownership and listener checks found no survivors from that
run. Its disposable profile remains under ignored `target/` because the
execution policy denied recursive cleanup.

The workspace passed 1,484 tests with 622 ignored. Clippy, the Windows build,
172 UI scripts and the dependency license, ban and source checks passed. The
final native run sampled at least 29,795,426,304 bytes of available host RAM.
Its browser process job peaked at 385,859,584 bytes of commit.

## Public search and application routes

Three anonymous materials-engineering queries through the compiled app's
FreeSerp integration returned 30 links over three successful pages. Engine
availability was unreported on all three. This was a small live sample.
[FreeSerp API documentation](https://freeserp.ai/docs.php).

Two vacancy-looking candidates were checked read-only through the app's
application-route workflow. One listing returned HTTP 404. The other
redirected to a same-origin careers homepage with no application-specific
signals. Both checks requested manual review. Zero of the two inspected
candidates reached a confirmed application form. Candidate data entry and
submission were outside the run.

The final native report is under `target/provider-proof-287f6f09` with
SHA-256 `b3fac95a672b32c965ed19c1771a8fa676a86545b5fee82e301c0851055e9c9b`.
The search sample in `target/public-provider-probe-838e48b0` has report digest
`83365dea1fc906e3eaba8a7b8092db43f2462d0847daaa654b1271dc6486663e`.
The linked route report in `target/public-route-probe-768bb4e2` has digest
`2f137f23c35b19c011cebb11c264fff20d807788c33577f2b5e4f93a1136c5a8`.
The paused-origin schedule is independent. Discovery still stores one latest
resumable batch. Separate provider histories, labeled retrieval quality and
reliable coverage of live application routes remain open.
