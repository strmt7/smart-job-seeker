# Embedded form viewport availability - 13 November 2026

This R02/R03 continuation starts from `1cc4029`. A reproduced native failure
prevented entry into a visible field whenever its tall iframe's full-box center
lay outside the browser viewport. The frame's document, owner and response were
verified; the availability probe still returned review. A separate adversarial
case exposed a clipped scope being omitted without a review warning.

## Changes

The owner probe now intersects the rendered rectangle with each accessible
ancestor viewport. It projects that intersection through native frame offsets,
requires a positive interior area and compatible dimensions, then hit-tests the
same point through the owner/shadow/ancestor chain. Inaccessible ancestors,
unsupported transforms, empty intersections and failed hit tests retain review.
Depth and inspection time budgets are unchanged. This read-only probe does not
scroll, focus, type or click.

Owner availability is separate from complete field visibility. Actual entry
still requires the existing fresh document/schema/census checks, a complete
native-bound field image, the review callback, actual input events and exact
native readback. A clipped or covered field cannot borrow availability from
another part of its frame. Structural evaluation metadata remains read-only;
it does not replace a native selector or frame lease.

Frame discovery now distinguishes positive hidden-state evidence from clipped
or transparent content that still needs inspection. The latter retains the
pending-frame barrier even when zero fields were inspected. The native test
records when no control was available and no command was attempted; it does
not invent a selector to claim action-guard coverage.

The [CSSOM View specification](https://www.w3.org/TR/cssom-view/#dom-document-elementfrompoint)
defines hit testing in viewport coordinates and its out-of-viewport behavior;
its [bounding-box contract](https://www.w3.org/TR/cssom-view/#dom-element-getboundingclientrect)
provides geometry context. The implementation and supported cases are qualified
with the actual configured engine. Hit testing alone is not visual proof.
The reviewed crawl retained HTTP 200, substantive extracted text, provider
version and digest in `target/frame-viewport-ugouaf82/cssom-view.native.private.json`.

## Retained failures and focused checks

The original tall-frame failure remains in
`target/frame-viewport-baseline-mgwghrm1/inline-tall.private.json`: one authored
field, verified document/owner and an offscreen-center review outcome. The
corrected focused run enters and reads back that field, then closes its browser.
A separate focused case enters a field after a 2,500 px source offset; its native
post-entry image shows the complete label and value.

The first expanded run preserves its clipped-frame failure in
`target/viewport-matrix-ccjfet33/inline-tall_clipped.private.json`: zero inspected
fields and no original warning despite uncertain owner rendering. The corrected
focused run in `target/frame-viewport-fixed-clipped-ut0l0b58` retains explicit
review, zero entries and no invented command target. Earlier native observations
and images are unchanged. New wide, partly-above and nested-frame images were
also inspected during development; this is not local-model vision credit.

## Final native and compiled-app checks

The final 24-case native matrix passes: thirteen entry cases produce sixteen
exact readbacks, and eleven cases retain review without entry. All twenty-four
browsers close. This includes all fifteen earlier frame cases and nine new
geometry cases. Five new visible cases cover tall, wide, partly above the
viewport, nested tall and deeply offset fields. Four new review cases cover a
covering overlay, ancestor clipping, a wholly offscreen owner and transparency.
Clipped/transparent cases record zero inspected controls and no command attempt;
the offscreen case exposes a native control but rejects entry before its callback.

The canonical Windows workspace passes 1,371 tests, with 612 opt-in tests ignored
across 43 groups. Warnings-denied Clippy, the app build and formatting pass.
Native receipts: `target/viewport-final-l5c9nca7/inline-coverage.private.json`;
check/controller receipts: `target/frame-viewport-ugouaf82/validation-final.private.json`.

A separate compiled-app read-only preview of the clipped fixture returns one
view with zero inspected fields, zero omissions, explicit review and
`independent_entry.blocked: true`. It retains the pending-frame explanation;
zero fields do not become a clean inspection. The replay serves four cached
reads, with zero blocked or cached non-read replies and no external delivery.
No model starts, the shared runtime remains unchanged and the app tree drains.
Private evidence: `target/clipped-frame-preview-qmybho9s`. This is an actual compiled route check,
not a claim of a complete frontend/resize acceptance pass.

Current app SHA-256:
`d0cee28195a3ff1c405f118a7ff27f3cad79181a5333fc982e194eb8709af3f6`.

## Actual local-model scroll and visual qualification

A new development variation places the existing eight fictional questions after
a 2,400 px source spacer inside a 4,800 px same-origin iframe. The initial native
image shows the header and empty upper frame, with the questions below the
screen. The compiled read-only inventory finds all eight controls, zero
omissions and no walls. Independent source values, requiredness and structural
control ownership are frozen before inference; the answer key remains outside
the app/model workspace.

One actual compiled local `gemma4:12b-it-q4_K_M` run, context 4,096 and owned
runtime 0.40.16, enters and verifies both supported facts. All six required
unknowns remain untouched. Incorrect, ambiguous, unexpected, unverified and
unbound performed entries are zero. Both native readback and the local model's
blind visual checks pass; the developer also inspected the original initial and
post-entry images. Those developer observations do not supply model credit.
The exact model digest is
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.

| Measured observation | Result |
| --- | ---: |
| Verified supported answers / declared supported answers | 2 / 2 |
| Unknown answers entered / declared unknowns | 0 / 6 |
| Critical entry or uncertainty | 0 |
| Native peak app-job commit, bytes | 9,147,359,232 |
| Minimum sampled available host RAM, bytes | 18,372,354,048 |
| Maximum sampled whole-adapter WDDM usage, bytes | 14,457,454,592 |

The unchanged app limit is 10 GiB inside an 11 GiB qualifier, with shared
half-free-RAM admission and a 3 GiB reserve. Verified model caches are reused.
The owned model reports native process exit and an empty listener after stop;
the app tree drains and the shared runtime remains unchanged. WDDM is whole-
adapter sampling, not process attribution or continuous peak. Runtime API
VRAM figures have a different scope. This single development qualification is
not a model promotion, repeatability study, held-out result or workflow pass.
Private evidence: `target/viewport-frame-model-fqhvkss0`.

The final scoped host audit reports zero matching processes, resident models,
services, scheduled tasks, startup entries or observation errors, with
30,265,565,184 bytes of RAM available. Receipt:
`target/development-host-70f8f8af476e49848b6dda1ba96a80e6.private.json`.
This is measured teardown evidence, not sustained leak qualification.

## Remaining scope

Wholly offscreen, covered, clipped, transparent and target-less documents need
review. Arbitrary transformed layouts, complete applications, independently
model-scored held-out families and clean-host installation are not established
by this geometry repair. The historical unconfirmed Close remains unresolved;
later clean exits are not a diagnosis or sustained leak qualification. No live
employer interaction, application submission or release is authorized here.
