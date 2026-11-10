# HTTP resource capture and replay bridge

Checkpoint: 2026-11-10. The network layer can retain response metadata, and a
Rust snapshot builder can turn already-obtained resource responses into the
[offline replay archive](RECORDED_RESPONSE_REPLAY.md). A fictional native test
now connects those components to the actual contained browser. This is a
prerequisite for permitted-site capture, not a live capture coordinator or an
interface feature. Employer-form interaction remains disabled during development.

## Network behavior

The former shared reader discarded headers and could return a body shorter than
its declared length. It also detected chunking with a header-substring search.
The new reader reuses `httparse` for headers and chunk syntax, tracks framing
explicitly, preserves ordered header/trailer bytes and the request method, and
refuses incomplete or ambiguous responses. Existing JSON callers retain their
status/body API; their truncated responses now fail instead of becoming data.

The reader handles bounded informational responses, HEAD/204/304 body precedence,
consistent repeated content lengths, chunk fragments and trailers, and binary
bodies. Unsupported transfer codings/upgrades, conflicting framing, malformed
chunks and budget overruns are explicit errors. Close-delimited responses remain
identified as such; their completeness cannot be established from EOF alone.
The implementation follows [HTTP/1.1 message framing, section 6.3](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3).
This is not qualification of every HTTP server or all protocol extensions.

`get_asset_capped` performs one credential-free public HTTPS GET on port 443,
without redirects, cookies, authorization or browser request-header forwarding.
It requests identity encoding and refuses encoded bodies rather than decoding
unbounded data. All resolved addresses retain the existing public-IP check and
the actual socket connects to a checked address. A shared remaining deadline
covers connection attempts and socket/TLS reads and writes. OS DNS resolution is
still synchronous and not cancellable here; callers must retain an outer worker
lifetime. A request permits 1–60,000 ms and at most the existing 32 MiB wire cap.
A separate bounded stream reader permits development and transport-independent
capture without opening a connection. These methods do not authorize sources:
permission, robots and candidate-data checks belong to the capture caller.

Query-only URLs are framed correctly and fragments stay out of HTTP requests.
Request targets/methods containing control characters or invalid raw bytes are
refused before writing. Public TLS continues to use Windows' native trust store;
no new host executable, DLL, network proxy or paid service was introduced.

## Snapshot contract

`SnapshotBuilder` writes only into a new caller-selected directory. Each accepted
GET body is saved once with SHA-256 inventory binding. HEAD responses cannot be
relabeled as GET bodies. Completion writes the existing strict `replay.json` plus
a capture report. Failed additions leave an incomplete marker and poison the
builder, preventing an ignored failure from activating a partial snapshot.
Finalization failures do not return an activatable configuration. Earlier files
are not overwritten or deleted.

Limits are 512 resources, 4 MiB per body, 1 MiB for the manifest and **64 MiB for
all bodies plus manifest/report/state files**. Final metadata is included in the
same content budget used by the browser worker. Entry HTML must have a successful
response, an HTML content type and the exact canonical URL without its fragment.
Generated filenames avoid copying untrusted paths. Duplicate request identities,
partial/not-modified responses, uncertain close-delimited bodies, trailers,
authentication responses, non-UTF8 or duplicate retained fields, unsupported
header-dependent variants and encoded bodies require review. Hop-by-hop fields
are removed, including fields named by `Connection`; response cookies are
omitted with an explicit `response_cookies_not_replayed` warning. No cookie jar
or authenticated browser state is restored.

The report explicitly leaves source permission to the caller and server
validation unperformed. A complete archive contains the supplied resources; it
does not establish that every page asset or application step was captured.

## Evidence

- Final full-workspace validation through the managed Windows helper passed
  **1,153 tests with 573 opt-in cases ignored**, across 41 reported targets,
  including empty documentation-test targets. Workspace Clippy and the app build
  passed. Audit and license/ban/source checks passed without new exceptions.
  The locked package versions did not change: three direct dependency edges
  reuse existing `httparse`, `url` and test-only `tempfile` packages. The license
  inventory remains current at 362 packages / 16 workspace members.
- Protocol tests cover every tested fragment boundary, binary/header preservation,
  incomplete declared lengths, informational/bodyless responses, exact field
  matching, conflicting lengths, chunk/trailer corruption and size/count limits.
  Snapshot tests cover inventory hashes, cookie/hop-field omission, failed
  activation, unsupported representations, URL identity, existing directories,
  body/metadata budgets and HEAD/GET identity. They run in the normal workspace
  suite. A first broad test command failed because it guessed an OpenSSL library
  subdirectory; the library actually resides at the private SDK's `lib` root.
  `build-windows.ps1 -Workspace -Test|-Clippy` now uses the existing layout
  discovery and automatic SDK provisioner. Helper regressions preserve default
  package selection and strict lint/ignored-test rules. No SDK was missing.
- A separate, explicitly selected public API check passed against the registry's
  permitted Greenhouse read route after a same-URL robots-checked capture. It
  retained a **61,477-byte JSON body with 87 postings**, HTTP 200, Content-Length
  framing and the original header bytes. It does not establish discovery,
  ranking, deduplication or employer-form completion. Saved response record fingerprint (SHA-256):
  `ab1d7ba5498344ab5ff3fcc3e848a269cb9a3ce7867fbbaaf2a7f955bfe20132`;
  body SHA-256 `a799d84e658aa7155b660e9b779d397a980f1d5e5012eb65540090c016e610b5`.
- The final fictional bridge passed **one selected native test, zero ignored**:
  raw HTTP responses, including chunked JavaScript/JSON, became a hash-bound
  archive; the real AppContainer browser rendered HTML/CSS/scripts, entered
  Unicode text, loaded a recorded next section and verified an email field.
  Four cached GET responses were acknowledged. Cookie values were omitted,
  `commit` refused, and normal close completed. The final PNG was directly
  inspected. This is not a renewed general no-egress qualification.

Private bridge evidence is in `target/capture-replay-qualification-7wm0a3p7/`.
Receipt SHA-256:
`780622e7a8a16972cb502516046639c8ac48ca943ff33dfd178f3e5855827211`.
Native result SHA-256:
`f0a380920370c1a565e63fff4628785edace65adacfad6bd683259307d0822e3`.
PNG SHA-256:
`cbb17f7e6bd98f8b5ec041e905737b25b689bb98b67313bc0efbed0224e8bf56`.
The contained wrapper took **27.151 seconds**, including Cargo startup/build,
inside a 4 GiB job. Fifty-four nominal 500 ms whole-host samples reported at least
**19,685,412,864 free bytes (18.333 GiB)**. Source and engine fingerprints remained
unchanged during the check. These are sampled host observations, not exclusive
application/GPU peaks or reservations. No model, candidate files, employer form,
application delivery, email delivery or desktop interaction was used.

Research retained native HTTP-200 Crawl4AI evidence for the RFC and public API;
SHA-256 values are `635dad7215360c114093275ce1ff5057d3cfc652a0dd2ae52e4d7089292127b7`
and `99ebfb55203a17c5eab7aba506d2098d1a301f6da6b523bb96e3257da8519fcb`.
Native crates.io metadata confirmed `httparse 1.10.1`, under `MIT OR Apache-2.0`.
No third-party application implementation was copied.

## Next integration gates

The live browser-driven capture coordinator still needs independent per-resource
permission/robots checks, budgets/cancellation, explicit handling of blocked
resources and missing steps, and a structural end to all fetch capability before
candidate entry begins. GET alone does not prove an endpoint has no side effect.
Do not allow candidate-bearing requests, authenticated sessions, automatic
application retry or live fallback into this capture path. Server-dependent
POST questions, cookies, unsupported encodings/variants and broader target
lifecycles remain incomplete. The remaining [AppContainer public/broker and
failure-schedule gates](ISOLATED_REHEARSAL.md) still apply before real interaction
or interface exposure. No release readiness is claimed.

Reproduce the fictional bridge with an existing private engine:

```powershell
py -3.13 -X utf8 -B tools/qualify_capture_replay.py --engine <absolute-headless-shell-directory>
./apps/web_app/scripts/build-windows.ps1 -Workspace -Test
./apps/web_app/scripts/build-windows.ps1 -Workspace -Clippy
```

The qualifier accepts an explicit `--python`, uses the shared bounded process
wrapper, verifies executed denominators and code/engine inventories, and keeps
failures and screenshots under ignored `target/`. These are development tools;
product installation does not require host Rust, Python or a developer SDK.
