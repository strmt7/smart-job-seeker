# Local answer repair check — 2026-11-01

On the development Windows host, Waypoint's resource-gated startup loaded the
installed `qwen3.5:9b` model. Ollama reported manifest digest
`c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`,
4,096 context tokens, and 5,506,859,006 bytes each for resident model size and
GPU memory. These are runtime-reported values, not independent measurements.

The ignored `live_answer_drafting_checks_unsupported_fictional_experience`
test passed: in its fictional example the model abstained from inventing team
management years, and drafted a supported Python answer without adopting SQL
or years from the job listing.

The ignored `live_model_repairs_a_fictional_invalid_first_draft` test also
passed. Its wrapper injected an invalid first citation; the resident model
generated the repaired answer and subsequent style and evidence passes. The
test checked the citation and the same fictional experience boundaries. The
initial invalid answer was injected by the test, not produced by the model.

Each test exercised one fictional scenario on one model and host. These results
do not establish broad drafting accuracy, resource safety on other machines,
or reliable behavior on real application forms.

Later on the same date, a separate attempt to start `ministral-3:14b` was
refused by the resource gate. One snapshot reported 10,141 MiB free GPU memory
against the gate's 12,443 MiB estimate. The model was not loaded, so this is
not a capability or performance measurement and does not establish whether it
would fit under different system load.
