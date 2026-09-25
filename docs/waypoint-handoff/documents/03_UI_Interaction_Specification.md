# Waypoint — UI and interaction specification

This is a precision design reference for a future Rust-native app, not a compiled Windows product. All employers, candidates, documents, counts and events in the screens are fictional. Hardware numbers are targets or pending measurements, not benchmark results.

## Visual system

The canonical design uses a quiet navy navigation rail, light workspace, restrained teal action color, readable typography and generous spacing. The generated concept board supplied the exploratory direction; the vector screens and this specification control actual labels, states and behavior. No font files are distributed. The app should use Windows system fonts or separately audited redistributable alternatives.

Reference frame: 1600 × 1000 logical pixels; minimum target workspace 1280 × 800 before responsive adaptation. At smaller widths, move secondary evidence into a collapsible inspector rather than compressing text. At 200% scale, stack panels or allow intentional document/table scrolling; never shrink text to fake fit. Keep primary action, disclosure status and recovery path visible.

Hierarchy: eight navigation groups; command/search bar; page goal; primary action; bounded content; evidence/uncertainty inspector; quiet status strip. Use a persistent, accessible task drawer for running work. Do not turn the interface into a chat-only app or a noisy grid of arbitrary AI scores.

## Native interaction contract

Respect Windows window controls, snap/resizing, file dialogs, keyboard shortcuts, high contrast and accessibility settings. Esc dismisses a reversible dialog but cannot undo a submitted action. Ctrl+K opens commands; Ctrl+F searches the current view; tab order is deterministic. Focus must return to the initiating control when a dialog closes. UIA/AccessKit semantics and real Narrator/NVDA tests are required; screenshots do not certify accessibility.

Use named icons plus text for consequential actions. Status is never color-only. Buttons have distinct hover, focus, pressed, disabled and busy states. Busy does not mean unresponsive: expose cancellation when safe and explain why a submission cannot be rolled back. Respect reduced motion. Streamed output is buffered into paced updates; inactive windows do not animate continuously.

## Required states for every data view

Loading distinguishes initial load from refresh. Empty explains whether the campaign is too narrow or no known source returned data; it never conceals an error. Degraded keeps partial results with source failures. Offline labels time-sensitive evidence stale. Permission-required describes the scope and consequence before consent. Validation errors identify the field and remedy. Recovery preserves the user's edits and accurately describes whether an external write might already have happened.

Every screen in screen_catalog.json lists the global seven-state baseline plus a demonstrated state and a specific safety invariant. The 40 images are reference states, not 280 separately drawn variants. Additional edge states are behavior contracts to implement and test, not claimed finished UI screens.

## Documents and comparison behavior

Provide source-linked sentence selection, exact change diffs, base version, lock controls, page-count indicator and render validation. Keep the original photo ratio. Prepared packets expose exact file names, hashes and answers. Fact corrections show all downstream changes and approvals invalidated. Submitted packets remain immutable snapshots.

## External-action dialogs

Prefill discloses personal information to a live page and needs a separate explicit approval. Final Submit authorizes one exact destination/payload with expiry and a one-use grant. Unknown authorization, salary history or required declarations remain empty until the candidate responds. An uncertain submission shows Check for confirmation, not a default retry button. Exported files cannot necessarily be revoked after sharing; do not imply otherwise.

## Generated concept art: correction notes

The concept board is exploratory imagery. Its example percentages, model/memory numbers, job salaries, dates, job examples, automated-check wording and UI text are not source facts or production requirements. In particular, do not adopt a generated suggestion to add skills from the job description, use Update & Reapply as a default, treat a low-confidence outcome as submitted, or assume a fixed waiting period is universally appropriate. The canonical specification overrides such concept-art details. The image tool was used; its exact backend variant was not exposed for independent verification.

## Screen catalogue

### UI01 — Your next move

Purpose: A focused plan across your career campaigns.

Primary action: Review priority roles. Demonstrated state: ready. Layout: dashboard.

Data: Review two prepared packets: Both need your approval (Ready); Resolve one uncertain submission: Do not retry until reconciled (Needs attention); Recheck a saved opening: Employer source is out of date (Stale); Prepare for an interview: Use your own evidence and examples (Scheduled).

Invariant: Prepared is not Submitted. Implementation tasks: T013, T042.

### UI02 — Set up your private workspace

Purpose: Choose where data lives and what may leave your device.

Primary action: Create local workspace. Demonstrated state: onboarding. Layout: form.

Data: Storage location: A folder under your Windows account (Local); Encryption: Protected key and encrypted database (Required); Cloud model access: Not needed for core workflows (Off); Diagnostic reporting: Preview each report before sending (Off).

Invariant: Never upload candidate documents during setup. Implementation tasks: T010, T044.

### UI03 — Qualify your local model

Purpose: Measure the complete workload before enabling AI tasks.

Primary action: Run qualification. Demonstrated state: unqualified. Layout: form.

Data: Target GPU: 16 GB discrete GPU · AMD reference (Target); Reasoning model: Qwen3.5-9B · candidate configuration (Unqualified); Context profile: 8K / 16K / 32K tests (Pending); Peak memory and recall: Not measured on this device yet (Pending).

Invariant: A small model file is not proof of VRAM fit. Implementation tasks: T007, T008.

### UI04 — Import career evidence

Purpose: Bring in documents without losing their origins.

Primary action: Select files. Demonstrated state: ready. Layout: form.

Data: Resume and work history: PDF, DOCX or plain text (Choose); Certificates and publications: Keep source, date and ownership (Choose); Portfolio and project evidence: Candidate-authorized files or URLs (Choose); Sensitive attachments: Macros disabled; restricted extraction (Protected).

Invariant: A CV statement starts as candidate evidence, not external proof. Implementation tasks: T012.

### UI05 — Your evidence ledger

Purpose: See the source behind every claim in your application.

Primary action: Review selected claim. Demonstrated state: ready. Layout: list.

Data: Coating test setup: Candidate CV · paragraph 12 (Attested); Thin-film deposition: Project notes · pages 3–4 (Attested); Employment dates: Two documents disagree (Conflict); Team contribution: Specific role needs confirmation (Review).

Invariant: Traceability is not the same as truth. Implementation tasks: T012, T022.

### UI06 — Resolve a profile conflict

Purpose: Confirm the fact before changing dependent documents.

Primary action: Confirm correction. Demonstrated state: conflict. Layout: modal.

Data: Earlier record: Role end date: June (Original); New candidate correction: Role end date: September (Proposed); Affected artifacts: 3 unsent documents; 1 approval (Review); Already submitted packet: Preserved exactly as sent (Immutable).

Invariant: Only affected unsent artifacts change. Implementation tasks: T025.

### UI07 — Campaign strategy

Purpose: Keep urgent income, specialist and relocation searches separate.

Primary action: Save campaign. Demonstrated state: ready. Layout: form.

Data: Primary campaign: Specialist engineering (Active); Location and working language: Explicit hard constraints (Strict); Compensation: Base annual salary; unknown shown separately (Strict); Unknown sponsorship: Keep in review, never mark eligible (Review).

Invariant: Working language is not the ad language. Implementation tasks: T013.

### UI08 — Discover the right roles

Purpose: Search permitted sources with visible coverage and constraints.

Primary action: Inspect selected role. Demonstrated state: partial_results. Layout: list.

Data: Optical Coatings Engineer: Northstar Optics · fictional employer (Strong evidence); Materials Process Engineer: Meridian Labs · fictional employer (Review); Surface Scientist: Cedar Research · fictional employer (Unknown salary); Manufacturing Lead: Harbor Systems · fictional employer (Constraint gap).

Invariant: Do not hide source failures or call this exhaustive. Implementation tasks: T014, T018, T019.

### UI09 — Opportunity details

Purpose: Separate source facts from estimates and unresolved questions.

Primary action: Build application plan. Demonstrated state: ready. Layout: detail.

Data: Employer requisition: Northstar demo · NS-101 (Source linked); Opening status: Observed open in fictional fixture (Demo); Base compensation: USD 160k–190k / year · synthetic (Demo); Sponsorship for this role: Not established by the example (Unknown).

Invariant: Never invent role-specific sponsorship. Implementation tasks: T017, T018.

### UI10 — Why this role fits—or does not

Purpose: Requirement-by-requirement evidence, not a hiring probability.

Primary action: Review unknowns. Demonstrated state: mixed_evidence. Layout: list.

Data: Physical vapor deposition: Supported by candidate evidence (Supported); Optical filter design: Supported by project notes (Supported); Production ownership: Not established in supplied evidence (Unknown); Working language: Required language needs confirmation (Unknown).

Invariant: Preferred and mandatory requirements remain distinct. Implementation tasks: T018, T022.

### UI11 — Company research

Purpose: Read a source-linked brief prepared by your local model.

Primary action: Refresh research. Demonstrated state: stale. Layout: detail.

Data: Products and teams: Official company pages first (Source linked); Role-specific evidence: Employer posting and team context (Source linked); Recent changes: Date and source required (Needs refresh); Conflicting information: Preserved, not silently averaged (Review).

Invariant: Current claims require fresh external evidence. Implementation tasks: T020.

### UI12 — Possible duplicate opportunity

Purpose: Inspect identity evidence before merging application history.

Primary action: Link as aliases. Demonstrated state: duplicate_review. Layout: modal.

Data: Employer identity: Same fictional employer namespace (Matches); Requisition identifier: NS-101 on both sources (Matches); Location wording: Different labels; inspect change (Review); Existing application: One prepared packet, not submitted (Prepared).

Invariant: Different URLs do not automatically mean new roles. Implementation tasks: T016.

### UI13 — Saved searches and watchlists

Purpose: Know what will run, where it will search and when it stops.

Primary action: Edit schedule. Demonstrated state: ready. Layout: list.

Data: Specialist engineering: Direct employer sources (Enabled); Local income options: Separate constraints and profile (Paused); Relocation opportunities: Unknown authorization in review (Enabled); Failed source refresh: Last good data retained as stale (Degraded).

Invariant: No hidden remote monitoring service. Implementation tasks: T013, T019.

### UI14 — Resume studio

Purpose: Edit a source-backed resume without altering locked facts.

Primary action: Review changes. Demonstrated state: draft. Layout: document.

Data: Base document: Resume v2 · fictional profile (Selected); Professional title: Exact wording is locked (Locked); Page target: Two pages; no silent deletion (Required); Layout validation: Render and text order checks (Pending).

Invariant: Do not stretch or distort profile images. Implementation tasks: T021, T023, T024.

### UI15 — Cover letter studio

Purpose: Write a specific letter from candidate and employer evidence.

Primary action: Review letter. Demonstrated state: validation_error. Layout: document.

Data: Candidate claims: From approved profile revision 2 (Linked); Employer references: From current company brief (Linked); Length and tone: Short, direct and role-specific (Preference); Unsupported sentence: Needs candidate confirmation (Blocked).

Invariant: A polished layout does not override a truth check. Implementation tasks: T022, T023.

### UI16 — Application answers

Purpose: Reuse context carefully and respect exact portal limits.

Primary action: Review selected answer. Demonstrated state: needs_input. Layout: list.

Data: Why this role?: Linked to employer and candidate evidence (Draft); Work authorization: Explicit candidate response required (Required); Expected salary: Currency, period and gross/base basis (Review); Project ownership: Specific individual contribution (Draft).

Invariant: Sensitive declarations are never guessed. Implementation tasks: T026.

### UI17 — Review your application packet

Purpose: Approve the exact files and answers—not a vague instruction.

Primary action: Open prefill review. Demonstrated state: prepared. Layout: detail.

Data: Resume v3.pdf: Content and layout checks passed in fixture (Demo); Cover letter v2.pdf: One specific role, exact employer (Demo); Portal answers v4: All required declarations reviewed (Demo); Submission status: No information has been submitted (Prepared).

Invariant: Prepared is never counted as confirmed. Implementation tasks: T025, T029.

### UI18 — Assisted application workspace

Purpose: Visible employer form with a narrow, approved data mapping.

Primary action: Review prefill disclosure. Demonstrated state: permission_required. Layout: browser.

Data: Name and contact: Candidate-approved profile fields (Review); Uploaded resume: Exact reviewed PDF hash (Review); Custom required question: Cannot infer candidate answer (Needs input); Login or challenge: Complete personally in the browser (Handoff).

Invariant: Never attach to unrelated personal browser tabs. Implementation tasks: T027, T028, T029.

### UI19 — Approve final submission

Purpose: One application, one destination, one exact reviewed payload.

Primary action: Approve and submit. Demonstrated state: awaiting_approval. Layout: modal.

Data: Destination: employer.example.invalid · fictional (Checked); Documents and answers: Exact packet digest and form revision (Bound); Permission lifetime: One use; expires shortly (Limited); What happens next: Receipt or an explicit uncertain state (Visible).

Invariant: A changed packet, form or destination cancels this grant. Implementation tasks: T029, T030.

### UI20 — Submission outcome uncertain

Purpose: The website may have received the application. Do not retry automatically.

Primary action: Check for confirmation. Demonstrated state: uncertain. Layout: recovery.

Data: Last recorded step: Submit command began; no receipt captured (Uncertain); Safe next action: Inspect employer account or authorized inbox (Review); Retry behavior: Automatic retry is disabled (Protected); Manual decision: Record your confirmation or deliberate retry (User choice).

Invariant: Never show a misleading Try again primary button. Implementation tasks: T030, T032.

### UI21 — Application receipt

Purpose: Keep the evidence of what was sent and what was acknowledged.

Primary action: Inspect receipt evidence. Demonstrated state: confirmed_fixture. Layout: detail.

Data: Application reference: DEMO-NS-101 · fictional identifier (Demo); Employer acknowledgement: Source origin and captured excerpt (Linked); Packet version: Resume v3 · letter v2 · answers v4 (Immutable); Timeline status: Confirmed by qualifying fixture evidence (Demo).

Invariant: A receipt ID alone does not prove remote delivery. Implementation tasks: T031.

### UI22 — Application timeline

Purpose: Every stage has an origin, date and supporting evidence.

Primary action: Open selected application. Demonstrated state: ready. Layout: list.

Data: Northstar · NS-101: Prepared packet; awaiting candidate action (Prepared); Meridian · ML-204: External write outcome not resolved (Uncertain); Cedar · CR-016: Candidate reports an external application (Attested); Harbor · HS-321: Interview invitation linked to source (Interview).

Invariant: Do not merge all company responses into one role. Implementation tasks: T030, T036, T042.

### UI23 — Inbox review queue

Purpose: Turn selected messages into proposed updates, with review.

Primary action: Review proposed update. Demonstrated state: ambiguous. Layout: list.

Data: Interview invitation: Matched by requisition ID in fictional email (High evidence); Application acknowledgement: Matches destination and role (Review); Rejection message: Two possible roles at the same company (Ambiguous); Unrelated personal email: Outside the selected import scope (Excluded).

Invariant: A low-confidence match is a proposal, not a timeline fact. Implementation tasks: T035, T036.

### UI24 — Interviews and calendar

Purpose: Prepare appointments with timezone and availability checks.

Primary action: Review calendar event. Demonstrated state: conflict. Layout: list.

Data: Technical interview: Fictional event · Europe/Zurich (Draft); Timezone conversion: Original timezone preserved (Checked); Existing appointment: Potential overlap detected (Conflict); Attendee identity: Confirm exact recipient before invitation (Review).

Invariant: No calendar action is sent from inferred attendees. Implementation tasks: T037.

### UI25 — Relationships and introductions

Purpose: Relevant conversations, not indiscriminate outreach.

Primary action: Review follow-up draft. Demonstrated state: ready. Layout: list.

Data: Former collaborator: Candidate-supplied relationship (Known); Professional introduction: Exact public or shared contact route (Review); Recruiter conversation: Follow-up date chosen by candidate (Scheduled); Unknown email address: Not guessed or scraped into a campaign (Blocked).

Invariant: No invented relationships or mass unsolicited sends. Implementation tasks: T038.

### UI26 — Interview practice

Purpose: Rehearse with the role and your actual experience.

Primary action: Start practice session. Demonstrated state: ready. Layout: detail.

Data: Technical example: Explain your coating-test setup (Ready); Behavioral question: Describe your individual contribution (Ready); Company questions: Use current source-linked research (Refresh); Voice and recording: Off until explicitly enabled (Off).

Invariant: Practice transcripts remain private by default. Implementation tasks: T039.

### UI27 — Close a real skills gap

Purpose: Build useful evidence without exaggerating what it proves.

Primary action: Create learning task. Demonstrated state: ready. Layout: list.

Data: Required production method: Not evidenced in current profile (Gap); Small practical exercise: Candidate-owned demonstrable output (Plan); Paid retraining route: Official source and criteria needed (Research); Completed short course: Not equivalent to years of employment (Qualified).

Invariant: A recommendation is not an eligibility verdict. Implementation tasks: T040.

### UI28 — Compare offers

Purpose: Separate actual compensation from assumptions and risk.

Primary action: Edit scenario assumptions. Demonstrated state: mixed_evidence. Layout: list.

Data: Base salary: Original currency and yearly basis (Source linked); Fixed installments: 12 versus 13 payments normalized (Explicit); Bonus and equity: Variable; not guaranteed base pay (Scenario); Relocation and authorization: Outstanding dependencies (Unknown).

Invariant: Estimated net pay is not a payroll quotation. Implementation tasks: T041.

### UI29 — Outcomes and effort

Purpose: Learn from your search without making unsupported causal claims.

Primary action: Inspect funnel definitions. Demonstrated state: ready. Layout: dashboard.

Data: Shortlisted: 12 fictional opportunities (Demo); Prepared: 3 local packets; not yet sent (Demo); Confirmed or attested: Shown as separate denominators (Demo); Active candidate effort: Measured during a future pilot (Pending).

Invariant: Do not label correlations as hiring uplift. Implementation tasks: T042, T048.

### UI30 — Source coverage and health

Purpose: Know what the search reached—and what it did not.

Primary action: Inspect source capability. Demonstrated state: degraded. Layout: list.

Data: Greenhouse-like public listing: Discovery fixture only (Demo); Lever-like public listing: Read path; employer submission key not assumed (Read only); Regional source: Unsupported locale in example (Unsupported); Aggregator refresh: Last-known records retained with warning (Degraded).

Invariant: Failed source is not zero jobs. Implementation tasks: T002, T019, T034.

### UI31 — Research run and task queue

Purpose: Visible progress, bounded work and meaningful cancellation.

Primary action: Pause queued work. Demonstrated state: running. Layout: list.

Data: Fetch employer evidence: Public request; no candidate profile sent (Running); Local reasoning: One GPU generation slot (Queued); Document render checks: Restricted local worker (Queued); Application submission: Never part of an unattended research run (Protected).

Invariant: An endless spinner is not a valid recovery policy. Implementation tasks: T009, T020.

### UI32 — Privacy and network controls

Purpose: Separate local inference from internet research and external writes.

Primary action: Review permissions. Demonstrated state: ready. Layout: form.

Data: Local model inference: Core AI stays on this device (Required); Public web research: Purpose-limited, candidate PII minimized (Allowed); Inbox and calendar: Independent explicit scopes (Off); Telemetry and enrichment: No default behavioral reporting (Off).

Invariant: Local storage alone is not a privacy guarantee. Implementation tasks: T003, T015, T047.

### UI33 — Backup and restore

Purpose: Protect the whole workspace, not only the main database.

Primary action: Create encrypted backup. Demonstrated state: ready. Layout: form.

Data: Backup scope: Evidence, versions and schema manifest (Selected); Encryption and key: Stored separately from exported archive (Required); Restore verification: Test hashes and counts in clean workspace (Required); Deletion limits: SSD overwrite is not guaranteed erasure (Disclosed).

Invariant: An encrypted database does not encrypt every export. Implementation tasks: T011, T047.

### UI34 — Resource diagnostics

Purpose: Measure memory, latency and correctness together.

Primary action: Run diagnostic profile. Demonstrated state: unmeasured. Layout: detail.

Data: UI process: Idle CPU and resident memory (Pending); Model process: Dedicated/shared VRAM; offload (Pending); Browser process: Included in aggregate resource budget (Pending); Task quality: Context recall and grounded answers (Pending).

Invariant: Rust is not a performance benchmark. Implementation tasks: T008, T009, T048.

### UI35 — Share a review packet

Purpose: Give a reviewer only the items you deliberately choose.

Primary action: Create scoped export. Demonstrated state: permission_required. Layout: modal.

Data: Included documents: Resume and selected answers only (Selected); Private evidence ledger: Excluded from this packet (Excluded); Sensitive contact details: Redacted in the preview (Redacted); Reviewer permissions: Comment-only; no application authority (Limited).

Invariant: Never share the entire vault by default. Implementation tasks: T043.

### UI36 — Get help without losing context

Purpose: Hand off a precise question when the system cannot resolve it.

Primary action: Prepare help summary. Demonstrated state: needs_human. Layout: detail.

Data: Question to resolve: Working-language requirement is ambiguous (Unknown); Evidence to include: Selected role excerpt only (Review); Human route: Candidate-chosen contact or official service (Choose); What is not included: Private credentials and unrelated history (Excluded).

Invariant: No false reassurance or invented authority. Implementation tasks: T040, T050.

### UI37 — Accessibility and appearance

Purpose: Make the workspace readable and controllable your way.

Primary action: Apply display preferences. Demonstrated state: ready. Layout: form.

Data: Text scale: Follow Windows; tested enlarged layouts (Preference); Motion: Reduced animation and static progress (On); Keyboard and screen reader: Visible focus and semantic control names (Required); Theme and contrast: Light / dark / system (System).

Invariant: A custom widget needs real accessibility semantics. Implementation tasks: T006.

### UI38 — Review an update

Purpose: Verify the publisher, package and migration before installing.

Primary action: Install verified update. Demonstrated state: awaiting_approval. Layout: modal.

Data: Release identity: Pinned version and signed metadata (Review); Package checksum: Must match the trusted manifest (Required); Database migration: Backup and compatibility check (Required); Rollback: Last supported version retained (Ready).

Invariant: A signature is checked, not merely displayed. Implementation tasks: T044, T045, T046.

### UI39 — Model capacity exceeded

Purpose: Preserve the task and choose an explicit smaller workload.

Primary action: Review safe recovery. Demonstrated state: out_of_memory. Layout: recovery.

Data: Current task: Draft saved before the resource failure (Preserved); Reduce context: Retrieve a smaller evidence set (Option); Qualified smaller model: Requires your explicit choice (Option); Cloud fallback: Never activated silently (Disabled).

Invariant: No paid request is triggered by a local failure. Implementation tasks: T008, T009.

### UI40 — Review campaign changes

Purpose: Preview exactly what relaxing a search constraint would change.

Primary action: Create separate branch. Demonstrated state: counterfactual_review. Layout: modal.

Data: Current campaign: Strict location and base-salary requirements (Preserved); Proposed branch: Wider location; original remains unchanged (Separate); Unknown evidence: Still displayed as unknown (Preserved); Existing applications: No retroactive changes or new sends (Unchanged).

Invariant: Relaxed results never enter strict results silently. Implementation tasks: T013, T018.
