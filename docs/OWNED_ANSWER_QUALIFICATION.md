# Owned local-model answer qualification

Reviewed 2026-11-10. The answer workflow now has an explicit private-runtime
qualification controller. It tests the production answer generator with
fictional facts; it does not interact with employer pages or send applications.

## Runtime ownership

`tools/qualify_owned_answers.py` takes an explicit Ollama executable, model store,
model tag and expected digest. It resolves the active v2 manifest before a legacy
downgrade anchor, checks the selected manifest and every referenced blob, then
copies only those files. A v2 manifest and retained legacy file can legitimately
have different digests; this was observed on Ollama 0.40.16 and verified against
its [manifest resolution source](https://github.com/ollama/ollama/blob/v0.40.16/manifest/paths.go).
It never rewrites or prunes the user's model store.

The controller uses the managed Windows build helper, verifies and stages its
OpenSSL runtime, and records the native test binary's hash. The worker, separate
Ollama server and native client run inside a kill-on-close Windows job with a
10 GiB aggregate committed-memory ceiling. Current RAM admission must allow
that ceiling; a 3 GiB whole-host reserve is checked during execution. Build
commands keep their existing 4 GiB limit. Time, output, model-copy size,
parallelism, queue length and context are bounded.

Only child-process environment variables select the private model directory,
profile and literal loopback port. Cloud inference and startup pruning are
disabled. The user's existing service must have no resident model; if another
model becomes active during the run, the controller stops only its own work.
No service, scheduled task, startup entry or visible window is installed.
The official [Ollama configuration documentation](https://docs.ollama.com/faq)
and [versioned environment definitions](https://github.com/ollama/ollama/blob/v0.40.16/envconfig/config.go)
were checked before implementing this development-only lifecycle.

Requests begin only after checking the private endpoint's native owning PID,
process creation time and executable, followed by version/model HTTP checks.
The initial CIM state-filtered observer failed on this host before inference;
its run and shutdown evidence remain private. The replacement uses
[GetExtendedTcpTable](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable).
The native endpoint tuple reported TCP state zero here. That value is retained
as observed, not relabelled as a known state; ownership and HTTP readiness are
checked independently. A real ephemeral-socket regression covers the observer.

The outer job drains before model copies and the runtime profile are removed.
Logs, model responses, usage, source/runtime hashes and sampled resources remain
under ignored `target/`. A forcibly killed outer controller can leave temporary
files; its kill-on-close job still owns the runtime tree. This is not a claim
that every product component is free of memory leaks or that all Ollama versions
and model-store layouts have been qualified.

## Observed model defect and correction

The first completed model run used `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`, with a
4,096-token context. A supported answer and an unsupported-experience abstention
met their labelled criteria. A conflicting writing preference caused the model
to invent leadership scope and years of unrelated experience. Its separate
evidence review correctly marked both claims unsupported, but the generator
still returned that text. All seven calls and the failed criterion are retained
in `target/owned-answer-qualification-u93724da/`.

The production generator now uses a known unsupported claim or numerical
support failure to request a factual correction within its existing two
revision attempts. Persisting violations fail rather than returning usable
text, including the length-review fallback. Newly introduced numeric source
markers are also refused in answer text; citations stay in structured metadata,
while existing references in source passages remain preservable. Successful
factual correction is disclosed through the shared evidence component.

Unavailable or uncertain model evidence remains advisory and requires manual
review; these changes do not establish that every claim has been detected or
proven. The three-case test is a narrow regression, not a universal capability
score or evidence of majority coverage on unseen applications.

The corrected run passed the same three criteria without changing its inputs:
supported text, an explicit unsupported-experience abstention, and a supported
answer after rejecting the conflicting factual preference. Generation used nine
calls in 12.954 seconds, versus seven calls and one failed criterion before the
fix. The two extra calls occurred only for the necessary factual revision and
its evidence review. The model did not need a smaller replacement or a larger
context. The successful report is
`target/owned-answer-qualification-8soc0wij/answers.private.json`, SHA-256
`93de6764fed83cbbc0853235bf85018e2abfaefc2770ac50d8a7cd44ae72dc1e`.

The contained runtime phase took 17.130 seconds, excluding build/staging.
Across 158 whole-host RAM samples, available memory stayed at or above
20,732,522,496 bytes. Runtime shutdown reported no cleanup errors; private model
copies and the private profile were removed. Eighteen request-boundary WDDM
samples on the 16 GiB-class RX 9070 XT ranged from 5,685,518,336 to
15,129,759,744 used bytes, including other host GPU usage. The runtime log reports
49/49 main-model layers and 5/5 draft-model layers offloaded. Ollama's resident
API reported 1,145,716,079 bytes for both `size` and `size_vram`; this is not the
whole-device WDDM measurement and must not be substituted for it. These are
sampled observations, not continuous application/GPU peaks or proof of zero
CPU memory use.

The subsequent integration follow-up preserved template behavior: a rejected
factual cosmetic retry keeps the earlier proposal and its original uncertain
evidence, with `needs_review`, rather than replacing it or confusing a factual
refusal with a transport failure. That error-classification change was checked
with the existing template regression; the unchanged successful model prompts
and cases were not rerun solely for that classification adjustment.

For another controlled run, use explicit local paths and the selected digest:

```powershell
python -B tools/qualify_owned_answers.py --ollama '<ollama.exe>' --model-store '<model directory>' --model gemma4:12b-it-q4_K_M --digest 6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b
```

The tool currently accepts regular library-model manifests in the verified v2
or legacy layouts, not arbitrary registry aliases, manifest lists or every
future storage format. The model must already exist locally; the qualification
does not download one. Native source captures, retained startup failures and
the successful run remain distinct evidence.

## Hosted browser follow-up

On pushed commit `f4d50769`, build/test, agent tools, PDF, supply chain, coverage
and CodeQL passed. CodeQL's three exact-commit analyses ingested without errors
and reported zero results/open alerts. The browser job passed 135 cases and
failed two; it is not counted as a successful suite. No finding was manually
closed, suppressed or excluded.

The parent-interception diagnostic now scopes Fetch to the exact fictional
worker destination, retaining its assertion of exactly one refused request and
zero arrival at the sink. Unrelated late page requests can no longer satisfy or
inflate that assertion. The redirect/subresource case now uses the existing
bounded fixture server with routed responses and full request buffering. It
keeps the server alive through all assertions and joins it on teardown. The old
15-second server lifetime included browser startup, and the old case stopped
its server before its final same-document navigation; the hosted log does not
establish the precise timing behind its refused connection.

All nine fixture tests passed, including fragmented redirect requests with an
idle connection. Both affected headless browser cases passed individually after
the correction, with their assertions intact. The full hosted suite still needs
confirmation on a subsequent pushed commit.

## Regression checkpoint

The final ordinary workspace run passed 1,237 tests with 581 opt-in cases ignored,
across 43 reported targets; workspace Clippy passed. The template integration
regression exposed the refusal-classification mismatch described above and
passed after preserving the earlier proposal's `needs_review` state. The failed
integration output is retained rather than counted as a successful run.
All 168 interface scripts and 41 development-tool checks passed. The compiled
saved-question history flow also passed its two connected checks and nine
viewport/actual-zoom cases with the new repair notice; all three screenshots
were inspected directly. Its private evidence is
`target/source-copy-ui-377763y5/`.
