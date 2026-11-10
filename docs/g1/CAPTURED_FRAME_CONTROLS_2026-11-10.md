# Recorded cross-origin form controls

Checkpoint: 2026-11-10, Windows x64. This is an isolated browser-engine capability,
not a web-interface feature or permission to interact with employer forms.

## Mechanism

The driver can enumerate a bounded set of direct cross-origin targets inside an
existing frozen-response rehearsal. `captured_frames`, `enter_captured_frame`
and `leave_captured_frame` retain the actual parent session, document, owner
node, child target, loader and origin. Frame descriptions are driver-created
references; arbitrary JSON cannot manufacture one. Ordinary/live browser owners
cannot enter this mode. Enumeration inspects at most four offered frames from
32 owner candidates and checks an eight-second budget between candidates;
individual protocol calls retain their own timeouts. Partial coverage is explicit.

Every returned field/control selector carries a fresh scope lease. Raw child
CSS selectors, another entry's selectors, a changed document and a removed frame
are refused. Reinspection of a same-origin child navigation creates new leases;
it does not make older answers or approvals current. Parent identity, owner
visibility and child metadata are checked before field operations. Unknown or
conflicting metadata never falls back to the parent's URL.

Chromium restricts navigation history and screenshot capture to top-level
sessions. The URL fallback accepts only that exact protocol error and verifies
the child's frame metadata. Screenshots are rendered through the bound parent's
iframe box, with child and parent caret styles restored independently. They
represent the frame viewport, not its full document. Field recapture scrolls the
actual child field into view; tiled full-child capture remains unavailable.

Clicks verify the child control and its corresponding parent hit point. Mouse
events use parent-viewport coordinates, as specified by CDP. The parent point
is checked again after hover; covered, transformed, padded or otherwise
unverifiable frames remain manual. This narrowly enables a non-submit section
control in recorded rehearsal without weakening ordinary cross-origin click
refusal. Existing submit guards and rehearsal commit refusal remain in force.
No model-generated scripts are introduced.

## Native result and retained failures

The final selected native test executed once, with zero ignored cases, in
87.75 seconds (88.156 seconds including the controller/build invocation).
Private evidence:

- `target/replay-qualification-pcb6csch/result.private.json`, SHA-256
  `3503b6c2c47898ac199f6120b5c130de0bc3921ab249fc256136189283b376fb`.
- `target/replay-qualification-pcb6csch/native.private.json`, SHA-256
  `0349a1ea2b895fa7b07fd3c2def434c1eca45ca0d6af5b4655b1e69af5cdfd30`.

Six fictional control criteria passed: text, textarea, checkbox, select, PDF
attachment with content readback, and text entry after a section change. The
parent's independently typed value was retained. Separate negative criteria
refused raw/expired selectors, pre-reload selectors, a six-pixel overlay over
the actual click point while the iframe center remained clear, a transformed
frame and a removed frame. A refreshed post-reload field had a new selector
and its actual empty value was verified. Frame/follow-up images were produced;
the follow-up screenshot was inspected and shows the selected PDF, section
control and entered follow-up. The earlier four-field image was also inspected.
This is developer visual inspection, not local-model qualification.

The controller retained 175 whole-host RAM samples at 500 ms intervals; minimum
available physical RAM was 32,367,529,984 bytes. Its process job was limited to
4 GiB; the contained browser retains its existing 2 GiB/16-process cap. No model
was loaded. Browser/profile teardown completed and the source engine inventory
remained unchanged. These observations are not GPU/application peak figures or
a long-running memory-leak guarantee.

Initial probes remain private evidence: `replay-qualification-ofzk8w00` exposed
the child navigation-history limitation; `replay-qualification-59z11rdf` passed
four direct controls but could not capture a child-session screenshot.
`replay-qualification-2d4xl436` exposed the same restriction on `Page.reload`;
the fixture reload now runs in its own child context. The subsequent
`replay-qualification-fmmptswf` preserved the cross-origin click refusal rather
than silently passing the section case. `replay-qualification-j9jbxvys` passed
six controls before the final adversarial checks were added. Failed probes and
partial criteria are not counted as successful qualification.

The broader browser run exposed two additional defects: inline opaque
child-document inspections were outside the existing four-document budget, and
the forced-parent observer assumed particular executable names. Inspection now
counts both inline documents and separately attached targets. The observer
requires profile identity, a live ancestry chain with creation-time ordering,
and nonempty identity observations. Eight portable PowerShell cases cover
packaged/renamed executables, missing creation times, unrelated lookalikes,
cycles and PID reuse.

The initial overly broad opt-in run is retained in
`target/captured-frame-final-cdp-live.private.log`: 136 passed, five failed.
Three probes lacked their explicit private/public inputs and failed before
navigation; the observer and inline-budget cases were actual failures. The
existing scripted-submit diagnostic is not no-send evidence. The corrected
fixture-only CI selection passed all 137 cases. The first following tool run
exposed stale ancestry fixtures and missing-root identity handling; both were
corrected rather than weakening criteria.

Final ordinary workspace checks passed 1,241 tests with 584 opt-in cases ignored
across 43 reported targets. Workspace Clippy, formatting, the managed build,
137 browser fixture cases, the native caret-restoration case and all 52 tool
contracts passed. After the final observer-only guard/fixture correction, its
native forced-parent case, all tool contracts, browser Clippy/formatting and the
recorded-frame journey were rerun. Byte-level reconstruction verifies that the
product sources and other browser test bodies did not change; the unchanged
136 cases were not repeated. Evidence is
`target/captured-frame-verified-checks.private.json` (including the preserved
first tool failure) and `target/captured-frame-closure-checks.private.json`,
SHA-256 `bc84708bdb6272d96cce3e7f77ff5adf7707d7ff4982d99e4060962eb58c8cff`.
Individual immutable logs carry their own hashes.

The final host audit recorded zero matching processes, services, scheduled
tasks, startup entries or resident models, no observation errors and
33,385,619,456 available physical bytes:
`target/development-host-13ff475004e14656a524db72e942efc4.private.json`.
This is a liveness/headroom snapshot, not proof of long-running leak freedom.
Automatic approval review rejected deletion of one blank-browser test profile
left by the failed observer; it supplied only "blocked by policy". The folder
and readiness file remain, recorded in
`target/captured-frame-blocked-cleanup.private.json`. Older unbound temporary
fixtures were left untouched. Do not generalize the clean process snapshot to
complete temporary-directory cleanup.

Reproduce with an explicit installed engine directory:

```powershell
python -B tools/qualify_replay.py --engine <absolute-headless-shell-directory> --frame-controls
```

The selector refuses combining this suite with churn/alias selectors before any
runtime inventory, temporary directory or child launch; a portable regression
checks that denominator boundary.

## Remaining work

The frozen-frame API is not connected to the web interface or the local-model
application agent. Nested cross-origin targets, transformed layouts, full child
document coverage and unseen-platform behavior are unqualified. Existing walls,
unknown response status and unsupported controls remain explicit; this API does
not certify that a form is ready or authorize an action. Ordinary/live
cross-origin forms retain their manual route.

Next: qualify blind local-model review of frame-specific captures, then connect
scope selection, fresh review and immutable attempt records in the contained
workflow. Complete the [isolation gates](../ISOLATED_REHEARSAL.md) before any real
candidate entry. No employer form was interacted with, no application/email was
sent and no release was created.

## Primary protocol references

[CDP Page](https://chromedevtools.github.io/devtools-protocol/tot/Page/) specifies
frame identity/origin metadata. [CDP Input](https://chromedevtools.github.io/devtools-protocol/tot/Input/#method-dispatchMouseEvent)
specifies mouse coordinates in the main frame's viewport. Native schema and
Crawl4AI receipts are retained privately under `target/research/cross-frame-*`;
the protocol references are separate from measured qualification.
