# Background source-page copies

Checkpoint: 2026-11-10. In a job's **Details and records**, **Save page copy**
now runs the public-resource coordinator in the background. The interface shows
stages, supports stopping, recovers an active task after reload and exposes its
saved outcome in application records. This is a source archive, not an
application-completion or submission feature.

## Collection and records

The parent checks the selected job's source permission and public HTTPS URL.
It refuses known candidate identifiers, user information in the URL and common
authentication parameters. This conservative check is not proof that an
arbitrary URL contains no personal information. The worker independently checks
the URL/source binding and accepts no profile, cookies, credentials, request
headers or form bodies in its request.

[Shared URL privacy checks](CAPTURE_URL_PRIVACY.md) now also guard redirects,
robots requests and browser-discovered resources before transport. Known private
keys/email-like references are refused; decoding and nesting uncertainty remains
reviewable. The parent checks known candidate identifiers in the bounded decoded
entry. Unknown opaque identifiers and arbitrary private data remain unassessed.

The collector uses the existing policy/robots/redirect gates, bounded anonymous
GET transport and separate schema-3 render-only discovery. Only the initial
origin is declared; another origin remains a review gap. Collection cannot use
the candidate-entry driver. No raw saved HTML is served on the app's origin.

Each operation creates an application-record folder containing the original job
snapshot, existing material snapshot when available, an event journal and
`source-page-summary.json`. Its `source-page/` subfolder holds bounded stage
files, the response archive and a frozen inventory if collection succeeds.
Cancellation and errors preserve available files. Finishing the task does not
change the job's application state or create an approval.

The interface distinguishes archived resources from an incomplete copy, shows
unassessed coverage and flags missing resources. Unopened sections, authenticated
content, server validation and later application steps are not verified. The
history projection keeps source copies separate from extracted-listing captures.
Known URL changes invalidate the cached result's current-source flag; an
unverifiable source remains unknown.

Only one copy runs at a time; competing browser operations are refused. The
client shares a single poll across rerenders. Expanded job details survive a
refresh, and a new capture remains disabled until the completion refresh ends.
The server keeps the latest terminal result for ten minutes. Durable attempt
records survive a server restart; restarting a worker automatically is not
implemented. A record left in progress after termination needs review.

## Dependencies and containment

Missing default Python/PDF tools and headless Chromium are provisioned
automatically through the existing [private runtime setup](RUNTIME_SETUP.md).
Verified distributions are staged under the current user's control directory;
the operation does not install a host Python, edit PATH or replace another
application's packages. Existing distributions remain intact on setup failure.
Network, disk, OS execution permissions and supported Windows architecture are
still prerequisites. Clean-Windows qualification remains open.

The capture worker starts before the normal UI/tray entry point, inside an owned
Windows process group with a 25-minute outer timeout and capped output. Admission
requires at least 5 GiB available physical RAM. The worker's aggregate committed
memory ceiling is 2–4 GiB, preserving a nominal 3 GiB admission reserve. Available
host RAM is checked every 500 ms; falling below 3 GiB stops only the owned group.
Cancellation waits for worker exit; unconfirmed shutdown is reported as failure,
not successful cancellation. The collector's shorter request/session budgets
remain in effect.

The qualification runner now also checks a 3 GiB host reserve during work and
closes its owned containment on pressure or failed counters. Neither the job
ceiling nor periodic observation reserves host RAM or prevents another
application from consuming it between samples. No unrelated process is stopped.

## Qualification

The explicit developer command requires an already built app, a test-owned
managed-runtime directory inside ignored `target/`, and a Chromium executable
capable of headless operation:

```powershell
./apps/web_app/scripts/build-windows.ps1
py -3.13 -X utf8 -B tools/qualify_source_copy.py --runtime-control <absolute-test-owned-control-directory> --browser <absolute-chromium-executable>
```

It copies only selected test runtimes into a disposable user-control folder,
uses a fictional job record and reads the public RFC 9309 reference page. No
employer application form or candidate information is used. The actual rendered
client starts and stops a worker, creates another copy, recovers after reload,
checks both history outcomes and verifies the job stays discovered. The browser
is headless and its web requests are limited to the loopback app. Only the native
collector reads the public reference.

Nine layout cases cover physical widths 320, 700 and 1280 px with actual browser
zoom from 80% through 400%, retaining at least 320 CSS pixels and 240 CSS pixels
of height. Cases below the app's existing minimum are excluded, as in the main
layout matrix. Surface screenshots avoid a zoom/scroll clipping error observed
with the higher-level screenshot helper. Geometry checks alone did not detect
that blank-image problem; visual inspection remains required.

The first rendered run found a real disclosure-collapse bug, now fixed. A later
run encountered host-wide RAM pressure despite its owned process ceiling. No
Smart Jobseeker test process remained afterward; unrelated WSL and Windows drive
optimization were left untouched. That run's optimistic completion receipt is
not accepted as resource or visual qualification. Continuous reserve checks were
added afterward. No deliberate host-memory exhaustion test is performed.

The final accepted receipt is `target/source-copy-ui-2nxq39mw/result.private.json`.
All three workflow checks and nine layout cases passed in 31.164 seconds. The
reference capture recorded four resources from five requests, 86,752 response
body bytes and three discovery rounds; three other-origin proposals remained
review gaps. All three retained surface screenshots were visually inspected,
including 1280 px at 400% browser zoom. The binary SHA-256 is
`0e7828c04defbcc96739b1674c57ef09136bf19bc478e6044932c8eaf6969f97`.
The high-zoom PNG SHA-256 is
`b52d3ccf65cd09a305b7c0efd1b9c3e39a40cb3b8898f8f7fc77434c1d7f9b64`.
Sixty-eight whole-host samples, 500 ms apart, recorded at least 20,160,962,560
bytes (18.776 GiB) available. These observations are not an application/GPU peak
or a host-memory reservation.

The full workspace run passed 1,187 tests with 576 opt-in cases ignored across
43 reported targets, including empty documentation targets. After adding the
continuous reserve guard, all eight affected capture checks passed again;
workspace Clippy, formatting and the managed application build passed. All 165
UI scripts and 30 development-tool checks passed. The latter include reserve
boundaries, unavailable counters and mocked in-flight pressure confirming owned
containment cleanup. The native cancellation check separately confirms worker
shutdown. Host memory was not deliberately exhausted to test the pressure path.

This run reused relocated test-owned private runtimes. Actual missing-tool
installation has separate [download and repair evidence](RUNTIME_REPAIR_QUALIFICATION.md).
The unchanged full native browser and layout matrices were not repeated. The
preceding `7ab0fee2` hosted result does not verify this local batch. Private
workspaces, URLs, tokens, archives, screenshots and logs remain outside Git.

## Open gates

The [isolated-rehearsal gates](ISOLATED_REHEARSAL.md), candidate-data provenance,
unseen page/platform coverage, local-model accuracy and clean-host installation
remain open. This interface does not expose a saved real-site form for candidate
entry or authorize interaction with real application forms. No application,
application email or release is created.
