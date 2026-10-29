# Local model evaluation — 2026-10-29

This is a small, reproducible capability probe, not a qualification of application automation. It uses fictional inputs and synthetic form screenshots. No employer websites, real candidate data, or applications were involved.

The Windows host ran its installed `qwen3.5:9b` model (Q4_K_M, digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`) through local Ollama. Eleven cases were run sequentially with a 4,096-token context, thinking disabled, temperature zero, and bounded output. The raw prompts, model responses, stop reasons, times, resident-model information and point-in-time resource readings are in [the receipt](local-model-evaluation-2026-10-29.jsonl). Fixtures and deterministic grading criteria are in `apps/web_app/tests` and `apps/web_app/scripts`.

The grader reports 10 passes, one partial, zero failures and zero missing cases. The partial case checks answer shape and citation only; it does not establish factual support. Duplicate-case explanations are not graded. The passing visual cases cover one four-field and one 16-field synthetic screenshot, not arbitrary real forms. A separate negative-fixture test verifies that the grader rejects several incorrect or truncated outputs and reports missing cases.

Across post-request samples, minimum available system memory was 23.11 GiB and maximum dedicated GPU usage was 11.01 GiB. Ollama reported the model fully GPU-resident throughout the cases. These are sampled readings, not continuous peak measurements or guarantees for other machines. The model had expired from residency when checked afterward.

To repeat, start the selected local model with a 4,096-token context and full GPU residency, then run `apps/web_app/scripts/run-local-model-evaluation.ps1` and pass its receipt path to `apps/web_app/scripts/grade-local-evaluation.ps1 -Receipt <path>`. The runner checks free memory and GPU usage before each request, requires exactly one fully GPU-resident local model, performs no downloads, and stops on an unmet guard. It cannot prevent other processes from changing host resource use between checks.

The probe calls Ollama directly with simplified evaluation prompts. The production adapter currently does not explicitly set the same context size; this result therefore does not verify production inference, full browser interaction, document editing, adversarial sites, multistep workflows, or real-world success rates. Broader independent cases and bounded end-to-end production-path measurements remain necessary.
