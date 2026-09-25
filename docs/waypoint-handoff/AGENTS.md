# Waypoint coding-agent entry point

This is a research and design handoff, not a production application. All implementation tasks start PLANNED.

## Read in this order

1. README.md and research/coverage_counts.json.
2. agent/MASTER_PLAN.md, especially mandatory contracts and gates.
3. research/RESEARCH_ATLAS.md, research/gap_ledger.json and research/PLAN_REVISIONS.md.
4. design/UI_SPEC.md and design/screen_catalog.json. Canonical wireframes specify behavior; generated concept art is not an authority for facts or state semantics.
5. schemas/, architecture/, fixtures/ and agent/backlog.json.
6. qa/DELIVERY_QA.md for what this handoff did and did not validate.

## Rules

Owned product logic and native UI are Rust. Do not quietly substitute React/Electron, Python services, WSL or cloud inference. Explicit third-party engines are permitted dependencies. Keep all end-user core functionality available without a paid LLM account.

Reasoning-enabled local inference is mandatory for AI decisions; internet retrieval is mandatory for current external facts used by those decisions. Offline drafts must remain visibly stale/unverified. No silent paid-cloud fallback, fabricated experience, invented job openings or unsupported visa guarantees.

A model cannot authorize a write. Each exact external application/email/calendar write requires explicit user approval enforced by Rust policy code. Preserve Prepared, Submitting, Uncertain, Confirmed and ExternalAttested distinctions. No automatic retry after an ambiguous write.

Do not copy public code without permission or mix code/data/model licenses. Pin exact dependencies and model checksums in G0/G1. Build the first native/accessibility/GPU qualification slice before investing in all features.

Every completion report must include commit, tests actually run, missing tests, resource measurements actually taken and known failures. Do not invent results. The lack of a documented competitor feature is not evidence that the feature is absent. Recheck novelty claims before publishing them.

Never put real candidate records, emails, cookies, API keys or browser profiles in this repo. Use fictional fixtures. Generated UI examples are not live job listings. Do not send real applications as an automated test.

## Checkpoint protocol

Maintain IMPLEMENTATION_STATUS.md with current commit, completed task IDs, exact validation commands/results, blockers and the next smallest action. Do not overwrite earlier research or silently shrink the agreed scope. A failed safety/performance test blocks the affected gate; record a scoped fallback rather than claiming success.
