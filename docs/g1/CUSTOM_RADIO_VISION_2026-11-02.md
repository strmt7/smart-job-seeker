# Custom radio visual qualification — 2026-11-02

A resource-gated test opened the fictional local `form_custom_radio_action.html` fixture and captured the visible question label and the unselected “Remote” option. The installed `qwen3.5:9b` model returned the exact expected text for both crops. Ollama reported the model as 100% GPU resident with a 4,096-token context during the run. The test did not activate a field or submit an application, and the model was unloaded afterward.

The first attempt requested `gemma4:12b-it-qat`; the startup gate refused it because its conservative estimate required 10,232 MiB of free GPU memory while the observed snapshot was 10,095 MiB. No model was loaded in that attempt. Available system memory was approximately 21 GiB before testing and 15.7 GiB in one sample during the 9B run; these snapshots are not peak measurements or reservations.

This is one fixture and two crops, not an accuracy benchmark for real application sites or other languages. The result does not verify radio selection or prove that activating a control cannot submit an application. The app keeps radio selection unexposed while that risk remains unresolved.

Reproduce on a suitably provisioned Windows host with no other resident model:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File apps/web_app/scripts/build-windows.ps1 -Test -Ignored -NoCapture -TestFilter resource_gated_radio_question_and_option_vision
```
