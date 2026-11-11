# App-owned local runtime lifecycle

The old startup path spawned Ollama, checked readiness and discarded the
child handle. It could leave an untracked server after failed startup or app
exit. The current path retains a bounded Windows process group on an ownership
thread, using the existing processkit dependency. No dependency was added.

The owner records native PID, creation token, image name and parent identity
from its own job. Readiness checks also require that root to remain in the job.
Explicit shutdown kills the owned tree, waits for the root and verifies empty
membership before acknowledging completion. Failed identity or handoff drains
the created tree. Root exit triggers descendant cleanup. Windows job
kill-on-close covers abrupt owner-process termination. The app invokes normal
shutdown after either success or failure of its main run, before a fatal-error
dialog can keep the process open. A shutdown flag prevents a late startup from
becoming a new retained owner.

The server has an aggregate process-memory cap of observed available RAM minus
3 GiB, capped at 20 GiB. It needs at least 5 GiB observed free RAM to start.
While running, one-second headroom sampling stops only its owned tree below
3 GiB or on unusable memory observations. Model startup retains its separate
installed-model, identity, GPU and host-resource admission gates. The cap is
not a claim about GPU allocation, peak memory or support for every model.

## Measured native checks

The initial focused Windows suite passed five tests:

- Budget boundaries reject unknown/inconsistent memory and preserve headroom.
- Explicit stop drains a root and child and supports repeated stop calls.
- A deliberately abandoned ownership handoff drains the started tree.
- Natural root exit does not leave its child running.
- Forced owner-process termination closes the inner job, while an unrelated
  process using the same executable remains alive. The test then cleans up that
  separately owned comparison process.

The five-test run took 1.06 seconds. Final review added a sixth native test
for an immediate stop after observing root exit. Shutdown now uses the joined
owner's actual drain result, avoiding a false failure when a stop reply could
otherwise be lost after natural exit. The final workspace run covers that
refinement. The subprocess fixture is separately
ignored by the ordinary suite and is invoked explicitly inside these tests
with a private directory and role. An initial root-exit probe failed; the
current implementation requires native job membership in addition to the
creation identity, and its fixture explicitly reaps a child on a helper
thread while the root is alive. Retained failed logs are not counted as passes.

The opt-in real Ollama probe started an isolated loopback runtime with an empty
private model directory, observed zero resident models, stopped through the
production owner and verified that the endpoint closed. The pre-existing
service's reported model state stayed unchanged. The initial probe passed in 7.21 seconds; the final joined-drain revision passed in 6.17 seconds.
This check loaded no model and does not establish runner/GPU teardown under
inference load. It exercises the runtime command and owner, rather than
claiming a Settings-route launch on an already occupied default port.

Native logs and bounded controller results:
`target/runtime-owner-final-native.private.log`,
`target/runtime-owner-final-ollama.private.log`, and
`target/runtime-owner-final-checks.private.json`.
The startup UI tests also passed for borrowed, app-owned and legacy responses.

## Remaining integration

Rehearsal still requires an already qualified resident vision model. Automatic
admission of the configured model needs a private endpoint, model-store and
digest binding, effective memory-budget propagation through the capture
worker, and actual local-model/GPU lifecycle qualification. Shared-service
identity alone cannot authorize unloading another workload. This lifecycle
change is a prerequisite, not a claim that automatic admission is complete.
Clean-machine Windows installation and long-running leak behavior remain
separate qualification gates. No application was sent and no release created.

## Final validation and host checkpoint

The final workspace passed 1,254 tests with 588 explicitly opt-in or
subprocess-fixture cases ignored across 43 targets. Clippy, formatting,
diff whitespace, the canonical Windows build and both focused runtime UI
scripts passed. The final real Ollama probe also passed. The bounded native
check manifest `target/runtime-owner-drain-final-checks.private.json` has
SHA-256 `3fa798d1306663de2d130a2af81fe27b3bfaf86fb8bf98a3a42067cc7e8259e3`.
Earlier manifests retain their own source/check scope.

The final host snapshot found zero matching development processes, scheduled
tasks, services, startup entries and resident models. Available physical RAM
was 25,298,493,440 bytes. Native snapshot:
`target/development-host-8bdc962a5a3c488e8b05cf0f0c03092a.private.json`.
The repository-owned source-index daemon was also stopped after routing; no
other repository's provider registration or cache was changed. A clean snapshot
and the native teardown cases do not establish long-running leak freedom.
