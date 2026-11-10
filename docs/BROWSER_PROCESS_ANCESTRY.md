# Process ancestry in the browser cleanup test

Checkpoint: 2026-11-10. This corrects a test observer, not the production
Windows job ownership implementation.

The full local headless sequence on Chrome `156.0.8078.80`, matching the hosted
runner's revision, reported 136 passes and one failure in forced-parent cleanup.
The reported survivors were two Python processes created the previous day.
Windows had reused their original parent's PID for a new browser process.
The observer correctly bound its final checks to PID and creation time, but
its initial recursive discovery still followed parent PIDs without checking
when each child was created. It therefore adopted unrelated older processes.
Those processes were not terminated or modified.

The initial discovery now requires a creation timestamp for every relevant
process and only follows an edge when the child was created no earlier than
its current parent. Missing timestamps fail the observation explicitly.
The final check still requires the same PID and creation timestamp. A process
with a reused PID cannot count as a surviving original browser. Legitimate
descendants remain included regardless of executable name.

Four regressions run the actual PowerShell commands extracted from the Rust
test, substituting only fictional CIM rows. They cover stale parent edges at
multiple depths, valid descendants, missing root/child timestamps, and a reused
PID during the final identity check. The old graph accepted the unrelated
fictional processes. The corrected fixture passes all four checks under Windows
PowerShell, and the full development-tool suite passes 28 methods. An initial
mock-array incompatibility between PowerShell versions was corrected; its failed
log remains private. The actual forced-parent browser case then passed with
the matching Chrome engine and a four-GiB process-tree memory budget.

The `2002ce51` hosted follow-up also passed the four previously failing
HTTP/WebSocket checks, with 136 passes and one cleanup-fixture failure. Here the
parent read the ready marker after file creation but before its contents were
written: the empty string had no parent directory. The helper now writes into
a same-directory temporary file and publishes the completed marker without
replacing an existing file. Three deterministic checks verify that partial bytes
stay unpublished, interrupted writes leave no marker/staging file, and an
existing marker is preserved. All three pass, as do formatting, browser Clippy
and the final affected native browser case.

The four original hosted HTTP/WebSocket failures remain unexplained; passing
subsequent checks is not a causal diagnosis. The focused cleanup rerun is not a
single all-green full-suite result. Keep the original failures and exact-commit
hosted results distinct. No test was skipped, no assertion or timeout was
weakened, and no employer form, application, email or visible desktop interaction
was involved.
