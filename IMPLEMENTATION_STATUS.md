# Implementation checkpoint

## Local model inventory checkpoint — 2026-10-29

- Current revision: see `git log -1`. Settings can list up to 100 installed Ollama models and their reported disk sizes. Disk size is not a GPU memory or readiness measurement; the app does not install or load models.
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
