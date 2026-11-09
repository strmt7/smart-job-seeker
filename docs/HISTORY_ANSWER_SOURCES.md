# Career history answer sources

Implemented and checked on 2026-11-09. Checked history now connects to reviewed
answer payloads, the production fill endpoint and saved application records.
This is source traceability, not independent verification of someone's experience
or proof that an answer is appropriate for every question.

## Selection and validation

Both scoped model suggestions and the manual saved-value picker can link a
history entry and component to an answer. The picker resolves the current value
through `/api/form/history-value`, without a model or browser action. It preserves
an answer or selection changed while the request was running. Shared input
handlers remove the source link when the answer is edited; reverting the text
does not restore that link. Choosing another draft also removes it.

The fill endpoint validates all linked values before browser writes. It refuses
missing, unchecked or changed entries, edited values carrying an old link,
unknown components, duplicate fields, conflicting draft links, incompatible
types, other-person subjects, model-review blocks and unselected fields. Validation
does not partially publish metadata when a later link fails. Exact saved text and
unique matching option labels are distinguished. A source hash cannot authorize
a fill or clear a review block; the existing one-use approval remains required.

Each answer keeps its own source. A shared DOM container can ask about current
and previous roles, so it does not establish that every answer must use one
record. A scoped automatic advice request selects one complete record for its own fields.
[Whole-form advice](WHOLE_FORM_HISTORY.md) classifies each question separately,
then resolves its checked record and checks the proposed answer. That distinction
avoids imposing a layout assumption on either explicit or whole-form selections.

## Application records

Requested-answer files now include `source_career_history` with entry identity,
kind, label, component, source/profile/schema digests, checked status and value
relationship. Only the selected source is recorded; unrelated history entries
are not copied. The digest encoding matches earlier history proposals.

The viewer compares archived links with current checked history and displays
unchanged, changed/unchecked, unavailable or inconsistent-source states. It also
checks the recorded typed value when the current source still matches. These
annotations are calculated for the response; saved files are never rewritten.
The comparison concerns the source entry, not current validity of the whole old
application. Plaintext archives remain user-editable and unsigned: hashes do not
authenticate a file or independently establish experience or delivery.

Writer and reader share an **8 MiB per-record** limit. The usual read-page target
remains **2 MiB**, but one bounded larger record is admitted on its own page.
Read handles enforce the byte budget even if files grow after metadata checks.
Invalid large records consume that budget and advance the cursor too. This
avoids inaccessible large first records while bounding reads. Publication uses
a temporary file and no-overwrite persistence; rejected writes do not advance
the sequence. A 200-answer snapshot larger than the normal page target was saved
and read completely in the archive test. This is not a 200-field browser test.
The existing viewer still shows the first 100 fields of a snapshot with an
explicit notice; per-record field pagination remains work to complete.

## Measured checks

Private evidence remains under ignored `target/`, using fictional data only.

- `history-lineage-bounded-final-tests.log`: **734 passed, 398 ignored**, no failures.
- `history-lineage-ui-contracts-final.private.json`: **161 JavaScript tests
  passed**. Initial helper-extraction/wiring assumptions in three test harnesses
  failed and were corrected; those failures remain in the earlier log.
- `history-lineage-bounded-endpoint.log`, `history-answer-sources-YWb4Iy/`: one
  headless production-endpoint check passed. The local source-value API returned
  a matching link; forged hashes, edited values and changed profiles were refused
  without field entry. A valid request filled/read back two controls and saved
  source-linked answers and screenshots. Clearing history changed the viewer's
  source status while the saved answer bytes remained identical. Receipt SHA-256:
  `aa71904c2237eaf594dcb2ff11745eb8705ec8ea2ad00480ccc8d47274eeda97`.
- `history-large-answer-archive.log`: the large 200-answer snapshot and pagination
  progress checks passed, including a large invalid record followed by a valid
  one. The over-limit writer check also verifies no published file or sequence
  advance on rejection.
- `history-lineage-bounded-rendered-ui.log`, `candidate-history-nbKRRn/`: **47 grouped
  cases** passed with no script errors, covering exact source payloads, edit/revert
  invalidation, five widths from 320–1920 px, and actual 80–400% zoom for both the
  advice panel and source-status cards. Responses are fictional/intercepted in
  this UI check. Narrow, wide and native 400% captures were visually inspected
  after a spacing/heading refinement. Result receipt SHA-256:
  `8daeb0a139439ceca83b89d16515dc8579defd12cbb70cc1c6d01b278b9d95b4`.
- Managed Windows Clippy and the debug build passed. The rendered check used a
  configured 4 GiB child-process limit, not a measured peak. A post-check host
  snapshot showed **25.46 GiB available RAM**, no resident Ollama model and no
  remaining owned UI browser. This is not a peak-resource or clean-host benchmark.

No new local-model quality measurement is claimed: this change validates and
records existing values. No real application was interacted with or submitted,
and no release was created. Broad real-site coverage, proven no-write isolation,
bulk repeated-row assignment, history-based document tailoring, authoritative
claim dependencies and clean-Windows qualification remain open.
