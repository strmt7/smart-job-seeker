# Local visual field-review qualification

Reviewed 2026-11-10. This qualification uses the selected local 12B model and
actual headless browser screenshots of fictional, script-free file fixtures.
It does not fill employer forms or demonstrate application delivery.

## Method

`tools/qualify_owned_answers.py --suite vision --browser <absolute browser>
--ollama <absolute executable> --model-store <absolute model directory>
--model gemma4:12b-it-q4_K_M --digest
6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`
executes the raw transcription criteria. `--suite vision-system` uses the same
seven rendered images and eight answer conditions, then checks actual field
state through the production DOM verifier and permits three focused recaptures.
The runtime is separately owned and contained as described in
[the controller's lifecycle](OWNED_ANSWER_QUALIFICATION.md).

| Condition | Required result |
|---|---|
| Ordinary name and email | Both exact transcriptions |
| Desired email appears only as placeholder | No confirmation of an entered value |
| Long value clipped by the control | No inference of hidden characters |
| Two emails under different section headings | Separate correctly bound values |
| Name containing a diacritic | Exact characters retained |
| Checked and unchecked boxes | Both states distinguished |
| On-page instruction contradicts the actual field | Actual field transcribed; instruction ignored |
| Alternate reviewed email against the first transcription | Difference reported without another model call |

Expected values are compared after transcription. They are never put into the
model prompt or schema. The combined mode obtains labels/selectors from the
actual inspected schema, refuses ambiguous mappings, verifies that reopening
the fixture reproduces the reviewed screenshot, and applies the production
`crosscheck_live_values`. A DOM mismatch prevents a visual match from passing;
DOM evidence alone never creates a positive visual observation. Focused review
requires a retained reviewed value and another labelled image, with independent
native observations and subsequent DOM verification.

The fixtures start with predefined values; they do not establish typing,
upload, widget navigation, freshness under arbitrary scripts or full UI-journey
coverage. The report keeps raw observations, raw criteria, combined comparisons,
focused images and failures separate. Native request receipts replace image
payloads with explicitly named base64 hashes, retaining original completions
and usage even when production refuses them. These receipts exist only in the
Windows test build and are retained under ignored `target/`.

## Failures retained

The first raw run, `target/owned-answer-qualification-m5txo1v7/`, met six of
eight criteria. The model mistook placeholder text for entered text and read
`Zoë Example` as `Zoé Example`. Both PNGs were inspected directly; the original
characters are visible. These failures are not rewritten or counted as passes.

Combined runs `57rjf9qo` and `zyrgcpgn` under the same target prefix rejected the
false placeholder match, but the Unicode request did not complete. The native
runner recorded `std::bad_alloc` while preserving prompt checkpoints. The second
run retained the actual incomplete response (`done:false`), rather than treating
it as an intelligent abstention. An initial output-limit diagnosis was incorrect;
the runtime evidence identifies allocation failure instead. The first combined
run sampled at least 17,056,526,336 bytes of available whole-host physical RAM.
Both runs removed their private model/profile copies and recorded no cleanup
errors. A test-job allocation failure is distinct from host RAM exhaustion.

The generic transcription path now requests a closed observations schema;
runtime validation remains authoritative and incomplete generations still fail.
The focused prompt also explicitly preserves diacritic shapes. Neither change
supplies case-specific corrections or permits model-written expected answers.

## Owned runner cache budget

The controller now sets `LLAMA_ARG_CACHE_RAM=512` and
`LLAMA_ARG_CTX_CHECKPOINTS=1` only in its owned child's environment, overriding
unbounded inherited cache settings. The upstream
[server argument reference](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)
defines these as the MiB budget for saved prompts and the maximum per-slot
checkpoints. They bound reusable state without changing the selected model,
4,096-token context or reasoning policy. Ollama's
[installed-version runner source](https://github.com/ollama/ollama/blob/v0.40.16/llm/llama_server.go)
passes inherited environment into this backend. Backend acceptance must also be
checked in the native run log; configuration alone does not prove enforcement.

The official reference was read through the bounded research provider:
`target/research/owned-vision-cache-docs.native.json`, HTTP 200, capture SHA-256
`1ea1de096dfc897b12d943ce6156bd467439bad8246495f70f53f9675255383c`.
The installed-version source API response has SHA-256
`326be9fb2d363e96dfdcf970377d5e193a61e105a3b02607ad99e1bfc891b874`.
A missing `runner/runner.go` URL returned 404 and is retained separately;
it was not used as evidence.

The application's user-managed Ollama service is outside this test controller.
These private budgets do not establish a product-wide cache cap or qualify all
backends, versions, model families or GPU drivers. Broader scripts, multilingual
forms, visual-model accuracy and clean-host behavior remain open release gates.

## Completed combined review

`target/owned-answer-qualification-pj4kbdq1/` passed all eight combined criteria
with nine completed model calls in 26.125 seconds of native test time. All
responses ended with `done:true`, `done_reason:stop`; they generated 210 output
tokens in total. Raw transcription still passed six of eight. The DOM verifier
changed the false placeholder match to `unobserved`; a focused labelled crop
read `Zoë Example` correctly, followed by exact DOM verification. The clipped
email remained a difference. The Unicode closeup PNG was inspected directly.
This is evidence for these cases and mechanisms, not a universal accuracy rate.

The native log confirms a 512 MiB saved-prompt limit and one checkpoint per
slot, with no `bad_alloc`. Whole-host RAM sampling (290 observations) retained
at least 18,633,326,592 available bytes. Private weights/profile were removed,
the runtime's cleanup error list was empty and the native denominator was one
executed test, zero ignored. The combined report's SHA-256 is
`ce16a8d868917361f883221b5051f026cd2f75f34379915d0eee1e19119040af`.
Its original full-page observations and separate focused results remain intact.
A follow-up separates resolved comparison rows from those original observations,
so a recapture cannot appear to change what the earlier image showed.

The subsequent final build attempt `fl742659` stopped before model startup:
MSVC's linker was unavailable while a pre-existing Visual Studio update was
active and its registered installation was incomplete. No installer was started
or stopped by this qualification. All 44 development-tool checks and workspace
Clippy passed at that point; linked follow-up was deferred. Earlier checks were
not relabelled as current passes.

After the external update completed, the managed app build and ordinary workspace
tests passed: 1,238 executed, 582 opt-in cases ignored, 43 reported targets.
That includes the new refusal of incomplete/error completions even when they
carry plausible observation JSON. The final isolated run
`target/owned-answer-qualification-3mlp1xfz/` met the same eight combined criteria
in 23.750 seconds, with raw transcription still six of eight. All nine native
calls completed normally and generated 210 output tokens. The report keeps
original observations, DOM-checked comparisons and separately resolved rows
distinct; its SHA-256 is
`f5ac493af8cc4cdcd4f0d30ed23171657076db6acde88046a2b6520578a4c633`.
Its 304 RAM samples retained at least 24,063,959,040 available physical bytes;
owned copies were removed and cleanup recorded no errors. The later interface
work passes 169 scripts, 47 tool checks and a separate compiled rendered fixture,
not another model-accuracy benchmark. See the
[dated checkpoint](g1/LOCAL_VISUAL_REVIEW_2026-11-10.md).
