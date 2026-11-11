# AGENTS.md — how to work in this repo (for AI coding agents and humans)

Smart Jobseeker (smart-job-seeker) is a Windows-first, local-first job-seeker prototype
with a Rust backend and local web interface. This file is the single entry point for any agent (or person) picking the
repo up cold. Read this first, then `README.md`, then the references below.

## Agent efficiency and research

- Keep user-facing responses and progress updates brief, direct and useful.
  Routine use of any current or future repository tool, workflow, protocol or
  skill does not need narration. This includes Crawl4AI, CocoIndex Code and
  caveman; do not spend tokens announcing, listing or explaining internal tools
  unless the user asks, a material finding or blocker needs that context, or a
  higher-priority instruction requires disclosure. Preserve required progress
  updates, meaningful failures, uncertainty and permission boundaries.
- Be efficient in execution as well as communication: prioritize work that
  advances the requested product, inspect bounded relevant evidence, reuse
  verified results and run checks appropriate to the actual changes. Avoid
  repetitive plans, status recaps, unchanged tests and incidental exploration.
  Brevity must not replace implementation, verification or honest reporting.
- Keep context economical: request small source ranges, structured summaries and
  bounded log tails. Do not dump minified assets, whole reports or unchanged files.
  Retain full evidence privately and reopen only the relevant failure or symbol.
  Run focused regressions first, then the required integration checks once per
  coherent change; repeat only for changed behavior, new failures or unresolved
  concerns. Reuse exact-commit evidence with its scope. Prefer existing shared
  mechanisms over parallel implementations. Never weaken reasoning, criteria,
  assertions, coverage, privacy or cleanup to save tokens.
- Use progressive evidence gathering: start with the changed paths and the
  question to resolve, retrieve a few relevant symbols, then read their callers
  or contracts only as needed. Before each large tool call, choose the fields,
  line range and output budget that can answer that question. If output is
  truncated, narrow the query; do not repeatedly request the same oversized dump.
  Keep full native reports privately and return a compact result, artifact path,
  exit status and live handle when work is still running.
- Batch independent reads/checks in one call and inspect every result; keep edits,
  dependent operations and resource-heavy work sequential where required. Reuse
  an existing running handle instead of restarting work after an observation
  timeout. Do not add a new abstraction, helper, dependency, web search or agent
  merely to save conversational tokens when an existing mechanism suffices.
- Carry forward decisions and unresolved failures in a short handoff containing
  current source identity, affected paths, measured results, remaining work and
  owned resources. Consult that evidence instead of reconstructing the whole
  history. Reuse checks only when their relevant source, dependencies, inputs and
  environment are unchanged; label older evidence and narrower scopes honestly.
  Token savings must come from irrelevant work and repeated context, never from
  reducing model reasoning, weakening review or silently lowering test criteria.
- Prefer substantial local development batches and reviewable local commits.
  Push tested, coherent checkpoints to `main` less frequently; avoid separate
  pushes for routine status/documentation updates or every small edit unless
  the user requests them. Do not publish a release.
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

## Development process and host ownership

- Track every process tree, browser/profile, temporary server, worker, model load,
  timer and background task started for development. Record PID, creation time,
  executable and parent/containment identity before treating a resource as owned.
- Prefer task-owned Windows jobs with kill-on-close and aggregate memory limits.
  Bound runtime, output, concurrency and memory; monitor host headroom during
  expensive work. Do not leave unbounded workers, watchers, queues or caches in
  memory. Clear timers/listeners and release handles, pipes and temporary profiles.
- Cleanup must run on success, error, cancellation and timeout. Await actual
  process-tree exit rather than treating a kill request as completed cleanup.
  Verify teardown at coherent checkpoints and after interrupted tests. Preserve
  partial evidence without leaving the worker running.
- Do not create persistent services, scheduled tasks, startup entries or other
  autostart mechanisms for ad-hoc development/testing. Deliberate installed-product
  features require their own documented lifecycle; they are not development leftovers.
- Record pre-existing model/server state. Unload only models newly owned by the
  task and stop only owned servers. Hard-stop model tests require an owned runtime
  or a verified recovery/ownership mechanism; a shared service must not become an
  excuse to leave newly loaded models resident or interrupt another workload.
- Before declaring memory unavailable, inspect owned leftovers. Never kill by
  process name, age or high RAM alone; revalidate identity/ancestry and ownership.
  Leave unrelated applications, active user sessions and services untouched.
- Reuse or gracefully stop repository-owned development daemons when idle. Keep
  other repositories' providers/registrations intact. Report unknown ownership,
  failed cleanup or unverified leak behavior instead of claiming a clean host.
- Before finishing a development checkpoint, check for owned survivors and
  unexpected persistent registrations, and record available host RAM. Repeat
  relevant lifecycle checks after changing process ownership or cancellation.
  A single clean snapshot does not prove absence of memory leaks; distinguish
  actual teardown evidence from long-running memory behavior. Native controller
  shutdown checks are in `tools/tests/test_owned_process_lifecycle.py` and run
  in the existing agent-tool CI job without browsers, models or network access.
- Reuse `tools/audit_development_host.ps1` for a compact read-only checkpoint
  snapshot instead of rewriting host-enumeration commands. Full observations
  stay under ignored `target/`; console output is summarized. Matching resources
  still require ownership review and are never stopped by the audit itself.

## Findings remain under detector control

- Never manually close, dismiss, resolve, ignore or suppress a finding from any
  current or future detection tool. This includes security, dependency, secret,
  static-analysis and quality tools, hosted dashboards, issue mirrors and local
  finding ledgers. The rule also applies to suspected false positives,
  duplicates, accepted risks and findings that cannot currently be fixed.
- Fix the underlying code or configuration and rerun the relevant detector.
  Only detector-confirmed automatic resolution may change a finding to fixed.
  Do not hide findings by adding ignores, baselines, exclusions or rule disables,
  removing an analyzer, reducing scan scope, editing reports or changing status
  through an API. Preserve immutable historical evidence.
- Reopen earlier agent-manual closures when the provider supports reopening,
  retaining the original history. Record inaccessible providers or unsupported
  state transitions honestly; do not claim they were inspected or reopened.
  Do not undo real fixes to recreate automatically fixed findings.
- Open findings may remain visible with evidence and limitations; a zero count
  is not a substitute for product quality. The user decides whether unresolved
  findings are acceptable and whether to publish a release. Never publish a
  release autonomously.

## Interface consistency

The product name is **Smart Jobseeker**. Keep existing storage keys, protocols
and crate identifiers compatible when changing public branding. Follow the
[current interface contract](docs/design/2026-11-smart-jobseeker/README.md):
shared visual/interaction styles belong in `workspace-theme.css`, and pane
sizing/appearance behavior belongs in `layout.js`. Use standard mouse-drag grips
for resizing. Avoid duplicate mechanisms, inert settings and fabricated status
indicators. Validate relevant rendered states at narrow widths and browser zoom;
passing geometry checks alone does not replace screenshot inspection.

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
   (`waypoint_policy` + `crates/waypoint_policy/data/source_permissions.json`).
6. **Honest accounting everywhere.** Coverage reports carry explicit denominators;
   failures are surfaced as degraded states, never swallowed; docs record measured
   results vs. design targets — never fake a measurement (see the pitfalls below).

## Map of the repo

- `docs/waypoint-handoff/` — the source-of-truth research/design handoff this build
  follows (`agent/MASTER_PLAN.md`, `agent/backlog.json` with the 50 gated tasks T001–T050,
  AGENTS.md handoff rules, JSON schemas, fixtures). **This package has priority over
  any older in-repo plan.** Later user instructions for a local web interface and tray
  supersede its older native-renderer requirement.
- `docs/g0/`, `docs/g1/`, `docs/g6/` — gate evidence: license/permission inventories,
  hardware qualification (real measured numbers), release audit, SBOM.
- `IMPLEMENTATION_STATUS.md`, `KNOWN_LIMITATIONS.md` — what is done vs honestly pending.
- `crates/` — backend components (see README table). `apps/web_app` is the
  local web interface and Windows tray entrypoint. The older native GUI packages
  were retired; do not restore them as a second product interface.
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
  web/tray release build, `cargo audit --deny warnings`, and deny checks.

## Build, test, quality commands

```bash
cargo test --workspace                 # ignored live tests require explicit selection
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
./apps/web_app/scripts/build-windows.ps1 -Release # canonical Windows web/tray binary
cargo deny check licenses bans sources
cargo audit --deny warnings
# real-browser submission tests (Edge/Chrome required; CI runs these too):
cargo test -p waypoint_browser --test cdp_live -- --ignored --test-threads=1 --skip public_listing_navigation_probe --skip identified_public_navigation_probe --skip generic_click_can_trigger_scripted_form_submission_on_loopback --skip inert_private_form_structure_qualification
#   set WAYPOINT_BROWSER_NO_SANDBOX=1 when the Chromium sandbox is unavailable (containers)
# live model qualification (needs local Ollama + qwen3.5:9b on 127.0.0.1:11434):
cargo test -p waypoint_inference --lib -- --ignored
# live ATS board fetch tests (network):
cargo test -p waypoint_connectors -- --ignored
```

The browser command above matches the fixture-only CI selection. `--ignored`
alone also selects public-navigation probes, a private-input qualification and a
scripted-submit diagnostic that demonstrates an open limitation. Those require
separate inputs and evidence; never count the diagnostic as a passing no-send
check. Keep their tests and historical results intact. Use the configured
packaged headless engine as well as supported installed browsers where relevant;
process observers must bind profile identity, creation time and ancestry rather
than assume executable names. A failed run is not permission to stop unrelated
processes or delete unverified profiles.

For Windows workspace validation, prefer
`./apps/web_app/scripts/build-windows.ps1 -Workspace -Test` and
`./apps/web_app/scripts/build-windows.ps1 -Workspace -Clippy`. These resolve or
provision the same private OpenSSL SDK as the app build. Do not infer its library
subdirectory from the host or set an unverified `OPENSSL_LIB_DIR`; the helper
checks supported layouts. Set `RUST_TEST_THREADS=4` for a bounded workspace test
run; ignored live tests still require explicit selection.

Windows/MSVC is the primary toolchain. The web interface uses JavaScript; document
operations may use the configured local Python worker. Python and Node are also
used by development tools and tests. Do not add WSL or Docker as product requirements.

## Ground rules for agents (from the handoff, still binding)

- Development and testing must be headless: do not open visible browser or app
  windows, take focus, move the pointer, type on the user's desktop, or otherwise
  show automated interactions. Do not use visible-session test fixtures unless
  the user explicitly requests that interaction in a future instruction. This
  constraint applies to agent work, not to the user's own use of the product.
- Until script-triggered external writes are prevented and verified, do not
  click, type, upload, select, or otherwise interact with real application forms
  during development; even a non-submit control can trigger a submission. Use
  fictional loopback fixtures for interaction tests and public sites read-only.
- Respect site terms/robots and the permission registry before any external fetch.
- Never store credentials/secrets in code or fixtures. Fictional fixtures only.
- Keep loops bounded (drafting ≤2 repairs, research has explicit budgets/stopping).
- Cheap deterministic filtering before any model call.
- Every state-changing feature ships with adversarial tests (the existing 115 are the
  floor, not the ceiling). `cargo test` must stay green; CI runs on every push to main.
- Measured facts go in `docs/g*/` with method + date; never convert a design target
  into a claimed result. If you can't measure it, say so in `KNOWN_LIMITATIONS.md`.

## Real pitfalls already hit (don't rediscover these)

- Windows process creation metadata can remain inspectable after exit while a
  handle retains the process object. Creation time binds identity; it does not
  establish liveness. Verify owned job membership or a native process wait
  signal, drain the owned job, and inspect listener ownership separately. A
  short TCP probe timing out is not evidence of a remaining listener.
- Application-record directory names contain a 16-hex job prefix followed by
  the task identity. Bind temporary runtime ownership to the task portion.
  Exercise the compiled route as well as direct renderer fixtures when
  changing these contracts.
- When changing `app.js` startup or introducing a script global, run all
  `*-ui.test.js` scripts once. Some tests evaluate bounded source fragments;
  new initialization must stay in the page startup section rather than leak
  into unrelated fragments at the end of the file.
- Keep test listeners on loopback. Exercise wildcard listener-table entries
  with synthetic native records rather than opening an all-interface socket.
- A fully GPU-loaded model can still retain substantial host-memory prompt
  checkpoints. The owned Ollama qualifier uses a 512 MiB saved-prompt cache and
  one checkpoint per slot; verify backend acceptance in its native log. Retain
  incomplete native responses and distinguish runtime allocation failures from
  model mistakes or host RAM exhaustion. Never diagnose an output limit without
  its completion metadata. See `docs/LOCAL_VISION_QUALIFICATION.md`.
- Build contract fixtures must restore their caller's complete process
  environment on success and failure. Remove added variables through the Env
  provider; assigning null through the native environment API left present empty
  variables on the measured host. Do not let deleted fake SDK paths reach later
  builds or modify an externally owned toolchain update to work around them.
- Keep source citations in structured metadata, not newly added numeric markers
  in usable template text. Preserve references actually present in the original.
  Candidate history must not be rewritten to match the hiring employer or role.
  Retain original visual-review observations and unknowns; optional model advice
  is not evidence for new candidate facts. See `docs/TEMPLATE_TAILORING.md`.
- After interrupted browser or fixture tests, check for orphaned Waypoint-owned
  browser profiles and fixture servers before concluding that host memory is
  unavailable. Verify each process's identity and that its test parent is gone
  before stopping it; never close another active workload. Browser `Drop` cleanup
  does not run if its parent is forcibly terminated; Windows job ownership now
  covers that case for newly started browsers, but old processes and temp
  profiles may remain. Record measurements rather than assuming all memory
  pressure is due to the app.
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

For continuation of the active product-development goal, read
`docs/REPO_STATE_REVIEW_2026-11-11.md`, the detailed
`docs/COMPLETION_PLAN_2026-11-11.md` and `docs/ACTIVE_DEVELOPMENT_PLAN.json`.
Use that queue's prioritized `next_task` once its prerequisites are ready; keep
all other unfinished criteria visible. Do not repeatedly
reconstruct a broad roadmap or treat the original T001-T050 `PLANNED` statuses
as a live progress ledger. The original handoff's contracts and all current
user/agent safety instructions remain binding. Update queue evidence/status
after meaningful checkpoints, distinguishing measured results from targets.
Keep work in the existing active goal; do not create another goal, branch,
release or agent delegation from the queue alone. A completed package does not
complete the overall goal. Historical plan/limitation text must not override
current-source evidence or silently remove unresolved acceptance work.

The 2026-09-26 gate results are historical, not evidence of the current checkout.
Read `IMPLEMENTATION_STATUS.md`, `docs/WEB_PROTOTYPE_CHECKPOINT.md` and current
source before selecting work. Preserve explicit unknowns and report measured
results separately from targets. Never submit a real application without explicit
authorization for that application; when instructed to stop before submission,
all live-site work must respect that boundary.
The product is a prototype; realistic employer-site behavior, accessibility,
installation and local-model quality need further verification.
