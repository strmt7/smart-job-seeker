# Browser rehearsal progress · 17 November 2026

The Windows browser relay now waits for a command, a completed output line or
a budget check. A command can wake it while it is waiting for browser output.
The existing byte, queue, session and shutdown limits still apply. Method-only
timing is opt-in for isolated replay. It records durations and counts without
capturing protocol parameters, answers or browser response bodies.

Optional dialogs now get one attempt per distinct observed description in a
section. The next verified section starts a fresh history. A repeated dialog
stops for review before another model request or click. An earlier section's
archived entries no longer block a new dialog. Active field captures retain
their original form binding. Target review, native closure, field-value
preservation and replay checks apply to each admitted action.

## Matched measurement

Both runs used the same first section of the frozen, fictional 20-field replay
and a 2 GiB owned Windows job with the model disabled. The test verified four
values and one guarded transition. The baseline was `62ba956a` plus the
measurement harness. The second build changed the Rust relay and used the
same Python worker, headless engine, archived bytes, test and field checks.

| Measurement | Polling relay | Event-driven relay |
| --- | ---: | ---: |
| Contained wall time | 99.765 s | 38.902 s |
| Measured section and transition | 97.119 s | 36.264 s |
| Browser calls | 3,026 | 3,026 |
| Total time in browser calls | 94.587 s | 33.744 s |
| Failed or omitted calls | 0 | 0 |

The full fixture then verified all 20 fields in five sections and archived
the terminal review view in 173.062 seconds. The screenshot showed the review
summary and the unchanged submit button. The simulated submission sentinel
stayed inactive. The run recorded 14,607 calls, zero failed calls and zero
blocked or unreplayed transports. The saved review observation still awaits
independent verification of the final summary.

## Validation

The Windows workspace passed 1,470 tests with 620 opt-in tests ignored. The
development build, warnings-denied Clippy and formatting passed. Three
explicitly selected headless tests passed under separate 2 GiB jobs. They
covered immediate reuse, consumer-failure cleanup and seven dialog cases
including changed values, stale controls and a scripted submission attempt
against the isolated fixture. The new admission test accepts three distinct
descriptions and stops when the first repeats in the same section.

The qualification RAM sampler now observes until teardown and tracks the
minimum across all successful readings. Its retained half-second history stays
bounded at 900 rows. A regression test crosses that retention limit and checks
the minimum from an earlier discarded row. All nine focused qualification-tool
tests passed.

## Local-model workflow

The existing private Qwen 3.8 27B run verified seven of the eight expected
fields through native readback and visual review. It advanced four times and
stopped in the last section with `required_fields_unverified`. The test
failed after 800.651 seconds and retained its seven progress records and the
earlier field recovery evidence. The browser replay recorded zero delivery
attempts. Native shutdown and removal of the private inference listener were
observed.

Ollama 0.40.16 admitted a 16,384-token context and configured `xhigh`
deliberation. It reported 12.825 GiB allocated to the GPU and zero offloaded
model bytes. The older sampler retained its first 900 readings, covering
457.672 seconds with 11.878 GiB as the minimum observed host availability.
The separate host-reserve watchdog ran until the contained process exited.

For the omitted field, the first structured response chose manual review. The
reasoning pass correctly identified the supplied given-name key, and a second
structured response agreed. The original explicit review decision still
blocked entry. The general mapping instructions now distinguish the fact a
field requests from its section heading. They reserve manual review for
government identifiers, credentials, attestations and other actual sensitive
requests. A batch with only mandatory review decisions skips the additional
reasoning pass. Explicit review decisions retain their authority.

The focused follow-up opened the last section with its original label,
autocomplete, control markup and required value. The private model proposed the
correct key on its first mapping call. The app verified the one field by native
readback and local vision, archived the observation and stopped when the section
repeated. The test passed in 73.051 seconds.

The byte-identical five-section regression still verified 7 of 8 fields and
stopped at `required_fields_unverified` in the final section. The first mapping
response marked that field for review again. The revised path made one mapping
call for the section and ended in 445.922 seconds. The full run retained 875
host readings with 11.667 GiB as the observed minimum. Private inference exited
and its listener had zero owners. The isolated success leaves sequence-dependent
mapping behavior open. Compare effective request inputs and decoding across
isolated and in-sequence cases before changing review authority.

Full timings, source and binary digests, and captures are retained under
ignored `target/native-perf-*`, `target/workflow-limit-audit-62ba956a`,
`target/relay-qwen-check-62ba956a` and `target/contact-scope*-62ba956a`. The
native timing comparison covers one headless engine and one development fixture.
Representative local-model completion and held-out cases remain pending.
