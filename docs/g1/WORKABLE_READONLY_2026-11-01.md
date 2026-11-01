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

## Staged rendering follow-up

If the first inspection is empty, the bounded retry now checks for one unchanged
nonempty schema instead of stopping as soon as the first control appears. A
fictional browser fixture rendering two fields in separate stages passed, as did
the earlier delayed-form fixture; both retained the CAPTCHA review wall. This
remains a bounded heuristic: fields rendered after the final inspection can be
missed, and an initially nonempty form is not monitored for later additions.
No real employer form was filled or submitted in this check.
