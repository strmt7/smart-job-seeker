# Field review recovery and capture results · 17 November 2026

Saved-page rehearsal can recover once when a valid host memory sample refuses
visual inference before the request starts. It saves the refusal and the original
entry evidence, waits up to 45 seconds, then verifies the same offline document,
field and value before capturing a fresh image. The field is entered once.
An unavailable sample, expired wait, changed form or unverified value leaves
the attempt for review. Recovery of an interrupted in-flight request still
needs qualification.

Field and target images now use distinct filename ranges. Previously the
eleventh field's image could collide with a target image and stop the rehearsal.

A capture worker saves its result to the attempt archive and returns a small
receipt. The parent checks the worker's exit, attempt identity, byte count and
SHA-256 before accepting it. Selected reports can contain up to 8 MiB. History
defers reports above 256 KiB until their details are opened, and rechecks the
selected job and attempt before returning them. Previously archived reports use
the same reader. The rehearsal's outer deadline includes its 45-minute browser
allowance as well as the existing setup allowance.

## Verification

| Check | Observed result |
| --- | --- |
| Native headless attachment review | A retained fictional PDF was confirmed from fresh evidence. Reopening the page made the prior evidence unusable. |
| Local Ollama and Qwen, 16,384 context | 3 of 3 fictional fields matched natively and visually after one simulated pre-request refusal. The fixture detects a repeated first-name entry. Whole-host samples stayed at or above 10.09 GiB of available RAM during the owned run. |
| Worker result transfer | A synthetic result above 512 KiB passed verification. A wrong attempt, corrupt digest and repeated write were rejected. |
| Compiled Windows app over loopback | A normal worker saved and returned a 1,228-byte result. The actual history and detail routes opened a separate 468,306-byte archived result without modifying it. |
| Headless interface | The visual follow-up suite passed 65 checks. Screenshots of the expanded saved-page details were inspected at 320 px and at 200% browser zoom. |
| Workspace | 1,466 Rust tests passed with 618 ignored. All 172 UI checks, warnings-denied Clippy and the Windows app build passed. |

The local-model case exercised native verification and actual inference with a
simulated memory refusal. It measures neither recovery under real host pressure
nor complete application workflows. The large-result case establishes transport
and retrieval behavior. Its single-record scope leaves populated history
performance open.
