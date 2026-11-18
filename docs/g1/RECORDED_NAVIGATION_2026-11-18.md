# Navigation in isolated application rehearsals

The saved-copy workflow gives the local model the current control inventory and
asks it to distinguish a new editable section, a read-only preview and actions
that need manual review. The inventory exposes short control IDs, visible labels
and native eligibility. The model receives no field values for this decision.
The application retains the proposed intent and the native completion metadata
in the attempt record.

The browser admits explicit button inputs with a unique visible name. It checks
the live button's behavior and attributes before planning and again before the
click. It requires the same document, native control identity, reviewed fields
and replay state. Native submit defaults, declared form overrides, commands and
popover targets require manual handling. These checks follow the activation
behavior and form semantics of HTML buttons. [HTML Standard, button element](https://html.spec.whatwg.org/multipage/form-elements.html#the-button-element)

A preview proposal stops after the transition. New editable questions at that
destination require review. A proposed advance needs a new field inventory in
the same inspected document. A read-only destination can be assessed as final
review only through the independent native and visual summary checks. The
typed decision runs inside the existing offline response-replay boundary.

## Native Windows qualification

The fictional browser cases used a private pipe and an AppContainer with zero
capabilities. The prior wording filter stopped at **Inspect your responses**.
The typed path reached its read-only summary. A forced advisory selection could
not activate a submit-default button, a command button, an explicit send label
or either of two duplicate labels. A replacement during pointer entry retained
the same label and was refused before its click handler. A preview intent that
reached another editable screen stopped for review. One scripted POST attempt
was blocked inside replay and stopped the workflow. All four sessions closed.
The native receipts are under `target/navigation-intent-c0bb6a36/first/`.

The HTML Standard button and activation pages returned HTTP 200 on 18 November
2026. Private captures retain the provider version, substantive source text and
digests `4319b93476e2…` and `53bc5acd938c…`. [HTML Standard, activation behavior](https://html.spec.whatwg.org/multipage/interaction.html#activation-behavior)

## Local-model runs

On the frozen employment-history case, Qwen selected the four checked values
and verified three entries. The before-entry reads for the last month differed
about the adjacent year's control type. The workflow stopped before navigation.
The initial image, detail image, independent reads and stop are retained in
`target/private-rehearsal-qualification-5lhx2kd8/`.

The first Spanish case verified its initial name field and stopped at an enabled
required field hidden in an inactive panel. It generated zero navigation
proposals. The hidden-field stop remains in the original result under
`target/private-rehearsal-qualification-xuhf2zqw/`. A separately frozen variant
mounts only the active step and keeps the submit control present throughout the
editable stages. Its inputs are under `target/navigation-intent-c0bb6a36/`.

Qwen 3.8 27B completed that mounted-step variant. It verified both fields,
classified **Ir a los datos de contacto** as an advance and **Previsualizar mis
respuestas** as a preview. **Enviar solicitud** was ineligible at each step.
The final summary matched **2/2** answers and the browser revalidated its native
rows. The replay's delivery result was `not_performed_by_replay`.

Both navigation calls recorded `xhigh` in the actual request, nonempty native
thinking, 16,384 context tokens and a natural completion. The run took 251.888
seconds including setup and cleanup. Its peak job commit was 13.42 GiB and its
minimum sampled host availability was 11,490,328,576 bytes. The model session
closed. The retained result is under
`target/private-rehearsal-qualification-ms6tq2hh/`.

Coverage remains scoped to authored replay fixtures and explicit native button
controls. The employment fixture needs target-focused detail capture to address
the neighboring control disagreement without weakening the conflict check.
Representative navigation across embedded, custom and server-managed workflows
remains open.

The Windows workspace suite passed 1,488 tests, with 624 opt-in checks ignored
by that command. Clippy passed with warnings denied. The first workspace run
exposed a parallel fixture race in which server teardown could drain another
test's search. The existing search fixture lock now covers that teardown in
test builds. The initial and passing logs remain in
`target/navigation-intent-c0bb6a36/`.
