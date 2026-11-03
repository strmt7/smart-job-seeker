# AGENTS.md — how to work in this repo (for AI coding agents and humans)

Waypoint (smart-job-seeker) is a Windows-first, local-first job-seeker prototype
with a Rust backend and local web interface. This file is the single entry point for any agent (or person) picking the
repo up cold. Read this first, then `README.md`, then the references below.

## Agent efficiency and research

- Apply [caveman](.agents/skills/caveman/SKILL.md) on every task. Keep internal
  work bounded while preserving exact evidence and normal public communication.
- Before broad or conceptual code navigation, use the source-bound
  [CocoIndex workflow](.agents/skills/cocoindex-code-search/SKILL.md). Exact
  names and small known scopes still use `rg`; confirm semantic results in live
  source. Never change another checkout's registration or cache.
- Use [Crawl4AI](.agents/skills/crawl4ai-research/SKILL.md) for web-page
  reading, retain native results and disclose failures. Search snippets are not
  page evidence. API and archive bytes retain their native transports. Follow
  product privacy, source-permission and robots rules independently.
- See [agent tools and coverage](docs/agent-tools.md) for explicit setup,
  external caches, receipt limits and the scope of the Codecov report. These
  are development tools, not application runtime dependencies.

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
  - `waypoint_browser` — the real `BrowserDriver`: Chrome DevTools Protocol over a
    throwaway browser profile. Reads the form from the live DOM, types with real
    input events, attaches files, clicks by box model, and uses bounded CDP
    inspection. Captchas and login
    walls are reported as walls, never climbed.
  - `waypoint_seal` — sealing candidate material at rest. Windows DPAPI through a
    hand-written three-call FFI (the workspace's only `unsafe`, scoped to that
    module with a justification comment). Values carry a self-describing envelope
    (`magic | version | backend | payload`) so a value sealed by one backend is
    refused by another instead of being misread.
  - `waypoint_store` — versioned migrations (v3 adds `claims`, `packets`, `grants`;
    v4 adds the submission journal). **Never edit a released migration; add v(N+1).**
    Grants persist their `consumed` flag so a restart cannot resurrect a spent
    approval, and the journal is append-only *enforced by database triggers* — an
    UPDATE or DELETE on it fails at the SQLite level.
- `deny.toml` — license/supply-chain policy; `cargo deny check` must stay green.
- CI (`.github/workflows/ci.yml`) runs fmt, clippy `-D warnings`, workspace tests,
  release build, `cargo audit` (with one documented informational ignore), deny checks.

## Build, test, quality commands

```bash
cargo test --workspace                 # ignored live tests require explicit selection
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --release -p windows_app   # native PE32+ binary, ~14 MB
cargo deny check licenses bans sources
cargo audit --deny warnings --ignore RUSTSEC-2026-0192   # documented unmaintained notice
# real-browser submission tests (Edge/Chrome required; CI runs these too):
cargo test -p waypoint_browser --test cdp_live -- --ignored --test-threads=1
#   set WAYPOINT_BROWSER_NO_SANDBOX=1 when the Chromium sandbox is unavailable (containers)
# live model qualification (needs local Ollama + qwen3.5:9b on 127.0.0.1:11434):
cargo test -p waypoint_inference --lib -- --ignored
# live ATS board fetch tests (network):
cargo test -p waypoint_connectors -- --ignored
```

Windows/MSVC is the primary toolchain. The web interface uses JavaScript; document
operations may use the configured local Python worker. Python and Node are also
used by development tools and tests. Do not add WSL or Docker as product requirements.

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

- After interrupted browser or fixture tests, check for orphaned Waypoint-owned
  browser profiles and fixture servers before concluding that host memory is
  unavailable. Verify each process's identity and that its test parent is gone
  before stopping it; never close another active workload. Browser `Drop` cleanup
  does not run if its parent process is forcibly terminated. Record what was
  actually measured rather than assuming all memory pressure is due to the app.
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
- Application references must keep their original casing: `extract_application_ref`
  matches on an ASCII-lowercased copy but slices the *original* text. Slicing the
  lowercased text silently downcases identifiers the candidate has to quote back.
- The upload directory must contain the file name the fill plan asks for
  (`waypoint-packet.docx`); the driver resolves names against configured folders and
  reports a missing file rather than submitting without it.
- Candidate material is sealed, public data is not. Everything the candidate
  writes (claim text, packet body/doc_json) goes through `Store::seal` when a
  sealer is attached and its plaintext column is written empty; postings and
  journals stay plaintext so search and reporting keep working with no key.
  There is exactly **one** read path per table (`all_claims`,
  `row_to_packet_full`) so a sealed column cannot be missed through a side door —
  a new query that reads `text`, `body` or `doc_json` directly reintroduces the
  bug this design prevents (it happened once while building it).
- After sealing existing rows the store VACUUMs: SQLite keeps freed page contents
  until it rewrites the file, and "the plaintext is gone" must be true of the file
  on disk, not just of a query result.
- `unsafe_code` is `deny` workspace-wide rather than `forbid`, so exactly one
  module can carry a reviewed `#[allow(unsafe_code)]`. Any unsafe code outside
  `waypoint_seal`'s DPAPI module fails the build, which is the intent.
- `validate_claim_support` reports *positive* support (`Supported`) as well as problems, and
  a citation that no longer resolves is reported as unsupported. Do not "optimise" it back
  to problems-only: the approval gate counts supported spans, and a dangling citation must
  never read as supported.
- Entity-escaping text through write/patch tooling corrupts `&`-style strings;
  write such code via line-index scripts, not string-replace through the tool.

## Current state and next work

The 2026-09-26 gate results are historical, not evidence of the current checkout.
Read `IMPLEMENTATION_STATUS.md`, `docs/WEB_PROTOTYPE_CHECKPOINT.md` and current
source before selecting work. Preserve explicit unknowns and report measured
results separately from targets. Never submit a real application without explicit
authorization for that application; when instructed to stop before submission,
all live-site work must respect that boundary.
The product is a prototype; realistic employer-site behavior, accessibility,
installation and local-model quality need further verification.
