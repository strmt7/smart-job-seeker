# Qwen 256k context assessment — 17 November 2026

## Result

The pinned Qwen 3.8 27B model **loaded a native 262,144-token context with Q8 KV
cache and completed a short request** on this Windows host. The successful probe
retained its eight-GiB sampled RAM reserve. This establishes short-request
capacity, not accuracy over a full 256k input or readiness of the application's
current inference lifecycle.

The standard F16 cache configuration failed with a native GPU allocation error.
The first Q8 attempt was stopped by its RAM-reserve monitor; the same eight-GiB
configuration later succeeded with more starting headroom. Both failures remain
retained. Do not present the selected success as first-attempt reliability.

## Native configuration and observations

The model/weight/projector identities are those in the
[default integration report](QWEN_DEFAULT_2026-11-17.md). Ollama 0.40.16 used its
native renderer/parser, one parallel request, two CPU threads, batch 128,
512-MiB saved-prompt cache and one checkpoint. The isolated Q8 runtime explicitly
set `OLLAMA_FLASH_ATTENTION=1` and `OLLAMA_KV_CACHE_TYPE=q8_0`. The native log
confirmed both quantized K/V caches and flash attention; environment variables
alone were not counted as acceptance.

The request asked for one fixed word, with thinking off and a 16-token generation
limit. Sampling used the official non-thinking profile. It contained no candidate
information and performed no browsing, application action or external write.

| Attempt | Native outcome | Host / lifecycle observations |
| --- | --- | --- |
| Initial admission | No process started; fixed 20 GiB maximum plus eight-GiB reserve exceeded the observed starting headroom | Shared runtime unchanged. The later probe admitted the largest ceiling below that maximum while preserving the full reserve. |
| F16, 256k | HTTP 500; ROCm allocation failed for a 7,168-MiB KV buffer | 58.969 seconds; sampled available RAM minimum 15,063,584,768 bytes. Initial immediate teardown observations conflicted with the controller's root-exit accounting; a later independent native check confirmed the PID absent and port empty. Preserve that discrepancy. |
| Q8, 256k, first | Monitor stopped the owned tree at the eight-GiB reserve | 30.726 seconds; minimum 8,438,792,192 available bytes; native process signaled exited, listener empty and request thread exited. |
| Q8, 256k, repeat | Native context 262,144; natural completion of the short request | 37.094 seconds; start 31,190,462,464 available bytes; sampled minimum 8,745,152,512 bytes; native job peak commit 18,317,221,888 bytes; process/listener/request-thread teardown confirmed; shared runtime unchanged. |

The successful repeat reported total model allocation **24,204,143,488 bytes**,
GPU allocation **14,846,777,096 bytes**, and therefore reported CPU allocation
**9,357,366,392 bytes** (about 8.7 GiB). GPU API allocation is not proof of
physical dedicated-VRAM residency. This is substantially more host spill than
the application's current six-GiB reported-CPU ceiling admits.

The F16 probe's native peak-job observation, 23,657,902,080 bytes, exceeded its
configured 21,289,172,992-byte job ceiling. Retain and investigate the accounting
and allocation-failure behavior; do not claim this single probe proves a hard
whole-host/GPU memory bound. The continuous host-reserve monitor and subsequent
native cleanup observations are separate evidence. No unrelated process was
terminated.

The repeat's private controller filename contains an obsolete experimental
reserve label. The attempted edit to that label failed before launch; the actual
controller, recorded settings and native result all retained **eight GiB**.
The table reports the effective configuration, not the filename.

## Why cache configuration matters

Native GGUF metadata advertises a 262,144-token context, 65 blocks, full attention
every fourth block, four KV heads, and 256-dimensional keys and values. The
observed attention cache has **16 layers**. At 256k, its native Q8 allocation is
**8,704 MiB**: 4,352 MiB each for K and V. The equivalent F16 attention-cache
estimate is about **16 GiB**, before weights, recurrent state, projector and
working buffers.

The [official Ollama FAQ](https://docs.ollama.com/faq) describes the memory and
precision tradeoff of cache quantization. Q8 reduces memory; its quality must
still be measured on these tasks. Q4 was not substituted to manufacture a fit.
The [Qwen model card](https://huggingface.co/Qwen/Qwen3.8-27B) advertises native
256k capacity; it does not establish task accuracy on this hardware/runtime.

## Product consequences and remaining gates

The inference adapter and visual-context validator now permit requests up to
**262,144 tokens**. That API ceiling is not an automatic allocation or a capability
claim for every model. The current managed rehearsal default remains **16,384**
until its owner can request the Q8 profile and account for its larger allocation
consistently. Silently overriding an existing shared daemon's cache settings,
weakening factual checks or accepting truncated output is not a valid integration.

Next work must:

1. Put model, renderer/parser, context, KV type, effort and memory policy in one
   source-bound runtime profile used by all consumers, including setup and status.
2. Qualify the larger reported CPU allowance against native runtime/process,
   whole-host and whole-adapter observations. Preserve hard host protection and
   actual teardown on error, cancellation and native allocation failure.
3. Compare F16/Q8 and smaller/256k contexts on fixed independent factual, visual,
   tool and long-distance retrieval tasks. Record full input-token counts and
   result denominators; a short request cannot grade long-context accuracy.
4. Run complete candidate/material/form-to-final-review journeys. Retain earlier
   field and formatting failures and their versioned outcomes.
5. Expose requested versus effective context/cache and resource reasons in
   Settings. Do not silently substitute a smaller context or another model.

Context capacity should be available when useful, while prompt assembly still
preserves exact facts, current state and relevant source evidence. Filling a
window with unrelated text is not evidence of better intelligence.

Private native logs/results remain under ignored
`target/qwen-default-6bjf38ki/context-256k-*/`. The official FAQ capture used
the bounded isolated crawler, HTTP 200, native capture SHA-256
`b8e867f8758ff35250a1ef73b9f70871bcdb06eb54f555d062ad3c22c398cb6f`.
No release or application submission occurred.
