# CAPTCHA solver candidate research — 2026-11-01

> Research evidence note (2026-11-02): Earlier Crawl4AI captures predate correction of an upstream automation-masking default and browser identity. Their compliance with the no-stealth research configuration is not established; reverify source claims before relying on them as qualifying captures. See [implementation status](../../IMPLEMENTATION_STATUS.md).


This is a source review, not an efficacy benchmark. No solver was run against
an employer page and no application was submitted. Popularity figures and
repository state are snapshots from GitHub's API, not evidence of reliability.

| Candidate | Snapshot | Practical observations |
| --- | --- | --- |
| [cfsolve](https://github.com/chris124567/cfsolve), `d1de858bf7b7f60c89fcc156031dfa9e5e8977b8` | MIT; 468 stars; created 2026-10-26; pushed 2026-10-31 | Rust, request-based Turnstile solver. The published manifest includes a large SWC dependency graph and three local crate patches. Its README describes captured browser profiles and saved request/response debugging files. Six days of repository history is insufficient evidence of mature maintenance or compatibility with Waypoint's live browser session. |
| [PyPasser](https://github.com/xHossein/PyPasser), `910c3def3d69e68d42d86ccba5a1b4cf105ee1ed` | MIT; 618 stars; created 2021-08-21; pushed 2026-10-24 | Claims multiple local solvers; its Turnstile path uses browser automation. Requires Python and, for browser features, additional browser tooling. Integration, resource use and current effectiveness remain unmeasured. |
| [playwright-captcha](https://github.com/techinz/playwright-captcha) | Apache-2.0; 360 stars; pushed 2026-06-12 | Offers browser click-based and external API solvers. The click-based path may be relevant, but its Playwright runtime and interaction with Waypoint's existing CDP browser remain untested. Paid API paths do not meet the user's free-tool requirement. |
| [NopeCHA extension](https://github.com/NopeCHALLC/nopecha-extension) | MIT; 11,084 stars; pushed 2026-10-02 | The extension has substantial adoption and Turnstile support, but its README says recognition uses a hosted API with a free request limit. It is not a self-contained local solver and would add an external service. |
| [EzSolver](https://github.com/ismoiloffS/EzSolver), `5eefb0043825389d7b93664295803da95328a79b` | MIT; 150 stars; pushed 2026-10-29 | Uses Python and a persistent Chrome browser. Its service binds all interfaces by default; its integration and effectiveness remain unmeasured. |

The MIT or Apache license of a top-level repository does not establish the
licenses of its complete transitive dependency graph. No candidate has yet been
admitted to the product, and none has been shown to solve an actual Workable
challenge in Waypoint's existing browser session. Future evaluation must measure
that session compatibility, challenge outcomes and resource use, preserve license
notices, and prevent a challenge interaction from submitting an application.

## Research receipts

GitHub pages were read using isolated Crawl4AI `0.9.5+flitzzip.portable1`,
with robots checks and a 30-second page timeout. Native receipts are retained
locally under ignored `target/captcha-research/`; SHA-256 digests:

- cfsolve: `812065e3dbee78651ae7d27fb1af30d75cd619c823dc60d414e249f9b598814f`
- PyPasser: `010d0da29612125dfd2624af5bb820e2ddec3f426a13465a6770b92cfd9d7e14`
- playwright-captcha: `028e022e4dd43643b3b2db02a33940a28c9c2b986b3b778a534f04fccd87d824`
- NopeCHA: `1fd9c30a7ce8dc01c34ad5a20e91973c6529d93fffab1fa68bc94b8f89891f0b`
- EzSolver: `75f3cd173a64ee9e3109752617b88665f42bc43a81b8f4e8de9f6de3a59d0e5d`

All five page captures reported HTTP 200 and substantive extracted content.
GitHub metadata and manifest checks used the native GitHub API.

## Existing-browser compatibility probe

On Windows, an isolated test launched Waypoint's `BrowserProcess` and attached
PyPasser `910c3def3d69e68d42d86ccba5a1b4cf105ee1ed` to its loopback CDP port
using the existing Playwright runtime. A fictional page contained a local
checkbox iframe which wrote a fictional response token when clicked. The
candidate reported success and the expected fictional token; the fixture's
submit handler was not triggered. This establishes basic attachment and
checkbox interaction only: it is not a real CAPTCHA, does not measure a real
provider's acceptance, and does not prove that interacting with a third-party
widget cannot trigger an application action.

The source archive SHA-256 was
`a11b655d4725967a85e7190d79d66e58b280da4cc02351e6fcce9d5a6a63d0d0`.
The temporary harness and pinned archive remain under ignored
`target/captcha-research/pypasser-test/`; no candidate code is shipped. The
adapter's result includes session cookies and user agent, and it may fall back
to synthetic click dispatch. A product integration must account for these
behaviors, enforce the existing application submission boundary and test its
actual effects. A local vision model may assist with image challenges and
inspection, but its effectiveness and resource use for this task are unmeasured.

## Local vision feasibility probe

On the same date, the app's Windows resource gate started the already-installed
`gemma4:12b-it-qat` at a 4,096-token context and verified full GPU residency.
In one run each, two generated 3-by-3 grids of solid circles and squares were
sent to the model through loopback Ollama. The model returned the exact circle
indices for both grids (`[1,4,8]` and `[2,5,9]`) and stopped normally. The model
was then unloaded. These synthetic, high-contrast diagrams are deliberately
simple; this is not evidence of performance on photographs, real provider
challenges, ambiguous tiles, localized instructions, or interactive puzzles.
Fixture images, prompts and responses remain under ignored
`target/captcha-research/vision-fixtures/` and `vision_probe.py`.

## Interaction behavior research

[Cloudflare's detection overview](https://developers.cloudflare.com/bots/concepts/bot-detection-engines/)
describes a combination of request, session and browser signals. The
[FP-Agent study](https://arxiv.org/html/2605.01247) reports measurable
differences in typing, scrolling and mouse behavior for seven tested agents;
its results are limited to the tested tasks and configurations. Neither source
establishes that adding random delays or interpolated pointer paths makes an
agent indistinguishable from a person. Waypoint previously inserted text in
whole strings and moved directly to click coordinates. Any change needs
to preserve exact text entry, verification and the user's no-submission
boundary while measuring added time.

The browser driver now emits individual keyboard events for nonempty printable
ASCII values of at most 64 bytes, using US keyboard metadata. It rechecks focus
between events and verifies the retained value; other text continues to use
the existing Unicode-aware insertion path. A fictional fixture verified the
emitted keydown events, exact retained value and stopping when a page redirects
focus. One run entered 37 characters in approximately 252 ms on this host,
including browser protocol calls and verification; no baseline or cross-host
performance claim follows from this single observation. All 59 browser
fixtures passed with the earlier alphanumeric-only implementation. After
extending it to punctuation, focused keyboard, retention and attachment
fixtures, a printable-character unit test and browser-crate Clippy passed.
This is not evidence of
human-equivalent behavior or successful avoidance of bot detection.

Before changing pointer trajectories, the shared click path was updated to
inspect its target after hover and reject a hidden, disabled, covered or moved
element. A local fixture checks both a submit control appearing over the
target on hover and a pre-existing overlay over an asymmetrically padded
button; neither was clicked. All 60 browser fixtures passed with the first
version of this check. After aligning the click and hit test to the border
center, the overlay, nested-frame and nested-shadow fixtures and browser-crate
Clippy passed. A page can still change between inspection and click, and this
check alone does not prove that every frame ancestor or site-side action is
safe.

A subsequent local fixture exposed a parent-page submit control covering an
embedded input: the earlier check allowed the click. The driver now follows
accessible frame and shadow-root ancestry and checks each hit target; it
refuses frames it cannot inspect or whose CSS transform cannot be safely
mapped. The fixture verifies that an uncovered embedded input remains
clickable, then that an overlay prevents the click without submitting the
fixture. The overlay and existing nested-frame and shadow fixtures and
browser-crate Clippy passed. This is not proof against all coordinate mappings,
cross-origin pages, or changes between inspection and the actual click.

Subsequent clicks now move from the last known pointer position to the next
target in up to five bounded curved steps, without artificial sleeps. The
first click still moves directly because the prior pointer position is not
observable. One fictional two-button fixture reported about 77 ms for the
second movement and click; this is a single-host observation without a
before-and-after comparison. All 62 browser fixtures and browser-crate Clippy
passed. This does not establish human-equivalent behavior or evasion of bot
detection; pointer movement can itself trigger page hover handlers.

Crawl4AI receipts, both HTTP 200 with substantive extracted content:

- Cloudflare: `c48e6a5619f498512f7109bd54402062430aa59c8cba071a605301ece065c513`
- FP-Agent: `e48aa0c529a1708ba82ae2503c5c01688841816cb3382795183c3795a8e180ec`
