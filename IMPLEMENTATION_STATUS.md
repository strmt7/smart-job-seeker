# Implementation checkpoint

## Direct upload-form inspection checkpoint — 2026-10-29

- Form inspection and application-page checking retain a page that already has a visible file-upload control instead of following another application link. This is a hint to review the current page, not proof that it accepts applications; pages without an upload continue through the existing link-selection path.
- Four focused form-task tests and three application-check tests passed, with their other local-browser tests ignored. A separate headless browser test passed against a fictional local upload form without submitting. Web Clippy passed with one build job; real employer layouts and peak resource use were not measured.

## Search planner input checkpoint — 2026-10-29

- Local search planning now reads at most 20 eligible reviewed facts through the existing bounded sealed-data path instead of loading the entire fact history. Proposed phrases still require user review before a search.
- Plans are bound to their selected facts and instructions. The status endpoint reports stale when those inputs changed, and the UI rechecks before copying a phrase into a search field. The web and Jobicy start endpoints check an unchanged copied phrase's binding again before starting a search. Editing the phrase makes it a manually authored search; it still requires user review.
- A pending-plan endpoint and a one-result in-memory replay let the UI recover an active or completed plan after a lost response or tab refresh. They do not survive an application restart and do not launch web searches automatically.
- The encrypted-workspace test passed with more than 20 fictional facts, a corrected fact, an unreviewed fact and an oversized fact; the selected 20 matched the prior full-read ordering. Three planner/context tests, planner recovery and binding UI tests, and web Clippy passed. Local handler tests rejected stale bindings for web and Jobicy starts before launching a provider. A synthetic headless Edge fixture displayed fictional recovered phrases and rejected a stale phrase at 1200 and 375 pixels without issuing a new planning request. No real local model or public provider was exercised for these changes. Peak memory was not measured, and other paths may still load larger data sets.

## Search coverage reporting checkpoint — 2026-10-29

- Search results now distinguish an omitted engine-availability report from an explicit zero and show examined versus provider-returned result counts. The denominator is one provider response, not global market coverage.
- The latest completed single-search result or failure remains available for repeat status reads, so a lost response need not discard its candidates. A pending-task endpoint lets the web UI resume a search whose start response was lost while its task remains active or unread. The tab retains its task ID across refreshes and prioritizes a newer active task. Only one terminal result is retained in memory; recovery does not survive an app restart.
- Four web-search tests, the search coverage and single-search recovery UI tests, and web Clippy passed; the existing visual-comparison UI test also passed. A local HTTP handler test recovered a fictional pending task, and a synthetic headless Edge page with mocked responses prioritized a newer task and restored its result across two reloads without another start request. A separate synthetic fixture displayed the unavailable-status wording at 1200 and 375 pixels. These checks did not test a running app or a public search provider. Hosted CI and peak resource use were not measured.

## Source-only listing capture checkpoint — 2026-10-29

- When screenshot capture fails, listing metadata can now be sealed separately using DPAPI and retained without claiming an image was captured. A later screenshot capture keeps its own metadata, while salary evidence continues to refer to the earlier retained source. The salary detail warns when the capture URL differs from, or cannot be compared with, the current job URL. Application records include the source-only metadata separately from screenshots.
- Store tests passed 46/46; a populated v24-to-v25 migration fixture also passed with SQLCipher enabled. The web suite passed 144 tests with 37 ignored, and web Clippy passed. Fictional local handler tests exercised both matching and different capture URLs, the no-screenshot result, source retention, salary retrieval and a later separately recorded screenshot. A synthetic Playwright fixture displayed the different-URL warning at 1200 and 375 pixels. A clean-host upgrade and real-site screenshot failure were not exercised; peak resource use was not measured.

## Confirmation scan boundary checkpoint — 2026-10-29

- The bounded application-reference scan now stops at a UTF-8 boundary and refuses an identifier token cut by the scan limit. This prevents a panic and avoids accepting a partial reference as a confirmation; it does not send an application.
- Five focused driver tests and `cargo clippy -p waypoint_applications --all-targets -- -D warnings` passed with one build job. The web-app suite subsequently passed 144 tests with 37 ignored, and web-app Clippy passed. No live employer response was tested; peak resource use was not measured.

## Listing metadata access checkpoint — 2026-10-29

- Existing listing-capture metadata can be read without loading or decrypting screenshot segments; repeated listing inspection uses this path. Structured source remains in the existing encrypted capture when capture succeeds. Job details can request bounded `baseSalary` evidence from one saved structured posting and display its stated fields and capture date for manual review. This is evidence from the original capture, not a salary comparison; filter wiring, employer-salary normalization and fresh-host testing remain incomplete.
- The DPAPI capture test passed, including metadata access when a fictional screenshot segment is corrupted. The web listing-inspection suite passed 8 tests with 3 local-browser tests ignored, including captured salary and ambiguous-posting cases. A local HTTP handler test passed with a fictional encrypted capture and checked returned salary evidence and timestamp. The real display function was exercised in a synthetic headless Edge page with a mocked response at 1200 and 375 pixels; it showed the evidence, date and verification caveat without horizontal overflow or console errors. This was not a running-app end-to-end test. One Cargo build job was used; peak resource use was not measured.

## Salary comparability checkpoint — 2026-10-29

- Salary thresholds now carry optional currency, gross/net basis, period and compensation category. If these are absent or disagree with a stated job amount, the comparison is explicitly unknown; a lower bound below the threshold no longer acts as an upper bound. Invalid inverted ranges are also unknown. Older settings without the new optional fields remain readable.
- `cargo test -p waypoint_search` passed 9 tests, including a legacy-settings compatibility check, and `cargo clippy -p waypoint_search --all-targets -- -D warnings` passed with one build job. The current workspace ranking path does not yet supply parsed employer salaries, and the UI does not yet collect these new comparison attributes; this change alone does not make salary filtering fully functional. Peak resource use was not measured.
- Ranking now carries unknown constraint reasons to each included result; the web detail and desktop row expose them instead of presenting them as confirmed passes. Workspace tests passed 40 with two ignored across unit and integration suites; desktop tests passed 13, the web suite passed 142 with 37 ignored, and the web lint check passed. The web interface still requests default constraints, so the new warning path was not exercised through its settings UI.

## Windows lint helper checkpoint — 2026-10-29

- `build-windows.ps1 -Clippy` uses the same OpenSSL discovery and one-job default as build and test. Conflicting `-Test`/`-Clippy` options and a filter without `-Test` are rejected before discovery.
- On this host, with OpenSSL environment variables cleared, the helper detected the existing OpenSSL installation and linted web-app targets successfully. `test-build-paths.ps1` passed. This does not install missing dependencies or verify a clean host; resource peaks were not measured.

## Required-field review checkpoint — 2026-10-29

- Required fields mapped to empty text or an unchecked checkbox remain in the review list both when preparing a preview and after autofill. When a user changes answers, generated required-field warnings are recalculated without removing unrelated review notes. This list is not browser validation; separate attachments and subsequent page changes still require inspection. The focused form-preview suite passed 3 tests with 4 local-browser tests ignored for the preview change. The required-field test passed again after warning refresh was added. A headless browser test passed against a fictional local form, checking offline suggestions and an unmapped required field. A separate local browser endpoint test passed after checking that filled fields lose stale warnings and unrelated warnings remain. These tests did not exercise the empty-value case in the browser or submit.
- With one Cargo build job and one test thread, the web-app suite passed 142 tests with 37 ignored. `cargo fmt --all -- --check`, the UI syntax and visual comparison checks, and the local build and package fixture checks passed. Direct `cargo clippy` without the helper's OpenSSL environment failed to find `OPENSSL_DIR`; the retry with the existing OpenSSL installation configured passed. This checkpoint did not exercise a live employer form or submit an application.

## Optional document conversion bundle — 2026-10-29

- The Windows packager can include a supplied LibreOffice distribution with `--libreoffice-dir`. It inventories the files by SHA-256, retains the distribution license and lets the installer verify the files before copying them into the existing private converter location. This is an optional packaging input, not automatic download or setup.
- `test-package-windows.py` verifies hashes and rejection of a missing license with a fictional three-file distribution. `test-install-package.ps1` copied fictional packages with and without a converter and verified that a damaged converter or absent license inventory is rejected before an installation directory is created; no executable was launched. Both scripts are registered in Windows CI but hosted execution has not been checked. On 2026-10-29 the local LibreOffice 26.2.5.2 distribution (13,930 files, 857,096,674 bytes) streamed through the bundler into a discard sink in 54.7 seconds. No archive, installer or conversion was run in that check. A full package, clean-host installation and bundled conversion remain untested. Resource peaks were not measured.
- Automatic dependency provisioning remains incomplete, including the OpenSSL development build prerequisite and automatic installation of LibreOffice, browser and Ollama. No applications were sent.

## Model inventory layout checkpoint — 2026-10-29

- The local model list has a bounded height, wraps long names and can receive keyboard focus for scrolling.
- `node --check apps/web_app/web/app.js` passed. A synthetic static fixture using the current stylesheet and 100 fictional entries was checked in headless Edge at 1200 and 375 pixels: the list remained bounded, had no horizontal overflow, and scrolled with Page Down. This did not exercise the running application or its model inventory endpoint.
- Automatic dependency provisioning and clean-host installation remain incomplete. The package includes OpenSSL and Python; the development build still requires an OpenSSL development installation, and browser, document conversion and model setup are not automatically provisioned. Peak resource use was not measured. No applications were sent.

## Visual comparison checkpoint — 2026-10-29

- Current revision: see `git log -1`. Visual comparison labels do not report a match when any capture changed or could not be checked for freshness; duplicate and missing observations remain unverified.
- With `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`, the eight `visual_review::tests` passed and `cargo clippy -p waypoint_web --all-targets -- -D warnings` passed. `node apps/web_app/tests/visual-review-ui.test.js` and `apps/web_app/scripts/test-build-paths.ps1` passed. The Windows CI workflow now runs both checks.
- A real vision-model review, rendered-browser verification, hosted CI, the full workspace suite and peak resource use were not measured for this checkpoint. No applications were sent. Automatic Windows dependency installation remains incomplete.

## Local model inventory checkpoint — 2026-10-29

- Revision: `2202d6c`. Settings can list up to 100 installed Ollama models and their reported disk sizes. Disk size is not a GPU memory or readiness measurement; the app does not install or load models.
- Validation with `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`: `build-windows.ps1 -Test` passed 141 tests with 37 ignored. `cargo test -p waypoint_web model_inventory_http_reads_local_ollama -- --ignored --test-threads=1` passed against the existing local service without loading a model. `node --check apps/web_app/web/app.js` passed.
- The full workspace suite, a clean-host installation and peak resource usage were not measured. No applications were sent. Automatic dependency and model provisioning remain incomplete.

## Local web prototype checkpoint — 2026-10-29

- Revision: `2f0cc0c`.
- The local fit assessment chooses up to 20 reviewed qualifications from the first 500 eligible facts using listing text overlap. The bounded fact read shares the full-history unsealing implementation. A fictional encrypted-workspace test checks corrected, unreviewed and oversized facts.
- `build-windows.ps1` keeps its OpenSSL search paths stable across repeated invocations. Automatic development dependency provisioning remains incomplete; the packaged runtime includes OpenSSL and Python but other tools still need setup.
- Validation on Windows with `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`: `build-windows.ps1 -Test` passed 140 tests with 36 ignored; `cargo test -p waypoint_store` passed 45; `cargo test -p waypoint_workspace` passed with one live journey ignored; `test-build-paths.ps1` and `node --check apps/web_app/web/app.js` passed.
- The full workspace suite, live model, third-party employer workflows, fresh-host installation and resource peaks were not measured for this checkpoint. No applications were sent.
- Next: complete automatic, verifiable Windows dependency provisioning and validate it on a clean host; continue end-to-end local-fixture application preparation and visual review.

The table below records a historical checkpoint, not current release evidence.

Status: G0-G5 complete; G6 release-readiness executed (audit/SBOM/packaging checks); remaining release blockers recorded honestly.

| Field | Value |
|---|---|
| Latest commit | see `git log -1` |
| Work layer | facts → packet → one-use approval → DOCX export, with correction propagation; verified live end to end (152 real postings → shortlist → approve → export → correct → approval voided) |
| Submission stage | engine behind a `BrowserDriver` trait (preflight at the boundary, durable consumption, typed fills, qualified receipts, append-only journal, no auto-retry) **plus a real CDP driver** (`waypoint_browser`) — verified end to end against a real browser on local fixtures, including a change-detected form being refused and the approval voided. Not yet exercised on a live third-party site |
| Completed tasks | T001-T020, T021-T026, T027-T034, T035-T043 (all gates' engineering tasks); T044 (binary builds, unsigned), T045 (migration rollback tested; update channel pending), T046 (audit+deny+SBOM executed), T047 (defaults verified in code), T048 (skeleton + measured model baseline), T050 (limitations register) |
| Partial | T006 (keyboard done; Narrator/IME/DPI unobserved), T007 (trait+live probe; production adapter unwired), T049 (pilot not run by design) |
| Validation | `cargo fmt`; `cargo clippy --all-targets` -> 0 warnings; `cargo test --workspace` -> 183 pass (+5 real-browser tests, run in CI); `cargo audit --deny warnings` -> clean (yanked `yoke-derive` replaced by 0.8.4); live network + live model probes pass; `cargo deny check` -> licenses/bans/sources ok; release binary PE32+ 14.3 MB |
| Known blockers | signing cert + installer; SQLCipher; production inference adapter; browser CDP wiring; a11y observation session |
| Next smallest action | Wire `OllamaInference` into the drafting/interview flows, then SQLCipher integration |
