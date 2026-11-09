# Whole-form career history suggestions

Implemented on 2026-11-09. The form review panel can prepare history answers for
all blank compatible editors in one request. Specific fields and explicit record
selection remain available under an expandable control. This prepares suggestions
for review; it does not navigate, enter a site, authorize filling or submit.

## Question-level planning

The local model classifies each question independently using its inspected label,
section path, type, autocomplete and bounded option metadata. Current and previous
roles may coexist in one container. Repeated headings do not force a shared record.
The classifier receives no saved candidate values. Explicit control precision
constraints distinguish a numeric month from a complete year-month.

The existing deterministic selector then resolves only checked records: a unique
confirmed ongoing entry, a uniquely most recent definitely ended entry, or a single
checked generic entry of the requested category. Overlapping/unknown dates,
multiple current entries, unsupported rankings and absent details remain deferred.
The model selects a component key; the app resolves its exact saved value and
checks the destination type, option uniqueness and other-person restrictions.
It never generates missing career facts or completes partial dates.

A second local-model call checks proposed values against the actual questions and
source period/precision. Country-only questions cannot assume a full location is
the requested country. Duties do not become motivation answers. This is a separate
semantic check by the same model, with correlated errors still possible; it is not
independent verification of experience or a correctness guarantee. Unsupported
verdicts defer that answer. Invalid or duplicate field-reference sets invalidate
that call atomically. A local abstention discards stray components without losing
other valid questions in the batch.

## Bounds, cancellation and review

- At most 200 unique eligible fields, 16 per call, 12,000 prompt bytes and 32 calls.
  Batches shrink to fit classification and answer-check context; oversized fields
  are deferred without silently truncating their meaning.
- The 240-second budget is cooperative between requests. One in-flight request can
  take the transport's 300-second timeout, so this is not a 240-second hard deadline.
  Successful whole-form startup extends the still-current review to ten minutes.
  Existing resource admission and the single local inference lease remain active.
- Progress reports accounted-for fields, suggestions, deferred fields and calls.
  Cancellation signals the active native request and stops further calls. Completed
  checked suggestions may be reviewed; unfinished answers remain explicitly deferred.
  Ending or invalidating the session cancels pending work and discards stale results.
  Reverting profile data cannot resurrect a previously rejected result.
- Blank-field preparation preserves existing answers. Every suggestion starts
  unchecked, shows its saved source entry and needs explicit selection. A select-all
  checkbox is a deliberate review action. Changed editors are preserved atomically.
  Source links flow through the existing checked-answer and one-use fill guards.
- Recoverable results remain private to the bound session. Running progress and
  returned expiry update the panel. No model response authorizes a browser action.

## Qualification

Private evidence is retained under ignored `target/`; every identity and destination
in the interactions is fictional. No real employer form was interacted with.

The local `gemma4:12b-it-q4_K_M` model was exercised through the existing contact
mapper, history planner, reviewed-fill coordinator and visual/DOM readback:

| Fictional case | History proposals | Final entered values | Review outcome |
| --- | ---: | ---: | --- |
| Flat current/previous questions in one container | 8/8 | 10/10 | All ten passed initial visual and live checks; four restricted fields stayed blank. |
| Repeated current/previous/education sections | 8/8 | 10/10 | Seven initial exact visual matches; three casing warnings resolved through existing focused capture plus exact live checks. Four restricted fields stayed blank. |
| Eight held-out mixed questions | 2/2 supported | No browser | Six unsupported questions deferred; no invented answers. |

The two initial planner failures are retained. Native-month questions originally
selected month numbers rather than complete year-month components; explicit
per-control precision metadata fixed the missing suggestions. Another run attached
stray component keys to abstentions and invalidated otherwise valid answers;
per-question abstentions now discard those keys. Full-page casing mismatches
persisted after explicit transcription instructions; focused captures resolved
them without case folding, expected-value prompts or dropping the warnings.
The follow-up qualification exercised the existing focused review path. A
subsequent [visual follow-up checkpoint](VISUAL_FOLLOWUP.md) extends the earlier
single-mismatch retry to a bounded queue in the interface.

Final model evidence: `target/history-pipeline-qNUcJt/` (flat),
`target/history-pipeline-ogdVET/` (grouped), and
`target/history-batch-heldout-cUDmkC/` (mixed questions). The model traces,
fixture/source hashes, original full-page visual results, focused captures and
final denominators remain private there. Earlier failed runs remain in
`history-pipeline-FnuPZU`, `history-batch-heldout-HJBGAV`,
`history-pipeline-a5fT55` and `history-pipeline-dIADSm` under `target/`.
These are small controlled observations, not majority-of-platform or repeatability
qualification.

The final managed Windows web suite passed **743 tests with 399 opt-in checks
ignored**; the later focused history batch suite passed **nine tests**, including the added
split-audit regression (`target/history-batch-split-audit-qualified.log`).
**162 standalone JavaScript scripts passed**. The compiled app passed **53 grouped
UI cases** spanning 320–1920 px and actual 80–400% zoom, with no script errors:
`target/candidate-history-tqjMuG/result.private.json`. The advice responses in that
UI check are intercepted fictional data. The production source/fill/archive test
also passed with whole-form input validation, duplicate-selector rejection and
stale advice that remains failed after profile reversion:
`target/history-answer-sources-GufPT7/`. Clippy, formatting and the debug build
passed. The checked executable SHA-256 is
`e145a90f9b0d4f97f2bb8e125b7291ddcd3ea1e08b0a626a27866a558e8fc677`.


Unit checks cover malformed classifier/auditor references, local abstentions,
semantic rejection, missing date precision, cancellation during either stage,
large questions, adaptive batches, call limits, progress overflow and stale-result
refusal. UI checks cover blank-field filtering, progress/cancellation, explicit
multi-selection, source payloads, edit preservation, five widths and actual browser
zoom. Rendered advice responses are intercepted fictional data, not model evidence.

Receipt SHA-256 values:

- Flat qualification: `b085c5205626db0bbf90546218e579ecc2572fec6d22cc44e5eabd438fac62b8`.
- Grouped qualification: `ba1a8465f20e1aef9ad3db746c2685ae77523ce8cde1940aa2fe6cc5c616a1b2`.
- Mixed-question result: `d5aa12e2602f1a15b24448d5d341a37fb643e1378a73861df06b18a6932acdb0`.
- Production source endpoint: `2e450af86d91ed2e39e395d082352028e86e6c5dbd10e8bbbd9af7b632caf6d1`.
- Rendered UI: `309499e14d1738bcb6383b5df9fdd8ac0c34a7c72fa539a5d5558d373455aaae`.

## Remaining work

This is not a general experience-row assignment engine. Generic repeated rows,
ordinal jobs, highest qualifications, derived answers, ambiguous subjects and values
that do not exactly fit remain manual. It does not add dynamic experience rows or
navigate sections. Large option lists omit their choices from model context and can
need review. Full-page visual transcription can misread capitalization; focused
inspection remains a separate review path rather than automatic agreement or
lowercasing. Broader held-out forms, repeated-run reliability, performance/resource
peaks and difficult real sites still need qualification behind the no-write boundary.
