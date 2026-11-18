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

## Field location in detail images

The browser now compares both the crop and the target's position before and
after capturing a native-associated detail image. The model receives the
target's fractional location within the image. It reads one label and control
type from the original pixels. The first visual observation and the detail
response are both retained. The reviewer blocks ambiguous observations and
checks for conflicting readings when the two passes describe the same control.

On the same frozen employment-history input, Qwen 3.8 27B visually verified
**4/4** native entries and selected **Review application**. The final review
matched all four expected answers, with zero extra rows and a fresh native
verification. The replay recorded `not_performed_by_replay` for delivery. The
early stop in the previous 3/4 run remains in the comparison evidence. Both
runs used source-fixture SHA-256 `70bc4c6311082063ea5f38e0e1c3005d3381098d231ed32da85ab691950a92c9`.

The four focused responses each contained one observation and ended naturally.
The native Ollama 0.40.16 requests used `xhigh`, 16,384 context tokens and an
8,192-token output budget. Their outputs used 139, 2,342, 170 and 1,643 tokens.
Both month controls were slower to reason about. The controller took 698.879
seconds including build, setup and cleanup. Peak job commit was 14,552,711,168
bytes and minimum sampled free physical memory was 13,285,388,288 bytes. The
private model session closed. Retained records and the reviewed summary image
are under `target/private-rehearsal-qualification-zqagvh4y/`.

An isolated native select fixture matched the recorded focus rectangle to the
browser's border quad. It withheld focus for hidden, distant and clipped labels.
The isolated file-label and dialog suites passed. The Windows workspace passed
1,489 tests with 624 opt-in tests ignored by that command. Clippy passed with
warnings denied. Native regression receipts are under
`target/focused-field-detail-7d921ac4/final/`.

This comparison covers one authored history workflow. The next qualification
needs unfamiliar repeated and embedded layouts, including close neighboring
controls, and should measure whether focused vision stays accurate on them.
Server-managed workflows also remain open.

## Workspace validation for the original navigation change

The Windows workspace suite passed 1,488 tests, with 624 opt-in checks ignored
by that command. Clippy passed with warnings denied. The first workspace run
exposed a parallel fixture race in which server teardown could drain another
test's search. The existing search fixture lock now covers that teardown in
test builds. The initial and passing logs remain in
`target/navigation-intent-c0bb6a36/`.
