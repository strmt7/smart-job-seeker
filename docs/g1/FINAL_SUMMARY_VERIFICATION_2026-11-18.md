# Final application summary verification · 18 November 2026

The saved-page rehearsal reads a summary independently after guarded
navigation reaches a view without editable fields. It compares the prepared
answers with the current browser's structured label and value pairs and with
the local model's transcription of the archived image. The model receives
the image and transcription instructions. Ordered section headings keep
repeated labels distinct. Values are compared exactly.

The browser reads single-pair definition entries and single-answer rows
with a row header, including their semantic role equivalents. It records the
document and root identities, heading and answer text, viewport and native
text geometry before and after the capture. After the visual read it checks
the same document, form, native summary and offline replay accounting again.
The archive retains the scoped prepared and observed answers, image digest,
native snapshot digest, raw model result and the comparisons. The review view
shows the number of verified answers and the specific fields needing attention.

## Headless qualification

Six hash-bound fictional applications each used two sections with the same
field caption under different headings. The guarded navigation and native
entry used the same packaged headless Chromium as saved-page rehearsals.
The positive case used a definition list and a table on one summary screen.
Actual Qwen 3.8 27B inference through private Ollama 0.40.16 read both answers
correctly at 16,384 context tokens using the literal review request. Both
section identities and exact values matched, and the browser revalidated the
native summary after inference. The case took 52.705 seconds including
contained navigation, model startup and shutdown. Minimum sampled available
host RAM was 12.888 GiB.

| Case | Result |
| --- | --- |
| Complete summary with repeated field captions | 2 of 2 answers verified |
| Changed answer | Native value mismatch |
| Missing answer | Missing from summary |
| Repeated label within one section | Ambiguous native label |
| Answer outside the image | Incomplete visual coverage |
| Replacement document with the same displayed summary | Native revalidation refused |

The five adverse cases received favorable synthetic visual observations to
isolate the independent browser gates. All five remained pending manual
review. The first fixture attempt exposed the inspector's section prefix
in flattened labels. The corrected fixtures bind the native full labels.
The product derives the leaf from the recorded section path and preserves
punctuation in the field label.

Every browser closed. All six cases kept the local submit sentinel intact,
used zero cached write responses and performed zero external deliveries.
The private model process exited and its port had no listener owners.
The live corpus and results are under `target/final-summary-2d099113/scoped/`.

The final Windows build and warnings-denied Clippy passed. The workspace
passed 1,474 tests with 621 ignored, and all 172 UI scripts passed. The
summary panel rendered from the native mismatch record at 430 and 1,024
pixels and with 200% CSS zoom. The viewport and comparison rows fit
horizontally in all three renders. The narrow and enlarged images were
inspected after the status and review notices were consolidated.

## Scope

These checks cover structured summaries within the archived viewport. They
establish shown-answer agreement for a saved rehearsal. An attachment filename
cannot verify current attachment bytes. Off-screen or unsupported rows,
missing or conflicting model reads, and evidence invalidated by a document
change retain a review item. Segmented coverage, current attachment bytes and
representative held-out workflows remain open.
