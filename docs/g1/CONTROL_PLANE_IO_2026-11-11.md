# Local control-plane I/O - 11 November 2026

R01 is **in progress**. This checkpoint removes socket-driven head-of-line
blocking; it does not qualify every handler's latency or complete the product.
No real application, application email, public-site interaction or visible
desktop/browser interaction occurred in these checks.

## Mechanism

`apps/web_app/src/http_server.rs` owns a nonblocking readiness loop using the
already-resolved Mio 1.2.4 and httparse 1.10.1 libraries. HTTP framing and response
delivery no longer perform blocking socket I/O on the workspace owner. Request
dispatch and state/grant mutations remain serialized. Public assets share one
lookup for serving and public-read eligibility; private requests retain literal
loopback Host, workspace token and Origin checks before body collection.

The transport bounds eight active connections, 96 MiB of accounted connection
buffers, 32 MiB per response, 256 KiB and 128 I/O calls per connection per cycle,
and 16 accepts per cycle. Interrupted calls cannot monopolize a cycle. Existing
endpoint-specific request/document limits remain.
Full connections get a best-effort, nonblocking 503 refusal. These are transport
buffer limits, not limits on the entire process, kernel buffers or application
state. A failed/oversized response produces a bounded 500 rather than publishing
a truncated success.

Header, total request and response-write deadlines are respectively 5, 30 and
15 seconds; progress does not renew them. They are checked by the owner loop,
so a blocking handler can still postpone a deadline check. Readiness is retained
across byte quotas until an actual WouldBlock, and stale connection identities
cannot attach a response to a replacement connection. Drop shuts down and
deregisters owned sockets. There are no per-connection worker threads, new
services, autostart entries or scheduled tasks.

## Compiled measurements

The original fresh-workspace reproduction delayed a settings read by 4.910
seconds behind an incomplete header, compared with a 0.040-second baseline.
It remains retained at `target/control-latency-90tt2x5t/`.

The final compiled binary has SHA-256
`0e6df331daa024cc0a38ce161951570227add686831aed66c260612835822180`.
The source-fenced repeat retained 120 raw request durations, rounded to
milliseconds, in `target/control-latency-07qd6ggc/result.private.json`:

| Condition | Samples | Observed p95 | Maximum |
| --- | --- | --- | --- |
| Baseline settings reads | 30 | 16 ms | 20 ms |
| Incomplete header held open | 30 | 16 ms | 25 ms |
| Incomplete authenticated body held open | 30 | 16 ms | 16 ms |
| After slow-client removal | 30 | 18 ms | 25 ms |

A separate trickling-header phase observed closure after 5.008 seconds while
27 intervening settings reads remained available, with a maximum of 17 ms.
Earlier post-change series had p95 values of 15-17 ms. These are small
series from one development host and admitted-load fixture, not deployment
percentiles or general cancellation/throughput guarantees. The original
pre-change comparison was a single reproduction, not a matched statistical
benchmark. Both old failures and intermediate receipts remain retained.

The private controller verified owned process-tree exit. Its app started in a
fresh fictional workspace, with browser/tray disabled and no model or public
requests. Receipt/harness/binary hashes and source fingerprints are retained;
authentication tokens stay in ignored private artifacts.

## Checks

- Eleven transport regressions pass: framing conflicts, authorization/origin and
  endpoint limits, incomplete headers and bodies, absolute deadlines, saturation,
  socket teardown, large uploads/responses across I/O quotas, slow readers,
  overflow responses, stale response identities and abandoned connections.
- Final workspace tests passed: 1,297 passed, zero failed, 591 ignored. Final
  transport tests, build and warning-denied Clippy check passed. Ignored cases
  are not counted as successful model, network or browser qualification.
- The actual compiled settings workflow passed selection, stale-write refusal,
  persistence and controls at 1,360, 480 and 320 pixels. Its binary matches the
  measurement above. The narrow screenshot was inspected. Evidence:
  `target/model-settings-native-e_pojnj0/`, which records drained processes and
  at least 29,663,428,608 bytes of sampled available host RAM. Its narrow screenshot
  is byte-identical to the earlier inspected settings capture. No model lifecycle
  or whole-interface scaling claim is made by this UI-only run.
- Cargo audit with warnings denied, license/ban/source checks and regenerated
  dependency-inventory verification pass. The inventory changed only its lockfile
  digest; the resolved package count remains 363. No finding was manually closed,
  ignored, dismissed or suppressed.

The previous hosted `800a2044` CI run had four passing jobs and one browser-job
failure: 136 passed and one failed in its selected native-browser suite. The
404 case received `ERR_SOCKET_NOT_CONNECTED` rather than the required HTTP 404.
Its bespoke server read only through the first newline and exited after one
response. It now reuses the existing bounded multi-connection fixture that reads
complete requests and joins its workers. Both HTTP-404 assertions remain intact;
the focused real-headless-browser case passed three contained local invocations.
This is not a claim that the earlier hosted failure changed state or that the
new full hosted suite passed; that requires the next exact-commit result.

Full native reports/logs and primary-library research receipts are in
`target/control-plane/`. The reviewed
[Mio readiness contract](https://docs.rs/mio/1.2.4/mio/struct.Poll.html) and
[httparse request API](https://docs.rs/httparse/1.10.1/httparse/struct.Request.html)
were captured through the isolated research provider with successful substantive
extractions. No upstream code was copied into the server.

## Remaining R01 work

Audit blocking work inside handlers, beginning with the synchronous three-second
model-inventory HTTP request. Move appropriate work to bounded existing task
lifecycles without changing authorization or serial workspace mutation ordering.
Measure status/stop responsiveness alongside admitted expensive work, saturated
queues and shutdown. Current socket measurements do not prove that all such
handlers meet the proposed 500 ms target. The broader R02-R14 gates remain open.
