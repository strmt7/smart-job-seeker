# Offline replay session lifecycle — 2026-11-10

Closed iframe sessions previously remained in the replay broker's live-session
inventory. An 80-replacement portable regression reached the 64-session ceiling
and failed with `replay_target_budget_exceeded`. That ceiling now applies to
live sessions: detachment retires the interceptor, its aliases and descendants,
releases target mappings, and requires fresh interception before reattachment.

Pending attachment callers receive explicit errors. Retired command replies
cannot resume a replacement target or increase served/blocked success counts.
Late control reads return a stale-session error rather than old values. A lost
accessibility read returns an unavailable outline, never its later values.
Invalidated related browser sessions are explicitly detached as well; removing
bookkeeping alone is insufficient when the underlying target remains alive.
Only Chromium's explicit missing-session response is an idempotent close;
unrecognized cleanup errors stop the replay rather than claiming success.
History remains bounded at 4,096 retired session identifiers and 512 outstanding
retired command identifiers; exhaustion is an explicit failure. Reports expose
counts, not identifiers, candidate values or URLs. Existing live-command,
process, memory and request limits remain in force. No live-fetch fallback was
added.

The official [Target protocol](https://chromedevtools.github.io/devtools-protocol/tot/Target/)
describes detach events per session, including multiple sessions for one target.
The broker consequently distinguishes an alias detaching from losing the
primary interceptor. Its retained native research receipt is
`target/research/offline-target-lifecycle.native.json`, HTTP 200, SHA-256
`61dcd98d92e6bc486d436850a6b81e8edf8b8e8c12ca0728a6cc028836be0c97`.

## Native process mode and retained failed probe

The first native fixture completed 80 frame loads inside the parent target.
It correctly failed the required target-retirement assertion: zero sessions
were retired. Evidence is `target/replay-qualification-8v0ch9pz/`; this is not a
successful detach qualification or a demonstration of the old implementation.

The contained worker now explicitly requests `--site-per-process`, Chromium's
[documented all-site process mode](https://www.chromium.org/developers/design-documents/site-isolation/).
This makes cross-site renderer separation explicit for the selected headless
shell. The browser remains headless, GPU-disabled and inside the zero-capability
AppContainer and its existing 2 GiB/16-process job. Extra renderer processes can
still reach these caps on complex pages; failure remains visible. The official
reference receipt is `target/research/offline-site-isolation.native.json`, HTTP
200, SHA-256
`73bc1520bf287d50e8555f4aaebb1e760b1cfabf241b8157b331405070b9f585`.
No host browser configuration or service was changed.

## Measured checks

| Check | Result and scope |
| --- | --- |
| Portable replay/archive/discovery | 39 passed, including 80 frame replacements, detach during setup, aliases/descendants, pending reads, stale acknowledgements, physical-close errors, fresh reattachment and metadata budgets |
| Portable worker protocol/ownership/storage | 14 passed |
| Native recorded replay | One executed, zero ignored; 80 cross-site iframe replacements and a later inspected frame completed, 82 retired sessions, two active sessions/targets, zero outstanding retirements |
| Native alias lifecycle | One executed, zero ignored; losing an interceptor closed two related sessions, a separate Chromium request confirmed the alias was absent, fresh interception retained the typed name, and the profile was removed |
| Existing native entry assertions | Name/email, recorded next-step response, PDF upload, later field readback, refusal of an uncaptured scripted request and refusal of commit all passed |
| Native containment/recovery | Five Rust checks and six Windows lifecycle tests passed, with private control-engine cleanup confirmed and the source runtime unchanged |

The contained replay command took 32.599 seconds, including Cargo startup/build
work. Its 65 whole-host RAM samples retained at least 33,777,410,048 available
bytes. The native report counted 88 served recorded responses, one refused
uncaptured request, and one unsupported worker target. That limitation remains
visible; no service-worker or arbitrary-worker replay coverage is inferred.
The completed screenshot was inspected directly and the profile was removed.
Evidence is `target/replay-qualification-58pnvheg/`, native report SHA-256
`46023fe487488ff656a5ebef132b613f9c6265ec132dad48963e5eed74760a8a`.

The final two-case run is `target/replay-qualification-s4hztkmm/`. Its contained
frame command took 40.341 seconds and its separate alias command 25.988 seconds,
including Cargo work. The 132 host-RAM samples retained at least 29,014,503,424
available bytes. Both profiles were removed and the engine inventory was
unchanged. The frame report SHA-256 is
`940ad18f3d0b56e19a3234941d13fbc49bfe684c9cf3a6f048aae7984345efe8`;
the alias report is
`c646bffc46d39ae801f29cf088eae02e0ef7980ecf667b9f511e050611bc5487`.
The final screenshot was inspected directly. Cross-origin form controls still
return the existing manual-review wall and are not exported as usable fields;
that contract is asserted in the native fixture. An intermediate probe expecting
such a field failed and remains in `target/replay-qualification-t67qgbqa/`.
Physical alias cleanup is tested through private native CDP controls, not a new
product API or a claim that cross-origin autofill is complete.

The separate containment/lifecycle receipt is
`target/rehearsal-qualification-1afj0zox/result.private.json`, SHA-256
`00e5bf8cfd969ffae48c8345fd0f0581f2d05244dc5803b404ed74ce4988e95c`.
Its resource observations cover four normal closes: maximum observed browser-job
commit peak 77,873,152 bytes, minimum sampled host availability 32,788,193,280
bytes. These are neither full-workflow physical/GPU peaks nor a long-running
memory-leak qualification. Late acknowledgement races are portable protocol
cases; this successful native churn run observed none.

Reproduce with explicit installed engine paths:

```powershell
python -B tools/qualify_replay.py --engine <absolute-headless-shell-directory> --frame-churn --alias-lifecycle
python -B tools/qualify_rehearsal.py --engine <absolute-headless-shell-directory>
```

The final ordinary workspace passed 1,238 tests with 583 opt-in cases ignored
across 43 reported targets; workspace Clippy, formatting and the managed app
build passed. Evidence is `target/offline-alias-final-test.private.log`,
`target/offline-alias-final-clippy.private.log` and
`target/offline-alias-final-build.private.log`. The added private native alias
test accounts for the extra ignored case; its separate selected run executed
one test with zero ignored. No dependency graph or web-interface code changed,
so unrelated model and layout suites were not rerun. The earlier containment
receipt covers unchanged schema-1 containment/recovery mechanisms, separately
from the latest schema-2 replay/alias changes.

All interactions used fictional recorded/file/loopback fixtures. No employer
form was interacted with, and no application or application email was sent. These checks do not prove
general no-egress, real-site completeness, local-model transfer or release
readiness; the [remaining isolation gates](../ISOLATED_REHEARSAL.md) still apply.
