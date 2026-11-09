# Career history

Implemented in the local web prototype on 2026-11-09. The Profile editor supports
up to 20 employment and 20 education entries, with independent identities,
ordering, descriptions, dates, ongoing status and a checked-details flag.
Saved records live inside the existing candidate profile, using its encrypted
storage and revision checks. This adds no plaintext history database or external
service.

Dates retain their supplied precision: `2020` stays a year and `2020-06` stays a
month. Empty dates remain unknown. An end date incompatible with an ongoing
entry is refused; an unsuccessful toggle preserves the original valid end date.
Chronology rejects a definitely earlier end, without inventing a month when
year-only intervals overlap. Input is versioned, bounded to 64 KiB, and rejects
unknown properties, invalid text and duplicate identities across both sections.

Editing a value clears that entry's checked flag. Only saved, checked entries
appear in the existing saved-value picker for nonsensitive text answers. The
user chooses the entry and component explicitly; its exact text is copied into
the answer editor. This does not fill a website, approve an action, attest a
qualification claim or authorize submission. Sensitive fields do not gain a
saved-value picker. An ongoing flag is not converted into an invented end date.

Save uses the existing optimistic profile revision and interrupts an old review
session. Concurrent edits are refused, with an explicit comparison/rebase path
that keeps the unsaved history when both editors changed it. History conflicts
are handled as a complete field; individual entries are not automatically merged.
An older editor omitting the new field preserves the saved history. Explicit
clearing still requires the current revision. Unreadable loaded data stays
visible as an error and cannot silently replace valid saved history with empty
data. Removal requires a second, labelled confirmation action.

## Interface and checks

The editor follows the shared ivory, white and forest-teal design system. Entry
legends use the entered role/qualification and organization. Opening the Profile
editor expands its card across the workspace; narrow containers stack fields.
Description boxes use the native mouse resize handle with a bounded height.
Reorder/removal controls retain usable focus targets, and checkboxes use the
shared accent color.

`docs/design/2026-11-smart-jobseeker/career-history-reference.png` is an original
generated hierarchy reference, not a product screenshot. Implemented viewport
captures were inspected after two refinements: expanding the open editor into
unused workspace space and correcting screenshot scroll/notification handling.
The reference depicts a standalone editor; the implementation remains inside
the existing Profile workspace and its responsive navigation. No pixel-parity
or complete accessibility qualification is claimed.

Local validation:

- Complete Windows web suite: 710 passed, zero failed, 391 opt-in checks ignored.
- 159 standalone UI scripts passed; three new history parser/validation tests
  are included in the web suite. Browser/model tests skipped by that suite are
  not counted as executed.
- Eleven headless workflow case groups passed against the built SQLCipher app:
  partial-date save/reload, edit invalidation, ongoing refusal, checked-value
  reuse, keyboard activation, reordering, confirmed removal, concurrent save and
  conflict rebase, malformed-data preservation, five viewport widths, and native
  mouse-drag description resizing. These are grouped observations, not eleven
  independent model trials. No model, real candidate or employer form was used.
- The populated-editor layout matrix passed 296 cases with zero failures or
  script errors, covering actual browser zoom, viewport changes, mouse-drag
  dividers and the existing menu/dialog paths. Widths span 320–3840 pixels and
  zoom factors 80–400% where the effective viewport fits the supported minimum.
  This is Chromium evidence, not qualification of every browser, DPI or device.
- Windows Clippy, formatting and managed build passed. The workflow and final
  layout receipts identify the same executable bytes. Private evidence remains
  under ignored `target/`; their SHA-256 values are respectively
  `191fab49ed71197f2c15842d956f51f37cfbed2bb0a75d9609c7150b909a76f8`
  and `75bfcd7f8543032182b33ac78db9db28c69037a3ef241149060ab00611c30609`.

Reproduce after the managed Windows build:

```powershell
node apps/web_app/scripts/qualify-candidate-history.cjs
node apps/web_app/scripts/qualify-layout.cjs
node apps/web_app/tests/candidate-history-ui.test.js
```

The headless scripts require the development-only `playwright-core` installation
documented by `qualify-layout.cjs --help` and an installed Chromium executable.
They use disposable workspaces; `--browser` and `--executable` select explicit
local paths. The measured runs additionally used the existing owned-process
controller with a 4 GiB job and bounded run time. No paid API was used.

## Scoped local suggestions

The form review now offers **Suggest answers from career history**. The user
selects an inspected section and up to 20 eligible fields, then lets the local
model interpret its purpose or chooses a saved, checked entry explicitly.
Automatic resolution uses current/latest-completed/unique-category rules across
all checked entries; ambiguous requests remain manual. The local model selects
component keys; the application resolves exact values from the selected
record. It cannot generate a new employer, qualification or date. Available
components preserve year/month precision and field-type compatibility. Unknown,
ambiguous, sensitive, duplicate-label and unsupported fields require review.

The result is bound to the profile digest, selected entry, inspected schema,
session and destination. A profile/session change invalidates the result,
including cached and late completions. Suggestions start unchecked. Explicit
selection copies values only into the existing answer editors, after another
binding check; any newer manual edit prevents the entire selection from applying.
There is no browser action, grant or submission in this operation.

The contact-key mapper still excludes serialized history and refuses applicant
contact values for employers and educational institutions. Its model instructions
now distinguish ordinary candidate career facts from sensitive questions and
other people's contact details. Existing model-review blocks remain in force:
history suggestions never clear a flagged field. A deterministic subject guard
also refuses history values for recognizable referee/manager/emergency-contact
labels even when the model tries to map them. Unclear subjects remain manual.
The feature does
not assign records across repeated sections autonomously, tailor documents or attach authoritative
claim provenance to an answer archive. Neither order nor a checked flag proves
that a role is current or a qualification completed.

In the earlier 2026-11-09 checkpoint, a fictional six-field live check with local
`gemma4:12b-it-q4_K_M` returned four expected grounded answers, left two unrelated
questions manual, filled/read back four fictional controls and passed two visual
comparisons. `ministral-3:14b` returned three of four expected answers, leaving
the month manual; that completeness check failed and is retained. These are
small diagnostics, not a broad platform benchmark or a reason to change the
user's selected model automatically.

The subsequent [integrated qualification](HISTORY_PIPELINE_2026-11-09.md) used
two different 11-field layouts. The local 12B model produced seven grounded
contact/history answers in each; all seven passed visual transcription and live
value checks. Four restricted fields stayed blank. This covers one selected
employment record and one selected education record, not autonomous record or
section selection. Native month display reconciliation uses typed Gregorian
locale data, retaining the original transcription and requiring live readback.

The compiled interface passed five additional advice-panel width checks from
320 to 1920 px and explicit-selection behavior with intercepted fictional API
responses. Those UI checks make no model-quality claim. Broader section resolution,
visual accuracy and no-submission isolation remain open completion gates.

The subsequent [section-selection checkpoint](HISTORY_SECTION_SELECTION_2026-11-09.md)
adds stable section identities, an automatic entry option and a section picker.
The local 12B model selected three appropriate records from multiple saved
entries and completed ten visually/live-verified values in one fictional form;
four restricted fields stayed empty. Seven additional semantic descriptions
passed. These are small checks, not universal model or employer-site evidence.

The [answer-source checkpoint](HISTORY_ANSWER_SOURCES.md) subsequently connected
those values, and explicit saved-value choices, to checked source links in fill
requests and application archives. Editing removes the link. Rust rechecks the
current saved entry before filling, while the viewer flags later source changes
without altering historical answer files. This is source traceability, not
authoritative claim verification or a new model-quality result.
