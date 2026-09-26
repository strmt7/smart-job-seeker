# Implementation checkpoint

Status: G0 complete. G1 code-complete & hardware-measured (a11y observation open). G2 core crates landed (T010-T013, T016-T018, T020); ATS discovery adapters + coverage dashboard UI next.

| Field | Value |
|---|---|
| Latest commit | see `git log -1` |
| Completed tasks | T001-T005, T007 (interface + live probe), T009; T006 partial; T008 measured (qwen3.5:9b @ RX 9070 XT); T010, T011, T013, T016, T017, T018, T020 |
| Validation run | `cargo fmt --all`; `cargo clippy --all-targets` -> 0 warnings; `cargo test --workspace` -> 34 tests pass |
| Runtime measurements | docs/g1/QUALIFICATION.md (13.23 GiB VRAM, zero offload, ~77 tok/s, think+structured OK) |
| Missing tests | release-build paint p95; Narrator/NVDA/IME/DPI observation; ATS fixture parses (T014 pending); coverage dashboard UI |
| Known limits | store unencrypted (SQLCipher planned, schema ready); no live source ingestion yet |
| Next smallest action | T014 Greenhouse/Lever/Ashby public JSON adapters with fixture tests; T019 coverage dashboard |
