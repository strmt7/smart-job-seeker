# Production answer abstention check — 2026-10-29

The ignored test `live_answer_drafting_checks_unsupported_fictional_experience` exercises the application-answer generator, its resident-model guard, and the Ollama adapter using fictional data. The supplied fact was research-scientist experience; the question asked for years managing a team. The model should not infer management experience or a duration from that fact.

Local `qwen3.5:9b` Q4_K_M (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`) returned empty text and an empty citation list in both runs. The production generator rejected the empty answer before any style revision. The initial error was vague; it now reports that the model returned no answer and the question needs manual review. The [repeat output](production-answer-unsupported-2026-10-29.txt) records the request and response. The focused answer-task tests passed.

The repeat preflight measured 25.17 GiB free system memory and 10.97 GiB dedicated GPU use; after the run the samples were 25.30 GiB and 10.97 GiB. Ollama reported the one selected model fully GPU-resident with a 4,096-token context. These are snapshots, not peak measurements. The ignored test requires external resource preflight, explicitly uses that context, and runs with no default features; it does not validate SQLCipher. No real candidate information, employer site, or submission was used.

This single abstention does not establish factual support for nonempty answers: citation indices are not evidence that all generated claims follow from the facts, and the style revision remains untested here.
