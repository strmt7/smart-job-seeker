# Independent field review - 11 November 2026

This checkpoint demonstrates partial offline entry on one saved real employer
page, not general application completion or release readiness. No employer form
was interacted with live and no application or application email was sent.
Candidate values were fictional. Native captures, advertisements, screenshots,
model files and detailed receipts remain under ignored `target/`.

## Production changes

Native inspection now distinguishes page barriers from unresolved completion
notes. Identifiable custom widgets, required hidden controls and a passive
challenge-script reference retain their review notes without preventing a
separate, supported offline field from being tested. An actual challenge element,
login wall, uncertain inspection or changed schema still prevents entry.
Eligibility is bound to the inspected schema, origin and complete review-note
snapshot. Legacy callers without this evidence still treat every wall as a stop.
All notes remain in the existing `walls` collection: section advancement and
completion checks have not been relaxed. This does not enable live prefill.

Focused visual review retains its validated observations in the attempt record.
The prompt does not supply the expected label, control type or answer before
entry. A unique, natively inspected required field can align one rendered
trailing asterisk with its accessible label. Optional fields, conflicting field
labels, duplicate observations, other punctuation and mismatched types do not
qualify. Original observations remain intact; answer values are compared exactly.

The rehearsal uses the existing shared eight-field budget instead of an
additional four-field-per-section ceiling. Its two-section-advance limit is
unchanged. A visual refusal before typing is now explicitly distinguished from
unverified entry. Hard barriers and omitted-note counts appear in the preview;
secondary completion notes use the existing disclosure pattern. Stops remain
visible outside that disclosure.

## Measured local-model behavior

The compiled production route reopened the same saved Greenhouse archive with
the selected local `gemma4:12b-it-q4_K_M` model, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.
It used an owned private runtime and an offline browser in the existing
zero-capability AppContainer. No new employer fetch occurred during these runs.

Before marker alignment, the model correctly transcribed `First Name*`, but
the matcher expected `First Name` and stopped before entry. That failed run is
retained as `target/real-form-vision-l3hduo2t/`.

After the narrowly scoped repair, `target/real-form-vision-nkq_erix/` recorded:

| Measurement | Result |
| --- | --- |
| Supported controls inspected | 9 |
| Before-entry target reviews attempted | 4 |
| Target reviews matched | 3 |
| Fields entered, read back and visually matched | 3: first name, last name, email |
| Target reviews stopped before entry | 1: Phone |
| Sections advanced | 0 |
| External submissions | 0 |
| Owned invocation duration | 110.182 seconds |
| Minimum sampled whole-host available RAM | 19,315,081,216 bytes (about 17.99 GiB) |

The Phone crop showed the end of a country list and a different nearby label;
the Phone label was outside the crop. The model omitted Phone and the run
stopped correctly. The saved page is still incompletely styled. Fix rendering
and label-context capture rather than accepting a guessed label. Attachments,
custom choices, later steps and general employer-site success are unproven by
this run. Three matches are not an accuracy estimate over applications.

The controller verified private runtime exit, drained its owned process tree,
and found the shared Ollama service unchanged and idle. RAM sampling is not
GPU utilization, an exclusive application peak, or a long-duration leak test.
The model measurement preceded the final secondary-note disclosure refinement;
its receipt binds the actual measured binary and dirty source hashes.

## Validation and retained limitations

- Workspace tests: 1,286 passed, zero failed, 591 ignored. Ignored browser,
  model and network cases are not counted as passed coverage. Clippy with
  warnings denied and formatting checks passed.
- New unit cases cover snapshot invalidation, legacy walls, custom/hidden
  review notes, active challenge barriers and required-marker collisions.
- Native contained-browser qualification verifies independent entry while a
  custom widget remains unresolved, refused section advancement, and refusal
  after a visible fictional challenge widget is inserted. This case and the
  existing capture privacy and transitive-resource discovery regressions each
  passed one native test in separate contained invocations.
- Focused client regressions cover truthful statuses, entry barriers outside
  collapsed notes, omitted counts and existing outline behavior.
- The production preview component rendered actual rehearsal metadata at
  widths 1,360, 480 and 320 pixels and CSS zoom factors 1, 1.5 and 2: nine
  combinations with no document-width overflow. Desktop and narrow screenshots
  were inspected. This is component/CSS-zoom evidence, not complete workbench,
  operating-system scaling or browser toolbar-zoom qualification.

Private evidence is in `target/read-query-capture/`,
`target/capture-replay-qualification-gkzbu9dg/`,
`target/capture-replay-qualification-huy16lea/`,
`target/capture-replay-qualification-by1tapny/` and
`target/review-panel-2o268n34/`. Earlier failed fixture invocations remain
retained and do not count as passes.

The saved Ashby page's remaining data requests target a robots-disallowed API
path. They were observed offline and not sent externally. GraphQL protocol
research does not override that restriction. The professional
[Set-of-Mark reference](https://github.com/microsoft/SoM) was also read through
the bounded research provider; no code, segmentation dependency or screenshot
annotation mechanism was added. The observed asterisk mismatch had a smaller,
independently testable repair.
