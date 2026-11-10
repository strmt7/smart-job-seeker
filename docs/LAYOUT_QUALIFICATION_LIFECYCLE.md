# Headless layout checks and development cleanup

Reviewed 2026-11-10. Development must leave no owned workers, model loads,
temporary servers, services, scheduled tasks or startup entries behind. The
[agent ownership rules](../AGENTS.md#development-process-and-host-ownership)
require bounded resources, verified teardown and preservation of unrelated
workloads. This is a lifecycle requirement, not a claim that all memory leaks
have been disproved.

## Changes and retained failures

At `d68dcd18`, hosted ordinary tests and the optimized Windows build passed.
The layout step then failed with `Cannot read properties of null (reading 'x')`.
Its old failure path did not retain observations or a stack, so the exact
control responsible for that hosted failure cannot be established.

The checker now waits for visible, stable geometry within bounded deadlines;
missing, zero-size or non-finite geometry remains an error. A focused regression
checks delayed readiness, hidden controls and invalid geometry. Failure reports
retain the phase, stack and cleanup result; a bounded screenshot/geometry capture
helps diagnose partial runs. Startup listeners/timers are removed after startup,
and startup output is capped instead of growing for the entire run.

The first two contained local attempts failed the PDF/text drag assertion.
Retained geometry and a screenshot showed the tall divider extending underneath
sticky navigation. The driver used a fixed offset near its top; it now drags
the visible grip, as a user does. The drag assertion remains intact. These
failures are not counted as successful qualifications or proof of the precise
cause of the earlier hosted null-geometry exception.

## Resource ownership

Run `python -B tools/qualify_layout.py` after a managed Windows build. The
controller reuses the existing suspended-child Windows-job implementation:
containment is assigned before Node executes, with kill-on-close and a 4 GiB
aggregate committed-memory cap. Runtime is bounded to 600 seconds and the log
to 8 MiB. Whole-host free RAM is checked every 500 ms against a 3 GiB reserve.
Normal/error/timeout cleanup terminates the owned job and waits until Windows
reports zero active processes. An abruptly killed controller closes its job,
which terminates the contained tree. This does not manage shared model servers;
the layout run loads no model. No service, scheduled task or startup entry is
created. Direct Windows invocation of the JavaScript runner is refused.

The JavaScript runner also awaits its app's exit and bounds browser shutdown.
The outer controller always writes a private completion record; missing native
reports, controller changes, cleanup errors or failed assertions cannot pass.
Controller hashes and the compiled binary/browser identity bind the evidence.
Reports and screenshots stay under ignored `target/`; no personal files or real
employer forms are used.

## Measured local result

The complete matrix passed **296 observations**, with zero overflow/geometry
failures, script errors or cleanup errors. It covers actual browser zoom,
viewport changes, mouse dragging, PDF/text resizing, license-dialog resizing
and preference-storage failure/recovery. The app exited; the owned job drained.
The PDF/text and resized-license screenshots were inspected directly.

The run took 15.082 seconds using compiled debug assets in Chrome
157.0.8103.39 and Playwright 1.65.0. Available physical RAM was
25,249,726,464 bytes before and 25,337,020,416 bytes after. These are endpoint
observations, not peak use, minimum headroom or a long-running leak measurement.
The fictional workflow does not qualify employer forms, ATS performance,
accessibility on every surface or clean-host installation.

Private evidence:

- `target/layout-check-run-4l5kiiyz/result.private.json`, native result
  `target/layout-matrix-dwnMrJ/results.json`, SHA-256
  `47c99d6b1b306771e5abfcf4490b40203d5ea40a29ce8017dcd652e3271082a3`.
- Compiled app SHA-256
  `3bedf287b75bf85d5461a131ce5c9b7ffc4ef813721b0cfcb0577ad7ac858566`.
- Earlier failed runs remain in `target/layout-check-run-fctjb20l/` and
  `target/layout-check-run-eptrtdqf/`; both reported successful app cleanup.
- A subsequent host inventory found no matching repository workers, services,
  scheduled tasks or Run/RunOnce entries, and no resident Ollama model. It
  preserved the pre-existing user-managed Ollama app/server. Unknown process
  ownership must never be resolved by killing a process based on its name or
  RAM use. This snapshot does not prove absence of every host issue.

Six existing qualification-controller checks also pass, including refusal under
host memory pressure and cleanup of only the owned containment. Product Rust
and inference code did not change in this follow-up, so their already-passing
ordinary suites were not rerun locally for a development-controller change.
