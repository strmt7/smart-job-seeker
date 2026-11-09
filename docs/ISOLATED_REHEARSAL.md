# Isolated browser rehearsal

Status: experimental Windows development interface, 2026-11-09. This mode is
not enabled in the application interface or qualified for employer applications.
It does not establish a general no-egress guarantee. Setup failure never selects
the ordinary browser as a fallback.

## Mechanism and budgets

`CdpDriver::launch_rehearsal` starts an embedded, standard-library Python worker
in an owned process group. The worker creates a unique zero-capability
AppContainer, copies checksum-bound engine/snapshot files into its private
directory, starts headless shell suspended **already assigned to its job**, checks its token before resuming it,
and periodically checks observed job members for the same identity and zero
capabilities. `--no-sandbox` is confined to this outer container. The earlier
ordinary-Chrome probes failed in Crashpad initialization; this interface requires
the separate headless-shell engine.

CDP uses inherited pipes with an explicit handle list and separate stderr.
There is no debugging listener, HTTP proxy or network broker. The existing
driver performs DOM inspection, real input events, section navigation and
screenshots. Downloads are denied; `commit` refuses isolated sessions before
inspecting or clicking a submit control. Hashing binds selected files but does
not authenticate their publisher. Inventories reject reparse points, case
aliases, traversal, Windows device names, unsupported content and changed bytes.

- One session per app process and a per-user-SID mutex in the global Windows
  namespace. Abandoned-mutex reacquisition is tested; simultaneous interactive
  Windows sessions have not been qualified.
- Browser job: 2 GiB committed memory, 16 processes, below-normal priority and
  termination when the owning job handle closes. Controller group: 3 GiB.
- Admission requires 6 GiB free physical RAM; periodic samples below 2 GiB stop
  the worker. These are checks, not reservations or measured peaks.
- Engine copy at most 600 MiB, content at most 64 MiB; disk admission retains
  1 GiB beyond the copy. A bounded metadata walk samples profile growth every
  two seconds: above 1 GiB logical file bytes, 10,000 entries or below 512 MiB
  free disk stops the worker. This is monitoring, not a filesystem quota;
  growth between samples remains possible.
- Frames at most 16 MiB; response queue at most 64 MiB/256 messages; bounded
  command deadlines and lifetime. Session output at most 100,000 lines/256 MiB.
- Normal close, malformed commands and stdin EOF terminate the owned browser
  and check profile deletion. A fresh bounded recovery worker handles controller
  failure; the next session also recovers records left by whole-parent death.

## Crash recovery

A private directory under the current user's local application data contains
write-ahead ownership records. Its protected DACL grants the current Windows
user and SYSTEM access. Records contain process/profile ownership metadata, not
applicant material. A flushed intent record precedes AppContainer creation;
atomic replacement records the browser's original PID and creation time before
its first thread resumes. Browser membership uses Windows 10+'s
[job-list process attribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute),
so interruption between creation and recording cannot leave an unassigned
suspended browser. [Global kernel namespaces](https://learn.microsoft.com/en-us/windows/win32/termserv/kernel-object-namespaces)
and [protected directory permissions](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow)
are separate from candidate database encryption.

Recovery queries the original identities and named job before deleting the
derived, application-owned profile. A reused PID is never permission to stop
its new owner. Live original processes block admission; unknown, corrupt or
mismatched ownership is retained and counted in readiness evidence. Recovery
has a bounded inventory and time budget, rejects reparse points, and never
terminates by a saved PID. A Rust transport failure drops its old process group,
then starts a separate 256 MiB/15-second recovery worker before releasing local
admission. The owner thread retains its embedded script until that operation
finishes. This cleanup does not resume an application or authorize a submission.

Records support interrupted startup and process death, not a demonstrated
power-loss durability guarantee. A directory beyond the recovery scan budget,
unknown access rights or an uninspectable process remains pending. Recovery
currently runs on session startup/failure; general app startup without a
rehearsal session does not invoke it.

ProcessKit's fail-loud policy counts cumulative output. An initial 128-line cap
discarded later DOM replies and stalled section two. Session and queue ceilings
are now distinct and exhaustion is actively reported. Qualification exceeds
that initial line count; no dropped protocol message is accepted as success.

## Local evidence and remaining gates

Checks used `HeadlessChrome/157.0.8103.38`, selected from official Chrome for
Testing stable-channel JSON on 2026-11-09. Its Windows x64 archive was
126,513,060 bytes; extracted files totaled 296,336,984 bytes. Archive SHA-256:
`1ff4540855478ba0848e03f1ef8d455f77d058b38285df91bc34273788a73446`.
This identifies tested bytes, not an independent publisher signature check.
Native metadata, archive, screenshots and logs remain under ignored `target/`.

The Rust driver filled/read back fictional Unicode text, opened another
section, filled its email field, refused a missing attachment, attached a
fictional PDF and verified its bytes through browser-side SHA-256 readback,
captured a visually inspected screenshot and removed its profile. A protected
host-only PDF could not be read through the contained browser's file input,
while the approved in-container PDF passed byte-hash verification. Matching
ordinary-browser scripts reached a fictional
receiver through fetch POST, beacon, WebSocket handshake, EventSource request,
image, iframe, popup, scripted form POST and dedicated-worker fetch. The isolated
run added no receiver requests during the observation window. An incomplete
ordinary-browser baseline was retained as a failed check. The control now uses
the **same headless-shell binary and reported version** as the isolated session,
and both use the shared concurrent HTTP fixture. The previous separate serial
receiver was removed; idle preconnections and fragmented bodies are covered by
fixture regressions. Earlier failures also involved installed Chrome on a
different build. These repairs remove concrete confounds, without proving an
exclusive cause for every missing request. All nine channels remain mandatory,
and a completion marker confirms the isolated script reached its last attempt.

The latest combined qualification passed six Rust checks and six Python lifecycle
methods. It covers malformed commands, stdin EOF, concurrent refusal, refusal
to recover an active session, hard-kill cleanup on the next session, abandoned
mutex recovery, and Rust event-consumer disconnection. One lifecycle method
checks three interruption points: before profile creation, after profile
creation, and immediately after atomic process creation but before saving its
browser stamp or resuming it. Recovery uses production code; test-only profile
deletion was removed. Thirteen portable checks cover protocol, ownership,
corruption, live identities and storage limits. Twenty-two development-tool tests
passed, including denominator checks, receiver-history validation and cleanup
failure handling. Seven shared-server regressions, two relay protocol/receiver
tests and three focused browser ownership/shutdown checks also passed. Browser
library checks passed 29 tests, with four opt-in checks ignored; Clippy and the
managed Windows build passed.

A private source-bound combined receipt has SHA-256
`136c92ba5629d8d8115a80769ee28abc9c6444c418482e95bfe59da3dd6aeb77`.
Its four normal-close **minimal lifecycle sessions** measured a maximum browser
job commit peak of 59,408,384 bytes (56.66 MiB), minimum sampled available host
RAM of 27,055,849,472 bytes (25.20 GiB), and maximum sampled profile size of
296,337,854 logical bytes (282.61 MiB, including the engine copy). These are not
full form-session, document-processing, local-model, GPU or whole-host peaks;
interrupted processes are excluded from this resource denominator. Windows job
peak accounting is queried after owned processes terminate.

The nine HTTP-related mechanisms also passed on IPv6 loopback. Separate
WebRTC data-channel probes generated TURN Allocate requests over UDP and TCP
on both IPv4 and IPv6 loopback with the unrestricted control; isolation added
no packets or connections. These receivers do not relay media or contact a peer.
Loopback restrictions are distinct from Internet capabilities: these results
alone cannot establish a public-network boundary.

An explicitly opted-in public HTTPS check used a temporary free receiver with
only random fictional markers. Unrestricted controls before and after isolation
each delivered one POST; the receiver reported zero isolated arrivals across
six inspections spanning ten seconds after both controls were visible. Browser
results independently reported control success and isolated failure. The
15-minute receiver was deleted with a confirmed API response. This is one
HTTPS destination and observation window, not qualification of every Internet
channel. No account, API key, candidate material or application was used. Service
request metadata is not retained in the receipt; private capability URLs remain
under ignored `target/`. Receiver failures, incomplete history, missing controls
and unsuccessful cleanup cannot yield a complete receipt.

The unrestricted control now runs from a separate hash-verified execution copy;
every selected engine file must match the isolation manifest. Chromium's network
utility wrote `debug.log` beside its executable during early relay checks even
with logging flags. Those inventory failures are retained privately; only that
generated extra file was quarantined after verifying all original file hashes.
The immutable source inventory was not weakened or expanded to accept it. The
latest run confirmed unchanged source bytes and deleted the control copy.
Windows process ownership now preserves explicit environment overrides and
working directory; diagnostic logs are directed to owned profiles, with bounded
isolated-log monitoring. Logging flags alone are not claimed to prevent every
child-process diagnostic file.

Public UDP/WebRTC, DNS-specific paths, external protocol handlers, broker
channels, power-loss behavior and all possible control/parent failure schedules remain unqualified.
Permitted-asset capture, server-dependent replay, local-model visual reliability,
installation and updating also remain open. No real form was interacted with
and no application or email was submitted. No whole-pipeline RAM/GPU peak is claimed.

Before interface integration, extend recovery beyond the measured schedules,
qualify non-loopback/broker channels and runtime installation, and decide how
pending ownership is surfaced. Extend the channel and disconnect matrix before permitting any
captured-site interaction. Do not infer recovery ownership from a PID alone.

## Reproduce headlessly

```powershell
py -3.13 -X utf8 -B tools/qualify_rehearsal.py --engine <absolute-headless-shell-directory>
# Optional: temporary public HTTPS receiver; random fictional markers only:
py -3.13 -X utf8 -B tools/qualify_rehearsal.py --engine <absolute-headless-shell-directory> --public-egress
py -3.13 -X utf8 -B crates/waypoint_browser/tests/test_rehearsal_worker.py -v
```

The default command creates fictional content, streams file hashes and runs five
opt-in Rust checks plus six lifecycle methods sequentially; the public option
adds one Rust observation check and independent receiver verification. It owns a 4 GiB job,
enforces time/log budgets, verifies unchanged source hashes, and retains private
results, explicit executed/ignored denominators, scoped resource observations
and a PNG in `target/rehearsal-qualification-*`. It does not download
engines or alter shared caches. `--python` selects an absolute interpreter; the
embedded worker targets Python 3.12+, while this development command uses 3.13.
The thirteen portable protocol/ownership/storage tests also run in CI. Opt-in live
checks must not be counted as executed when skipped.

Protocol references: [TURN URI syntax](https://www.rfc-editor.org/rfc/rfc7065),
[TURN Allocate framing](https://www.rfc-editor.org/rfc/rfc8656),
[WebRTC relay policy](https://www.w3.org/TR/webrtc/), and the receiver's
[free token API](https://docs.webhook.site/api/tokens.html) and
[request history API](https://docs.webhook.site/api/requests.html). Native
research captures are retained privately; no third-party implementation was copied.
