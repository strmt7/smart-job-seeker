# Super-Comprehensive Plan: Rust Windows AI Job-Seeker Platform

## Product thesis
Build the most trusted, high-performance, **Rust-first Windows** job-seeker platform that combines:
- **Local LLM on 16GB GPU** for privacy and low-latency drafting/reasoning,
- **Internet reasoning mode** for evidence-backed market/company intelligence,
- **Enterprise-grade controls** (auditability, policy, reliability, observability).

## Non-negotiable requirements
- Native-feeling Windows app (Rust core; Tauri shell acceptable for UI delivery).
- Efficient on commodity hardware and offline-capable for core workflows.
- Public/open-source code and reproducible local deployment.
- Explainable, source-cited recommendations.

## “Solve what others do not” innovation targets
1. **Decision provenance graph**: every recommendation links to data sources, prompts, model version, and confidence.
2. **Adaptive reasoning router**: decides local-only vs internet-required reasoning with cost/latency guardrails.
3. **Application experiment engine**: A/B tracks resume variants and learns what improves callbacks.
4. **Career risk radar**: detects scam listings, stale posts, ghosting patterns, and role instability signals.
5. **Negotiation copilot with evidence**: compensation benchmark synthesis with source citations and confidence.

## Architecture (high-level)
- **Desktop shell**: Tauri (Rust backend, modern front-end UI).
- **Core services (Rust crates)**:
  - ingestion (jobs/email/calendar/docs),
  - ranking (skills fit, constraints fit, growth fit),
  - generation (resume/cover/interview),
  - policy engine (automation boundaries),
  - telemetry & audit log,
  - orchestration/router.
- **Data**: SQLite + vector index local-first.
- **Model layer**:
  - local inference via Ollama/llama.cpp-compatible runtime,
  - model presets for 16GB GPU,
  - optional remote reasoning connectors.

## Suggested local model envelope (16GB GPU)
- Quantized 7B–14B instruct models for drafting + reasoning.
- Embedding model for semantic retrieval.
- Router policy: local first; remote only for freshness/verification or low confidence.

## Roadmap

### Phase 0 — Foundations
- Monorepo scaffolding (`apps/windows`, `crates/*`, `docs/*`).
- Secure secrets handling and policy manifest format.
- Baseline performance harness.

### Phase 1 — Core workflow MVP
- Job ingestion connectors (boards, saved searches, email parsing).
- Unified opportunity model + deduplication.
- Tracker with timeline and reminders.

### Phase 2 — AI fit + explainability
- Resume-role semantic matching.
- Explainable scoring card (fit reasons, missing signals, confidence).
- Local-first draft generation for resume bullets/cover letters.

### Phase 3 — Internet reasoning and trust
- Company/role intelligence retrieval with citations.
- Scam/staleness detection features.
- Provenance graph viewer.

### Phase 4 — Enterprise-grade hardening
- Tamper-evident audit logs.
- Policy packs by region/site and automation mode.
- Reliability SLOs, observability dashboards, crash recovery.

### Phase 5 — Outcome optimization
- A/B experiment engine for application variants.
- Funnel analytics (view→apply→interview→offer).
- Recommendation policy that maximizes expected outcomes.

## Efficiency strategy
- Rust async pipelines, bounded concurrency, incremental indexing.
- Token/cache budgets by workflow and user intent.
- Retrieval before generation to minimize context/token load.
- Batched embeddings and SIMD-friendly preprocessing.

## Security and compliance baseline
- Local encrypted storage for sensitive profile data.
- Principle-of-least-privilege connector auth.
- Redaction pipeline for exported artifacts.
- User-verifiable audit/export package.

## Validation strategy
- Unit tests per crate (ranking, router, policy, parsers).
- Golden tests for prompt-output stability on fixed fixtures.
- E2E scenario tests for discovery → tailoring → tracking.
- Performance gates for startup, search latency, and generation throughput.

## Definition of “world-class” success
- Faster high-quality application throughput than incumbent OSS tools.
- Measurably higher interview callback rate over user baseline.
- Transparent AI: every recommendation is explainable and source-cited.
- Robust on a single Windows machine with 16GB GPU.
