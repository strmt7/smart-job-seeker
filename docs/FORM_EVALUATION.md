# Form evaluation inputs and accounting

The existing `tools/qualify_model_preferences.py` qualifier accepts optional
`--corpus <private-json> --case-id <id>` arguments. It still owns its normal
compiled-app, local-model, cancellation and cleanup journey. The metadata helper
`tools/form_corpus.py` does not launch another runner, send expected answers to
the model or fetch public pages. Without a case binding the report explicitly
says `unclassified` and `scored: false`; successful settings checks are not form
benchmark results. Settings-only mode cannot attach a form case.

Keep real advertisements, source manifests, native receipts, screenshots and
corpus files outside Git. The small bundled `apps/web_app/data/rehearsal-profile.json`
is fictional test input, compiled into the app. Each native rehearsal result now
reports its exact input-byte SHA-256 and any parse failure. This changes neither
live-site permissions nor the requirement for fictional offline entry. It does
not replace the planned generic candidate-aware application planner.

## Schema 2: independent answers and control identity

Schema 1 remains readable with its original label/status-based meaning. New
schema-2 cases add independent expected answers and logical native control
identities. The controller never sends annotations or answer keys to the model.
Do not upgrade old reports by relabeling them or treating their field credit as
value-level correctness.

Schema 2 keeps the original input/profile, family, partition, revision and size
bindings. Each expected field additionally requires:

| Field | Meaning |
| --- | --- |
| `control` | Exact native `origin`, lease-independent `selector` and ordered section-label path; repeated labels do not match by order |
| `required` | Explicit Boolean; an unknown required answer prevents workflow credit |
| `expected` | Literal `value` for text/select/radio, or `sha256` for a file; exactly `null` for unknown answers |

Native saved-page inventories now expose kinds and logical identities without
candidate values. The view retains its full `field_count` and explicit
`omitted_fields` count when the bounded 40-field list is incomplete. It describes
supported native controls; custom-control walls and unseen sections remain
separate. Identity descriptors do not authorize interaction. Temporary frame
leases are removed only by the driver after scope validation, not by a model or
string heuristic in the evaluator.

Successful rehearsal records publish `native_value` and `native_verified` only
following the existing native command/readback checks and final revalidation.
Visual matching alone is insufficient: schema 2 also requires the correct
control, label/kind, exact independently expected answer, native proof and the
actual screenshot's SHA-256. File credit compares the expected digest with the
attachment digest whose bytes the native driver verified; a filename is not
content proof. The absolute owned attempt screenshot directory must be supplied
to `evaluate_case(..., evidence_root=...)`; missing/changed images cannot pass.
The compiled qualifier supplies that directory for its actual result attempt.

Exact value comparison is the default. A text answer may explicitly declare
`expected: {"value":"+12025550123","comparison":"international_telephone_presentation"}`.
This narrow mode preserves the explicit plus and every ASCII digit, allows only
bounded display separators and refuses inferred prefixes or extensions. The
actual native value, intended value and recorded comparison mode must be
consistent; visual success and the same control/image checks are still required.
It is not general text normalization or proof of number assignment. Old corpus
versions keep their original literal expectations. See the
[telephone checkpoint](g1/TELEPHONE_PRESENTATION_2026-11-13.md).

Incorrect values, unverified entries, unknown-answer entry, duplicate controls
and unexpected entries retain separate counts. Every answerable field remains
in the denominator, including controls never reached. A passing field gate
requires all declared answerable values and no critical or ambiguous entry.

A case's `workflow` is either `null` or independently annotated `final_review`
control identity and `required_sections`. Only declared-form truth can request
workflow credit. Actual native final-review evidence, matching image, visited
sections, zero unresolved required controls, no required unknowns and the field
gate are all necessary for `verified_declared_final_review`. This is a scoped
final-review result, never submission or delivery. The general product does not
yet emit this complete workflow proof; metadata-only unit tests of that contract
do not establish real application completion.

## Failed-entry evidence

An eligible recorded target can produce a read-only witness before an action.
If entry fails, the renderer can retain a current diagnostic screenshot and
bounded text value, with before/after checks of document, schema, barriers,
network and value stability. Changed or unavailable evidence is explicitly
unavailable. Diagnostic observations cannot become approval or successful-entry
proof; `entry_unverified` stays unverified, and the original failure is retained.
The witness is unavailable for section-navigation targets and cannot be created
from model-authored identifiers.

## Corpus contract (legacy schema 1)

Version 1 contains `schema: 1`, a stable `corpus_id`, positive integer `revision`
and `cases`. Metadata is capped at 1 MiB, 256 cases and 128 expected fields per
case. Duplicate JSON keys and duplicate case/field identifiers are rejected.
Each case declares:

| Field | Meaning |
| --- | --- |
| `id`, `family` | Stable case and employer/layout-family identities |
| `partition` | `development` or `held_out`, fixed before tuning |
| `source_kind` | `fictional` or `saved_public`; not a source-permission grant |
| `used_for_tuning` | Must be false for a held-out case |
| `input_manifest_sha256` | Exact native replay-manifest identity |
| `profile_sha256` | Exact compiled fictional-profile input identity |
| `coverage_scope` | `initial_native_snapshot` or `declared_form`; an initial inventory is not the complete application |
| `expected_fields` | Stable ID, visible label, native kind (`text`, `select`, `radio`, `file`) and `answerability` (`answerable`, `unknown`) |

A family or the same source input cannot appear across development and held-out
partitions under different IDs. Unknowns are not silently made answerable. Inputs
are checked before the owned journey and again during/following collection;
changing the manifest/profile or frozen corpus invalidates the binding. Source,
model, resident context, runtime and configuration identities remain in the
native/app qualification reports. Expected metadata stays in the controller;
it is not added to visual prompts or browser actions.

## Conservative field accounting

The report retains the expected answerable and unknown denominators, even when
fields are omitted or never reached. Only a unique observed label with matching
native kind and `visually_matched` status receives verified-answerable credit.
Repeated labels/observations remain ambiguous; they are not matched by order.
Unknown or unexpected entry/uncertainty is counted as critical. Refusal explicitly
recorded before entry is distinguished from uncertain or failed after-entry work.

`accounted` means the observations were classified, not that an application passed.
`field_gate_passed` requires a nonzero answerable denominator, every answerable
field verified, and no unsafe/unexpected/ambiguous entry. A case-bound qualifier
fails if this gate is unmet, while retaining observations and completing cleanup.
The controller's outer `complete` flag also covers collection/teardown; neither
flag is a model-promotion or whole-application result. Workflow
completion remains explicitly `not_qualified`; field counts do not prove that
all sections or a final review page were reached. Schema 2 adds control/value comparisons above. Complete-form native proof,
aggregate held-out acceptance and repeated model-promotion comparisons remain
work in R02/R04.

## Initial coverage and next work

The initial private seed records two previously studied **development** cases:
one fictional three-step form and the initial nine-control snapshot of one saved
real form. It contains **zero held-out cases**. The unrenderable saved-source
cases remain exclusions/review observations rather than fabricated form successes.
These are a starting input contract, not the planned 48-scenario/12-saved-page
corpus or evidence of majority-site accuracy.

Before promotion, add independently annotated held-out employer/layout families
and document exact eligibility, unavailable cases, complete-form truth, model/
prompt/runtime identities, repetitions and resource denominators. Preserve prior
failures and corpus versions. See the [active execution plan](COMPLETION_PLAN_2026-11-11.md).
