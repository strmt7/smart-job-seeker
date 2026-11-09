# Recorded-response rehearsal

Status: experimental Windows development interface, 2026-11-10. This adds
bounded response replay to the existing [isolated browser](ISOLATED_REHEARSAL.md).
It is not enabled in the web interface or qualified for interacting with real
employer forms. No ordinary-browser or live-fetch fallback exists.

## Behavior and archive contract

Schema 1 rehearsal configurations retain their local-file behavior. Schema 2
requires `replay_manifest`, naming a JSON file already included in the
checksum-bound `content_files` inventory. Both versions keep the same process,
AppContainer, ownership, memory, disk, session and recovery limits. The embedded
controller receives the replay helper alongside its main script, including when
running Python in isolated mode; it does not rely on a host module installation.

The archive has exactly `schema: 1`, `body_representation: "decoded_http"`, an
HTTPS `entry_url`, and 1–512 `resources`. Each resource has these fields:

| Field | Meaning |
|---|---|
| `url` | Exact browser-canonical HTTPS URL, without fragment; default/443 port |
| `method` | Uppercase HTTP method; CONNECT and TRACE refused |
| `request_body_sha256` | SHA-256 of exact request bytes; empty-body digest for GET/HEAD |
| `status` | Recorded status 200–599, excluding partial-content and not-modified responses |
| `headers` | Array of exact `{name, value}` pairs, at most 128 |
| `body_file`, `body_sha256` | Inventory-bound local decoded response bytes |

Entry GET must resolve to the configured HTML entry file with a successful HTML
response. The manifest is bounded to 1 MiB, individual bodies to 4 MiB, and unique
body files to 64 MiB. Duplicate JSON keys, ambiguous resource identities,
credential/authentication response headers, unsupported fields, malformed paths,
changed files and truncated bodies are refused. `.bin` response files are allowed
only in explicit schema 2 content; executable assets remain refused. Files are
hashed before launch and again when their responses are supplied.

Response bodies are decoded HTTP payload bytes. Compression, framing and
hop-by-hop transport headers are removed and content length recalculated. HEAD
keeps a validated original content length and carries no response body. This
normalization is not a byte-for-byte replay of the original wire encoding.

The continuously pumped controller enables CDP Fetch interception before releasing
an attached page session, recursively attaches related targets, and waits for
page/iframe interception setup before resuming paused children. Manual aliases
for the same target reuse its primary interceptor. Host commands cannot override
Fetch/automatic-attachment ownership or use the reserved controller command IDs.

Lookup uses **exact method, URL/query ordering and request-body bytes**. A complete
UTF-8 body or complete bounded base64 byte entries can supply the body; conflicting
representations, missing file bytes and multipart payloads are refused. Fragment
routing stays in the browser. Unknown responses receive `BlockedByClient`; they
are never fetched or synthesized. A matching cached POST is a local response,
not a server write or evidence that a server accepted any candidate answer.

`rehearsal_replay_report` returns bounded counters and reason codes after successful
CDP acknowledgments, without URLs, request bodies, document contents or candidate
values. `server_validation` is explicitly `not_performed`. Existing typing guards
still stop on observed non-read requests. Replay does not weaken form safeguards,
approve applications or change submission states. `commit` always refuses this
browser owner before inspecting or clicking a submission control.

## Qualification and retained failures

The final private fictional run passed one explicitly selected native integration
test with zero ignored tests using the previously installed headless-shell engine.
It exercised preserved HTTPS origin and fragment routing, external CSS/scripts,
an exact cached JSON POST, cross-origin iframe execution, a dedicated worker,
a fetched next section, Unicode text, email, a hash-verified PDF upload, screenshot
capture, submission refusal and AppContainer profile cleanup. Eight recorded
responses were acknowledged; one uncaptured script-triggered request was blocked.
The typing guard stopped after its first character. The final PNG was directly
inspected and shows the expected values, sections, uploaded filename and blocked
probe. No employer site, account, real CV, email delivery or application was used.

The wrapper took 40.735 seconds including Cargo startup/build. Its 4 GiB Windows
job remained active throughout. Eighty-one whole-host samples at nominal 500 ms
intervals reported at least 15,176,175,616 available bytes (14.134 GiB); these are
sampled physical host observations, not application/GPU peaks or reservations.
Code and source engine inventories remained unchanged during the run.

Private receipt SHA-256:
`6ad7d8e38c6342f6e3409288fb8d3dc6f425f4fb6a5d7e00b44a33ccedc80442`.
Native result SHA-256:
`4404bc7a15ff335f4c97752eb4c0dc63efb4115bfb9e88eeaaf5f94a1bcb528a`.
PNG SHA-256:
`7c18b62dea2cee21ec6ebe58a96edee534007347b97775fce9ecfffaec43e4da`.
Artifacts are in ignored `target/replay-qualification-v6ucu9xn/`.

Earlier failures remain under ignored `target/`: the first fixture incorrectly
used editable-field verification on read-only status inputs; the next expected
success when typing actually triggered the existing non-read-request refusal.
The fixture now separates successful entry from an explicit side-effect refusal
check. Complete byte-entry support, duplicate-session interception and acknowledgment
accounting also have focused regressions. A subsequent source-bound schema 1
regression passed the existing local-file fill/section/upload/privacy/screenshot/
cleanup test. Eighteen portable replay checks and fourteen worker checks passed;
the two suites run in CI. Browser library validation passed 30 tests with five
opt-in tests ignored, all-target browser Clippy passed, and four focused
qualification-tool checks passed. These counts are not full workspace coverage.

## Remaining gates

- No permitted-site capture pipeline exists yet. Archives are explicitly supplied
  and qualified here only with fictional resources. Do not capture candidate
  credentials, authenticated sessions or arbitrary sites into this format.
- Worker Fetch is unsupported by the selected engine; this run obtained the
  dedicated worker resources through parent interception. The report still
  records the worker gap. Nested/shared/service workers, detached-target churn,
  redirects, authentication, websocket/WebTransport traffic, concurrent races and
  every browser target lifecycle are not qualified. Session/command limits refuse
  overload instead of claiming universal completion.
- Server-dependent validation, changing questions and missing resources require
  an explicit incomplete/manual-review outcome. A cached successful response
  cannot validate an answer or imply an application was completed.
- Replay interception is not the independent network boundary. The AppContainer
  boundary still lacks complete public UDP/DNS/broker and failure-schedule
  qualification. This work does not renew the broader nine-channel or public
  receiver evidence from the earlier checkpoint. Real-site interactions and web
  interface integration remain disabled.
- Local-model and visual-inspection accuracy, clean-Windows distribution and
  generic document fidelity remain separate product gates.

## Reproduce headlessly

```powershell
py -3.13 -X utf8 -B tools/qualify_replay.py --engine <absolute-headless-shell-directory>
py -3.13 -X utf8 -B crates/waypoint_browser/tests/test_rehearsal_replay.py -v
py -3.13 -X utf8 -B crates/waypoint_browser/tests/test_rehearsal_worker.py -v
```

The development command accepts an explicit interpreter with `--python`, retains
failures privately, bounds time/logs/memory and verifies actual test denominators
and unchanged source/engine bytes. The command selects an existing private runtime;
it is not a product installer. Product automatic installation is covered by
[runtime setup](RUNTIME_SETUP.md).

Protocol design follows the official [Fetch domain](https://chromedevtools.github.io/devtools-protocol/tot/Fetch/)
and [Target domain](https://chromedevtools.github.io/devtools-protocol/tot/Target/).
Native Crawl4AI captures returned HTTP 200 with substantive method/event content;
their SHA-256 values are respectively
`289bdfffece324fe30347b28df0db6f5c951b71aac8906715b6780ddc4137dc6` and
`7c70499028fa006cfd63cd20d60a0e319fbef94c35cae56304ad624aab9b5b14`.
No third-party implementation was copied. Separate native Chromium source API
captures informed investigation of inherited external-protocol cancellation;
that reading is not runtime broker qualification and does not close that gate.
