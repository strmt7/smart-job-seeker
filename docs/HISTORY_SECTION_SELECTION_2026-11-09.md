# Career record selection — 2026-11-09

The history panel now offers **Choose from saved history**, an inspected-section
picker, and section numbers for repeated labels. The user can still select an
entry explicitly. Proposals start unchecked and copy only into answer editors;
the separate reviewed-fill grant remains mandatory for browser interaction.

The local model classifies section intent using page labels, without candidate
records. The application resolves a whole checked record by repeatable rules:

- Current: exactly one explicitly confirmed ongoing entry of the requested kind.
- Latest completed: known end dates for every compatible non-current entry, with
  one end-date interval strictly later than all the others and wholly before
  the current UTC month. Missing dates, ties
  and overlapping year/month precision remain ambiguous.
- Generic: exactly one checked entry in that category.
- Other restrictions, ambiguous intent, mixed sections and other-person questions
  require explicit entry selection or manual handling.

Every checked entry is considered, independent of storage order. The existing
component mapper resolves exact saved values and retains date precision. Model
semantic errors remain possible; deterministic selection does not prove correct
interpretation. Existing model-review blocks are never cleared to improve results.

Named ancestor sections carry labels, inference status and browser backend-node
identities. Frame/shadow prefixes prevent scope collisions; equal heading text
does not merge structural identities. The initial browser test exposed changing
frontend node IDs; persistent backend IDs corrected that failure. Missing backend
identity refuses inspection. Automatic selection accepts one comparable
ancestor/descendant path; divergent sibling paths are refused. Radio fields
currently retain their existing handling without the new section path, requiring
explicit entry selection for this feature.

An unresolved child heading, excessive nesting or incomplete group scan exposes
no selectable partial path, even when a parent has a verified name. The nested
ambiguous/hidden-heading check passed in
`history-section-incomplete-path-live.log`. Model/UI evidence below precedes this
final conservative guard; its changed browser path was independently checked.

Context participates in schema hashes and advice bindings. Section movement,
replacement, labels or inference status invalidate old approvals. Empty context
is omitted to preserve legacy flat-schema serialization. The full profile,
checked records, destination, schema, UTC month and session bind automatic advice. Reverting
a change does not resurrect an already invalidated result.

Measured evidence, retained privately under ignored `target/`:

- Web suite: **726 passed, 397 ignored**, no failures. Affected application,
  browser, inference and workspace library suites passed **34, 29, 20 and 60**
  tests respectively; seven opt-in checks skipped. Managed Windows Clippy passed.
- `repeated-section-identity-live-2.log`: unchanged schema stability, distinct
  equal-labelled rows, nested equal labels, frame/shadow scope and field movement
  passed in one headless fictional browser. First failed attempt retained.
- `history-auto-stale-binding-live.log`: automatic advice and changed-section
  invalidation passed, alongside existing profile/session freshness checks.
- `history-record-choice-pipeline-gemma-final.log`, `history-pipeline-XmuSm5/`:
  local `gemma4:12b-it-q4_K_M` selected correct current work, previous work and
  education records from multiple saved entries in a 14-field fictional form.
  Ten expected values passed visual transcription and live DOM checks; four
  restricted fields stayed empty. The filled screenshot was also inspected.
- `history-intent-variants-gemma-1.log`, `history-intent-O5ffAQ/`: seven semantic
  cases passed, including German work, French ongoing study, generic education
  and three ambiguous/unsupported requests. No browser was used in this check.

Failures are retained. A new `ministral-3:14b` run conservatively blocked ordinary
career fields and failed completeness. The first automatic 12B run classified
generic education as unknown and stopped. Separating the model's `generic`
question category from the application's one-entry requirement corrected that
conceptual ambiguity. No selector, company or site-specific rule was added.
These small samples do not establish broad coverage or repeated-run reliability.

The attempted Crawl4AI DOM reference returned the protocol overview, failed its
scope marker and was rejected (`research/history-section-cdp-dom.native.json`).
Exact native [upstream protocol JSON](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/json/browser_protocol.json)
was captured instead: 1,423,215 bytes, SHA-256
`cb4b0a40e03fddfdc4191f832259b9fed6684c303842c55d8c6fe1702f9a5efb`.
Behavioral stability is proven by the browser check, not that type description.

The compiled-app UI check passed 29 grouped cases, including five widths
(320–1920 px), native mouse resizing, automatic entry mode, section selection,
duplicate-label distinction and 12 actual browser-zoom cases (80–400% at
1280/1920 px). Each zoom case verifies a visible, unobstructed section picker
and action button as well as absence of overflow. No script errors occurred.
Evidence: `history-auto-ui-qualification-native.log`, `candidate-history-fuaVfN/`;
result receipt SHA-256
`fbaa187f2fe1714b7ec919632a2eb8933eea1bc87aa25e9de7e4847ed6de6920`.
These checks use intercepted fictional advice, not an employer or model benchmark.
An initial zoom capture was blank despite passing DOM checks. Diagnostics retained
the mismatch; direct compositor-surface capture produced the correct visible
image without changing application styles. The final native 400% capture and
ordinary narrow/wide captures were visually inspected. This is Chromium evidence,
not qualification of every browser or display configuration.

Global repeated-row assignment, history-based document tailoring, answer-archive
provenance, broad live-site qualification and proven no-write isolation remain
open. No real application was interacted with or submitted; no release was made.
