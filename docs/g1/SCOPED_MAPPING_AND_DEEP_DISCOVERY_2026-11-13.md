# Scoped model output and deeper discovery — 2026-11-13

This continues `main` from pushed `5b1c986`. The product goal remains active.
All interaction tests used fictional data in zero-capability saved browsers;
public collection was read-only. No application, application email or release
was sent or published. Native source/model artifacts remain ignored and private.

## Deeper bounded resource discovery

The shared discovery ceiling is now eight sequential passes. The existing
90-second, 96-resource, 160-request and 32 MiB aggregate default budgets remain;
a no-progress/empty round stops early. An exhausted smaller resource budget
still refuses the next proposal without fetching it. The range is validated
before any render, and a discovery run cannot be restarted on the same session.

A selected native fictional chain needs six progressively discovered scripts
before its contact field exists. It now finishes after seven passes, with seven
archived resources, eight simulated HTTP requests, one native field and zero
entries. Chromium's AppContainer reports zero capabilities; no live transport
fallback is used. The corresponding unit fixture also verifies that a four-
resource budget stops after four records/five transport requests, counting the
unrecorded refused proposal rather than exceeding the retained-outcome limit.

A fresh permitted capture of the existing development employer source records
34 resources from 42 requests and 2,364,660 response-body bytes, stopping after
six passes as review-required. The older snapshot had 30 resources/37 requests
and reached its four-pass limit while still making progress. The new collector
elapsed observation is 64,637 ms, within its existing budget. Different capture
times and response bytes make this unsuitable for a controlled speed comparison.
Ten native controls and three review walls remain. Header variants, delayed
POSTs and missing/unsupported responses are not ignored or fabricated.

## Diagnose the actual local-model failure

The optional-dialog fixture previously exposed four fields but verified none of
its three answerable contacts. Opt-in native generation observation now runs
before completion and JSON validation. The saved-copy actor retains at most
16 diagnostic records, an 8 KiB UTF-8 response prefix per record and 64 KiB of
serialized records, with explicit omissions, full response digest, completion
reason and selected scalar usage metadata. It does not copy request-prompt
fields, context-token arrays, headers or unknown runtime fields. Model response
text can contain data and belongs in private application records. Diagnostic
prefix truncation is separate from the runtime's completion reason.

The retained native response establishes the cause:

- The first call stopped naturally after 46 evaluated output tokens, with three
  mappings but no required `manual_review` array.
- The smaller review invented selectors starting at `#f04`, continuing beyond
  the offered fields, and exhausted 512 tokens with `done_reason=length`.
- The actor correctly withheld all entries. This was not diagnosed as RAM
  exhaustion, a missing fact or a successful review.

The inference boundary now accepts an explicit bounded object schema. Ordinary
named-schema calls and guarded transport share the same generation implementation.
Existing backends can retain ordinary generation through the trait default;
callers still validate every result. The contact planner constructs enums from
only the offered native references and available contact-profile keys, requires
both mapping/review arrays, and bounds arrays to the actual batch size. Smaller
review repairs use only their supplied references. There are no employer/label
special cases, invented candidate values or additional model authority.

Semantic, sensitive-answer, profile-value, native-control, image and freshness
checks remain independent. Loopback tests verify the actual native request
schema and refusal before connecting for invalid/oversized schemas. Native
incomplete or non-JSON responses are retained diagnostically but still refused.
The bounded diagnostic tests cover UTF-8, omissions, unknown/private runtime
fields and unchanged original native input.

## Measured model and client results

All cases use local `gemma4:12b-it-q4_K_M`, digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`,
context 4096, the existing verified cache and independently frozen answer/control
truth. They are development cases, not untouched held-out tests.

| Input | Verified / answerable | Unknown entered | Incorrect values | Unverified attempts |
| --- | --- | --- | --- | --- |
| Fictional optional dialog/contact form | **3/3**, previously 0/3 | **0/1** | **0** | **0** |
| Earlier frozen French contact/choice form | **6/7**, unchanged | **0/2** | **0** | **0** |
| Earlier saved employer form | **4/6**, unchanged | **0/3** | **0** | **1**, unchanged |

The first revised mapping is a natural-stop 54-token response with the review
array and all three contact mappings; no repair call is needed. Each performed
contact passes native value/control and local visual checks. Required salary
stays blank. Its optional dialog still requires native closure, blind visual
closure and fresh native evidence. It is not a whole-workflow pass.

The French residence radio remains withheld. The saved-employer résumé attempt
still fails visual observation and prevents further entry; it remains unverified
and counts as critical uncertainty. No contact-format improvement masks that
failure, the unsupported/custom controls or missing server validation.

The compiled client renders the improved three-field result at 320, 480, 900 and
1360 px without page errors or horizontal overflow. It verifies the retained
closure image digest and updated-view navigation; wide/narrow screenshots were
inspected. This does not qualify every menu, zoom level or unrelated workflow.

Workspace: **1,350 passed, 601 opt-in ignored**, zero failures. Clippy, formatting
and the Windows app build pass. The selected deep-browser test is additional
explicit evidence, not credit for the ignored set. No dependency graph, source
permission, transport header or detector suppression changed.

## Resource scope and retained evidence

Heavy work was serial: build/native jobs at 6 GiB; model controllers at 12 GiB
with 10 GiB app jobs and the 3 GiB host reserve. The three final model cases
sampled at least 19,210,596,352 available host RAM bytes; the largest aggregate
app-job committed-memory peak was 9,369,276,416 bytes. These are different
measurements, not exclusive physical application peaks.

Sixty-nine whole-adapter WDDM observations while the owned model identity was
live ranged from 14,626,267,136 to 15,157,600,256 used bytes of 16,977,985,536.
They are not process attribution or a continuous peak. Runtime residency fields
remain separate and do not establish a general 16 GiB deployment envelope.

One next-case launch was deferred before creating a child by shared-policy RAM
admission. The completed model/app trees were drained; an ownership audit found
no matching survivors. Fresh admission then allowed the saved-employer case to
pass at the original 12 GiB ceiling. No memory policy was weakened and no
unrelated application was stopped. The final audit reports zero matching
processes, resident models, services, tasks, startup entries or errors, with
33,280,212,992 available RAM bytes. These are scoped teardown snapshots, not
sustained leak proof or memory reservations.

Command logs/controllers are `target/replay-gap-repair-msc2fick/`. Native deep
proof is `target/capture-replay-qualification-k7t7w828/`; fresh source collection
is `target/deep-source-capture-adatg7qs/`. The pre-fix native model evidence is
`target/dialog-model-pyv1nkir/`; final cases are `target/dialog-model-c06adns_/`,
`target/semantic-profile-model-9nprrjig/` and
`target/semantic-docs-model-znabuuez/`. Rendered evidence is
`target/dialog-review-ui-ackl1e6r/`. The audit is
`target/development-host-3456b47c72734fdcac43b8c6acc25405.private.json`.

The built app digest is
`3ec50768f550526e6fc20633b0b632de3772b73c45bbd4abee561f28e0a9cfa4`.
Final model source digests still match current runtime sources. The prior pushed
`5b1c986` has successful CI, coverage and CodeQL; those hosted checks do not attest
to this newer local batch.

R03 still needs faithful multiple request representations, visible requiredness,
complicated panels and broader source eligibility. R02 still needs untouched
family/workflow evaluation, repeats for promotion, the withheld radio and
résumé-vision repair. The app remains a prototype; no universal accuracy,
workflow completion, release-readiness or submission guarantee is claimed.
