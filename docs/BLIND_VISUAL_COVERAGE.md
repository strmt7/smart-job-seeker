# Blind visual coverage in saved-page rehearsals

A native control identity is not enough to establish that the local model can
read its image. Before a contained rehearsal enters a value, a blind visual
pass must identify the visible label and control type. The native label, type,
candidate answer and previous model observations are excluded from its prompt.
Only the controller compares the returned observations against native evidence.

The original image and first-pass prompt remain unchanged. A second pass is
allowed only when the target is absent from the first observations. It inspects
the image systematically, including empty and unfocused controls. It receives
no hint naming the missing control. Existing matches need no second inference.

| First observation | Behavior |
| --- | --- |
| Unique matching label/type | Continue through the existing native checks |
| Explicit difference | Stop for review; do not retry the difference away |
| Ambiguous repeated label | Stop; singleton comparisons distinguish ambiguity from absence |
| Observation limit reached | Stop with incomplete coverage visible |
| Invalid response, model failure or ownership change | Stop; do not continue with invented observations |
| Missing target below the limit | At most one blind coverage recheck |

For main-page text and select controls, the driver also captures a tighter
native-bound detail view. It uses the actual control rectangle with up to
48 CSS pixels above and 12 below, clipped to the visible page, within the
existing 720×400 CSS-pixel budget. It changes neither focus nor form values.
Embedded controls, radio groups, uploads and section buttons retain their
existing images. A distant label may be absent from a detail view; that is not
permission to infer it.

The coverage recheck uses the detail view when available, otherwise the original
image. Both views belong to the same native target witness. The driver rechecks
the document, schema, earlier entered fields and request barriers before acting.
The additional view cannot override these checks or enable live-site writes.
Failure to create detail context is recorded; failure to save available evidence
prevents entry. Original image paths/digests remain separate from detail images.

The record retains the complete first review under `initial_review`, the actual
second observations, and `coverage_recheck` scope/status/conflict metadata. It
does not rewrite either model answer or combine them into a fabricated
transcription. A type disagreement for an overlapping normalized label vetoes
the result. Missing, duplicate, invalid, stopped or conflicting second
observations remain review outcomes. There is no third pass or majority vote.
The record interface provides the saved detail image and distinguishes a second
check from verified entry.

This is a bounded development mechanism, not a general visual-accuracy claim.
Its first prompt-only experiment still omitted the French email control; that
failure remains retained. Corpus partitions, unknown answers and independent
field/workflow gates continue to apply. Successful development cases must not
be relabelled held-out or promoted into company-specific rules.

## Research basis and limits

[MVP](https://arxiv.org/html/2512.08529v1) reports sensitivity of GUI grounding to
small view changes and studies multiple-view aggregation. This implementation
does not reproduce its attention proposals, coordinate clustering or benchmark
results: it tests a native-bound region as a supplementary blind observation.

[OmniParser](https://github.com/microsoft/OmniParser) separates screen parsing
from planning. Here the existing native DOM identity and local visual
transcription remain separate checks. No upstream code or weights were copied,
and no detector, paid service or additional runtime dependency was introduced.
These sources motivate the experiment; they do not establish this application's
accuracy. Native Crawl4AI extracts and their digests are retained privately under
`target/visual-coverage-2x385_b7/`.
