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

The application index excludes `third_party/` so upstream amalgamations do not
dominate retrieval or embedding memory. This is an explicit coverage limit.
Use exact paths for dependency inspection and `python tools/verify_sqlite_vendor.py`
to check the reviewed SQLite binding, its file inventory and manifest patch.

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

For a client-rendered page, `--settle-seconds 5` adds a bounded wait after page
load; the maximum is 10 seconds and the overall crawl remains time-limited.

Captures include the native result, status, error, provider version and digest.
They are never overwritten. The adapter checks robots, uses a bounded timeout,
and does not enable stealth, personal browser profiles or external LLMs. Keep
queries and URLs free of candidate information and credentials. A successful
crawl is evidence for that page at that time, not permission to use it in the
product: Waypoint's source policy and permission registry still apply. `qualify`
is an explicit multi-site network run, not an installation prerequisite.

## Inert form-structure fixtures

`py -3.13 -B tools/inert_form_fixture.py <local-html> target/<new-file>.html`
creates an inert, local-only approximation of standard controls and labels from
an existing HTML capture. It omits scripts, form submission, network attributes,
entered values and unsupported controls. Output is restricted to `target/`;
captures and fixtures can contain private or third-party text and must not be
committed. This is not a replay of the original site's behavior, styling,
JavaScript, validation, custom widgets or multi-step flow. Use it alongside
other evidence, not as proof that the real application can be completed.
For the headless local browser qualification, set `WAYPOINT_INERT_FIXTURE` to
the generated file and `WAYPOINT_INERT_FIXTURE_SHA256` to its SHA-256, then run
`cargo test -p waypoint_browser --test cdp_live inert_private_form_structure_qualification -- --ignored --exact`.
The test is excluded from CI because the private fixture is not in the repo.

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

## Embedded Rust license inventory

Run `py -3.13 -B apps/web_app/scripts/update-license-inventory.py` after reviewing
Cargo dependency changes, then use `--check` to verify the saved inventory.
The generator uses locked, all-feature Cargo metadata for all resolved target
platforms, records the lockfile digest and keeps complete SPDX expressions.
The UI embeds this inventory; it is separate from the full dependency license
files collected by the Windows packager. CI checks inventory freshness.

The earlier native GUI packages were retired on 2026-11-09. Their source and
tests remain in Git history through `b166ea93`; they are outside the current
workspace and coverage denominator. Historical native-GUI gate records must not
be presented as current web-app coverage or verification.

## Headless layout qualification

Install the pinned free development dependency with
`npm install --prefix target/layout-qa --no-save --ignore-scripts playwright-core@1.65.0`,
build the app, then run `node apps/web_app/scripts/qualify-layout.cjs`.
Use `--browser <absolute Chromium executable>` for a nonstandard installation,
`--executable <app executable>` for a packaged build, `--quick` for focused drag
checks, or `--source-assets` for an explicitly reported uncompiled design pass.
The default checks compiled assets. `--help` requires no browser or app launch.

The tool uses an isolated headless browser/profile and a private Windows app
workspace under `target/`. A temporary test extension controls actual per-tab
browser zoom; its debugging flag is confined to that test browser and is not a
product option. The renderer's external requests are refused. Jobs, documents
and PDF preview responses are fictional; this is layout evidence, not a document
or employer-form completion test. Results bind the binary digest and browser
version; CI retains JSON observations only. Local screenshots can include paths
and must not be committed. Available-memory checks are admission checks, not
reservations or peak measurements. All new repository tools follow the concise
communication rules in `AGENTS.md`.

## Experimental browser qualification

Experimental Windows browser isolation has a separate, fictional-only
[qualification command and explicit limits](ISOLATED_REHEARSAL.md). It runs
headlessly, retains private source-bound evidence under `target/`, and does not
enable employer-form interaction or select an unrestricted fallback.

## Native database dependency provenance

The reviewed SQLite binding is vendored under `third_party/libsqlite3-sys/`.
Its registry archive and original file-map digests are anchored in
`tools/verify_sqlite_vendor.py`; only the normalized manifest changes package
build metadata and advances the SQLCipher source pin. CI verifies those bytes.
The original upstream license and source remain intact. The scoped license
policy exception retains the full compound license expression of the pinned
native-source package; it is not an advisory exemption.

Windows bundles collect normal and build dependencies using Cargo metadata,
including path dependencies outside the workspace and nested native notices.
The bundle license denominator therefore differs from the embedded inventory's
all-platform workspace graph. Do not classify a vendored dependency as an
internal workspace package or omit a C source provider because it is a build
dependency. Current engine-floor and unsigned-integer conversion tests run with
both plain and encrypted store configurations. The three opt-in
`db_compat_qualification` tests require an explicitly selected private directory
inside ignored `target/`; preserve the old test executable before upgrading if
checking backward readability. Those fixtures are fictional and are not user
workspace migrations or a general compatibility guarantee.


## Source-copy workflow qualification

`tools/qualify_source_copy.py` exercises the compiled local web app headlessly
with a disposable control folder and fictional job record. It copies only
explicit test-owned managed browser/PDF runtimes from `target/`, reads one public
technical reference through the native coordinator, and verifies rendered
cancellation, archive records, reload recovery and nine bounded width/zoom cases.
It uses no personal browser profile or employer form. Private evidence stays in
`target/source-copy-ui-*/`; a passing process exit alone does not establish visual
quality or resource qualification. The shared qualification runner checks host
RAM every 500 ms and closes only its own containment below a 3 GiB reserve.
See [scope, evidence and remaining gates](SOURCE_COPY_WORKFLOW.md).

## Document workflow qualification

`tools/qualify_document_journey.py --control <test-owned-folder-under-target>
--browser <absolute-Chromium-executable>` connects the real HTTP handlers,
compiled client, installed local model and explicit private Office/PDF tools.
It saves fictional profile/template inputs, drafts a CV and cover letter,
exports PDFs, checks text and local visual review, verifies archived page hashes
and refuses stale exports. It also exercises one source-citation repair with the
actual model. The development controls require Node and already-provisioned
test-owned runtimes; they do not imply host-installed product dependencies.
All interaction stays in the disposable loopback workspace; application/form
actions and email preparation/sending are refused by the native test allowlist.
Private output stays in `target/`. The shared ownership, 4 GiB job and monitored
3 GiB host reserve apply; Ollama remains outside that job. See
[scope, observations and remaining gates](DOCUMENT_PDF_JOURNEY.md).
