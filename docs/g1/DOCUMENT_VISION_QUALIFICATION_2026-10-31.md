# Document vision qualification, 2026-10-31

A fictional two-page PDF fixture has clean text on page one and deliberately overprinted text on page two. The packaged PDF worker rendered both pages in a local test. The ignored `live_model_detects_overlapping_document_text` test uses the production page renderer, prompt and vision adapter, and requires an eligible model already resident in the local runtime. It checks that page two receives a layout or legibility finding describing the defect. Set `WAYPOINT_PDF_PYTHON` to the installed PDF worker and `WAYPOINT_VISION_BENCHMARK_MODEL` to the resident model before running the test.

The live vision test was not run: the resource-gated startup of `gemma4:12b-it-qat` refused to load it because observed available GPU memory was below the conservative estimate. At that time the runtime reported no resident models, system free memory was approximately 26.1 GiB, and total dedicated GPU usage was approximately 5.8 GiB. These are point-in-time observations, not peak measurements. The fixture and rendering check do not establish that the model can detect document defects or that visually inspected documents are correct.

## Selectable text geometry check — 2026-10-31

Document visual review also checks selectable character boxes on each rendered page for substantial overlap. A separate warning appears when pairs overlap; this can flag accents, font effects or duplicate text and does not prove a defect. Zero warnings do not prove the page is legible. Scanned pages report no selectable text, and the check reports unavailable if its 5,000-character or 100,000-comparison limits are exceeded or its geometry cannot be read. It is separate from the local model's findings and does not inspect images or guarantee correctness.

The installed PDF worker detected overprinted characters in a fictional two-page fixture while reporting none on its clean page; separate blank and 6,000-character pages exercised the unavailable states. The focused document tests, UI warning test and web Clippy passed. The local model was not run in this check, and the earlier GPU startup refusal remains unresolved.
