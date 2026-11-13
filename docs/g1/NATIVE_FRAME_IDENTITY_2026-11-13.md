# Native embedded-document identity - 13 November 2026

This continues R03 under the existing product goal. It adds identity evidence;
it does not complete embedded-document inspection or enable additional form
entry. Same-origin and target-less same-site forms still require review.

## Implementation

The browser can now bind the native frame graph to document backend identities,
loader identities, owner backend identities, owner ancestry and attributes.
The graph and DOM census are bounded to 16 frames, four nested frame levels, 20,000 DOM
nodes and 128 DOM levels. Owner lookups have the existing one-second command
timeout within an eight-second lookup budget. Missing, duplicate, mismatched,
opaque or over-budget observations are unavailable evidence.

An embedded-scope inspection observes this identity before and after traversal.
A changed previously verified identity adds a review barrier. An unchanged
identity becomes part of the private inspection evidence, so a fresh form can
differ from an old one even when supported fields and displayed review text
are identical. Selector namespacing preserves the identity and existing barriers.
The evidence is boxed to keep form/result stack sizes bounded; no lint rule was
disabled. A named inspection result carries the additional native evidence.

Native frontend node handles are deliberately excluded from the stable identity.
They can be refreshed while the backend owner remains the same. Each owner is
verified using the actual `DOM.getFrameOwner` backend result; a frame identity is
never used as a fabricated target identity.

The engine's parent-session frame tree omits the independently targeted
cross-site child in the fictional native case. That produces unavailable census
evidence, while the existing frame review and separately leased child-target
path remain intact. It must not be treated as an inspected inline child. An
earlier implementation incorrectly added a new parent barrier for this case;
the native regression caught it. Both failed runs and their actual observations
remain private and immutable.

## Qualification scope

The native matrix retains four distinct outcomes: main-page and independently
targeted cross-site radio selections verify; same-origin and target-less
same-site cases remain explicit no-entry review outcomes. Native identity
reobservations are stable in the supported inline contexts. Hiding and replacing
the same-origin owner change its identity and make its old form differ from a
fresh inspection. These mutation checks demonstrate snapshot differences; they
are not successful form entries or evidence of completed scope inspection.

Pure regressions cover loader/document/owner/ancestry/hidden-attribute changes,
frontend-handle refresh, missing and duplicate identities, opaque descendants,
graph/DOM bounds and identical-looking forms with different native scope
identities. Existing recorded-entry checks compare the complete fresh form.

The separate [choice/radio model checkpoint](CHOICE_CONTEXT_AND_RADIO_CAPTURE_2026-11-13.md)
now has two post-crop local 12B passes on committed source `32d2bec`: each verifies
2/2 supported English facts and leaves six required unknowns untouched. Those
are development repeats on the prior exact binary, not model qualification of
this new frame mechanism. The French case remains 6/7. Whole-workflow and
held-out qualification remain open.

Private evidence is retained in `target/residence-review-fm9o0g4o/` and the
identity-bound `target/frame-identity-native-*` folders. Public protocol research
in the former folder retains native captures of the official
[DOM](https://chromedevtools.github.io/devtools-protocol/tot/DOM/),
[Page](https://chromedevtools.github.io/devtools-protocol/tot/Page/) and
[Target](https://chromedevtools.github.io/devtools-protocol/tot/Target/)
documentation. Native engine observations determine the qualified scope.

Next: build per-scope inspection-completeness evidence on these identities,
cover mixed opaque siblings, nested scopes, quotas, navigation swaps and stale
leases, and only then consider discharging an individual frame barrier. No
warning was deleted, no missing target was invented, and no source, challenge,
replay, write or submission boundary was relaxed. No real application form was
interacted with and no release was created.

## Final local checks

Windows workspace: **1,365 passed, 610 explicitly ignored**, across 43 result
groups. Warnings-denied Clippy, Windows app build and formatting pass. Final
native qualification on the final candidate source runs one selected test
with all four context outcomes above; all browsers close. A test-only report key
was clarified after the workspace run and received final native compilation,
execution, formatting and Clippy. No production behavior changed afterward.
No dependencies changed, and no unchanged UI tests were rerun for this backend
slice. This is not a new whole-interface or accessibility qualification.

Final scoped host observation: 28,679,987,200 available RAM bytes, zero matching
processes, zero shared resident models and zero matching services, scheduled
tasks or startup entries; no audit errors. Both exact owned model creation
identities were independently rechecked exited. An earlier audit included its
own controller; that PID was verified absent before the clean final observation.
These checkpoints do not prove sustained leak-free behavior. Private artifacts
retain source hashes, actual test-executable hash, command/log identities and
all failed native/lint attempts. No hosted result is claimed for this local
continuation; the last verified hosted source remains `22c682c`.

The Windows build helper now refuses undeclared arguments before dependency
setup. Two routing regressions cover an unsupported argument and a misspelled
filter; the existing valid-command and environment-restoration contract suite
passes under the discovered PowerShell 7.6.9 runtime, followed by real Clippy.
The first contract-suite invocation through Windows PowerShell 5.1 could not
autoload `Get-FileHash` in this inherited development environment. That failure
is retained; PowerShell 5.1 contract qualification in this environment remains
open under R13. No host module path, global runtime or system configuration was
changed to hide it. This does not claim that all Windows hosts are qualified.
