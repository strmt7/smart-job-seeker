# Isolated browser rehearsal

Status: experimental Windows development interface, 2026-11-09. This mode is
not enabled in the application interface or qualified for employer applications.
It does not establish a general no-egress guarantee. Setup failure never selects
the ordinary browser as a fallback.

## Mechanism and budgets

`CdpDriver::launch_rehearsal` starts an embedded, standard-library Python worker
in an owned process group. The worker creates a unique zero-capability
AppContainer, copies checksum-bound engine/snapshot files into its private
directory, starts headless shell suspended, checks its token before resuming it,
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

- One session per app process and a named mutex within the Windows logon session;
  cross-session admission remains open.
- Browser job: 2 GiB committed memory, 16 processes, below-normal priority and
  termination when the owning job handle closes. Controller group: 3 GiB.
- Admission requires 6 GiB free physical RAM; periodic samples below 2 GiB stop
  the worker. These are checks, not reservations or measured peaks.
- Engine copy at most 600 MiB, content at most 64 MiB; disk admission retains
  1 GiB beyond the copy. Continuous profile disk quotas remain open.
- Frames at most 16 MiB; response queue at most 64 MiB/256 messages; bounded
  command deadlines and lifetime. Session output at most 100,000 lines/256 MiB.
- Normal close, malformed commands and stdin EOF terminate the owned browser
  and check profile deletion. Forced worker death closes its browser job, but
  automatic recovery of remaining profile files is still missing.

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
captured a visually inspected screenshot and removed its profile. Matching
ordinary-browser scripts reached a fictional
receiver through fetch POST, beacon, WebSocket handshake, EventSource request,
image, iframe, popup, scripted form POST and dedicated-worker fetch. The isolated
run added no receiver requests during the observation window. An incomplete
ordinary-browser worker baseline was retained as a failed check; all nine
channels remain mandatory, with browser/worker failure diagnostics.

Four worker checks passed: malformed-command cleanup, stdin-EOF cleanup,
concurrent-instance refusal without disturbing the first session, and owned root
browser termination after hard kill. The last check holds the original process
handle and performs **test-only** profile recovery. It does not prove automatic
production recovery or every descendant/startup race.

Non-loopback hosts, IPv6, DNS, UDP/WebRTC, external protocol handlers, broker
channels, interrupted startup and all control/parent failures remain unqualified.
Permitted-asset capture, server-dependent replay, local-model visual reliability,
installation and updating also remain open. No real form was interacted with
and no application or email was submitted. No peak RAM/GPU measurement is claimed.

Before interface integration, add durable ownership records before profile
creation, bind worker/browser identities to process creation times, eliminate
the suspended-process/job-assignment crash gap, and recover only proven inactive
owned profiles. Extend the channel and disconnect matrix before permitting any
captured-site interaction. Do not infer recovery ownership from a PID alone.

## Reproduce headlessly

```powershell
py -3.13 -X utf8 -B tools/qualify_rehearsal.py --engine <absolute-headless-shell-directory>
py -3.13 -X utf8 -B crates/waypoint_browser/tests/test_rehearsal_worker.py -v
```

The first command creates fictional content, streams file hashes and runs two
opt-in Rust checks plus four lifecycle checks sequentially. It owns a 4 GiB job,
enforces time/log budgets, verifies unchanged source hashes, and retains private
results and a PNG in `target/rehearsal-qualification-*`. It does not download
engines or alter shared caches. `--python` selects an absolute interpreter; the
embedded worker targets Python 3.12+, while this development command uses 3.13.
The six portable protocol/copy/configuration tests also run in CI. Opt-in live
checks must not be counted as executed when skipped.
