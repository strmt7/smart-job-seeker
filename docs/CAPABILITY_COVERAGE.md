# Smart Jobseeker capability coverage

Updated 2026-11-09. This is an implementation and evaluation map, not a claim
that the prototype already supplies every capability below. It supplements the
handoff and [completion plan](COMPLETION_PLAN.md); their invariants still apply.
Implemented controls, demonstrated behavior and broadly qualified behavior are
different states. A feature needs an end-to-end evidence record before it is
called ready. Never infer completion from a menu item or dependency.

## Research baseline

The first comparison covers five public first-party sources. This is a useful
starting sample, not an exhaustive survey of every product or an independent
evaluation of their quality. Product pages describe advertised capabilities.
No third-party implementation or design asset was copied for this comparison.
Any later code adoption requires its own pinned-source and license review.
Runtime features must continue to work with free local dependencies and models.

| Source read on 2026-11-09 | Relevant published capabilities |
| --- | --- |
| [Simplify Copilot help](https://help.simplify.jobs/help/articles/2415391-use-copilot-to-autofill-applications) | Document selection, profile autofill, question reuse, job-specific drafts, multipage filling, field exclusions and unsupported-field recovery. |
| [Teal tracker help](https://help.tealhq.com/en/articles/14435727-how-to-track-your-job-applications) | Pipeline stages, saved descriptions, notes, contacts, follow-up dates, linked resume versions and communication templates. |
| [Huntr product overview](https://huntr.co) | Resume and letter preparation, job-specific tailoring, PDF export, autofill, job/contact/interview tracking and search metrics. |
| [Reactive Resume repository](https://github.com/reactive-resume/reactive-resume) | Visual document editing, version recovery, PDF checks, application/calendar views, contacts, CSV exchange, local-model support and structured exports. |
| [Browser Use repository](https://github.com/browser-use/browser-use) | General browser agents, local browser/model operation and retained authenticated sessions; its hosted offerings are separate. |

Native captures remain in ignored `target/research/`, including substantive text,
HTTP status, provider version and errors. All five captures succeeded with final
HTTP 200; the resume repository redirected before returning 200. Provider:
`0.9.5+flitzzip.portable2`. Native-capture SHA-256 values, in table order:

```text
7ede52d58d6d1e65870afc0939f44f6322635fce84a5751fc56eb37d36ada127
c20c8a3d8068814e85e56fad013867a74c3996595969078ddd17cba3680bec44
d59545ade83e152afc2966f83d3e5eb7b73cd9e690919ecca7a7f0c8a4e99147
14ef8f4ac8f9c41e30ae4ffd1c58df5a687dc5f742a99a5d47df026bfc5c3592
e2edcb72f4cf8a5124991a75075177db317c5f5b879272efc9071220d666bf7e
```

## Form completion: the primary acceptance gate

The target is a dependable application agent, rather than a collection of
site-specific field-name rules. These capabilities must work together:

| Capability | Evidence required |
| --- | --- |
| Observe the complete application | DOM/accessibility structure, viewport captures and local visual inspection cover visible controls, frames, shadow roots and scrollable regions; incomplete inspection is explicit. |
| Understand field meaning | Distinguish applicant, referee, employer and dependent details; map repeated education/employment entries, dates, phone components, addresses and jurisdiction-specific questions without guessing missing facts. |
| Act on complex controls | Verify searchable dropdowns, native/custom choices, date controls, file widgets, repeaters, tabs and conditional sections with held-out layouts. |
| Navigate multiple steps | Detect step identity, prerequisites, validation errors and disabled controls; revisit and repair only affected answers without losing prior sections. |
| Verify each action | Read back the live value and visually inspect the relevant region; detect truncation, substitutions, stale nodes, overlays and validation failures. A successful command alone is insufficient. |
| Use the correct documents | Bind role, candidate revision, final PDF hash and attachment readback. A tailored draft must never silently upload the default CV or an old version. |
| Reuse knowledge carefully | Retrieve versioned, scoped platform lessons and user-approved answer examples; revalidate their preconditions. Measure held-out success and regressions before promoting lessons. Sensitive answers still require explicit review. |
| Recover intelligibly | Bounded repairs, resumable progress, cancellation and precise manual-review reasons for uncertain controls, authentication, challenges or changed pages. Never label partial work complete. |
| Preserve the submission boundary | Independently verified protection against script-triggered writes, including input/upload/navigation/worker/socket behavior and control disconnection. Current CDP interception alone does not establish this. No real-form interaction is permitted for development until this boundary is proven. |
| Measure actual local-model ability | Frozen prompts/model digests, held-out forms, repeat runs, false fills, missed fields, visual verification errors, navigation outcomes, manual-review rates, time and peak RAM/VRAM. Do not use the development agent's judgment as a substitute for model evaluation. |

This is the highest-priority workstream. The current fixture checks and single
model examples do not demonstrate exceptional or near-universal completion.
Submission remains disabled during development. Future delivery would require
explicit one-use approval for the exact application; the model cannot grant it.

## Breadth without disconnected features

Each row describes a connected workflow and its acceptance gate. Existing
prototype components are entry points for implementation, not completion proof.
The dated [implementation status](../IMPLEMENTATION_STATUS.md) remains the
record of what was actually measured.

| Area | Required connected workflow | Acceptance gate |
| --- | --- | --- |
| Profile and qualifications | Multiple document sources, structured employment/education, languages, links, preferences and custom instructions; provenance and correction propagation. | Changes invalidate affected pending answers/documents and preserve historical packets. No fabricated experience or silently inferred sensitive data. |
| Broad discovery | Local-model query variants, general-web and board sources, company career pages, saved searches and bounded refresh. | Diverse live read-only sources, retained ads, inspected application routes, stale/inaccessible distinctions and visible source failures. |
| Identity and duplicates | Compare employer, role content, location, requisition, source aliases and repost history despite title differences. | Labeled near-duplicate cases; reviewable merges, no lost distinct openings and no repeat application to the same position. |
| Matching and prioritization | Semantic requirements, objective constraints, supported experience, preferences, evidence and unknowns. | Stable rubric, held-out labels and repeatability; a score is not a hiring probability. |
| Document workspace | PDF/DOCX import, visual editing, per-job versions, factual diffs, undo/recovery and final PDF exports. | Diverse templates and page-by-page review; correct final attachment version, readable text and no clipping or unsupported claims. |
| Document checks | ATS-readable extraction, reading order, headings, factual consistency, relevant coverage and natural writing. | Independent parser comparisons and visual review, with explicit limitations; no promise of screening acceptance or AI-detector evasion. |
| Reusable answers | Searchable answer library with source facts, role/company scope, consent and revisions. | Stale company names/dates and fact changes invalidate suggestions; ambiguous and sensitive questions remain reviewable. |
| Applications workspace | List and board views, filtering, priority, tags, notes and job-specific next actions. | User-marked stages remain distinguishable from verified delivery; moving a card cannot manufacture a submission event. |
| Attempt archive | Original ad, source URLs, document versions, final PDFs, answers, screenshots, errors and navigation history per attempt. | Complete manifest with hashes and incomplete-capture indicators; interrupted attempts remain recoverable and submitted records immutable. |
| Contacts and interviews | Role-linked recruiter/contact records, interview rounds, preparation notes, dates and timezone-aware calendar exports. | Private local records, correct timezone/date handling and no outreach or calendar writes without authorization. |
| Follow-up and email | Connected inbox reconciliation, follow-up reminders, drafts, templates and email-application preparation. | Conservative message-to-job correlation, uncertainty review, no duplicate delivery and no automatic send during development. |
| Offers and decisions | Comparable compensation, work arrangements, deadlines and private decision notes. | Preserve currency, pay period, location and unknown components; record assumptions instead of inventing net-pay estimates. |
| Search insights | Funnel stages, source yield, time in stage, follow-ups due, field failures and model/manual-review outcomes. | Explicit denominators, time ranges and missing data; fixture results never appear as personal search outcomes. |
| Import, export and recovery | Structured JSON, safe CSV, PDF packets, calendar exports, encrypted backups and restore previews. | Round-trip tests, spreadsheet-formula handling, duplicate-safe imports and preservation of evidence/revisions after restore. |
| Local setup and resources | Per-user Windows install, dependency provisioning, browser/model checks, storage choices, bounded queues and cancellation. | Clean-host installation and update/recovery evidence; sustained RAM/VRAM measurements, owned-process cleanup and no paid API requirement. |
| Interface and access | Consistent settings, responsive panes, drag grips, useful statuses, reduced motion, readable errors and language/locale handling. | Actual rendered journeys at resize/zoom, assistive-technology checks and functional controls; design references alone do not qualify behavior. |

## Execution and evidence

1. Restore the browser integration baseline, then prove an independent
   no-submission development boundary. Keep real pages read-only meanwhile.
2. Evaluate and improve the local agent on multi-step, repeated-section and
   custom-control forms, preserving a held-out set and platform diversity.
3. Complete the document-to-attachment chain and attempt archive as one journey.
4. Extend tracking, contacts, interviews, reminders and portable records around
   the same job identity and immutable attempt history. Avoid parallel stores
   that disagree about a role or its documents.
5. Add breadth in successive verified slices. Each change records its source
   evidence, implementation, meaningful checks, limits and remaining gate.

Refresh the comparison as sources change and expand it to additional tools.
Do not turn feature counts into a quality ranking. The distinguishing result
must be measured form-completion quality and useful recovery, with the broader
workflow consistently supporting it.
