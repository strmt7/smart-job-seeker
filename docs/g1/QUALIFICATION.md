# G1 qualification evidence — Waypoint native shell + inference probe

Machine recording date: 2026-09-25. All numbers are *measured on this machine*, not targets.

## Hardware / runtime
- GPU: AMD Radeon RX 9070 XT, driver 32.0.31041.1004 (Windows 11).
- Total 16 GB class card; adapter-reported VRAM via WMI is clipped at display (4 GB field), measured residency below is from the runtime's own report.
- Inference runtime measured: Ollama 0.34.4 (`ollama serve`, localhost API). This is a llama.cpp-based local backend — a *dependency*, not app logic (ADR-003).
- Rust: rustc 1.98.1 stable-x86_64-pc-windows-msvc, installed 2026-09-25 via rustup.

## T008 measurements — qwen3.5:9b (6.6 GB model file)

| Metric | Measured | Gate target |
|---|---|---|
| Cold model load | 11.70 s | n/a (model load excluded from app cold-start target) |
| VRAM while resident (`size_vram`) | **13.23 GiB** | ≤ 14 GiB on a 16 GiB card — **PASS** |
| Reported CPU offload | `size_vram == size` → zero CPU offload bytes | no hidden shared-memory paging — **PASS** |
| Structured constrained output (think=false, `format=json`) | `{​"a":1}` exact | typed schema validation path exists — **PASS** |
| Bounded reasoning (think=true, num_predict=2000) | thinking present, final line `{"answer": 391}` correct, done_reason `stop` | reasoning control works — **PASS** |
| Generation speed | ~77–79 tok/s | record, not gate |
| Prompt eval (TTFT-ish) | ~79 ms for 26-token prompt | record |

Known pitfall recorded (for the inference adapter): with `think=true`, small `num_predict` budgets return an **empty final answer** because the budget is consumed by reasoning. The adapter must therefore size `max_output_tokens` ≥ reasoning budget + answer budget and treat `done_reason != "stop"` as a validation failure rather than empty content. The `StubInference`-level test (`zero_token_budget_refused`) mirrors this rule.

## T005 shell
- eframe/egui 0.36 pure-Rust native window; no Node/Python/WSL/Electron — cargo tree contains Rust crates + wgpu only.
- Long-widget virtualization: only the visible row range is laid out per frame; a 50,000-row fixture frame renders headless in **25.3 ms** in *debug* test build (test `one_frame_completes_quickly_with_50k_rows` asserts < 500 ms on the unchanged debug profile; release will be faster). The 16.7 ms/p95 paint budget is a *release target yet to be recorded on a steady-state profile* — see Known limits.
- UI thread does no blocking work: shell holds pre-built view models and emits typed `ShellMessage`s only (test `navigation_messages_are_typed`).

## T006 accessibility — PARTIAL, honestly recorded
Implemented: full keyboard navigation via egui focus, selectable rows, virtual scrolling.
**NOT yet observed**: Narrator/NVDA announcement correctness, IME composition, 125/150/200% DPI, multi-monitor, RTL sample. These need an interactive desktop session and are explicitly recorded as *not validated* — gate G1 remains **partially open** until screen-reader/DPI passes are observed on hardware.

## T007 handshake
`waypoint_inference::Inference` trait: probe → capability report (reasoning + structured-output flags, offload bytes), `generate_structured`, `cancel`. `StubInference` deterministic fixture passes. Live probe against qwen3.5:9b done manually above; the automated Ollama-backed probe adapter is the next task (T007 completion criterion: repeatable probe in code, not just curl).

## T009 resource scheduler
`ResourceBudgets` defaults per MASTER_PLAN §13 (global fetch 4, per-origin 2, browser 1, reasoning 1, GPU ceiling 14 GiB); counted scheduler refuses over-budget acquisition (tests `scheduler_never_exceeds_budgets`).

## What this does *not* claim
- No competitor was executed; baseline table is API-verified metadata only.
- 50k-row latency recorded here is a debug-profile layout time, on an unloaded machine; the §13 p95 budget needs a release-build soak test.
- Screen-reader/IME/DPI are not yet validated (see T006 partial).
- No signing/packaging yet (G6).

## Live ATS parse validation (T014 evidence, 2026-09-25)
Read-only public GETs (registry `allowed_public`):
- Greenhouse `boards-api.greenhouse.io/v1/boards/vercel/jobs` — parses (wrapped `{"jobs":[...]}` shape).
- Ashby `api.ashbyhq.com/posting-api/job-board/ashby` — parses (wrapped `{"jobs":[...]}`).
- Lever `api.lever.co/v0/postings/spotify?mode=json` — parses (79 live rows).
- Lever boards for netflix/cloudflare/stripe and others 404 on this endpoint — recorded; adapter must surface 404 as `AccessDenied/NotFound`, not empty results.
Live checks are `#[ignore]` tests run via `cargo test -p waypoint_connectors -- --ignored`.
