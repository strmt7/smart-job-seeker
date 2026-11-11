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

## Corpus contract

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
all sections or a final review page were reached. Stable identifiers for repeated
controls, complete-form truth, independent value-level review, aggregate held-out
acceptance and repeated model-promotion comparisons remain work in R02/R04.

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
