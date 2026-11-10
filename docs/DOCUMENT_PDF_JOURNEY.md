# Document drafting through saved PDF review

Checkpoint: 2026-11-10. One headless Windows journey now connects the real web
client and production HTTP handlers to the local 12B model, PDF conversion,
extracted-text comparison, page rendering, local visual review and durable
application records. Both documents are fictional, single-page examples. This
closes a narrow integration gap; it does not establish general document quality
or release readiness.

## Implemented behavior

Drafting supplies explicit numbered sources to generation and natural-language
revision. An invalid document can receive one bounded repair per pass, at most
two repair calls. Repairs must reconsider source support rather than relabel
citations; unknown references are never clamped to an available number. Complete
repair inputs over 8,000 bytes and still-invalid outputs are refused. Repair
metadata records the original validation reason and input hash.

If a cover letter lacks usable opening/closing text after revision, a separate
bounded call proposes neutral framing in the requested language. Failure stays
explicit. This is not a substitute for proper personal signature/layout work.

Visual review now receives reviewed statements, checked career history and
contact values using whole-source budgeting. Its report records how many sources
were supplied or omitted. Candidate-entered sources remain unverified; omission
does not establish a contradiction. The host's UTC date is supplied explicitly,
recorded and bound into the context digest. A new UTC day or changed source
context makes the saved review stale. An unrepresentable host date is refused;
the application does not establish that the host clock is accurate.

Every rendered PNG sent for review is saved under that attempt's `materials/`
folder with its hash, size and page number. A failed image save cannot produce
a claim that all pages were inspected and archived. The original PDF remains
saved alongside the report. Interrupted work preserves available evidence.

The page model returns separate defects and optional suggestions. Up to three
content observations per page receive one additional text-only critical review
against the supplied sources/date, capped at 700 output tokens and 8,000 input
bytes. Unknown, unchecked, malformed and unavailable judgments retain the
original concern. Removed/reclassified observations, reasons and counts remain
visible in the saved audit. Layout and legibility observations are unaffected.
This uses the same local model and is correlated advisory evidence, never an
approval or proof of correctness. Cancellation shares the existing inference
guard and resource admission checks.

## Actual journey and results

The native test saved a fictional profile, checked employment and unfinished
education, a DOCX style template and a manual fictional job through the real
handlers. The real client then drafted a CV and cover letter, saved each as PDF,
checked extracted text against the reviewed text, rendered the pages and invoked
the local visual model. No response was replaced to make the journey succeed.
The test also checked the CV's local readability report, saved draft recovery,
source snapshots, unchanged original template, stale draft/review detection and
refusal to export after source changes. The job remained shortlisted. No form
session, application, email or release was created.

The final run used `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.
One explicitly selected opt-in native test executed and passed, with zero ignored
in that invocation. Both documents contained one page. Their source coverage
was six of six available records, including two checked history records. Both
rendered pages had zero reported clipped characters or overlapping text pairs;
direct inspection found readable one-page output. Original model suggestions
remain advisory. The cover letter still needs a personal signature refinement.

| Material | SHA-256 |
| --- | --- |
| CV PDF | `7f0fe7d070957ac8dfd56063f352c13a9e4c55b9adda3ba538406805d2b45b1a` |
| Cover-letter PDF | `70e178f2749225ab6c04d9814c3f28ff47bfc8a53e6cc0befc7f8e82ccecc466` |
| CV page PNG | `b297bdd14a2535ad2a5a59528c3b030516421f87b17c3573c1bad5b954086903` |
| Cover-letter page PNG | `6f3bef9018c16eae495d3b396aa8e2b6018a7eda9913bc26fe1814b6cdcbbb16` |
| Native result JSON | `fcc0d8265f04d5f71f94dc08354b16e88d2da081e84b7554788281575a8fa452` |

Private receipts are `target/document-journey-run-x7xtbnqx/result.private.json`
and `target/document-journey-native-dcxFkT/`. The contained command took
100.697 seconds. Across 198 whole-host physical-memory samples at approximately
500 ms intervals, at least 11,303,608,320 bytes (10.527 GiB) remained available.
This is not an application/GPU peak or a memory reservation. The runner capped
its owned Windows job at 4 GiB and monitored the shared 3 GiB host reserve;
Ollama's process is outside that job. The ownership guard unloaded the model it
loaded; the subsequent resident-model query was empty.

A separate actual-model probe in the same invocation repaired an unknown source
reference while removing an unsupported claim of managing 200 people. Its
evidence check found the replacement supported by the supplied Rust-tooling
fact. No developer rewrote that model output. This is one probe, not a measured
general repair success rate.

Earlier iterations exposed unknown fact citations, a body-only cover letter,
a false claim that September 2025 was in the future, and a consistency statement
incorrectly classified as a defect. These observations drove the numbered input,
framing fallback, explicit host date and bounded content check. Earlier failures
are retained privately and are not counted as successful qualifications. In the
final run the first reviewer still put an affirmative consistency statement in
the issue array; the second pass correctly removed it and preserved both
judgments. This demonstrates why model findings remain reviewable.

Final ordinary workspace checks passed 1,203 tests, with 578 opt-in cases ignored,
across 43 reported targets including empty documentation targets. All 166
JavaScript test scripts passed. The subsequent UI-only change replaces machine
verdict names with readable labels and is covered by that script suite; the
native model receipt binds the assets before this wording adjustment. Dependency
versions did not change. Workspace Clippy, formatting and the managed Windows
build also passed; their separate logs are not part of the native journey
denominator.

## Reproduction and remaining gates

```powershell
py -3.13 -X utf8 -B tools/qualify_document_journey.py --control <test-owned-runtime-folder-under-target> --browser <absolute-Chromium-executable>
```

The development command requires Node, the selected installed local model and
an explicit private qualification folder containing `tool-office.json`,
`pdf-runtime.json` and `fictional.docx`. Those are test-owned runtime controls,
not paths shipped in product configuration. It uses a disposable application
data directory, literal-loopback client requests, a hidden browser and an API
allowlist that refuses form actions and email preparation/sending. The original
template and selected executable bytes are fingerprinted. This invocation reused
private LibreOffice 26.8.2.2 and PDF tools; it does not prove first-use setup or
clean Windows installation. See [automatic setup and repair](RUNTIME_SETUP.md)
for the distinct runtime mechanisms and their qualification limits.

The exporter still creates a basic layout using template text as style guidance.
It does not directly preserve arbitrary original PDF/DOCX layout. Microsoft
documents that Word paragraph styles include paragraph/run properties and are
referenced by style IDs; retaining text alone does not retain those structures.
See the [Open XML paragraph-style documentation](https://learn.microsoft.com/en-us/office/open-xml/word/how-to-create-and-add-a-paragraph-style-to-a-word-processing-document).
LibreOffice exposes tagged-PDF and PDF/UA export parameters, but those options
alone do not establish recruiter-system compatibility. See its
[PDF export parameters](https://help.libreoffice.org/latest/en-US/text/shared/guide/pdf_params.html).
Both primary pages were read with native retained Crawl4AI results on this date;
no upstream code was copied and no tagged-PDF change is claimed here.

Complex/multipage documents, scanned templates, multilingual output, original
layout fidelity, signature handling, comprehensive source correction propagation,
held-out factual/visual accuracy and actual ATS behavior remain open. The local
readability endpoint uses one extractor and literal terms; it is not an employer
ATS result. Clean-host installation, diverse real documents and remaining
employer-form isolation/coverage gates also remain open. These two fictional
documents cannot establish majority success on unseen applications.
