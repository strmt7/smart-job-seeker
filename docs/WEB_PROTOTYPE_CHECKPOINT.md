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

When a single structured job posting omits its title or employer, inspection can now use the existing local-model page-text fallback, requiring exact page excerpts and recording a warning for manual review. Multiple structured postings and other structural failures remain blocked. In a fictional local-browser fixture, resource-gated `qwen3.5:9b` recovered the title and employer; unit checks covered partial data, missing evidence and multiple postings. The model was unloaded afterward. This does not establish real-site extraction accuracy or a working application route.

For this fallback, a nonempty structured title or employer must match the recovered identity before its bounded description or requisition identifier is retained. The result remains marked for review because one matching field cannot prove the sources describe the same vacancy. Conflicting or unanchored structured details are not transferred. A fictional browser/model fixture retained a requisition, and a store fixture confirmed that it can trigger duplicate review; conflicting and unanchored cases were checked separately. No employer site was contacted.

New listing captures now retain a bounded extraction provenance summary with the first saved source. Job details show whether the local model or structured data supplied the listing, which fields came from incomplete structured data, and whether the saved values and source URL still match the current job. Older captures may not have this summary. A matching digest is not proof that the source is correct or current on the employer site. Fictional encrypted-source/API and UI tests passed; the standalone panel was rendered at 1,200 and 390 pixels using mocked data.

New listing captures also retain up to eight single-address `mailto:` links from the bounded top-document link inventory. Job details show them as unverified contacts and can open an unsent draft with a selected address; no link subject or body is used, and no email is sent by this action. A link may be unrelated to applications, hidden or later changed, and addresses in ordinary text, unsupported address formats, other frames or links beyond the inspection limit may be missed. A fictional browser fixture, encrypted-source/API and simulated UI tests passed; a standalone mock was rendered at 1,200 and 390 pixels. No employer site or email was contacted.

The same bounded contact list now also includes recognized email addresses in captured page text, with a short source excerpt and duplicate suppression against mail links. Link and page-text availability and truncation are reported separately. These are still unverified contacts; unsupported address formats, obfuscation, hidden content and text beyond the capture limit may be missed. A fictional browser fixture found both an application contact and an accessibility contact without opening either, and source/API and simulated UI checks passed. No email was sent.

The contact panel can ask the resident local model to suggest whether each captured address is for applications, another stated purpose or uncertain. For a non-uncertain result the model must quote saved context containing the exact address; malformed, incomplete or unsubstantiated output is rejected. Advice is rechecked against the saved source and job identity on retrieval, remains advisory, and does not choose a recipient or send email. The bounded task result is held in memory, not persisted across restart.

With resource-gated `qwen3.5:9b`, fictional cases classified application and accessibility contacts correctly, including when both appeared in the same captured page context. A fictional local API test exercised result recovery and stale-source rejection; focused and mocked UI checks passed, and a standalone panel was rendered at 1,200 and 390 pixels. These examples do not establish classification accuracy on real listings. The model was unloaded; no employer or email was contacted.

Contact advice now makes a second separately prompted local-model assessment without including the first judgment. Disagreement or an invalid/unavailable second assessment leaves the purpose uncertain. Each decisive quotation must identify the selected address without including another captured address. These checks are advisory and the two judgments can share the same error. Fake-model disagreement, failure and shared-address tests and a mocked UI check passed; the changed two-pass flow has not yet been tested with a real model because the resource gate detected an already loaded model and left it untouched.

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

The search parser now examines up to 500 provider entries per page while retaining at most 100 new candidates. This allows later unique results to be found when earlier entries repeat a previous page; the existing batch, response-size and request limits remain. A fictional loopback provider returned 100 repeated results followed by one new result, which was retained. Pages with unexamined entries are still reported as limited. This does not verify vacancies or establish live-provider coverage.

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

Fit assessments now include deterministic breakdowns of the existing weighted
rubric by listing criteria versus user preferences and required versus preferred
criteria. Each nonempty group reports its criterion count, evidence weights,
estimate and unknown-evidence range; empty groups are explicitly unavailable.
The groups overlap and do not constitute independent probabilities. The overall
score formula and its model-generated criteria have not changed, and older
saved assessments do not acquire breakdowns until reassessed. Fictional tests
checked group separation, empty groups, unknowns, contradictions and ordering.

On 2026-10-30, local qwen3.5:9b at 4096 context passed the fictional listing
reordering check and the semantic candidate comparison, recognizing coaching
as relevant to mentoring. The existing C/C++ fixture exposed an extra generic
"programming experience" criterion. A proposed extraction prompt removed it in
one run but a reordered-listing assessment then failed validation. A separate
redundancy-review prompt flagged an independent five-year requirement as
redundant. Both prompt changes were rejected. These are single-case results,
not accuracy estimates; the unmodified system can still count overlapping
criteria and its review suggestions require human judgment.

Native text inputs and textareas now report a valid HTML maxlength in the form
review, measured in UTF-16 code units as browsers do. Reviewed suggestions and
manual answers exceeding that limit are refused before filling; the limit is
also part of the form schema identity, so a changed limit invalidates a review.
A local Chromium fixture checked extraction, invalid negative limits and schema
identity; a fictional review test covered an emoji boundary. The page can use
other validation rules, change limits after inspection, or count text differently
for custom controls, so this is not a guarantee that an answer will be accepted.
No external application was used.

Local form-answer drafting now receives the native field limit when present.
Both drafting passes are asked to fit it, and the final text is checked in UTF-16
code units. One bounded revision repair is allowed; if the result remains too
long, no draft is returned as usable. The review panel shows the draft's length,
and changing the page limit invalidates the session's schema binding. Fictional
fake-model tests covered a successful repair, a persistent overrun, emoji and a
zero-length limit. A real model was not resident for this check; its ability to
produce a suitable concise answer for arbitrary limits remains unverified.

A local qwen3.5:9b run on 2026-10-30 tested one fictional Python-experience
question at native limits of 80 and 35 UTF-16 units. The final answers used 51
and 27 units and cited the supplied fact. The shorter answer omitted the
original backend and three-year details, illustrating that fitting a limit can
lose useful information; this is not a general accuracy result. The model was
reported fully GPU-resident at 4096 context after a free-memory preflight.
Editable native text answers now show a live UTF-16 counter, including when a
draft is inserted; the counter warns rather than truncating an overlong edit.
A local UI fixture checked an emoji boundary and draft provenance retention.

Length-limited application drafts now receive a separate advisory local-model
check for potentially omitted details in their cited candidate facts. Returned
quotes and source indices are validated; invalid or unavailable checks are
reported, and an empty result is not proof of completeness. On 2026-10-30,
qwen3.5:9b identified the omitted three-year duration in one fictional short
answer and returned no omissions when the answer reproduced the complete fact.
It also flagged that duration in the 35-unit drafting fixture; these small cases
are not a general accuracy measurement. The check adds one bounded local model
call for a length-limited answer.

A local two-stage fictional form was also rerun with the resident model. It
filled and visually checked the name and email on separate pages and left a
sensitive authorization field empty. The test itself simulated the next-step
click; it did not verify automatic navigation or use an employer site.

The missing-detail review now receives all bounded qualifications supplied to
answer drafting, not only the subset cited by the answer. It validates each
returned quote against the corresponding supplied fact and shows the full fact
for review. On 2026-10-30, qwen3.5:9b flagged one uncited PostgreSQL fact for a
fictional backend-and-database question and did not flag an unrelated office
fact. This single case does not establish reliable relevance judgments; facts
excluded by selection or the context budget remain outside the check.

Structured job captures now expose the listing's original datePosted and
validThrough values alongside salary evidence, including capture date and a
warning when the capture URL differs from the current job URL. The app does
not infer that a listing is open or closed, interpret ambiguous timezones, or
change hard constraints from these strings. A fictional browser DOM fixture,
source-parser and endpoint tests, and a mocked UI check covered the dates and
provenance; no external listing was used. Dates remain unavailable when the
site does not provide a usable structured JobPosting capture.

The wider-web candidate queue now accepts up to 100 selected links per run,
with page-level select and clear controls for the paged batch results. It still
inspects one listing or application page at a time, observes the existing
source policy, and can stop after the current page. A mocked 100-item queue
checked that no more than one inspection was active; UI tests covered selecting
three pages, the cap and clearing selection. The queue depends on the open UI
and its recoverable session state; it does not establish that candidates are
live vacancies or that application pages are usable. No external pages were
opened in these tests.

The selected-listings queue can now recover a lost start response by checking
the server's pending listing URL or application-check job ID. It resumes only
when the pending task matches the current queue item; an unrelated task or an
ambiguous network failure pauses the queue for review. Explicit server
rejections with no pending task remain recorded as manual-review outcomes.
The pending listing endpoint includes its normalized source URL for this local
binding check. Fictional endpoint and mocked UI tests covered both task types,
an unrelated task, a lost response and an explicit rejection. Recovery remains
in-memory and depends on the browser session and running app; it does not
survive an app restart or establish that a page is an application.

Application-page checks can now optionally follow one unambiguous external
application link when the user enables the option. The link must pass the
existing label filter, have no query or fragment, avoid final-action-looking
paths, resolve to public addresses at the preflight, and belong to a known
source whose registry permits public reads. Unknown sources and sources that
require review remain manual. The final URL is checked for a further
cross-origin redirect; the browser may already have followed that redirect,
and page scripts can perform their own network actions. The check never issues
field-entry or submit commands and still reports manual review rather than
confirming a usable application. Fictional policy and UI tests covered opt-in,
ambiguity, rejected destinations and option propagation. No real external ATS
was contacted, so behaviour across employer sites and redirect chains is
unverified.

With the external-destination option enabled, an application-page check may
inspect a final redirected URL if it is a known source with public-read
permission, public HTTPS, no query or fragment, and no final-action-looking
path. The final host is checked for public DNS addresses after the browser has
already followed the redirect; prior redirect hops and page scripts are not
controlled by this check. Any subsequent link must remain on that destination's
origin, and the result identifies the redirect and requires manual review.
Policy tests covered opt-in and rejected destinations. A real local-browser
fixture confirmed a loopback cross-origin redirect remains manual even with the
option on. No real external ATS or redirect chain was tested, and the app did
not issue field-entry or submit commands in these checks.

The browser can now offer explicitly labelled `contenteditable=true` textboxes
for reviewed plain-text entry. It rejects nested input or editable controls and
read-only or disabled editors, verifies focus and exact retained text, and
limits multiline entry to 40 line breaks. Input replaces the editor contents;
formatting and advanced editor features are not preserved or interpreted.
A fictional local Chromium fixture covered single-line and multiline entry,
refusal of nested and disabled controls, and the line limit. A separate
qwen3.5:9b fixture mapped a fictional name, filled it with a simulated review
grant and visually transcribed it. The first vision attempt failed when the
label was not visible; after adding a visible label, it passed. This is not
proof of accuracy on real rich editors, hidden labels or changing pages; failed
or mismatched entry still needs manual review. No application was submitted.

Form schemas now distinguish native text controls, plain-text editing hosts and
labelled rich editing hosts. The editor mode is included in the review binding
and requested-field record, and rich editors receive a field-specific warning
that formatting may be replaced. A fictional local browser fixture changed one
editor from rich to plain-text-only and verified that the schema identity
changed; the local qwen3.5:9b rich-text fill and visual fixture also passed
again. Older serialized form fields without the new property deserialize with
no editing mode. This does not prove compatibility with arbitrary dynamic
editors, and a changed form still requires a new review.

Combobox widgets and editable inputs that expose a native datalist or list/inline
ARIA autocomplete are now left for manual review: retaining typed text does not
prove an option was selected. This applies to optional as well as required
controls; unrelated ordinary fields can still be reviewed and filled. The driver
also rechecks after focus so a field that turns into a combobox is not typed
into. A fictional local browser fixture covered these cases and ordinary name
autocomplete; an application-check fixture confirmed that other fields are
reported without mislabelling the custom control as a CAPTCHA. These controls
are not automatically selected, and custom widgets without these signals can
still be missed. No employer site or application submission was used.

The in-memory submission journal now validates transitions against the last
entry for the same application, rather than the last entry globally. An
interleaved fictional test verifies independent state transitions and that an
uncertain write still cannot be retried automatically. The durable workspace
already retrieves journal entries per application; migration v28 adds an index
for those lookups without altering prior migrations. Tests exercised a v27-to-v28
upgrade, preserved history and append-only triggers, and confirmed SQLite used
the new index, both with bundled SQLite and SQLCipher. This is not a performance
benchmark or proof of real-world submission behaviour. No application was sent.

## Match estimate sensitivity — 2026-10-30

A fit assessment now reports the lowest and highest scores obtained by removing one extracted criterion at a time, and the largest change from the original score. This is deterministic for a fixed validated rubric and helps identify dependence on individual criteria. It is not a confidence interval, calibration, or a check for omitted requirements or incorrect local-model interpretations. A single-criterion rubric has no leave-one-out score. The model still selects and judges criteria, so repeatability and real-world accuracy remain unproven.

## Stable fact-selection order — 2026-10-30

The local model selects relevant facts as a set; the application now processes each valid set in its original deterministic batch order. Previously a permutation of the same response could change which facts survived the 20-fact limit. The fit-input version changes so existing saved assessments are marked stale. A fictional fixture covers both fit and application-answer selection at the cap; this does not establish that the model chooses the same facts across runs or that its judgments are correct. No model was resident during this change, so no new live-model result is claimed.

## Fact-selection listing coverage — 2026-10-30

When more than 20 facts require local-model selection, long job listings are now sampled from the beginning, middle and end within the existing 2,400-byte listing budget. Separate passages are identified in the prompt, and the interface reports omitted text. This changes the balance of what is seen: a requirement in an omitted span can still be missed, and this does not establish improved model accuracy. The input version invalidates earlier saved fit assessments. Fictional tests exercised the later listing text, bounds and Unicode boundaries; no local model was resident, so no live-model result is claimed.

## Serialized fact-selection batch limit — 2026-10-30

Fact-selection batches now count JSON escaping, brackets and commas toward their 3,000-byte fact-array budget. A fact that cannot fit alone is omitted without cutting it and is reported for review; fallback selection also excludes and reports such facts. The fit-input version invalidates earlier saved estimates. This bound applies to the fact array only, not the entire prompt, model memory, or the path that supplies at most 20 pooled facts directly. Fictional tests cover escaped text and oversized facts. Ollama had no resident model; a live run was not started because the observed GPU use plus a prior model-load measurement would exceed the evaluation runner's 12 GiB threshold.

## Incomplete application archives — 2026-10-30

An application-attempt folder now receives an incomplete metadata record before its posting, screenshots directory and materials are initialized. It enters preparation in progress only after initialization finishes. If a later archive step fails, the records panel identifies the incomplete folder and warns that files may be missing. Windows tests covered a normal archive and an invalid captured-page record that fails partway through initialization. Failure to create the initial folder or metadata can still prevent a record from being listed. These records do not establish whether an external submission occurred.

## Fieldset accessible names — 2026-10-30

Native radio groups inside a fieldset can now use `aria-label` or `aria-labelledby` as their group question, with a referenced name taking precedence over a legend. A real local browser selected and verified options in two fictional groups; a separate fixture with duplicate referenced IDs raised a manual-review wall. This does not add automatic navigation or support every custom radio widget, and no employer site, local model or application submission was involved.

## Accessible disabled state — 2026-10-30

Form inspection now excludes controls individually marked `aria-disabled` or `aria-readonly`; required excluded fields raise the existing review warning. Native radio selection and later verification also refuse a radio inside an ARIA-disabled or ARIA-readonly ancestor. A fictional local-browser fixture checked an individually disabled option and a group disabled after inspection. Equivalent live-state checks are not yet implemented for every other control type, and no employer site or application submission was used.

## Live ARIA state for native controls — 2026-10-30

Text, date, checkbox, single-select, multi-select, editable text and file attachment operations now check their live ARIA-disabled and ARIA-readonly ancestry before acting and when verifying retained values. Single and multi-select also reject options carrying those states. A fictional local-browser fixture exercised normal operations, disabled options and a container disabled after inspection, then confirmed the later operations and verifications failed. This does not cover every custom widget or make page changes atomic; no employer site or application submission was used.

## Available options in reviewed schemas — 2026-10-30

Native select inspection now excludes options and option groups marked `aria-disabled` or `aria-readonly`, matching the driver's live selection checks. A fictional local-browser fixture verified exclusion and that changing an option's state changes the review schema; the existing grouped-select fixture also passed. The availability of controls inside ARIA-disabled containers can still be discovered during inspection and is checked again at action time. No employer site or application submission was involved.

## Unavailable controls during inspection — 2026-10-30

Rendered native controls and supported editable text fields now have their live disabled and read-only ancestry checked during the existing bounded visibility inspection. Unavailable controls are omitted from the review schema. A required unavailable control retains the blocking review warning; an optional one adds a non-blocking review obligation so other fields can still be prepared. The browser fixture covered a container becoming disabled, preservation of the changed-option schema check, and both warnings; a web-app unit test checked warning classification. Custom controls and live employer pages remain outside this fixture.

## Installer destination containment — 2026-10-30

The prototype installer now rejects a destination lexically inside the extracted `app` source before creating the destination, avoiding a recursive source-to-child copy. Fictional Windows installer tests checked nested paths, case-insensitive comparison and a similarly named sibling. This does not resolve junction or symlink aliases, establish clean-host installation, or provision the still-missing Chromium, Ollama and Visual C++ runtime dependencies.

## Installer junction-aware containment — 2026-10-30

The prototype installer now resolves the source directory and the destination's nearest existing parent through Windows directory handles before checking containment. Fictional local tests rejected a destination junction into the source and a source junction pointing to the real package, while allowing an unrelated destination junction. The installer fixture passed under PowerShell 7 and Windows PowerShell 5.1. This check is not atomic against a concurrent path change and does not establish support for every network, mount or alias configuration; installation and dependency provisioning on a clean host remain unverified.

## Startup error visibility — 2026-10-30

A failure returned from the application startup path now produces a bounded error message on stderr and, on Windows, requests a native error dialog using a hidden Windows PowerShell process. The message is sent over stdin rather than embedded in the command. A focused unit test covers message bounds and control characters; the dialog itself was not exercised in this checkpoint. Missing DLLs that prevent the executable from starting, a missing or restricted PowerShell host, and errors swallowed inside the tray implementation are not covered by this fallback.

## Small-model fact-selection check — 2026-10-30

Fact-selection batches now provide explicit IDs beside each fact and explicitly require an `indices` array, including for an empty result. The encoded batch budget includes these IDs, and the fit-input version invalidates older estimates. The local `gemma3:4b` digest `c30276f7dffec93a536d283e15088990454b8d21cf180a3d6eea4959e4147291` was loaded for a fictional long-listing case with a 4096-token context; Ollama reported it fully GPU-resident. The initial prompt produced an out-of-range index in one batch; an intermediate prompt returned empty objects for two batches. With explicit IDs and an empty-array example, two runs returned valid arrays and included the relevant screen-reader fact, but also selected two unrelated inventory facts. The stricter ignored live test fails on those false positives. This is one synthetic case, not a general model comparison or accuracy measurement. GPU usage snapshots were 6.54 GiB before loading and 10.59 GiB after; later 10.48 GiB, with 25.13 GiB free system memory. Snapshots are not peak measurements. The model was left to its Ollama expiry rather than forcibly unloaded. Four synthetic selector tests and web-app Clippy passed. No employer site or application submission was involved.

## Fact-selection cross-check — 2026-10-30

Model-selected facts now receive one additional bounded local-model relevance comparison, with at most one repair after an invalid response. Facts judged uncertain are retained; an unavailable, incomplete or invalid comparison retains the original selections. A 6,000-byte fact-payload limit skips the check without dropping facts. Clearly unrelated judgments remove facts, subject to the existing lexical fallback; the interface reports the removed facts and any reintroduced by fallback. This applies to fit, document and answer selection and can itself wrongly remove relevant evidence, especially because the job context is sampled.

On the previously failing fictional case, the first cross-check prompt returned an incomplete judgment list and was rejected. A revised prompt produced complete judgments and retained the screen-reader qualification while removing both unrelated inventory facts in two local `gemma3:4b` runs; the strict live test passed. This is one synthetic case, not a measurement of general accuracy, recall or repeatability. A resource preflight required at least 12 GiB free system memory, at most 12 GiB dedicated GPU usage and one fully GPU-resident 4,096-token model; a later snapshot showed 25.40 GiB free and 10.49 GiB GPU use. These are snapshots, not peaks. The model is left to its normal Ollama expiry. Five focused synthetic tests, web-app Clippy and JavaScript syntax checking passed. No employer site or application submission was involved.

## Complete-context fact cross-check — 2026-10-30

The relevance cross-check now covers the supplied listing in up to four 2,400-byte UTF-8 chunks and removes a selected fact only when every chunk returns a valid unrelated judgment. A missing, invalid or uncertain judgment retains the fact. It skips removals when the listing cannot fit the chunk budget, reaches the callers' 8,000-character truncation boundary, or preferences or the application question exceed the 1,200-byte comparison limit. These are conservative bounds; they do not prove the source listing itself is complete. The check can use up to eight sequential calls including one repair per chunk, and an assessment input version change marks prior estimates stale.

Two synthetic tests covered chunk agreement, Unicode boundaries, incomplete context and invalid or uncertain responses. The previously failing fictional `gemma3:4b` case passed with two chunks and one repaired malformed response; it retained the relevant fact and excluded the two unrelated facts. This remains one case, not a general recall or accuracy benchmark. Web-app Clippy and JavaScript syntax checks passed. No employer site or application submission was involved.

## Overlapping fact-check passages — 2026-10-30

Adjacent relevance-check passages now overlap by up to 256 UTF-8 bytes, while each remains at most 2,400 bytes and the four-passage limit remains in force. This reduces the chance that a short requirement split at a boundary is judged from incomplete fragments; it cannot preserve every longer or cross-section relationship. If overlap prevents full coverage within four passages, selected facts are retained and the check is reported incomplete. The assessment input version marks prior estimates stale.

A synthetic test covered a marker split at a passage boundary, UTF-8 boundaries and the reduced four-passage capacity. A live fictional `gemma3:4b` test retained the relevant screen-reader qualification and excluded two unrelated facts when the requirement crossed the boundary; one malformed check was repaired. This is one case, not a general model accuracy result. Web-app Clippy passed. No employer site or application submission was involved.

## Selected-qualification review and paraphrase checks — 2026-10-30

Fit, answer and document review panels can now show the bounded qualifications selected for the task. Later draft context limits may omit some selected facts, and the full profile may contain relevant facts that were not selected. The display does not make the selection editable. A focused UI fixture and selector test checked the displayed data and bounded selection paths.

With the resident local `gemma3:4b` model at a 4,096-token context, two additional fictional selection cases passed: a VoiceOver experience fact was retained for a job about services for visually disabled users, and a checkout rollback fact was retained for an application question about production incidents. The first case required one repaired cross-check and removed two unrelated inventory facts; the second retained one selected fact. These two cases are not a general recall or accuracy benchmark. The resource preflight snapshot showed 24.72 GiB free system memory and 10.49 GiB dedicated GPU use, not continuous peaks. No employer site or application submission was involved.

## Draft qualification inputs — 2026-10-30

Answer and document generation now return the exact qualification facts sent to drafting after their context budgets, alongside the initially selected facts. The review panels display both lists so a user can see which selections were omitted; selected template passages can appear in the drafting list even when they were not in the model's selection. Contact details are handled separately by the document workflow. The lists do not prove every fact was used correctly, and the selection remains non-editable. Focused fictional answer, document and template budget tests, the selected-facts UI fixture and web-app Clippy passed. No local model or employer site was exercised in this checkpoint.

## Prioritized qualifications for document drafts — 2026-10-30

The job panel now lets a user prioritize up to three reviewed qualifications for the next CV or cover-letter draft. The server rechecks their active reviewed identities, puts their facts first in the bounded drafting context, and returns an error if they cannot fit. The resulting review shows the actual drafting inputs. Priorities can displace model-selected facts and do not guarantee the model includes or accurately describes them. Choices remain in the current page and must be selected again after a reload; stale identities are rejected. Focused fictional tests covered reviewed, unreviewed and corrected claims, context limits, ordering and the picker; web-app Clippy passed. The picker was not exercised in a rendered full-app session or with a live model in this checkpoint. No application was submitted.

## Prioritized local draft and evidence retry — 2026-10-30

A fictional `gemma3:4b` cover-letter run sent the prioritized VoiceOver qualification to the model and included it in the generated paragraph. Initially its paragraph evidence check exhausted the 700-token output limit without a natural stop and was reported unavailable. The check now makes one shorter, 500-token retry only after a structured-output failure; runtime and budget errors are not retried. In two subsequent runs the first check still exhausted its budget, but the retry returned a valid supported judgment. The model's explanation inaccurately described the wording as identical, so the check remains advisory. The UI indicates when the shorter retry was used. The live fixture, focused retry and evidence tests, UI fixture and web-app Clippy passed. This is a single fictional drafting case, not proof of factual accuracy or coverage. Before the first run the model was fully GPU-resident at 4,096 tokens with 26.86 GiB free system memory and 10.37 GiB dedicated GPU usage; those are snapshots, not peaks. No employer site or application submission was involved.

## Evidence-check structured output and numeric claims — 2026-10-30

A four-case fictional `gemma3:4b` evidence test initially returned invalid structures for three cases. The document check now uses a task-specific Ollama JSON schema and retries one invalid response with the shorter prompt, while retaining application-side shape and ID validation. The model then marked a fabricated 90% improvement as supported by a fact with no number. The application now downgrades a model-supported paragraph to uncertain when an ASCII numeric literal in it does not appear in its cited facts. It reports the reason for manual review.

The four fictional cases passed after these changes, but the model still incorrectly marked the invented percentage supported; the deterministic check changed the displayed result to uncertain. Literal matching does not verify numerical meaning, units, spelled-out numbers, or claims without numbers, and can conservatively flag equivalent formats. The model also produced extra judgments in some first responses, which were rejected and retried. Focused validation, retry and numeric tests, schema parsing and web-app Clippy passed. The resident model and resource thresholds were checked before the live run; no employer site or application submission was involved.

## Numeric review for drafted answers — 2026-10-30

The application-answer evidence path now applies the same conservative numeric-literal check used for document paragraphs. It downgrades a model-supported claim containing a number absent from the cited facts and separately checks the entire draft, including when the model omits that claim or its response is invalid or unavailable. The answer review panel displays the warning. This is advisory: matching literals do not establish meaning or units, and spelled-out or non-ASCII numbers are not covered.

In one fictional live `gemma3:4b` case, the model itself marked an invented 90% claim unsupported and the whole-answer numeric warning also appeared in the result. Focused synthetic backend and UI tests covered a falsely supported claim, an omitted claim and an invalid model response; web-app Clippy passed. The isolated UI fixture needed its existing helper stubs updated. This is not a general accuracy measurement. No employer site or application submission was involved.

## Edited answer evidence state — 2026-10-30

Form and email draft panels now state that evidence checks apply to the original generated text. After inserting a draft, editing the destination field changes the panel to a visible, live-announced review warning; using a draft again resets the message. The existing backend continues to reject a stale generated-answer provenance binding after edits. Focused UI fixtures covered insertion, editing, rebinding and the existing answer evidence panel. These checks do not validate edited text, and no rendered employer form or email was sent.

## Numeric units in evidence review — 2026-10-30

The shared document and answer numeric check now distinguishes percentages and a limited set of English time units when comparing a model-supported claim with cited facts. A number with a different or missing recognized unit is downgraded to uncertain. Percent signs and the word percent are treated alike. This is a conservative literal check: it can flag equivalent wording and does not establish semantic relationships, currencies, spelled-out numbers or correctness when the same number and recognized unit refer to different things.

Two additional fictional `gemma3:4b` evidence cases used the same number with different meanings. The model rejected them after a repair, although one first response contained contradictory extra judgments. Synthetic tests forced a model-supported response and verified that the unit check flags a 90-percent versus 90-minute mismatch and a five-engineer versus five-year mismatch; a matching percent format remained supported. The six-case live fixture, focused backend and UI tests, and web-app Clippy passed. This is not general accuracy evidence, and no employer site or application submission was involved.

## Qualification picker rendered check — 2026-10-30

A fictional local fixture rendered the qualification picker in headless Edge at 1440×900 and 800×900. The checkbox alignment and action-row layout were corrected; the narrow viewport had no horizontal overflow. Selecting the reviewed qualification and starting a CV draft sent its ID in the request to the mocked local API, and the fictional draft review appeared without console errors. This did not exercise the live backend, a model, an employer site, or submission. Playwright Core was installed temporarily under the ignored target directory for the check.

## Preference evidence beyond the model excerpt — 2026-10-30

Preference comparison now checks repeated citations and nearby numeric intervals against the entire assessment listing, rather than only the first 3,000 bytes sent to the preference model. A repeated quote anywhere in that assessment input leaves the preference unknown. The assessment listing itself is capped at 8,000 characters, so repetitions or context beyond that cap may still be missed. The input digest version was updated so earlier assessments become stale.

A focused fictional regression test passed for both supported and contradicted model judgments when the second occurrence lies beyond the model excerpt. This tests deterministic handling, not the model's semantic accuracy or overall match calibration. No local model, employer site or submission was exercised.

## Original posting in preference citation checks — 2026-10-30

The separate preference comparison still sends only its bounded listing excerpt to the local model, but deterministic repeated-quote and numeric-context checks now use the original posting when it fits within 1 MiB. If that source exceeds the limit, positive or negative preference judgments remain unknown. The first assessment response cannot establish a preference judgment by adding an unsolicited citation: a separate comparison must establish it. The assessment input digest version changed, marking older scores stale.

Fictional tests covered a repeated quote beyond the 8,000-character assessment input, an unavailable verification source and a failed separate comparison after a first-pass positive response. The fit test group passed 32 tests with 18 live-model tests ignored; web Clippy and formatting passed. These checks do not establish semantic correctness, score calibration, or complete treatment of omitted requirements. Qualification checks and model prompts still use bounded inputs, and the source-size limit is in bytes. No local model, employer site or application submission was exercised.

## Original posting in qualification checks — 2026-10-30

Qualification cross-checks now verify that a quoted requirement occurs uniquely in the original posting before asking the local model to compare it with a candidate fact. If the posting exceeds the existing 1 MiB verification limit, the source is unavailable for this check and affected judgments remain unknown. The model receives only the bounded surrounding context, and the assessment digest version changed so older scores become stale. Repeated words may conservatively require review even when their meanings are consistent.

A fictional fixture covered a repeat beyond the assessment's 8,000-character listing and an unavailable verification source; it verified that neither is sent to the model as uniquely identified. The fit test group passed 32 tests with 18 live-model tests ignored; web Clippy and formatting passed. These results do not establish model reasoning accuracy, score calibration, or detection of omitted requirements. No local model, employer site or application submission was exercised.

## Local preference ambiguity check — 2026-10-30

A six-case fictional preference benchmark ran on local `gemma3:4b` (digest prefix `c30276f7dffe`, 4,096-token context). Before this change, three cases produced the expected completed judgment; two comparisons were unavailable and the model falsely treated an unstated sponsorship policy as an explicit rejection. A conservative English phrase check now downgrades judgments citing certain statements that information is unstated. On the subsequent run, four of six cases produced the expected completed judgment; the two unavailable cases still failed the benchmark. The same model also passed a separate two-case fictional semantic fact-selection test. This is not a general accuracy or repeatability measurement; different phrases, languages and unsupported claims can still be missed, and the phrase check can flag unrelated text.

The focused synthetic preference test, web Clippy and formatting passed; the benchmark remained failing as described. A pre-load snapshot showed 28.88 GiB free system memory and 6.15 GiB dedicated GPU usage; after loading the model, snapshots showed 26.33 GiB free and 10.06 GiB GPU usage. The model was fully GPU-resident; peaks were not measured. No employer site or application submission was involved. Older fit assessments are marked stale.

## Preference citation repair — 2026-10-30

Preference comparison now makes at most one additional local request when the first response has an invalid structure or citation. The retry asks for exact listing text and receives the same validation; runtime failures are not retried, and unresolved responses leave the assessment unknown. Successful responses record whether a retry occurred. The input digest version changed so older assessments become stale.

In the six-case fictional `gemma3:4b` ambiguity benchmark, the prior 4/6 result became 5/6 after one location comparison returned a valid citation on retry. The remaining case still failed: its first quote was paraphrased and its retry used a string instead of JSON null. The model also continued to misinterpret the sponsorship example; the existing deterministic guard changed that outcome to unknown. This is a small evaluation of one model and does not establish general accuracy or repeatability. A focused synthetic test verified one repair and no retry on runtime failure; four preference tests passed, with two live tests ignored, along with web Clippy and formatting. No employer site or application submission was involved.

## Schema-constrained preference comparisons — 2026-10-30

The Ollama adapter now supplies a task-specific JSON schema for preference comparisons, including structured IDs, statuses, citations and explanations. Application-side validation still checks exact source text, IDs and completeness; one invalid response may be retried. The fit input digest version changed so older assessments become stale.

On two sequential runs of the six-case fictional `gemma3:4b` ambiguity benchmark, all six final outcomes matched the fixture expectations, compared with 5/6 on the prior run using generic JSON mode. Some results used the existing retry or deterministic guards; this is not evidence of general semantic accuracy or performance on other models. The schema parsing test, four focused preference tests and web Clippy passed; two other live preference tests were ignored in the focused run. Before evaluation, free system memory was 25.72 GiB and dedicated GPU usage was 10.02 GiB with the model already fully GPU-resident; peaks were not measured. No employer site or application submission was involved.

## Held-out preference comparison cases — 2026-10-30

An additional eight fictional preference cases cover work arrangements, sponsorship, on-call duties, management, leave, work focus, pay and relocation. This is a separate small benchmark from the earlier six cases; its expected judgments are fixture assumptions and are not a representative sample of real postings. The test can use a named, already resident local model and prints its fictional responses for diagnosis.

Two runs with `gemma3:4b` (digest prefix `c30276f7dffe`, 4,096 context) each returned seven expected completed judgments out of eight. The fully remote versus three-days-in-office comparison remained unavailable: the model initially described the conflict but returned null instead of an exact quote, and its retry also returned null with a different explanation. The benchmark remains failing and the application retained unknown. This does not establish general accuracy, real-world calibration or repeatability. Web Clippy and formatting passed. Before the first run, free system memory was 26.12 GiB and dedicated GPU usage was 10.01 GiB with the model resident; peaks were not measured. No employer site or application submission was involved.

## Short preference repair prompt — 2026-10-30

The one allowed preference-comparison retry now uses a shorter prompt that separates the job text from the preferences and requests an exact quotation from the job. Response validation and unknown fallback are unchanged; older assessments become stale. An experiment that included the prior malformed response was discarded after the model quoted the candidate preference instead of the job.

With local `gemma3:4b` (digest prefix `c30276f7dffe`, 4,096 context), the eight-case fictional set returned 8/8 expected completed judgments in two runs, compared with 7/8 before this prompt change. In the previously failing hybrid case the retry returned the exact job quote. The prior six-case set passed once. The eight-case set was used to tune this prompt and is no longer an independent hold-out; these small results do not establish general accuracy. Four focused preference tests passed with two live tests ignored; web Clippy and formatting passed. Before evaluation, free system memory was 25.96 GiB and dedicated GPU use was 10.02 GiB with the model resident; peaks were not measured. No employer site or application submission was involved.

## Full fit assessment on a smaller local model — 2026-10-30

The existing fictional full-assessment repeatability test can now use a selected resident local model and records the model's assessment responses. With `gemma3:4b` (digest prefix `c30276f7dffe`, 4,096 context), three identical attempts produced zero valid assessments, including after their allowed repairs. The model added candidate facts as listing or preference criteria, attached fact indices to unknown judgments, and cited unrelated facts as support. The application rejected the invalid responses rather than assigning a score. A task-specific structural schema was tried separately but still produced invalid fact references and excerpts, so it was not retained. This is one fictional candidate and listing, not a general model benchmark; the smaller model is not established as reliable for full fit assessment.

Web Clippy and formatting passed. Before the first run, free system memory was 25.43 GiB and dedicated GPU usage was 10.07 GiB with the model resident. A later snapshot during the schema experiment showed 23.66 GiB free and 10.16 GiB GPU use; peaks were not measured. No employer site or application submission was involved.

## Split extraction experiment rejected — 2026-10-30

A bounded fallback was tested after two invalid full-assessment responses: ask the model to extract criteria separately, initialize them as unknown, then run the existing evidence search and qualification cross-check. In one fictional `gemma3:4b` repeatability run (three assessments, digest prefix `c30276f7dffe`, 4,096 context), it produced valid-shaped results scoring 83%, 60% and 60%. The 83% assessment incorrectly treated a Python fact as evidence of SQL even after the model cross-check; the latter two did not establish the same result. The fallback was removed. The experiment demonstrates that syntactic validity and agreement between calls to the same model do not establish semantic support. It is one fictional case and does not measure general performance.

A resource snapshot during the experiment showed 23.11 GiB free system memory and 10.18 GiB dedicated GPU usage; peaks were not measured. No employer site or application submission was involved.

## Short acronym qualification review — 2026-10-30

A fictional local-model check reproduced a false 100% score when a Python fact was accepted as supporting both SQL and Python requirements. A prompt-only clarification did not fix the batched result and was discarded. The qualification check now moves a supported judgment to unknown when the entire requirement is a 2–12 character uppercase ASCII acronym and the cited fact does not contain that acronym as a separate case-insensitive term. This is a conservative guard, not semantic proof: equivalent terms such as a product name can require review, and mixed-case, longer or more complex mismatches remain outside its scope. The user-facing explanation says so; previous assessments become stale.

In one fictional `gemma3:4b` check after the change, both single and batched SQL-versus-Python cases stayed unknown; the deterministic guard made that decision before the model could confirm SQL. This does not show that the model's reasoning improved. A synthetic test covered a substring-only acronym occurrence; 34 fit tests passed with 20 live tests ignored, and web Clippy and formatting passed. No employer site or application submission was involved.

## Related skill names in qualification checks — 2026-10-30

Requirement-context lookup now distinguishes a standalone word from the same letters embedded in another word, so `Java` and `JavaScript` are not automatically treated as duplicate occurrences. A separate conservative check leaves a supported judgment unknown when a single alphabetic requirement appears only inside a longer term in the cited fact; it does not establish that two terms differ and may require manual review for legitimate inflections or compound words. Other paraphrases still use the model comparison. The assessment digest version changed so older results become stale.

In a fictional `gemma3:4b` fixture, correcting the context lookup exposed a false Java match from a JavaScript fact. After the new check, Java stayed unknown, but the model returned an unusable response for the other items and JavaScript also stayed unknown; the live benchmark therefore still failed. A synthetic fixture confirmed that the embedded term stays unknown while a separately validated matching term can remain supported. The fit group passed 35 tests with 21 ignored before a small allocation-only edit; the focused fixture was rerun after it, and web Clippy and formatting passed. This does not establish general semantic accuracy. No employer site or application submission was involved.

## Qualification response IDs — 2026-10-30

Qualification comparisons now request a task-specific JSON schema and make at most one retry for an invalid structured response. The retry supplies the exact expected item IDs; runtime failures are not retried, and unresolved comparisons remain unknown. The application still validates the IDs, statuses and evidence independently. The assessment digest changed so earlier assessments become stale.

In the fictional `gemma3:4b` related-skills fixture, the first response invented an ID for a criterion already marked for review. The initial schema-only experiment still failed; after adding the bounded ID repair, one run completed with Java unknown, JavaScript supported and Python unknown as expected. This is a single small fixture, not proof of semantic accuracy or repeatability; the embedded-term guard rather than model reasoning handled the Java distinction. A synthetic fixture tested retry and runtime failure; 36 fit tests passed with 21 live tests ignored, schema parsing, web Clippy and formatting passed. Before the first experiment, free system memory was 23.46 GiB and dedicated GPU usage was 10.1 GiB with the model resident; peaks were not measured. No employer site or application submission was involved.

## Visual mismatch review queue — 2026-10-30

When a form segment's local vision transcription directly differs from a reviewed answer, the visual-status handler now adds a persistent manual-review reason and includes it in the active form preview. A field not observed in a single segment does not by itself trigger this reason because it may appear in another segment. This is an advisory model observation: an incorrect transcription can cause a false warning, and an unobserved field or model error may still be missed. It does not establish that the form is complete or correct.

A fictional workspace test verified that a mismatch creates the review reason and an unobserved field does not; web Clippy and formatting passed. This checkpoint did not run a local vision model, render a live form or submit an application.

## Failed visual review queue — 2026-10-30

A failed form-vision task now retains its associated job ID. When its status is polled, the handler adds a persistent manual-review reason even if the form session has expired; an active preview is updated only for the matching session and job. A failed write is returned as an error. This does not cover a task that fails but is never polled, such as after the application exits, and does not imply that a completed visual review is accurate.

Fictional tests covered the job ID retained by a failed task, repeated polling, and the manual-review reason; 16 visual-review tests passed with four live tests ignored, and web Clippy and formatting passed. This checkpoint did not use a local vision model or a live employer form, and no application was submitted.

## Interrupted visual review tracking — 2026-10-30

Starting a form-vision task now records a task-specific pending review reason before launching the worker. A successfully processed result clears that reason only at its recorded revision; a failed result first adds the failure reason and then attempts to clear its pending reason. If the app exits before polling the result, the pending reason remains for manual review. Starting the worker unsuccessfully attempts to remove its own pending reason. This does not record failures that happen before the pending reason is created, such as an unavailable model or failed screenshot capture.

Fictional tests verified that a pending reason survives closing and reopening an on-disk workspace, and that stale completion cannot clear a newer or separate reason. The store can advance a remaining reason's revision while another reason is resolved, so an already completed review may conservatively remain pending and require manual resolution. Two focused tests, web Clippy and formatting passed; a broader visual-review group passed 17 tests with four live tests ignored before the reopen fixture was added. No local vision model, employer site or application submission was involved.

## Review reason revision preservation — 2026-10-30

Store migration v29 distinguishes an actual manual-review report from an internal queue-summary update. Resolving one reason no longer advances the revision of a different surviving reason, while reporting the same reason again still advances its revision even at the same timestamp. This removes the specific false-pending behavior described in the preceding checkpoint without weakening stale-resolution checks. Existing migration definitions were not changed.

A version-28 in-memory migration fixture covered preserved reasons, same-timestamp re-reporting, stale resolution and independent reason removal. The SQLCipher store suite passed 58 tests, including an older encrypted on-disk migration through v29; the focused pending visual-review tests, ordinary SQLite review tests, web Clippy and formatting also passed. A clean-host upgrade and a version-28 encrypted on-disk fixture were not separately exercised. No employer site or application submission was involved.

## Failed visual-status endpoint check — 2026-10-30

A fictional local HTTP endpoint test now exercises a completed failed visual task after its form session is gone. It verifies that polling returns the failure, adds the failure reason to the task's job and removes that task's pending reason. The fixture injects the failed task directly; it does not run a browser, model, or application submission. The focused endpoint test, web Clippy and formatting passed.

## Local visual placeholder evaluation — 2026-10-30

The fictional placeholder fixture can now run against a selected, already resident local vision model and requires both a confirmed filled name and an unconfirmed empty email field. In one headless-browser run with `gemma3:4b` (digest prefix `c30276f7dffe`, 4,096 context), the model transcribed the placeholder as an entered email; the independent live-value check rejected that match and confirmed the name. An attempted prompt clarification did not change the raw transcription and was discarded. This is one fictional page, not evidence that the model or DOM checks are reliable on other forms.

The strengthened live fixture passed; the visual group passed 18 tests with four live tests ignored, and web Clippy and formatting passed. The pre-load snapshot showed 28.39 GiB free system memory and 6.15 GiB dedicated GPU use; after loading, snapshots showed 25.86 GiB free and 10.06 GiB GPU use. The model was fully GPU-resident; peaks were not measured. No employer site or application submission was involved.

## Refresh visual review issues — 2026-10-30

The form interface now refreshes job state when a visual comparison differs from a reviewed answer or a live field value cannot be confirmed. This applies to both current and recovered visual reviews, so the existing manual-review filter and badge reflect the server's saved reason without requiring navigation. Fields merely absent from a segment do not trigger this refresh.

Two fictional local HTTP fixtures rendered the normal and recovered flows in headless Edge at 1200×900, with mocked APIs. In each, a visual mismatch caused a second state fetch and made the job appear under the manual-review filter; no browser console or page errors were observed. The Browser plugin was unavailable, so the checks used a temporary pinned Playwright Core installation in the ignored target directory. The screenshot content and results were fictional; no live backend, model, employer site or application submission was involved. Focused visual UI tests and JavaScript syntax checks passed.

## Visual pending state in local form workflow — 2026-10-30

The existing fictional local-browser prefill endpoint fixture now also seeds a task-specific pending visual-review reason before a completed visual result is processed. After the form changes, the status endpoint retains the separate changed-form review reason and clears that task's pending reason. The complete local fixture passed with one headless browser; it also exercises reviewed field filling, value changes, screenshots and application-record events. The visual result itself is injected by the fixture, so this is not a live model test or proof of behavior on employer sites. Web Clippy and formatting passed. No application was submitted.

## Interrupted form navigation tracking — 2026-10-30

Opening a reviewed application link or rescanning the current form now records a task-specific pending navigation reason before launching the worker. A processed result clears only that task's recorded revision after saving any needed review flag; a disconnected worker replaces its pending reason with a manual-review reason. If the app exits before the result is processed, the pending reason remains. If starting the worker fails, the handler attempts to clear its own pending reason and restores the prior session. A job already at the 20-reason limit may need review before a new navigation can start.

A fictional on-disk workspace test verified that pending navigation survives reopening and can be cleared without removing another reason. The local HTTP failure fixture verified that a disconnected worker leaves a failure reason and removes its pending reason. Nine navigation tests passed with three browser tests ignored; web Clippy and formatting passed. This checkpoint did not test a live employer site or automatic next-step controls and did not submit an application.

## CV check recovery — 2026-10-31

A CV readability check now retains an uncollected task and exposes its document and job IDs for recovery after a lost start response. Polling a finished task retains the latest response for a retry; completed responses and uncollected tasks are accessible for up to ten minutes, and a new task cannot replace an uncollected one. Retention is in process memory and checked when accessed; application restart loses the task. The result remains a literal text comparison, not an employer ATS score.

A local task test covered task identity, duplicate start refusal, repeated result retrieval and expiry. A simulated UI test covered a lost start response followed by a lost status response; it recovered the same check without launching another. Focused tests, JavaScript syntax and web Clippy passed. No employer site or model was used.

## Keep an open data-entry form — 2026-10-31

Application inspection now keeps the current page for review when it contains at least two required data-entry controls, even if another application link is present. This structural hint does not rely on field labels and does not establish that the page is an application: search or other forms may also match, while applications with fewer required controls may not. The existing manual navigation warning remains visible. No fields are filled or submitted by inspection.

A fictional local browser fixture with two required controls and another application link stayed on the original page; a synthetic test covered the threshold and excluded a checkbox. The form-inspection test group and web Clippy passed. This is not a test of a live employer site.

## Wide-page application captures — 2026-10-31

Application records can now capture wide pages in horizontal and vertical tiles, preserving the existing six-segment budget. A page up to 4,096 pixels wide can be captured when its height also fits within that budget; larger or more complex dimensions are explicitly marked incomplete. This does not establish capture of hidden content or nested scroll areas.

A headless browser captured a fictional 3,000-by-2,400-pixel page in four bounded tiles, including the tile covering the far-right field. The existing off-screen capture and collision fixture, tile-planning test and web Clippy passed. No application was submitted.

## Wide-page visual review — 2026-10-31

Form vision review now segments wide pages horizontally as well as vertically, using at most six captures. Freshness checks recapture the same tile, and application-record events include its coordinates. If the page exceeds the width, height or six-tile budget, coverage remains explicitly incomplete. Hidden content, nested scrollers, inaccessible frames and errors in the model remain possible.

A headless browser captured and recaptured the far-right tile of a fictional wide form. Geometry tests covered the six-tile limit and subdivisions; 20 visual-review tests and web Clippy passed. No local vision model or employer site was used in this check, and no application was submitted.

## Wide job-ad captures — 2026-10-31

Saved job-ad screenshots now use horizontal and vertical tiles, with each captured image's coordinates stored in metadata. The existing five-image and six-MiB total byte limits remain. Captures exceeding the tile or dimension limits are marked incomplete; a complete dimensional capture can still omit hidden content, nested scrollers or dynamic changes.

A fictional 3,000-by-2,400-pixel local page produced four positioned PNG segments; the existing listing-capture fixture, tile-budget test and web Clippy passed. No employer site or application submission was involved.

## Saved job-page view — 2026-10-31

New application-record folders with saved listing images now contain `original-job-page/view.html`. It positions captures with valid tile coordinates and warns when coverage is incomplete; captures without usable coordinates display up to five separate images. The view embeds no captured page text or URLs as markup and does not fetch remote content. Existing application-record folders are not backfilled.

A fictional browser fixture opened the local view and checked tile positioning. Metadata tests covered missing, overlapping and untrusted data; the attempt-record group and web Clippy passed. The view cannot reveal content that was not captured.

## Existing text before autofill — 2026-10-31

Before consuming a prefill approval, Waypoint now checks supported text, email, date, textarea and editable-text fields for existing values. An empty field or an exact match can proceed; a different or uninspectable value stops the batch for manual review. Each such field is checked again immediately before its command, so a later change can stop a partially completed batch. This does not inspect every custom control or eliminate races, and it does not authorize or submit an application.

A fictional local-browser fixture checked empty, matching, conflicting, textarea and editable-text values and confirmed that an existing conflict stopped the batch before its first command or approval consumption. A separate empty-field autofill fixture, the focused form-preview tests and web Clippy passed. No employer site or local model was used.

## Retry after an existing-value conflict — 2026-10-31

The fill endpoint now checks selected text fields before creating the attempt record or consuming approval. When that check finds a different or uninspectable value, it keeps the review session open, marks the job for manual review and lets the user unselect the field or inspect the browser before retrying. A later failure after commands begin remains a partial-fill case requiring inspection; the existing per-command checks remain in place.

A fictional local HTTP and browser test first encountered a conflicting field, then retried the same session with that field unselected and verified the original value remained unchanged. The existing endpoint partial-fill fixture, UI failure-handling check and web Clippy passed. No employer application was submitted.

## Existing choices before autofill — 2026-10-31

The existing-value preflight now also checks supported native selects, multi-selects, radio groups, datalists and checkboxes. A matching selection can proceed; a different existing selection stops the batch for review. An empty select value, no multi-select or radio selection, and an unchecked checkbox are treated as unselected. Some sites use empty or unchecked states as meaningful answers, so these checks cannot establish user intent; custom controls still require review. A default nonempty selection may also cause a conservative stop. The user can exclude the field or inspect the browser and retry.

Fictional browser tests covered empty, matching and conflicting controls. A local endpoint test confirmed that a selected-option conflict leaves the form unchanged and the session available for retry; the existing autofill endpoint fixture and web Clippy passed. No employer site, local model or application submission was used.

## Reviewed dropdown replacement — 2026-10-31

A form review can now display a supported native dropdown's current label and value and let the user explicitly approve replacing that specific selection with their reviewed answer. The approval is bound to the session, field and selection snapshot; the one-use prefill grant also binds the replacement approval and requested answers. A changed selection or edited replacement answer requires review again. Other existing-value conflicts still use the manual-review path, and native dropdown support does not cover datalists or custom controls.

A fictional local browser and endpoint test rejected an unreviewed replacement and a changed selection, then filled after a valid review. Focused UI tests covered acknowledgment and changes during snapshot retrieval; ordinary prefill tests and web Clippy passed. No model or employer site was used, and no application was submitted.

## Reviewed text replacement — 2026-10-31

The existing-answer review also supports bounded text, email, textarea, date and supported editable-text controls. It shows the current text locally alongside the reviewed replacement; an explicit acknowledgment binds the current value to the session, field and one-use fill grant. If the current text or reviewed replacement changes, the user must review again. Sensitive and model-flagged fields cannot use this path, and unsupported controls remain manual. The captured current text is not included in the attempt event; its digest is recorded with the reviewed replacement.

A fictional local endpoint and browser test rejected sensitive and stale text snapshots and completed a reviewed text and dropdown replacement. Focused UI and binding checks, a rich-text inspection fixture and web Clippy passed. No employer site, local model or application submission was used.

## Reviewed choice replacement — 2026-10-31

The existing-answer review now covers supported native multi-selects, radio groups and checkboxes as well as text and single-select fields. It shows the current state and reviewed replacement, requires explicit acknowledgment, and binds the state to the session, field and one-use fill grant. A changed state requires review again; sensitive, model-flagged and unsupported custom controls remain manual. Re-inspecting a field revokes its prior approval until the new state is acknowledged.

Fictional local browser and endpoint tests covered matching and conflicting choices, rejected changed checkbox, multi-select and radio snapshots, and completed reviewed replacements. Focused UI checks and web Clippy passed. No model, employer site or application submission was involved.

## Existing-answer review layout — 2026-10-31

The acknowledgment checkbox is aligned with its label in the existing-answer review. A fictional mocked-backend browser fixture exercised checkbox and multi-select replacement approvals and the no-fill retry path at 1,200, 800 and 390 pixels wide; no horizontal page overflow or unexpected browser errors were observed. The single HTTP 400 was deliberately returned by the fixture to exercise retry. This did not use a live backend, model or employer site, and did not submit an application.

## Visual comparison across segments — 2026-10-31

The form-review summary now combines live-value checks from the segments that actually observed a field. A field first seen on a later segment can therefore show a matching transcription and live value; any unconfirmed check, ambiguous label, duplicate observation or stale capture remains unconfirmed. Segment captures can still change between checks, and the model's transcription can be wrong.

Synthetic UI tests covered later-segment matches, conflicting live checks, mismatched expectations and changed captures. A fictional mocked-backend browser fixture rendered the later-segment match with no browser errors. No local model, live employer site or application submission was used.

## Converted text comparison — 2026-11-01

After converting UTF-8 text or Markdown as plain text, the background worker compares the source with selectable PDF text, ignoring whitespace differences. The result is advisory: a match does not check layout, reading order in an ATS, or visual correctness. A difference or unavailable extractor is reported without discarding the PDF. Sources over 200,000 bytes are not compared; DOCX and image conversions are not compared. The result panel remains visible in the current page and links to the PDF preview; it is not persisted across reloads.

A fictional Unicode text document was converted with the locally installed office and PDF runtimes and its extracted text matched. Unit tests covered differences, missing extraction, the size limit and conversion result recovery; a simulated UI test covered the result messages and preview action. No local model or employer system was tested.

## DOCX conversion comparison — 2026-11-01

DOCX-to-PDF conversion now compares locally extracted DOCX text with selectable PDF text after whitespace normalization. The DOCX extractor covers body, header and footer text but may miss other content; repeated headers, fields and layout changes can also cause differences. A match is not a visual or ATS compatibility check. Extraction failures leave the saved PDF available and report the comparison as unavailable. Images remain outside this comparison.

The same advisory comparison is now included when saving a template edit as DOCX and PDF, displayed after saving or recovering the task and included in a job-linked edit record. Direct PDF edits do not have a DOCX conversion comparison. Empty extracted source text is reported as unavailable rather than a match. A fictional edited DOCX matched with the bundled PDF runtime; a separate run with the default Python lacking the PDF package correctly reported the check unavailable. Neither check establishes visual correctness or complete content extraction.

A fictional styled DOCX was converted with local runtimes and produced a comparison result; the conversion tests, simulated UI checks and web lint passed. The result panel is not persisted across reloads. No employer site or local model was used.

## Filled-field record ordering — 2026-11-01

After successful form commands and their immediate live-value checks, the attempt record now stores the completed selectors and labels before requesting a review screenshot. The earlier requested-answers event contains the corresponding reviewed values. If this completion event cannot be written, the session remains open for manual inspection; a screenshot failure also attempts to record the failure and set manual-review status. A successful check is a point-in-time observation and does not establish later field values or submission.

A fictional local browser and HTTP fixture verified the new event and its ordering before the screenshot event. The screenshot-failure branch was not separately induced. No employer site or application submission was involved.

## Headless form review guidance — 2026-11-01

When a headless form inspection reports any manual-review reason, the interface now explains how to reopen in a visible browser and warns that closing the session may discard unsaved changes on the site. The same warning appears for review reasons produced after filling. This guidance does not transfer browser session state or navigate application sections automatically. Focused simulated UI checks passed; no employer site or submission was involved.

## Fit comparison model identity — 2026-11-01

New assessment comparisons require identical saved inputs, model name, reported 64-character model digest, context length and scoring version. The model identity is sampled before and after each assessment; if either sample is unavailable or differs, the assessment can still be saved but is not compared. The runtime or model could change between samples. Existing saved name-only comparisons retain an explicit older-comparison warning, and older assessments without identity data cannot be compared under the new rule.

Focused scoring and model-status tests passed. A resource-gated local model reported a 64-character digest and 4,096-token context; it was unloaded afterward. This did not run the complete fit assessment against the model or establish score accuracy.

## Duplicate candidate ordering — 2026-11-01

When more than twelve undecided employer-neighbor candidates are found, the model-search shortlist now reserves four places for the existing employer-token ordering and ranks the remaining eight by title overlap. This can bring a similar title ahead of unrelated roles without excluding all differently named roles. Selection still examines at most 100 indexed neighbors per employer token and at most eight tokens; it retains only the selected job records during ranking. It can miss matches beyond those limits or prioritize the wrong candidates, and no jobs are automatically merged.

Thirteen focused duplicate-store tests passed, including a fictional crowded-employer case with a similarly named role late in ID order; web lint passed. This is not a measured duplicate-recall improvement on real postings and did not use the local model.

Title comparison now ignores standalone bracketed or trailing gender notations such as `(m/f/d)` when identifying possible duplicates, while retaining other title words and single-letter skills. A database migration queues affected existing titles for bounded rechecking without removing prior decisions. A fictional same-employer title pair was flagged for review rather than merged; store tests and the encrypted-store migration test passed. This limited normalization does not establish duplicate identity or recall across job boards.

## Paged duplicate scan — 2026-11-01

The job details now offer a separate “Scan next group with AI” action. It walks indexed, undecided related-employer candidates by ID in groups of at most twelve, retaining a continuation cursor for one job in the current browser tab. The first group can overlap the initial ranked search. An incomplete model batch keeps the current cursor for retry; after all groups are reached, the scan can restart to include earlier newly added jobs. Each employer token reads at most thirteen indexed IDs per page, and already-reviewed pairs are skipped after bounded retrieval, including when an entire group has been reviewed. Job data, candidate IDs and cursor are bound to each result and checked again before saving suggestions.

Store fixtures covered 29 candidates across overlapping tokens and a fully reviewed group; web tests covered cursor progression, incomplete batches, input binding and recovery behavior. UI checks used a simulated backend, not a rendered browser. The scan remains limited to indexed employer tokens and model decisions can miss duplicates. No live model or employer site was used in this check, and no application was submitted.

## Paged duplicate scan verification — 2026-11-01

The page cursor now advances when all candidates were assessed, even if an initial batch failed and individual retries recovered it. It remains on the page when any candidates were not assessed or the counts are invalid.

A fictional local HTTP fixture used the resource-gated `qwen3.5:9b` model to assess two consecutive groups of twelve and two saved jobs; both completed with no unexamined candidates and no flagged matches. A separate rendered, mocked-backend fixture exercised both page actions at 1,200 and 390 pixels, with no browser errors or horizontal overflow. The Browser plugin was unavailable; the existing local Playwright Core and Edge were used. No employer site or application submission was involved. These fixtures do not establish duplicate-detection accuracy.

## Abandoned web-search result queue — 2026-11-01

A web-search worker now stops waiting after 30 seconds when its one-result queue remains full. Already delivered pages remain recoverable; the run reports failure rather than appearing complete. If a fetched page could not be delivered, coverage counts it as attempted and separately reports it as fetched but unreported. Cancellation keeps its existing stopped status and also accounts for a fetched page that could not be delivered.

Eleven focused batch-search tests passed, including a short-timeout fictional queue fixture, along with the simulated coverage-summary check and web lint. The production 30-second timeout and a live search provider were not separately exercised.

## Board discovery response recovery — 2026-11-01

Board discovery now retains the run's per-board reports and latest terminal response in memory. The interface redraws the report history rather than appending one-time responses and can recover an active run when its start response is lost. A lost status response can be retried without repeating already persisted board imports. A stopped worker retains its processed-board history and reports failure. Recovery is bounded to the active run and latest terminal response and does not survive an app restart.

A fictional local fetcher fixture covered incremental persistence, a degraded board, completed-result replay and a disconnected worker. A simulated interface check covered lost start and status responses. No live job board or employer site was contacted in this check.

## Focused-field screenshot interval — 2026-11-01

When focusing a completed field, the app now captures the viewport before and after its bounded live-value checks. A visible change prevents the field from being reported as retained and adds a manual-review reason; animation may cause this warning. When an application record is available, both screenshots are saved for a changed interval. A capture failure is reported for manual review. The checks remain sequential and cannot prove that the page did not change before, after or invisibly between them.

A fictional local browser fixture covered an unchanged screenshot and changes to filled values; a simulated interface check covered the changed-image warning. A changing screenshot during this endpoint was not separately induced. No local model, employer site or application submission was involved.

## Changing focused viewport fixture — 2026-11-01

A fictional local browser fixture now animates a visible page element while a completed field is checked. The endpoint reported a changed viewport, withheld the retained-value indication, recorded manual review, and saved distinct before-and-after screenshot files. The expanded fixture passed four runs after adjusting its final assertion to account for deliberately restoring a field. This tests a visible animation, not every possible dynamic or hidden field change; no model or employer site was used.

## Advisory button navigation — 2026-11-01

Form review can now inventory up to 30 detected button-like controls and ask the resident local model which labels may lead to another application section. The result is read-only: it does not click controls or authorize a submission. The interface displays the detected labels, marks disabled or repeated labels, reports an incomplete inventory, and warns that model suggestions and button labels may be misleading. The inventory uses the top document and rendered-visibility checks; frames, shadow content, hidden controls and controls after the limit can be missed. URL, form schema, walls and inventory are checked again before advice is returned, but this does not compare screenshots or field values and cannot prove a button's effect. Uncollected completed advice is retained for up to ten minutes while the process runs.

A fictional browser fixture covered accessible labels, disabled controls including an ancestor-disabled button, and absence of clicks. A separate local HTTP fixture rejected advice after the page URL changed. With resource-gated `qwen3.5:9b`, one English fictional page selected “Continue to questions” and omitted “Submit application”; a separate German-label fixture selected “Weiter zur Berufserfahrung” and omitted “Bewerbung absenden.” These are limited examples and do not establish model accuracy. A rendered UI check used a mocked backend at 1,200 and 390 pixels with no browser errors or horizontal overflow; the Browser plugin was unavailable, so the existing local Playwright Core and Edge were used. One resident sample showed approximately 20.6 GiB free system memory and 12.3 GiB dedicated GPU use, not peak measurements. The model was unloaded. No employer site or application submission was involved.

## Control-advice endpoint check — 2026-11-01

A fictional local HTTP and browser fixture exercised the asynchronous control-advice endpoint with resource-gated `qwen3.5:9b`: it returned the next-section label, omitted the submit label, replayed the result, and rejected it after the URL changed. No control was clicked. A separate disk-backed fixture verified that stale advice marks the job and current session for review and writes an attempt event when a record exists. The interface refreshes the review queue after an advice error. These examples do not establish behavior on employer pages or guarantee correct model suggestions.

A resident sample showed approximately 20.0 GiB free system memory and 12.3 GiB dedicated GPU use, not peak measurements; the model was unloaded. Focused UI and endpoint checks and web lint passed.

## Locate suggested controls — 2026-11-01

The button-navigation panel can now scroll toward a detected control and show a captured viewport. It binds the operation to the advice task, current session and a fresh URL/form/control snapshot, then rechecks previously filled fields and attachments within bounded limits. Changes, failed checks or checks beyond those limits are marked for manual review. The operation does not click a control; scrolling itself can trigger site behavior, and the image does not prove the control's identity or effect. The screenshot is added to an existing attempt record when available.

A fictional local HTTP/browser fixture scrolled to a control whose page cleared a filled name on scroll; the app reported the field for review and saved the capture. It also rejected the old advice after navigation. A separate fictional endpoint run used `qwen3.5:9b` to suggest and locate a control without clicking; one resident sample showed approximately 20.5 GiB free system memory, not a peak measurement, and the model was unloaded afterward. A mocked rendered UI fixture exercised the locate action at 1,200 and 390 pixels without browser errors or horizontal overflow; the Browser plugin was unavailable, so the existing local Playwright Core and Edge were used. No employer site or application submission was involved.

## Control-area visual transcription — 2026-11-01

The review panel can ask the resident local vision model to transcribe visible button text from a captured control area. It displays the exact image, the model's text, and a comparison with the accessible label; mismatches, repeated labels, failures and changed captures require manual review. Results are advisory: a matching label does not identify the button or establish its effect, and the model can misread or invent text. The capture and result are recorded when an attempt record is available. No button is clicked.

In one fictional local browser/API fixture, resource-gated `qwen3.5:9b` transcribed “Next” where the accessible label was “Continue to questions”; the comparison reported the mismatch. The same fixture rejected the visual result after navigation. This is one example, not an accuracy benchmark. Focused transcription and mocked UI checks and web lint passed. The model was unloaded after testing; no employer site or application submission was involved.

Control visual review now saves a task-specific pending review before starting inference. If the process or session ends before the result is collected, that marker remains in the workspace for manual review; a processed result clears only its own unchanged revision. A disk-backed restart test and an orphaned-result test covered recovery, and the fictional local model/browser fixture checked the pending marker before and after processing. Uncollected model output itself is still held only in memory and is not recovered after restart.

Targeted visual captures now require the target's bounding box to fit within the viewport, and the surrounding crop is limited to that viewport. A fictional browser fixture verified rejection of off-screen and partially clipped fields and successful capture after scrolling. This check does not detect every obstruction or clipping by an ancestor element; the captured image still requires inspection. The existing fictional control-vision fixture passed with `qwen3.5:9b`, which was unloaded afterward.

Button transcription now uses an enlarged capture of the target control's bounding box rather than the surrounding field context. In one fictional fixture, `qwen3.5:9b` transcribed “Next” from the narrower capture, without including text from the neighboring submit button; the accessible-label mismatch remained flagged. Overlapping elements and obscured or visually transformed content can still appear in the capture, and this single example is not an accuracy benchmark. The model was unloaded after the test.

When returning control vision results, the app now compares the control inventory both before and after recapturing the pixels. A changed or unavailable inventory is treated as stale; these checks do not detect changes that occur and reverse between observations. Focused interval checks and the fictional local browser/model fixture passed.

## Experience durations in fit assessments — 2026-11-01

A fictional local-model assessment incorrectly treated a fact stating seven years of Java and three years of Python as supporting a requirement for five years of Python. It returned 100 for that one-criterion assessment. The assessment now marks a duration-based judgment unknown when its cited fact contains multiple recognized durations, rather than letting repeated model judgments resolve the ambiguity. Saved assessments from before this change are stale and must be rerun. This is a conservative safeguard, not a general parser for every language or duration expression; it can also require review when the correct duration is clear to a person.

In the same fictional check after the change, the mixed-duration case was unknown with a 0–100 range, three years alone remained unknown, and seven years of Python was supported. The focused fit suite passed 44 tests (23 live tests ignored), and the three-case check passed with `qwen3.5:9b`; the model was unloaded afterward. These examples do not establish overall scoring accuracy.

The duration safeguard now recognizes common abbreviated and several non-English year and month units without depending on a short list of number words. It also considers nearby source text when a quoted skill omits the duration. It conservatively counts unit mentions and may associate nearby, unrelated requirements, so some cases require unnecessary review; other languages and expressions may still be missed. A fictional local-model case using “fifteen years” and “3 yrs” remained unknown, and older saved assessments are marked stale. German unit recognition was checked deterministically, not with a German local-model assessment.

## Reviewed answer snapshots — 2026-11-01

Each reviewed form-fill request is now also saved as a separate JSON file in the attempt's `answers` folder, with questions, requested values, provenance when available, and a note that the request is not proof of filling or submission. The event log links to the snapshot. If saving either the snapshot or its event fails, the app refuses that fill request and marks the job for review. These plaintext records may contain personal information, as do the other application-record files.

A fictional browser/API test checked that the saved snapshot matches the event's reviewed fields; a focused test checked separate snapshots and a write failure. No employer site or application submission was involved.

Each fill now also records a linked outcome in the same folder. A completed outcome identifies the commands whose values matched after filling; an interrupted outcome identifies commands completed before the failure without claiming those values remained correct. Both link to the corresponding requested answers and remain distinct from evidence of submission. The fictional browser fixture exercised both outcomes and their links. The records do not establish that the page retained the values afterward.

The application-record panel can now display the saved answer snapshots and linked outcomes for the selected job and attempt. Reads are bounded and report truncation or unreadable files; older attempts without the folder point the user to their event log. The panel displays requested values and labels completed versus interrupted fills without asserting submission or later value retention. A fictional end-to-end browser/API fixture and UI checks passed; a rendered standalone panel was checked at 1,200 and 390 pixels using local Edge and mocked data.

Saved job descriptions also contribute email contacts when no page capture exists. They may be pasted by the user or supplied by a board, and are labeled separately from captured page evidence. Discovery scans at most 32,000 UTF-8 bytes and displays at most eight contacts; truncation is reported. Contacts and local-model purpose suggestions remain unverified, and opening a draft does not send it. Focused tests cover description-only discovery, the API, retained page evidence, UTF-8 truncation, and the interface.

On 2026-11-01, resource-gated qwen3.5:9b tests passed for the two-pass contact classifier on a fictional shared-page browser fixture and through the local API, including stale-listing rejection. The initial checks exposed case-only quote transcription and a synthetic label/newline in the model context; the classifier now uses one source excerpt and restores a case-only quotation from the unique matching source text. Ambiguous matches remain rejected. These small fixtures do not establish general model accuracy. The model was unloaded after testing.

When the independent contact check gives invalid evidence for one address, only that address is downgraded to uncertain; other independently validated addresses can retain their advisory result. A malformed contact list still invalidates the entire check. Fictional tests covered a fabricated quotation, disagreement, and a malformed list; no live model was used for this change.

Control visual-review recovery now binds the pending task and completed result to the original session, advice task, and control index. Selecting another control while a review is pending reports the mismatch rather than presenting the wrong capture. Focused backend and simulated interface checks passed; no live model or employer site was used for this change.

A fictional browser/API test with resource-gated qwen3.5:9b verified that control-vision pending and replayed results carry the original session, advice task, and index; no page control was clicked. The model was unloaded afterward. The interface also recovers the original advice task when a visual review is pending after a refresh; a simulated UI test covered this route. Recovery remains limited to the in-memory task retention window and does not survive a restart.

The local CV readability check now flags DOCX sections declaring more than one column, and reports an invalid declared column count for manual inspection. Fictional tests cover alternate XML namespace prefixes, single columns and invalid counts. This is a structural warning, not an ATS score or proof of reading order; layouts made with tables, text boxes or other mechanisms need separate review.

The application-engine mapping validator now rejects blank required answers, unchecked required checkboxes, empty required file names or selections, and invalid calendar dates before form commands. Optional unchecked checkboxes remain valid. Focused tests and the application-crate suite passed; this does not establish that a live site will accept or retain a value.

Attempt folders now include `original-job-source-text.txt` when bounded source-only page text is available, including when no screenshot exists. `original-job.json` records whether the text was captured, truncated, unavailable or invalid, along with the source URL and whether it matches the current listing URL. The saved workspace description remains separate. Fictional disk-backed tests covered source-only and screenshot captures, missing and malformed text, and the size limit; web lint passed.

Duplicate-comparison freshness now binds the full saved listing text, even when the local model receives only the first 8,000 characters. The interface warns when either input was shortened. Fictional tests verified that changing a suffix beyond the model input changes the freshness digest, and that the warning appears; this does not prove the model's duplicate judgment is correct.

Duplicate comparison now limits each model input to 2,600 JSON-encoded bytes, including escaping, while retaining full-listing freshness checks and reporting truncation. Its freshness digest also includes the saved URL and remote flag. A decisive model suggestion without quotations from both inputs is downgraded to uncertain. With resource-gated qwen3.5:9b, the fictional comparison suite passed three short cases and returned a valid result for one pair of long, shortened listings; this is not a broad accuracy measurement. The model was unloaded after the run. Focused unit and UI checks and web lint passed.

When every scored criterion is unknown and the evidence range is 0–100%, the match badge, assessment heading, affected breakdowns and temporary review display now say there is insufficient evidence instead of presenting the 50% midpoint as a useful estimate. The underlying deterministic score and range remain available for accounting; an uncertainty range does not cover omitted criteria or model errors. Simulated UI checks passed for the badge, breakdowns and temporary corrections.

The per-user installer now checks destination-available disk space before creating its installation directory: the extracted app files plus a 512 MiB reserve must fit. It uses the current user's available space as reported by Windows, including for redirected destinations; free space can still change during the copy. Fictional installer tests passed in PowerShell 7 and Windows PowerShell 5.1, including simulated insufficient-space and overflow cases. No genuinely full disk or clean host was tested.

After a document is attached, a failed viewport capture or save is now recorded and marks the job for manual review instead of silently omitting that evidence. A focused fixture covered capture and storage failure plus a saved screenshot; the existing fictional browser prefill/attachment test and web lint passed. A live browser screenshot failure was not induced, and a saved screenshot alone does not prove server acceptance.

Form suggestions now use an explicit list of candidate contact fields. Application settings, preferences, template identifiers, work-authorization answers and future profile keys are not offered to the model or accepted as mappings. Fictional tests checked both exact-label and model-proposed attempts to map a setting; existing suggestion tests and web lint passed. Sensitive questions still require fresh user review.

On 2026-11-01, resource-gated qwen3.5:9b passed the fictional Portuguese contact/sensitive-question fixture after the contact-field allowlist change. The fictional German browser fixture failed repeatedly: it mapped the email but classified `Vorname` as requiring manual review despite the `given-name` hint. Two prompt variants did not resolve this and were discarded; the existing review gate was retained. The fixture additionally checks that the app setting is absent from the model prompt. These cases do not establish broad multilingual performance. The model was unloaded; no external form was contacted or submitted.

A follow-up on 2026-11-01 changed model-prompt formatting for groups consisting entirely of text controls with autocomplete hints; the hints remain untrusted and the existing review gates remain active. With resource-gated qwen3.5:9b, the previously failing German browser fixture passed. Fictional Portuguese, Spanish/French sensitivity, misleading-hint and adversarial-label cases, plus an existing production-mapping fixture, also passed. Applying the same formatting to all short forms failed the adversarial production fixture and was discarded. These few cases do not establish general accuracy; the model was unloaded afterward. The focused suggestion suite and web lint passed.

On 2026-11-01, resource-gated qwen3.5:9b passed fictional visual checks for a mismatch, a dense form, and a placeholder when combined with the live field-value cross-check. A new raw-vision-only placeholder benchmark failed: the model transcribed the placeholder as an entered value even after the prompt explicitly distinguished placeholders. The cross-check remains necessary and does not prove correctness on every site. The vision prompt also treats image text as untrusted data; this is an instruction, not a proven defense. Focused non-live tests and web lint passed, and the model was unloaded.

A 2026-11-01 attempt to start the locally installed `gemma4:12b-it-q4_K_M` for vision comparison was refused before loading: the resource gate observed 10,207 MiB free GPU memory against its 11,229 MiB conservative estimate. This sample is not a reservation or a measurement of the model's actual requirement; no quality comparison was made. Startup errors now include the observed and estimated memory amounts, with rounding and snapshot limitations. The resource limits were not relaxed. Focused resource-gate tests and web lint passed.

The local DOCX readability check now warns about explicit enabled hidden-text or web-hidden formatting in the document body and reports unrecognized on/off values. Extracted text can include material that is not visible in the rendered document. The check does not resolve inherited formatting, headers or all layout mechanisms. Fictional tests covered enabled, disabled and malformed markers, including the generated review report; web lint passed.

DOCX text extraction now preserves Word's nonbreaking and soft hyphen elements rather than joining adjacent words. The readability report also flags tracked-change markers in the document body so the user can inspect revisions before relying on extracted text. These checks do not resolve every Word revision, style or rendering difference. Fictional extraction and review tests and web lint passed.

DOCX extraction now uses an accepted-change view for explicit deleted and moved-from text: it omits those containers while retaining insertions and moved-to text. The separate readability warning still flags revision markers for review; this is not complete resolution of every Word revision or rendering feature. Fictional tests covered nested removed text, moved text and an unclosed revision; the existing readability tests and web lint passed.

Web-search coverage now displays up to eight names supplied by the provider for unavailable engines, while retaining the total reported count and indicating when entries are not shown. Unrecognized entries remain counted but unnamed; provider error messages are not retained in this display. Fictional parser, local-provider batch and simulated interface tests plus web lint passed. No external search provider was tested for this change.

Application-route checks now mention bounded email contacts found in the initial inspected listing's links, without claiming they accept applications or opening a mail client. Contacts still require user verification, and addresses remain available through the listing-contact workflow. Fictional unit and local-browser tests covered extraction and advisory wording; the complete worker was not exercised with a public HTTPS listing for this change. An initial file-URL worker fixture was unsuitable because file origins are opaque and was replaced with a direct browser fixture. Web lint passed; no email or application was sent.

Application-route checks now also count bounded email-like addresses in listing page text, combining them with link contacts and reporting a lower bound when inspection is incomplete. They remain unverified contacts, not confirmed application recipients. A fictional local HTTP server and browser exercised the complete worker with both forms of contact and observed only GET requests; the browser-only and focused tests and web lint passed. No public site or email service was contacted.

Route-check messages now include the bounded email addresses they found, so contacts observed only during the check can be copied for review. They are not automatically selected or sent to, and query parameters from mail links are excluded. This supersedes the earlier statement that every observed address would be available through the saved listing-contact workflow. A fictional local HTTP/browser worker test and focused tests verified both link and text contacts; web lint passed.

Contact extraction now omits an address whose domain reaches the end of truncated page or saved-description text, since the unseen suffix might change the address. Complete addresses earlier in the text and complete untruncated addresses remain available. Fictional boundary tests, the local route-check worker test and web lint passed; truncated text can still omit other contacts and requires review.

A current route-check result now exposes its bounded, unverified contacts beside the selected job, with an explicit button to open an unsent email draft for each one. No address is selected automatically. Contacts remain available when the completed response is replayed, but a changed listing URL permanently invalidates the result and clears its contacts, including if the URL is later restored. Fictional local browser, HTTP replay/staleness and simulated UI tests passed; no mail or application was sent. Contact results remain in process memory and are not restored after app restart.

Route-contact actions now show the captured surrounding listing text or link label when available, so users can review context before opening an unsent draft. Simulated UI checks and a standalone rendered Edge check with fictional data at 1,200 and 390 pixels passed with no page errors or horizontal overflow. Browser plugin not available; the rendered check used the existing local Playwright Core and Edge. This did not exercise a real listing or the complete application interface.

Duplicate indexing can now flag a pair when an employer is missing but the postings share a requisition identifier and have either similar titles or strongly overlapping descriptions. The pair remains pending for user review and is never merged automatically; missing-employer coverage remains explicitly incomplete because requisitions can be reused. Fictional tests covered both import orders, different titles with matching descriptions, unrelated roles and different identifiers. Existing jobs are not automatically reindexed by this change; the rule applies when a job is indexed or refreshed. The duplicate suite and web lint passed.

Database migration v31 queues existing missing-employer records with usable requisition identifiers for the existing incremental duplicate indexer. It removes only index markers and retains user decisions; rechecking a pair marked different does not reopen it. Fictional migration tests passed with ordinary SQLite and the SQLCipher feature, along with the store suite and web lint. The indexer processes one job per idle interval, so coverage remains pending until it catches up; no live workspace was migrated during testing.

Duplicate indexing now defers description reads and tokenization until title evidence is insufficient on its own. Existing different-title description comparisons remain in place; candidate reasons explicitly say when descriptions were not compared or were too short. Fictional tests covered the skipped-read path, unreadable descriptions when needed, and different-title matches. The duplicate suite and web lint passed. This reduces unnecessary work by construction, but no timing benchmark was run and the idle index step still runs on the web server thread.

Each idle duplicate-index step now groups its database changes in one transaction; a failure rolls back partial pair changes and leaves the job pending. In one fictional SQLCipher-on-disk run with 200 candidates and 25,000-byte descriptions, the same indexing path took 1.394 seconds before the transaction and 0.309 seconds afterward. These are single samples, not a controlled benchmark or a latency guarantee; the step still runs on the server thread. SQLCipher duplicate tests, rollback tests and store lint passed. Web lint was stopped while rebuilding a native dependency; it was not completed for this change.

Unsent email drafts now validate the recipient with the same ASCII address rules as the SMTP handoff. Malformed addresses are rejected during draft creation, and the message explains the ASCII limitation; this does not verify that a mailbox exists or accepts applications. Fictional draft and simulated handoff tests passed, and no mail was sent.

Duplicate indexing now uses one background database connection for disk workspaces, with the workspace lock retained for the connection lifetime. It processes one job at a time and falls back to idle server-thread indexing if the connection or worker is unavailable. Database writes can still contend and delay requests; a failed indexing transaction is retried later. A fictional SQLCipher workspace test exercised indexing alongside a candidate-data write, reopening, and lock retention; the server shutdown fixture and web lint passed. No latency benchmark or live workspace test was performed.

The background duplicate indexer now yields without waiting when another database writer holds the lock, and retries the same cursor after two seconds. It reserves a write transaction before reading the candidate to avoid a read-to-write upgrade race. Fictional SQLite and SQLCipher contention fixtures verified that pending work remains and can complete after the lock is released; the encrypted-workspace fixture and web lint passed. A write that starts while indexing already holds its reservation may still wait; no interface latency benchmark was run.

Text-field verification now refuses elements hidden by layout, visibility, or a fully transparent ancestor; a positioned offscreen field remains eligible for nonvisual value verification. A fictional real-browser fixture covered these cases. This does not detect occlusion, clipping, near-transparency or all visual deception. Resource-gated qwen3.5:9b passed the fictional changed-after-capture visual and live-value check; it was unloaded afterward. The installed gemma4:12b Q4 startup was refused at 10,207 MiB observed free against an 11,229 MiB estimate, so no comparison was made. No employer site was contacted.

Choice verification now also declines native checkboxes, selects and selected radio buttons hidden by layout, visibility or a fully transparent ancestor. Such controls require review, including custom-styled widgets that hide their native input; visible controls continue to be checked. Fictional real-browser fixtures covered visible and hidden choices and the existing value-retention flow; web lint passed. This does not establish visibility through clipping, overlap or every custom control.

Multi-tile application captures now stop and record an incomplete capture if the page address or dimensions change between tiles. A fictional comparison fixture and a normal four-tile browser capture passed, along with web lint. This check cannot detect content changes that leave the address and dimensions unchanged; dynamic-page changes during capture were not exercised in a browser.

Application page capture now recaptures its first tile and flags a pixel change, saving the later image for review; a fictional animated form and a stable four-tile browser fixture passed. This detects only changes visible in that sampled tile and may flag harmless animation. In another fictional page, Chromium changed the reported width by 15 pixels during capture, which triggered the existing dimension-change review even though this may be a scrollbar effect. The user may need to retry or review those captures; this has not been resolved. Web lint passed.

A capture whose dimensions change now gets one fresh, separately recorded attempt when the address is unchanged. A fictional narrow page that previously hit Chromium’s 15-pixel width change completed on retry; stable wide and changing-content browser fixtures and web lint also passed. Repeated dimension changes and address changes still require review, and the content check remains limited to the first tile.

Linking a completed PDF visual review now leaves the application marked for follow-up when the report contains model findings, overlapping text geometry, missing geometry or incomplete page evidence. The review remains advisory and the user can inspect the linked report; a clean result is not proof of correctness. Fictional Gmail-draft and local-browser attachment fixtures verified that follow-up is recorded, and focused classification tests and web lint passed. No application or email was sent.

PDF visual reviews now bind their freshness to the complete saved listing description, active fact digest and candidate profile, while the model still receives bounded excerpts. Document instructions are included in that bounded prompt. Changes outside the prompt now make the old review stale; reviews created before this fingerprint version also need to be repeated. Fictional tests covered long listing tails, unselected facts, profile changes and a saved review after fact correction; the browser attachment and email-draft fixtures and web lint passed.

Saved match assessments now bind to all active fact content, including facts omitted from the model’s bounded input. A change to an omitted fact marks the old score stale; the fact is still not evaluated until it fits the model input rules. The digest is reused across a batch of job reads. A fictional oversized-fact test and batch freshness tests passed, as did the focused fit suite and web lint. Existing saved assessments use the older fingerprint and require reassessment.

New match assessments report how many reviewed facts were excluded by the 1,000-byte per-fact limit or the 500-fact inventory limit. The score view warns that omitted qualifications were not assessed; older source-coverage records without these counts say coverage is unavailable. A fictional 503-fact fixture, simulated interface checks and web lint passed. Splitting a long fact still requires the user; this does not improve model recall or score accuracy by itself.

The qualifications panel now offers bounded sentence or line excerpts from entries over the matching size limit. Each excerpt requires the user to select and attest it before it is added as a separate qualification; the original remains intact. Long indivisible excerpts and text beyond the preview limit are reported for manual handling. A simulated interface check covered selection, Unicode byte limits and truncation; no local model or real candidate data was used. Splitting can remove context, so the interface asks the user to check each statement independently.

Section-control advice now includes bounded field labels, required markers, step status and detected walls from the same page snapshot as the button inventory. It does not intentionally send field values, although a site could put personal text in a label. In fictional tests, qwen3.5:9b suggested the section control and excluded the submit control, including a multilingual label example; a browser fixture confirmed the entered value was absent from the constructed context. The model was unloaded afterward. Advice remains fallible and does not click controls or submit applications.

On a fictional mixed-language control list, qwen3.5:9b initially omitted a German review-section control while excluding combined review-and-submit controls. A prompt revision asking for each eligible option returned both section controls and excluded the three combined submission controls in two runs; two earlier navigation fixtures also passed. This is a small model check, not a general guarantee. Advice does not click controls. The model was unloaded; web lint passed.

A fictional ambiguous-control test exposed qwen3.5:9b recommending finalization and submission labels despite the earlier prompt. A revised prompt excluded those labels in this test, but initially also missed a review-only control; a second revision passed both cases and the three earlier navigation fixtures. These are a few synthetic examples, not a reliability or safety guarantee. Control advice remains read-only and never clicks. The model was unloaded and web lint passed.

Wider-web search now retains valid HTTPS results without provider titles as explicitly untitled links, so the user can inspect them. If a later result on the same page supplies a title for the same URL, it replaces the placeholder; manual entry does not copy the placeholder into the job title. Coverage also reports examined results omitted as invalid or unsupported and repeats within a page. Fictional parser, local-provider batch and simulated interface checks passed; no public search provider or employer was contacted.

Form inspection now excludes fully transparent native controls from the visible-field inventory and flags them for manual review. This prevents a fictional listing containing only transparent email and upload controls from being described as a visible application form. Native file inputs continue to use their parent for visibility, since sites commonly hide the input itself. Browser fixtures and web lint passed; clipped, obscured and near-transparent controls remain limitations.

Form inspection now excludes controls fully outside an ancestor’s hidden or clipped overflow region and records a custom-control review reason. A fictional browser fixture covered a clipped input and a separate scrollable input; the application-check and prefill fixtures and web lint passed. Partial clipping, overlays, clip paths, scrolling and post-inspection changes are not fully resolved by this check.

Post-fill verification now rechecks rendered visibility for text and native choices, including the selected radio option. A fictional browser fixture verified that a field becoming fully clipped after inspection is no longer confirmed, while a scrollable offscreen field remains eligible. The local prefill fixture and web lint passed. This does not detect every partial clip, overlay or change after verification.

DOCX extraction now handles paired and self-closing Word tab, line-break and hyphen elements consistently. A fictional fixture checked the extracted text and namespace handling; the document-review endpoint test and web lint passed. This does not resolve inherited formatting or every DOCX layout feature.

PDF text extraction now reports pages with no selectable text. The document review displays the remaining text with page numbers to inspect; automated consumers that cannot carry the warning reject the partial extraction. Blank pages also trigger this conservative check, and pages with some selectable text can still contain unextracted images. Fictional complete and partial PDFs, the background extraction worker, simulated interface checks and web lint passed using the local bundled PDF runtime. OCR is not added by this change.

PDF pages without selectable text are now distinguished when they have neither a content entry nor annotations: those structurally blank pages are reported but no longer block automated extraction. Image-only and annotated pages remain review-required. Fictional blank, image-only and annotation fixtures, the background worker and simulated UI checks passed with the bundled PDF runtime; this is not a complete visual blank-page test and OCR remains unavailable.

PDF preview now offers explicit, single-page local vision transcription for scanned pages. The worker renders one bounded page and returns that exact image alongside unverified model text; selecting text only copies it to the qualification editor, where the user must separately review and add it. One running task is allowed, and an uncollected task can be resumed in the current process. On two fictional image-only fixtures, resource-gated qwen3.5:9b returned the expected text; one initially returned an array of lines, now accepted with limits. This is not an OCR accuracy benchmark, and the model may invent, omit or reorder text. Mocked UI checks and rendered Edge checks at 1,200 and 390 pixels passed; no application or email was sent.

Scanned PDF transcription now offers the full page or overlapping top and bottom regions, each shown with the exact image sent to the model. Regions are rendered directly from the source page, with a higher scale when dimensions permit and a bounded bitmap budget. A fictional regional fixture passed with resource-gated qwen3.5:9b, and PNG dimensions confirmed overlap. Mocked interface checks at 1,200 and 390 pixels passed. This does not establish improved accuracy on dense or real CVs; each result remains unverified and requires user review.

PDF transcription also offers overlapping left and right regions for side-by-side layouts, rendered directly within the same pixel budget. A fictional two-column scan passed one resource-gated qwen3.5:9b check for each column, and the interface test checked the selected region and image label. This does not establish transcription accuracy for real CVs; the output remains unverified and needs user review.

PDF extraction review now flags pages that have selectable text and image resources, because images can contain text omitted from extraction. A bounded resource walk includes nested form resources and reports inspection failures separately; it does not decode images. A fictional mixed image/text PDF and simulated UI checks passed. Logos may trigger the notice, inline images and other content may be missed, and automated consumers still receive selectable text without this warning; the notice is not a completeness guarantee.

The PDF text extraction pass now also flags inline image operators on selectable-text pages. A fictional inline-image fixture passed alongside the resource-image and empty-page checks. This remains an advisory inventory, not proof that every image or missing word is detected; logos can still trigger it.

Model-assisted qualification review now carries PDF image-coverage warnings through its result, including recovery of a completed task. The model still reviews selectable text only; images may contain additional qualifications and need separate inspection. A fictional mixed PDF, task-result and simulated UI checks passed. Other automated consumers do not yet carry this warning.

Model-assisted qualification review can now process selectable text from a partly scanned PDF and reports pages without selectable text in the result. Other extraction consumers without coverage handling still reject that partial PDF. A fictional mixed-page fixture passed a resource-gated qwen3.5:9b review, extraction and simulated UI checks; scanned content itself is not included in that review.

Form preview now omits text suggestions that exceed a field’s HTML length limit and flags them for review, allowing other valid suggestions to remain in the plan. The limit uses UTF-16 units as browsers do. A fictional two-field test covered an emoji and warning refresh after correction; the focused form-preview suite passed. This does not establish that a site will accept every suggested value.

Prefill now verifies each written field immediately and stops before later commands if that check fails; it also retains the final pass because subsequent changes can affect earlier fields. A scripted rejecting driver and local-browser fictional form passed. One existing browser fixture needed visible combobox text to exercise its intended review state. Immediate checks can require manual review for sites that validate asynchronously; they do not prove the site will retain values later.

A fictional local-browser form confirmed that when the first filled control becomes hidden, prefill stops before touching the second control. Verification errors now distinguish repeated labels by their position in the inspected form. Scripted and local-browser checks passed; this does not cover every dynamic validation pattern.

Visual form review now reports incomplete comparisons when a completed field has an unsupported or oversized value, or its mapping is unavailable, in addition to the existing 200-field limit. Previously such fields could be omitted without this warning. Focused comparison and UI checks passed; the notice does not identify which field was omitted.

Visual-review results now list up to 200 omitted field labels, distinguishing repeated labels where the form schema permits. The capture record and recovered review retain the labels; an unknown field is identified generically. Focused comparison and interface checks passed. The overall warning remains when the list itself is limited.

Search constraint evaluation now marks a missing or whitespace-only job location as unknown when a location is required, rather than a definite mismatch. Known mismatches still fail and surrounding whitespace is ignored in location comparisons. The focused search suite passed. Remote status is still represented as a boolean, so this change does not distinguish unknown remote status from a known non-remote role.

Ranking now carries unknown remote status explicitly: stored false values can mean either non-remote or missing data, so remote-only searches and location mismatches do not treat them as proven non-remote. Known false still fails when provided to the constraint API. Search and workspace ranking checks passed. Distinguishing source-stated non-remote roles in stored records requires a future data-model change.

Discovery now stores explicit remote evidence separately for Ashby isRemote, Lever remote/on-site workplace types and Jobicy’s remote feed. Evidence is bound to the listing content hash, so a changed or interrupted update cannot reuse an older status. Legacy false values and unsupported or hybrid source values remain unknown until rediscovery supplies evidence. Connector, store and workspace tests passed, including changed and missing evidence; no live board was queried.

Job cards now show whether remote work was stated, the source did not mark the role remote, or status is unconfirmed. The ranked result carries the same revision-bound evidence used by constraints. Fictional workspace and UI-helper checks passed; this does not classify hybrid arrangements.

Match assessment input now includes the current source remote indicator, explicitly leaving hybrid details and eligible locations unestablished by that flag. A changed indicator invalidates old assessments; the scoring version change also makes earlier saved assessments stale. A fictional qwen3.5:9b check left German remote eligibility unknown when the source only said remote, and focused fit tests passed. This single example does not establish general model accuracy.

A fictional local-model check found that the earlier false remote-flag wording was interpreted as excluding all remote work. Revised wording mentions possible hybrid work; qwen3.5:9b returned unknown for that case and for an absent flag, and supported for a true flag in two runs. Geographic eligibility remained unknown in a separate check. This is a small synthetic evaluation, not a general guarantee.

Lever discovery now reads top-level workplaceType and leaves conflicting or hybrid values unknown. On 2026-11-01 a bounded public Spotify Lever API response returned HTTP 200 with 89 postings (SHA-256 52e3f546e146d22bafb5e2384c1b7435b7d1afb543baf8f5ded053be7e74d181); an offline check compared the parsed status to that captured payload. Connector fixtures and workspace ranking tests also passed. This captures one board at one time, not all Lever variants.

Lever discovery now combines its introduction, titled list sections and additional text, preferring plaintext where supplied and retaining bounded HTML-to-text fallback. The previously captured public response had list sections on all 89 postings; an offline check confirmed the first nonempty list section appeared in each parsed description. A fictional workspace check verified requirements in a list contribute to ranking. Output remains capped at 100,000 characters and 100 lists, without a per-list truncation report.

Lever discovery now retains all listed location alternatives, and location constraints compare each alternative individually. Job cards separate the locations visually. An offline check covered all 34 multi-location postings in the previously captured public response, and a fictional workspace test verified matching a secondary location. Existing listings need rediscovery to receive the extra locations.

Lever list items now retain line boundaries in saved descriptions instead of merging adjacent bullets. The captured public feed used standard closing list-item tags in 248 of 267 list sections; an offline content check and a fictional qwen3.5:9b requirement-audit test passed. Nonstandard or encoded closing tags may still be flattened, and one model fixture does not prove general extraction accuracy. Existing jobs need rediscovery.

Lever and Ashby fields explicitly supplied as plain text now retain literal angle brackets, ampersands and line boundaries instead of being treated as HTML. Fictional connector tests and the previously captured Lever feed check passed. A read-only request to the Ashby public API returned HTTP 403 on 2026-11-01; no live Ashby payload was obtained or retried. Existing descriptions need rediscovery.

Location constraints are now evaluated independently of remote-only selection. A remote flag alone does not establish that the candidate can work from a requested region, even if the posting names an office there, so that outcome is explicitly unknown. Search and workspace checks passed. The system does not yet parse geographic remote eligibility into a confirmed constraint.

The web profile now saves a remote-only setting and up to 50 exact allowed locations. The job-list state uses these settings: unknown outcomes are labelled, definite mismatches remain visible after other jobs with reasons, and the settings also enter local match-assessment preferences. Fictional state-endpoint, validation, interface-helper and desktop compatibility checks passed. Location matching is exact, remote geographic eligibility remains unknown, and model interpretation of these new preference strings has not been benchmarked.

A fictional local-model check covered Berlin/Paris as alternative locations: two matching listings and one explicit mismatch were classified as expected in two runs. A separate remote listing with unspecified eligible regions was initially misclassified as a mismatch. The existing absence-of-evidence guard now also recognizes “not established” and “unspecified” in cited text; that fixture then returned unknown in two runs. Other phrasings or citations may still be misclassified.

A rendered Edge check of the search-settings flow at 1,200 and 390 pixels found the empty job list showed pagination despite its hidden attribute. The hidden state now takes effect, while pagination becomes visible when enabled. The mocked profile save and summary also worked at both widths; this did not exercise the real server or candidate data.

Local search-phrase planning now includes the saved remote and location filters, binds the complete instruction text for freshness, and reports qualifications or preference text omitted by input limits. A fictional qwen3.5:9b run produced phrases containing remote and a requested location; context-binding and UI-helper checks passed. Phrases remain review-required and this does not establish that all requested locations are covered.

Search-plan freshness now binds all active qualifications, including those omitted from the bounded model input. A fictional oversized qualification correction invalidated the earlier plan even though the model prompt was unchanged. Focused binding tests passed; omitted qualifications still are not considered by the model and remain reported in coverage.

Location filtering now marks a whole comma-separated component match, such as “Berlin” against “Berlin, Germany,” as unknown for review instead of a definite mismatch. It does not treat substrings such as “York” in “New York” as a match. Search, fictional workspace and rendered settings checks passed. Geographic aliases and eligibility are still not resolved automatically.

Duplicate decisions already reopened when core job fields changed. They now also reopen for changed descriptions, remote flags, source URLs, source names and requisition identifiers. The database records the previous same/different decision before reopening it, and the review reason survives candidate rescanning. Fictional store tests covered same/different decisions, unchanged updates and rollback on logging failure. Decisions made before this migration cannot be reconstructed from the new log.

Reopened duplicate pairs now show the most recent prior decision, which listing was kept when applicable, and which listing changed. An index keeps the latest-history lookup bounded per pair. Fictional store and UI-helper checks passed; decisions predating the history migration remain unavailable.

Web duplicate decisions now carry a per-pair revision. Material job or description changes advance it even while review is pending; stale and repeated decisions are rejected atomically, and the interface refreshes on failure. Fictional store and local API checks passed, including a listing change between state read and decision. This protects the web decision path; older internal store calls remain separate.

New application-record job snapshots now write the revision-bound remote indicator as true, false or null; null means the status is unconfirmed. The snapshot also explains that the indicator does not establish geographic eligibility or hybrid arrangements. Fictional archive tests covered all three states. Existing snapshots remain unchanged.

New application records retain the expected original-page image count. The record list checks the capture folder, metadata, viewer and expected image files for presence, and flags missing or inaccessible files. Older records without a count are marked unknown. Fictional archive and inventory tests passed. Presence does not verify image contents, completeness or provenance.

Before writing text, dates, checkboxes or native selects, the browser now rechecks the target's rendered visibility, including fully clipped ancestors. In a fictional browser fixture, a first answer hid later controls; text, select, checkbox and date writes were refused without changing those controls. Two existing browser fixtures also passed. This does not cover radio-group targets, partial clipping, overlays, or page changes between the check and the write. No application was submitted.

Radio selection now checks visibility of the chosen option before changing it, including fully clipped ancestors, even when another option represents the group. Text entry also rechecks visibility after focus events. A fictional browser fixture verified hidden options and a field hidden on focus were left unchanged, while a visible option in a group with a hidden representative could be selected. Existing form-fill fixtures passed. Partial clipping, overlays and changes after the checks remain limitations.

Text entry now rechecks focus and visibility between replacement operations and multiline input events. A fictional browser fixture moved focus to another field during a rich-text answer; entry stopped without writing into the other field, while a stable multiline editor still worked. Page changes between checks and individual browser events remain possible.

Drafting now carries PDF template extraction coverage into the draft review. If selectable-text pages also contain images or their image inventory is unknown, the interface warns that the model saw selectable text only. A fictional mixed-image PDF and interface checks passed; scanned pages without selectable text still stop this drafting path. The warning cannot detect every image or omitted word.

Drafting can now use a bounded local vision transcription of one scanned PDF template page as style guidance when selectable text is missing. The draft shows the transcription for review and reports that other pages were not transcribed; it still uses reviewed qualifications for factual support. A fictional scanned PDF exercised the real renderer and a fake vision response, and partial-PDF and draft tests passed. No local model was loaded for this check, so transcription quality and end-to-end model behavior remain unverified.

On 2026-11-01 the resource-gated qwen3.5:9b completed a fictional scanned-template CV draft through the actual background worker. A second run also found the fixture name, date range, Rust, SQL, German level and Berlin in its vision transcription. Ollama reported 5.5 GB and 100% GPU; the model was unloaded afterward. This is one synthetic template, not an accuracy benchmark or proof that every generated claim is supported.

In two further resource-gated qwen3.5:9b runs, the scanned-template draft test checked that six template-only details (name, dates, SQL, language and location) did not appear literally in the generated paragraphs. Both passed; the model was unloaded afterward. This narrow string check cannot detect paraphrases or other unsupported claims, so the draft and its advisory evidence check still require review.

The scanned-template draft review now includes the exact rendered image sent to the vision model alongside its transcription, so the user can compare them in place. The retained image is bounded by the existing 4 MiB transcription limit; older results without it show an unavailable notice. A fictional PDF renderer/vision fixture checked image identity, and simulated interface checks covered image display and the missing-image case.

The scanned-template review now lets the user correct the transcription and explicitly request a new draft with the corrected text as style guidance. The server binds the request to the retained original draft, job, document kind and current context; the original model transcription and page image remain available alongside the correction. Fictional context-validation and interface tests passed, and resource-gated qwen3.5:9b completed a fictional initial draft and corrected redraft. The model was unloaded afterward. Drafts remain advisory, and the source draft and its review are held in memory only.

The application-route check can now follow a single vetted, simple fragment route such as #/apply, preserving the fragment for the browser. It still rejects query strings, encoded or complex fragments, ambiguous destinations and obvious final-action routes. A fictional local browser fixture verified that a fragment route exposes a form; link-filter tests and web lint passed. This does not establish support for every single-page application or a usable employer application.

Image-only template PDFs now retain page coverage even when no selectable text exists. Drafting skips structurally blank pages when choosing its one page for vision, rather than assuming the first page is usable; ordinary text extraction still rejects image-only PDFs. A fictional blank-then-scanned PDF selected page two using the real renderer and a fake vision response. The existing scanned-template draft and redraft fixture also passed with resource-gated qwen3.5:9b, which was unloaded afterward. Structural blank detection is conservative and is not a visual blank-page guarantee.

The match review can now temporarily change a criterion between required and preferred and shows how that changes the deterministic estimate and range. Split criteria inherit the chosen importance. The interface explains the 3:1 weighting and that these changes are not saved or applied to the job badge. Simulated checks covered weight changes, split criteria and existing badge behavior; this does not establish that the model assigned the right importance.

Match review can now save user-selected judgments and required/preferred weights as a separate scenario in the protected workspace, while retaining the original AI assessment and badge. Saving requires explicit review and an assessment revision; stale inputs and conflicting edits are rejected. Split scenarios remain temporary, and a new AI assessment replaces the saved review. Fictional encrypted-workspace API and interface checks covered saving, reopening and stale rejection; web lint passed. The resulting score is still a scenario, not an independently validated measure of suitability.

Job cards now show a saved user-reviewed scenario beside the original AI estimate, with both ranges where available. A fully unknown reviewed scenario is labeled insufficient evidence. The batch endpoint returns only the review score and range, and stale assessments do not show either estimate. Fictional batch and interface checks passed; neither score is a calibrated probability.

The jobs panel can explicitly sort the current filtered list by saved reviewed match scores, reading batches of 20 sequentially for at most 2,000 jobs. Definite constraint mismatches remain last; missing, stale and fully uncertain reviews remain visible in their original order within each constraint group. Sorting is a snapshot and the interface prompts for a new sort after a review changes. Fictional interface checks covered ordering, batching, the cap and incomplete results. This is prioritization of user-reviewed scenarios, not a calibrated hiring prediction.

A rendered fictional sorting fixture exposed narrow job titles when both AI and reviewed badges appeared. Job cards now place the badges below the title, and sorting controls align with the job rail. Mocked Edge checks at 1,200 and 390 pixels verified sorting, restoring default order, no page overflow and no page errors. This did not exercise a real workspace or employer site.

Native month inputs can now be detected and filled with an explicitly reviewed YYYY-MM answer. Year/month format, site min/max and the exact retained value are checked; full dates and months cannot be substituted for one another. A fictional local-browser fixture and answer-validation tests passed without submission, as did a workspace compatibility check. An attempted separate mocked fill-interface test was rejected by automatic approval review without a stated reason, so rendered UI behavior remains unverified.

When a new AI match assessment replaces an earlier one, up to five previous user-reviewed scenarios and their original rubrics are retained in the protected workspace. They are displayed as historical and never applied to the current score; older omitted reviews are counted. Fictional encrypted-workspace and interface checks covered retention, replay, bounds and invalid history. Earlier reviews already replaced before this change cannot be recovered.

The local model startup size cap is now 10 GiB, allowing larger installed GGUF models to reach the existing conservative memory gate. The system-RAM reserve, GPU estimate, single-model check and 14 GiB fully resident limit remain in force. Synthetic gate and interface checks passed. On 2026-11-01 an attempt to start the installed 14B model was refused before loading: 10,207 MiB GPU memory was observed free against a 12,443 MiB estimate. No quality evaluation was performed, and snapshots do not reserve memory or prove peak usage.

Ministral 3 14B Q4 is now an optional local-model installation choice. On 2026-11-01 the native Ollama registry manifest response was HTTP 200, 858 bytes, SHA-256 4760c35aeb9d9e9c6174c2492562c0b999e80a222804fd96b1915ab72bbcdcf7, matching the locally installed model. Local metadata reported vision support and an Apache 2.0 license. A live inventory check verified the already-installed model without pulling or loading it; mocked installation and interface tests passed. The earlier resource gate refused startup on this host, so this change does not establish runtime fit or model quality.

Completed AI CV and cover-letter drafts are now saved individually in the protected workspace when their result is collected. The job panel can reopen the newest 50; a changed job, profile, qualifications or template marks a saved draft historical and prevents treating it as current. Reopened current drafts can retain their evidence for export, and export provenance now hashes rather than embeds the potentially large transcription image. Fictional encrypted-workspace tests covered save, replay, reopen after workspace restart, context changes and bounded provenance; store and simulated UI checks passed. If secure saving fails the draft remains temporary with a warning. An uncollected draft can still be lost on process exit, and saved drafts are not submitted applications.

A draft completed after its job or profile context changed is now saved and shown as historical instead of discarded. The historical view includes opening and closing text, and does not offer current-draft export. Fictional encrypted-workspace and simulated interface checks covered the changed-context completion and reopening. A draft whose result is never collected can still be lost when the process exits.
