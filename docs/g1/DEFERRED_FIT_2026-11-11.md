# Cancellable fit assessment - 11 November 2026

This R01 continuation removes fit startup's model HTTP preflight from the
serialized workspace owner. R01 remains in progress: native observations and
other blocking handlers, including `main.rs::form_resident_model` during form
opening, still need qualification. This is not general form-completion evidence.

## Implemented behavior

Fit startup captures the existing input digest and acquires the existing model
work lease, then registers one named worker and its join handle. Runtime
selection, installed/resident model binding, semantic fact selection and
assessment run inside that worker. No unbounded per-request workers or new
services are created. The internal runtime argument accepts literal IPv4
loopback with a nonzero port; HTTP input cannot select this endpoint. The normal
entry point retains its existing local runtime address.

The task publishes checking-model, assessing and stopping phases. Stop targets
the exact task ID, sets its shared cancellation flag and returns promptly.
Result collection joins only a finished worker; a stopped task has no new score.
Cancelling an already completed result does not retroactively discard it.
Server shutdown takes ownership outside the task mutex, cancels and joins the
worker, and does not publish uncollected work over a saved assessment.

The local HTTP reader now enforces a total request deadline, including trickling
responses, with periodic socket checks. Adapter metadata GETs and the generation
resident-context lookup honor the same cancellation/resource guard as generation.
Startup status observation additionally has a six-second guard across its model
and inventory reads. Individual native operations, connection establishment,
parsing and scheduling still bound when cancellation is observed; this is not a
hard real-time guarantee. Closing a client request does not establish that a
shared Ollama server has stopped computing or unloaded its model. This checkpoint
never loads, unloads or interrupts an unrelated model service.

The expected model name, digest and resident context are checked through the
production identity fence before/after inference. Missing or changed metadata
cannot become a verified identity. The API still recomputes the current job and
candidate input digest before saving a result. Corrections, result persistence,
repeatability checks and saved-result retry behavior retain their existing gates.

The manual assessment UI now exposes Stop and accurate progress/terminal text.
A delayed Stop reply cannot overwrite a collected result or a subsequent task. Failed Stop requests
remain retryable while the assessment result remains collectable. Existing
batch/automatic stop-after-current semantics are unchanged.

## Verification and scope

- Managed Windows workspace tests: **1,305 passed, zero failed, 593 ignored**.
  Ignored model/browser/network cases are not counted as executed successes.
- Warning-denied workspace Clippy, formatting and canonical Windows build passed.
- Explicitly selected isolated owner-loop test: **one passed**, exercising two
  fresh fictional runtime sessions. Start returns under 500 ms; settings/status
  remain under their combined 500 ms threshold while `/api/ps` is held open.
  Wrong task cancellation is refused. Stop or server shutdown drains the worker
  and the fixture independently observes upstream socket closure. These bounded
  assertions are not a p95 distribution or all-handler latency guarantee.
- Inference-focused tests: **19 passed, two ignored**. Added regressions verify
  resident-context cancellation disconnects without generation and a trickling
  response cannot extend the total deadline. Production model-identity admission
  also has missing/changed name, digest and context regressions.
- Fit client suite: **20 passed**, including Stop, retry, terminal state and late
  response behavior. This suite contains different assertions from Rust totals.
- Headless rendered source client with fictional loopback API: checking-model,
  assessing, stopping and stopped at **1,360 / 480 / 320 px**; no page errors or
  horizontal overflow, Stop remains reachable at the inspected scroll positions,
  and resume state clears. Representative actual captures were inspected. This
  is client-state evidence, not a compiled end-to-end model journey, browser
  zoom/DPI qualification or complete accessibility acceptance.

Private receipts: `target/fit-preflight/`. The first inference-test compile
exposed a scoped fixture-channel lifetime error and was repaired. The first
native fixture called a nonexistent settings route; it was corrected to the
existing model-preferences endpoint. Initial rendered captures exposed an
incomplete fictional selection response and fixed-navigation overlap at one
capture scroll position; corrected fixture responses and action-visibility
captures retain the earlier observations rather than overwriting them.
No production matching/visual criteria or detector finding was suppressed.

CI explicitly selects the isolated native case with private APPDATA and checks
its executed denominator. Hosted evidence for this newer checkpoint remains
pending its eventual coherent push. No dependencies or lockfile changed. No
real employer form, application submission, application email or visible desktop
interaction occurred. All broader product acceptance gates remain open.
