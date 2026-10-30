# Browser-interface prototype checkpoint

The current checkout is based on upstream `f5a0de0d16cbe6c3c1ac37c69e751dc7d4020d30`. The user requested a Windows tray-controlled local web interface, local Ollama inference, and reviewable application autofill. Do not submit real applications during development or tests. A model must never authorize an external write; retain the existing one-use approval and uncertain-result rules for any future submission capability.

The previous scratch Python prototype is retained locally in `prototype-local/` and excluded from Git locally. It is not the shipped Waypoint implementation and must not replace the Rust domain, workspace, browser, inference, or storage components.

The existing `waypoint_workspace`, `waypoint_browser`, `waypoint_inference`, and `waypoint_materials` crates are the starting point for the browser interface. Existing browser support does not yet handle all third-party forms, nested frames, or multi-step flows; the production inference adapter is not wired into all drafting flows. See `KNOWN_LIMITATIONS.md` for the upstream state and verify current behavior before claiming completion.

The GitHub account inventory identified other potentially relevant projects, notably `rejection-rejector` (local Ollama and review workflows), `model-protocol-lab` (structured-output evaluation), and `TextForensics` (a local browser interface). Initial README inspection is not a full review of those repositories. Keep all inspection read-only and use an isolated, correctly bound CocoIndex search before broad code navigation.

Resource constraint: avoid concurrent model loads and unbounded indexing or compilation. Do not download model weights or run GPU qualification without checking current available resources. Limit build parallelism, and distinguish measured runtime use from historical model qualification records.

First web slice: `cargo run -p waypoint_web -j 1` opens a loopback-only browser page backed by the existing Rust workspace. It exposes public-board discovery, candidate facts, lexical ranking, shortlisting, personal details and instructions, and file upload/download; it has no submission endpoint. Profile details are stored but do not yet drive matching, drafting, or autofill. Files are limited to 10 MiB each and, where Windows DPAPI is available, profile details, filenames and content are sealed in the workspace database. UTF-8 text, Markdown and DOCX body text can be extracted locally for review; the user can select text into the qualification editor and explicitly save it as an attested fact. PDF extraction and automatic document analysis are not implemented. UI design and validation are in progress. Tailored documents, Ollama-assisted form interpretation, browser autofill, tray control, and packaging remain incomplete. Matching searches job title, employer, location, and board-supplied description text when available. Missing descriptions remain missing until the board is refreshed; this is a lexical score, not a suitability judgment.

The web page can also prepare and inspect an existing fact-backed packet for a shortlisted job. Preparation refuses to replace an existing packet. These drafts are deterministic summaries, not yet tailored CVs or model-written cover letters, and they must be reviewed. A browser check with fictional qualifications exposed pre-existing wording that implied an application had already been made; newly generated packets now label the target position without asserting submission. Previously saved packets are not silently rewritten.

An experimental local-model form-mapping component can propose associations between live field labels and profile keys, then validates the response and fills values from the stored profile deterministically. It does not send candidate values to the model, authorize or execute any browser action, or submit anything. It is not yet connected to the web UI or a live form; fixture tests use a fake inference provider. No actual Ollama model was loaded or qualified for this workflow in this round.

The browser page now lets users mark and filter jobs for manual review without changing application state. A form-preview component detects walls and missing required answers as review reasons and has been exercised against a local fixture with one real headless browser and a fake model. The web interface has an explicit Inspect form action for shortlisted/prepared jobs; it reads the page and proposes mappings but does not fill or submit. The coordinator records a generic review flag for blocked or incomplete inspections. It only attempts model work when Ollama reports one fully GPU-resident model within 14 GiB; this is a snapshot, not a memory reservation, and runtime state can change after the check. No end-to-end test with an actual model or third-party application was performed. Navigation across sections, multi-step forms, and actual autofill remain to be implemented. CAPTCHA completion requires the user's involvement. The Ollama adapter rejects HTTP failures and structured responses without an explicit natural stop, and bounds response size to 8 MiB.

An experimental reviewed-text prefill function is connected to the web interface. Inspection holds one temporary browser session for at most 120 seconds. A separate explicit user action reviews the mapped values and creates a durable consumed prefill grant bound to the mapping, profile digest, form schema, and origin before field entry. The browser captures a screenshot and closes; the app does not press submit. Fixture tests exercise the endpoint, one real headless browser, synthetic candidate data, and a fake model. Real employer websites, dynamic pages, selectable options, file uploads, multi-step navigation, and continuing a browser session after the screenshot remain unverified or incomplete. Employer pages may perform their own network activity while fields are entered; this has not been evaluated on real sites.

Text prefill now replaces existing field contents through browser input events, and duplicate field selectors or required disabled/read-only fields are reported as requiring review. The local fixture verifies replacement and ambiguous-selector handling; no employer form was used. Select controls are not yet proposed for autofill.


Manual handoff update: inspection can optionally open a visible, temporary Chromium window for the user to handle sign-in, CAPTCHA and extra sections. The controlled browser remains open after reviewed text prefill, with a 10-minute visible-session limit (two minutes headless) and an explicit close button. One session is held at a time, and prefill is one-use. If no eligible local model is available, visible review can still open the job and records a manual-review flag; it does not propose or fill fields. The app does not solve CAPTCHAs, navigate multi-step forms autonomously, or submit applications. The visible-window path and actual external sites have not been exercised; the headless local fixture was tested. The current page is inspected without navigating before prefill.


The review panel can inspect the current page after manual navigation, solve a wall manually, and request a new set of suggestions with a fresh one-use review session. Multi-step status remains flagged for manual review; the browser does not automatically advance or submit. Current-page prefill checks the live origin and schema without reloading. A local headless fixture confirms that manually entered values on the page survive a separate reviewed fill. The rescan endpoint and visible window have not been exercised against a real employer or real model.

Each suggested field now has a checkbox in the review panel. Only selected mappings are included in the one-use prefill grant; omitting a suggestion marks the job for manual review. A synthetic local browser fixture checked that an omitted email field was left untouched. Editing individual suggested values is not implemented.

A saved document can now be explicitly selected for a detected file input in the form review panel. The current form schema and destination are rechecked, a consumed prefill record is written before the attachment, and at most five files of at most 10 MiB each are staged for one session. The browser receives a temporary local plaintext copy, retained until the session closes; an application crash can leave it behind. A local headless fixture exercised file attachment and normal session cleanup. Real employer sites, custom upload widgets, MIME/type restrictions, and crash cleanup are not yet verified. Attaching may cause the site to transmit the document immediately, independently of submission.

The job list now has per-job selection, select-visible and clear controls, and sequential bulk shortlisting. Selections persist while filtering and are removed once the corresponding job is no longer in the discovered state. Partial failures are reported, and no applications are sent. A local browser fixture with two fictional jobs exercised selection and bulk shortlisting.

Template settings now allow a saved PDF or DOCX to be designated separately as the CV and cover letter template, plus document-specific instructions. They are not yet used to generate or visually edit documents; PDF extraction, visual editing, and mandatory final PDF conversion remain unimplemented.

A disk-backed preparation session creates an application-records subfolder when the user first fills fields or attaches a document. It stores job metadata, planned and completed actions with selected answers, copies of attached materials, a prepared draft if one exists, and viewport screenshots after actions. Records are plaintext and separate from the DPAPI-sealed database. This is preparation evidence only, not a submission receipt; a viewport screenshot is not a full capture of every field or manual action. Real submission attempts and their receipts are not yet implemented. A synthetic browser fixture checked answer, material, screenshot and cleanup artifacts.

Each new preparation record also saves `original-job.txt` and `original-job.json`, containing the workspace's saved description, source URL and listing metadata as they stood when the record was created; the JSON includes a description checksum and explicitly indicates when text was unavailable. This preserves the stored listing, which may be normalized or user-pasted, rather than a complete visual capture or original page bytes. A fictional disk-backed fixture verified the saved text, metadata and checksum. Records created before this change are not backfilled.

For a shortlisted job, the user can prepare an unsent `.eml` file with a manually entered recipient, subject and message plus up to five saved PDF attachments totaling 10 MiB. The file, attachments, listing snapshot and draft event are saved in a new plaintext application record. The file is downloaded for review in a mail client; this draft workflow does not send email. The MIME unit test uses fictional data; mail-client interoperability and the complete UI flow have not yet been tested. `rejection-rejector` commit `48c672b29cf098ff5da10d324de3e87766a2f483` supplied the MIME subject encoder and its license is retained. Its documented Gmail OAuth/SMTP and at-most-once delivery mechanisms need adaptation for new messages with attachments; its existing Gmail send method expects a thread ID.

Gmail settings now provide an explicit Desktop OAuth connection using a user-supplied client JSON, loopback callback, PKCE and read/send consent. The OAuth and callback code was adapted from the same rejection-rejector commit, with its license retained; exact upstream source files and hashes are in the ignored `prototype-local/rejection-rejector-reviewed` directory. Refresh credentials are accepted only when Windows DPAPI is available and stored in a sealed SQLite record; disconnect removes the local record but does not revoke Google's consent. A fictional Windows disk-backed test verified that the credential value can be recovered and does not appear as plaintext in the database. Real Google consent and token exchange have not been tested. There is still no Gmail send implementation or mailbox identity check in this app. The application-record folders, including `.eml` files, materials, answers and screenshots, remain plaintext and require further protection; DPAPI protection of database values does not extend to those folders.

An optional local-model search planner suggests up to ten editable search phrases from attested qualifications and saved search instructions, using the existing single-model lease and resident-model check. The user selects a suggestion for the Jobicy search box before sending it to the provider. Suggestions are not global search, their relevance and removal of personal information are not guaranteed, and a fake-model test covers output validation rather than real-model quality.

Newly imported and rediscovered listings are checked against a bounded, indexed set of existing jobs with similar employer names. The candidate comparison uses normalized title words and, when both descriptions contain enough text, description word overlap; it records possible pairs for user review rather than merging them. Pending pairs hold autofill and attachment for both jobs. The user can mark listings different or choose which listing to keep for a confirmed match; the other listing remains held. Material job changes reset decisions for review. At that checkpoint the matcher used employer prefixes; the later indexed comparison and gradual backfill are described below. It remains a conservative prototype rather than AI or semantic duplicate detection, and protection against all duplicate submissions is not established.

The user requested encryption of the complete settings and position-information database, while application record folders may remain readable. At that checkpoint, candidate profile fields, documents and Gmail credentials were sealed with Windows DPAPI, while job records and database metadata remained plaintext. The later SQLCipher work below changes the Windows default, while retaining old plaintext copies during migration.

An experimental `waypoint_store/sqlcipher` feature now opens a SQLCipher database with a supplied 32-byte key and exports a plaintext SQLite database into a separate newly created encrypted file, retaining the original and verifying the new database's integrity. Fictional Windows tests checked encrypted headers, rejection of the wrong key, restored content, version preservation and refusal to overwrite the destination. At that checkpoint the feature was not enabled in the default app build; the later Windows default change is documented below. Production migration, backup policy, packaging and crash-recovery handling remain unfinished. This host build used its installed OpenSSL; a vendored OpenSSL probe failed because Git's Perl installation lacks `ExtUtils::MakeMaker`. Neither approach establishes a host-independent installer. SQLCipher's BSD-style license is retained in `docs/SQLCIPHER_LICENSE.txt`. Official SQLCipher API research used Crawl4AI 0.9.4+superzip.portable2 with robots checking and a 30-second timeout; the successful capture is `prototype-local/sqlcipher-research/sqlcipher-api-retry.json` (HTTP 200, SHA-256 `fd4e69c2b46617999e3a2b2e8c187c8b34dfd4a700ec36598a30d19a10eeed3c`). The initial CLI output-file attempt failed and its empty output was retained locally.

The optional `waypoint_web/sqlcipher` build now opens the encrypted store at startup and generates a 32-byte key protected by the current Windows account's DPAPI. Existing plaintext databases are exported to a separate validated encrypted file; the original remains in place and the UI reports that plaintext copy. Both current build variants acquire an exclusive workspace lock, and the plaintext variant refuses to open a folder with an encrypted database, preventing silent fallback to the retained old data. A fictional Windows test verified migration of a position and qualification, encryption of position text, reopening, and refusal of a second workspace. The later Windows default change is documented below. Packaging, key recovery, legacy plaintext backup handling, interoperability with older unlocked builds, and crash recovery are not yet qualified; this is not a production security claim.

Unresolved or non-preferred duplicate listings now also block creation of an email application draft, in addition to form filling and attachment. A fictional endpoint test covered that hold. No application or email submission capability exists yet, and no duplicate prevention is guaranteed across postings missed by the candidate matcher.

The workspace location defaults to the user's `%APPDATA%/smart-job-seeker` folder, falling back to a folder under `%USERPROFILE%` if APPDATA is unavailable. Settings can select an absolute folder for the next restart. The small location pointer is saved separately under the user's profile; selecting a new folder does not move, copy, or delete the existing workspace. A local test checked persistence, validation and non-movement. The displayed application-record location must be interpreted relative to the selected workspace.

The DOCX-to-PDF action now runs conversion on a background worker, using the existing single-converter limit and 30-second process timeout; the UI follows progress and can resume polling after a reload. An offline test converted a fictional DOCX using the installed LibreOffice and retrieved the resulting PDF from the workspace. Other paths, including browser rescans and legacy synchronous endpoints, still block the single request-serving thread; full asynchronous operation is not yet implemented and no hard process memory ceiling has been established.

Initial form inspection now runs browser startup, navigation and model mapping in one background worker, with one browser inspection at a time and the existing shared model lease. The UI polls and can resume after reload. Inspecting the current page again also runs in that worker and keeps the browser session for review; changed job or profile data invalidates the returned suggestions. Fills, some tool checks and legacy endpoints still run synchronously, so full asynchronous operation remains incomplete. Non-exact native select options can be proposed in one bounded local-model request after the initial key-to-field mapping; only exact unique page options are accepted and the user reviews them before filling. The second request includes the relevant saved profile value, but stays on the local inference provider. Fake-model tests cover the option-validation path, not semantic accuracy or real employer forms.

Form inspection can follow one unambiguous same-origin application anchor before proposing fields. For other detected application links, including cross-origin ATS links, the user can explicitly select the displayed destination and open it in the existing browser session. The worker rechecks that the link is present, applies public HTTPS and source-policy checks, and marks further cross-origin redirects or blockers for manual review. Navigation performs no deliberate field action or submission, although page scripts can make network requests. Candidate filtering and destination policy have a scripted test; the new cross-origin navigation has not been tested against an employer site. Arbitrary interactive navigation and general multi-step applications remain incomplete.

A fictional local browser fixture exercised the new background inspection worker without filling or submitting. The user's benchmarked skills collection was inspected at local commit `4bd209752567f5c35074d4ee4a73600e1c45c283`; its documented `web-discovery` skill was adapted into the self-contained project skill `.agents/skills/waypoint-job-discovery/SKILL.md`, with upstream provenance and its benchmark limitation stated. The collection's required CocoIndex provider was not present at its documented local path, so this is a targeted inspection, not an exhaustive review of the collection.

Several interface passes used an image-generated desktop concept, then refined the existing split layout, compact job actions, duplicate review, collapsed discovery controls, responsive layout, and working section links. Synthetic browser fixtures exercised job intake, duplicate decisions, selection, Jobicy attribution and navigation at desktop and 390-pixel widths. The fixture used mocked local responses and did not contact external job sites; this is not a full accessibility or real-site acceptance test.

A saved DOCX can now be explicitly converted to a separate saved PDF using an installed LibreOffice (`WAYPOINT_SOFFICE` may specify its executable). Conversion uses a temporary profile, one serial request, 10 MiB input and output limits, and a 30-second process timeout. A fictional one-page DOCX was converted locally; its PDF text and rendered page were inspected. Complex layout fidelity, process-tree termination and a hard memory ceiling are not verified. This is conversion only: visual editing, model-directed tailoring, PDF text extraction, automatic final-PDF enforcement and automatic inclusion in an attempt remain incomplete.

Attempt records now include sequential page-segment screenshots after reviewed fill and attachment, limited to 2048 CSS pixels wide and 12000 high, at most six segments. Exceeding those limits or capture failure is recorded and flags manual review. Nested scroll areas, hidden fields, and cross-origin frames can still be absent even if page dimensions fit. A local tall-page fixture exercised multiple segments; it did not use an employer site.

The requested personalized style pass remains unimplemented. When implemented, it must use candidate-provided writing preferences and supported experience, preserve facts, and require review. Detector outcomes cannot be guaranteed.

Each job now explains the existing literal qualification-overlap percentage and can request a separate local-model fit estimate. The on-demand worker uses up to 20 reviewed qualifications, 2,000 characters of search preferences and 8,000 characters of listing text, sharing the single model lease. Its cited qualification references and job excerpts are checked against those inputs, but the score, interpretation, concerns and factual relevance still require human review. A focused validator test and a mocked local UI fixture passed; no live model or employer listing was used. The estimate is sealed with Windows DPAPI when available and can be reopened from the job card. Its exact bounded inputs are hashed and a changed input makes the saved assessment stale. When secure storage is unavailable, the result stays temporary. It does not replace or automatically reorder the lexical ranking. A DPAPI round-trip and mocked UI reload were tested; no live model was used.

Native single-select dropdowns can now be proposed and filled when a profile value matches exactly one option label (ignoring ASCII case). The browser uses a fixed operation that rejects disabled or ambiguous options and dispatches input and change events. A fictional local fixture verified a country selection and site change event; a focused test covers unknown and duplicate options. Custom comboboxes, multi-selects, option aliases and real employer sites remain unverified or unsupported.

Document review can request up to twelve exact qualification excerpts from the first 12000 UTF-8 bytes using one already-resident eligible Ollama model. Suggestions run in a background thread so the web server remains responsive, and share a single model-use lease with form mapping. Outputs must exactly occur in the source and remain unsaved until the user selects and attests them. Long documents are explicitly marked truncated; this is not a full document analysis. A fake-model test covers exact-source enforcement, but no live model was loaded for this round (Ollama reported zero resident models). PDF extraction remains unavailable.

Selectable-text PDF extraction is available through a separate local Python process with pypdf, discovered beside the executable, on PATH, or through WAYPOINT_PDF_PYTHON. The web UI runs document extraction in a background task; one PDF worker is allowed at a time, with 10 MiB input, 30 pages, 200 KB output, 8 MiB per-filter decompression and a 30-second timeout. A fictional PDF and malformed PDF were tested with the configured bundled Python. This is not OCR; scanned or encrypted PDFs require manual handling. The worker has no OS-enforced total memory ceiling, and packaging or installing its runtime and pypdf on other hosts remains incomplete.

A shortlisted job can request a temporary cover-letter text draft using an already-resident eligible Ollama model. The worker uses up to 25 attested qualifications, bounded job text and document instructions, and optionally extracted cover-template text as style guidance. It performs a mandatory drafting pass followed by a natural-language style pass; both must return per-paragraph references to supplied facts. Citation indices are checked, but semantic support and factual accuracy are not guaranteed. The draft is editable in the review panel, is not saved or attached, does not preserve template layout, and is not a final PDF. A fake-model test verifies the two passes and citation bounds. Ollama reported no loaded models, so no live-model test occurred.

Reviewed cover-letter text can now be explicitly exported in a background task to a saved PDF named for the employer and role. Conversion uses the existing single LibreOffice worker and a temporary DOCX; the PDF can be downloaded and attached through the document library. A fictional example was exported, saved, reopened and visually inspected. This uses a plain layout, not the template layout; it does not yet visually edit a supplied template, create a tailored CV, or automatically associate final documents with an application record before attachment. The LibreOffice worker still has no OS-enforced memory ceiling.

A shortlisted job can now request a CV text draft using the selected CV template text, document instructions, up to 18 attested qualifications and up to seven supplied contact fields. The local model performs the same drafting and natural-style passes and cites supplied inputs; the user must verify semantic accuracy and completeness. Reviewed CV text can be exported to a saved, job-named PDF through LibreOffice. The current PDF uses a plain layout and does not preserve or visually edit the template. Fake-model tests exercised both passes and a fictional CV was exported, text-checked and visually inspected. No real model or human CV was used.

Windows builds now create a notification-area icon with Open Waypoint and Exit actions; release builds use the Windows subsystem instead of a console window. Exit signals the loopback server to finish its current request, then drops the browser session. Background tasks may be interrupted when the process exits. A focused test verifies the server releases its listener, and the web suite passes; the actual Windows notification-area interaction has not been verified. A local, hidden, isolated smoke launch was rejected by the automatic command policy. The Windows installer, single-instance behavior and dependency provisioning remain incomplete.

The job list now accepts a user-entered public HTTPS URL, title, employer, location and optional pasted description without opening the site. Job and description are inserted together; exact duplicate URLs retain their existing state. Known platform domains retain the corresponding source-policy category. The pasted description participates in the existing lexical ranking and draft context, but is not automatically fetched or independently verified. Local unit and browser fixtures used fictional data only.

The web discovery controls now support 1–10 explicit Greenhouse, Lever or Ashby boards in a background run. Boards are fetched sequentially, with 15-second fetch timeout, 4 MiB response cap and a one-result queue; the UI persists and displays each board as it arrives and reports failures separately. Cancellation stops after an in-flight fetch, and the active task id is kept in session storage for page reloads. This still requires user-supplied board names and is not general web search. The legacy single-board API remains synchronous. An offline fixture covered one successful board plus a degraded board, and a local UI fixture covered incremental display; no live job boards were queried in this round.

Jobicy keyword search adds a public remote-job source. Results retain the Jobicy listing URL and attribution, rather than claiming that link is an employer application. The first page is capped at 50 results and the persisted rate reservation limits requests to one per hour. The connector and background discovery were tested with fictional offline fixtures, and the UI with a mocked local page; no live job search was run. This is not global web search or support for all major boards.

The wider-web search panel now accepts a user-configured SearXNG-compatible provider and a free-form query, including queries suggested by the local model. It performs one background request with a 15-second socket timeout, a 2 MiB response limit, and at most 30 displayed candidates. A public provider must use HTTPS; a user-specified local provider may use literal `127.0.0.1` over HTTP. Results are explicitly unverified links: selecting one prepopulates the manual job form, where the user supplies an employer and can check the source before adding it. No candidate page is opened automatically. A local fake provider and mocked browser fixture exercised the request and review flow; no public provider or live job search was used. A configured SearXNG instance is required, JSON may be disabled, coverage depends on its engines, and this does not yet automatically parse thousands of job sites or verify application panels.

Each wider-web candidate now offers an explicit background inspection that reads a single schema.org `JobPosting` JSON-LD object from the selected page and saves its available title, employer, location and normalized description as a job, marked for manual review. It requires a public HTTPS URL and source read permission, checks DNS before launching a single browser, and marks cross-origin redirects, blockers, missing fields or multiple job objects for manual handling. It does not establish that the job is legitimate, still open, or has an accessible application panel; the separate application-page check remains necessary. A fictional browser DOM fixture and mocked UI flow passed, with no external job site contacted. JSON-LD is not universal, and this is not yet broad automatic crawling or semantic extraction of arbitrary page layouts.

When a page contains no `JobPosting` JSON-LD, an already-resident eligible local Ollama model may extract a role, employer, location and description from up to 16,000 bytes of normalized DOM text. Accepted values must be exact excerpts; the page and application route remain flagged for review, including a warning when the text sample was truncated. The source text may include content hidden by CSS, and exact quotation does not prove the model identified the correct job or that an excerpt is a complete description. A fictional browser page and fake model verified extraction and rejection of invented text; no real model or employer page was tested. The browser's full DOM retrieval itself does not have a hard memory cap.

An explicit application-page check for a discovered job opens one headless browser in a background task, without using a model, entering data, clicking controls or submitting. It can follow one unambiguous same-origin application anchor with no query or fragment; ambiguous, cross-origin and final-action-looking links are marked for manual review. Redirects are reported for review after navigation. It reports blockers and empty pages for manual review; detected form controls are explicitly not proof of an application. A fictional local browser fixture exercised link extraction and opening a form without submission, and a unit test exercised the destination policy. Jobicy listings require the user to find an employer URL. The check does not identify profiles, persist successful results, or establish that a multistep application can be completed, and it has not been exercised against a real employer. As with any page load, the website's own scripts may make network requests. The single-browser restriction limits concurrency, but is not an operating-system memory limit.

Saved AI fit estimates now appear as a separate badge on job cards when those cards approach the viewport. The browser requests at most 20 saved assessments per batch, sequentially; each is rechecked against its bounded inputs before the score is shown. Stale assessments show a warning in place of a score. A fictional UI fixture covered current and stale badges, and a Windows workspace test covered invalidation after changing search preferences. This does not automatically assess all jobs, and the on-demand assessment remains an unverified model estimate.

The user can assess up to ten selected jobs sequentially from the job list. The browser drives the existing one-at-a-time local-model worker, records progress in session storage and can resume a running task after a page reload; stop takes effect after the current assessment. The page must remain open for subsequent jobs to start. Each saved result uses the existing input-staleness check and Windows-protected storage; errors or unsaved results are counted for attention. Mocked UI fixtures covered two sequential results and reload resumption, with no live model or external site. This does not automatically assess every discovered job or submit applications.

The form-review panel now allows the user to enter answers for unmapped, non-sensitive text, email, textarea and native single-select fields. Values are included only in the explicit reviewed fill request; the live schema and destination are checked again, and the per-attempt record includes requested answers. Sensitive fields still require manual handling in the visible browser. A fictional browser fixture verified a user-entered email was filled and recorded without submission, and a mocked UI fixture exercised an unmapped textarea. This does not answer arbitrary application questions automatically or cover custom widgets.

A non-sensitive unmapped text question can request a local-model draft grounded in up to 20 reviewed qualifications and bounded job text, followed by a second natural-language style pass. Both passes must cite valid qualification numbers; citations do not establish semantic support. The form and supporting inputs are checked before and after drafting. The proposed answer is displayed with its sources and enters the editable field only when the user chooses it; filling remains a separate review action. A fake-model test covered two passes and citation validation, and a mocked UI test covered reviewing, choosing and filling a draft. No live model or employer site was used; sensitive questions and unsupported controls still need manual handling.

The reviewed fill panel also accepts a user-entered native date, such as an available start date. The server validates the calendar date; the browser checks the native control, min/max validity and retained value before reporting success. A fictional browser fixture verified a valid date and rejection of an out-of-range date; a mocked UI fixture covered date entry. Sensitive date fields remain manual, and custom date widgets are not supported.

Non-sensitive native checkboxes can be explicitly set to checked or unchecked in the form-review panel, or left unchanged. The browser verifies the control type and final state, and dispatches input and change events only if the value changes. A fictional browser fixture verified clearing an initially checked control, and a mocked UI fixture covered an explicit unchecked answer. This is user-directed; consent and other sensitive fields remain manual, and custom checkbox widgets are unsupported. Page event handlers can transmit data before submission.

The review panel can present a native radio group when it is inside a fieldset with a legend and named, labelled options. The user chooses an option, and the browser checks the group and unique option before setting it. Sensitive questions, groups without that structure and custom radio widgets remain manual. A fictional local-browser fixture verified switching between two options, and a mocked UI fixture verified selection; neither contacted an employer. As with other fields, site event handlers may transmit information before submission.

The Windows web-app build now enables SQLCipher by default and opens the encrypted workspace with a DPAPI-protected key. Existing plaintext workspaces are copied rather than deleted, so the retained original remains readable and is reported by the interface. The Windows build helper locates a compatible developer OpenSSL and places its runtime DLL next to the executable; it does not install dependencies. The default web-app suite passed with the host OpenSSL settings, the helper built the executable, and a fictional encrypted-workspace migration and reopen test passed. This does not qualify an installer, other hosts, crash recovery, or automatic removal of old plaintext copies.

A prototype Windows ZIP packaging script now checks hashes against the local build receipt, copies the executable and matching OpenSSL DLL, and collects the available local license files for the Windows runtime dependency graph. An included PowerShell script can copy the extracted package into a new per-user directory and optionally create a Start Menu shortcut; existing installation directories and shortcuts are not overwritten. A debug ZIP was checked for contents and installed into a fictional folder inside the repository with shortcut creation disabled. This is unsigned, dependency license coverage has not been legally reviewed, and Visual C++ runtime, Chromium, Ollama and document tools are not provisioned. The packaged executable has not been tested on a separate Windows host; no release ZIP has been produced in this check.

Duplicate candidate discovery now indexes up to eight normalized employer words per job, including non-ASCII words, so reordered employer names can be compared. A matching requisition identifier or very high description overlap can flag different role titles for review. The app indexes older records incrementally during idle periods, one job per two seconds, and reports while indexing remains incomplete. Comparisons remain bounded to 200 candidates per job and do not cover translations, paraphrases or all employer aliases; this is not a guarantee against duplicates. Fictional tests covered reordered Unicode employer names, matching requisitions, different titles and older-record backfill.

For an existing pending duplicate pair, the user can request an advisory local-model comparison of the two bounded listing texts. Quoted excerpts must appear in the supplied texts, and the result is discarded if either listing or the pending decision changes. The model never changes the duplicate decision or releases the application hold; its reasoning can still be wrong. A focused validator and decision-change test and a mocked UI fixture passed. No live model or employer listing was used, and this does not identify pairs the initial candidate search misses.

A user can request a local AI duplicate search for a saved job. It selects up to 12 undecided listings sharing indexed employer words, with at most 100 entries scanned per token, and refuses to run while old-job indexing is incomplete. The local model must return valid candidate numbers and exact excerpts; the server rechecks the candidate snapshot and adds flagged pairs to the pending review queue without overriding prior decisions. A fake-model validator, store test and mocked UI fixture passed. No live model was used. The bounded, employer-token-based candidate selection can miss aliases and later candidates, and a model can miss or incorrectly flag vacancies; this does not guarantee duplicate prevention.

The wider-web panel can now search up to ten explicitly entered phrases sequentially through a configured SearXNG-compatible provider, with one request in flight and a one-second gap. Results appear per query, duplicate URLs are suppressed within the batch, and individual failures remain visible; the browser can resume polling after reload or request cancellation. Results are unverified links, not confirmed jobs or application forms. A fictional loopback provider exercised two queries and URL deduplication, and a mocked UI fixture covered incremental display. No public search provider was used. Provider availability, paging and coverage remain limited; no application is sent.

The wider-web results now support selecting up to ten links for sequential listing inspection, with an optional application-page check after each saved listing. The UI shows extracted-listing and check outcomes, supports stopping after the current page, and resumes an in-progress task after a page reload. It reuses the existing one-browser limit and source rules. Mocked local UI fixtures covered two listings, checks and reload resumption; no public page was contacted. The batch is driven by the open browser page, results do not prove a valid or completable application, and difficult links remain manual review.

Inspecting a wider-web listing now captures up to five bounded PNG page segments (at most 1,600 pixels wide, 8,000 pixels tall and 6 MiB total). The first capture is protected with DPAPI in the job database; when an application preparation record begins, its images and capture metadata are copied into original-job-page beside the saved listing text. Partial or unavailable captures are flagged for manual review. Captures can omit hidden content, nested scrollers or changing content and do not cover jobs added without page inspection. A fictional browser fixture, DPAPI storage test and attempt-record test passed; no employer site was contacted.

The document review panel can now replace one unique single-line passage in a DOCX body, including text split across Word runs, and save both a new working DOCX and a PDF in one database transaction. Untouched archive entries are preserved; the replacement uses the first affected run style, and the original remains unchanged. A local-model suggestion can use reviewed qualifications and a selected job, with a second style pass and cited sources; the user must explicitly choose and review it. Ambiguous, cross-paragraph and unsupported passages are rejected. A fictional formatted DOCX was edited, converted with LibreOffice, rendered and visually inspected; fake-model and mocked UI tests covered the suggestion and review flow. This does not visually inspect automatically, edit PDF templates directly, support every DOCX structure, guarantee unchanged layout or verify model claims.

An isolated PDFium experiment edited a text object in a fictional LibreOffice-generated PDF. Text extraction after saving reported the requested replacement, but the rendered page omitted a character, apparently because the original embedded font lacked that glyph. Extraction alone therefore cannot validate visual fidelity. Direct PDF editing remains unavailable; a future implementation needs visual or glyph-coverage checks, layout review, and explicit manual fallback. An embedded PDF preview was also tried in a headless Edge fixture, but the frame did not render a page in that test and was not integrated. No PDFium dependency was added to the application.

Saved PDFs can now be previewed one page at a time as locally rendered PNG images. Rendering uses the existing single PDF worker and requires pypdfium2, Pillow and pypdf in the configured Python runtime; they are not yet provisioned by the installer. Limits include 10 MiB input, 30 pages, approximately 3.2 million rendered pixels, 3 MiB PNG output and a 30-second worker timeout. A fictional PDF was rendered through the Rust worker and inspected, and a mocked local UI fixture covered preview and closing. Resolution is capped; preview is for human review, not automated visual validation or direct PDF editing. There is no OS-enforced worker memory ceiling.

PDF tools can now be installed on request from the web interface into a new per-user Python virtual environment. Setup uses a user-specified existing Python with venv, installs pinned binary wheels for pypdf 6.10.0, pypdfium2 5.14.0 and Pillow 12.3.0 from PyPI, and activates the environment only after an import check. One setup runs at a time; steps have timeouts and partial environments are left in place if setup fails. A real installation into a temporary folder inside the repository succeeded with a local Python 3.12 runtime. Python itself and the Visual C++ runtime are not installed; other Python versions and hosts are unverified. An explicit WAYPOINT_PDF_PYTHON override takes precedence over the managed environment. Installation and PDF processing still lack OS-enforced total memory limits.

Application inspection now starts a plaintext preparation record even when no field has yet been filled, and records failed inspections as manual review when a disk workspace is available. Each successful inspection or rescan records the current origin, review reasons and available viewport screenshot in the same attempt folder. Missing screenshots and recording failures are surfaced for manual review. Closing a session keeps an existing manual-review status; expiration or replacement marks the session interrupted. External submissions in a visible browser are not monitored, so submission status is unknown. A focused record test and the existing fictional no-submit autofill browser fixture passed; no live employer site was tested. Inspection screenshots are viewport captures and can omit off-screen content.

The browser now resolves ordinary form labels referenced through aria-labelledby, including multiple IDs and labels located after the control. Missing or duplicate references require manual review; radio options using such references remain manual. Text autofill checks that the input remains connected, editable and contains the exact requested value immediately after typing. Truncation and immediate page-side clearing now stop autofill for manual review. Local browser fixtures exercised these cases and the existing reviewed-prefill fixture passed without submission. This immediate check cannot guarantee that a site will not later change or transmit a value, and real employer layouts remain unverified.

Native radio inputs can now be grouped in a labelled ARIA radiogroup as well as a fieldset, and individual radio options can use aria-labelledby. The selected option must be unique, remain checked, and belong to the same form and group; similarly named radios outside the group cause manual handling. Sensitive group labels remain protected. A fictional local browser fixture checked both group types, change events, ambiguity and sensitive labeling without submission. Custom non-native radio widgets, unlabelled groups and real employer sites remain unverified or manual.

The local model can now propose profile-backed choices for non-sensitive native radio groups as well as dropdowns. Exact choices are matched locally; a second model request can propose a non-exact equivalent only if it is a unique option on the page. The user still reviews the choice before filling. Focused fake-model tests covered exact, translated, invented, duplicate and sensitive options, and a fictional local browser fixture exercised suggestion through reviewed fill without submission. No model was loaded during this check, so real-model interpretation remains unverified. Required fields without suggestions were already flagged for review.

Wider-web batch searches can now request one to five SearXNG-compatible results pages per phrase, up to ten phrases. Requests remain sequential with a one-second gap and stop after 600 unique candidate links; the interface shows page-level progress, failures and the cap. Pagination uses SearXNG's documented pageno parameter (https://docs.searxng.org/dev/search_api); individual engines may not support equivalent paging or return unique results. A fictional loopback provider exercised page numbering and cross-page deduplication, and a mocked browser fixture checked the page-count control. No public provider or employer site was queried; links remain unverified.

Listing inspection now preserves bounded JSON-LD script text from the live DOM in DPAPI-protected capture metadata, alongside existing screenshots. When an attempt record starts, the script texts are copied into the original-job-page folder as plaintext .json.txt files. The interface reports when structured source is unavailable or exceeds limits; existing first captures are not overwritten. This is a DOM snapshot of selected structured data, not the original HTTP response or a complete page archive. A fictional browser capture, encrypted-store test and attempt-archive test passed. No public job page was inspected. The user requires free tools and no paid APIs; external search providers must be free or self-hosted, and provider pricing is not automatically verifiable.

On Windows, the PDF processing subprocess is assigned to a job with a 512 MiB committed-memory limit; PDF environment setup uses a 768 MiB job. Both paths share a single in-process PDF lease, and the job closes after the worker. This uses the MIT-licensed ProcessKit 3.3.4 safe API. A controlled 64 MiB job rejected a 96 MiB allocation; fictional PDF rendering and a real isolated PDF-tools installation also passed. Process assignment happens immediately after launch, not atomically at creation, so there is a short uncontained window. Limits apply to these jobs, not the browser, the web server, Ollama, other app instances, GPU memory, or total system memory; they cannot guarantee freedom from freezes. Windows may reject job assignment in restrictive host environments, in which case the worker is stopped.

PDF workers on Windows now use ProcessKit's suspended launch: they are assigned to their memory-limited job before execution resumes, with no visible console. Output capture is bounded and timeouts remain enforced. A controlled immediate allocation test, output-cap and timeout checks, fictional PDF rendering, and a real isolated PDF-tools setup passed; the web suite passed 52 tests with 18 ignored. If the parent dies between creation and job assignment, ProcessKit documents that a suspended child can remain orphaned. The limits still do not cover the browser, Ollama, GPU or total system memory, and do not guarantee freedom from freezes.

LibreOffice conversion on Windows now runs under a 768 MiB committed-memory job, launched suspended and assigned before it executes. It shares the PDF worker lease, preventing concurrent preview, extraction, PDF setup and conversion within one app process. A fictional DOCX converted successfully under the limit and its rendered PDF was visually inspected; a focused check covered the shared lease. Complex documents or other LibreOffice versions may need more memory and are unverified. These limits do not bound browser, model, other instances, GPU or total system use.

Document review now supports a limited direct edit of selectable-text PDFs: one unique single-line passage contained in one top-level PDF text object can be replaced and saved as a new PDF. The original is retained. The worker rejects encrypted, interactive or signed PDFs, ambiguous or multi-object text, output text changes outside the edit, replacement bounds outside the original object, and detected zero-area or unverified glyphs. It does not guarantee visual fidelity or detect every missing glyph; users must preview the entire result. PDFium and pypdf remain required. A fictional PDF passed a reviewed edit and visual inspection, while missing-glyph, oversized replacement and absent-passage cases were rejected; the DOCX edit regression and mocked PDF UI fixture also passed. No real CV or live model was used.

The direct PDF edit worker now renders each original and edited page at a bounded resolution and checks for pixel changes outside the original text object, with a small antialiasing margin around that object. Rotated target pages and unsupported crop geometry go to manual editing. A synthetic pixel fixture covered changes inside and outside the permitted area, and a fictional two-page PDF with a graphic was edited and visually inspected. Poppler reported missing display fonts for Symbol and ArialUnicode during that rendering, although both fixture pages were viewable. This comparison cannot prove accuracy inside the edited region or detect changes below its rendering resolution; users still need to review every page.

Two publicly hosted academic CVs (5 and 12 pages) were evaluated locally without adding the files to Git. Four short generic-heading edits were attempted; all four were rejected because PDFium regeneration changed pixels outside the target area. Therefore the current direct PDF editor is not demonstrated to work on these real CVs, and its visual guard was not relaxed. A separate source page timed out in Crawl4AI. The review interface now retains editing failures with an explicit manual/DOCX fallback and offers a direct preview button after successful saves; a mocked UI fixture checked both paths.

A targeted PDF content-stream edit path now handles some single-byte font mappings and passages split across strings in one text-show operator. It leaves surrounding operators in place, then retains the existing saved-text, glyph, bounding-box and rendered-page checks; unsupported structures fall back to the prior guarded PDFium method. The two previously rejected public academic CVs each accepted one different, short heading replacement with this method, and the edited pages were visually inspected. A synthetic split-operator fixture and the fictional PDF edit regression passed. Poppler again warned about Symbol and ArialUnicode display fonts for one CV. This is not evidence of broad PDF compatibility: longer edits, unsupported mappings, multi-object text, signed forms and other layouts still need review or manual editing. Public CVs and altered copies remain Git-ignored.

The document editor now queues up to five reviewed, single-line replacements and applies them in order to the selected original. Each PDF change is checked separately, with at most one bounded PDF worker running at a time; DOCX changes are converted to PDF once after all replacements succeed. If any replacement fails, no edited document is saved and the original remains available. A completed edit must be collected or retried before another starts. This is still passage editing, not unrestricted visual editing: supported PDF fonts, text objects and layouts are limited, and users must inspect every resulting page.

Application form attachment selection now lists saved PDF files and offers a local page preview. The server checks both a PDF filename and PDF signature before staging or attaching a file; this is a format gate, not full validation of its contents. DOCX files must first be converted through the document library and visually reviewed. Other input formats do not yet have an automatic PDF conversion path. Attaching may transfer a file to the destination before final submission.

The document library can convert single-frame PNG and JPEG images to one-page PDFs locally, including transparent certificate images (flattened onto white) and EXIF orientation. Conversion uses the existing single PDF-worker lease, a 512 MiB Windows job limit, a 30-second timeout, a 10 MiB input/output cap, and a 20-megapixel limit. Output is an image-based PDF, not OCR; page size uses 150 DPI and users must review the result. Other image/document formats and multi-page image conversion remain unsupported.

Form inspection now marks an application for manual review when it encounters a possible next-step button with a small set of common labels. This is a heuristic: it can miss other labels or custom controls and can include controls hidden by CSS. It does not determine the number of steps or click any control; the user can navigate in the visible browser and inspect the current page again.

After a reviewed fill, the form panel lists the labels handled by that fill and the required labels not handled by it. This is a record of the operation, not validation that the page is complete: a required field might have been entered manually or attached separately, and a site can subsequently change a value. The existing manual-review notices and screenshot remain visible.

Checkpoint validation (2026-10-28): `cargo fmt --all -- --check` and the offline workspace test suite passed with one build job and one test thread. The web crate reported 54 passed and 24 ignored; ignored cases were not covered by that suite. The subsequent workspace Clippy run with `-D warnings` is not yet clean: after correcting store, workspace and browser warnings it still reported 13 diagnostics in the web crate. The local changes remain uncommitted pending that cleanup.

Follow-up checkpoint validation (2026-10-28): after the warning cleanup, `cargo clippy --workspace --all-targets --offline -- -D warnings` and the offline workspace test suite passed. Two additionally selected browser tests used fictional local forms and did not submit. Other ignored tests remain outside the workspace suite's coverage.

Each job card can now list up to 50 matching local attempt records, including their folder locations and recorded preparation status. The scan inspects at most 2,000 directory entries and reports truncation or unreadable records. It does not establish whether the user submitted an application outside Waypoint.

Hosted checkpoint CI for cb44690 failed in the Windows build job before Clippy could run: libsqlite3-sys could not find OPENSSL_DIR. The Windows build job now configures the GitHub runner's OpenSSL headers, x64 MSVC library directory and runtime path explicitly; hosted verification of this change is still pending.

When no eligible local model is loaded or model mapping fails, form inspection can suggest normalized exact matches between field labels and saved profile keys. Choice values must match one unique page option. Sensitive fields, ambiguous options and non-exact labels remain for review; no form is filled without a separate user action.

If a local model task is already using the exclusive inference lease, initial form inspection and navigation rescans continue with exact-label suggestions and explain that the model is busy. This does not run another inference request or automatically fill the form.

Document qualification review now retains a completed in-process result until collected. The web page remembers an active review in session storage and can recover its excerpts after reload while the server remains running. A server restart still loses the in-memory task; no qualifications are saved until the user selects and adds them.

The job-card percentage is labeled as literal text overlap and excludes qualifications awaiting review. It is unavailable when there are no reviewed qualifications or the listing description is missing; the local AI assessment remains separate. Hosted CI for b08db5a showed a race between two tests sharing the single batch-search task; those tests now coordinate while leaving the production concurrency limit intact. The web-app suite passed with default parallel test execution after the change. The hosted supply-chain job also failed; it was not investigated in this development pass.

The dashboard reuses the description-availability result already obtained during ranking, avoiding a second description lookup per displayed job. This reduces duplicate database work but is not a measured performance benchmark.

New listing captures also retain up to 32,000 bytes of normalized DOM page text from the already-open page, with captured, truncated, empty, unavailable or size-limit status. When an attempt begins, available text is copied to original-job-page/page-text.txt alongside the existing screenshots and structured data. This is not a complete HTTP/HTML archive; page text may include hidden or dynamic content and omit material in frames or after the limit. Older captures do not gain this data retroactively.

Checkpoint validation (2026-10-28): the current workspace passed `cargo fmt --all -- --check`, `cargo test --workspace --offline` with the default parallel test runner, and `cargo clippy --workspace --all-targets --offline -- -D warnings`, using one build job. External-network and other ignored tests are not included in that result.

Reviewed CV and cover-letter PDF exports retain completed results until collected, including when a save needs retrying. The page remembers an active export in session storage and can recover it on reload while the server remains running; a server restart loses the in-memory task. The completed export offers a local page-preview button, and the user must still review the PDF before attachment.

New local CV and cover-letter drafts carry a digest of the saved job, description, qualifications, profile and selected template. The server checks that digest when collecting the draft, starting its reviewed PDF export and saving the result; changed or unavailable inputs require a new draft. This does not establish the truth of generated claims or check whether the employer has changed its live listing. The export and draft task are in-memory and do not survive a server restart.

An in-progress CV or cover-letter draft is now recoverable from the current browser session after a page reload while the server remains running. Draft collection still checks its source-context digest; a changed job, profile, qualification or template is reported as stale. The task itself remains in memory and is lost on server restart.

Form inspection no longer requires Ollama or a saved profile merely to start in headless mode. Without an eligible model it uses the exact-label fallback, marks unresolved requirements for review and exposes the page screenshot; challenges and complex navigation still need a visible browser or other manual handling. This change does not load a model or submit an application.

The local-model status control now distinguishes a reachable runtime with no model, multiple models, incomplete reports, a model not fully reported on GPU, an over-budget model, and a ready model. It reports Ollama's values as a snapshot rather than a reservation; it does not install, load, unload or benchmark a model.

After typing into supported native text, email, phone, URL, search or number inputs, the browser driver checks both exact value retention and the field's current native validity. An invalid value stops the fill and flags the session for manual review; it does not undo earlier input or guarantee that later page scripts will retain values.

After a reviewed fill sequence, the browser rechecks each filled native text-like or date field for its expected value and current validity. If a later field changed an earlier one, the fill is reported as incomplete and the attempt requires manual review. This is an immediate check; checkboxes, radio buttons and selects are verified at their own action, and later asynchronous changes can still occur.

The immediate post-fill recheck now also verifies reviewed native checkboxes, single-select dropdowns and radio groups without changing their values. This supplements the existing text and date check; asynchronous changes after verification remain possible and still require visual review.

On interrupted reviewed form filling, the plaintext attempt event records which field commands reported success before the failure. The failing command may also have changed the page, and earlier fields can be changed by the site; these records are progress evidence, not proof of final field state.

Interrupted form fills now also attempt the same bounded segmented page capture used after successful fills. Capture failures or pages exceeding the limits are recorded as incomplete; hidden content and nested scrollers may still be missed. The local browser fixture also exercises the full fill sequence where a later field clears an earlier one and confirms that it is reported for review.

A fictional CV export was converted with the local LibreOffice path and rendered for visual review after adding draft-context checks. On GitHub's 91d676c browser job, two browser launches timed out and one local-fixture submission test had a CDP connection timeout; the root cause was not established. Browser startup now reports an early process exit and its status rather than waiting until the port timeout. This diagnostic change has not reproduced or fixed the hosted failure.

The setup panel can run an on-demand PDF dependency check in the bounded local worker. It imports pypdf, pypdfium2 and Pillow and reports their installed versions; a successful import does not guarantee every PDF will be supported. The check uses the configured Python runtime, performs no installation or network request, and can be resumed after a page reload while the server remains running.

Form schemas now retain bounded HTML autocomplete attributes and include them in their identity. The local model sees those hints, and the no-model fallback can use a small set of standard contact and address tokens alongside exact labels. Conflicting direct labels and hints, sensitive fields, unknown tokens and ambiguous select options remain for review. Hints are site-provided and do not authorize filling or prove semantic correctness.

A local browser fixture now verifies that an autocomplete-derived suggestion can be reviewed and filled while a sensitive field is left untouched and no submission occurs. A changed autocomplete hint changes the form schema identity, so an existing reviewed fill cannot silently apply to the changed form.

Shortlisted or prepared jobs can be selected into a guided review queue of up to 20. The user explicitly opens each next application and closes the current browser review before proceeding; the queue can be skipped or cleared. It exists only in the current page and does not fill or submit applications by itself. Existing per-job duplicate and destination checks still apply.

The guided review queue now persists job IDs in the current browser session across reloads. On reload the page checks the server for an active review and offers to resume or close it before continuing. Queued inspections also require the server to confirm that no unexpired review session is active, preventing a stale page from replacing one. The queue does not survive a closed browser session and does not submit applications.

The application review and recovery cards now precede the optional local-tool setup controls, which are grouped under a disclosure. Queue and recovery actions have explicit spacing and wrap on narrow screens. A local Playwright fixture exercised queue, reload, close and next-job flow at desktop and 390 px viewports; it used fictional data and no external application.

Checkpoint validation (2026-10-28): workspace formatting, offline tests with the normal parallel test runner, and workspace Clippy with warnings denied passed after coordinating the single-search and batch-search fixtures. The web-app suite reported 60 passed and 28 ignored. These checks do not resolve the earlier hosted browser timeouts or hosted supply-chain failure.

The review queue controls can now select visible shortlisted or prepared jobs and clear that selection separately from the discovery shortlist selection. Selection is capped at 20 and reports when visible jobs were omitted. Selection alone does not inspect, fill or submit any application.

Form inspection also recognizes common Cloudflare Turnstile widget markers as a challenge requiring manual review. A local browser fixture verifies detection without contacting a challenge service. This heuristic cannot identify every challenge implementation.

Required custom ARIA controls, such as a non-native combobox, are flagged for manual review rather than omitted silently when native fields are also present. Their detection depends on the page exposing a supported role and aria-required; custom widgets are not yet filled by the driver.

When a local model omits a field from its mapping, exact label and standard autocomplete matches can supplement its suggestions. Existing model mappings retain precedence, and all suggestions still require review before filling.

If the saved profile changes during a review, a fill request now leaves the browser session open and asks for the current page to be inspected again. This lets the candidate retain their place and review suggestions based on the updated profile.

Suggested form values can be edited or omitted in the review panel before filling. An edited choice is checked against the live form's available options, and the actual reviewed answer is saved in the attempt record.

An invalid reviewed answer leaves the current session and original suggestions intact, so it can be corrected and retried without reopening the application.

Native select fields now expose enabled options inside option groups for review and autofill, including explicit display labels. Disabled options and disabled groups are excluded; a local browser fixture exercised selection without submission.

Native multiple-select questions can now be answered in the review panel using individual checkboxes. The driver checks that each choice is unique and available, fills the selection, and verifies that the page retained it. Local fixtures exercised the panel and browser without submitting; custom multi-select widgets still require manual handling.

For long multi-select lists, the review panel renders at most 40 choices at a time and offers paging and a filter. Selections remain checked while the filter or page changes. This limits the panel's rendered controls; it does not bound the memory consumed by the page or browser itself.

Application-link inspection resolves bounded accessible label references and uses a nonempty accessible label in preference to visible text, rather than concatenating them. Missing or ambiguous references are not guessed. The destination still requires user review before navigation; the fixture only reads local links.

Form selectors now quote generated IDs and escape attribute values when needed, so punctuation, leading digits, and quotes do not make otherwise supported fields inaccessible. A local browser fixture filled two such fields without submission.

Browser protocol messages are limited to 16 MiB, and the retained event buffer is limited to 64 events of at most 64 KiB each. Older non-load events and oversized events are discarded. An oversized protocol response fails inspection and may require manual review. These limits do not cap the browser process or the operating system's total memory use.

Each browser protocol command now sets its own remaining read deadline, rather than inheriting a short timeout used to poll page events. A loopback fixture exercised a response arriving after that short poll interval.

The browser process is now owned immediately after launch, so startup errors after spawning trigger the normal process and temporary-profile cleanup path. A failed process spawn also removes the newly created profile directory.

Page-node and text traversal use an explicit depth-first work stack, reducing call-stack pressure on deeply nested pages. This does not remove the browser protocol's message-size limit or cap the browser process itself.

Unparseable or excessively nested browser responses now produce an explicit inspection error instead of being ignored until another response or timeout. The parser's nesting limit still applies.

Controls carrying the HTML hidden attribute or inside an element with that attribute are excluded from form inspection. The driver also checks rendered visibility for native controls and uses structural selectors when hidden and active fields share identifiers. A visually hidden file input remains available when its parent is rendered. Visibility inspection is limited to 160 controls and checks an eight-second budget between fields; a form that exceeds either requires manual review.
Required custom ARIA widgets and possible next-step buttons are also checked for rendered visibility. Visual state such as opacity and overlays is not fully modeled.
Required inputs with combobox and other complex widget roles are treated as custom controls too; a typed value alone may not select a valid option.

Gmail can also be connected with an app password. Waypoint checks SMTP and IMAP authentication on a background task without reading or sending a message and saves successful credentials with Windows DPAPI. An explicit inbox check can retrieve total and unread counts without opening messages. Actual in-app email delivery and message reading are not implemented. A live test account authenticated through the local API, retrieved inbox counts, and disconnected in a temporary workspace; no account details are stored in this repository.
Starting a new inbox check replaces an uncollected completed count, while a still-running check remains single-instance.
App-password connection can be cancelled; cancellation prevents new credentials from being stored. An in-flight network operation may need to finish or time out first.
An uncollected app-password authentication result expires after ten minutes once another connection operation checks it. Expiry discards the result without storing the credential; a running operation is not replaced.
OAuth connection results use the same expiry rule, so an abandoned completed consent flow does not indefinitely block reconnection or disconnection.
OAuth connections can also request inbox counts using the Gmail labels API. The response must identify INBOX and contain consistent total and unread counts. A loopback fixture exercised token refresh and the read-only request; OAuth connectivity has not been tested against a live authorized account. The app-password path was tested separately.
The endpoint and fields were checked against Google's native Gmail API discovery document at `https://gmail.googleapis.com/$discovery/rest?version=v1` (2026-10-28, local ignored capture `prototype-local/gmail-discovery-20261028.json`, SHA-256 `5b31d46375e98c88594c810d651e55ee9a4c9b8b3bc85086611f0a570b85ca9b`).

The unsent email editor can request a two-pass local-model body suggestion using the selected job, reviewed qualifications, and writing instructions. The suggestion displays its cited qualifications and must be explicitly copied into the message; the candidate must check its claims. If the supporting data changes before the result returns, it is discarded. A pending suggestion can be recovered after a page reload during the same browser session; other unsaved email text is not preserved. The feature still requires an eligible already-loaded local model.

A fictional two-section browser fixture checks that inspecting again after the section changes detects and fills the newly visible field. Navigation between steps in the product remains manual; the fixture's local button is not an employer application.

The multi-step review heuristic also recognizes visible custom controls with `role=button` and bounded, unambiguous `aria-labelledby` names. Unresolved referenced names are flagged for manual review. A local Chromium fixture verified detection without clicking or submitting; this still cannot identify every next-step control and does not advance the application.

Manual-review reasons for required custom controls include a bounded `aria-label` when provided, helping locate the question in the browser. Controls without such a label remain generic; detection does not fill custom widgets.
Bounded, unambiguous `aria-labelledby` references can also identify these questions. Missing or duplicate references leave the reason generic rather than guessing a label.

Pending local-model form answers can be recovered after a reload in the same browser session. The recovered answer and its sources appear separately for review; they are not inserted into a newly inspected form. The existing server-side freshness check can reject an answer when the original session, page or supporting information changed. Unsaved form edits are not recovered.

Required custom widgets remain marked for manual review, but no longer prevent separately reviewed native fields on the same page from being suggested or filled. Other walls still block filling. The driver rechecks the form and its custom-widget review reasons before acting. A local Chromium fixture exercised this without submission.

A user-authorized self-addressed test message was accepted by Gmail SMTP and found in the test inbox using IMAP. This used a separate local test client; in-app Gmail delivery is not yet implemented or verified.

The app-password settings now also offer an explicit self-addressed Gmail test. Its recipient and body are fixed and cannot send an application. A live test using a temporary DPAPI-protected workspace exercised the Rust self-test transport and found that specific message in the inbox via IMAP; a mock browser fixture exercised the button. This did not test general application-email delivery, which remains unimplemented. A lost SMTP response after message data is reported as uncertain, with no automatic retry; a restarted app cannot recover the in-memory test result, so check the mailbox before retrying.

The self-test now uses a reusable bounded SMTP handoff component that validates envelope addresses and CRLF-framed ASCII MIME, dot-stuffs DATA, and distinguishes failures before DATA from uncertain handoff. A local fixture exercised a generated PDF attachment and uncertain response; the self-addressed Gmail test was repeated successfully after the refactor. Application email is not wired to this transport: durable authorization, journaling and recovery must be integrated before exposing it.

Store migration 16 adds email-handoff reservations bound to message, sender and recipient hashes, plus append-only outcome events. A reserved, accepted or uncertain handoff blocks another active reservation for the same job and for directly linked listings marked as the same job; unresolved duplicate decisions also block reservation. A disk-backed fixture checked restart and directly linked duplicate behavior. This is infrastructure only: it does not create or consume an approval, check every possible duplicate relationship or browser submission, or invoke SMTP. The future application workflow still needs those checks, atomic approval integration and user-facing recovery.

Migration 17 adds a separate email approval record. Reserving a handoff consumes a matching, unspent approval for the same job and exact message, sender and recipient hashes in the same transaction; it expires after ten minutes. Tests cover changed hashes, expiry and transaction rollback. No UI or model can create these approvals yet, and the records are not connected to SMTP. The caller must establish an explicit user gesture, recheck the live reviewed content, and address broader duplicate and browser-submission history before an application send can be exposed.

The email reservation check now traverses chains of listings marked as the same job, checks related job states and browser submission journal entries, and stops for unresolved duplicate decisions. Traversal is limited to 500 related listings and fails closed when the limit or an unknown state is encountered. A fictional store fixture covers a three-listing chain and uncertain browser history. This only covers relationships already recorded in the database; discovery can still miss duplicates, and no application-email send path is exposed.

The workspace now has an approval-bound email handoff coordinator: it hashes the exact MIME bytes and envelope, reserves and consumes the matching approval, invokes a supplied transport once, and records its outcome. An unexpected transport panic is uncertain. A fictional test verified changed recipients are refused before the transport and uncertain outcomes cannot be automatically retried. The coordinator has no built-in network client or UI endpoint; the future caller must run it on a bounded worker, preserve the attempt record and supply an explicit review gesture. No application email was sent.

An application-email adapter now constructs one immutable MIME packet and connects the approval-bound coordinator to the Gmail app-password SMTP transport. A fictional in-memory SMTP fixture tested a PDF attachment, a recorded handoff and rejection of a second attempt. The send path is not exposed: the application must still connect the saved packet to genuine user review, run the transport in a bounded worker and provide recovery before exposing application sending. Actual application email has not been sent or tested.

The email editor can now prepare an unsent Gmail application packet with the connected app-password sender. It downloads the exact MIME file and saves that file, its SHA-256, selected PDF materials and the saved job snapshot in a plaintext attempt folder. A fictional disk-backed API test and a mocked browser test exercised preparation without sending. The saved packet is not yet connected to approval or delivery, and Gmail OAuth is not supported by this preparation path.

Newly prepared Gmail packets also save review details in the attempt folder. The application-record panel can reopen these details after a reload, reconstruct the MIME from the saved message and PDF materials, and refuse review when it differs from the saved packet or hash. This detects accidental changes, not malicious coordinated edits to plaintext records. Earlier packets do not have the new review metadata. Waypoint does not monitor whether a downloaded draft was sent separately in a mail client.

The saved-packet review can record an explicit ten-minute, one-use email approval without sending. The server reconstructs the packet again and checks its hash, the review context, the connected Gmail sender and duplicate hold. Migration 18 binds approval to a conservative job, profile and fact snapshot and invalidates outstanding approvals on changes; previous email approvals cannot be used after migration. Fictional disk-backed and mock-browser fixtures exercised stale-context rejection and the review gesture. Approval is not yet connected to application delivery; no application was sent.

A single-instance background email worker now reserves and consumes approval before starting a transport, then records its outcome when polled. A local simulated transport verified reservation precedes execution and the result is recorded once. There is no application-send route or UI for this worker yet; an unpolled or interrupted handoff remains reserved for recovery. No application was sent.

The worker now reloads the saved packet and, after recording an outcome in the database, attempts to copy it to `email-handoff.json` in that attempt folder. The worker reports whether that copy succeeded; the database remains authoritative. The application-record panel displays recorded email handoff states, including unresolved reservations after a restart, and cautions against retrying until checked. A disk-backed simulated worker test and mock UI test exercised these paths. Application sending is still not exposed.

Migration 19 and the application-record panel add explicit reconciliation for reserved or uncertain email handoffs. After checking the mailbox, the user may report that the message was sent or not sent; the action sends nothing, records the report in an append-only database table and attempts a separate folder copy. User reports are not independent delivery evidence. Reconciliation is blocked while a worker or uncollected worker result is active; reporting not sent does not automatically retry and a new approval is required. Store and fictional UI fixtures exercised these behaviors; application sending remains unavailable.

After reviewing and approving a Gmail packet, the user can run a no-send rehearsal. It reconstructs the saved packet, checks the connected sender and the normal approval and duplicate reservation gates in a rolled-back transaction, and runs SMTP framing against an in-memory fixture with no network access. It does not consume the approval or create a handoff; it attempts to save a rehearsal report in the attempt folder. A disk-backed fictional test ran it twice and confirmed no handoff was created, and a mock-browser test exercised the control. A later real handoff must recheck all conditions; this does not test Gmail connectivity or employer delivery.

The server loop now collects a completed email worker without requiring the browser page to remain open. A disk-backed simulated worker test exercised collection through the server loop and verified the recorded outcome after reopening the workspace. Application records also warn when an authoritative database outcome or reconciliation has no corresponding folder copy. A crash before collection still leaves a reserved handoff for manual reconciliation. No application-send route is exposed.

Duplicate comparison coverage is now tracked separately from the completion marker. The interface reports pending jobs, comparisons limited by the eight-token or 200-candidate bounds, and listings without usable employer tokens; it refreshes counts while indexing is pending. Migration 20 resets old completion markers for bounded background reindexing without deleting jobs. Tests covered a capped candidate set, a missing employer and migration of a legacy marker. Even a completed bounded pass is heuristic and does not prove that every duplicate was found; these counts are not yet a send-authorization gate.

Migration 21 binds saved Gmail review approval and no-send rehearsal to the duplicate-coverage revision. Pending indexing blocks approval and rehearsal; limited comparisons or missing employer information require an explicit acknowledgement. Changes to indexed jobs, coverage, or duplicate decisions invalidate existing approvals. A focused store test and fictional API and browser fixtures exercised the gates. Duplicate matching remains heuristic, and application delivery is not exposed.

Initial form inspection and each rescan now also save bounded page segments in the application record, in addition to the viewport image. At most six segments cover up to 2048 by 12000 CSS pixels; oversized or failed captures mark the attempt for manual review. A local headless-browser fixture verified a two-segment capture without filling or submitting. Hidden content and nested scrollers may still be omitted, and no employer site was tested.

Form inspection now flags visible required contenteditable rich-text fields and custom ARIA switches and searchboxes for manual review, alongside supported native controls. A fictional headless-browser fixture checked rich-text and switch detection, including a hidden editor; searchbox handling follows the same role check but was not independently exercised. These custom widgets are not autofilled, and markup without a required signal can still be missed.

Explicit `contenteditable="plaintext-only"` editing hosts with no unsupported role are now offered as text areas in reviewed form filling. The driver checks editability and focus, replaces the text, and verifies the exact retained value; editors with nested form controls or nested editing hosts are refused. A fictional browser fixture exercised multiline text and the existing reviewed-prefill fixture passed. Rich-text editors remain manual, and this has not been verified on an employer site.

After a reviewed form fill, the web panel attempts an asynchronous visual transcription of the current viewport using the single already-resident local Ollama model only when it reports vision capability. It shows the exact image sent and compares uniquely labelled observations with up to 20 reviewed text answers; missing or ambiguous observations remain unobserved. The result is advisory and the job is marked for manual review regardless. A bounded fake local HTTP model fixture checked the image request and comparison, and the existing local browser prefill fixture passed. No vision model was resident during this checkpoint, so real model accuracy and employer sites remain untested. Off-screen content, other input types, visual changes after capture and hallucinations can be missed; this cannot guarantee error-free applications or authorize submission.

Documents now offer an on-demand local CV diagnostic against a selected saved job. It reports the first twelve extracted lines, frequent literal posting words, and DOCX table, text-box and header indicators. A fictional DOCX fixture, fictional PDF extraction fixture and mocked browser panel exercised this initial check. This is a snapshot and a heuristic, not a prediction of any employer's screening decision; user-directed rewriting and full visual iteration are not yet integrated. Research and licensing notes are in `docs/ATS_RESEARCH.md`.

A saved PDF can now be visually reviewed against a selected job using a single already-resident local vision-capable Ollama model. The worker renders pages sequentially, asks for bounded layout, legibility and content concerns using the saved posting and reviewed qualifications as context, and reports each page separately. The panel links to the PDF preview and document editor so the candidate can make a correction and run the review again. It processes documents up to the existing 30-page PDF renderer limit and refuses longer documents as incomplete. A fictional two-page PDF was rendered with a fake model, validator and page-limit tests passed, and a mock browser exercised the findings panel. No model was resident, so actual model behavior and visual accuracy remain unverified. Findings are advisory, do not prove a document is accurate or ATS compatible, are not yet automatically attached to a per-application record, and do not prevent using an unreviewed document.

Each newly started PDF visual review now creates a separate job-linked preparation record containing the saved job snapshot, the exact PDF copy, a start event and, on completion or failure, an advisory review report. Completed reviews remain available until collected and can be reopened from the job's application-record panel; reopening checks the PDF copy against the recorded hash. This detects accidental changes, not coordinated edits to plaintext records. A failed or interrupted review remains manual; a crash before report writing may leave only the start event. A disk-backed fictional fixture exercised reopening and changed-PDF rejection, and a mocked browser fixture exercised the record panel. These are separate preparation records, not yet automatically merged into a later application attempt, and they do not establish visual correctness or authorize submission.

When attaching a PDF in a form session, the user can now select and acknowledge a saved visual review for the exact PDF and current job/qualification context. The app checks the source record and PDF hash again, copies the advisory findings into the application preparation folder, and records their source. A stale, incomplete or mismatched selected review is rejected. Attaching without a linked review remains possible but flags the job and record for manual review; model findings do not prove correctness. Candidate lookup is bounded and reports truncated or unreadable records. A fictional local browser attachment fixture checked acknowledgement and evidence copying; disk and mocked UI fixtures checked hash/context matching and the review control. Email attachments are not yet connected to this mechanism, and application submission remains unavailable.

Email draft and Gmail packet preparation now let the user link and acknowledge a current saved visual review for each chosen PDF. The server rechecks the PDF hash, job context and completed-review state, saves the advisory report in the email attempt folder, and marks missing reviews for manual handling. Reopened Gmail packets show whether each linked review is current, stale, absent or invalid; earlier packets without the new field remain readable. A fictional disk-backed Gmail fixture verified acknowledgement, saved evidence, stale context and no-send rehearsal; a mocked browser fixture exercised email review selection. A model review does not establish correctness, and neither preparing nor reviewing a packet sends it.

The job-specific CV diagnostic now shows up to three reviewed qualifications containing a missing posting word and links directly to the template editor with that job selected. This is literal matching, not proof that a qualification satisfies the job requirement. After an edited PDF is saved, controls offer another CV check and visual review for the selected job. The existing local-model passage suggestion remains user-reviewed and uses two drafting passes; it is not automatically applied. Fictional PDF/DOCX, fake-model and mocked-browser checks exercised these paths. No real model or employer screening system was tested.

Duplicate indexing now prioritizes jobs with the same normalized requisition ID and a shared employer token before spending its 200-candidate comparison budget. Migration 27 adds an indexed requisition lookup and requeues previously bounded jobs with usable IDs; it preserves existing review decisions and invalidates dependent coverage revisions. A fictional crowded-employer test reproduces a match that was previously beyond the cap, and migration and query-plan tests cover the index and requeue behavior. Matching requisitions are review signals, not proof that jobs are identical. The 200-candidate and employer-token limits still apply, normalization is limited, and incomplete coverage remains visible.

New application records now store materials under bounded, UTF-8-safe names that retain a short extension, and use no-overwrite writes. A plaintext `materials/manifest.jsonl` records the name requested by each caller, stored filename, byte count and SHA-256; some export callers intentionally add a prefix to the source name. A Windows fixture reproduced a failure with a 255-byte PDF name before this change and exercised long Unicode names and collision refusal afterward. Existing records are not retroactively updated, and a manifest write failure is reported even if the material file was already saved. Fictional email, DOCX editing and letter-export tests passed; no application was sent.

Form attachment now uses a bounded Windows-safe temporary filename, retains the saved document name, and shows the website-facing filename before the attachment gesture. The UI sends the reviewed name and the API rejects a mismatch; compatibility callers that omit the optional name are not bound by that check. The attempt event records both names. A local browser fixture attached a fictional PDF whose saved name was 255 bytes, verified the staged file and rejected a stale filename. A rendered desktop and narrow-screen check used a mocked local API; it exercised the filename notice and request but did not contact an employer. Older application records remain unchanged, and website behavior is not established by these fixtures.

Wider-web search now retains up to 100 valid candidates from each provider page, matching its existing 100-item examination bound; previously it stopped after 30 and could leave additional results on the same page unexamined. The global 600-link batch cap, single in-flight request, 2 MiB response cap and five-page-per-phrase limit remain. If a page has more valid candidates than the remaining batch budget, coverage reports how many were omitted instead of describing them as unexamined. A fictional loopback provider returned 70 candidates on one page and a later new result after 70 duplicates; the search tests passed. A rendered local mock exercised pagination and the coverage notice. These are unverified links, not confirmed vacancies or application pages; no live provider or employer was contacted.

## Fit input coverage — 2026-10-30

New job-fit assessments report when the bounded model input excludes part of
the saved listing or search instructions. Their cache binding includes the full
saved description and instructions, so a change outside the model's input limit
also makes the old assessment stale. Existing assessments are invalidated by
the input version change. The percentage remains a deterministic calculation
over a model-generated rubric, not a calibrated probability; its displayed
range does not account for omitted text or model interpretation errors.

The bounded qualification-selection pass now alternates lexical candidates and
sampled facts before batching. This allows sampled facts to be examined when
longer facts exhaust the six-batch budget. The assessment reports how many
sampled facts were evaluated and keeps the same limits on calls, bytes and
selected facts. A fictional local-model check with long facts selected a sampled
accessibility paraphrase; this single case does not establish general recall or
calibration. The revised input version invalidates older assessments.

Template passage suggestions now include the user-selected passage as an
explicitly labelled, unverified source, even if no reviewed qualifications are
available. The source remains visible with the suggestion and the replacement
still requires review. On one fictional DOCX passage, the local model previously
returned no answer when the relevant experience existed only in the template;
after this change it produced a replacement retaining the accessibility and
screen-reader details. This is one case, not proof that all original details or
model-generated claims are correct. No application was sent.

Form-field interpretation now sends at most 20 fields per sequential model
request and splits batches by serialized field-description size, with an 8,000
byte prompt limit. An oversized individual description remains a manual-review
case. A deterministic test exercised six long labels across three requests. In
a 25-field fictional local-model check, the second batch mapped a family name;
the model incorrectly flagged an ordinary given-name question in the first
batch, which remained explicitly pending review. A prompt wording change did
not correct that result and was not retained. These checks do not establish
reliability on employer forms or prove that smaller batches always fit a model's
token context. Requests remain sequential, and no application was sent.

AI duplicate search now compares up to three related listings per sequential
model request. Valid matches from successful batches are retained when another
batch fails, and the interface reports the number compared, unassessed and
failed, alongside the existing candidate-limit notice. A local-model fictional
check compared four listings across two batches, flagging two variants of one
requisition (including different titles) and leaving two unrelated roles out.
This is one small fixture; model omissions, incorrect matches, the employer
neighbor filter and its candidate cap remain limitations. Every flagged pair
still requires user review.

Completed AI duplicate searches now remain available until the result has been
saved or rejected as stale. The interface can resume a pending search after a
request interruption, and the last saved response can be replayed without
inserting duplicate pairs. A fictional local API fixture checked repeated
polling, saving and replay after the saved pairs changed the neighbor set.
Pending task state and the replay are in memory only and do not survive an app
restart; this does not establish model accuracy or eliminate duplicate jobs.

The template editor can request local suggestions for up to five selected,
unique, non-overlapping lines in sequence. Each result shows its original,
replacement and cited sources; the user individually adds desired changes to
the existing reviewed queue, which retains job and context bindings. A failed
later request leaves earlier suggestions available. A fictional DOCX test ran
two passages through the resident local model; a rendered mock exercised the
queue at desktop and narrow widths. The browser test used mocked API responses,
not a real file export. Suggestions are not automatically applied, model
accuracy remains unproven, and a lost model response may require another request.

The Windows development build helper can provision OpenSSL 4.1.0 x64 into its
repository-local `target` cache when no compatible developer installation is
found. It downloads a pinned publisher MSI, verifies its size, SHA-256 and
Windows signature, extracts it without a system installation, and verifies the
development-header, import-library, runtime-DLL and license inventory on reuse.
A fresh network download, a SQLCipher test build and a debug build using the
extracted package were exercised on this host. The debug runtime matched its
new receipt and the previous receipt-matched runtime was retained as a backup.
Offline fixture checks covered cache reuse and
rejection of modified headers and an invalid installer. Rust and MSVC build
tools still require separate setup; this does not finish application installer
provisioning for Chromium, Ollama or other missing dependencies.
When changing OpenSSL versions, the build helper replaces an existing runtime
DLL only when the previous build receipt identifies it, and retains a backup.

Windows browser discovery now checks fixed per-user and machine App Paths for
Edge, Chrome and Chromium in addition to conventional locations. Registry
queries run without a visible window, have bounded output and an 800 ms deadline
each, and accept only absolute executable paths matching the queried browser.
Windows path duplicates are compared without case or slash differences, and up
to twelve existing candidates can be tried. An installed registered browser was
started with a private profile on this host, and a separate local attachment
fixture passed without submitting. Non-UTF-8 registry output and expandable
registry values are skipped, and this does not install a browser.

A Chrome-for-Testing archive and a Chromium snapshot were inspected for a
possible private runtime. The Chrome archive did not contain a standalone
redistribution license; [Google's general terms](https://policies.google.com/terms?hl=en)
restrict software redistribution. The Chromium snapshot was unsigned and its
test launch in the repository directory reported a sandbox access failure. No
browser was bundled on this evidence. Automatic browser provisioning remains
open; no third-party application was contacted.

Local form mapping now formats larger field batches with indented JSON while
retaining the earlier compact layout for shorter batches. In a fictional
ablation the resident model mapped a given-name question in the indented
10-, 15- and 20-field variants where it omitted or flagged it in compact input.
The [evaluation record](g1/FORM_MAPPING_LAYOUT_2026-10-30.md) also describes a
short-form regression seen when applying indented formatting everywhere and the
limited safety checks run on the revised path. These results are not evidence of
general form accuracy; uncertain fields still need review.

Batch listing inspection now stores application-check results in the same
shape as individual checks, so a current result is no longer shown as stale
solely because it came from a batch. The selected-job panel also displays the
check outcome; a changed URL or stale result remains explicitly marked for
review. Mocked local UI fixtures checked current and stale outcomes at desktop
and narrow widths. Results in the jobs panel remain in memory for the current
page session, and an application check remains advisory rather than proof that
an application can be completed. No employer page was contacted in this check.

Reviewed application-link lists now also exclude destinations whose observed
link labels indicate a final action, even when another visible anchor uses the
same destination with a different label. Link extraction retains distinct
labels for the same destination; a fictional local browser fixture exercised
that behavior. The filter only covers certain English labels and URL patterns,
and link extraction is bounded, so it cannot prove that a destination is safe
or prevent every external side effect. No application was submitted.

Link review now marks the result for manual handling when the browser's
200-link inventory reaches its limit, since later links may have unobserved
labels. It also warns when the 10 likely-link or 30 other-link display limits
are reached. A local browser fixture with 200 links and focused mock tests
exercised the limit handling; this does not establish coverage on employer sites.

A fictional local-browser and `qwen3.5:9b` vision check exposed a placeholder
failure: the model transcribed an empty email field's placeholder as if it were
an entered value. The result path now independently rechecks visually matching
values against the current DOM, downgrades unconfirmed values and marks the job
for manual review. The local fixture subsequently reported the filled name as
confirmed and the placeholder email as unconfirmed. The vision model's raw
transcription is still saved as an observation; a separate event records the
live-value comparisons. Both checks can be wrong or become stale, and this does
not establish correctness on real employer sites.

Visual form freshness now compares captures taken before and after live-value
verification, and rechecks the page URL and dimensions afterward when fields
were checked. A change in either capture remains a changed result even if the
page returns to its earlier appearance. A focused test covered the freshness
decision and the local prefill fixture passed; no employer site was exercised.
These snapshots still cannot rule out a change between observations or after
verification.

The profile can now store an optional full name exactly as supplied by the
candidate, alongside separate first and last names. Form mapping can use the
explicit value for a full-name label or standard `name` autocomplete hint; it
does not assemble one when the field is empty. A fictional local-model check
mapped a nonstandard applicant-name label to that value, and focused tests
covered profile input, conflicting hints, search-query identifier filtering and
the profile summary. The model can still misinterpret labels; review remains
necessary.

When the document evidence-check prompt exceeds its 8,000-byte limit, the app
now checks individual paragraphs sequentially (at most eight) rather than
skipping all of them. It reports unchecked paragraphs and stops further calls
if the model is unavailable. A fictional local `qwen3.5:9b` test completed
three separate checks and marked an invented management claim unsupported;
fixture tests also covered partial failure and the interface message. This is
not proof of factual accuracy, and edits after generation remain outside the
check.

Template rewrite suggestions now use bounded fact selection and prompt inputs,
while always retaining the user-selected passage as an explicitly unverified
source. The interface reports selection and truncation limits. A fictional
local-model test with a long listing and 25 facts produced a rewrite; one
selection batch failed and the reported fallback was used. A separate vague
fictional passage produced no answer and was left for manual review. These
checks do not establish that rewritten claims are accurate or complete.

Automatic application-page checks now require both the existing conservative
single-link rule and the label-aware review filter before following a candidate.
A fictional driver fixture verified that a shared destination labelled both
“Apply now” and “Submit application” is not followed. The check remains
heuristic: labels and paths cannot prove that an arbitrary page has no external
side effects. No employer site or application submission was used in this test.

A pairwise local-model duplicate comparison can now be retried after a lost
status response. The latest result is held in memory for up to ten minutes and
its listing and decision binding is checked again when read; a decided pair
cannot revive an old result. The interface retains the task ID for the current
browser session and can resume it after a rerender. A fictional API fixture and
UI retry test covered response loss and a subsequent duplicate decision. The
cache is bounded to the latest result, does not survive an app restart, and a
lost start response before the browser receives a task ID is not recovered.

Duplicate candidate search now bounds serialized model input and checks quoted
excerpts against exactly the shortened text supplied to the model. It reports
shortened source listings, shortened model inputs, unexamined candidates and
failed model calls, and retries an invalid multi-candidate response individually
within a fixed bound. The full listing text is hashed before the candidate
comparison is saved, so edits outside the displayed prefix invalidate an in-flight
result. A fictional multilingual long-text fixture with local qwen3.5:9b
identified one matching requisition with a different title and excluded one
unrelated job after the prompt requested short excerpts. Earlier runs of that
fixture exceeded the model output limit; the successful run does not establish
general duplicate accuracy. Candidate selection remains capped and does not
cover every potential duplicate, and all proposed matches require review.

On 2026-10-30, the existing fit repeatability fixture was run three times with
resident qwen3.5:9b at a 4096-token context. All three fictional assessments
produced the same cited rubric and 83% estimate, with a 67–100% uncertainty
range. Each also flagged a possible overlap between a remote-work preference and
a listing criterion. This single repeated fixture does not establish accuracy,
calibration or repeatability across other jobs, profiles or model versions.
