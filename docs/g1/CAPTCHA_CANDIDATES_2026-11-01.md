# CAPTCHA solver candidate research — 2026-11-01

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
