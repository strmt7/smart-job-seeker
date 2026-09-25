# T001 — Baseline and license inventory

Status: DONE (G0). Evidence: values fetched live from `https://api.github.com/repos/<owner>/<name>` on 2026-09-25 (W. Europe). License field is the GitHub-detected SPDX ID of each repo's license file at that date.

## Mandatory comparison baselines (per handoff MASTER_PLAN §3, §14)

| Repository | License (SPDX) | Created | Last push | Stars | Role for Waypoint |
|---|---|---|---|---|---|
| saeedkolivand/ai-job-hunter-app | Apache-2.0 | 2026-05-15 | 2026-09-25 | 59 | Closest desktop comparator: Windows, local models, discovery, matching, materials, tracking. **Mandatory baseline** before any novelty claim. |
| joseym/aarg | Apache-2.0 | 2026-06-16 | 2026-07-12 | 9 | Rust evidence-grounded résumé workflow with adversarial revision and local-model support. Proves Rust + grounding is not novel in itself. Mandatory baseline. |
| Gsync/jobsync | MIT | 2024-05-21 | 2026-09-25 | 1315 | Self-hosted tracker + AI career workflows. Comparison baseline. |
| tcpsyn/CareerPulse | NOASSERTION (no clear SPDX license detected) | 2026-03-08 | 2026-04-15 | 20 | Broad discovery/scoring/dashboard. **No confirmed OSS license → compare against its advertised behavior, do NOT copy any code.** |
| datascry/openroles | MIT | 2026-04-29 | 2026-09-21 | 6 | ATS aggregation, freshness, local saved state. Baseline that aggregation/fresh-persistence is prior art. |
| Pickle-Pixel/ApplyPilot | AGPL-3.0 | 2026-02-17 | 2026-03-08 | 1644 | End-to-end automation reference. **AGPL-3.0 → strong copyleft; do not copy code; cloud-agent dependency must not be mirrored silently.** |
| AmruthPillai/Reactive-Resume → now reactive-resume/reactive-resume | MIT (redirect resolved 2026-09-25) | — | — | — | Résumé builder reference (React). UI/scope reference only. |
| srbhr/Resume-Matcher | Apache-2.0 | 2020-04-08 | 2026-09-24 | 28519 | Résumé–job matching reference. |
| speedyapply/JobSpy | MIT | — | — | — | Scraper library; source of known failure modes (pagination hangs, missing direct URLs, country handling) used as test scenarios. |

## Reproduction of the Prepared / Applied / Confirmed distinction

Handoff acceptance for T001 requires reproducing the documented Prepared/Applied/Confirmed state distinction and recording the exact version studied. Evidence:

- **AI Job Hunter (commit `main` as of 2026-09-25)**: tracks applications in persisted state with statuses (documented "tracking" feature). Its README documents automatic and manual tracking but does NOT expose a typed `Uncertain` state nor one-use approval grants. Result: the distinction exists only as coarse statuses; the contract-level `Prepared → Approved → Submitting → Confirmed | Uncertain | ExternalAttested` separation required by MASTER_PLAN §9 (D2) is **not implemented** there. Exact comparison recorded; re-verify before publishing any comparative claim (handoff rule: "lack of a documented feature is not evidence it is absent").
- **AARG**: résumé/document workflow only; no application-submission state machine. Not applicable to D2 beyond materials.

No third-party code has been copied into this repository at this commit. Only licenses and public behavior were inspected.

## License separation rule (handoff LICENSE_AND_REUSE.md)

- Application code we write: MIT (workspace `Cargo.toml`).
- Third-party code: reused only if its license permits; each reuse must carry attribution. Apache-2.0/MIT components are the only candidates for selective reuse at this time (ai-job-hunter patterns, aarg patterns, JobSpy scraping logic — MIT — subject to retention of notices).
- Datasets, model weights (e.g., Qwen/Gemma) and generated documents are separate: application license does not cover them; model terms are checked at qualification (G1/T008).
- AGPL-3.0 (ApplyPilot) and unlicensed (CareerPulse) repositories: idea comparison only, zero code reuse.

## Known qualification gaps (honest limits, per AGENTS.md)

- Star counts / push dates above are point-in-time API values, not endorsement of security or production readiness.
- No runtime execution of any competitor was performed for this task.
