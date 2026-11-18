# Web search recovery — 18 November 2026

The Windows search worker commits each returned page and advances its cursor
before making the next provider request. The workspace stores the reviewed
plan, prior results and terminal outcome as independently sealed records in
its SQLCipher database. Version 50 adds the journal. Existing single-record
histories remain readable. A failed receipt write stops the worker and leaves
earlier receipts recoverable. The shutdown path cancels and joins the worker.
The page and cursor share one transaction. [SQLite atomic commits](https://sqlite.org/atomiccommit.html).

The parser now limits normalized rows by byte size and indexes duplicate URLs
before merging their provenance. Search keeps the two MiB response and
normalized-row budgets, the eight MiB retained-run budget and per-provider
pacing. The quick search shows 40 candidates at a time. Batch recovery keeps
prior results by byte size, restores provider and freshness settings for
review, and places the retry action above the results.

## Windows qualification

The compiled app received 620 fictional candidate links on page one. The
fixture held page two's response while the owned app process was terminated.
The client had made no status calls. After reopening the same encrypted
workspace, the app restored all 620 candidates and cursor version 1 with one
committed page. The reviewed retry requested pages two and three and retained
the original page. The loopback provider's complete request sequence was
1, 2, 2, 3.

The rendered browser ran headless at 1440 × 900 and 860 × 820. The viewport
and settled document scroll widths matched in each run. The results rendered
40 cards, with the retry action above the first card and no blank inspection
status band. Candidate paging advanced to 41–80. Preparing the retry in the
page selected only pages two and three and retained the 620 prior links.
The fixture issued only search GETs to its loopback provider.

The Windows workspace ran 1,480 passing tests with 622 ignored. Clippy,
the Windows build and 172 UI scripts passed. These checks covered cursor
rollback on a failed row insert, stale cursors, legacy history and a retry
with 51 prior pages. The largest compiled app job committed 13,844,480
bytes and the rendered browser job peaked at 405,061,632 bytes. Available
host RAM stayed at or above 31,836,966,912 sampled bytes in the native run.
Both app instances, the renderer, the provider and the native listeners
were verified closed.

Private native evidence is in `target/search-evidence-8f9de38d`. Its report
SHA-256 is
`712bfd2cb3275a09e3ede37cc997a077336121e71f8972dbba991aa939cbe961`.
The report binds the final source files and app binary. The integration run
is under `target/search-release-gate-494dd7b5` and the final frontend build
and UI checks are under `target/search-verified-7953889c`. This qualification
covers process interruption, local page recovery and a reviewed retry.
Provider-specific backoff, public search recall and actual application-route
reachability remain open.
