# Private converter and model-runtime qualification

Measured 2026-11-09; checkpoint recorded 2026-11-10. This extends the browser/PDF
setup evidence in [runtime setup](RUNTIME_SETUP.md). It does not establish
clean-Windows compatibility, updater correctness or recovery after every failure.

## Production fix

First-use conversion reserved the PDF worker before provisioning a missing
converter. Provisioning bootstraps private Python under that same worker lease,
so the nested acquisition refused the operation. Conversion now retains its own
conversion lease while releasing the document-worker lease for setup. It must
reacquire the worker before executing the converter. Another PDF task starting
during setup therefore delays conversion rather than allowing overlap.
Two regressions cover bootstrap ownership and the intervening-task case; the
existing busy-operation test remains intact. Related PDF fixtures share a test
coordinator. Production concurrency limits were not loosened.

Version probes now give their children private profile/temp directories.
Ollama's probe uses an unconnected loopback port instead of the user's server.
Repair publishes only after archive verification, complete file readback and
its bounded executable check.

## Actual observations

| Component | Verified inventory | Functional smoke | Command duration | Minimum sampled free host RAM |
|---|---:|---|---:|---:|
| LibreOffice 26.8.2.2 | 19,442 files | Fictional DOCX through production conversion; PDF text/page checks and rendered PNG | 241.600 s | 17.619 GiB |
| Ollama 0.40.15 | 1,010 files | Private server; version, empty model list and empty resident-model list | 45.252 s | 17.810 GiB |

The Office smoke selects the newly installed converter explicitly in the same
production routine. A separate regression covers the missing-tool branch. This
is not a clean-host, interface-to-first-use qualification. The one-page PDF
retained three independently expected fictional headings; its rendered page was
inspected. This simple document does not establish generic template fidelity or
ATS compatibility.

Ollama ran on a dedicated temporary loopback port with its own user profile and
empty model directory. Only local version/list/status reads were issued; no
model was downloaded or invoked. Its owned server was terminated afterward.
The user's running service and model cache were not adopted or stopped. This
does not qualify all network egress paths of Ollama itself.

Each command ran under a four-GiB Windows job; setup/smoke workers had their own
768-MiB limits. Whole-host RAM was sampled every 500 ms: 479 Office observations
and 90 Ollama observations, with no sampler errors. Sampling is capped at 450
seconds and covered these runs. Values are rounded observations, not reserved
memory or exclusive application/GPU peaks. Runtime files stayed under ignored
`target/` directories. No employer form, application or application email was sent.

## Reproduction and evidence

With the existing Windows Rust/MSVC development toolchain:

```powershell
py -3.13 -X utf8 -B tools/qualify_runtime_setup.py --component office
py -3.13 -X utf8 -B tools/qualify_runtime_setup.py --component ollama
```

The wrapper uses the managed build helper, checks that exactly one opt-in test
executed, binds source/report bytes and preserves failures. Its timeout range is
60–1200 seconds. Ordinary Rust tests cannot implicitly download missing tools.

Private source-bound receipts:

- `target/runtime-component-run-odbae97k/result.private.json`, SHA-256
  `c54662d5e0d20b556767f40813f75deda25a0516b90fc80350c1055178e596c2`;
  native report in `runtime-component-nVCCLK`, SHA-256
  `e9b5022549ae40d7cf7d1aa0ca698a1181fd4ef6ac75dda829eefceaa81aecea`.
- `target/runtime-component-run-g0fqap1t/result.private.json`, SHA-256
  `3ffc995cfd2be594f1e0be718595f51615b7620feadab39d5daec534ed53cff2`;
  native report in `runtime-component-S9Ghrj`, SHA-256
  `b2360bc3869cb5ba2562dad9bb28442641e95627292a8e2b60f38df992b59198`.

The final managed web suite passed 753 tests with 403 opt-in checks ignored;
Clippy passed. Eight offline runtime-setup checks passed, including child-profile
isolation. Clean VM installation, installer-service failure schedules,
interruption cleanup and full application first-use recovery remain open gates.
