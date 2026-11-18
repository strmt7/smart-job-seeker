# Verification below the fold · 18 November 2026

Saved application summaries can contain answers beyond the overview image.
The rehearsal now identifies those rows in the native document. It captures
regions that include the field label and answer and, when needed, separate
regions for headings that are too far away to fit. The local model reads each
isolated heading and the answer separately. The browser supplies their
structural order. The model receives the pixels and transcription
instructions. The review panel links the overview and the retained detail
images.

Only a uniquely identified row with the expected native value can receive a
detail check. The detail must establish the same section path and label. An
ambiguous or conflicting observation stays pending review. The complete
native summary is checked around each capture and again after visual review.
An image records its page coordinates and digest. A conflicting overview
observation remains available in the attempt record.

## Stable capture geometry

The first contained run refused the crop after Chromium hid a vertical
scrollbar. The native layout shifted centered content by 7.5 CSS pixels even
though the window dimensions and scroll offsets stayed the same. The
isolated replay engine now starts with consistent scrollbar rendering so
the same geometry holds through the overview and lower-page captures.
The refusal and diagnostic runs are retained under
`target/summary-regions-93b32602/`.

## Qualification

The eight fictional replay variants pair prepared answers with definition
list and row-header table layouts. The tests inspect a scroll-event sentinel
and the delivery control after summary review.

| Case | Observed result |
| --- | --- |
| Nearby lower-page heading and answer | Verify using one combined region |
| Distant heading and answer | Verify using two ordered images |
| Row clipped inside a separate scroll container | Pending review |
| Conflicting overview observation | Pending review |
| Detail read names a different section | Pending review |
| Separate heading names a different section | Pending review |
| Repeated detail observation | Pending review |
| Replacement document with the same summary | Pending review |

The first visual request supplied both distant crops in one call. Qwen
copied the value and incorrectly used the section heading as the field
label. The current version read the byte-identical overview and both
byte-identical crops in three separate requests. All three completed, and
the result verified both prepared answers. The native model was the
private Qwen 3.8 27B Q3_K_XL through Ollama 0.40.16 with a 16,384-token
context. The contained run took 53.587 seconds and kept at least 12.731 GiB
available RAM in the host samples. The model exited and its port had no
listener owners. The run retained each native response, image and digest.

All eight cases passed their frozen checks on the same test executable.
Two verified both answers. The six adverse cases stayed pending review.
The combined-region and adverse cases used prescribed visual observations
to exercise the browser and comparison gates. Every browser closed after
recording zero scroll events during final review, zero cached write
responses and zero external deliveries. Final records are under
`target/summary-regions-93b32602/separate/`.

The workspace passed 1,475 tests with 621 ignored. Warnings-denied Clippy,
the Windows build, 15 worker tests and all 172 UI scripts passed. The panel
fit at 430 and 1,024 pixels and under 200% CSS zoom. The narrow and
enlarged renders were visually inspected after the overview and answer
image buttons received separate labels and consistent spacing.

Embedded summaries can use the overview evidence. Page-region capture
currently requires the main recorded document. Rows that cross incompatible
clipping boundaries remain review items. Summary filename agreement still
requires separate verification of current attachment bytes, and broader
layout and complete-workflow qualification remain open.
