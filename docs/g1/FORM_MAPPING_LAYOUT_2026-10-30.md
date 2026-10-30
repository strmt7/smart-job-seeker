# Local form-mapping layout evaluation — 2026-10-30

The local model's output changed when only the serialization layout of the
fictional form fields changed. The [direct ablation](form-layout-ablation-2026-10-30.jsonl)
used the same prompt, model, fictional fields and options with compact or
indented JSON at four field counts. For the two-field fixture both layouts
mapped the given name. For the 10-, 15- and 20-field fixtures, compact output
omitted or flagged it; indented output mapped it. These are related variants of
one fictional example, not an accuracy benchmark for general forms.

Using indented JSON for every form then caused an existing short, adversarial
fixture to leave a country dropdown for manual review. The implementation keeps
compact JSON for groups below ten fields and uses indented JSON for larger
groups. It accounts for indentation in the prompt budget. The threshold is a
heuristic supported by these cases, not a guarantee of model behavior.

The [production-path live checks](form-layout-live-2026-10-30.txt) exercised a
25-field form, a 20-field form with a Portuguese authorization question and a
conflicting email hint, a short form with an adversarial label and country
option, and a multilingual browser fixture. The revised path mapped both names
in the 25-field case and left the authorization question for review. The short
fixture selected the country option but left the adversarial name for review.
These checks do not prove that the model detects all sensitive questions or
correctly understands arbitrary fields. Users still review every suggestion.

The independent [eleven-case evaluation receipt](model-evaluation-2026-10-30-form-layout-baseline.jsonl)
and [grades](model-evaluation-2026-10-30-form-layout-grades.json) were collected
before the layout change: ten cases passed their stated checks and one was
partial because citation shape does not establish factual support. That runner
uses its own prompts and is not a production-path regression test.

All live calls used local `qwen3.5:9b`, digest
`c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, one
fully GPU-resident model and a 4,096-token context. The production live-check
source file SHA-256 was
`e6773e887ae9a71f6af469cd4382c8db952f75c27c2dc4759394cde3b664c52c`.
Resource preflight before the direct ablation sampled 21.56 GiB free system RAM
and 10.82 GiB dedicated GPU usage; these are snapshots, not measured peaks. No
employer website or application submission was involved.
