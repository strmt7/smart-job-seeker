# Checked employment dates in saved applications

The saved-copy rehearsal can now carry a checked career-history answer to a
native number or month control. Previously the history planner could produce
both answers and the executor discarded those proposals. The replay keeps a
month as a typed `YYYY-MM` value. Chromium checks the current type, minimum,
maximum and step using a detached input before entry and checks the live value
afterward. A conflicting existing value requires review. Each capture stays
bound to its inspected document, control and native section.

The vision model transcribes the displayed month independently. The app compares
it to the native month using the existing Gregorian locale data. A final review
row may display that month in words. Its value must agree with the typed capture,
and the visual observation must agree with the actual summary row. Ambiguous
labels, conflicting observations, wrong months and reduced years require review.

## Windows fixture results

The fictional fixture uses identical leaf labels under **Current employment**
and **Most recent completed employment**. The original native run failed. The
first repaired browser run filled four fields, including `April 2021` and
`July 2014` as rendered by Chrome. It refused a full date, an out-of-range month,
an off-step month and a conflicting replacement. The active fixture and private
receipts are in `apps/web_app/tests/form-history-periods.html` and
`target/history-periods-4bfa92dc/`.

The final native run passed both the scoped periods test and the existing six
numeric admission cases. The compiled workspace passed 1,487 tests, with
623 opt-in checks ignored by that command. Clippy passed with warnings denied.

## Local-model qualification

The frozen candidate records have different start years and months. The first
Qwen run correctly selected the checked record and component for all four
questions. It verified the first year and stopped before the first month when
its two visual reads disagreed about the neighboring year control. The revised
control prompt describes the visible difference between editable steppers,
calendar inputs and choice-list selectors. Both visual reads still have to
agree. The first scored run is retained in
`target/private-rehearsal-qualification-okk_g7x9/`.

A second run reached entry of the first month and exposed a missing typed-date
arm in the capture's intended-value accessor. The native regression now checks
both commanded and observed values. That failed run is retained in
`target/private-rehearsal-qualification-ju5h2933/`.

The next run independently planned, entered and visually verified **4/4**
fields. Each answer used the checked record for its own section. The replay
stopped at the visible **Review application** button because the current native
navigation filter has no recognized forward candidate for that wording. The
workflow qualification therefore failed its final-review assertion. The review
page and the calendar values it would display remain unqualified together on
this fixture. The native receipt is under
`target/private-rehearsal-qualification-r4y5kf02/`.

The run used the default Qwen 3.8 27B manifest `552b71d6…e9590e0d8f`, Ollama
0.40.16, 16,384 context tokens and explicit `xhigh` reasoning. It took 292.08
seconds including startup and cleanup. Peak job commit was 13.44 GiB and minimum
sampled host availability was 11,830,239,232 bytes. The owned model session
closed and the replay recorded zero external delivery actions.

Retain the failed workflow result for the next navigation iteration. Exercise
section changes and final-review transitions using current native control
identity, typed intent and post-action evidence. The existing no-send boundary
applies to every rehearsal control.
