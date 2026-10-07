# AGENTS.md — how to work in this repo (for AI coding agents and humans)

Waypoint (smart-job-seeker) is a native-Rust, Windows-first, local-first job-seeker
platform. This file is the single entry point for any agent (or person) picking the
repo up cold. Read this first, then `README.md`, then the references below.

## Non-negotiable product invariants (enforced by tests — do not break)

1. **Unknown never silently passes a constraint.** Salary/sponsorship/language constraints
   either pass, fail, or stay explicitly `Unknown`. An empty result set must never weaken
   the user's stated constraints. See `crates/waypoint_search/src/lib.rs` and its tests.
2. **No misleading application states.** `Prepared → Approved → Submitting →
   Confirmed | Uncertain` — a click is never "confirmed delivery", and automatic retry
   after an ambiguous external write is structurally forbidden
   (`crates/waypoint_applications/src/journal.rs`, `recovery.rs`).
3. **Corrections propagate.** A corrected candidate fact must identify every affected
   document/answer/pending application, invalidate stale approvals, and leave submitted
   packets immutable (`crates/waypoint_materials/src/propagation.rs`).
4. **The model can never authorize.** Only explicit user gestures create one-use,
   hash-bound `ApprovalGrant`s; sensitive form answers are never auto-reused
   (`crates/waypoint_domain` grants; `waypoint_applications/src/form_mapping.rs`).
5. **Privacy by construction.** Inference dials only a literal `http://127.0.0.1:<port>`
   (see `waypoint_inference/src/ollama.rs` — non-loopback origins are refused at the
   type level). Research queries carrying PII are refused
   (`waypoint_research`); source operations are permission-registry-gated
   (`waypoint_policy` + `data/source_permissions.json`).
6. **Honest accounting everywhere.** Coverage reports carry explicit denominators;
   failures are surfaced as degraded states, never swallowed; docs record measured
   results vs. design targets — never fake a measurement (see the pitfalls below).

## Map of the repo

- `docs/waypoint-handoff/` — the source-of-truth research/design handoff this build
  follows (MASTER_PLAN.md, backlog.json with the 50 gated tasks T001–T050,
  AGENTS.md handoff rules, JSON schemas, fixtures). **This package has priority over
  any older in-repo plan.**
- `docs/g0/`, `docs/g1/`, `docs/g6/` — gate evidence: license/permission inventories,
  hardware qualification (real measured numbers), release audit, SBOM.
- `IMPLEMENTATION_STATUS.md`, `KNOWN_LIMITATIONS.md` — what is done vs honestly pending.
- `crates/` — 13 crates, one job each (see README table). `apps/windows_app` is the binary.
  - `waypoint_net` — the ONLY HTTP client: HTTPS-only for public hosts, OS-native TLS
    (schannel), resolves-then-validates every address (DNS-rebinding guard), size-capped.
  - `waypoint_workspace` — the end-to-end pipeline (permission gate → bounded fetch →
    parse → identity/dedup → store → ranked search) plus `work.rs`: facts → prepared
    packet → one-use approval → DOCX export, with correction propagation. Degradation
    is reported, never silently converted to "0 jobs". This is where the honesty
    rules become behaviour.
  - `waypoint_store` — versioned migrations (v3 adds `claims`, `packets`, `grants`).
    **Never edit a released migration; add v(N+1).** Grants persist their `consumed`
    flag so a restart cannot resurrect a spent approval.
- `deny.toml` — license/supply-chain policy; `cargo deny check` must stay green.
- CI (`.github/workflows/ci.yml`) enforces: fmt, clippy `-D warnings`, 168 tests,
  release build, `cargo audit` (with one documented informational ignore), deny checks.

## Build, test, quality commands

```bash
cargo test --workspace                 # 168 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --release -p windows_app   # native PE32+ binary, ~14 MB
cargo deny check licenses bans sources
cargo audit --deny warnings --ignore RUSTSEC-2026-0192   # documented unmaintained notice
# live model qualification (needs local Ollama + qwen3.5:9b on 127.0.0.1:11434):
cargo test -p waypoint_inference --lib -- --ignored
# live ATS board fetch tests (network):
cargo test -p waypoint_connectors -- --ignored
```

Windows/MSVC toolchain assumed; no Node/Python/WSL/Docker anywhere in the product stack.

## Ground rules for agents (from the handoff, still binding)

- Respect site terms/robots and the permission registry before any external fetch.
- Never store credentials/secrets in code or fixtures. Fictional fixtures only.
- Keep loops bounded (drafting ≤2 repairs, research has explicit budgets/stopping).
- Cheap deterministic filtering before any model call.
- Every state-changing feature ships with adversarial tests (the existing 115 are the
  floor, not the ceiling). `cargo test` must stay green; CI runs on every push to main.
- Measured facts go in `docs/g*/` with method + date; never convert a design target
  into a claimed result. If you can't measure it, say so in `KNOWN_LIMITATIONS.md`.

## Real pitfalls already hit (don't rediscover these)

- **Ollama `think:true` + strict JSON/schema format silently returns an empty
  `response`** while filling `thinking` (done_reason=length): structured generation
  in `ollama.rs` therefore pins `think:false`. Reasoning probes check the separate
  `thinking` field and verify a multi-step answer instead of requiring JSON.
- Tiny `num_predict` under `think:true` yields an empty final answer → fake
  "reasoning unavailable". Probe budgets are 768+ tokens.
- Greenhouse/Ashby wrap payloads in `{"jobs": [...]}`; Lever uses a bare list; one
  board (netflix) 404s — live tests use vercel/ashby/spotify.
- Zero/negative timestamps mean "unrecorded observation" (Unknown), never Fresh.
- MSYS/Git-Bash path quirks: pass `C:/...` forward-slash paths to native tools.
- Adapters expect the providers' real camelCase payloads (Lever `hostedUrl`/`workplaceType`,
  Ashby `jobUrl`/`isRemote`/`publishedAt`). Fixtures written in snake_case silently default
  to empty strings and get dropped as untrusted URLs — mirror real payloads.
- The shell has exactly ONE frame path (`WaypointShell::frame`: poll → render → dispatch).
  Tests must render through it, or controller results never arrive.
- `Cargo.lock` moves with the graph: adding a dep re-resolved `url`→`idna`→ICU and pulled a
  yanked `yoke-derive 0.8.3`. Always re-run `cargo audit --deny warnings` after dependency
  changes; fix yanks by moving versions (`cargo update -p <crate> --precise X`), never by
  silencing the check.
- `validate_claim_support` reports *positive* support (`Supported`) as well as problems, and
  a citation that no longer resolves is reported as unsupported. Do not "optimise" it back
  to problems-only: the approval gate counts supported spans, and a dangling citation must
  never read as supported.
- Entity-escaping text through write/patch tooling corrupts `&`-style strings;
  write such code via line-index scripts, not string-replace through the tool.

## Honest current state (2026-09-26)

All 50 backlog tasks through gate G6 are implemented, tested (115 green), audited
(0 vulnerabilities; licenses/bans/sources clean), and CI is green on GitHub.
Deliberately open (see KNOWN_LIMITATIONS.md): code signing + installer, SQLCipher
encryption-at-rest, browser CDP wiring, interactive a11y observation, usability pilot.
None of these are silently skippable, and none are claimed done.

## Where to start next (highest value, in order)

1. The submission stage: a browser engine that consumes an approval through
   `Workspace::consume_grant` (CDP behind the typed `BrowserCommand` surface). Today an
   approval can be created, exported and voided, but nothing can spend it for real.
2. Wire `OllamaInference` into the drafting/interview flows so materials are generated
   through the real adapter (keeping a deterministic offline fallback).
3. SQLCipher at-rest encryption behind the existing store API.
4. Accessibility observation session (Narrator/IME/DPI) to close T006.
5. Expand the adversarial fixture corpus toward the §14 benchmark targets.
