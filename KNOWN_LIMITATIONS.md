# Known limitations register (T050)

Last updated: 2026-09-25. Honesty rules from AGENTS.md apply: absence from this list is not a claim of absence of problems.

## Hard limitations (known, deliberate, documented)

1. **Store is not encrypted yet.** SQLite plaintext; schema and API are encryption-ready, SQLCipher integration is a planned task. Until then: local store contains profile data unencrypted — candidate decision to use remains theirs, but the limitation is explicit.
2. **No automated local-inference adapter in code yet.** The `Inference` trait + stub + capability probe exist and live measurements were taken via Ollama (qwen3.5:9b), but the production adapter wiring (private pipe / authenticated loopback, model download integrity checks) is not written.
3. **Accessibility observation incomplete (T006).** Keyboard navigation is implemented; Narrator/NVDA announcement quality, IME composition, 125–200% DPI and multi-monitor behavior have NOT been observed and recorded. Gate G1 stays partially open for this reason.
4. **No code signing / installer.** The release binary builds; signing certificates and MSIX/NSIS packaging are not produced. Until signed, users must verify hashes manually.
5. **No update channel.** Update metadata/signing blocked on T044 signing; store-level migration rollback is implemented and tested.
6. **Live application submission not exercised.** The supervised-apply engine is fixture-tested end to end (preflight, one-use grants, journal, receipts, recovery), but no real external submission has been performed — by design (no automated real applications as tests).
7. **Browser automation backend not integrated.** `BrowserCommand` is the narrow typed surface; chromiumoxide/CDP wiring is not written yet. All form logic runs against `FormSchema` fixtures.
8. **Benchmark corpus is a skeleton.** Measured baselines exist only for the local model; the §14 corpus (5,000 observations etc.) is a design target, not collected data.
9. **ttf-parser 0.25.1 unmaintained notice** (RUSTSEC-2026-0192, informational, no CVE): transitive via egui. No safe upgrade at pinned versions; revisit on egui upgrade.
10. **WMI AdapterRAM reporting is clipped** (reports 4 GB on a 16 GB card — a known Windows WMI limitation). VRAM figures in docs come from the inference runtime's own report, not WMI.
11. **Semantic search is lexical only.** No embeddings/vector index yet; MASTER_PLAN §ADR-002 says to introduce one only when corpus size and quality tests justify it.
12. **Single-user, single-machine.** No sync, no multi-device, no team features. Coach/reviewer sharing is implemented as scoped, expiring, redacted packets only.
13. **Usability pilot not run.** T049 pending by design.

## Non-claims (per MASTER_PLAN §16)

- No claim of "world's best", no hiring-uplift claims, no "undetectable AI" claims, no exactly-once submission guarantee, no universal ATS support, no immigration/eligibility verdicts.
- Competitor comparisons in `docs/g0/BASELINE_AND_LICENSE_INVENTORY.md` are API-verified metadata, not executed audits.
