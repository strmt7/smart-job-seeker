# Background source-page copies

Saved-copy discovery now allows up to eight sequential passes within the existing
time/request/byte limits and stops early without new resources. Contact mapping
uses output vocabularies derived from the actual field references and available
profile keys; semantic/native/visual checks still control every entry. Bounded
native-response diagnostics remain in private application records. Incomplete
reviews, unknown answers and unsupported flows still require review.
[Current measured scope](g1/SCOPED_MAPPING_AND_DEEP_DISCOVERY_2026-11-13.md).

Checkpoint: 2026-11-11. In a job's **Details and records**, **Save page copy**
now runs the public-resource coordinator in the background. The interface shows
stages, supports stopping, recovers an active task after reload and exposes its
saved outcome in application records. This is a source archive, not an
application-completion or submission feature.

## Collection and records

[Public control outlines](PUBLIC_FORM_OUTLINES.md) add a bounded inventory of
root-frame labels, descriptions, group context and exposed flags to the saved
copy and history. Values/handles are excluded; missing data remains unknown.
The page-controls view does not enable filling or establish application readiness.

The parent checks the selected job's source permission and public HTTPS URL.
It refuses known candidate identifiers, user information in the URL and common
authentication parameters. This conservative check is not proof that an
arbitrary URL contains no personal information. The worker independently checks
the URL/source binding and accepts no profile, cookies, credentials, request
headers or form bodies in its request.

Internally observed asset GETs may carry one validated public Origin from an
admitted document. The native request and saved response retain that binding;
replay requires the same context. GET preflight-header absence also requires
explicit native evidence. Cookies, arbitrary headers and bodies remain excluded.
Per-resource responses now allow 8 MiB with the 32 MiB default aggregate bound
unchanged. [Context and real-page evidence](g1/PUBLIC_REQUEST_CONTEXT_2026-11-11.md)
describe unsupported variants and remaining data-dependent stops.

[Shared URL privacy checks](CAPTURE_URL_PRIVACY.md) now also guard redirects,
robots requests and browser-discovered resources before transport. Known private
keys/email-like references are refused; decoding and nesting uncertainty remains
reviewable. The parent checks known candidate identifiers in the bounded decoded
entry. Unknown opaque identifiers and arbitrary private data remain unassessed.

The collector starts with the listing origin. Its isolated render pass can
observe an anonymous document request from an attached child frame. It can
add up to two embedded origins per copy, limited to the exact public Greenhouse,
Lever and Ashby hosts supported by the capture policy. Each needs source Read
permission, URL privacy checks, its own robots check and a successful HTML
response. Document redirects during admission stay on the same origin. A
failed admission leaves that origin unavailable for resource collection.
The report retains the origin, source, observation round and document URL
digest. Unrecognized document origins and top-level navigations cannot expand
that document scope. Separately, typed observed public assets can use up to four
supporting origins, with permission, privacy, robots and matching MIME checks.
An asset origin never gains document access. Unsupported variants and unknown
rules remain review items. [Real-page measurements](g1/PUBLIC_SOURCE_DIVERSITY_2026-11-11.md)
describe the remaining CORS/request-context gap.

The native transport remains bounded to anonymous GETs. Collection cannot use
the candidate-entry driver. No raw saved HTML is served on the app's origin.
The existing [offline preview](RECORDED_PAGE_PREVIEW.md) can show eligible
frames retained by the copy. A successful HTML response does not establish
that an employer form is accessible, complete or ready for entry.

The Windows headless qualification used fictional responses delivered through
the native capture transport interface. It verified that a child-only field
was visible after freezing and replaying the copy, and that an unrelated
image on a different known ATS host never reached the transport. The run
sampled at least 29.50 GiB of available host RAM. This evidence covers the
fixture, with employer-site and frame coverage still unmeasured.

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
