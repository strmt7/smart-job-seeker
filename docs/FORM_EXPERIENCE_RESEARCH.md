# Form experience research — 2026-11-02

Waypoint stores reviewed form experiences as sealed, revisioned records. They
are advisory context, not executable workflows, candidate answers or approval.
Retrieval matches the current stage and control, prioritizes a recognized
platform, retains diverse observations, and reserves room for another context
when one exists. A reported outcome is a user observation, not proof of causal
improvement. Changed or revoked records invalidate dependent pending results.

## Research and implications

- [Agent Workflow Memory](https://arxiv.org/abs/2409.07429) studies reusable
  routines for web agents, including transfer across tasks and websites. Its
  benchmark results do not establish Waypoint's performance. Reuse suggests
  hypotheses; it cannot replace inspection of the current page.
- [Managing Procedural Memory in LLM Agents](https://arxiv.org/abs/2606.23127)
  finds both transferable and specialized skills in enterprise tasks. Test
  improvements on held-out platforms, employers and changed layouts, not only
  the examples that produced a lesson.
- [WebVoyager](https://arxiv.org/abs/2401.13919) studies visually grounded web
  navigation. An old observation or a DOM label alone does not establish what
  is currently visible or whether a change succeeded.
- [OWASP Agentic AI memory guidance](https://cornucopia.owasp.org/cards/AAI3)
  treats retrieved memory as untrusted and emphasizes provenance. Employer
  content and notes cannot become system instructions or authorize actions.

These public pages were captured through the repository's bounded Crawl4AI
provider with robots checks, status 200 and substantive text. Native captures
are local build artifacts under `target/` and are not committed. Their SHA-256
digests respectively are `c85ca7a590bd376a2e54253b8e990f8b243ef08e0b67949a45f50bcecc5840b4`,
`310baf0d6762d08f0318a77024c21924837f40e1197c795acd797879aa0e3772`,
`c15c0463cde54deb7d79461c3ca7f5b3263545a2dda3dd7949a54f4b65726151` and
`9c6ed9103823ed0b0b59db3ed6a5121eef3ab0b1096a7c93b695879cffcb5c29`.

## Remaining evaluation work

The current mechanism does not automatically extract or validate lessons from
all attempts, train model weights, or demonstrate improvement on live employer
sites. Build a consent-scoped, privacy-preserving observation pipeline before
proposing lessons automatically. Evaluate paired runs with and without memory,
using the same local model, current-page observations and action permissions;
measure completion, incorrect actions, manual-review rate and latency. Hold out
entire employers and platform variants, include changed layouts and misleading
or conflicting memories, and report denominators and regressions. Do not
promote an observed correlation to a validated general rule. No application
submission is authorized by a learned lesson.
