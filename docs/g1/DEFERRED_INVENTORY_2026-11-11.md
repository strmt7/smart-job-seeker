# Deferred local inventory - 11 November 2026

R01 remains in progress. The model-inventory HTTP fetch no longer runs inside
the serialized workspace handler. This extends the
[nonblocking socket checkpoint](CONTROL_PLANE_IO_2026-11-11.md) to one previously
blocking runtime check; it does not establish all-handler responsiveness.

## Implemented behavior

`inventory_read::Pending` owns one lazily started Rust worker and its join handle.
Up to four waiting connections share the same in-flight `/api/tags` observation.
Further requests receive a bounded 503 response. Completed observations are
collected and discarded, so a later request performs a fresh read rather than
using a permanent inventory cache.

The worker accepts only a literal `127.0.0.1` address with a nonzero port; the
normal product entry point retains its existing default local-runtime endpoint.
Request bodies, candidate facts and URLs never become worker inputs. The worker
uses the existing guarded HTTP client, its response-size cap and a three-second
total-deadline guard. It performs no model generation, loading or unloading.
Socket guard checks run periodically; connection establishment and JSON parsing
can still delay observation of cancellation within their bounded operations.

Workspace state and response publication remain on the owner thread. Existing
inventory fields and advisory memory checks are retained. A runtime/parse error
stays an explicit error, not a successful empty model list. Pending connections
now expire under the transport's total request deadline; late results cannot
respond through an expired/replaced identity. Unneeded waiters are pruned, and
shutdown cancels and joins the worker before ownership ends. No persistent
worker service, scheduler or startup registration is created.

The Windows memory/GPU observation that annotates the result is still performed
on the owner. It is not included in a claim that every native or handler call is
interruptible. Other synchronous preflight paths, including fit startup's model
status query, remain R01 work.

## Qualification

The isolated full owner-loop test holds a fictional runtime response open while
four real loopback HTTP callers wait for inventory. It verifies an overflow
request receives 503, an independent settings request returns 200 in under
500 ms, no inventory success is published before release, and all four waiting
callers then receive the expected model. Arrival order is not assumed.
This test starts no browser, model or public request and uses an in-memory
fictional workspace. It is explicitly selected, rather than counted among
ignored cases that did not execute.
The Windows build job now selects this case in a separate process and verifies
that exactly one test ran, with zero failures or ignores. Its hosted result is
pending publication of this local checkpoint.

Focused unit cases verify literal-address/cancellation checks, errors versus
empty success, one fetch for bounded concurrent waiters, a new fetch after
completion, and cancellation that both joins the reader and closes its upstream
socket. The transport regression additionally verifies expiry while waiting for
a deferred handler and refusal of its late response.

Workspace validation passed 1,302 tests, with 592 ignored. Warning-denied Clippy,
the focused native saturation case and the final build passed after test-only
cleanup/arrival-order refinements. Those refinements changed no production
behavior. Dependency versions and the lockfile did not change in this checkpoint.
No detector finding was manually closed, dismissed or suppressed.

The compiled settings client then passed selection, stale-write refusal,
persistence and controls at widths 1,360, 480 and 320 pixels. The narrow capture
is byte-identical to the previously inspected one. This is the existing settings
journey, not whole-product usability or local-model accuracy qualification.

Private evidence: `target/control-handlers/` and
`target/model-settings-native-4hic__iz/`. The latter binds source fingerprints
and binary SHA-256
`8ff66942ce998f7c8e4b3e63be406d44dfedb51f954e2380025b67526e14383d`,
records drained processes, and sampled at least 29,222,436,864 bytes of available
host RAM. No real application, application email, model inference or visible
desktop/browser interaction occurred.

The preceding pushed `68b3e891` checkpoint now has successful hosted CI,
coverage and CodeQL runs. Those runs validate that earlier source, not these
newer local changes. Further hosted evidence must name the subsequent exact
commit. All broader completion-plan gates remain open.
