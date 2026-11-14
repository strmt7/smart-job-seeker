# Same-site cross-origin fixture readiness — 14 November 2026

The qualification checkpoint `b09ea6455c4757aa984bc8cddf0dab9ec778bb99`
passes hosted workspace/build/layout, coverage, PDF, tool-contract, supply-chain
and code-scanning jobs, but CI `43622309552` fails its browser job
`132530484115`: **138 passed, one failed**. The failed test is
`inaccessible_same_site_cross_origin_form_is_detected_without_fields`.
It retains the generic uninspected-frame review wall and exposes no fields,
but lacks the expected more specific inaccessible-form wall. That is not a
passing browser selection or permission to interact with real forms.

The unchanged test passes locally once (3.39 seconds); the original hosted
failure did not retain child readiness observations, so its exact timing/cause
is not established. A local pass alone is not presented as a reproduction or a
root-cause fix. The fixture nevertheless had a concrete premise gap: it assumed
a top-level open had established its cross-origin child's parsed/rendered form.

The owned child now signals DOMContentLoaded plus two animation frames through
an origin- and window-source-bound message. Only the test uses that independent
fixture checkpoint, with a five-second polling budget. Individual native calls
retain their existing transport deadlines; actual monotonic elapsed time is
recorded. A fresh native inspection
must still establish the inaccessible controls; the signal never clears a
product review barrier or supplies native form evidence. Both the initial and
fresh inspections must expose zero fields. The original inaccessible-form wall
assertion and hidden-frame follow-up remain. No fallback accepts a generic wall
in place of that assertion. Icons are inline to avoid unrelated icon traffic.

Both fixture servers remain stopped/joined on success or panic. Their finite
60-second service lifetime covers launch/navigation, readiness, inspections and
teardown; production timeouts are unchanged. Immutable observations go to the
workspace's `target/browser-qualification/` and include initial/final walls,
field counts, readiness, inspection errors and monotonic wait time. The first
artifact-path attempt wrote below the crate's ignored target folder; it remains
retained. A subsequent borrow-of-moved-path compilation failure is also retained
and is not counted as a native case result.

The fixture-only local browser selection passes **139/139** in 380.19 native
seconds, using the same four explicit exclusions as CI. The final additional
initial-field assertion/icon refinements then pass the targeted case, one of
one, in 3.37 native seconds. Warnings-denied workspace Clippy passes. Production
browser/actor code and dependencies are unchanged by this follow-up; qualification
model/ordinary/UI evidence retains the source and scope recorded in its original
checkpoint. The new exact-source hosted browser run is still required.

This corrects fixture readiness and adds diagnostic evidence. It does not prove
the exact cause of the earlier CI event, eliminate every timing problem, qualify
all cross-origin application entry or complete R03/R05. The earlier failed CI
result, current numeric/whole-workflow limits and no-send boundaries remain
visible. No detector finding was manually closed, dismissed or suppressed.
Private evidence: `target/candidate-facts-s39iwmr6/` and
`target/browser-qualification/same-site-frame-readiness-*.json`.
