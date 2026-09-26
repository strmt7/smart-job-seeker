# G6 — Release readiness (T044–T050)

Date: 2026-09-25. All statements below are backed by the recorded commands and outputs in this file's history and in CI-configurable scripts.

## T044 — Windows release packaging
- `cargo build --release -p windows_app` **succeeds**: `target/release/windows_app.exe`, PE32+ console subsystem, x86-64, **14,318,080 bytes** (14.3 MB) — no Node/Python/WSL/Electron/Docker runtime required by the binary.
- Static analysis of the dependency graph confirms a pure-Rust native stack (eframe/egui/wgpu + bundled SQLite via rusqlite).
- **NOT yet done (honest gaps):** code signing certificate, MSIX/NSIS installer, icon/versioning resources. Signing requires a certificate decision (self-signed vs purchased) — recorded as an explicit release-blocker, not silently skipped.

## T045 — Update and rollback
- The store layer already snapshots `migration_backups` (row counts + timestamp) before every migration step, and `VACUUM INTO` backup/restore is tested (`backup_produces_a_restorable_copy`).
- Failed-migration policy per MASTER_PLAN §ADR-002: leave the prior store intact, enter recovery mode. Restore is exercised from a clean backup file in tests.
- **NOT yet done:** automatic update channel with signed metadata (blocked on the signing cert from T044).

## T046 — Dependency and supply-chain audit (executed)
- `cargo audit`: **0 vulnerabilities** (count: 0, list: [] as of 2026-09-25, advisory DB with 1271 advisories). One informational *unmaintained* notice: `ttf-parser 0.25.1` (RUSTSEC-2026-0192), transitive via egui's `sctk-adwaita` (Linux-decorations path); no CVE, no safe upgrade pinned upstream. Recorded, not hidden.
- `cargo deny check`: **licenses ok, bans ok, sources ok**. Config in `deny.toml`: only permissive licenses allowed; GPL/AGPL would fail the build. The one `GPL-2.0-only` string in the inventory (`self_cell`) is dual-licensed `Apache-2.0 OR GPL-2.0-only` and is consumed under Apache-2.0 — verified in its Cargo.toml.
- egui's bundled fonts (`epaint_default_fonts`) carry OFL-1.1 and Ubuntu-font-1.0 — OSI/FSF-recognized font licenses, explicitly allow-listed with justification.
- SBOM: `docs/g6/sbom.json` — 750 components, CycloneDX-lite with per-crate licenses, generated from `cargo deny list`.

## T047 — Privacy defaults
- Telemetry: **off by default; none exists in code**. No analytics endpoints, no crash reporting, no update pings in the codebase.
- All network egress in the codebase goes through permission-registry-gated discovery plans (`waypoint_policy`) and the PII-declaring research query gate (`waypoint_research`): queries carrying `CandidatePii` are refused outright.
- Secrets: no tokens/cookies in config files; Windows Credential Manager integration specified (threat model); fixtures use fictional people only.
- Secure deletion caveat recorded: overwriting is not guaranteed on SSDs; encryption + key disposal is the specified mechanism (SQLCipher integration still pending — see limitations).

## T048 — Comparative benchmark (skeleton + measured baseline)
- Measured on this machine: qwen3.5:9b via Ollama — 13.23 GiB VRAM, zero CPU offload, ~77 tok/s, structured output + reasoning pass (docs/g1/QUALIFICATION.md).
- Benchmark corpus targets recorded per MASTER_PLAN §14 (50 profiles / 5,000 observations / 300 form variants / 200 adversarial fixtures / 100 correction scenarios) — **design targets, not collected yet**.
- The adversarial fixture suite that exists and passes today: 87+ unit tests covering hard-negative constraint cases (unknown salary, language-of-ad vs required, sponsorship history vs stated), correction propagation, receipt qualification (generic success ≠ confirmed), no-auto-retry invariants, sensitive-field answer gating, PII-in-query refusal.

## T049 — Usability pilot
- Not run. Per the plan this is opt-in human research; it requires consent protocols. Recorded as pending, not claimed.

## T050 — Release gate and limitations register
See `KNOWN_LIMITATIONS.md`. The release gate is **not passed yet**: signing, installer, a11y observation (Narrator/IME/DPI), SQLCipher, live ATS end-to-end apply (fixtures only) and the pilot remain open. What *is* verified today: 110 tests green, 0 clippy warnings, 0 audit vulnerabilities, license/bans/sources clean, native release binary builds.
