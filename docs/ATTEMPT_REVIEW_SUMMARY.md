# Saved attempt review summary

Checkpoint: 2026-11-10. Application records now offer **View review summary**
alongside their saved answers and screenshots. The action reads that attempt's
own activity log and explains selected recorded events in normal language:
interrupted entry, unverified attachments, unavailable request observation,
missing captures, changed pages and stale advice. Inspection, requested answers,
field checks, document selection and recorded visual findings also appear.

This is a selective history view, not an assessment of the current application.
Older issues may since have been resolved. Successful later checks do not erase
past failures or establish resolution. Neither the browser session nor candidate
answers are automatically restored, replayed, retried or submitted. Existing
manual-review flags, approvals and application state are unchanged.

## Read and display contract

- The job must exist and the attempt's directory and metadata must match that
  job and attempt. No arbitrary filesystem path is accepted by the endpoint.
- Read at most the latest 2 MiB of `events.jsonl`, with the read bounded on the
  open handle. New appended bytes belong to a later refresh. Incomplete boundary
  lines are excluded. Parse at most 5,000 latest complete lines and display at
  most 40 recognized activity items in append order, newest first in the UI.
  Timestamps are observations, not a trusted ordering key.
- Missing logs are explicit. Byte/line limits and malformed or partial entries
  produce partial coverage. Counters describe the inspected portion; omitted
  activity and other event types remain visible. A complete log read does not
  imply complete event classification, complete preparation or valid evidence.
- Only selected event identifiers, positive timestamps and fixed explanatory
  text are returned. Raw error messages, page URLs, applicant answers, document
  contents and untrusted notes are not copied into the summary. Original
  materials, answers and logs remain in their existing plaintext archive.
- `resolution_status` and `submission_status` are both `not_assessed`. The UI
  never presents a success indicator based on this summary. It points the user
  to saved evidence and a fresh inspection before repeating uncertain actions.

The new read-only route is `POST /api/attempts/review-summary`, using the same
local workspace token and job/attempt selection as the existing history routes.
Summary loading is on demand; job-list rendering does not scan every event log.
The existing shared buttons, typography, cards, review messages and scrolling
styles are reused.

## Measured evidence

Six focused Rust checks cover retained failure ordering, candidate-value
exclusion, explicit unknown outcomes, missing/damaged/partial/unknown evidence,
byte/line/display limits, workspace reopen, the actual HTTP endpoint, ownership
rejection and preservation of manual-review flags. The managed web suite passed
**759 tests with 403 opt-in tests ignored**. Clippy, formatting and the managed
Windows build passed. All **165 JavaScript scripts** passed on final source.
One existing test's extraction boundary was adjusted for the newly inserted
summary button; its original integrity-message checks remain intact.

The compiled app passed **10 headless checks** with production handlers and
fictional records: history button loading, no returned private error text,
unchanged source log, 320/390/768/1440/1920 widths, last-guidance reachability below
fixed mobile navigation at 320/390, truncated-log refresh, and a missing legacy
log. Screenshots of the actual interface and summary at 390/1440 were directly
inspected. Two earlier harness selector ambiguities were retained as failures;
they were corrected by selecting the actual summary element within its nested
history disclosure, without weakening product behavior. No API response or
frontend asset was substituted.

Final private evidence: `target/attempt-review-ui-LUTlqr/result.private.json`,
SHA-256 `75e62c9a7cc9d378deacf639bd4db0518f679a755d22d9a0ffffd2facf45b87f`.
Compiled executable SHA-256:
`16e665f78d2724f25c135f7dd5c7201144202333e14f6f69b648c5fcc9cf98e4`.
The wrapper took **4.722 seconds** inside a 4 GiB job. Ten nominal 500 ms samples
reported at least **19,293,753,344 bytes (17.969 GiB)** of free physical host RAM.
This is a short whole-host sample, not an application/GPU peak or reservation.
Code fingerprints were unchanged during the rendered check. Logs, screenshots,
workspace fixtures and wrapper receipts remain under ignored `target/`.

## Remaining work

The summary covers selected existing event types. It does not supply a durable
browser checkpoint, prove which answers remain on the employer site, validate
server-dependent questions or resolve historical review events. Form completion,
local-model reliability, isolated replay, the complete preparation archive,
installation and release gates remain open. All checks used fictional data;
no employer form, application delivery, email delivery or desktop interaction
occurred. No release was created.

Reproduce after building the app:

```powershell
node apps/web_app/scripts/qualify-attempt-review.cjs --browser <absolute-headless-browser> --executable <absolute-app-executable>
```

The development script requires the existing private `target/layout-qa`
Playwright dependency and explicit runtime paths. Run under the shared bounded
process wrapper, as in the retained evidence; these are development dependencies,
not product installation assumptions.
