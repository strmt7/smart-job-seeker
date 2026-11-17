# Qwen default integration — 17 November 2026

## Decision and scope

The default rehearsal model is now **Qwen 3.8 27B**, using reviewed compressed
weights, its native renderer/parser and explicit reasoning controls. This follows
the user's model-family decision. It is not a claim of superiority or universal
application completion. Explicit custom selections remain available through the
generic inference interface; missing capabilities must still fail explicitly.
Other consumers still use shared resident-model paths; unifying them under the
default owned profile remains product work, not a completed global migration.

The [15 November component results](MODEL_CAPABILITIES_2026-11-15.md) remain
historical evidence, including their visual regressions, malformed responses and
grading ambiguity. They do not become passing results because Qwen is the default.

## Reviewed model and runtime

| Item | Configuration |
| --- | --- |
| Product model name | `smart-jobseeker/qwen3.8:27b-ud-q3_k_xl` |
| Product manifest SHA-256 | `552b71d645d94564cfacdce245ba98de155c265de8ec3aa7c5e515e9590e0d8f` |
| Source model | `hf.co/unsloth/Qwen3.8-27B-GGUF:UD-Q3_K_XL` |
| Source manifest SHA-256 | `56d747cb510c031f215eea0c2cef98662ef76c95177667b11b0080e2a63f7bf0` |
| Weight SHA-256 | `7b20f0679da6f5c6ec7971e241630f44fe1e123e5c99bfea90c433268b355723` |
| Projector SHA-256 | `83ee4f4f205fa514161778c41df1ea14144faa0f713510893b63c2395f5c2d53` |
| Installed model bytes | 14,077,540,622; this is disk size, not peak RAM or VRAM |
| Runtime used | Ollama 0.40.16 |
| Renderer / parser | `qwen3.8` / `qwen3.5` |
| Integrated rehearsal context | 16,384 tokens; the manifest's original 8,192 default is explicitly overridden by the owned profile |
| Deliberative effort | Explicit `xhigh`; the native runtime advertises `medium` as its default |
| Deliberative sampling | Temperature 1.0; top-p 0.95; top-k 20; min-p 0; presence penalty 0; repeat penalty 1 |
| Structured extraction sampling | Temperature 0.7; top-p 0.8; top-k 20; min-p 0; presence penalty 1.5; repeat penalty 1; thinking off |
| Generation limits | Up to 8,192 deliberative tokens, 1,024 for literal transcription; complete reasoning and final output share the native limit |

The profile uses the [official model card's recommended parameters](https://huggingface.co/Qwen/Qwen3.8-27B).
Source GGUFs and their projector are distributed under Apache-2.0. The curated
alias reuses those bytes through the native [model-creation API](https://docs.ollama.com/api/create);
it does not train or duplicate weights in the same content-addressed store.

The [new Qwen4-Max announcement](https://qwen.ai/blog/qwen4-max) describes a
2.4-trillion-parameter model. The official Qwen Hugging Face API query for
`Qwen4` returned an empty model list on 17 November. Neither establishes an
available model suitable for this 16 GiB GPU. Qwen 3.8 27B remains the current
reviewed local choice; this is not an exhaustive ranking of all possible models.

## Product changes

1. Settings presents the curated Qwen install first. Installation verifies the
   pinned source before deriving the native profile, and verifies the derived
   manifest and advertised controls. Existing different model bytes are preserved.
2. A missing rehearsal selection resolves the curated default from local model
   metadata. It never silently chooses an older resident model. Preparation still
   verifies the actual model files before use.
3. The native renderer/parser, vision, thinking, tools and advertised `xhigh`
   support are checked explicitly for the curated default. Other model names keep
   the generic adapter contract rather than inheriting Qwen-specific controls.
4. Deliberative calls request Qwen's advertised effort and sampling. Structured
   extraction and blind first-pass transcription use the official non-thinking
   profile. Independently required native-associated detail and eligible visual
   rechecks use the deliberative profile. Original observations and all native,
   duplicate/conflict, factual and natural-completion checks remain intact.
   This configuration needs matched/held-out qualification before an optimality
   or general quality claim.
5. Qwen startup uses automatic placement rather than forcing every layer onto the
   GPU. The Qwen-specific reported CPU-offload ceiling is **6 GiB**, accommodating
   a busy GPU while keeping host admission and process containment independent.
   Other models retain their earlier ceiling. This is an upper bound, not a target
   or permission to exhaust RAM.
   The adapter now allows requested context up to 262,144; the
   [256k capacity assessment](QWEN_256K_CONTEXT_2026-11-17.md) records why the
   larger Q8 profile is not yet the integrated product default.
6. The Windows runtime package pins advance to Ollama 0.40.16 using the upstream
   release's archive sizes/digests. Full archive download and clean-host install
   qualification are separate from the local fixture checks.

## Resource controls and interpretation

Startup still requires model-file size plus **9 GiB available host RAM**. Its GPU
estimate is a snapshot, using the curated profile's bounded offload allowance.
Actual positive GPU allocation and reported CPU allocation are checked after
loading. The owned runtime retains a **3 GiB live host reserve**, aggregate job
limits, one loaded model, one parallel request and a bounded prompt cache. The
existing inference/browser checks also continue to apply.

Ordinary development containment uses shared half-free RAM admission. The first
integrated rehearsal used a 14.5 GiB inner application job and a 14.75 GiB outer
controller job. Subsequent 16k trials use 14 / 14.25 GiB, above the observed
14,406,070,272-byte eight-k integration peak. Retain actual context/load results
and resource failures rather than assuming an adequate limit. The separate
256k allocation probe uses an explicitly documented larger isolated-model
budget/reserve; it does not change ordinary tool admission.

The rehearsal deadline rises from 900 to **2,700 seconds** after the observed
mixed-material timeout. Deliberative native calls may use 900 seconds; the owned
qualification controller supports a bounded 3,600-second run. Cancellation and
host/resource checks are unchanged. A longer allowance is not evidence that a
workflow now completes.

Ollama's reported GPU allocation is **not** proof that all bytes are resident in
physical dedicated VRAM. Windows driver sharing, whole-adapter usage and process
commit are distinct scopes. Preserve all of them when diagnosing pressure or
latency. Unrelated desktop applications are not owned resources to terminate.

## Evidence checkpoint

Private native results, frozen fictional inputs, source/binary hashes and
controllers are retained under ignored `target/qwen-default-6bjf38ki/`. None of
these private files is a product fixture, credential source or tracked artifact.

- The first integration attempt stopped before loading on the earlier GPU
  estimate: 11,284 MiB free versus 14,575 MiB required. This is not a model score.
- The 17 November automatic-placement attempt with the earlier 4 GiB ceiling
  stopped before loading: 10,185 MiB free versus 11,184 MiB required. Its entire
  owned tree exited; sampled free RAM stayed at or above 32,321,011,712 bytes.
- One subsequent 15 GiB outer job was refused before process launch by shared RAM
  admission. A host audit found no matching survivors or resident models. The
  refusal remains retained rather than being counted as a model failure.
- With the 6 GiB offload ceiling, the same fictional rehearsal reached a native
  ready state using manifest `552b…0d8f`, runtime 0.40.16 and 8,192 context tokens.
  Its native report initially showed 13,231,340,910 GPU bytes and zero CPU bytes;
  the physical-residency limitation above applies.
- That all-deliberative eight-k run hit `session_budget_exhausted`, taking
  1,037.462 seconds including setup. It retained five verified fields and one
  entered/unverified field, not a passing complete case. Native application-job
  peak commit was 14,406,070,272 bytes; the sampler's first 900 half-second
  observations had a 10,325,499,904-byte minimum. The sampler did not cover the
  entire run. The owned process tree exited and the later host audit found zero
  matching resources/resident models.
- The first 16k non-thinking-transcription case ended cleanly in 263.103 seconds
  but failed its field gate, **0/1**: one known field verified, then a required
  detail observation used different section separator punctuation. Two unknown
  fields remained untouched. Raw observations/images and the refusal are
  preserved. Native peak was 14,375,993,344 bytes; the full run's sampled free-RAM
  minimum was 11,229,040,640 bytes; process/listener cleanup and unchanged shared
  runtime were confirmed.
- The subsequent deliberate-detail configuration completed its accounted replay
  in **924.609 seconds** and verified **7/9 known fields**, with **0/2 unknowns
  entered**, no source-binding failures and zero critical-entry/uncertainty flags.
  Its field-case gate still failed, **0/1**. Current employer was marked ambiguous
  despite one explicitly current reviewed record; the numeric projection omitted
  the question's unit and was correctly refused as `Question unit is missing`.
  Neither failed answer was silently filled. The actor stopped at
  `no_forward_control`; whole-workflow completion remains `not_qualified`.
  Source/inputs/oracle stayed bound and the shared runtime stayed unchanged.
  Native application-job peak was 14,426,419,200 bytes. The first 900 half-second
  samples had an 11,351,629,824-byte available-RAM minimum; sampling did not cover
  the full run. The owned tree exited. A subsequent scoped audit found zero
  matching processes/services/tasks/startup/resident models and 34,506,153,984
  available RAM bytes. This is teardown evidence, not sustained leak freedom.

This recovers the earlier separator refusal without relaxing the comparison or
supplying an expected value to vision. It does not prove generic improvement:
context, effort allocation and budgets changed, this is one authored development
case, and two known answers plus final navigation remain unresolved. Next work
needs a per-field proposal/coverage audit and guarded deliberative repair,
including whole-record temporal intent and exact question-unit grounding.

The final coherent local check passed **1,460 workspace tests / 618 ignored**
across 43 result groups, Clippy, the canonical debug build and 18 worker/native
owned-lifecycle tests. Packaging/runtime fixtures and the two model-settings
JavaScript checks passed at unchanged relevant source. The initial larger-context
check caught an old test still rejecting 131,073 tokens; the existing boundary
test now covers valid 256k and refuses values above it. No failure was hidden.

Compiled settings interactions passed at 1360/480/320 widths, including revision
conflicts, persistence and returning to the default. Default panels and native
action captures were inspected. These checks do not qualify every screen/zoom or
the default model's field accuracy. No release or application submission occurred.

## Research receipts

All page reads used the isolated, bounded development crawler. API captures used
their native transport. Captures remain private; public URLs above identify their
scope.

| Native capture | Result / SHA-256 |
| --- | --- |
| Official model card, earlier reviewed capture | HTTP 200; extracted text `aeb0caeca64be3588367ac48ae9fd53b84706418cc4f47ef32d09df0001c8b31` |
| Ollama model tags, earlier reviewed capture | HTTP 200; extracted text `7b552e0dff4f5f6dd7cd47bbc8cd2be56ad36101af74cd79f867e6a8636ba192` |
| Qwen homepage, 17 November | HTTP 200; capture `3bdf68bed3a80c68fb30e128fb3789689cdbf438e341f1a6131cf07cd81fe9b9`; initial content omitted the release detail, so it is not release-detail evidence |
| Qwen release page, 17 November, five-second bounded settle | HTTP 200; capture `7c5e733919f2f8088fbb93728821a160c413615fcab1a9e620a8db5752de746f`; extracted release summary, not the full announcement body |
| Official Qwen4 model inventory API, 17 November | HTTP 200; empty array; bytes `4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945` |
| Ollama 0.40.16 environment configuration API | Bytes `87e61f512a468e20a5c1e32699e45fb98669f807dad85770e010005924a8de07`; confirms the documented native GPU-overhead control, not its efficacy on this host |
| Ollama 0.40.16 scheduler source API | Bytes `5662ac7d42d2095c7fb3e78e60ca7fb3fc8e39a538cadde0bbd6cfdd6c5bb75e`; inspected to plan placement diagnostics, not evidence of a product change |
