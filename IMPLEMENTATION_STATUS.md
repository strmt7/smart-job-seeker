# Implementation checkpoint

## Search-plan identifier check — 2026-10-29

- Locally generated search phrases are checked against normalized known profile identifiers, including the full name, email, address, links and phone number. Unsafe phrases are omitted with a visible count; if none remain, planning fails. The identifier list is not added separately to the model prompt, although user-provided qualifications or instructions may contain identifying text. This is not an exhaustive PII detector, and users are still asked to review phrases before searching.
- The plan digest includes the identifier list so a profile change invalidates old suggestions. Three focused planner/context tests, the planner-recovery UI test and web Clippy passed. A synthetic Edge fixture displayed the omission count and review reminder at 1200 and 375 pixels without issuing a planning request. The local Ollama inventory reported zero resident models and NVIDIA GPU telemetry was unavailable; no model was loaded and no live model or public provider was exercised. Peak memory was not measured.

## Visual label comparison checkpoint — 2026-10-29

- Visual field comparison now treats ASCII line breaks, repeated spacing and ASCII letter case consistently in both the backend and web UI. Duplicate normalized labels remain unobserved, and values must still match exactly. Non-ASCII formatting is not silently normalized.
- Eight focused visual-review tests, the UI comparison test and web Clippy passed with one build job. The model comparison was tested with fictional observations, not a live vision model or employer form; peak resource use was not measured.

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

## Planner validation checkpoint — 2026-10-29

- A generated plan retains usable phrases when individual suggestions are invalid or contain known personal identifiers. The interface reports omitted counts and still requires review; the identifier check is not exhaustive. Plans remain bounded to ten suggestions.
- With one build job and one test thread, three search-plan tests, eight visual-review tests and web-app Clippy passed. The search-plan recovery, binding and visual-review JavaScript checks passed. No model was loaded or application sent. A real model, installed-app startup, third-party applications and peak resource use were not measured for this checkpoint.

## Longer form visual comparisons — 2026-10-29

- Visual review compares up to 200 supported filled fields across captured segments, increased from 20. Further fields are explicitly reported as omitted. Each model response remains limited to 20 observations per segment, and review remains advisory; hidden content may be missed.
- With one build job and one test thread, nine visual-review tests passed (one unrelated PDF test ignored), web-app Clippy passed, and the visual comparison UI check passed. No live vision model or employer site was tested; peak resource use was not measured. No application was sent.

## Dense visual segments — 2026-10-29

- When a segment returns the maximum 20 model observations, the application is marked for manual review and the normal and recovered review flows warn that additional fields may have been missed. The saved visual result includes the limit indicator. Fewer observations also do not prove completeness.
- With one build job and one test thread, ten visual-review tests passed (one unrelated PDF test ignored), web-app Clippy passed, and the visual comparison UI check passed. No live model, real employer site or resource peaks were measured. No application was sent.

## Adaptive dense-segment review — 2026-10-29

- A visual segment reaching the 20-observation limit is rechecked sequentially in two non-overlapping halves; at most 18 captures are attempted for six segments. The job remains flagged for manual review because fields crossing the split or otherwise hidden can be missed. Saved captures identify their half and freshness recaptures use the same geometry.
- With one build job and one test thread, eleven visual-review tests passed (one unrelated PDF test ignored), web-app Clippy passed, and visual comparison and adaptive-retry UI fixture checks passed. A live model, employer site, full application startup and resource peaks were not measured. No application was sent.

## Conflicting form mappings — 2026-10-29

- When a model suggestion conflicts with an available exact-label or autocomplete suggestion, neither value is proposed for that field. Other mappings remain available; required fields are flagged for manual review. Matching values still require user review and do not establish correctness.
- With one build job and one test thread, seven suggestion tests, three non-browser form-preview tests, and two targeted conflict tests passed; four browser tests were ignored. Web-app Clippy passed before the final test-only addition. No live model, employer site or resource peaks were measured. No application was sent.

## Partial model form mappings — 2026-10-29

- Invalid, unknown or restricted individual model mappings no longer discard unrelated valid mappings. Duplicate or invalid known selectors remain unmapped, including from exact fallback; required unmapped fields remain review obligations. Model results and exact hints still need user review.
- With one build job and one test thread, eight suggestion tests and four non-browser form-preview tests passed (four browser tests ignored); web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Form mapping explanations — 2026-10-29

- The form preview explains which fields had conflicting, invalid or duplicate model suggestions and flags unidentifiable entries without exposing profile values. Field-specific warnings clear when all fields sharing the label receive reviewed mappings.
- With one build job and one test thread, eight suggestion tests and four non-browser form-preview tests passed (four browser tests ignored); web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Bounded mapping for longer forms — 2026-10-29

- Forms up to 200 fields can be mapped in sequential batches of at most 40 eligible fields with a 24,000-byte prompt limit. Batch results are validated against their own fields; failed batches are reported and retain exact-hint fallback. Larger forms remain available for manual review. Option interpretation is separately bounded.
- With one build job and one test thread, nine suggestion tests, four non-browser preview tests and the added failed-batch fixture passed; four browser tests were ignored. Web-app Clippy passed before the final test-only addition. No live model, employer site or resource peaks were measured. No application was sent.

## Bounded option interpretation — 2026-10-29

- Non-exact dropdown and radio options are interpreted in up to five sequential batches of ten fields, capped at 12,000 bytes of field data per batch. Unresolved choices are reported for manual review; duplicate selections for one field are discarded. Exact matches remain available beyond the batch limit.
- With one build job and one test thread, twelve suggestion tests and web-app Clippy passed. No live model, employer site or peak resources were measured. No application was sent.

## Long-form reviewed fill — 2026-10-29

- Reviewed fill accepts up to 200 distinct fields, matching the mapping limit. Its request body is separately capped at 1 MiB; other normal request limits remain unchanged. Larger answers can still exceed this byte limit and need manual handling. Oversized-form guidance recognizes the current mapping-limit message.
- With one build job and one test thread, ten reviewed-path tests passed (four ignored), including a 200/201-field boundary test; the ignored local-browser prefill fixture was run separately and passed without submitting. Web-app Clippy and JavaScript syntax checks passed. No live model, employer site or resource peaks were measured.

## Application link inspection failures — 2026-10-29

- Initial inspection and subsequent navigation now record failures to read likely or other page links as manual-review reasons instead of treating errors as empty results. The current job and saved inspection evidence carry the review state; navigation and submission behavior are unchanged.
- With one build job and one test thread, five link-navigation tests and web-app Clippy passed. No live employer site or resource peaks were measured; no application was sent.

## PDF export response recovery — 2026-10-29

- The most recent completed or failed reviewed PDF export can be polled again after a lost response without creating a second document. This replay is in memory, limited to the latest result and does not survive process restart.
- With one build job and one test thread, four letter-export tests passed (one local conversion test ignored); web-app Clippy passed before the final test-lock addition. No live model, employer site, installed-app startup or peak resources were measured. No application was sent.

## Pending PDF export recovery — 2026-10-29

- The local UI can discover an active reviewed PDF export when the start response is lost and recover the result without launching another export. The active task takes precedence over an older tab task. Recovery remains limited to the running process and its most recent completed result.
- With one build job and one test thread, four letter-export tests passed (one conversion test ignored), the recovery UI fixture passed, and web-app Clippy and JavaScript syntax checks passed. No live model, installed-app startup, employer site or resource peaks were measured. No application was sent.

## Job-linked template edit freshness — 2026-10-29

- A job-linked template conversion now checks a digest of its job-fit context and document instructions before saving; changed context stops the save. The fit context uses bounded, selected qualifications, so this does not prove that every possible candidate change is detected. Edits without a job retain their existing source-file check.
- With one build job and one test thread, the context-change fixture and existing template-record test passed; four conversion fixtures were ignored. Web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Template edit correction tracking — 2026-10-29

- Job-linked template conversion freshness now includes the workspace correction revision and the full stored job record and description, in addition to its bounded fit context and document instructions. A correction outside the selected fit facts changes the context. Adding a new fact does not itself advance the correction revision; the fit selection remains bounded.
- With one build job and one test thread, three template-edit tests passed (four conversion fixtures ignored), and web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

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
