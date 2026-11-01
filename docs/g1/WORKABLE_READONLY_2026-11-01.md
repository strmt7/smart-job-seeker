# Public application inspection — 2026-11-01

This is a limited, read-only examination of three real application pages. No
application was submitted, no form field was filled, no button was clicked, and
no file was uploaded. The candidate profile used for model advice was entirely
fictional and was sent only to the local model. Results do not establish how
the system behaves when a candidate proceeds through these applications.

The pages were discovered publicly and checked against `robots.txt`. An
isolated Crawl4AI `0.9.5+flitzzip.portable1` environment captured the pages
with robots checking and a 30-second page timeout. The resulting public-page
receipts remain in ignored local `target/workable-research/` files:

| Page | HTTP | Native receipt SHA-256 |
| --- | --- | --- |
| [HelloGov application](https://apply.workable.com/hellogov/j/1C4622FA90/apply/) | 200 | `a88327fcff042ad2c630cbba10d1f35d8aa3d6a84140043e284ae82810738209` |
| [GoGlobal application](https://apply.workable.com/goglobal/j/4905C591AD/apply/) | 200 | `ea22857171b6a7d48a2bc7c15f1abe397f7c3fc80af82b696a319031156d9eb4` |
| [Huzzle application](https://apply.workable.com/huzzle/j/C10A5DE374/apply/) | 200 | `d7ec884b51a5e8e0d1c24e1183e1366f11bc8f3bcec02717465bfc268d428871` |

Separately, Waypoint's browser driver opened each page without executing form
commands. Its initial inspection found zero fields on each page; a second
inspection three seconds later found respectively 5, 12 and 17 fields. Each
page produced a `captcha present` review flag, alongside custom-control and
required-field review flags. The captured HTML included a hidden Turnstile
container; this observation does not establish whether an interactive challenge
was active. The normal preview returned no fill suggestions because the
CAPTCHA-related barrier remained in place.

For diagnostic purposes only, the detected schemas were separately given to
Waypoint's mapping routine with fictional profile values and the local
`gemma4:12b-it-qat` model identified in
`LOCAL_VISION_12B_2026-11-01.md`. This read-only advisory call did not remove
the barrier or authorize any action. It returned respectively 1, 1 and 3
candidate mappings, with unresolved-field warnings. A screenshot transcription
returned respectively 0, 0 and 2 observations. No automated correctness score
was assigned to these suggestions or observations.

The 12B model was resource-gated, used a 4,096-token context and was unloaded
after the evaluation. A CAPTCHA or other unresolved wall still requires manual
review. These three observations are not a representative sample and are not
evidence that the application can fill or navigate Workable forms. Dynamic
loading, omitted fields, visual coverage and final-step handling need further
evaluation without submitting or bypassing challenges.

## Delayed form loading follow-up

Waypoint's driver was changed to reinspect an initially empty page up to three
times, starting retries within a three-second window, without clicking or entering values.
Review walls observed earlier in that inspection are retained. A fictional
delayed-form browser fixture passed and retained its CAPTCHA review flag.

One read-only reinspection of the same three public pages, with the same 12B
model and fictional profile, found 5, 12 and 17 fields on the initial `open`
result respectively; previously that result contained zero fields on all three.
Each still reported `captcha present`, and the normal preview still made no
fill suggestions. Separate read-only advisory mapping produced 1, 1 and 3
suggestions; screenshot transcription produced 0, 0 and 2 observations.
No field was filled, no control clicked and no application submitted. The model
was unloaded. This small comparison does not establish reliable loading, field
coverage, visual accuracy, or support for these sites' application flows.

## Advisory mappings on CAPTCHA-marked pages

When an inspected page has fields and its only blocking wall is a detected
CAPTCHA integration, the form preview can now display model or exact-match
suggestions as advisory information. It still requires a fresh unblocked
inspection before filling. The browser UI hides its fill control for this
preview, and the backend rejects a fill against the old preview even if the
wall later disappears. A fictional offline test covered both suggestion paths,
the unchanged and disappeared wall, and an additional login wall. UI helper
tests covered the fill control. In one subsequent read-only visit to the
GoGlobal Workable page above, the resource-gated `gemma4:12b-it-qat` model
received a fictional profile and returned **zero suggestions** for 12 detected
fields; the CAPTCHA and custom-control review flags remained. The preview was
blocked from filling both with the existing wall and if it later disappeared.
No field was filled or clicked, and the model was unloaded. This does not
solve a CAPTCHA or establish useful mapping coverage on that page.

The initial profile supplied only a name and email; the 12 detected fields did
not include name or email inputs. The zero-suggestion result therefore did not
establish a model failure. With a fuller fictional profile containing phone,
address and a profile link, two subsequent read-only runs each produced three
suggestions for the corresponding phone, address and profile-link fields.
The second run checked the suggested values against their field labels. These
are results of the model-assisted pipeline, which also includes deterministic
exact matching, not an isolated measurement of model contribution. The CAPTCHA
wall remained, no fields were filled or clicked, and the model was unloaded.
Two runs on one page do not establish general mapping accuracy.

## Hidden proxy follow-up

The earlier 12-field schema included a required `aria-hidden` text input behind
the visible Job location combobox. The browser reader now excludes controls
under `aria-hidden` from ordinary suggestions and reports a directly marked,
required hidden control for manual review. The visible combobox remains a
separate custom-control review item. A fictional browser fixture and all 63
browser integration fixtures passed; browser and web Clippy checks passed.

One subsequent read-only GoGlobal check found 11 ordinary fields, reported both
the Job location custom control and the required hidden control, and retained
the three expected fictional contact suggestions. No field was filled or
clicked and the local model was unloaded. The combobox is not automatically
filled by this change; hidden controls nested under an `aria-hidden` ancestor
are excluded, but only a required control directly marked `aria-hidden` gets
the specific hidden-required warning.

## Custom choice inventory

The form review panel can request a read-only inventory of visible ARIA
comboboxes and currently rendered options. It does not open the control or
select an answer. Lists may be virtualized, clipped or incomplete, and embedded
scopes are reported as incomplete. A fictional browser and local API fixture
covered an open list, a closed list, hidden and disabled options, and duplicate
list identifiers. A rendered panel check used fictional data.

A read-only revisit of the GoGlobal page found its Job location control
collapsed; no options were available to inspect without interaction. No
control was clicked and no field was filled. Automatic opening, option choice,
and verified retention are not implemented by this inventory.

The browser crate now also has separate primitives to open a closed ARIA
combobox and choose one unique rendered option. A fictional fixture verified
opening, displayed-value retention, `aria-owns`, duplicate labels, a control
changing to submit on hover, and a selection that was not retained. Direct
submit-style controls were refused. The full 65-fixture browser suite passed
before the `aria-owns` fallback refinement; its focused fixture and Clippy
passed afterward. These primitives are not connected to the web workflow,
approval recording, local-model visual review or employer pages. A successful
display check is not proof of server-side retention, and a site can run its own
handlers during any interaction.

The review panel can now request a local-model visual check of a custom-choice
label. The task is bound to the observed URL, form schema, review walls,
choice inventory and screenshot; a changed snapshot or image is reported as
stale. The check is advisory and does not authorize or click the control.
On a fictional local fixture, resource-gated `gemma4:12b-it-qat` read the Job
location label and the full start-and-poll flow passed. A separate fixture
changed the label after capture and the result became stale. Screenshot and
result recording report failure when no disk-backed attempt record is present.
The model was unloaded afterward. This single fixture does not establish
visual reliability on real employer pages or support for automatic selection.

## Staged rendering follow-up

If the first inspection is empty, the bounded retry now checks for one unchanged
nonempty schema instead of stopping as soon as the first control appears. A
fictional browser fixture rendering two fields in separate stages passed, as did
the earlier delayed-form fixture; both retained the CAPTCHA review wall. This
remains a bounded heuristic: fields rendered after the final inspection can be
missed, and an initially nonempty form is not monitored for later additions.
No real employer form was filled or submitted in this check.

## Reviewed custom-choice opening

The web review flow can open a closed custom choice after a saved, current
local-vision review and a separate user gesture. It records the action and
capture, consumes the review once, and requires a fresh visual review of the
opened choice before further filling. The opening runs on a bounded background
worker; an uncertain result remains in manual review. A fictional local
browser fixture exercised the approval, recording, replay and follow-up gate.
Its visual results were simulated for this test. This does not establish safe
behavior on arbitrary employer pages: page scripts can have side effects, and
no real employer control was opened or application submitted in this check.

## Visible option transcription

An expanded choice can be checked for one uniquely rendered option using the
local vision model. The full viewport is captured because a field-context crop
on the fictional fixture omitted the list: `gemma4:12b-it-qat` returned only
the control text from that crop. On the same fixture, a viewport capture let
the model transcribe `Berlin` among the visible options. This was a single
local-model probe, not a measurement of general accuracy; the application
pipeline and its one-use separation were checked with simulated vision results.
Option checks alone do not select or authorize an option. A changed snapshot,
unreadable option, duplicate label or unavailable evidence requires review.
The model was unloaded after the probe.

Option review now captures the uniquely identified rendered option itself,
instead of the whole viewport used in that earlier probe. This avoids the
model's eight-item transcription limit and reduces unrelated page content in
the image. The browser refuses missing or duplicate options and captures only
within bounded visible viewport dimensions; clipped or unreadable options
still need review. In a fictional 14-option list, `gemma4:12b-it-qat` read
the tenth option, `Sydney`, from its capture. This single result does not
establish accuracy across layouts or sites. A separate fictional end-to-end
API run used the same local model for both option observation and the later
selected-value check; the earlier field-label observation was simulated. No
application was submitted, and the model was unloaded after the checks.

## Advice for a visible custom option

The review panel can ask a local model to suggest an option for an answer
typed specifically for that question. Suggestions are limited to currently
rendered options and must include an exact excerpt from the supplied answer;
duplicates, invalid indices and unsupported excerpts are rejected. This is
advice only: it does not select an option or authorize an action, and a
matching excerpt does not prove that the model interpreted the answer
correctly. The answer is not added to the candidate profile or attempt record.
The page snapshot is rechecked before advice is returned.

`gemma4:12b-it-qat` suggested Berlin for one fictional answer and abstained
for one undecided answer. A separate fictional browser/API run exercised the
same model-backed endpoint and rejected simulated stale advice. The model was
unloaded afterward. These few cases do not establish general accuracy; other
options may be absent from a virtualized list, and the user must review the
suggestion and visual evidence.

On a further fictional probe, `gemma4:12b-it-qat` chose Lisbon for an answer
rejecting Berlin, abstained when two locations were acceptable or sponsorship
was unknown, and ignored an instruction-like option in one test each. The
interface now displays the entire entered answer alongside the suggestion, so
a short model-selected quotation cannot hide a qualification or negation. A
button can start the existing visual check for the suggested option; it does
not select it. These observations do not guarantee correct behavior on other
questions. The model was unloaded after the probe.

## Reviewed option selection

The web flow can select a uniquely rendered option after a saved, current
option visual check and an explicit destination and option review. Its one-use
approval binds the option and snapshot, the background action records its
outcome, and further filling stays blocked until the displayed value receives
a separate saved visual check. The selected-value check can be resumed from
the choice inventory after reloading the interface. On a fictional local form,
the complete API workflow selected `Berlin`, rejected reuse of the evidence,
and completed the displayed-value check with resource-gated
`gemma4:12b-it-qat`; the earlier option observation in that workflow was
simulated. A separate model probe transcribed both the visible option and the
displayed value. Neither test establishes reliability across employer sites,
and site scripts can still have side effects. No employer form was operated
or application submitted in these tests. The model was unloaded afterward.

## Rechecking earlier custom choices

Visually confirmed selections are tracked in the active form session and
rechecked before later filling, attachments, choice actions and navigation.
They are also checked after filling, attaching or another choice action. The
check uses one bounded control inventory for up to 30 tracked choices. In the
fictional form, changing a confirmed value from `Berlin` to `Lisbon` caused
the next fill request to be refused. Navigation to another section warns that
previous selections can no longer be rechecked there; the tracking does not
survive closing the browser session or prove that an employer retained values.

## Resuming a pending choice review

Rescanning an open form now carries any pending choice visual-review gate into
the new inspection. The choice inventory retains the selected value awaiting
review, and previously filled fields are flagged for fresh review because their
old verification is no longer associated with the new inspection. The web
interface can recover an earlier selected-value vision task or report it as
unavailable before starting a new check. A fictional browser and workspace
fixture exercised selection, rescan, the retained gate and subsequent review;
the vision result in that test was simulated. No employer site was used.
