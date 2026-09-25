# T002 — Source permission registry

Controls which external sources and which operations the platform may use, per MASTER_PLAN §6, §9 and AGENTS.md ("Site terms, geography and robots restrictions must be rechecked; robots permission alone is not a legal license").

Machine-readable copy: `crates/waypoint_policy/data/source_permissions.json` (loaded by the policy layer; the struct definitions live with task T002 code in `crates/waypoint_policy`).

## Permission model

For each source, six operations are recorded explicitly (they are not implied to hold together):

- `discover` — list/search public postings
- `read` — fetch a single posting detail page/document
- `prefill` — place personal data into a live form (information-disclosure boundary)
- `upload` — upload documents to the source
- `submit` — perform the final external write
- `confirm` — read receipt/status evidence

Each operation is one of: `allowed_public` (no credential, permitted by provider's published interface), `allowed_authenticated` (requires candidate's own account and explicit consent), `requires_review` (allowed only after a per-site review is recorded), or `forbidden` (not to be automated by this app).

## Registry (initial rows)

| Source | discover | read | prefill | upload | submit | confirm | Notes |
|---|---|---|---|---|---|---|---|
| Greenhouse public job-board JSON (`boards.greenhouse.io` /<board>.greenhouse.io/api) | allowed_public | allowed_public | requires_review | requires_review | requires_review | requires_review | Public listings documented; application flow is employer-hosted. Employer-side API credentials are NOT available to candidates — those endpoints are `forbidden` for this app. |
| Lever public postings (`jobs.lever.co`, `api.lever.co/v0/postings`) | allowed_public | allowed_public | requires_review | requires_review | requires_review | requires_review | Public posting listing is documented; submission is employer-hosted form only. |
| Ashby public postings | allowed_public | allowed_public | requires_review | requires_review | requires_review | requires_review | Same shape as Greenhouse/Lever. |
| Workday tenant pages | requires_review | requires_review | requires_review | requires_review | requires_review | requires_review | Per-tenant terms vary; adapter only after drift/permission review (MASTER_PLAN §9). |
| iCIMS / SmartRecruiters / SuccessFactors | requires_review (each) | requires_review | requires_review | requires_review | requires_review | requires_review | Later adapters only. |
| LinkedIn | **forbidden** | **forbidden** | forbidden | forbidden | forbidden | forbidden | No automated scraping/applying solely because a public bot exists (MASTER_PLAN §4 ADR-004; ToS). Manual handoff only. |
| General web search (user-configured provider / SearXNG instance) | requires_review | requires_review | n/a | n/a | n/a | n/a | Optional coverage extension; must not send candidate PII in queries (§7). |
| Direct employer URLs supplied by the user | allowed_public | allowed_public | requires_review | requires_review | requires_review | requires_review | Manual URL research remains useful without a paid search API. |

## Acceptance mapping

The policy struct (T002 code) rejects selecting an `allowed_authenticated`-only submit route as a candidate public submit route, and rejects any submit/prefill operation for sources marked `forbidden`. "Employer credential-only endpoints cannot be selected as candidate public submit routes" is enforced by unit tests in `waypoint_policy`.

The registry is editable data, but every change is versioned and shown in the UI coverage dashboard (T019); silent capability changes are not permitted.
