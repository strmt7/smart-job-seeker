# Finding-state review

Follow-up, 2026-11-10: exact commit `c05037e1` passed hosted CI, coverage and
CodeQL. All three language analyses reported empty errors; the Actions analysis
`2133956763` no longer has the earlier ingestion error. The five permission
findings resolved automatically. A fresh inventory has zero open CodeQL alerts
and ten `fixed` alerts, all with null dismissal actors/reasons. No status mutation
was performed. Native responses are retained under
`target/finding-state-audit/c050-*.private.json`. This updates the GitHub state
below; DeepSource's stale-analysis, quota and history limitations remain open.

Reviewed 2026-11-10 against GitHub main `1c411684` and the accessible provider
state. No finding state was changed through a dashboard or API. The
[agent instructions](../AGENTS.md#findings-remain-under-detector-control) prohibit
manual closure, dismissal, suppression, ignores and equivalent status changes
across all current and future detection tools. Fix code and let the detector
recheck. Unfixable findings remain visible; publishing remains the user's decision.

## GitHub

Paginated API inventories found five open CodeQL alerts and five automatically
fixed alerts. None of the closed alerts had a dismissal actor, reason or time.
There were zero dismissed, automatically dismissed or open Dependabot alerts,
zero resolved/open secret-scanning alerts and zero closed ordinary issues.
No manually dismissed finding was present to reopen in these inventories.
Automatically fixed findings were not reverted or recreated.

Open CodeQL alerts 5, 7, 8, 9 and 10 all report missing `GITHUB_TOKEN` permission
limits in `.github/workflows/ci.yml`. The workflow now declares `contents: read`,
as the separate coverage workflow already does. This fixes configuration rather
than changing alert status. Confirmation must come from a new exact-commit
analysis; the observations above remain open until the detector updates them.

The `d68dcd18` CodeQL workflow completed successfully, but the Actions-language
analysis API returned `Unknown Error` (analysis `2133433784`). Its zero returned
results are not valid clean acceptance. Python and JavaScript analyses reported
no analysis errors; the five permission alerts remained open in the retained
inventory. Do not manually close them or infer resolution from a green workflow.
The hosted ordinary tests/build and browser job passed; a separate layout-checker
failure is retained and addressed in the
[layout lifecycle review](LAYOUT_QUALIFICATION_LIFECYCLE.md).

The hosted `1c411684` browser suite passed 137 cases. Its ordinary build/test job
failed one inference mock: the server closed without consuming the request, and
Windows returned connection-reset before the final-response pressure guard could
run. Mock servers now drain complete bounded requests and join their threads;
a fragmented-request regression and all 21 ordinary inference tests passed
with two opt-in cases ignored. The production pressure assertion was preserved.

## DeepSource and local records

The repository API reports zero ignore rules. Enabled analyzers are Python,
Rust and secrets; the retained configuration excludes generated build/runtime
artifacts rather than reported source findings. The occurrence API does not
expose historical dismissal actors/statuses or a complete team-level suppression
history, so this is not a claim to have reconstructed every historical action.
No suppression/reopening mutation was issued without an identified target.

The current DeepSource check was skipped because the free analysis quota is
exhausted. A source-bound inventory correctly refused the current commit because
the provider returned **Run does not exist**. The latest returned main-branch
analysis finished on 2026-11-01 at commit `01ea65df`, with status `FAILURE`.
Do not treat that as verification of the current checkout, trigger a paid upgrade
or conceal the gap by disabling analysis.

A separate read-only traversal retained all 2,265 provider-reported occurrences
and reconciled page counts/unique identities. These are stale reported findings:
2,059 minor, 202 major and four critical; categories include 44 security and one
low-entropy secret report in a Rust test source. They are not 2,265 confirmed
current vulnerabilities. All remain under provider control.

The four critical reports concern two Python globals supplied by the composed
PDF-worker script, a font-encoding tuple unpack and a package-file context
manager. Current source assembles the helper definitions before the PDF editor
and closes the package file through a context manager. Original analysis
locations are stale and need source reconciliation; these contextual observations
do not dismiss the reports. The test-source secret report likewise remains open
and must not be called a real credential or a false positive without source-bound
evidence. No secret values were printed or copied into repository documentation.

No local manual-closure ledger or current audit-ignore configuration was found
in the inspected repository records. Historical gate/SBOM documents remain dated
evidence, not current scanner acceptance. Private receipts, paginated native API
responses and their digests are under `target/finding-state-audit/`; they are not
committed. Inaccessible historical provider state remains an explicit limitation.
