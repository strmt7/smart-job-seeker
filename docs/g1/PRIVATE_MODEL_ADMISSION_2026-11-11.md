# Automatic private-model admission for saved-copy rehearsal

Date: 11 November 2026. This checkpoint connects encrypted model selection to
the existing fictional saved-page rehearsal. It does not enable candidate entry
on employer sites or authorize application delivery.

## Product behavior

Settings can save an installed local vision model and an optional local model
storage folder. The selection binds the model name, manifest digest and size;
the settings row is Windows-user sealed and revision checked. Clearing it keeps
the revision history, and changing it does not alter candidate facts or approvals.

A matching eligible resident model is borrowed. Otherwise rehearsal checks the
shared service, verifies the selected manifest and blobs into a private cache,
then starts a separately owned Ollama on a verified loopback listener. The
runtime has its own temporary profile, one loaded model, bounded concurrency,
4096-token context, host-headroom checks and process-tree memory containment.
It neither loads nor unloads a model on the shared service. Missing/changed
model bytes, another active model, insufficient resources and ambiguous ownership
stop admission visibly. Verified cache bytes remain for reuse; damaged recognized
caches have bounded quarantine, while unknown files are preserved.

Runtime identity and resource checks run before, during and after visual
inference. Each supported action still requires pre-action model inspection,
exact readback and post-action visual verification. Normal completion stops the
private runtime and verifies process exit, listener absence and profile cleanup.
Cancellation drains the containing worker job before removing its task-bound
temporary namespace. A cancelled worker's last `ready` session snapshot is
historical; it is not rewritten into a fabricated graceful-close receipt.

## Measured evidence

All forms and identities were fictional. Tests were headless, with external
browser requests blocked. No real application interaction or delivery occurred.
The installed model was `gemma4:12b-it-q4_K_M`, manifest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
using runtime 0.40.16. Raw receipts and caches stay ignored under `target/`.

| Check | Observed result |
| --- | --- |
| Production automatic-admission renderer | Three sections, 3/3 completed fields and 5/5 action targets visually matched; session closed, process signaled exited, no private listener, namespace removed |
| Compiled app route | Saved model settings, cold private cache, completed three-section rehearsal, persisted record and unchanged discovered job |
| Cancellation after warm-up | Owned model exited, no private listener, namespace removed; shared model list unchanged |
| Rendered settings | Persistence after reload, stale-write refusal, 1360/480/320-pixel layouts, refresh, clear and reselect; no horizontal overflow or page errors |
| Native ownership regressions | Terminated process with a deliberately retained handle; wrong listener PID; wildcard IPv4 and IPv6 listeners; absent versus unresponsive private test endpoint |
| Cache regressions | Source immutability, verified reuse, changed manifest, same-size corruption, repair, unknown-cache preservation, disk/time budgets, path refusal and dangling-lock refusal |

The direct renderer native test took 160.26 seconds (190.775 seconds including
build/controller overhead), with at least 16,178,274,304 bytes of sampled host
RAM available. The compiled settings/rehearsal/cancellation controller took
243.490 seconds, with at least 17,919,606,784 bytes available. Each had an outer
10 GiB process-tree limit. The runtime's own 20 GiB upper limit was further
constrained by that ancestor job. These are sampled whole-host observations,
not exclusive memory peaks, reservations or a proof against long-term leaks.

Primary receipts:

- `target/private-rehearsal-qualification-31na0609/result.private.json`
- `target/model-settings-native-re2aacng/result.private.json`
- `target/model-settings-native-qiozp8e1/result.private.json` (settings-only
  interaction and unobscured control captures; no repeated model run)
- `target/model-admission-final-57nnlwjm/` (final native absent/listening probe,
  workspace and Clippy checks)

Final workspace checks passed 1,260 tests across 43 targets; 590 live/optional
tests remained explicitly ignored. Clippy, formatting, the managed Windows
debug build, 67 Python tests and all 169 UI scripts
passed. Staged-source Gitleaks scanned 26 files with no findings. The final
host audit observed zero matching development processes, services, tasks or
startup entries, zero resident models and 27,679,498,240 bytes of available RAM.
These inventories are checkpoint observations, not a long-running leak proof.

The full model runs precede the expanded wildcard/IPv6 listener inspection and
absent-service fallback. The subsequent native regressions exercise those added
cases. Retain that source distinction when reusing the evidence.

## Failures retained and corrected

Two initial renderer runs were not accepted. One used process identity metadata
as a liveness check even though Windows can retain a terminated process object.
The next classified a short TCP timeout as proof of a live endpoint. Native
process wait signals and listener tables now distinguish these observations;
the accepted run retained a timed-out socket probe alongside an empty native
listener table. An early compiled run also found that application-record folders
have a job prefix; the namespace contract and regression now use the task suffix.
Failed receipts remain intact.

The first hosted CI run also caught the settings initializer in an unrelated
source-fragment test. Wiring it into the page startup section fixed the cause;
all 169 local UI scripts and a rebuilt compiled settings check then passed.
The original job log is retained privately, with no test exclusion or suppression.
The compiled follow-up is `target/model-settings-native-tthm7apq/`; its controller
and complete UI-script results are under `target/model-preferences-bootstrap-n2md8yer/`.

## Remaining limits

Automatic admission currently serves saved-copy rehearsal, not every model
feature. The selected model must already be installed locally; model download
and installation controls are separate workflows. A private cache needs disk
space and currently re-verifies blobs on each admission. Shared-service absence
is accepted only after native inspection; a listening but unresponsive service
is left unchanged and blocks admission.

Only bounded fictional saved-form journeys were qualified here. Complex custom
controls, authenticated/server-dependent flows, unseen ATS families, candidate
data isolation, clean Windows installation and broader document/model quality
remain open. Follow the [completion plan](../COMPLETION_PLAN_2026-11-11.md).
