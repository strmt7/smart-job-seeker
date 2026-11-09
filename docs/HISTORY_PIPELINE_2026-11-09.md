# Career-history integration qualification — 2026-11-09

## Problem and implementation

The contact mapper's instructions previously put any employer/organization
question into the same manual category as referee and emergency-contact details.
That prevented the separate reviewed-history flow from handling ordinary employer
names and career fields. Its instructions now distinguish ordinary candidate
history from genuinely sensitive, other-person and organization-contact questions.
No existing model-review flag is cleared or bypassed.

Applicant contact values remain forbidden for employer and school fields,
including misleading autocomplete hints. The conservative organization guard
also recognizes educational institutions. History resolution independently refuses
recognizable other-person labels even if the model proposes a valid candidate
history component. Model calls still select keys, not newly invented values.

Visible group context is retained in labels. The two fictional layouts exercise
fieldset/legend grouping and visible headings with semantic/ARIA grouping, in
line with the [W3C grouping guidance](https://www.w3.org/WAI/tutorials/forms/grouping/).
No site-specific adapter or employer-specific field rule was introduced.

## Local-model observations

`history_pipeline_tests::resource_gated_history_pipeline_keeps_subjects_separate`
runs the production contact suggestion path, reviewed history resolution,
content/schema-bound prefill coordinator, actual headless controls, local vision
transcription and independent live-value checks. Only its two owned fictional
fixtures are selectable; there is no external application or submit control.

With `gemma4:12b-it-q4_K_M`, both layouts produced the expected two applicant
contact suggestions and five career suggestions: employer, title, starting month,
institution and qualification. All **seven of seven** intended fields passed DOM
and visual checks in each layout. Referee name, referee employer, employer postal
address and pay expectation remained blank (**four of four** restricted fields).
The second layout changes wording and replaces fieldsets with heading/ARIA groups.
The successful runs took 34.57 and 35.01 seconds respectively on this host.

These are two controlled examples, not a majority-of-platforms result. The user
chooses the history entry and field subset; the test simulates explicit review
only for its fictional grant. It does not qualify autonomous repeated-section
selection, real-site safety or application submission. The user-selected model
is not changed. Earlier 14B completeness failures remain recorded separately.

Schemas, model/digest metadata, textual request/response traces, source hashes,
filled screenshots and visual results remain under ignored `target/history-pipeline-*`.
The second layout's metadata includes classifier, resolver, normalizer, visual
checker, qualification source and lockfile hashes. Both screenshots were inspected.
No real candidate data was used or committed.

## Native month visual checks

The first integrated run correctly filled and visually transcribed the month but
failed literal comparison: the visible **June 2020** differs from the native DOM
value **2020-06**. That failed run and its original observations were retained.

Native-month reconciliation now uses ICU4X 2.4 / CLDR reference formatting with a
fixed Gregorian calendar and explicit full-year precision. Its 42 locale
preferences provide names, ordering and digit shaping; they are not hardcoded
translations of field labels. Only unambiguous reference strings are accepted.
Two-digit years, partial dates, added days, wrong months/years and ambiguous
references do not pass. Unknown display forms remain review concerns. The
two-year reference cache is bounded and never changes the host locale.
See [the formatter's documented API](https://docs.rs/icu_datetime/2.4.0/icu_datetime/struct.FixedCalendarDateTimeFormatter.html).

Normalization applies only to a completed native month control with one matching
field, one observation and its grounded typed value. The original transcription
and canonical expectation remain in the result, with a normalization note.
An independent live-control check must still pass. Text fields do not gain date
equivalence. A headless adversarial check verified that changing the live control
to another month defeats an otherwise matching transcription; duplicate labels,
missing completion and the wrong control type also remain unconfirmed.

The formatter's initial automatic name-loading path exposed a missing numeric
formatter for a locale pattern alternative. Explicit numeric-data loading through
its public names/builder API resolved that failure; no upstream source was edited
and no locale was removed to make the check pass. Pure normalization checks cover
English, German, French, Spanish and Chinese strings plus wrong/partial dates and
cache bounds. This does not establish every native browser/OS locale rendering.

## Validation and remaining gates

The web suite passed **722 tests**, with **396 ignored**; opt-in checks above are
separate. Relevant contact and adversarial history tests passed. Clippy, dependency
advisory and license/ban/source checks passed; the license inventory was refreshed.
The calendar library is compiled into the app, introducing no host prerequisite.

Real applications were not interacted with or submitted. Broad held-out model
evaluation, multi-step/repeated-section behavior, unsupported calendars/displays,
authoritative answer provenance and complete no-submission isolation remain open.
