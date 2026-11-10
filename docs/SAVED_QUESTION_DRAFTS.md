# Preparing answers from saved questions

Checkpoint: 2026-11-10. Open **Application records → Page controls → Prepare
answer** on a shortlisted job, or use the controls in a just-completed page copy.
Review the saved question, then choose **Draft answer**. This starts the existing
local-model answer pipeline with reviewed qualifications and writing preferences.
Opening a question preview does not start inference.

## Behavior

- Question text combines the retained group context, label and description.
  Only labelled text controls without known disabled state or omitted surrounding
  context are eligible. Buttons, search boxes, choices, unlabelled controls and
  known sensitive questions require manual handling. The existing multilingual
  sensitivity markers are review triggers, not a complete semantic classifier.
- The server selects the question from its own typed, bounded source record;
  client-supplied question text or selectors are not used. The saved URL must
  match the current job, the copy must have archived successfully, and the
  outline must carry a valid rendering-checkpoint hash. Older records without
  the persisted source-URL hash require another page copy. New captures persist
  that hash before writing their immutable summary.
- The preparation digest binds the source record, observed control, full stored
  listing-description hash, bounded qualification pool, selected facts and profile.
  Preview/start and every successful status response revalidate the binding.
  Changed inputs fail the draft rather than borrowing the prior citations.
  Wrong-question polling cannot consume another form or email worker's result.
- Generation reuses the existing semantic fact selection, structured drafting,
  style revision and advisory evidence checks. Question text and writing
  instructions that would be shortened by the local context budget are refused
  here. Fact/listing selection remains bounded and can miss relevant material;
  the shared answer-evidence view exposes those limits and individual claim
  judgments. Model agreement does not prove factual support.
- A completed answer is written once to
  `application-records/<job-prefix>-<attempt>/question-draft-<task>.json`.
  It includes the original question, source attempt/control number, context
  digest, answer and evidence, with explicit `filled:false` and
  `submitted:false`. Lost-response retries must match the immutable file.
  Changes to qualifications leave earlier files intact and mark them historical
  when the question is reopened. Original ads and capture summaries are untouched.
- The interface shows readonly answer text, cited qualifications and the same
  evidence-review component used by existing form answers. It supplies no live
  selector, field value, upload action, browser command or approval grant.
  A future form interaction needs its own current inspection and explicit review.

## Boundaries

These are observations from the saved rendering checkpoint, not fresh employer
questions. A matching URL does not establish an unchanged live page. Later steps,
embedded forms, hidden text and actual field-length limits may be missing. Review
the current form before using a draft. This path cannot establish that a full
application is ready, that a sensitive question was classified correctly, or that
any data was filled or delivered.

Records are capped at 256 KiB each. Reopening a question inspects at most 2,000
directory entries and reads at most eight draft files, reporting a partial listing
when the budget is reached. The view retains at most eight archived drafts and
one newly generated result. A single status observer polls for at most ten minutes
and finishes archiving even if its original view has been rerendered; there is no
additional global UI draft cache or persistent development worker. The shared
answer worker retains its existing single-result, ten-minute retry cache.
Cancellation of an in-flight answer model request remains a limitation of that
worker.

## Verification scope

Seven focused native tests cover question composition, context binding, manual
refusals, stale/legacy capture refusal, immutable lost-response replay, historical
drafts and the real loopback preview/start handlers. The ordinary workspace suite
passed **1,235 tests**, with **580 opt-in cases ignored**, across 43 reported test
targets. Workspace Clippy, formatting and the managed Windows build passed.
The new interface check exercises deliberate drafting, bounded polling, the
shared evidence renderer, readonly output and stale-result failure. All 168
`*-ui.test.js` scripts passed; only the changed checks were repeated after the
subsequent presentation refinements.

The contained compiled-app qualification uses explicit fictional saved-page and
answer files, then the production history UI and native context API. A new fact
and reload must mark the answer historical. It checks nine viewport/actual-zoom
combinations and retains three screenshots for direct inspection. Run:

```powershell
python -B tools/qualify_source_copy.py --questions-only --browser '<absolute headless Chromium executable>'
```

This mode uses no model inference, live capture or employer form. It is not an
end-to-end qualification of local-model answer quality on real applications.
The prior model and anonymous-capture measurements retain their separate scope;
live question/vision reliability and complete no-send isolation remain open.

The final compiled UI run passed two connected history/context checks and nine
width/actual-zoom cases in 9.267 seconds. Three retained screenshots were directly
inspected after refining text styling, panel order and shared evidence review.
The 19 whole-host RAM samples had a minimum of 24,368,254,976 available bytes;
these samples are not an application/GPU peak or a long-running leak measurement.
Private evidence is in `target/source-copy-ui-y5wbaeuz/result.private.json` and
`rendered.private.json` (SHA-256
`036e4e3c7f9c0b402fba3e29054c427d51fff9c75c48e97360e15548c8703f49`).
The earlier unrefined run is retained in `target/source-copy-ui-nwhu3n26/`.
