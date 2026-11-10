# Tailoring existing templates

Checkpoint: 2026-11-10. In Documents, open **Review text**, choose a job and select
**Suggest template changes**. The local model proposes useful original passages
and drafts their replacements through the existing natural-language revision and
evidence checks. Review each suggestion, add the changes you want, then save the
edited document. DOCX edits produce both a working DOCX and a final PDF. The
original remains unchanged.

The planner uses whole, unique single-line passages: at most 80 passages within
4,500 serialized bytes, five selections and one bounded planning call. Counts
show repeated, unsupported and omitted material. The overall planning prompt is
capped at 8,000 bytes. Original DOCX selections are checked with the actual
paragraph/run editor before rewriting. PDF replacement geometry and font
applicability are checked when applying edits. An empty plan is an explicit
review outcome, not proof that the template needs no changes.

Plans and individual suggestions bind the source document, job and profile
context. Changes invalidate stale results. Planning does not authorize edits or
applications; saving still requires the user's reviewed queue. The original
template is candidate-entered material, not independently verified evidence.
Its source label now accurately covers direct and model-proposed selections.
Detailed sources/checks use the same disclosure in single and batch review;
unsupported or unavailable checks open the disclosure automatically.

## Problems exposed and repaired

The real local model inserted `[1]` into replacement text. Template output
validation now refuses newly introduced numeric citation markers and uses the
existing bounded repair attempts. References already present in the selected
original remain allowed. Source references belong in structured metadata; the
application does not silently delete a reference or save a contaminated rewrite.

Visual review also confused a past employer with the hiring employer and
recommended replacing it. Its general instructions now distinguish candidate
history from the advertised role. The bounded second check covers up to three
content observations and four optional suggestions in one request, capped at
1,000 output tokens and 8,000 input bytes. Unsupported factual-change advice can
be rejected; original observations, judgments and reasons remain archived.
Malformed, unavailable and unknown judgments retain the observations for review.
The context version advances to v7 so older reviews become stale.

Four labeled local-model checks distinguish an employer-name difference, an
actual degree contradiction, advice to falsify history and a conditional
portfolio suggestion. All four passed in both final template invocations. This
is a small repeated test, not an independent or universal accuracy estimate.
Optional advice can still be insufficiently conditional or poorly justified;
never use it as a new candidate fact or apply it automatically.

## Measured workflows

Actual production handlers, the compiled client and `gemma4:12b-it-q4_K_M`
completed two fictional template workflows. Each selected and rewrote two
experience passages, saved DOCX/PDF, matched extracted text, archived the rendered
PDF page, retained the original and refused a stale plan after preference changes.
The native check verified identical bytes for every untouched DOCX package part.
Screenshots and geometry checks covered 320, 768 and 1440 px without measured
horizontal overflow or action clipping. The final PDFs and narrow/desktop review
surfaces were directly inspected. The complete letter retained its original
greeting, closing and signature.

| Workflow | Command duration | Minimum sampled free host RAM | Private receipt |
| --- | ---: | ---: | --- |
| Existing CV | 69.985 s | 11.338 GiB, 138 samples | `target/document-journey-run-sr5fysk2/result.private.json` |
| Existing cover letter | 79.894 s | 13.996 GiB, 158 samples | `target/document-journey-run-k10u574r/result.private.json` |
| Separate CV/letter drafting through PDF review | 82.305 s | 11.037 GiB, 163 samples | `target/document-journey-run-xp1rn0v7/result.private.json` |

Each invocation executed one selected native test, with no ignored cases in that
invocation. The third row produced two additional one-page PDFs through the
separate drafting workflow. Measurements sample whole-host physical RAM at
approximately 500 ms; they are not application/GPU peaks or reservations. The
runner uses its owned 4 GiB job and monitored 3 GiB host reserve. Ollama runs
outside that job; the model loaded by qualification was unloaded afterward.

The letter and separate-drafting receipts capture resident identity before and
after work: digest
`6114515d63c17436a7c0417d82820ac65ad643e2806c5a3c89cb62846436ed0b`.
That metadata was added after the CV invocation; its selected production-source
hashes remain in its own receipt. Native results are retained respectively in
`document-journey-native-AmVMPL`, `document-journey-native-J0vUqN` and
`document-journey-native-dLwecZ` under `target/`.

Ordinary workspace checks passed 1,211 tests with 578 opt-in cases ignored across
43 reported targets, including empty documentation targets. All 166 UI scripts,
workspace Clippy, formatting and the managed Windows build passed. Dependency
versions did not change. Early harness selector errors and a test-fixture type
error are retained privately and are not counted as model failures or passes.
Earlier completed workflows exposed the content defects described above; those
completions are not presented as final quality evidence.

## Reproduction and remaining scope

Use `tools/qualify_document_journey.py` with explicit test-owned runtimes as
[documented here](DOCUMENT_PDF_JOURNEY.md), adding `--flow template` or
`--flow template-letter`. The default `--flow documents` checks separate drafting.
All interaction is headless and restricted to a fictional loopback workspace;
form actions and email sending remain refused.

This is passage tailoring, not arbitrary whole-document reconstruction. Text
crossing unsupported DOCX boundaries, ambiguous occurrences, long omitted
sections, scanned PDFs and difficult PDF fonts can require manual work. A rewrite
uses the first affected run's formatting; preservation of every inline emphasis
or original line/page placement is not established. Diverse privately retained
real documents, multilingual quality, ATS behavior, complete correction
propagation and clean-host installation remain open. A newly generated cover
letter still needs improved personal-signature handling; preserving an existing
template signature does not close that separate gap.

No real application, application email or release was created.
