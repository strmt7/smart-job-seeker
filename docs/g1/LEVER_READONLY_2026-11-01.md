# Public application inspection — 2026-11-01

> Research evidence note (2026-11-02): Earlier Crawl4AI captures predate correction of an upstream automation-masking default and browser identity. Their compliance with the no-stealth research configuration is not established; reverify source claims before relying on them as qualifying captures. See [implementation status](../../IMPLEMENTATION_STATUS.md).


The public [Filevine application](https://jobs.lever.co/filevine/05857657-b264-4d4c-916c-40751525d5ea/apply) was inspected read-only. No employer field was filled, file uploaded, control clicked or application submitted. The profile used for mapping contained fictional values and was sent only to the local model.

An isolated Crawl4AI `0.9.5+flitzzip.portable1` run, with robots checking and a 30-second page timeout, returned HTTP 200 and substantive application HTML. Its native capture is retained in ignored `target/lever-readonly-20261101/application.json`, SHA-256 `82dc6b9aa5d53a5ae6a41b02dffe4839c3461e283e5b1cfad7374583e3e53943`. The application's status and contents can change.

Initially, unnamed yes/no radios appeared as separate fields sharing selectors, producing an ambiguity wall and no preview suggestions. The parser now groups otherwise unlabelled native radios only within their owning form, marks those groups sensitive and leaves them for manual handling. Conflicting selectors across forms still produce an ambiguity wall. One subsequent read-only inspection reported the CAPTCHA wall without the earlier selector ambiguity; resource-gated `gemma4:12b-it-qat` suggested four fictional profile values for full name, email, phone and LinkedIn. Those suggestions were advisory: the CAPTCHA wall still blocked filling.

The same page contained three textareas whose visible questions were not programmatically associated with their fields. A bounded, generic nearby-text check now exposes such text as a **possible** question for review; these textareas remain sensitive/manual and are not automatically filled. The last inspection showed the three question texts, left both radio groups and all three textareas manual, and again produced the four fictional profile suggestions. Other fields, including the location control, remained unresolved. These observations do not prove the suggested values are correct in general, identify every hidden control, or establish support for this employer's application flow. The model was unloaded after testing.

The 69 local browser fixtures passed before the final empty-label handling adjustment; the affected radio and textarea fixtures passed afterward. Web and browser Clippy passed. The local fixtures also cover ambiguous nearby text, hidden text and repeated radio names across separate forms.

## Unlabelled radio question context

The parser now looks for one nearby rendered text block around an otherwise
unnamed radio group, within bounded ancestor and control counts. It presents
the result as a possible question and keeps the group sensitive/manual; it
does not assume the visual proximity is a programmatic association. Ambiguous,
hidden or unavailable text leaves the group unlabelled. Fictional browser
checks covered two distinct questions, ambiguous and hidden context, and
repeated names across forms.

On a later read-only inspection of the same public page, the two work-eligibility
and sponsorship prompts appeared as possible questions, with both groups still
manual. The resource-gated local model again suggested the four fictional
profile values, and the CAPTCHA wall remained. No field was filled or control
clicked, and the model was unloaded. This single page does not establish
correct association on other layouts.

## Rendered labels and an unannotated choice hint

Form inspection now reads rendered label text while excluding controls and
hidden descendants. If the label cannot be checked, or multiple labels are
ambiguous, the field is marked for manual review. A separate bounded structural
hint detects an editable input with a hidden companion and nearby list-like
container; it also keeps the field manual. This hint can miss other widget
implementations and can flag unrelated markup. It does not select an option.

In a subsequent read-only inspection of the same page, the location label no
longer included hidden loading and no-result messages, and the location input
was flagged as a possible custom control. The CAPTCHA wall remained; the local
model suggested the same four fictional profile values for other fields. No
employer field was filled or control clicked. The model was unloaded afterward.
The 71 local browser fixtures passed before the final empty-label and duplicate
label refinements; focused label and choice fixtures passed afterward. Browser
and web Clippy passed. A separate fictional preview check confirmed that the
flagged choice did not receive a profile suggestion.
