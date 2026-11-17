# Progress-based saved-page rehearsal · 17 November 2026

The rehearsal no longer stops at 16 entries, two forward actions, or four
additive reinspections. Forward actions still require fresh native state,
visual confirmation and verified required fields. The browser refuses a
previously inspected field set. Conditional entry still requires a verified
same-document addition that preserves all earlier field descriptors and values.
Earlier conditional views also participate in the forward cycle check.

The worker uses the configured browser-session deadline for inference admission
and continued action. One screenshot sequence covers view, target, field and
recovery images. Each attempt can retain up to 256 MiB of screenshots. Native
value checks keep their bindings after reviewed pixels are released from memory.
One immutable checkpoint per entry replaces repeated copies of all earlier
entries. A failed later capture preserves the existing views and its stop reason.

## Verification

- 1,468 Rust tests passed with 618 ignored. All 172 UI scripts passed. The
  archive regression saved 85 distinct images, enforced the byte budget
  without overwriting a prior image, and exercised a checkpoint at index 65.
- The compiled Windows app saved a fictional one-field preview, retrieved
  its `0001.png` through the authenticated screenshot route and matched the
  image digest. It also deferred and reopened a separate 468,306-byte
  history record without rewriting it. Clippy and the Windows build passed.
- The headless visual suite passed 65 checks. Saved-page details were
  inspected at 320 px and 200% browser zoom.
- A fictional 20-field, five-section test froze its expected answers and
  returned to its first section after the last Continue. The first Qwen run
  reached the initial four-field view and produced no usable mappings. It
  entered zero fields and closed its owned runtime. A subsequent build
  preserves the first mapping error alongside the existing batch warnings.
- The first diagnostic retry stopped at unconfirmed model startup. Its
  containing job admitted 11.40 GiB. Whole-host samples remained above
  9.52 GiB of available RAM. The cause of startup failure was unverified.
- With a 13.75 GiB containing job, Qwen proposed all four initial entries
  in the 20-field fixture. The first entry passed native and visual checks,
  including recovery after a simulated pre-request memory refusal. The second
  entry passed native readback. The main image transcription matched and the
  blind detail read returned an empty value, so the rehearsal stopped on
  disagreement. All archive transport counters for blocked or unreplayed
  activity were zero, and the model session closed. Whole-host available RAM
  sampled at or above 9.09 GiB.
- Blind detail transcription now uses visible styling to identify
  placeholders and reports ambiguity without guessing that a visible control
  is empty. The prompt receives no expected value or target identity. Both
  observations and the disagreement rule are preserved.
- The first run of a shorter fixture stopped during owned-model startup at
  8,133 MiB available RAM. The shared-runtime continuation policy required
  8,192 MiB. A verified app-owned model job now uses a 4 GiB request floor,
  with the existing startup admission, commit limit and 3 GiB emergency
  stop. The boundary test admits the observed 8,133 MiB sample under the
  owned policy and refuses one byte below 4 GiB.
- The repeat under the owned policy opened the form at the intended 16,384
  context. Ollama returned HTTP 500 during the first mapping request and the
  rehearsal entered zero fields. It closed the model session. The adapter
  now preserves a bounded native error message alongside the status code.
- The next diagnostic run stopped at startup admission. It observed
  19,409 MiB available against the 22,642 MiB startup estimate. That run
  produced no generation response. The earlier HTTP 500 cause and the
  expanded workflow remain open.

## Follow-up

The private preload sends `num_thread=2` and `num_batch=128`. Current
text and vision generation carry the resident context while omitting
those runner settings. Test whether the mismatch recreates the runner or
raises allocation pressure and bind the effective options across those
requests. The external generate specification documents request options.
The scheduler source page returned HTTP 503 and supplied no usable
source evidence. [Ollama Generate API](https://docs.ollama.com/api/generate)
