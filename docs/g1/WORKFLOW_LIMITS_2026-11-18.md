# Form, document and discovery workflow limits — 18 November 2026

Three implementation limits interrupted ordinary workloads. A saved form could
be editable in a same-origin frame while the navigation census searched the
top-level document. Template selection showed the model only the beginning of
the job ad and the first eligible lines of the template. Discovery stopped at
600 candidates and could skip a valid link after the first 500 provider rows.

## Saved application rehearsal

The browser binds navigation to the frame chain that owns the current fields.
It checks the native owner, document, loader, source, and control inventory
again before advancing. The same chain binds the terminal summary, screenshot
regions and retained attachment checks. Ambiguous document scopes require
review. All interaction in this qualification took place under offline replay.

The frozen three-section fixture had SHA-256
`438de9abc430779742f5ac1c637def304b36d25b324bddc676093f65236fca1a`.
The previous run verified two text fields and stopped with `no_forward_control`.
The revised run advanced twice and independently matched five of five summary
answers. Both original PDF byte digests were confirmed, the unfilled optional
field stayed untouched, and two answer rows used frame-bound detail captures.
The replay recorded zero refused requests, cached non-read replies or
unreplayed transports. External delivery and server validation were outside
the replay. See [the prior checkpoint](SCROLLABLE_APPLICATION_REVIEW_2026-11-18.md).

The owned Qwen 3.8 27B run used Ollama 0.40.16 and a 16,384-token context. The
test took 980.60 seconds. Two deliberative file-label image reads generated
2,620 and 4,720 tokens with natural completion. Those calls warrant an ordered
comparison of literal and deliberative detail reads on the same frozen images.
Recorded peak process-job commit was 14,544,531,456 bytes. Sampled available
host RAM reached a minimum of 10,324,180,992 bytes. The owned runtime closed.

Private evidence is in `target/private-rehearsal-qualification-029einc2`. The
native result SHA-256 is
`df5e9bf7e4482c42e39dd4c9927fadac19641b7944da31897bb5cba905d39083`.

## Template selection and discovery

Template planning sizes each request from the resident model's reported
context and preserves whole eligible passages. A bounded request can offer
its own identifiers, then the planner carries selections back to stable
original IDs. If the job ad needs multiple windows, the coverage record gives
the number fully compared. A regression selected passage 93 using a requirement
in the last listing window, with all 93 passages offered.

The separate native template qualifier now uses the production model
inventory. It resolved all six files and the expected digest of the
14,077,541,677-byte namespaced Qwen model. Admission initially stopped at
about 8.5 GiB available against a 10 GiB job and 3 GiB reserve. After
physical memory recovered, a 12 GiB run reached a native allocation
assertion during model load. It generated no template response. The
original failure remains under `target/owned-answer-qualification-nzht7u2d`.

The 16 GiB job retry used the same Qwen and Ollama 0.40.16. At an observed
8,192-token context, Qwen selected the exact editable DOCX passage for the
requirement at byte 2,400 of the ad. All eight eligible passages were
considered. The request completed naturally after 780 prompt tokens and
88 output tokens. The owned job peaked at 13,783,310,336 bytes of commit.
Its minimum sampled host headroom was 14,055,555,072 bytes. The runtime
closed and its disposable model copy was removed. The result under
`target/owned-answer-qualification-lbo2rill` has SHA-256
`d44bf047e0a402fe4bbc64bfc82e329286bf5a192ec464faf07bc269e44b4bd3`.
This qualifies a native late-listing selection case. The larger 93-passage
case exercises batching with fixed model responses.

Discovery parses every row within its capped transport response and retains
exact URL provenance. A regression admitted the only valid link at row 502.
A retry retained 601 distinct URLs, and a separate size-boundary case kept
smaller candidates and accounted for the oversized omission. The sealed
history format and provider rate and access checks remain in use. Durable
per-page storage, independent provider recovery and a representative measure
of real application-route coverage remain open.

## Integration

The Windows workspace ran 1,478 passing tests. The 622 ignored cases are
separate from that result. Clippy, the canonical Windows build and 172 UI
scripts passed. Seventeen focused qualification-controller tests passed.
Native regressions verified three answers in the separately managed frame
and two in the direct document. The scroll-time answer mutation still
produced pending manual review. These native regressions use fixed visual
responses. This checkpoint establishes one repaired offline application
journey with the actual local model. Release qualification still requires
independent no-send isolation,
unseen workflow coverage, complete discovery-to-document-to-archive journeys,
and clean Windows installation.
