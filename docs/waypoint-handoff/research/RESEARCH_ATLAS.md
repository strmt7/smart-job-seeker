# Waypoint — competitive research and opportunity atlas

Research date: 25 September 2026 • Open-source, public-code and commercial comparisons

## Executive findings

The best available starting comparison is AI Job Hunter, an already broad desktop product with a Rust backend, local-model options, application materials, research and tracking. AARG is a particularly relevant Rust/local evidence-grounding reference. JobSync and CareerPulse remain useful integrated alternatives. There is no basis to present our proposed combination of local AI, job discovery and tailored documents as an invention. [S01 S02 S63 S66]

The expanded survey contains 92 unique repository candidates: 33 annotated entries at varying depths, 37 entries returned by the requested jobseekers topic query, and 31 additional discovery leads, with overlap removed only for the total. The annotated entries include infrastructure/adjacent apps and some lower-depth forks; they are not 33 fully tested products. Ten commercial/adjacent offerings and nine scientific papers are separately recorded. No repository or commercial app was installed or exercised end to end in this survey.

The defensible strategy is a comprehensive workflow with testable quality contracts: coherent evidence across documents and portal answers; correct prepared/submitted/uncertain states; bounded and recoverable external actions; explicit coverage and constraints; and a hardware-qualified private reasoning path. These are competitive hypotheses and implementation targets. Universal novelty and superiority have not been established.

## How to read the evidence

“Documented” means described in inspected first-party documentation. “Code path located” means a specific source path/search excerpt was found, not necessarily audited end to end. “Planned” means a roadmap or explicit not-implemented status. “Unverified” is not the same as “absent.” “Reported failure” is an issue author's observation, not an independently reproduced defect or a population failure rate.

The research used general web discovery, live GitHub searches, direct README/license fetches, targeted repository search, official product/help pages, current model/runtime documentation and scientific abstracts/publication records. Some web pages were cached before the research date. Specific license disputes used live connector fetches. No fabricated star counts, popularity ranking, successful-application rates or performance measurements are supplied.

An attempted auxiliary Firecrawl research run failed with authorization errors and produced no evidence. It is not counted as an independent review. One incorrectly matched arXiv identifier was excluded after resolving to an unrelated audio-visual paper. The retained literature table contains only relevant records.

## What to try or inspect first

For a candidate who wants an existing native product rather than a build project, inspect AI Job Hunter first, then evaluate its actual release and data permissions in an isolated environment. For a self-hosted web workspace compare JobSync and CareerPulse. For grounded Rust document work, inspect AARG. For broad ATS discovery, inspect openroles; its code and dataset have different licenses. These are trial priorities, not security or production-readiness endorsements. [S01 S02 S63 S66 S67]

Automated-applying scripts are not interchangeable with an integrated, reliable platform. They can be source-licensed yet prohibited by a website's policy; they can generate documents but require a cloud coding agent to apply; and they can expose roadmaps near working features. Test exact operations and permissions, not the repository name. [S03 S12 S14 S25–S28]

## Repository atlas

The CSV/JSON matrix preserves every reviewed field, including local inference, delivery model, discovery, materials, submission, tracking, review depth and source IDs. Entries below summarize the decision-relevant results. “Not established” records evidence limits; it is not a negative feature assertion.

### R01 — Gsync/jobsync

Integrated workbench. License: MIT. https://github.com/Gsync/jobsync

Documented scope: discovery — Scheduled Greenhouse/Lever/Ashby; materials — Resume import, matching, cover letters; submission — Not established; tracking — Yes. Local inference: Ollama documented. Delivery: Self-hosted web; PowerShell deployment, not native app.

Assessment: Strong self-hosted alternative; compare with closer native AI Job Hunter first. Caveat: Public postings integration is not evidence of ATS submission; no runtime tests performed. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S01 S62]

### R02 — tcpsyn/CareerPulse

Integrated workbench. License: MIT. https://github.com/tcpsyn/CareerPulse

Documented scope: discovery — Multi-source, maintainer documented; materials — DOCX/PDF preparation documented; submission — Extension autofill documented; actual breadth untested; tracking — Yes. Local inference: Ollama documented. Delivery: Self-hosted web plus browser extension.

Assessment: Broad local workflow candidate to compare. Caveat: Do not treat advertised ATS list as validated current coverage or security certification. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S02 S57]

### R03 — Pickle-Pixel/ApplyPilot

End-to-end agent workflow. License: AGPL-3.0. https://github.com/Pickle-Pixel/ApplyPilot

Documented scope: discovery — JobSpy and career sources; materials — Tailoring and cover letters; submission — Documented automated execution with dry-run option; tracking — Workflow records. Local inference: Preparation supports local endpoints; apply uses Claude Code. Delivery: CLI/Python/Node/Chrome; not native GUI.

Assessment: End-to-end automation reference with a cloud-dependent applying stage. Caveat: Complete documented flow still depends on a proprietary coding assistant; exclude CAPTCHA-bypass features. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S03 S59]

### R04 — jlifeng/JobPilot

Native desktop materials. License: Apache-2.0. https://github.com/jlifeng/JobPilot

Documented scope: discovery — Not established; materials — Resume studio, matching, interviews; submission — Not established; tracking — JD/application tracker on roadmap. Local inference: Cloud providers in quickstart; bundled local inference not established. Delivery: Tauri 2 Windows MSI documented.

Assessment: Native Windows UX/shell reference with roadmap caveats. Caveat: Local files do not imply local AI; avoid counting planned tracker as shipped. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S04 S60]

### R05 — career-ops-hq/career-ops

Coding-agent workflow. License: MIT. https://github.com/career-ops-hq/career-ops

Documented scope: discovery — Source scanning documented; materials — Evaluation, tailored materials and form drafts; submission — Form assistance; universal automated submit not established; tracking — Local workflow dashboard. Local inference: Depends on selected coding CLI/provider. Delivery: CLI/TUI, not native Windows GUI.

Assessment: Useful research/orchestration reference. Caveat: Coding-agent subscription and data egress are separate from code license. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S05 S58]

### R06 — arspesk/sur9e

Coding-agent workbench. License: MIT. https://github.com/arspesk/sur9e

Documented scope: discovery — Posting screening documented; materials — CV tailoring and offer evaluation; submission — Not established; tracking — Yes. Local inference: Depends on chosen coding agent/model. Delivery: Web UI over local toolkit.

Assessment: Additional workflow/UI reference. Caveat: Preserve upstream attribution; brand is not licensed by MIT; audit host/provider. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S06]

### R07 — vesaias/JobNavigator

Integrated workbench. License: MIT label; disclaimer caveat. https://github.com/vesaias/JobNavigator

Documented scope: discovery — Career pages, ATS endpoints, aggregators; materials — PDF CV and cover letters; submission — Capture extension, not established autofill/submit; tracking — Kanban; Gmail documented. Local inference: Ollama and compatible endpoints. Delivery: Docker/web, not native GUI.

Assessment: Evaluate after clarifying license/disclaimer. Caveat: Do not strip frame-protection headers; historical LCA is not role sponsorship; disable risky sources. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S07]

### R08 — amruthpillai/reactive-resume

Resume component. License: MIT. https://github.com/amruthpillai/reactive-resume

Documented scope: discovery — No complete discovery workflow; materials — Template-based resume editor/export; submission — No complete submit workflow; tracking — Not a job tracker. Local inference: Core builder is not dependent on LLM. Delivery: Browser/self-hosted web.

Assessment: Strong materials/editor building-block reference. Caveat: Integrate through a canonical document model; template appearance does not prove parsing accuracy. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S08 S61]

### R09 — xitanggg/open-resume

Resume parser/builder. License: AGPL-3.0. https://github.com/xitanggg/open-resume

Documented scope: discovery — No; materials — Resume builder and parser; submission — No; tracking — No. Local inference: Core browser tools do not require cloud LLM. Delivery: Browser.

Assessment: Useful parser/formatting comparison baseline. Caveat: Do not relabel AGPL code Apache; one parser is not an all-ATS certification. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S09]

### R10 — srbhr/Resume-Matcher

Resume tailoring. License: Apache-2.0. https://github.com/srbhr/Resume-Matcher

Documented scope: discovery — User supplies JD; materials — Master resume, tailoring, cover letter, PDF; submission — No complete submit workflow; tracking — Not a full search tracker. Local inference: Ollama documented. Delivery: Web/Python/Node; Docker/WSL route documented.

Assessment: Good local tailoring comparison baseline. Caveat: Importing DOCX is not equivalent to editable DOCX export fidelity. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S10]

### R11 — speedyapply/JobSpy

Discovery library. License: MIT. https://github.com/speedyapply/JobSpy

Documented scope: discovery — Job board scraping; materials — No; submission — No; tracking — No. Local inference: No LLM required. Delivery: Python library.

Assessment: Optional isolated adapter, not mandatory dependency. Caveat: Reported missing fields/hangs/country gaps; policy and network availability need per-source decisions. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S11 S33 S34 S35]

### R12 — GodsScion/Auto_job_applier_linkedIn

LinkedIn automation. License: MIT, current. https://github.com/GodsScion/Auto_job_applier_linkedIn

Documented scope: discovery — LinkedIn; materials — AI-assisted configuration; submission — Automated LinkedIn applying documented; tracking — Execution records. Local inference: Provider/configuration-dependent. Delivery: Python/browser; Windows scripts.

Assessment: Reference only; not a default production connector. Caveat: LinkedIn policy prohibits this class of automation; old cached license differs from live file. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S12 S28 S56]

### R13 — DaKheera47/job-ops

Integrated source-available workbench. License: AGPL + Commons Clause. https://github.com/DaKheera47/job-ops

Documented scope: discovery — Yes; materials — Yes; submission — README says no auto-apply; tracking — Gmail/status workflow. Local inference: Compatible local endpoints documented. Delivery: Self-hosted web.

Assessment: Include as restricted/public-code competitor; reuse only under its actual terms. Caveat: Commercial restrictions; advertised open-source wording is insufficient. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S13 S23 S24]

### R14 — wodsuz/EasyApplyJobsBot

LinkedIn automation. License: CC BY-NC-SA 4.0. https://github.com/wodsuz/EasyApplyJobsBot

Documented scope: discovery — LinkedIn; materials — Configuration-driven; submission — EasyApply automation; tracking — Limited records. Local inference: Configuration-dependent. Delivery: Python/browser.

Assessment: Include as restricted/public-code competitor; reuse only under its actual terms. Caveat: Noncommercial restriction; paid Apllie offering is not the same as this repository. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S14 S28]

### R15 — Liam-Frost/AutoApply

Source-available workbench. License: PolyForm Noncommercial 1.0.0. https://github.com/Liam-Frost/AutoApply

Documented scope: discovery — Discovery/scoring documented; materials — Evidence-grounded materials documented; submission — Browser fill/click-submit explicitly not_implemented; tracking — Tracking UI; outcome sync incomplete. Local inference: Ollama among providers. Delivery: Web/containers, not native GUI.

Assessment: Include as restricted/public-code competitor; reuse only under its actual terms. Caveat: Many desired ideas already documented; do not mistake roadmap or test-count claims for runtime verification. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S15 S23]

### R16 — feder-cr/AIHawk

Historical/replaced reference. License: Current target not audited for reuse. https://github.com/feder-cr/AIHawk

Documented scope: discovery — Historical claims not accepted; materials — Historical claims not accepted; submission — Historical claims not accepted; tracking — Not established. Local inference: Not applicable to old product claim. Delivery: Not established.

Assessment: Do not recommend old URL as current complete job applier. Caveat: Redirect now identifies invisible_playwright_mcp, a different project; forks require separate audit. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S16]

### R17 — ibarrajo/ApplyPilot

ApplyPilot fork. License: Unverified independently. https://github.com/ibarrajo/ApplyPilot

Documented scope: discovery — Fork pipeline; materials — Tailoring; submission — README describes automation/human review; tracking — Gmail additions documented. Local inference: Local preparation possible; apply dependency remains. Delivery: CLI/browser.

Assessment: Lead for deeper review, not verified recommendation. Caveat: Fork lineage does not prove every copied file has compatible licensing. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S17]

### R18 — alok-sin/applypilot

ApplyPilot-Plus fork. License: Unverified independently. https://github.com/alok-sin/applypilot

Documented scope: discovery — Expanded sources claimed; materials — Tailoring/JSON-PDF alignment; submission — Documented workflow, untested; tracking — Execution workflow. Local inference: Configuration-dependent; coding CLI for apply. Delivery: CLI/browser.

Assessment: Lead for hardening patterns. Caveat: Do not silently remove content merely to fit a page; license and coverage audit outstanding. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S18]

### R19 — yvonnehe772/applypilot

Agent workflow. License: Unverified. https://github.com/yvonnehe772/applypilot

Documented scope: discovery — Lead collection workflows; materials — Profile-driven preparation; submission — Human/blocked paths described; tracking — Workflow state. Local inference: Depends on agent/provider. Delivery: Agent workflow.

Assessment: Discovery-stage lead only. Caveat: Do not conflate same project name with the AGPL ApplyPilot project. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S19]

### R20 — iknalos/ApplyPilot

Profile/browser workflow. License: Unverified. https://github.com/iknalos/ApplyPilot

Documented scope: discovery — Public ATS/job imports; materials — Profile assistance; submission — Supervised browser workflow; tracking — Workflow tracking. Local inference: Claude-based documented workflow. Delivery: Web/bookmarklet/browser.

Assessment: Discovery-stage lead only. Caveat: License unresolved; browser eligibility differs by employer and site. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S20]

### R21 — kaanddemir/applypilot

Tracker/local-browser AI. License: Unverified. https://github.com/kaanddemir/applypilot

Documented scope: discovery — Not established; materials — Resume critique and letters; submission — Not established; tracking — Kanban. Local inference: WebLLM option documented. Delivery: Browser application.

Assessment: Discovery-stage lead only. Caveat: Browser-local model support is not proof of Windows GPU/16 GB workload fit. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S21]

### R22 — andrew-shwetzer/career-ops-plugin

Coding-assistant plugin. License: MIT label. https://github.com/andrew-shwetzer/career-ops-plugin

Documented scope: discovery — Workflow assistance; materials — Research/evaluation/materials skills; submission — Not established; tracking — Host workflow. Local inference: Depends on host/provider. Delivery: Cowork plugin, not standalone app.

Assessment: Useful prompt/workflow reference. Caveat: Proprietary host dependency remains; inspect actual license before reuse. Evidence depth: Documentation/tree/license inspection; not installed or executed. [S22]

### R23 — saeedkolivand/ai-job-hunter-app

Closest integrated desktop comparator. License: Apache-2.0 code; bundled ATS company data CC BY-NC 4.0. https://github.com/saeedkolivand/ai-job-hunter-app

Documented scope: discovery — 24 boards claimed, ATS/aggregators and extension imports; materials — Resume/cover/answers, DOCX/PDF/TXT, evidence stages; submission — Opt-in contact autofill; glossary says never submits; tracking — Lifecycle, IMAP confirmations, status history, saved answers. Local inference: Ollama/local compatible engines plus cloud/CLI. Delivery: Tauri Rust core + React; Windows release documented.

Assessment: Primary baseline and selective licensed reuse candidate. Caveat: Derived Applied badge can reflect generation, separate from lifecycle; reproduce exact revision. No independent runtime/GPU benchmark. Code/data licenses differ. Evidence depth: README + selected glossary/code-search paths + pinned tree; not executed. [S63 S64 S65]

### R24 — joseym/aarg

Rust evidence/tailoring comparator. License: MIT OR Apache-2.0. https://github.com/joseym/aarg

Documented scope: discovery — Posting input; no broad discovery established; materials — Adversarial revision, evidence, Typst ATS/human PDFs, diffs; submission — Not established; tracking — Generation history. Local inference: Ollama/LM Studio end-to-end documented. Delivery: Rust CLI and WASM browser workspace.

Assessment: Reuse or learn from typed evidence and bounded iteration. Caveat: Do not claim Rust, evidence grounding or revision loops as novel; not independently run. Evidence depth: README/product documentation reviewed; not executed. [S66]

### R25 — datascry/openroles

Multi-ATS discovery. License: MIT code; CC BY-SA 4.0 dataset. https://github.com/datascry/openroles

Documented scope: discovery — 51 in README / 52 in About; nightly ATS discovery; materials — Not established; submission — Direct source ATS links; tracking — Browser-local saved/applied/ignored/searches. Local inference: No LLM needed. Delivery: Static browser app.

Assessment: Benchmark discovery/freshness and adapter coverage. Caveat: Count discrepancy and coverage claims are not measured recall. Dataset obligations separate from MIT code. Evidence depth: README/product documentation reviewed; not executed. [S67]

### R26 — nicobrenner/commandjobs

Terminal job search. License: Apache-2.0. https://github.com/nicobrenner/commandjobs

Documented scope: discovery — HN and selected company ATS searches; materials — Not a full materials studio; submission — Not established; tracking — Some tracking/alerts presented as future. Local inference: Cloud GPT in documented workflow. Delivery: Python terminal.

Assessment: Reference terminal discovery; avoid roadmap inflation. Caveat: Local SQLite is not local AI; untested. Evidence depth: README/product documentation reviewed; not executed. [S68]

### R27 — lucasnevespereira/swizjobsbot

Regional alerts. License: MIT. https://github.com/lucasnevespereira/swizjobsbot

Documented scope: discovery — Swiss Google Jobs via SerpApi; materials — Not established; submission — Not established; tracking — Alerts, schedule, retention. Local inference: No local model requirement documented. Delivery: Node/Telegram; French interface.

Assessment: Regional discovery and lifecycle reference. Caveat: External API dependency; not an integrated applying platform. Evidence depth: README/product documentation reviewed; not executed. [S69]

### R28 — AkmalFairuz/job-hunter

Search library and alerts. License: Unresolved. https://github.com/AkmalFairuz/job-hunter

Documented scope: discovery — Public LinkedIn search only; materials — AI filter/one-line overview; submission — Not documented; tracking — Repost awareness, content caching, per-channel dedup. Local inference: OpenAI-compatible bot; local deployment not verified. Delivery: Go CLI + Discord.

Assessment: Reference efficient caching and partial-failure behavior. Caveat: LinkedIn policy/rate limits; no universal coverage or local inference proof. Evidence depth: README/product documentation reviewed; not executed. [S70]

### R29 — Kumneger0/afriApply

Regional integrated autoapply. License: Unresolved. https://github.com/Kumneger0/afriApply

Documented scope: discovery — Afriwork; materials — AI cover letters and profile; submission — Automatic submission without approval documented; tracking — Telegram and application records. Local inference: Cloud API keys documented. Delivery: Bun/Hono web, self-hosted.

Assessment: Functional counterexample to regional end-to-end breadth. Caveat: Unsafe default for our design; self-hosted does not negate cloud/Turso dependencies. Evidence depth: README/product documentation reviewed; not executed. [S71]

### R30 — damonleelcx/Opportunity-Bridge-Agent

Opportunity and public-support agent. License: Unresolved. https://github.com/damonleelcx/Opportunity-Bridge-Agent

Documented scope: discovery — Jobs/training/support paths; sample corpus clearly labelled; materials — Criteria explanations and prepared materials; submission — Hash-bound approval for irreversible actions; tracking — Followups, consent and human fallback. Local inference: Qwen API; offline scripted demo only. Delivery: Go binary + embedded web UI; Windows documented.

Assessment: Counterexample for inclusive workflows and approval semantics. Caveat: Sample data/demo must not be counted as verified real nationwide job coverage. Evidence depth: README/product documentation reviewed; not executed. [S72]

### R31 — KarstenEvans/aletheia-app

Adjacent protocol/UI apps. License: No license selected. https://github.com/KarstenEvans/aletheia-app

Documented scope: discovery — Job discovery not established in inspected README; materials — General evidence-aware tasks; submission — Not established; tracking — General task/reconstruction protocols. Local inference: Host-dependent. Delivery: Portable Markdown/HTML.

Assessment: Topic triage: adjacent, not established job platform. Caveat: Public visibility explicitly does not grant reuse; do not infer job features from topic tag. Evidence depth: README/product documentation reviewed; not executed. [S73]

### R32 — p95max/JobApply

Tracker and inbox workflow. License: Unresolved. https://github.com/p95max/JobApply

Documented scope: discovery — No broad discovery established; materials — Document/import workflow; submission — Auto-apply label may mean tracker proposal acceptance; tracking — Applications/interviews, inbox proposals, Drive backup. Local inference: Deterministic Gmail + optional cloud AI. Delivery: Django web.

Assessment: Counterexample for provenance-aware email tracking. Caveat: Review terminology carefully; public code is not a verified license. Evidence depth: README/product documentation reviewed; not executed. [S74]

### R33 — mareurs/researcher

Supporting Rust infrastructure. License: Unresolved. https://github.com/mareurs/researcher

Documented scope: discovery — General internet research, not jobs specifically; materials — Research synthesis; submission — Not applicable; tracking — Research state. Local inference: Local GPU/compatible API documented. Delivery: Rust CLI/MCP.

Assessment: Potential research-engine reference, not counted as job app. Caveat: Performance claims not independently measured; license/backend review required. Evidence depth: README/product documentation reviewed; not executed. [S75]

## Public code is valuable, but not automatically reusable

The broadened scope retains JobOps, EasyApplyJobsBot and AutoApply because their ideas and workflows matter competitively. Commons Clause and noncommercial terms still constrain reuse. Aletheia explicitly has no selected license. Do not copy unlicensed files or remove conditions because a repository is public. AGPL code can be genuinely open source while imposing obligations; it must not be mislabeled non-open merely for copyleft. [S13–S15 S23 S24 S73]

Separate code licensing from models, datasets, icons, templates and fonts. AI Job Hunter's inspected description distinguishes Apache-2.0 application code from noncommercial ATS company data. openroles describes MIT code and CC BY-SA listings data. Neither mixed artifact should be redistributed under a single blanket Apache label. This is a practical audit finding, not a legal opinion. [S63 S67]

## Commercial and adjacent benchmark floor

Features shown below already exist in official descriptions. None was purchased or tested here. Comparative benchmarks must use the accessible product version/tier and document what was actually exercised. A vendor's advertised scale, user count, accuracy or testimonial is not an independent result.

### C01 — Simplify

Autofill; reusable profile; job-specific AI answers; tracker; tailored resumes; navigation advertises talent agent, interviewer and introductions.

Qualification: New navigation features not walkthrough-tested; supported-site and outcome claims not independently reproduced. [S29 S30 S77]

https://simplify.jobs/copilot

### C02 — Teal

Job capture/tracking; keyword guidance; materials; contacts/referral followups; advertised interview practice and negotiation tools.

Qualification: Do not reduce comparator to a Kanban board; local inference and exact approval semantics unverified. [S31]

https://www.tealhq.com/tools/job-tracker

### C03 — Huntr

Clipping/saved job descriptions; contacts; interviews; tailored CV/cover; autofill; versions and AI edit review.

Qualification: Platform outcome observations do not establish causal product uplift; exact permission/egress tests outstanding. [S32 S78]

https://huntr.co/

### C04 — Jobright

Matching; tailored resume; autofill; insider connections; fresh/verified job claims.

Qualification: Freshness and matching performance are vendor claims; no recall or verification audit conducted. [S79]

https://jobright.ai/

### C05 — LoopCV

Search loops; optional manual review; auto-apply; recruiter outreach; exclusions; application analytics and CV A/B tests.

Qualification: Outcome learning and recruiter outreach are not novel; opens and replies are not proof of hiring impact. [S80]

https://www.loopcv.pro/

### C06 — AIApply

Job discovery/application automation; tailored materials; tracker; resume translation; mock interviews; live interview assistance.

Qualification: Differentiate ethical rehearsal from covert interview help; paid credits/cloud delivery, with no benchmark reproduced. [S81]

https://aiapply.co/

### C07 — Scout

Search criteria/profiles; tailored packets; AI/human delegated applications; visible receipts and status.

Qualification: Receipts already marketed; exact recovery behavior and independent completion rate unknown. [S82]

https://applyscout.app/

### C08 — HireProof

Job/recruitment fraud audits with sources; receipts; alternative listings; integration interfaces.

Qualification: Risk signals cannot guarantee safe/fake verdict; interface openness is not an open-source license. [S83]

https://hireproof.tech/docs

### C09 — RolePatch Proof

Evidence-linked achievements; private imports; selective sharing; verification preview.

Qualification: Some verification flows future-facing; evidence portability alone not new. [S84]

https://rolepatch.com/proof

### C10 — ApplyFlow Radar

Review workspace; fit checks; portal plans; human approval; evidence receipts.

Qualification: Do not assume shared ownership with GitHub career-ops; production maturity and source license unresolved. [S85]

https://career-ops.space/

## Integration limits that change the product design

Public job-posting endpoints are not public candidate submission endpoints. Greenhouse and Lever's documented submission interfaces require employer credentials; Ashby's public postings API does not establish unrestricted submission access. The product must use permitted hosted forms, authorized integrations or a manual handoff rather than inventing universal API access. [S25–S27]

JobSpy issue reports demonstrate useful failure scenarios: an apparent pagination hang, missing direct URLs and country-validation gaps. These reports motivate bounded worker timeouts, partial-result states and source coverage reporting. They do not establish current failure rates or prove every release is affected. [S33–S35]

Local data and local AI are separate. A web app can store everything on localhost while sending prompts to a cloud provider. Conversely, internet research can fetch public pages while model synthesis stays on the GPU. Treat inference, search, enrichment, inbox access, telemetry and submission as separate permission axes. [S03 S63 S64 S71]

## Scientific evidence and what it does not prove

The strongest job-outcome evidence here concerns specific interventions, not this proposed agent. The writing-assistance study is a randomized evaluation of nongenerative resume editing in an online marketplace; it cannot justify claiming that a local LLM auto-applier will produce the same hiring effect. The networking study supports considering relevant weak ties, with nonlinear and industry-dependent effects. Matching and browser benchmarks supply evaluation methods, not personal job guarantees. [S86–S94]

### P01 — Writing assistance improves hiring in a studied marketplace

Management Science, 2025; peer-reviewed journal. Nongenerative writing assistance randomized at large scale improved hiring in the studied setting.

Product implication: Prioritize clear truthful writing and user-approved edits; test our own LLM workflow separately. Boundary: Not evidence that mass autoapply or this platform raises hires by the same amount. [S86]

### P02 — Weak ties and job mobility

Science, 2022; peer-reviewed journal. Randomized network recommendations support a nonlinear role for weaker ties, differing by industry.

Product implication: Build opt-in relationship maintenance and relevant introductions alongside applications. Boundary: Does not justify indiscriminate contact scraping or mass outreach. [S87]

### P03 — ConFit v2

Findings ACL, 2025; peer-reviewed conference findings. Hard-negative training and hypothetical reference representations improve studied resume-job matching tasks.

Product implication: Use hard near-miss examples and contrastive evaluation before expensive reranking. Boundary: Hypothetical training representations are not candidate facts and never enter CV claims. [S88]

### P04 — Multilingual Swiss occupational classification

Findings ACL, 2025; peer-reviewed conference findings. Taxonomy and LLM-refined labels address multilingual occupational classification.

Product implication: Distinguish ad language, required working language and output locale; test CH-ISCO/ISCO categories. Boundary: Classification accuracy does not prove eligibility or hiring success. [S89]

### P05 — Lost in the Middle

TACL, 2024; peer-reviewed journal. Evidence position affected long-context answers in tested models.

Product implication: Evaluate evidence at beginning/middle/end; retrieve compact relevant evidence. Boundary: Do not assert every current model has identical bias. [S90]

### P06 — Distance between relevant pieces

Findings ACL, 2025; peer-reviewed conference findings. Spacing among related evidence pieces is another long-context failure axis.

Product implication: Benchmark multi-document experience synthesis and cross-source contradictions. Boundary: This does not validate a particular production context limit. [S91]

### P07 — WebArena

arXiv 2307.13854; benchmark paper copy reviewed. Functional web task evaluation checks outcomes rather than plausible narratives.

Product implication: Use local ATS fixtures and independently verify the submitted payload/end state. Boundary: Historical success rates are not contemporary job-agent accuracy. [S92]

### P08 — OSWorld

arXiv 2404.07972; benchmark paper copy reviewed. Execution-based computer task testing captures desktop interaction complexity.

Product implication: Include Windows DPI, window focus, browser restarts and uploaded-file correctness. Boundary: The benchmark is not a job-application-specific certification. [S93]

### P09 — InjecAgent

Findings ACL, 2024; peer-reviewed conference findings. Untrusted tool content can manipulate tool-using assistants.

Product implication: Build injection fixtures and isolate reading from authorized writing capabilities. Boundary: Passing a test corpus does not eliminate prompt-injection risk. [S94]

## Gap ledger: the counterexamples matter

This section deliberately records why broad novelty claims were rejected. The narrower target is not automatically unique either; it must be tested against current alternatives.

### GAP01 — Native Windows + local reasoning

Existing evidence: AI Job Hunter already has Windows/Tauri and Ollama; AARG has Rust/local-model workflows. [S63 S66]

Revised opportunity: Not novel. Target a native Rust UI with measured end-to-end AMD 16 GB qualification.

Falsification/validation test: Run matched correctness, resource, accessibility and latency tests; do not assume Rust alone wins.

### GAP02 — Truthful/evidence-backed materials

Existing evidence: AARG, AI Job Hunter, AutoApply and RolePatch already ground material in evidence. [S15 S63 S65 S66 S84]

Revised opportunity: Not novel alone. Investigate graph-wide correction propagation to unsent artifacts and approval invalidation.

Falsification/validation test: Change 100 facts across multi-artifact fixtures; detect all impacted claims, preserve unaffected ones and invalidate the exact approvals.

### GAP03 — Application receipts

Existing evidence: Scout and ApplyFlow document receipts; AI Job Hunter has confirmation tracking. [S63 S82 S85]

Revised opportunity: Not novel. Test receipt-qualified state semantics and reconciliation after uncertain writes.

Falsification/validation test: Crash before click, after click and before local commit; no automatic ambiguous retry and no false confirmed state.

### GAP04 — Already-applied detection

Existing evidence: AI Job Hunter has applied checks and an independent lifecycle; job-hunter documents repost handling. [S64 S70]

Revised opportunity: Generation-based badges are a specific semantics risk to reproduce, not proof tracking is absent.

Falsification/validation test: Compare Prepared versus Confirmed across URL aliases, imports and generation-only workflows at pinned versions.

### GAP05 — Job freshness and aggregation

Existing evidence: openroles already refreshes many ATS sources and exposes stale filtering; Jobright markets fresh verified roles. [S67 S79]

Revised opportunity: Not novel. Target explicit observation evidence and measured source/locale coverage under failure.

Falsification/validation test: Gold-label open/closed/unknown samples; hold out source outages; score stale false positives and missing coverage notices.

### GAP06 — Visa/language/eligibility guidance

Existing evidence: Opportunity Bridge already explains met/unmet/unknown criteria; specialist directories exist. [S72 S76 S89]

Revised opportunity: Not new as a category. Target precise source-linked distinctions without legal verdicts or silent relaxations.

Falsification/validation test: English ad with German requirement; historical sponsorship with unknown role support; failed and missing constraints stay separate.

### GAP07 — Privacy and data ownership

Existing evidence: Local SQLite/keychain, local inference and private proof imports already exist. [S63 S64 S66 S84]

Revised opportunity: Not novel. Target independently tested purpose-level network isolation and coherent deletion/backup policy.

Falsification/validation test: Canary resume/email text must not enter general search queries or telemetry; test every egress path.

### GAP08 — Human approvals

Existing evidence: AutoApply, Opportunity Bridge and assisted products already have human review; hash-bound approval appears in Opportunity Bridge. [S15 S29 S72]

Revised opportunity: Not novel. Require typed, one-use, expiring grants tied to profile/packet/form/destination revisions.

Falsification/validation test: Mutate each bound field; stale/replayed grants cannot cause writes. Confirm approval is enforced in code, not prompt.

### GAP09 — Learning from outcomes

Existing evidence: LoopCV advertises analytics and CV A/B testing; mature trackers have funnels. [S31 S32 S80]

Revised opportunity: Not novel. Target accurate denominators, response lag and honest causal limitations.

Falsification/validation test: Simulated censoring and selection shifts must not become claimed causal gains or calibrated hire probabilities.

### GAP10 — Networking and interviews

Existing evidence: Simplify, Huntr, Teal, Jobright and AIApply already address these stages. [S31 S77 S78 S79 S81 S87]

Revised opportunity: Table stakes. Improve source-backed preparation and consented, relevant outreach rather than claim invention.

Falsification/validation test: Blinded specificity/usability scoring; no invented contacts, fake experience or covert live interview functionality.

### GAP11 — Document quality and revision loops

Existing evidence: AARG exports ATS/human PDFs and iterates; AI Job Hunter has multiple templates and DOCX/PDF. [S63 S66]

Revised opportunity: Not novel. Target explicit layout locks, fact consistency, corrected evidence and export regression gates.

Falsification/validation test: Render representative fonts/locales; verify exact page targets, image aspect ratio and semantic text parity.

### GAP12 — Recovery and efficient orchestration

Existing evidence: General systems, browser frameworks and several workbenches already have queues/logging. [S33 S39 S64 S92 S93 S95]

Revised opportunity: No universal missing-feature claim. Target reproducible bounded-resource workflow reliability as a combined quality property.

Falsification/validation test: Inject hangs/OOM/sleep/resume/cancel; assert queue bounds, visible degradation, preserved drafts and accurate external-action states.

## Research-driven revisions

The implementation plan was changed as evidence and user constraints evolved. The full before/evidence/after ledger is also supplied in JSON and Markdown.

**V1 — Initial platform framing.** Existing workbenches already cover much of this chain. Decision: Use integrated platforms as baseline rather than claiming integration is new. [S01 S02 S05]

**V2 — License and dependency audit.** Commons Clause/noncommercial terms and cloud-dependent applying steps surfaced. Decision: Separate code, data, model, host and platform permissions. [S03 S13 S14 S15 S23 S24]

**V3 — User broadened scope.** User explicitly allowed public code alongside open source. Decision: Include these competitors; keep reuse restrictions, do not equate public visibility with reuse permission. [S13 S14 S15 S73]

**V4 — Topic expansion.** openroles and Opportunity Bridge provide direct counterexamples. Decision: Add topic coverage and narrower end-to-end quality hypotheses. [S67 S72 S76]

**V5 — Closest competitor discovered.** AI Job Hunter already spans Windows, local AI, research, materials, autofill and tracking. Decision: Make it the main comparator; inspect glossary/code before asserting deficiencies. [S63 S64 S65]

**V6 — Rust requirement.** User requires Rust; AARG also shows Rust grounding/iteration prior art. Decision: Select a Rust-native UI spike and typed modular architecture; reuse inference engines explicitly. [S66 S95 S96 S97]

**V7 — Efficiency and model qualification.** Context, browser/display memory, runtime reasoning controls and backend correctness alter the result. Decision: Add aggregate VRAM, zero-offload, context recall and latency qualification with explicit performance targets. [S36 S38 S43 S90 S91 S99 S100]

**V8 — Scientific and commercial countercheck.** Existing commercial products cover them; scientific evidence supports narrow mechanisms, not a universal agent outcome claim. Decision: Prioritize valid applications and active effort; require blinded benchmarks and later a properly powered outcome study. [S77 S78 S80 S81 S82 S86 S87 S88]

**V9 — Final scope/QA revision.** No repo was installed, target GPU benchmarked or production Rust binary built in this session. Decision: Label mock data and all targets; deliver a traceable build specification, not a false completion claim. [S63 S66 S95]

## Coverage limits and remaining verification

This is a substantial bounded survey, not a proof of exhaustive internet coverage. The requested topic query yielded 37 entries, all retained below; GitHub topic membership and ranking can change. Related topics, private products, unindexed projects, new branches and paid features may contain additional relevant capabilities. README-level absence is not absence of a feature.

Before choosing a codebase, the implementation agent must reproduce the main competitors on current Windows releases, inspect actual dependency/data/model licenses, verify permitted source operations and compare on shared fixtures. Claims needing special caution include advertised ATS counts, “verified jobs,” literal source-quote grounding versus semantic truth, “Applied” badges versus actual external submissions, and local GPU compatibility.

The detailed source register includes URLs, access notes and evidence qualifications. It is not a bundle of copied repository source or a replacement for a full dependency/security audit. Every performance budget in the engineering plan is proposed; none has been achieved by a production app in this session.

## Appendix A — Every returned jobseekers-topic entry

Entries not inspected beyond discovery are explicitly labeled. Their names are not used to infer missing features. Exact repository URLs are supplied in topic_jobseekers.csv and JSON.

- career-ops-hq/career-ops — Reviewed; see R05.

- Gsync/jobsync — Reviewed; see R01.

- nicobrenner/commandjobs — Reviewed; see R26.

- Kajaasfaq/Company-list-Chennai — Discovery metadata only; feature/license verification outstanding.

- GabeGiro/EasyApplyJobsBot — Discovery metadata only; feature/license verification outstanding.

- amol-can/eu-visa-sponsoring-companies — Discovery metadata only; feature/license verification outstanding.

- Developer-RONNIE/List-of-Top-Unicorn-Startups-India — Discovery metadata only; feature/license verification outstanding.

- CarlosZBent/Directorio_trabajos_tech — Discovery metadata only; feature/license verification outstanding.

- pavangudiwada/job-resources — Discovery metadata only; feature/license verification outstanding.

- datascry/openroles — Reviewed; see R25.

- Kumneger0/afriApply — Reviewed; see R29.

- Boki136/Jobbing — Discovery metadata only; feature/license verification outstanding.

- xuanthanhtah/Find_Job — Discovery metadata only; feature/license verification outstanding.

- algosup/2023-2024-project-5-flutter-team-6 — Discovery metadata only; feature/license verification outstanding.

- nmaties/jobs-scraper-linkedin — Discovery metadata only; feature/license verification outstanding.

- Noospheracr/JobArmyKnife — Discovery metadata only; feature/license verification outstanding.

- AkmalFairuz/job-hunter — Reviewed; see R28.

- tomigbel/apply-within — Discovery metadata only; feature/license verification outstanding.

- tacitup/Resume-Match — Discovery metadata only; feature/license verification outstanding.

- vaishnaviteja05/CareerUpdates — Discovery metadata only; feature/license verification outstanding.

- lucasnevespereira/swizjobsbot — Reviewed; see R27.

- rajkumar-avachar/JobGrids-job-portal-platform — Discovery metadata only; feature/license verification outstanding.

- bughunter7/bughunter7 — Discovery metadata only; feature/license verification outstanding.

- AndreiBanu1/romania-job-aggregator — Discovery metadata only; feature/license verification outstanding.

- sameersalihabdalla/JobsAgent---jobs-Board-Platform — Discovery metadata only; feature/license verification outstanding.

- jamessnguyenn/EZ-Apply — Discovery metadata only; feature/license verification outstanding.

- faketechlabsberlin/st21-jobzilla — Discovery metadata only; feature/license verification outstanding.

- Blessman-Newton/RemotePathy — Discovery metadata only; feature/license verification outstanding.

- SreecharanV/Visualizing-Current-Data-Science-Salary-Trends-Key-Insights-for-Job-Seekers — Discovery metadata only; feature/license verification outstanding.

- Aimin-Nur/Job-Seeker — Discovery metadata only; feature/license verification outstanding.

- KarstenEvans/aletheia-app — Reviewed; see R31.

- PydevAzmi/pydevazmi.github.io — Discovery metadata only; feature/license verification outstanding.

- damonleelcx/Opportunity-Bridge-Agent — Reviewed; see R30.

- MyTurn25176/MyTurn — Discovery metadata only; feature/license verification outstanding.

- mahinjust/Job-Portal-Application — Discovery metadata only; feature/license verification outstanding.

- Pranav-un/ATSight — Discovery metadata only; feature/license verification outstanding.

- Dixievaseshaped907/job-tracker — Discovery metadata only; feature/license verification outstanding.

## Appendix B — Additional discovery-only leads

These are recorded for subsequent coverage, not recommended as verified products. Their license and current runtime behavior remain unresolved. Exact URLs are in discovery_only_leads.json.

- applypack/applypack — discovery only.

- brockamer/findajob — discovery only.

- KeelDev-tech/keel — discovery only.

- emnl51/bert — discovery only.

- bacobjarton/Simple-Job-Search-Tracker — discovery only.

- MadGapun/PBP — discovery only.

- tkv00/Career-Atelier-AI-Context-Pack — discovery only.

- humancto/mr-jobs — discovery only.

- CryptoJones/OSApplyTrack — discovery only.

- Muatasim-Aswad/job-tracker — discovery only.

- XelWorks/JobGraph — discovery only.

- kaylaehman/jobtrail — discovery only.

- karl-cta/jobctrl — discovery only.

- kirankumarrout/jobsync — discovery only.

- jungrama/job-grep — discovery only.

- TadMSTR/jobsearch-mcp — discovery only.

- StevSant/HireSense — discovery only.

- sametcn99/application-tracker — discovery only.

- VerdantForge/JobTracker — discovery only.

- ergon-automation-labs/portable_job_applications — discovery only.

- itsmeslof/job-application-tracker — discovery only.

- arpit4k/gmail-job-application-tracker — discovery only.

- kamalmaharjan/job-application-helper — discovery only.

- saar324/job-application-system — discovery only.

- Anshul439/job-auto-apply — discovery only.

- imon333/Job-apply-AI-agent — discovery only.

- sharzoy/auto-job-apply — discovery only.

- slothsheepking/jobclaw — discovery only.

- us/linkedIn_auto_jobs_applier_with_AI_fast — discovery only.

- pillow34/aihawk — discovery only.

- kalpthakkar/JobPilot-AI — discovery only.
