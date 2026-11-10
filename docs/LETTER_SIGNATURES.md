# Profile-sourced cover-letter signatures

Checkpoint: 2026-11-10. Generated cover letters now end with the candidate's
saved application name. `full_name` takes precedence; otherwise the supplied
first/last names are joined without inventing missing parts. Mononyms and UTF-8
spelling are preserved. The feature does not infer a name from email, preferred
name, templates or model output. No new setting or dependency is needed.

Missing names remain missing. Malformed, multiline/control-containing or
over-200-byte names receive an explicit correction notice rather than being
truncated or replaced by another field. The editable review remains available.
The signature records its profile field names and source hash as candidate-entered
data, not independent identity verification. It is added after the unchanged
generation/natural-language revision passes; the model does not invent or approve
the signature. CVs do not receive a cover-letter sign-off.

One Rust composition produces the canonical text used by the editor, saved-letter
reuse and export provenance. The closing and name are separated by one line.
Later user edits remain explicitly edited material. A mismatched cached canonical
text is refused. Old unsigned cover drafts become historical through the new
letter-structure context version; legacy text is still readable. Profile changes
invalidate the current context without rewriting previously exported PDFs.

The simple DOCX-to-PDF exporter keeps a short final closing/signature block
together. It does not insert a handwritten image or cryptographic signature,
preserve arbitrary template formatting or approve an application.

## Verification and an actual defect found

The native qualification uses fixed fictional body text, the real profile/name
composer, production PDF export, private Office/PDF tools and durable attempt
records. It loads no model and does not test new model behavior. Cases cover a
Latin apostrophe name, a CJK name/body and a two-page layout stress letter with a
mononym. All four pages were captured and directly inspected. Each PDF contained
its name once, with closing/name on the same page; text comparison matched and
provenance was saved. Correcting the profile left earlier PDF bytes unchanged.

The initial run stopped when the dense page's geometric inspection was unavailable.
Its existing X-axis sweep exhausted 100,000 comparisons on normal pages with
4,772/4,573 characters. A bounded two-dimensional cell lookup now finds relevant
glyph pairs with 22,144/21,555 comparisons. The 5,000-character and 100,000-pair
limits remain intact. Three portable checks compare results with an independent
pairwise oracle, verify dense normal pages and keep invalid/pathological work
explicitly unavailable. Geometry is advisory, not visual or semantic approval.

The accepted source/runtime-bound native receipt is
`target/document-journey-run-6xgovm3z/result.private.json`, native result
`target/letter-signature-native-oSdiRm/result.private.json`, SHA-256
`15c7a467dbefa952371994ec3507308bcbbca1cee5d79a69d1655d60edc57009`.
One opt-in case executed with zero ignored tests in 48.721 seconds. Ninety-seven
whole-host samples recorded at least 18,535,862,272 bytes (17.263 GiB) available
RAM. This is not a reservation, application peak or long-running leak claim.
The initial failure remains in `target/document-journey-run-r9wnxeke/`.

Reproduce without loading a model or opening a browser:

```powershell
py -3.13 -X utf8 -B tools/qualify_document_journey.py --control <absolute-test-owned-Office-and-PDF-runtime-directory-in-target> --flow signature
```

The ordinary workspace passed 1,224 tests with 580 opt-in cases ignored across
43 reported targets. All 167 JavaScript checks passed, including editor and
saved-letter reuse. The contained layout matrix passed 302 observations,
including a long unbroken name, missing/invalid notices and 320 px/400% zoom.
Its desktop/mobile screenshots were inspected after explicitly opening Workspace
and bringing the signature into view. The first matrix invocation captured the
Jobs area on mobile and is not accepted as visual signature coverage. Final receipt:
`target/layout-check-run-6jkbngeg/result.private.json`; native result SHA-256
`2f2806f2f955cc4c6f32f60214e082bacbf8947558186f98122a897d05e787ee`.
Workspace Clippy, formatting, managed build and the PDF page-boundary check
passed. CI includes the new geometry tests.

This is narrow signature/pagination evidence on the selected private runtimes.
Diverse real-document fidelity, clean-host fonts, language quality, automated
visual judgment, ATS performance and remaining application-isolation gates still
need qualification. No real application form, email or submission was involved.
