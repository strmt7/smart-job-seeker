---
name: local-model-qualification
description: Qualify changes to local-model reasoning, vision, tool use, prompts or learned browser/document procedures using actual runtime configuration and source-bound task outcomes.
license: MIT
---

# Local model qualification

Use this when changing the local actor or comparing models, prompts, inference
profiles or reusable lessons. Follow repository `AGENTS.md`; this skill grants
no new browser, network, submission, process or delegation authority.

## Check the effective model, not its label

Bind the weight/projector digests, imported model manifest, runtime version,
renderer/parser, context, sampling and actual generation settings. Imported
weights can advertise vision/tools/thinking while lacking the native parser
needed to separate reasoning, final answers and tool calls. Check the selected
runtime's current model metadata and a real response for each capability used.

Choose the highest appropriate **advertised** reasoning level for deliberative
work; model-defined `xhigh`, `high` and boolean `true` are not interchangeable.
Boolean thinking can select a runtime's medium default. Do not invent an effort
setting or silently fall back after a refused one. A 200 response or correct
answer alone does not establish that thinking controls took effect. Preserve
native completion evidence and observable thinking presence without printing
private reasoning or candidate information to public logs.

Keep reasoning and final structured extraction distinguishable. Give deliberate
reasoning enough admitted context/output budget; retain length stops, missing
final answers and malformed output. Never convert an interrupted response into a
complete answer, strip unexplained prose to manufacture a pass, or infer that
every typed primitive benefits from another reasoning call. Use the current
native output contract and qualify the actual production path.

## Compare complete behavior

Freeze independent task truth, source/input/image hashes and grading contracts
before tuning. Keep answer oracles outside the model-visible task. Verify that
the grader accepts valid outcomes and rejects plausible wrong, incomplete and
unsupported outcomes. Label an ambiguous prompt/oracle contract honestly.

Use the same task inputs, permissions and declared budgets across paired
conditions. Record intentional configuration differences, cold/cache state and
all failed attempts. A response reused from another model cannot measure the
new model. Changing renderer, effort, context and model together tests a complete
configuration; it does not isolate any single cause.

Prioritize complete source-to-final-review journeys, correct facts/documents and
appropriate abstentions. Separate field matches, complete-workflow outcomes,
manual walls and harness/resource failures. Preserve planned/attempted/scored
denominators and paired regressions; a selected retry is not pass@1. Promote a
model or learned correction only after repeated representative and untouched
held-out qualification, not a vendor score or a few development probes.

Use `tools/form_corpus.py` for existing independent form truth. A native-bound
detail capture may clarify a clipped label; preserve the original failure and
never provide the expected label/value as a visual hint. Scope learned procedures
to their source, dependencies and expiry; they cannot replace current perception
or the authorization checks defined in `AGENTS.md`.

## Keep continuation reliable

At a substantial checkpoint retain the source/configuration identity, frozen
inputs, measured outcomes, unresolved failures and next executable step. Record
owned PID/creation/job/profile/port identities, model pre-state, actual teardown
and available RAM. Recheck volatile ownership on resume; do not rerun unchanged
qualified work simply because context changed. Do not install global hooks or
write personal memory as part of this workflow.

Adapted from the experimental MIT `eval-harness` and `strategic-compact` overlays
in the user's AI Skills Collection Benchmarked. Their upstream source is
`affaan-m/everything-claude-code` at
`846ffb75da9a5f4e677d927af1ad4a1951652267`; the reviewed overlay SHA-256 values
are `c598eed23babdb8fcc2eea2498c5841516bd641e7d0e373aeadb504201daf1d9`
and `3f53ae09458f05fd1938198c8266e578463eb68146ffeae8443f910b6ab1c875`.
Retain the accompanying MIT notice. The catalog's packaging/grader evidence
does not establish an isolated skill-performance gain. See the
[current research checkpoint](../../../docs/AGENT_RESEARCH_CHECKPOINT_2026-11-15.md)
for measured application scope and remaining work.
