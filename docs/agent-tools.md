# Development agent tools and coverage

These tools assist development and research. They are separate from the Rust
application, do not process candidate data by default, and do not submit jobs.

## CocoIndex Code

With CPython 3.13 available, run `py -3.13 -B tools/cocoindex_agent_search.py install`,
then `index`, then `search --limit 5 "question"`. Installation is explicit;
it creates a dedicated environment and may download packages and an embedding
model. The direct package version is pinned, but transitive Python dependencies
are not hash-locked. An existing exact-version executable can instead be named
by absolute path in `WAYPOINT_COCO_CCC`; it is used read-only and does not share
its settings or index. The pinned embedding model is downloaded to an owned
directory without cache symlinks and its files are hash-checked. An existing
model snapshot can be selected read-only via `WAYPOINT_COCO_MODEL_DIR` and is
subject to the same checks. The index uses CPU and keeps its mirror, index and search
receipts under the current user's Waypoint cache. `WAYPOINT_COCO_HOME` may name an
absolute external alternative; it cannot point inside the checkout. Only
tracked eligible source is mirrored. Add new source to Git's index before
indexing it, and never use private data or secrets in a query. Search refuses
source changes until an explicit reindex. The returned paths are candidates;
verify them in the current source.

On Windows the adapter checks available RAM before launch and limits CPU math
threads; this is admission control, not a hard memory limit on the index daemon.

## Crawl4AI

Use CPython 3.13 or 3.14 and run `py -3.13 -B tools/crawl4ai_tool.py install`.
It creates a dedicated user-cache environment, verifies pinned upstream source,
builds the reviewed portable wheel, installs hash-locked dependencies, checks
the installed package, and installs a private Chromium browser. This requires
network access and disk space. `WAYPOINT_CRAWL4AI_HOME` or `--home` can select
another absolute directory outside the repository; it never reuses or changes
another project's environment. `doctor` checks local extraction. To read a
public page and retain evidence, choose a new output file, for example:

```powershell
py -3.13 -B tools/crawl4ai_tool.py capture https://example.org/ --capture target/research/example.native.json
```

Captures include the native result, status, error, provider version and digest.
They are never overwritten. The adapter checks robots, uses a bounded timeout,
and does not enable stealth, personal browser profiles or external LLMs. Keep
queries and URLs free of candidate information and credentials. A successful
crawl is evidence for that page at that time, not permission to use it in the
product: Waypoint's source policy and permission registry still apply. `qualify`
is an explicit multi-site network run, not an installation prerequisite.

## Coverage

The coverage workflow runs Rust workspace tests under `cargo-llvm-cov` on a
Windows runner and uploads the measured LCOV report to Codecov using GitHub
OIDC, without a stored upload token. Fork pull requests retain the artifact but
skip the authenticated upload. Coverage excludes ignored live-browser and model
tests, JavaScript, Python and runtime behaviors not exercised by those Rust tests.
It is a testing aid, not evidence of correctness or a release qualification.
The Codecov badge may show no data until the hosted integration accepts its first
upload. Local focused collection uses `cargo llvm-cov -p waypoint_domain --lcov
--output-path target/domain.lcov`; full collection uses `--workspace` and the
same SQLCipher/OpenSSL build environment as the application.
