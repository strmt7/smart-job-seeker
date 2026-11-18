# Job discovery and application tooling recheck — 18 November 2026

Four GitHub searches returned 55 distinct repository leads. The review retrieved
54 primary documentation pages across open-source projects, browser tooling,
candidate services and employer platforms. Fifty-two supplied usable content.
The iCIMS candidate guide returned a loading and login shell, and the LinkedIn
candidate help page returned HTTP 403. LinkedIn's partner documentation was
available separately. The application comparisons describe documented
behavior. Competitor workflow accuracy and completion remain unmeasured.

## Discovery and application workspaces

| Systems | Reusable pattern and limits | Sources |
| --- | --- | --- |
| Career Ops and AI Job Search | Keep a posting's liveness, fit evidence, document revisions and tracker record connected. AI Job Search separates drafting and review and defines portal-specific search skills. Use this workflow structure with the app's local tools and approvals. | [Career Ops](https://github.com/career-ops-hq/career-ops), [AI Job Search](https://github.com/ai-job-search-dev/ai-job-search) |
| JobSpy, SearXNG and curated feeds | JobSpy normalizes concurrent board results. SearXNG exposes pagination and engine selection. Curated feeds show the value of dated original links. The app can use these contracts to expand coverage through permitted sources. Board access follows each source's terms. | [JobSpy](https://github.com/speedyapply/JobSpy), [SearXNG Search API](https://docs.searxng.org/dev/search_api.html), [Simplify internship listings](https://github.com/SimplifyJobs/Summer2027-Internships), [OpenRoles](https://github.com/datascry/openroles) |
| HiringCafe, JobSync, JobNavigator and CareerPulse | Source identity, freshness, duplicate handling and a job's current stage belong in one record. Search, matching, documents and tracker views can share that record across a long-running search. | [HiringCafe](https://hiringcafe.com/about), [JobSync](https://github.com/Gsync/jobsync), [JobNavigator](https://github.com/vesaias/JobNavigator), [CareerPulse](https://github.com/tcpsyn/CareerPulse) |
| ApplyPilot, AIHawk, JobOps and JobPilot | Staged discovery, scoring, material preparation and application work make failures resumable. JobOps retains manual application ownership and JobPilot focuses on the private resume and review workspace. The authoring, provider and site restrictions of each project differ. | [ApplyPilot](https://github.com/Pickle-Pixel/ApplyPilot), [AIHawk](https://github.com/feder-cr/Jobs_Applier_AI_Agent), [JobOps](https://github.com/DaKheera47/job-ops), [JobPilot](https://github.com/jlifeng/JobPilot) |

## Form completion and documents

| Systems | Reusable pattern and limits | Sources |
| --- | --- | --- |
| FormFilla | Separates scored form-to-profile candidates from global assignment. The inspected MIT-licensed assignment permits unmatched fields. Compare it with our existing mappings inside each current form section, with explicit roles for repeated questions and confirmation fields. Evidence from the actual form still governs entry. | [Project](https://github.com/AhmadHassan-BTed/FormFilla), [Pinned assignment](https://github.com/AhmadHassan-BTed/FormFilla/blob/89b0bcb93b4e612ea9496b6a94a043d73c6d1fcd/src/content/matching/assignment.ts), [Pinned license](https://github.com/AhmadHassan-BTed/FormFilla/blob/89b0bcb93b4e612ea9496b6a94a043d73c6d1fcd/LICENSE) |
| OpenJobAutofill, Job Autofill and Application Assistant | Separate a reusable profile from current-page filling. Keep unsupported custom controls and ambiguous entries visible for correction. The extension model offers useful review patterns for the local webpage and its isolated browser. | [OpenJobAutofill](https://github.com/Br1an67/OpenJobAutofill/blob/main/README.en.md), [Job Autofill](https://github.com/kitswas/job-autofill), [Application Assistant](https://github.com/duanyiyao/application-assistant) |
| Playwright, Power Automate and UiPath | Use role and label targets, check actionability, and resolve fallbacks in a defined order. Validate the target's current scope and identity when any fallback succeeds. Visual evidence helps recover a target when the DOM alone is insufficient. | [Playwright locators](https://playwright.dev/docs/locators), [Actionability](https://playwright.dev/docs/actionability), [Power Automate UI elements](https://learn.microsoft.com/en-us/power-automate/desktop-flows/ui-elements), [UiPath descriptors](https://docs.uipath.com/activities/other/latest/ui-automation/advanced-descriptor-configuration) |
| Stagehand, browser-use, Skyvern and BrowserGym | Separate observation, typed extraction, action and evaluation. Generic engines demonstrate useful browser abstractions while benchmark harnesses help compare agents on identical observations and independent truth. Retain our owned Chromium sessions and typed authorization boundary. | [Stagehand](https://github.com/browserbase/stagehand), [browser-use](https://github.com/browser-use/browser-use), [Skyvern](https://github.com/Skyvern-AI/skyvern), [BrowserGym](https://github.com/ServiceNow/BrowserGym) |
| Reactive Resume, OpenResume, Resume Matcher and YAMLResume | Keep source content editable and versioned, compare the tailored document with the requested role, then export a usable PDF. Use parsing and rendered-output fixtures to test this app's existing pipeline. Template fidelity and factual preservation need their own checks. | [Reactive Resume](https://github.com/reactive-resume/reactive-resume), [OpenResume](https://github.com/xitanggg/open-resume), [Resume Matcher](https://github.com/srbhr/Resume-Matcher), [YAMLResume](https://github.com/yamlresume/yamlresume) |

## Commercial workflows and employer interfaces

| Systems | Documented workflow and lesson | Sources |
| --- | --- | --- |
| Simplify, Jobright and Careerflow | Simplify distinguishes page autofill with user review from subscription Autopilot that submits after selection. Jobright's reviewed agent guide and Careerflow's autofill guide put the user's review and submission after preparation. Keep these product states explicit. | [Simplify Copilot](https://help.simplify.jobs/en/articles/2415391-using-copilot-to-autofill-applications), [Simplify Autopilot](https://help.simplify.jobs/en/help/articles/1784339-get-started-with-autopilot), [Jobright](https://jobright.ai/tools/ai-job-apply), [Careerflow](https://www.careerflow.ai/job-autofill) |
| Teal and Huntr | Saved jobs act as working records for follow-ups, application stages and documents. Huntr also clips listings and fills forms using saved profile details. Keep the offer and related artifacts together in our application records. | [Teal tracker guidance](https://help.tealhq.com/en/articles/9530119-job-tracker-guidance), [Huntr extension](https://help.huntr.co/en/articles/9859408-the-huntr-chrome-extension) |
| LoopCV, AIApply, JobCopilot, ApplyHero, LazyApply, Sonara and Massive | These services describe combinations of saved search preferences, queued work, generated materials, autofill and submission. Their public descriptions give useful settings and workflow examples. Site coverage, per-attempt review and error recovery need separate evidence. | [LoopCV](https://www.loopcv.pro/autoapply/), [AIApply settings](https://support.aiapply.co/en/articles/14218613-auto-apply-settings-setting-up-your-profile-and-job-preferences), [JobCopilot](https://jobcopilot.com/), [ApplyHero](https://www.applyhero.ai/), [LazyApply](https://lazyapply.com/), [Sonara](https://www.sonara.ai/), [Massive terms](https://app.usemassive.com/terms) |
| Jobscan | A separate resume scan reports match factors and formatting advice. Keep document check findings distinct from the job-fit score and from any employer's screening outcome. | [Jobscan tutorial](https://www.jobscan.co/jobscan-tutorial) |
| Greenhouse, Lever and Ashby | Public posting metadata can supply requisition identity, intake questions or a hosted apply link where documented. Greenhouse application writes require a board credential. Verify the current employer destination with the app's read-only route check. | [Greenhouse Job Board API](https://docs.greenhouse.io/job-board.html), [Lever Postings API](https://github.com/lever/postings-api), [Ashby public job API](https://developers.ashbyhq.com/docs/public-job-posting-api) |
| Workday, SmartRecruiters, Indeed and LinkedIn | Candidate profiles, account/passcode flows and employer consent are distinct application states. Indeed Apply and LinkedIn Apply Connect document partner integration surfaces. Keep employer-managed transitions and manual handoff visible. | [Workday Candidate Profile](https://community-content.workday.com/content/workday-community/en-us/public/products/talent-management/talent-acquisition/workday-candidate-profile-employer-faq.html?lang=en-us), [SmartRecruiters Application API](https://developers.smartrecruiters.com/docs/application-api-1), [Indeed Apply](https://docs.indeed.com/indeed-apply/), [LinkedIn Apply Connect](https://learn.microsoft.com/en-us/linkedin/talent/apply-connect) |

## Implementation priorities

1. Extend the existing form mapper with a measured candidate-assignment pass
   before adding wider browser tooling. Use repeated work entries, similarly
   named contacts, explicit confirmation fields and unmatched controls in the
   comparison. Admit only source-bound values with current native and visual
   verification. Grade complete reviewed workflows and manual stops alongside
   field accuracy. [FormFilla assignment](https://github.com/AhmadHassan-BTed/FormFilla/blob/89b0bcb93b4e612ea9496b6a94a043d73c6d1fcd/src/content/matching/assignment.ts).
2. Expand discovery with the existing public adapters and general web search.
   Bind a single-vacancy identity to the actual employer page, keep expiry and
   redirect evidence, and use the existing application-route verifier to
   separate an actionable entry point from a career landing page. Keep separate
   provider runs recoverable. The first recent live sample returned 30 links
   and both inspected candidates required review. [Provider recovery evidence](g1/SEARCH_PROVIDER_RECOVERY_2026-11-18.md),
   [Job posting structured data](https://developers.google.com/search/docs/appearance/structured-data/job-posting).
3. Retain the local model and existing document tools. Borrow experiment design
   and fixture shapes from document parsers and browser benchmarks. Measure
   truthful tailoring, parsable final PDF output, visual fidelity and current
   form acceptance as separate checks. [BrowserGym](https://github.com/ServiceNow/BrowserGym),
   [OpenResume](https://github.com/xitanggg/open-resume).

GitHub recorded MIT for FormFilla, Career Ops, AI Job Search, JobSpy, Stagehand,
browser-use and Reactive Resume. It recorded AGPL-3.0 for Skyvern, OpenResume and
ApplyPilot. The BrowserGym and JobOps repository metadata returned
`NOASSERTION`. Any future adoption needs the exact file and dependency
license review. This recheck added no third-party code or runtime dependency.

Native source captures, repository search results, licenses and digests are
retained privately under `target/competitive-discovery-725587d3`. The existing
research register remains the broader lead catalog. The recheck prioritized
actively documented workflows, material engineering overlap and the leading
repositories surfaced by search. [Existing research catalog](waypoint-handoff/research/RESEARCH_ATLAS.md).
