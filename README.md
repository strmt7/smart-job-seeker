# smart-job-seeker (Waypoint)

An enterprise-grade, fully free and open-source **job-seeker platform for Windows written in Rust** — native desktop UI, local-first data, local AI reasoning, and an application engine designed around correctness under failure rather than feature-count marketing.

**Status: research-complete, core built through gate G6.** See `IMPLEMENTATION_STATUS.md` and `KNOWN_LIMITATIONS.md` for exactly what is verified versus pending. No hiring-uplift, superiority or "undetectable AI" claims are made.

## Quick start

Requires: Rust stable (MSVC toolchain). No Node.js, Python, WSL or Docker anywhere in the end-user stack.

```
cargo test                 # 110 tests
cargo run -p windows_app   # native eframe/egui shell
```

## Workspace layout

| Crate | Role |
|---|---|
| `crates/waypoint_domain` | Claims, amounts, application-state machine, one-use approval grants |
| `crates/waypoint_policy` | Source permission registry (per-operation: discover/read/prefill/upload/submit/confirm) |
| `crates/waypoint_store` | SQLite store, transactional migrations, backup/restore, reversible alias graph |
| `crates/waypoint_search` | Hard constraints (unknown != pass), identity/dedup, freshness, lexical matching, coverage reports |
| `crates/waypoint_connectors` | Greenhouse/Lever/Ashby public ATS parsers + SSRF-guarded discovery plans |
| `crates/waypoint_research` | Bounded research runs (query/fetch/verification budgets; PII-in-query refused) |
| `crates/waypoint_inference` | Inference trait, capability probe, resource scheduler (13-budget defaults) |
| `crates/waypoint_materials` | Semantic document tree, evidence validation, **correction propagation**, DOCX/Typst export, answer library |
| `crates/waypoint_applications` | Preflight, one-use grants, durable journal, receipt qualification, uncertain-state recovery, typed form mapping, manual fallback |
| `crates/waypoint_lifecycle` | Consent-scoped inbox, timeline reconciliation, calendar checks, CRM, interview practice, offers, analytics, review sharing |
| `crates/waypoint_desktop` | Native eframe/egui shell: virtualized lists, coverage and model-qualification screens |
| `apps/windows_app` | Windows entrypoint |

## Product principles (enforced in code, not prose)

1. **Corrections propagate.** Every artifact sentence cites claims; correcting a fact identifies every affected document, answer and pending application — and never rewrites an already-submitted packet.
2. **No misleading application states.** Prepared is not submitted; a click is not confirmed delivery; a timeout is Uncertain, not failure; automatic retry after an ambiguous external write is structurally forbidden.
3. **Unknown is not pass.** Salary, sponsorship and language constraints fail or stay unknown explicitly; empty result sets never silently weaken constraints.
4. **A model cannot authorize anything.** Only explicit user gestures create grants; grants are one-use and hash-bound to packet + form schema + destination; sensitive form fields require the user's own fresh answers.
5. **Honest coverage.** The app reports which sources it searched, which failed, and what is stale — with denominators.
6. **Local and private.** Local model + local store by default; telemetry does not exist; queries carrying candidate PII are refused at the policy layer.

## Quality gates recorded on real hardware

- qwen3.5:9b on AMD RX 9070 XT (16 GB): 13.23 GiB VRAM, zero CPU offload, ~77 tok/s, structured output + reasoning pass — `docs/g1/QUALIFICATION.md`.
- 110 tests green, 0 clippy warnings, `cargo audit`: 0 vulnerabilities, `cargo deny`: licenses/bans/sources clean — `docs/g6/RELEASE_READINESS.md`, SBOM at `docs/g6/sbom.json` (750 components).

## Documentation

- Research/design handoff: `docs/waypoint-handoff/` (competitive atlas, Rust blueprint, 40-screen UI spec, schemas, fixtures).
- Gate evidence: `docs/g0/`, `docs/g1/`, `docs/g6/`.
- Honest limits: `KNOWN_LIMITATIONS.md`.

## License

MIT. Dependencies: permissive licenses only (enforced by `cargo deny`; see `deny.toml`). The research handoff package retains its own attribution and reuse terms under `docs/waypoint-handoff/`.
