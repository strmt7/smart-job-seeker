# Local 12B form evaluation — 2026-11-01

This is a small fictional-fixture check, not a model qualification or an
estimate of performance on employer sites. No applications were submitted.

The installed Ollama model was `gemma4:12b-it-qat`, digest
`38044be4f923e5a55264ed7df4eaac2676651a905f735197c504045140c02bd3`
(11.9B parameters, Q4_0, vision capable). The app's Windows startup gate
reported the model fully GPU-resident with a 4,096-token context. Before the
first startup, approximately 28.15 GiB of system memory was available and
dedicated GPU usage was approximately 4.72 GiB. These are snapshots, not peak
measurements or reservations. The model was unloaded after each test group.

Results from one run per fixture:

- `live_model_distinguishes_applicant_from_referee_contacts`: passed. On a
  local browser page the mapping included the applicant's email and excluded
  the reference and manager fields.
- `live_vision_does_not_transcribe_placeholder_as_a_value`: **failed**. The
  vision model transcribed the placeholder email as if it were entered.
- `live_visual_review_does_not_verify_placeholder_as_entered_value`: passed.
  The separate live DOM-value check prevented that placeholder from remaining a
  confirmed visual match on this fixture. This does not prove correctness on
  other sites or prevent every model error.
- `live_visual_distinguishes_identically_labelled_sections`: **failed**. The
  full-page observation identified both fictional email values with their
  section labels, but the focused crop returned only the short label. The
  focused comparison therefore remained `unobserved` rather than verified.

All tests used the app's existing resource and residency checks, a 4,096-token
context and one local browser at a time. Results can vary between runs. Keep
these cases for future model and prompt evaluation; neither a successful DOM
check nor exact transcription establishes overall application correctness.

## Focused-crop refinement

On the same fictional grouped-contact page, the original 70-pixel margin
above the field led the model to return the short label `Email` twice. With
160 and 240 pixels of context above the field, it returned the full
section-qualified label and expected fictional value in both trials for each
crop. These were local browser captures with the same model and prompt; they
are not a representative benchmark.

The driver was then changed to allow up to 160 pixels above a focused field,
while retaining the 400-pixel capture-height limit and the previous 70-pixel
margin for taller controls. One subsequent run of
`live_visual_distinguishes_identically_labelled_sections` passed with the
actual driver and model. The viewport-boundary browser fixture also passed.
Nearby fields may still confuse the model, and headings outside the viewport
remain unavailable; a missing or ambiguous label remains unverified.

## Placeholder prompt experiment

Four additional fictional pages displayed the same email as either an entered
value or a placeholder, using black or gray text. Chromium screenshots of the
black entered value and the black placeholder were byte-identical (SHA-256
`48fc9dbb084d479c5eb2ee375e3cba0f4ab3822a26216248694d67160b8ed330`).
The images were created locally and are not included in the repository.

With one response per image, a shortened version of the current transcription
prompt reported the email in all four cases, including both placeholders. A
second, deliberately cautious prompt omitted all observations in all four
cases, including genuinely entered values. These results do not establish
general prompt performance, but the identical-image pair demonstrates that
pixels alone cannot distinguish those two page states. The cautious prompt was
not adopted; the live-value check remains necessary, and an unavailable or
unconfirmed check must remain a manual-review case.
