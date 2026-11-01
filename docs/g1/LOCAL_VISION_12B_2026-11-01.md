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
