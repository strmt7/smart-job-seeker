# T003 — Threat model and data map

Per MASTER_PLAN §12. Enforcement owners are Rust modules; every external write and every secret store has a named enforcement point.

## Data map (PII and sensitive categories)

| Data | Category | Stored where | Egress | Enforcement point |
|---|---|---|---|---|
| Candidate profile revisions (name, contact, employment history) | PII | Encrypted SQLite (T010) | only via explicit application/share actions | `waypoint_policy` approval layer; `waypoint_store` encryption |
| Evidence items (CVs, certificates, links) | PII + documents | Encrypted store; attachments in staging dir | same | parser sandbox (`waypoint_ingest` restricted worker) |
| Job observations (public postings) | public | store | n/a (inbound) | network broker fetch limits |
| Research artifacts (employer facts) | public-derived | store | queries may reveal which employer is researched | network broker query policy (no candidate PII in queries) |
| Application intents & approval grants | sensitive | store (durable journal) | their *contents* leave only on approved submit | one-use nonce, content-hash binding (T029) |
| Browser session data (cookies, profile) | credentials-equivalent | dedicated restricted profile dir, never committed | only to the specific employer origin | chromiumoxide-scoped profile; no unrelated-tab access |
| OAuth tokens / mail credentials | secrets | Windows Credential Manager only | never logged | secrets subsystem; config files must never hold tokens |
| Diagnostics/crash reports | may contain PII | local only by default | explicit opt-in per report, shown contents | `waypoint_export` redactor; default telemetry OFF |

## Threats and mitigations

| Threat | Capability assumed | Mitigation (owner) |
|---|---|---|
| Malicious job page / PDF delivering payloads or prompt injection | attacker controls posting content | All external content treated as untrusted text; parser in restricted child process; model never given authority to act; content-hash + sniffing for attachments; archive-bomb/size/depth caps (waypoint_ingest) |
| Prompt injection steering the model to an unauthorized write | attacker controls page text | Model output is structured data only; only Rust policy code can grant approvals; write tools are separate from read tools (waypoint_policy, waypoint_browser) |
| Rogue connector or drifted adapter | maintainer error / upstream change | Source permission registry (T002); adapter drift tests (T034); per-origin rate limits in network broker (T015) |
| Credential theft (config file exfiltration, token reuse) | local malware reading disk | Secrets only in Windows Credential Manager; SQLite encryption key from OS-protected store; tokens never in config/logs; secret canaries in tests |
| Cross-campaign data leakage | app bug | Campaign-scoped IDs and queries; tests assert no cross-campaign reads (waypoint_store) |
| Local IPC attack on inference server | local process | Private pipes or authenticated loopback with restrictive binding; "localhost alone is not authentication" (ADR-003) |
| Hostile update package / supply chain | network attacker | Signed packages, verifiable update metadata, pinned dependencies + SBOM + `cargo audit` in CI (T044–T046) |
| Symlink/path traversal on import/export | crafted archive | Canonicalize and confine to staging directory; refuse escapes (waypoint_ingest/waypoint_export) |
| Archive bombs / resource exhaustion | crafted file | Decompression caps, timeouts, bounded worker pools |
| Sensitive logs accumulating candidate data | later inspection | Log redaction; structured logs with data-category labels; retention policies; secure deletion documented with SSD limitation caveat |
| Employer form changes after approval | site behavior | Form-schema hash in the approval grant; mismatch invalidates the grant; preflight rechecks before write |
| Duplicate submission after ambiguous retry | crash/timeout | No automatic retry after external-write boundary; Uncertain state with user-driven reconciliation |
| Employer-side scraping terms violation | legal | LinkedIn and similar are `forbidden` (T002); only documented public interfaces or user-supplied URLs are used; capability table honest |

## Invariants code must enforce (testable)

1. A model/tool can never mint an `ApprovalGrant`.
2. No external POST/PUT/DELETE without a one-use, content-hash-bound grant checked immediately before the write.
3. Network fetches deny private/link-local addresses and re-check every redirect.
4. Candidate PII appears in network queries only when the query's purpose explicitly declares that data category.
5. Telemetry default is OFF; diagnostics show their exact contents before any optional report leaves the machine.
