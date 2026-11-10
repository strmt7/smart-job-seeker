# Career history in document drafts

Follow-up: the [draft-to-PDF qualification](DOCUMENT_PDF_JOURNEY.md) now connects
the real HTTP/client journey to PDF export, saved page images and local visual
review for two fictional one-page documents. The evidence below remains the
earlier generator/preview checkpoint; its missing integration gate is narrowed
by that follow-up, not a claim of universal template or document quality.

Checkpoint: 2026-11-10. CV and cover-letter drafting now includes saved,
checked employment and education records alongside reviewed qualification
statements. A candidate with checked history can start drafting without copying
that history into separate qualification claims. Matching scores and automatic
form-answer generation are unchanged by this addition.

## Source and revision rules

- Only checked records from the versioned, validated career-history profile
  enter the document pool. Unchecked records are excluded. Checking a record
  permits reuse of candidate-entered values; it does not attest their truth or
  authorize an external action.
- Complete records remain atomic. Null dates and ongoing status stay unknown;
  year-only dates remain years. A recorded end date is distinct from an unknown
  ongoing status. An education entry does not establish qualification completion.
  The generation and style prompts preserve these distinctions.
- Each supplied history source has its original record identity, kind, label,
  source hash, statement hash and 1-based fact reference. Only sources actually
  supplied to drafting are bound into the result. Existing selection limits
  still apply; oversized records are omitted with explicit accounting rather
  than cutting through a description or its negations.
- The source bindings survive style revision, saved drafts and export provenance.
  The full candidate profile already participates in the draft context digest;
  editing history or unchecking a record makes an earlier context stale. Current
  context is required by the existing export-provenance guard. This does not
  establish complete correction propagation through every historical PDF,
  material dependency or pending application; those gates remain open.

The preview shows readable source fields and unknown dates rather than raw JSON.
Draft paragraphs and source details preserve line breaks and wrap long text.
Selection accounting distinguishes pooled qualifications from checked career
records and labels the latter as candidate-entered, not independently verified.

## Local-model findings and repairs

The first fictional run used the production generator and local
`gemma4:12b-it-q4_K_M` model. Its CV became first-person prose during the natural
language revision. The style pass now retains the requested document structure;
CVs ask for concise entries and line breaks rather than cover-letter narrative.

A second run exposed a separate loss of content: the model returned education
under an extra root property. The old parser accepted that response, but the
renderer and evidence checker only consumed `paragraphs`. The model's education
text therefore disappeared from the displayed CV without an error. Both drafting
tasks now use a registered, closed JSON schema. All sections must be represented
inside the bounded paragraph array, with explicit fact references. Runtime
validation also refuses unknown root/paragraph properties instead of silently
discarding them. Contact additions respect the existing maximum of 25 facts.

These repairs came from the actual local model's output. No generated sentence
was manually substituted to make the check pass. A schema constrains shape; it
does not establish factual accuracy, source completeness or CV quality.

## Measured evidence

The final private receipt is
`target/history-document-run-zeshdol8/result.private.json`; native results are
under `target/history-document-native-gU5LiO/`. One opt-in Rust qualification
covered a CV and cover letter using checked employment, year-only employment
dates, ongoing education with a year-month start date, and an unchecked director
record containing an invented staffing claim. The unchecked record was absent
from every recorded prompt. Both drafts retained the two checked sources and
their precision; the final CV contained both employment and education entries,
with unfinished studies explicit. Generation and natural-language revision ran
through the local model. Its paragraph evidence check marked all four final
paragraphs supported; direct inspection found no added factual claims in this
small fixture. The same model checking its output is correlated evidence, not
an independent accuracy measure.

Model digest:
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.
Native result SHA-256:
`247af836f2656a41f0b51c5f7de0ec17e66171772184e5a5c420b136868af967`.
The contained command took 55.422 seconds. At least 12,860,854,272 bytes
(11.978 GiB) of physical host RAM remained available across 110 samples. This
is not an application, model or GPU peak measurement.

Three history-source checks cover checked-only inclusion, exact date precision,
unknowns, complete long descriptions, invalid data, fact-number binding and source
changes. Draft checks cover preservation through style/export and refusal of
unexpected sections. The workspace passed 1,192 tests with 577 opt-in cases
ignored across 43 reported targets including empty documentation targets. The
affected drafting module passed 21 ordinary checks with nine opt-in checks
ignored. All 165 UI scripts, workspace Clippy, formatting and the managed Windows
build passed. Dependency versions did not change.

The actual compiled renderer was separately inspected with these privately
retained local-model results at 320/1280 px and at 400% browser zoom. The source
labels, unknowns and CV line breaks rendered correctly, with no measured document
or control overflow. Its private receipt is
`target/history-draft-view-hc047pxt/result.private.json`; the command took 4.622
seconds and ten RAM samples recorded at least 22,492,573,696 bytes (20.948 GiB)
available. This rendering check supplied the fictional draft directly to the
production renderer. It did not exercise profile-save → HTTP draft-start → saved
draft → PDF export as a complete browser journey. No API response was used to
claim that missing integration evidence.

Reproduce the local-model check explicitly:

```powershell
py -3.13 -X utf8 -B tools/qualify_history_drafts.py --model gemma4:12b-it-q4_K_M
```

The command requires the selected model already installed locally and uses the
existing model admission/residency guard. A model it newly loads is unloaded by
the existing qualification ownership guard; another resident workload is not
silently replaced. Source hashes, model calls, drafts and measurements remain
under ignored `target/`, not in the repository.

## Remaining gates

These two fictional drafts do not establish unseen-document reliability,
multilingual quality, source recall, chronology reasoning, correct omission of
irrelevant records, full template fidelity, ATS compatibility, final-PDF visual
quality or complete source-correction propagation. Reviewed fact dependencies,
diverse held-out documents still need qualification. The follow-up qualifies
only the two fictional HTTP/UI/export examples linked above. No employer form, application delivery, application email or
release was used or created.
