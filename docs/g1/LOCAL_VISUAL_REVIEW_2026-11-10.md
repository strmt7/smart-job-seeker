# Local visual review checkpoint — 2026-11-10

This is narrow development evidence, not a release or universal form-accuracy
qualification. All interaction used fictional file/loopback fixtures headlessly.
No employer form, application or application email was sent.

| Check | Measured result | Method / retained evidence |
| --- | --- | --- |
| Ordinary Windows workspace | 1,238 passed; 582 opt-in cases ignored; 43 reported targets | Managed `-Test -Workspace`, four test threads, owned 4 GiB job; `target/owned-vision-final-workspace-tests.private.log` |
| Workspace lint / formatting | Passed | Managed `-Clippy -Workspace` after the external compiler update; `cargo fmt --all -- --check` |
| Interface regressions | 169 scripts passed | Every `apps/web_app/tests/*.test.js`; `target/missing-visual-interface-suite.private.log` |
| Tool regressions | 47 passed | `unittest discover -s tools/tests -v`; `target/missing-visual-final-tool-suite.private.log` |
| Actual local vision | 8/8 combined criteria; 6/8 raw criteria | Selected 12B model, 4,096 context, same seven images/eight answer conditions; `target/owned-answer-qualification-3mlp1xfz/` |
| Inference / resources | Nine normal completions, 210 output tokens, 23.750 seconds native test time | 304 host-RAM samples, minimum 24,063,959,040 available bytes; private model/profile removed, cleanup errors empty |
| Compiled rendered flow | 18 grouped checks passed | Missing transcription plus mismatches; five viewport widths and twelve actual-zoom combinations, 80–400%; `target/layout-check-run-r64na2kt/` |
| Visual inspection | Three compiled PNGs byte-identical to directly inspected source-pass PNGs | `target/visual-followups-ui-sxULNP/` versus `target/visual-followups-ui-oy2OhQ/` |

The actual vision report SHA-256 is
`f5ac493af8cc4cdcd4f0d30ed23171657076db6acde88046a2b6520578a4c633`;
the compiled rendered report is
`d1fdb12ce5d4e890696c35b7a231d506d021e02ce7394f56aec74dc33af3ca25`.
The latter uses deliberately substituted API/model responses and verifies
presentation/scheduling, not additional model intelligence. The former compares
blind local-model transcriptions against labelled criteria and production DOM
checks, preserving raw failures and separate focused observations.

[Visual method and retained failures](../LOCAL_VISION_QUALIFICATION.md) include
the placeholder and diacritic mistakes, checkpoint allocation failures, initial
startup timeout and interrupted linker build. [Interface method](../VISUAL_FOLLOWUP.md)
records the source-routing failure, later compiled run and owned cleanup checks.
The externally started Visual Studio update was observed, never modified.
Unseen forms, complete no-send isolation, clean-host installation and broader
local-model transfer remain open in the [completion plan](../COMPLETION_PLAN.md).

Hosted follow-up for commit `45a533614a19b92db0c96fb9373b24d93b7e3391`:
[CI run 42888732033](https://github.com/strmt7/smart-job-seeker/actions/runs/42888732033)
passed all five jobs; coverage and the CodeQL workflow also succeeded. The
JavaScript/TypeScript, Python and Actions analyses ingested without errors and
each reported zero results (analysis IDs 2138393134, 2138389642, 2138387804).
Native API observations and normalized connector job evidence remain under
`target/visual-45a-*`. This confirms that pushed checkpoint, not later local work.
No finding was manually closed, suppressed or excluded.
