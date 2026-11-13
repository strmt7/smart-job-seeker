# Structural control evaluation - 13 November 2026

This continues R02 on baseline `741bf8b8`. Temporary inline backend IDs made
previous native selectors unsuitable as a cross-reload evaluation key. Schema 3
adds a separate structural key while preserving native action identity and all
freshness, permission, completeness and visual guards.

## Implementation and limits

The driver emits optional logical metadata: exact root document URL, structural
leaf path and an ordered frame/shadow owner chain. Frame scopes retain the exact
child URL. Paths exclude generated HTML IDs; sibling ownership distinguishes
identical leaf controls from the same child URL. The existing node resolver
collects this read-only metadata without adding probes to ordinary action
resolution. Missing or over-budget metadata retains a bounded explicit error.
Logical metadata never resolves an action or substitutes for a native lease.

Schema 3 keeps independent expected values, requiredness, native readback, actual
image hashes and workflow requirements. It matches structural identities with
origin and section path. Missing logical identity never falls back to labels or
native IDs; performed unmatched entries remain critical. Duplicate keys remain
ambiguous. Schema 1/2 retain their historical matching behavior and reports.
See [the evaluation contract](../FORM_EVALUATION.md).

This is identity within a fixed, reviewed source structure, not learned identity
across arbitrary page rearrangements. Structural changes need new reviewed
annotations. Eight scopes, bounded selectors/URLs and 32 KiB cap the descriptor.
Unsupported independent/inline/shadow contexts retain their original barriers.

## Measured native and accounting checks

Six fictional, read-only scenarios pass, with two observations each and sixteen
control observations in total. They cover a changed HTML ID, inline document
reload, same-URL sibling frames, inherited srcdoc, an open shadow root and
same-URL independently targeted sibling frames. Identical sibling leaf paths
remain distinct; logical identities remain equal across the native changes.
All six browsers close, with zero entries and no clicked/typed trace events.
The final native report is `target/logical-native-22hyl6r6/logical-controls.private.json`.
The earlier successful run remains separately preserved.

The 29 corpus tests pass, including seven new schema-3 tests for volatile native
bindings, sibling collision, document/scope differences, missing identity,
legacy compatibility, malformed metadata and workflow identity. These accounting
unit tests are not model or real-workflow evidence.

## Compiled local-model evidence

One new embedded variation of the existing fictional choice-boundary family is
**development**, not held out. An independent source map declares two answerable
facts and six required unknowns. The compiled app's read-only inventory finds all
eight controls with no omissions or walls. Its owner chain, source IDs and
requiredness are checked before freezing the schema-3 annotations. Answers stay
outside the app/model workspace. The repeat checks identical source/profile and
structural/value truth before inference; native diagnostic selectors are not
used as the equality oracle.

Both runs use `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
context 4,096, owned runtime 0.40.16 and app SHA-256
`e18ece28339a530a2ea68364e64065d76f8b49d1add94d63a150d50b1d1df8e7`.
Native and blind local-model visual checks verify both supported facts in each
run. All six unknowns remain unentered. Incorrect, ambiguous, unexpected,
unverified and unbound performed entries are zero in both runs. The four actual
post-entry images were also inspected during development; that inspection does
not replace the local model's observations. No whole-workflow or held-out credit.

| Observation | First run | Same-input repeat |
| --- | ---: | ---: |
| Verified answerable / declared answerable | 2 / 2 | 2 / 2 |
| Unknown entered / declared unknown | 0 / 6 | 0 / 6 |
| Critical entry or uncertainty | 0 | 0 |
| Native peak app-job commit, bytes | 9,141,411,840 | 9,144,242,176 |
| Minimum sampled available host RAM, bytes | 23,285,112,832 | 22,474,211,328 |
| Maximum sampled whole-adapter WDDM usage, bytes | 15,055,536,128 | 16,802,340,864 |

The app has the unchanged 10 GiB job limit inside an 11 GiB qualifier, with the
shared half-free-RAM admission and 3 GiB reserve. Existing verified caches are
reused without another model copy. Both model sessions report native process
exit and an empty owned listener after stop; both app trees drain and the shared
runtime remains unchanged. WDDM samples cover the whole adapter, not process
attribution or continuous peak. Runtime API size/VRAM values have a different
scope and do not prove complete model residency. These short runs do not establish
sustained leak freedom or clean-host installation readiness.

Private evidence: `target/logical-frame-model-el0gkz4d`,
`target/logical-frame-repeat-ejm0a6i5` and the controller/check receipts in
`target/logical-control-smil5o4_`. Frozen input/profile/corpus hashes, raw native
and visual observations, actual screenshots, runtime and ownership evidence
remain private. The later Clippy correction only collapses an equivalent nested
condition in the ignored native test; production and model input code are
unchanged.

## Final integration and cleanup

The Windows workspace passes 1,371 tests, with 612 opt-in tests ignored across
43 result groups. The canonical Windows app build, warnings-denied Clippy and
format checks pass. The initial Clippy finding in the ignored native test and
its corrected rerun remain separate private reports; no lint was suppressed.
The production executable hash is unchanged after this test-only correction.

The existing 15-case entry/stale-scope matrix passes on the shared resolver:
eight entry cases produce ten exact readbacks; seven cases remain review-only.
All fifteen browsers close. This preserves the separate native action contract;
read-only logical metadata does not authorize entry. Native evidence is
`target/entry-native-vtei9c17/inline-coverage.private.json`.

The final scoped audit reports zero matching processes, resident models,
services, scheduled tasks, startup entries or observation errors, with
34,060,013,568 bytes of host RAM available. Receipt:
`target/development-host-6569801fccaf40b0a140e03cbcd80cb1.private.json`.
The earlier unconfirmed shutdown remains unresolved; these clean runs are not
a diagnosis of its cause or sustained leak qualification.

## Remaining work

Broader field/workflow truth, model-scored held-out families, repeated controls
across complete applications and the French 6/7 case remain open. Target-less
child forms retain review. Tall or partly offscreen frame behavior needs its own
native qualification; this result covers the authored 650 px embedded viewport.
The earlier unconfirmed Close is still unexplained. No real employer form was
interacted with, application submitted or release published.
