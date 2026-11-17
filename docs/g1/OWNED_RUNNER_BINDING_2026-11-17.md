# Owned Ollama runner settings · 17 November 2026

The private rehearsal worker now passes the same runner options to preload,
structured text, reasoning and vision. It records the requested options and
verifies the runtime's reported context. A different context stops the request.
The same binding applies to the owned public-research qualifier. Borrowed
runtimes keep their existing request options and memory policy.

Vision now uses the verified owned-runtime guard during generation. Previously
it also ran the shared-runtime memory check, which could stop an owned request
even when the private job's guard admitted it. The process memory budget and
emergency stop still apply.

## Verification

- Loopback tests compared the text, reasoning and vision request bodies with
  the preload options and refused a changed resident context before sending a
  generation request. They also verified that borrowed requests omit those
  runner settings.
- The resident-context reader now uses the same 262,144-token ceiling as the
  adapter's explicit context setting. A loopback test carried a reported
  262,144-token context into generation. The private Qwen default stays at
  16,384. This test qualifies request handling only.
- The Windows development build, warnings-denied Clippy and all 1,469 workspace
  tests passed, with 618 ignored. The test binary SHA-256 was
  `2c778d252e4d45e1694f562d1bd309b86dc6cf235770a983aeadee1e24cee3d7`.
- The same eight-field, five-section fictional fixture is prepared for the GPU
  replay. The host admission sample was 19,428 MiB available against the
  22,642 MiB startup estimate, so this checkpoint has no new Qwen response.

The previous Ollama HTTP 500 remains unresolved. Matching the request settings
removes a known source of drift. Confirm native runner reuse, actual generation,
the full replay and teardown when the host can admit the run.
