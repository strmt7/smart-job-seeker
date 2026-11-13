# Inline form inspection and entry - 13 November 2026

This continues R03 on the existing product goal. Accessible inline documents
previously exposed their fields while an unconditional frame warning prevented
entry. The browser now distinguishes discovery, native identity, completed
inspection and field entry. It does not grant submission permission.

## Implementation

Each document inspection returns a typed result with deferred frame observations.
The traversal records an owner/document pair only after inspecting that document.
A separate completeness certificate requires the exact set of visible accessible
owners to match those completed inspections. The native graph, document and
loader identities, owner ancestry/attributes, rendered availability and origin
observations must remain unchanged across traversal. Duplicate, missing,
uncertain, covered, opaque or over-budget scopes cannot produce that certificate.
Positively hidden owners remain explicit native observations, not guessed absent
siblings. Existing frame/document/depth/control/time limits remain bounded.

The generic pending-frame warning is emitted when certification fails. No warning
strings are filtered away. Missing native identities and all other page-wide,
challenge, login, custom-widget, hidden-required and shadow-document barriers
remain. Independently targeted children retain their separately leased path;
this engine can omit them from the parent session's frame tree. Target-less
cross-origin children still need review.

HTTP documents require an observed successful response bound to the exact native
frame and loader. The existing cached main-document observation is reusable only
for that same pair. Missing or failed HTTP observations never become success.

`srcdoc` and `about:blank` documents have a distinct source contract: matching
native owner/document identity, correct source attributes, actual parent access,
verified effective origin and a verified ancestor representation. The engine
reports `://` as the protocol origin for these inherited documents; that raw
observation is retained alongside a separately observed accessible origin.
Conflicting or missing origin evidence refuses certification. Explicitly empty
loaders are supported only for these inherited non-network representations;
missing loaders and HTTP-document loaders remain unknown. No HTTP response is
invented for an inherited document. Data URLs are not admitted by this contract.

The [HTML iframe specification](https://html.spec.whatwg.org/multipage/iframe-embed-object.html#attr-iframe-srcdoc)
provides the source-representation design context. Native qualification determines
what this particular engine supports. A bounded fixed owner probe consolidates
repeated rendering/access/hit-test observations; this is not a controlled latency
benchmark or a universal performance claim.

The resulting native evidence participates in complete fresh-form comparison.
A stale form can therefore be refused even when its field schema and displayed
review text have not changed. Source identity alone still cannot certify an
uninspected document, authorize a model action or replace visual/value checks.

## Qualification method

Fictional archived HTML runs inside the configured headless engine with native
AppContainer evidence and zero capabilities. Native entries use the existing
review callback, current-form comparison, input events and exact readback. The
model is not used as an answer oracle. Same-ID controls in sibling documents must
remain separate; entering one must leave the other unchanged.

The expanded matrix covers nested/sibling forms, a hidden sibling, mixed opaque
and independently targeted siblings, scope/depth exhaustion, a covering overlay,
a failed child response, an unobserved child response, inherited documents,
reloads, owner replacement and state-preserving reparenting. The missing-response
case disables the real response observer before reloading the child; it neither
edits retained events nor fabricates a response. Production inspection does not
disable that observer.

The initial reparenting fixture used an ordinary DOM move, which reloads an
iframe and changes its document/schema. It did not isolate identity from schema.
The corrected fixture feature-detects a state-preserving move, asserts equal
schema/review text, then proves the old entry is refused before its callback and
a fresh inspection permits correct entry. This distinction follows the
[Chrome engine documentation](https://developer.chrome.com/blog/movebefore-api)
and is qualified on the actual configured engine. No production barrier was
weakened to satisfy the fixture.

## Validation status

Final Windows validation: **1,371 workspace tests passed, 611 opt-in tests
ignored**, across 43 result groups. Warnings-denied Clippy, app build and
formatting pass. The fifteen-case native matrix passes: eight entry cases with
ten exact native field readbacks, seven explicit no-entry review cases, and all
fifteen browsers closed. Reload/replacement/reparenting each refuse the stale
entry before its callback; reparenting separately proves equal schema and review
text. The four-context radio integration also passes: root, same-origin inline
and independently targeted cross-site choices verify; target-less same-site
entry remains review. All four browsers close. This is three native choices and
one refusal, not four completed applications.

`srcdoc`, `about:blank` and nested field captures were decoded and visually
inspected. Labels and the actual entered values remain visible. Those human
image checks are not local-model observations. No new model was loaded, and
these native results do not establish local-model accuracy, whole-workflow
completion, held-out performance or real employer behavior. Earlier development
reports retain their original source identity and case definitions.

Final scoped host audit found zero matching development processes, zero shared
resident models and zero matching service/task/startup entries, with no errors
and 28,081,995,776 available RAM bytes. Earlier shared model inventory was
unavailable; that transient observation was retained rather than called empty.
No model was loaded by this work. Teardown and sampled headroom are not proof of
sustained leak-free behavior.

The first expanded run verified its two native entry cases but did not confirm one
browser shutdown. A later isolated repeat and subsequent matrix closed their
browsers; the original shutdown failure remains unexplained under R01. Shutdown
errors now preserve the already-recorded native owner failure detail without
changing the failure result or deadline. Original reports and interrupted runs
remain immutable. A missing process handle and no per-case results receive no
success credit.

All private artifacts remain under `target/inline-frame-completeness-dzv5zs30/`
and identity-bound `target/inline-native-*` folders. Official-page research
retains native URL/status/error, substantive content, provider version and
capture/content digests. No dependency, global service, scheduled task, paid API or visible desktop
interaction is introduced by this change. Temporary browser profiles and
processes remain owned and subject to explicit qualification cleanup.

Next: independently stable logical control keys for inline-frame evaluation,
then compiled local-model frame qualification and target-less child capability.
Current report selectors retain temporary inline backend identities, which must
not become a brittle held-out answer key. R03, R02/R04 and the overall goal remain
unfinished. No real application or application email was sent, and no release
was created.
