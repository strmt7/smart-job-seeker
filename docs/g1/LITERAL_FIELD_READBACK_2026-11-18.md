# Literal field readback · 18 November 2026

The prior two replays used byte-identical last-name images and produced
different visual readings. The new qualification freezes that filled image,
the preceding blank control, an email, an accented form label and an entered
number. Each case specifies its visible value and a contrary value that the
comparer must refuse. All images and the fixture are hash-bound before
inference.

## Initial comparison

The first experiment used the same blind prompt for both decoding modes.
Qwen ran in a private Ollama 0.40.16 session with a 16,384-token context
and the same 8,192-token response allowance. The decision mode used
`xhigh`. Literal decoding used zero temperature and presence penalty
with thinking disabled. The native replies confirmed thinking on the
deliberative path and completed JSON on both paths.

The deliberative request read four of five cases correctly. Literal decoding
got eight of ten reads correct across two requests per image. All three
readings of the filled surname reported a blank. The other cases matched
their expected values, and every contrary value was refused. The 195.796-second
run failed the frozen acceptance rule and preserved its raw outputs.

## Separating characters from headings

A second run asked the literal model to copy the displayed characters.
It copied the filled surname in both reads. Six of ten scoped comparisons
reported the target as unobserved because the model used ASCII separators
or joined heading and label text without a boundary. The deliberative
baseline passed five of five on that run. The literal matrix retained the
failed comparisons and finished in 246.417 seconds.

The current literal response supplies the ordered section headings, field
label and characters separately. The comparer inserts the structural
separators and preserves field and value punctuation. Both the raw response
and the resulting comparison remain in the attempt record.

With that response shape, the same five images passed both literal reads.
All ten native responses completed, all ten scoped values matched, and all
contrary values were refused. The test reused the retained deliberative
baseline. It took 93.745 seconds including private runtime startup and
teardown. The whole-host minimum available RAM in this run was 12.547 GiB.

## Browser evidence

The browser now records whether the detail image belongs to a labelled
native control with a nonempty value and a hidden placeholder on both sides
of capture. That evidence allows the model to copy visible characters as text.
The prompt supplies the image and transcription instructions. The comparer
checks the independently held actual value, section identity and disagreement
conditions. Scene decisions continue to use their separate model path.

Six contained numeric cases verified the native gate. The visible placeholder
stayed ineligible as an entered value. Exact entry qualified. Invalid quantities,
prose and changed constraints were refused. The positive case also refused
stale verification after the value changed.

Pre-request memory pressure recovery is scoped to each entered field. Each
recovery preserves the initial observation and requires a fresh native
readback and image under the remaining workflow deadline before visual
review resumes.

## Workflow rehearsal

Qwen 3.8 27B verified all eight fields natively and visually in the retained
five-section fictional workflow. Seven fields used the literal detail read.
The runner advanced four times and stopped when the next action produced no
new section. Simulated admission refusals at entries one and seven both
recovered within the shared deadline. The report preserves the first image and
the fresh verification for each recovery.

The test finished in 433.070 seconds. Minimum available RAM across 851 host
samples was 12.041 GiB. The private model session closed, its process exited,
and its port had no listener owners. The browser replay used zero cached write
responses and performed zero external deliveries.

The Windows workspace run passed 1,471 tests with 621 ignored. Clippy and the
Windows development build passed. This evidence covers this guarded fictional
workflow and the five development images. Independent final-summary coverage
and held-out workflows remain open.

Private evidence is under `target/literal-vision-862f02c9/`. All browser
interactions in this qualification were confined to fictional replays.
