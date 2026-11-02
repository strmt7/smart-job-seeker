# Custom radio visual qualification — 2026-11-02

A resource-gated test opened the fictional local `form_custom_radio_action.html` fixture and captured the visible question label and the unselected “Remote” option. The installed `qwen3.5:9b` model returned the exact expected text for both crops. Ollama reported the model as 100% GPU resident with a 4,096-token context during the run. The test did not activate a field or submit an application, and the model was unloaded afterward.

The first attempt requested `gemma4:12b-it-qat`; the startup gate refused it because its conservative estimate required 10,232 MiB of free GPU memory while the observed snapshot was 10,095 MiB. No model was loaded in that attempt. Available system memory was approximately 21 GiB before testing and 15.7 GiB in one sample during the 9B run; these snapshots are not peak measurements or reservations.

This is one fixture and two crops, not an accuracy benchmark for real application sites or other languages. The result does not verify radio selection or prove that activating a control cannot submit an application. The app keeps radio selection unexposed while that risk remains unresolved.

Reproduce on a suitably provisioned Windows host with no other resident model:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File apps/web_app/scripts/build-windows.ps1 -Test -Ignored -NoCapture -TestFilter resource_gated_radio_question_and_option_vision
```

## Varied labels and a transcription limit — 2026-11-02

A second fictional fixture added three questions and one option per question (six crops): two English questions and one French question. In three runs, `qwen3.5:9b` matched five of six crops exactly each time. On the French question it transcribed the initial `Ê` as `É`; the comparison returned `not_observed`, so the control requires manual review. A stronger prompt asking for exact accents was tried in the second run and produced the same mismatch; it was not retained. The first two runs failed an initial all-six accuracy assertion; the final run reports the measured count and checks that unmatched transcriptions are classified for review. A separate deterministic test verifies that these two accented strings do not compare equal.

Each run also matched both original English fixture crops. Ollama reported the 9B model as fully GPU resident in a sampled run, and no model was resident after the last run. This small, repeated fixture set is not a representative multilingual accuracy estimate; all examples are fictional and no application was sent.

## Radio answer advice — 2026-11-02

The review panel can suggest an available radio option from a freshly entered answer. Exact, unambiguous option labels are matched deterministically without loading a model. Other answers use local inference; the suggestion must quote supporting text from the supplied answer, and a changed page invalidates the result. Suggestions are advisory and never activate the radio control.

In one fictional 9B local-model run, the exact answers “Yes” and “No” matched without inference. For two longer explicit authorization answers the model selected the expected option but returned the option label instead of a quote from the answer; both suggestions were refused. The model abstained for an uncertain answer. A follow-up prompt emphasizing verbatim quotation produced the same two refusals and was not retained. These examples are not a reliability estimate for other wording or models. The model was unloaded after testing; no application was sent.

## Indexed answer evidence and a consistency pass — 2026-11-02

A follow-up test changed local advice to select the full, unmodified answer by index instead of asking the model to reproduce a quote. The model then selected the expected options for the two previously refused explicit answers. With multiple selectable excerpts, however, it chose a supportive sentence from a contradictory answer and suggested “Yes”; presenting the full answer alone still produced that wrong suggestion. A separate local consistency pass was added, and the excerpt is restricted to the full answer.

In two subsequent fictional runs the consistency pass detected that contradiction and the advice abstained. The same runs mapped the two explicit answers as expected and abstained on an uncertain answer and an answer whose country was unclear. The model had also matched exact “Yes” and “No” deterministically. These are a few examples, not proof that the same model can reliably detect every contradiction or reason correctly; suggestions remain advisory and require review. No application was submitted, and the test unloaded the model.
