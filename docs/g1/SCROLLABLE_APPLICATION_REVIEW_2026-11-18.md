# Scrollable application review · 18 November 2026

Two fictional saved applications were frozen before implementation. The
development case puts a résumé review row below the fold of an independently
scrolling panel. The second case combines repeated contact fields and two
distinct PDFs inside an embedded application. The frozen fixtures, hashes
and answer oracles remain under `target/nested-summary-196c5303/`.

## Capturing the current review

The browser still uses a direct document clip where native geometry supports
one. For a nested viewport, it identifies the heading and answer nodes from
the same summary census and records the positions of their scroll ancestors.
It uses element scroll offsets to reveal each part within the current document.
The element scroll operation is described in the [CSSOM View specification](https://www.w3.org/TR/cssom-view-1/#dom-element-scrollto).

The browser verifies visibility, node continuity and the complete set of answers
before and after capturing the revealed region. Embedded captures additionally
check the iframe owner's position and hit targets in its parent. The original
native snapshot and replay counters are checked again after restoring scroll
positions. The visual reader receives the resulting image only after these
checks finish. Each archived detail records its capture mode and viewport.

## Measured results

The `196c5303` baseline completed the three entries in the development fixture
and verified two at final review. The lower résumé row could not be captured.
The run took 316.182 seconds on private Qwen 3.8 27B. The original native result,
visual reads and manual-review outcome are preserved.

The paired run through the standard launcher verified all three entries and
all three review rows. It retained the original PDF bytes and left the
unsupported optional field untouched. The exact frozen application and model
were reused. The run took 214.714 seconds, peaked at 14,442,532,864 bytes of
aggregate job commit, and sampled at least 11,523,973,120 bytes available on
the host. The lower-row screenshot and its independent reads were retained
under `target/private-rehearsal-qualification-fdgtagz5/`. The owned model and
browser shutdown checks passed.

Two headless browser checks isolated the capture behavior using controlled
visual responses. The positive case verified all three answers and the retained
PDF. The panel started and finished at 24 CSS pixels and recorded two scroll
events. In the adverse case, a page handler changed a visible answer while the
panel was scrolled and restored it afterward. The verifier withheld the final
pass even with a favorable visual oracle. The terminal delivery control stayed
untouched and both cases had zero cached write responses. Captures and results
are saved under `target/nested-summary-196c5303/native-v4/`.

## Embedded application observations

The first run on the frozen same-origin iframe stopped before entering the
first field. Its overview showed the field below the document title, and the
model included that title in its proposed label. The independent native-label
detail showed the header instead of the label and input. The field detail was
using child coordinates for a parent-page image. The original stop and images
are preserved under `target/private-rehearsal-qualification-u_vgtb9g/`.

Field detail geometry now translates through verified same-origin frame owners
before taking the parent-page screenshot. The comparison still uses the exact
visible label and type. Transformed, obscured and unstable frame geometry can
still require review.

The second run reused the exact same-origin fixture and verified both fields
in its first section. It stopped with `no_forward_control` because the current
navigation census did not include the same-origin child document. That run
took 133.657 seconds. The corrected detail image, native field receipts and
remaining navigation gap are preserved under
`target/private-rehearsal-qualification-ol6_vs_s/`.

The native browser fixture with a separately managed iframe verified all
three review rows, retained the original PDF, and restored the panel from its
temporary position to 24 CSS pixels. It saved separate heading and answer crops
with `embedded_viewport` geometry. A direct-document regression also verified
both retained PDFs and recorded zero scroll events. These two browser checks
used controlled visual replies. Their evidence is under
`target/nested-summary-196c5303/captured-frame/` and
`target/nested-summary-196c5303/direct-document/`.

The first local-model run in the separately managed frame stopped before
entry. Its field crop showed the input but omitted the enclosing section
heading. Both blind reads reported the unscoped field label. The retained
stop is under `target/private-rehearsal-qualification-qr2qpy3v/`. Native
associated-label details now use the same verified frame projection as the
summary, with frame geometry rechecked after capture.

The paired model run on that unchanged frame verified all three entries and
all three terminal review rows, including the original retained PDF. The
heading and answer images both used `embedded_viewport` capture. The
verification still required the exact native summary after restoration.
The run took 249.465 seconds at 16,384 context tokens. The job peaked at
14,497,722,368 bytes and sampled at least 12,265,189,376 bytes available on the
host. The private runtime exited and replay recorded no blocked requests,
cached writes or external delivery. Evidence is under
`target/private-rehearsal-qualification-4oknngwh/`.

The Windows workspace passed 1,477 tests with 621 ignored. The final Windows
build, warnings-denied Clippy and the private launcher test passed. The live
qualification used saved response replay and fictional documents. Same-origin
navigation and review within that child scope still require implementation
and an unchanged-fixture rerun.
