# Telephone presentation and retained visual evidence

This checkpoint advances R04 without completing the application-completion or
release gates. Testing uses fictional candidate facts, contained loopback
fixtures and a previously collected public-page archive. No employer form is
acted on and no application or email is sent.

## Comparison contract

The browser may accept a changed telephone presentation only after observing a
native `INPUT` with explicit `type=tel`, acceptable current validity and the
existing visibility, editability, freshness and authorization checks. Both the
commanded value and the current value must have an explicit international plus,
the same ASCII digits and at most 15 digits. Only bounded ASCII display
separators are removable. Extensions, inferred country prefixes, national
numbers, different digits and unsupported Unicode symbols remain refusals.
Other text controls retain exact matching.

The record preserves the commanded value, actual native value and comparison
mode separately. The screenshot is bound to the actual value: a later change
invalidates that image even when the new presentation has equivalent digits.
Post-entry local-model transcriptions and comparison results are retained
alongside the image, including differences. Only a capture carrying the native
telephone-presentation proof admits the same separator comparison for its
independent visual transcription; the review explicitly records that mode and
preserves the original observation. Unique label/section and required-marker
checks remain shared with exact comparison. Native success alone does not make
an entry visually verified. Schema-2 expected answers must explicitly opt into
the telephone comparison; the independent evaluator still requires native,
visual, control and actual image evidence.

This is an identity comparison, not proof of number assignment or dialability.
The [HTML telephone-input specification](https://html.spec.whatwg.org/dev/input.html#telephone-state-(type=tel))
leaves telephone syntax to the site; `pattern` and custom validity therefore
remain enforced. [RFC 3966](https://www.rfc-editor.org/rfc/rfc3966.html) separates
global numbers from local context and extensions. Its URI grammar is not a
general printed-number grammar. The current
[ITU-T E.164 recommendation](https://www.itu.int/rec/T-REC-E.164/), February 2026,
retains a maximum of 15 digits for international geographic numbers. No country
or extension is inferred from adjacent fields.

## Native qualification

The contained fixture exercises actual input events and display formatting,
separate intended/observed values and invalidation of a previously captured
image after an equivalent reformat. Twelve negative cases cover wrong digits,
missing/different prefixes, extensions, nontelephone inputs, pattern/custom
validity, ARIA invalidity, disabled/readonly/hidden controls and unsupported
combobox/autocomplete semantics.

The corrected native qualification passed in
`target/capture-replay-qualification-jm9vpq2x/`. A preceding attempt failed during
initial `DOM.resolveNode`, before the telephone checks; that failure remains in
`target/capture-replay-qualification-677fas88/`. No limit was increased to turn
that failure into a pass. The old challenge/schema/document witness refusals
run before the added-control fixture, so a schema change cannot make those
checks pass vacuously.

Final-source requalification also passed in
`target/capture-replay-qualification-0vqfjuva/`, retaining the native values,
telephone comparison, twelve refusals and stale-image rejection. This includes
the final native attribute-iteration implementation.

## Evidence and remaining work

Private standards captures, hashes, source-bound searches and controller logs
are retained under `target/telephone-presentation/`. The ITU PDF was obtained
through its native transport after a robots check and extracted with the
already configured private PDF runtime. No new runtime dependency was added.

The first actual local-model development run accepted Phone natively but
reported a visual difference. Its independent result remained three verified
of six answerable initial controls, one unverified and two unreached, with
three unknown controls and no unexpected entries. The full workflow was not
qualified. This known layout is development data, not held-out evidence.

The follow-up run retained the exact local-model observation:
`+1202-555-0123`, compared with native `+1 202-555-0123`. The missing display space
was the measured difference; the digits and explicit prefix were unchanged.
That run still failed the exact visual gate and remains historical failure
evidence. Two new visual-comparison tests cover preservation of that observation,
strict defaults, missing prefixes, changed digits/countries, extensions,
unsupported Unicode and duplicate/incorrect section labels.

The final compiled-app run, `target/telephone-model-pi75ea9_/`, independently
verified **four of six** answerable initial controls: First Name, Last Name,
Email and Phone. Both PDF upload controls remained unreached because the
fictional rehearsal mapper does not recognize their section-qualified labels.
Three controls remained unknown; unknown entries, incorrect values, ambiguous
controls and unexpected entries were zero. The field gate remained **failed**
and whole-workflow completion **not qualified**. A page-review wall prevented
section advance. This is a measured improvement in one known development
family, not a general success rate or a held-out result.

The private runtime used `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`, context
4,096. The controller finished in 211.313 seconds, recorded its closed private
session and drained process tree, and verified shared Ollama stayed unchanged.
The 418 whole-host RAM samples retained at least **21,327,642,624 available
bytes**. This is not an application-exclusive peak or a long-running leak test.
The separate Windows GPU-counter availability test passed on the Radeon host;
it does not establish the model's physical peak allocation or complete GPU fit.

## Interface and final checks

The compiled record view passed at 1,360, 900, 480 and 320 px. The Phone
explanation remained unobscured in the actual viewport, its saved field image
opened, page errors were empty and horizontal document overflow was absent.
Desktop/mobile screenshots and the displayed Phone crop were inspected.
`target/telephone-ui-5z4we11m/` retains the final observations. Early private UI
controller attempts assumed nested details were open or selected the wrong
mobile pane; those failures remain retained and are not application passes.

The canonical quick layout run passed **22 cases**, including mouse-drag
resizing, settings/license states and actual browser zoom. Its broad viewport
and zoom matrix was deliberately not repeated for this small record-copy
change. Receipts: `target/layout-check-run-41s8fupz/` and
`target/layout-matrix-YVTTEy/`.

Final ordinary workspace validation passed **1,316 tests**, zero failed and
595 opt-in tests ignored. Warning-denied Clippy, canonical Windows build and
format checking passed. The development-tool suite passed 90 tests; it covers
the changed independent oracle, not local-model accuracy. No dependency graph
changed. Previously pushed baseline `f711750` has successful hosted CI, coverage
and CodeQL checks; those hosted results do not validate this new local batch.

The final host audit found zero matching development processes, zero resident
shared models and no matching startup/service/scheduled registrations, with
**30,546,604,032 available physical bytes**. Owned-tree teardown is measured;
sustained leak behavior remains unqualified. No visible windows, source-form
actions, application submissions or email deliveries were used.

Diverse held-out families, complete workflow
truth, generic candidate-aware planning and broader resource qualification
remain open. The runtime's VRAM self-report is not an independently measured
hardware peak or a proof of GPU fit.
