# Owned form inspection and deferred admission - 11 November 2026

This extends the [fit lifecycle checkpoint](DEFERRED_FIT_2026-11-11.md).
The form-opening API no longer performs resident-model HTTP or inference-memory
preflight on the serialized workspace owner. R01 remains open for other native
observations, cleanup latency and broader admitted-workload qualification.

## Behavior

The owner captures the selected job/route, candidate profile digest, pending-review
revision and bounded navigation-experience snapshot. The inspection worker then
resolves the local model, checks resident versus installed identity, acquires the
existing inference lease if eligible and opens the browser. The runtime endpoint
is an internal literal IPv4-loopback argument, not an HTTP-controlled URL.

An eligible model is bound to its name, digest and resident context; observations
before/after inference must still match. Metadata reads use cancellation and a
six-second overall status guard alongside the shared HTTP deadlines. Missing or
ineligible models retain explicit no-model/manual-review inspection behavior.
Only actually used experience revisions are returned. Existing current-profile,
job/route and experience-revision checks on collection are preserved.

One named thread and its join handle belong to the task. A failed spawn clears
its registration. Result collection joins only a finished thread; server shutdown
takes the task outside its mutex, cancels and joins it, then drains any uncollected
result/session. Panic, disconnection and cancellation stay explicit failures with
the existing pending-review/attempt-record lifecycle. A stopped result cannot
publish suggestions or used-lesson credit.

The inspection UI displays checking-model, opening, assessing and final-check
phases, with an actual Stop control. The progress panel stays outside selected-job
content that normal refresh replaces. Stop targets its task; a late reply cannot
replace a later inspection's panel. A failed Stop remains retryable. Stopped work
is reported through the existing failed/review path with a stopped reason; it is
not reported as completed or submitted.

## Evidence

- Managed workspace tests: **1,306 passed / zero failed / 594 ignored**.
- Warning-denied workspace Clippy, formatting and canonical Windows build passed.
  A production-only single-variant destructuring warning was repaired without
  disabling its rule; the unchanged test branch's prior full-suite result is
  retained, and production Clippy/build passed after that refinement.
- Focused form tests: **21 passed / 22 ignored**. Missing identity/context,
  unavailable/unknown model states and already-cancelled preflight are covered.
- Explicitly selected isolated owner-loop controls: **two passed**, covering fit
  and form startup, wrong-task cancellation, Stop and server shutdown. The new
  form case holds a fictional `/api/ps` open, verifies start and independent
  settings/status requests against 500 ms thresholds, then observes socket exit
  after cancellation or shutdown within two seconds. This is bounded fixture
  evidence, not a p95 distribution or all-handler responsiveness guarantee.
- Selected real headless browser/file fixture: **one passed**. Inspection still
  returns live fictional form controls without filling when model admission is
  unavailable; its browser is dropped and drained. No employer page was involved.
- Focused inspection/navigation/link client tests: **six passed**, including
  Stop, failed-Stop retry and late replies across successive inspections.
- Actual source client with a fictional loopback API: model-check, opening,
  assessment, stopping and stopped-failure handling passed at **1,360, 480 and
  320 px**, with no page errors or horizontal overflow. Representative rendered
  panels were inspected and Stop actionability checked. This is not an integrated
  real-model employer journey or whole-interface zoom/DPI acceptance.

The native test starts with isolated control storage and cancels before browser
launch. It does not contact the fictional public job URL, load a model or mutate
the shared Ollama service. CI now explicitly executes both fit/form cases and
checks that exactly two ran. Earlier hosted CI, coverage and CodeQL have passed
for `aaf02d03`; those results do not validate this subsequent local continuation.

Private evidence: `target/form-preflight/`, including retained compile/fixture
failures, corrected client extraction for Windows line endings and revised
capture-position evidence. No detector finding was dismissed or suppressed.
No dependencies or lockfile changed. No real application, application email or
visible desktop interaction occurred.

## Remaining boundaries

Cancellation of a browser operation is observed at checkpoints around bounded
native calls; this work does not prove immediate interruption inside launch/CDP,
OS memory observations or cleanup. Closing a model client does not prove that a
shared model server has stopped computing. Uncollected-session idle expiry and
cleanup latency need further qualification. Those open items remain R01/R11 work;
passing these cases is not a general leak or universal-browser guarantee.

The next independent priority is R02/R03: a representative, held-out corpus and
faithful saved rendering/label context, followed by generic local-model completion.
No overall completion percentage, universal autofill claim or release approval
follows from this checkpoint.
