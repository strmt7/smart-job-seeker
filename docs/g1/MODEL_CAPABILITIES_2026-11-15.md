# Local model capability qualification — 15 November 2026

## Scope and decision

The compressed Qwen3.8-27B candidate now has verified **vision, observable
thinking/final-answer separation, and a native typed tool proposal**, using an
explicitly advertised `xhigh` request. No proposed tool was executed. This is
component/configuration evidence, not universal application completion or a
reason to replace the existing model.

The ordinary Hugging Face GGUF import advertised these capabilities but lacked
the native renderer/parser configuration used by the official model. Its high
request could return valid structured output without observable thinking; its
unformatted reasoning arrived mixed with final output. Those failures remain
retained. Model identity and successful HTTP status alone did not qualify the
capabilities.

## Inputs and configuration

- Runtime: Ollama **0.40.16**, private loopback listeners and task-owned model
  stores/profiles. Shared runtime pre/post resident-model states remained equal.
- Baseline: `gemma4:12b-it-q4_K_M`, manifest
  `6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.
- Candidate weights: `unsloth/Qwen3.8-27B-GGUF` commit
  `4af51b3542f775b0a8e4b310bfb3f17e01c306c9`, UD-Q3_K_XL file
  `7b20f0679da6f5c6ec7971e241630f44fe1e123e5c99bfea90c433268b355723`,
  BF16 projector
  `83ee4f4f205fa514161778c41df1ea14144faa0f713510893b63c2395f5c2d53`.
  Actual source files were hashed before inference.
- Native private alias: `sjs-qwen38-27b-q3-native:trial`, manifest
  `8f54e264d8793ea36ac67b7ffc89cf727bfe1e2d0e286a1cc3ac4295eecd5f0b`.
  It reuses the same weights/projector; no training or additional model download.
- Official registry metadata supplies renderer **`qwen3.8`** and parser
  **`qwen3.5`**. The native alias advertises thinking values
  `[false, "low", "medium", "xhigh"]` with runtime default **`medium`**.
  Therefore boolean thinking is not proof of maximum effort. Explicit `xhigh`
  was requested and native thinking was observed.
- Six development probes were frozen before inference: two blind image reads,
  source-specific quantity extraction, unsupported sponsorship advice, unstated
  unit conversion, and semantic review intent without action authority. Images
  are historical fictional native captures; expected answers were not included
  in visual prompts. These cases are neither held-out nor full browser journeys.

The native alias was created only in the private test store using the documented
[model-creation API](https://docs.ollama.com/api/create). Registry manifest/config
bytes and small layers were captured through native registry transports and
digest-verified. No official large Q4 weights were downloaded. See also
[thinking-control discovery](https://docs.ollama.com/capabilities/thinking).

## Retained observations

All results below retain original model outputs and native completion metadata.
Strict matches include representation contracts; they are not semantic accuracy
or universal application rates.

| Configuration / scope | Strict result | Interpretation |
| --- | --- | --- |
| Baseline, 4,096 context, thinking off | 4/6 | One clipped-label visual failure and one unknown-value representation mismatch. |
| GGUF candidate, same 4,096 profile | 4/6 | Both visual probes failed, including invented clipped-label text; four text probes matched. |
| Baseline, 8,192 context, boolean thinking, 4,096 generation budget | 3/6 | Observable thinking in all six responses; more reasoning did not establish better task results. |
| GGUF candidate, 8,192 context, `high` request, structured output | 6/6 | No observable native thinking. This cannot qualify enabled reasoning or isolate why outputs changed. |
| GGUF candidate, boolean thinking without output grammar | 0/6 valid structured finals | Unparsed reasoning/final prose failed the consumer's JSON contract. Runtime/import configuration failure, not six independently established reasoning failures. |
| Native alias, 8,192 context, `high` request | 5/6 | Separate thinking/final fields and both visual probes matched; one unknown representation mismatch remained. No maximum-effort claim from this request alone. |
| Native alias, explicit advertised `xhigh` | 4/6 | Observable thinking in all six responses. The clipped numeric image was omitted; unsupported sponsorship remained unknown but used a non-null value representation. |
| Native alias, explicit `xhigh`, separate native capability checks | 2/2 | Completed arithmetic with a separate thinking field and exact final integer; one correctly typed public-reference tool proposal. No execution of the proposed tool. |

The unknown representation mismatch means `status: unknown` accompanied by a
string rather than the oracle's null. It does **not** establish a fabricated
sponsorship answer or a performed unsafe entry. The original prompt/schema did
not make that conditional representation as explicit as the oracle; future
grading must clarify that contract before a new frozen trial, preserving these
results. Do not silently reinterpret them into passes or model-intelligence
failures.

The six-case configurations intentionally differ in context, generation budget,
effort and/or import metadata. They diagnose configurations; they do not isolate
the causal effect of one setting. The highest effort regressed one visual result
relative to the earlier native configuration. No profile/model promotion follows
from this small development set.

## Resource and lifecycle evidence

Model jobs used a RAM-admitted **14.5 GiB aggregate commit limit**, retained a
three-GiB host reserve check and a 512-MiB prompt cache/one checkpoint. Native logs
confirmed the projector and model layers loaded. The runtime reported all of its
model allocation on GPU; that is not whole-adapter WDDM usage or proof that every
allocation resides exclusively in VRAM.

The largest observed model-job commit was **14,172,241,920 bytes**. The minimum
half-second sampled host headroom across these inference trials was
**15,834,972,160 bytes**. Model sampling covered each run until teardown; these
observations do not qualify sustained leak freedom or every instant between
samples. All owned model trees drained and shared resident state remained
unchanged. The ordinary product profile/default model was not changed.

Private native reports, original failures, source/image hashes and request
controllers remain under ignored `target/model-agent-research-6vxs2qxd/`.

## Product changes and remaining acceptance

The core capability probe previously treated a correct arithmetic final answer
as evidence that thinking was enabled, and accepted any JSON object as the
structured probe's answer. It now requires naturally completed expected JSON,
observable separate or complete inline thinking, and an exact final arithmetic
answer outside that thinking. Three adversarial regressions cover wrong JSON,
missing/empty/malformed thinking, misleading numeric substrings and partial stops.
This checks controls, not the truth or quality of a model's reasoning.

A native compatibility check on the existing 12B model retained a **768-token
length stop** rather than treating its partial arithmetic prose as success. The
bootstrap probe now requests brief arithmetic reasoning with a **2,048-token**
budget and **90-second** generation deadline; metadata reads keep their shorter
deadline. The same baseline then passes both the completed structured check and
separate-thinking/exact-final-answer check (**2/2**, natural stops). The successful
reasoning response used 314 tokens. Prompt and budget changed together, so this
does not isolate their effects. This bootstrap is not a cap on substantive task
reasoning or evidence that maximum effort improves all tasks.

A native HTTP fixture additionally exercises the actual `probe()` route, checking
the separate structured/reasoning requests and refusing answer-only or incorrect
JSON runtimes. The first compile of that added test failed on a missing test-local
listener import; the failure is retained and the import is corrected.

The focused inference suite passes **31 tests / 2 ignored**. Final canonical
Windows workspace verification passes **1,452 tests / 618 ignored / 43 result
groups**, with warnings-denied workspace Clippy and the canonical debug app
build. Nine scoped-search regressions and the
new skill's structural validation also pass. Browser/UI assets are unchanged;
earlier live-browser and rendered-interface evidence retains its original scope.
The new repository qualification skill incorporates the catalog's experimental
evaluation/checkpoint guidance with its MIT notice; it is development guidance,
not automatic runtime prompt injection or a demonstrated performance gain.

Next, qualify the complete production actor with native-supported per-role
inference profiles and sufficient admitted context/reasoning budgets. Keep the
generic navigation/final-review gap, independent held-out workflows, tool-result
validation and current source/native/visual guards open. A bigger model or a
working tool call cannot close those gates by itself.
