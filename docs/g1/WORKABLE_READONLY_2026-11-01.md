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
