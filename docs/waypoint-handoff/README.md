# Waypoint — complete research and agent handoff

Windows-native • Rust-owned logic and UI • local reasoning on a qualified 16 GB GPU • permissioned internet research

**Status: research, specification and design package. This is not a built Windows app.** No production Rust binary or target-GPU benchmark was created in this session. All implementation tasks are PLANNED.

## Start here

For the research findings, open `documents/01_Competitive_Research_Atlas.pdf` (or editable DOCX). For implementation, read `AGENTS.md` then `agent/MASTER_PLAN.md` / `documents/02_Rust_Platform_Blueprint.pdf`. For the UI, open `documents/03_UI_Atlas.pdf`, `design/UI_SPEC.md` and `prototype/index.html`.

The reference gallery is a static offline design artifact, not the production UI technology. The intended production UI is native Rust; this HTML gallery only makes the images convenient to review. Browsers with restrictive local-file policy may block it; the PDF/PNG/SVG files remain usable. Its navigation logic was smoke-tested with in-memory assets under the container's restricted browser policy.

## What is included

- Research: 92 unique repository candidates, 33 annotated entries at varying depths, all 37 returned jobseekers-topic entries, 31 additional discovery leads, 10 commercial/adjacent comparators and 9 papers. Counts overlap as documented; not all candidates were audited.
- Evidence: 103 source records with retrieval/qualification notes; CSV/JSON matrices; a 12-item gap/counterexample ledger; nine research-driven plan revisions.
- Implementation: a 16-section Rust master plan, 50 planned tasks across seven gates, architecture/state/reasoning diagrams in Mermaid, performance contracts, six JSON Schemas and eight fictional fixtures.
- Design: 40 canonical reference screens, each as PNG and editable SVG; screen/task/state catalogue; design tokens; one AI-generated exploratory concept board; static offline reference gallery; 41-page UI atlas.
- Validation: 17 passing design-contract checks, gallery/image smoke checks, document render QA and explicit limitations. These are not product safety certification or live application tests.

## Package navigation

`documents/` — reader-friendly DOCX/PDF/Markdown reports.
`research/` — complete registers, matrices, literature, discovery leads and research revisions.
`agent/` — implementation protocol, master plan, backlog and untouched implementation checkpoint.
`architecture/` — behavioral invariants, performance contract and editable flow diagrams.
`schemas/` and `fixtures/` — structural contracts and fictional positive examples.
`design/` — canonical screen catalogue, SVG/PNG files, tokens and concept art.
`prototype/` — standalone screen-gallery HTML.
`qa/` — actual validation results and limitations.
`tools/` — optional handoff validation helper; not part of the Rust product runtime.

## Non-negotiable distinctions

Documented is not runtime-tested; unknown is not absent; public code is not permission to reuse; local storage is not local inference; generated is not submitted; clicked is not confirmed; a receipt is not a hire; a 16 GB model file is not a 16 GB workload guarantee.

The product's ambition is to beat current alternatives in honest task-level comparisons. No universal novelty or market-superiority claim is established by this package. Read the counterexamples before writing promotional claims.
