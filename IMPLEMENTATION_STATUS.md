# Implementation checkpoint

## First-pass conflict review — 2026-10-29

- The conservative negation prerequisite for candidate-fact contradictions now also applies when validating the initial model rubric and later rubric updates. A partial positive fact without an English negation marker is counted as unknown, not as a contradiction. This marker does not prove a conflict, may miss implicit or non-English conflicts, and the model's semantic judgments still require review. The input digest version was changed so existing assessments become stale.
- In a fictional end-to-end test, the resident qwen3.5:9b model left one year of SQL unknown against a five-year requirement. It also left an explicitly stated six-year Python fact unknown against a five-year requirement. An experimental follow-up search failed to recover that fact and was removed; this false negative remains unresolved. One example is not a general accuracy measurement.
- The web suite passed (193 passed, 64 ignored), along with web Clippy and formatting. The live test used one already GPU-resident local model after a free-memory preflight; peak resource use was not measured. No application was sent.

## Explicit conflicts in additional fit evidence — 2026-10-29

- The additional search for initially unknown requirements can now propose an explicit contradiction as well as support; the separate qualification comparison must agree before the judgment contributes to the score. In this recovery path, a contradiction is considered only when the cited fact has an English negation marker. This is a conservative lexical prerequisite, not proof of a relevant conflict; other languages or implicit conflicts may remain unknown. It does not govern the first-pass rubric.
- The resident qwen3.5:9b model produced 87% on all three repeated runs of one fictional assessment. The recovery pass proposed an unrelated fact for SQL on each run, and the qualification comparison downgraded it. In a separate fictional near-miss test, both model passes incorrectly treated one year of SQL as proving insufficient total experience. After the guard, that item remained unknown, six years of Python supported the five-year requirement, and an explicit statement of never holding a nursing licence was marked contradicted. The result was 50%, range 33–67%. These cases do not establish general model accuracy or repeatability; negation can refer to unrelated context and model agreement can still be wrong.
- The previous Python-evidence live case also passed after the change. The web suite passed (192 passed, 63 ignored), as did web Clippy and formatting. Live tests used one already GPU-resident local model after a free-memory preflight; peak resources were not measured. No application was sent.

## Additional evidence for unknown requirements — 2026-10-29

- Fit assessment now asks the local model to revisit up to six initially unknown listing criteria against the selected candidate facts. Candidate facts must be referenced by valid index and then pass the existing separate qualification comparison before they affect the score. Responses with missing, duplicate, or invalid IDs or fact indices, model failures, and payloads over 6,000 bytes leave the original unknowns in place. The UI reports selection and proposal counts; the changed input digest marks older assessments stale.
- On the previously recorded fictional qwen3.5:9b case, the first pass again missed explicitly stated Python experience. The additional search proposed the existing fact, the independent comparison agreed, and the resulting rubric marked Python supported while leaving the unstated SQL and chemistry doctorate unknown. The score was 70% with a 40–100% range. One example does not demonstrate broad accuracy, repeatability or calibration, and two passes of the same model can agree incorrectly.
- The web suite passed (192 passed, 62 ignored), along with web Clippy, formatting and the fit-badge UI test. The live test used one already GPU-resident model with a 4,096-token context after a free-memory preflight. Peak resource use was not measured. No application was sent.

## Qualification judgment cross-check — 2026-10-29

- Local fit assessment now asks the resident model to independently compare cited candidate facts with listing requirements and nearby job context. It does not show the initial judgment to the second pass. Agreement is not verification of truth; disagreement, malformed or unavailable responses, and requests over the 6,000-byte payload budget downgrade affected judgments to unknown. The interface reports the check and its limitations. Existing assessments become stale through the input-digest version change.
- A fictional live test of qwen3.5:9b with a 4,096-token context retained a six-year Python fact against a five-year requirement and downgraded a one-year SQL fact against a five-year requirement. In a separate end-to-end case the model incorrectly marked Python unknown even though the candidate fact explicitly stated Python use; this cross-check only revisits positive and negative judgments and does not fix that false negative. These two cases do not establish general accuracy or repeatability.
- The web suite passed (191 passed, 62 ignored), web Clippy, formatting, and the fit-badge UI test passed. Live tests used one already GPU-resident local model after a free-memory preflight; peak resources were not measured. No applications were sent.

## Local fit repeatability and page-vision checks — 2026-10-29

- The fit rubric now warns when a preference cites job text that also contains a listing criterion. This is a possible overlap, not proof of semantic duplication; the existing score is unchanged. The fixed weighted arithmetic is repeatable for a fixed rubric, but model-selected criteria and judgments remain subjective.
- Three successive runs of the resident qwen3.5:9b model on one fictional fit case each produced 87% with a 73–100% range. The model counted remote location in both a listing criterion and a preference, illustrating possible double counting. This single case does not establish general repeatability, accuracy, or calibration.
- Application checks may request a local vision assessment of the current viewport, with a bounded exact DOM text excerpt, current URL and screenshot freshness checks, and a single-model lease. They stay advisory and require manual review. A conservative limited-language cue check downgrades unsupported application-form claims; it is not a comprehensive classifier. The model initially labeled a fictional support-contact form as an application form; the guard downgraded that same unsupported claim in a subsequent live run. One fictional application form and one contact form are not evidence of general performance, and DOM text does not prove visible evidence.
- The web suite passed (190 passed, 61 ignored); web Clippy and formatting passed. Live checks used one already GPU-resident local model with a 4096-token context after a free-memory preflight. Peak resource consumption was not measured. No employer application was submitted.
- The configured CocoIndex MCP points to another repository and its daemon failed on non-UTF-8 settings. No shared registration was changed; semantic navigation of this repository remains unavailable until an isolated current-root index is established.

## Application-check destination evidence — 2026-10-29

- A completed application-page check now includes the actual inspected page URL in its result when it is public HTTPS, remains on the original origin, and has no credentials, query or fragment. URLs with those components or cross-origin redirects are not displayed. The result still explicitly requires manual review and does not establish that an application can be submitted. The existing source URL remains the freshness key.
- Five focused application-check tests passed with one browser fixture ignored, covering URL eligibility, query and redirect exclusion, stale results and result replay. Web Clippy and formatting passed with one Cargo job. No real employer site, live browser or submission was exercised for this change; peak resource use was not measured.

## Automatic discovery of a local converter for packaging — 2026-10-29

- Windows prototype packaging now automatically selects a LibreOffice distribution in standard machine or per-user locations when its executable and license are present. `--libreoffice-dir` selects a different source; `--no-libreoffice` opts out. Bundling still inventories and checks copied files and may substantially increase package size and packaging time. If LibreOffice is absent, packaging reports that separate setup is needed; automatic download and clean-host installation are not implemented.
- Discovery selected this host's installed LibreOffice, and fictional packaging and installer fixtures passed, including a missing-license candidate. An attempt to copy the real distribution into an isolated temporary folder and run a conversion was rejected by automatic approval review with the stated reason `blocked by policy`; therefore no copied-distribution conversion or full real bundle was qualified. The installed converter was tested in the earlier plain-text PDF checkpoint, which is not proof that a bundled copy works.
- Crawl4AI was unavailable through the current tool inventory, PATH and default Python environment; no external page research or download-source claim was made. Peak packaging resources were not measured.

## Pending PDF conversion recovery — 2026-10-29

- The interface can now discover a running or finished-but-uncollected PDF conversion after a lost start response or tab refresh, and prefers the current server task over an older tab-stored ID. Status retries reuse the existing saved result instead of writing a duplicate document. Recovery is in-memory only and does not survive an application restart.
- Three focused conversion tests and the UI recovery helper test passed. A local HTTP fixture started a fictional Markdown conversion with installed LibreOffice, discovered the same task through the new pending endpoint and saved the PDF. Web Clippy, JavaScript syntax and formatting passed with one Cargo job. No rendered browser session, real document, employer site or submission was exercised; peak resource use was not measured.

## Plain-text PDF conversion — 2026-10-29

- Saved UTF-8 `.txt` and `.md` files can now be converted to PDF through the existing local background conversion workflow. Markdown is converted as plain source text, not rendered markup; the UI says so. Input is bounded to 10 MiB, invalid UTF-8, NUL and empty text are rejected, and the original saved document is retained. Conversion uses the existing isolated LibreOffice profile, 30-second limit and 768 MiB Windows worker limit.
- A fictional Unicode text fixture converted with the installed Windows LibreOffice. Text extraction preserved `München · São Paulo`, and a rendered first page was visually inspected; the resulting layout was a plain monospace page, not a professionally typeset CV. Separate background-task and local HTTP endpoint fixtures saved a PDF and retained the original Markdown. This does not verify every language, long document, clean host or missing-dependency setup; other file formats and automatic conversion remain incomplete.
- The web suite passed 186 tests with 60 live/browser tests ignored; three LibreOffice fixtures ran separately. Web Clippy, JavaScript syntax and formatting passed with one Cargo job. No model, employer site or submission was exercised; peak resources were not measured.

## Unclassified model batches — 2026-10-29

- If an active model's sensitivity review cannot be obtained for a batch, its fields now remain manual-only for that review session, including fields with exact autocomplete hints. Other successfully reviewed batches remain usable. Missing or invalid review lists still get one bounded repair; runtime failures are not retried. The session reports that classification is unavailable, rather than asserting that each field is sensitive. This change does not alter the separately available offline exact-hint mode when no model is selected.
- Tests cover an invalid repair alongside a valid second batch, an unreachable model, and propagation into the form preview. The web suite passed 185 tests with 57 live/browser tests ignored. The local-model repair and fictional Portuguese browser/prefill/vision fixtures passed separately with resident `qwen3.5:9b` after RAM/residency preflight. Web Clippy and formatting passed with one Cargo job.
- A valid model review can still miss sensitive fields or flag ordinary ones; offline hints are not a sensitivity classifier. No real employer page or submission was exercised, and peak resource use was not measured.

## Bounded form-sensitivity repair — 2026-10-29

- When a mapping response is received but its sensitivity-review list is missing or malformed, the app now makes one bounded local repair request using field descriptions without candidate profile values. Valid known flags from either response are retained. If repair fails or is invalid, the existing warning remains and exact fallback may still be offered; model inference failure itself is not retried by this mechanism.
- A fictional test deliberately removed the initial review list and used resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context) for the repair. It flagged the Portuguese work-eligibility field and prevented a misleading email autocomplete fallback. This single example does not establish general classification accuracy or eliminate false negatives.
- The web suite passed 182 tests with 57 live/browser tests ignored; the repair fixture was run separately after RAM/residency preflight. Web Clippy and formatting passed with one Cargo job. No real employer site or application submission was exercised; peak resource use was not measured.

## Model-flagged browser prefill fixture — 2026-10-29

- A fictional Portuguese browser fixture now exercises the local model's review flags through the real prefill boundary. The work-eligibility field has a misleading email autocomplete hint and the consent checkbox uses a neutral ID. With resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context), both fields were flagged and the separate contact email was mapped. A forged flagged-field mapping was rejected before the simulated grant was consumed; the approved fictional contact value was then filled, while browser checks and visual observations showed the flagged fields blank. The fixture did not submit.
- The live browser/model fixture passed after RAM/residency preflight; five focused non-live form-preview tests passed with nine browser/model tests ignored. Web Clippy and formatting passed with one Cargo job. This is one synthetic layout with simulated approval, not a general accuracy or real-employer result; peak resources were not measured.

## Form review with empty profiles and non-text controls — 2026-10-29

- Model-assisted sensitivity review now runs even when no usable profile keys are available, and it inspects checkbox, date, file and multi-select labels as well as mappable controls. The existing deterministic mapping restrictions remain in force. An explicit prompt distinction leaves ordinary contact fields unmapped rather than classifying them as sensitive solely because the profile is empty.
- A fictional local `qwen3.5:9b` test (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context) flagged Portuguese work eligibility and consent questions with and without a profile email; the ordinary email field was not flagged in the final empty-profile run. An earlier prompt incorrectly flagged that email and was revised. The fictional local two-step prefill and vision fixture also passed. These are limited examples: model errors, unclassified batches and offline fallback can still miss sensitive wording.
- The web suite passed 181 tests with 55 ignored; the two live model fixtures were run separately after RAM and residency preflight. Web Clippy and formatting passed with one Cargo job. No real employer site or application submission was exercised; peak resources were not measured.

## Model-assisted form review flags — 2026-10-29

- The local mapping model can identify additional fields for manual handling, including wording not covered by the fixed sensitivity markers. Valid flagged selectors suppress both model mappings and exact-autocomplete fallback. The review session presents these fields as sensitive and refuses them in generated answers, reviewed prefill and file attachment paths; the underlying browser schema remains unchanged for freshness checks and approval hashes. A missing or invalid model review list produces a warning, and recognized flags from a partially invalid bounded list are retained.
- With resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context), a fictional Portuguese work-eligibility label not recognized by the fixed list was flagged even though its autocomplete hint misleadingly requested email; a separate email field remained mapped. A local-browser test verified a model-flagged field is rejected before a simulated prefill grant is consumed. The model can still miss sensitive wording or flag ordinary fields, and when its review list is unavailable the warning does not convert all fields to sensitive. This is not a guarantee of safety on other pages or languages.
- The full web suite passed 179 tests with 55 ignored before the final added regression fixture; the added form-preview and reviewed-answer tests passed separately. The local-browser grant test and local-model Portuguese test passed separately, as did the prior multilingual-browser and production-mapping model fixtures. Web Clippy passed with one Cargo job. No real employer page or submission was exercised; peak resources were not measured.

## Fictional multi-step review — 2026-10-29

- Headless form review now explains how to reopen the application in a visible browser when a possible next-step control is detected. Navigation still requires the user's action; the app does not automatically click a next button as a result of this change.
- A local two-step fictional form exercised the existing production mapping, reviewed prefill and visual-observation paths with resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context). A test-only click simulated the navigation gesture between steps. After reinspection the browser filled the second field, left work authorization empty and the visual model transcribed the visible values on both steps. The fixture recorded no submission. This does not validate autonomous navigation, real employers, different layouts, or the browser UI rescan endpoint end to end.
- The live model/browser fixture passed after a RAM/residency preflight; four non-live form-preview tests passed with eight browser/model tests ignored. UI guidance and visual-comparison checks, web Clippy and formatting passed with one Cargo job. Peak resources were not measured.

## Multilingual prefill and visual fixture — 2026-10-29

- A fictional local-browser fixture now exercises the existing local-model mapping, a simulated reviewed prefill grant, DOM verification and screenshot-based model observation together. With resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context), the browser filled the German first-name and email fields, while five German, Spanish and French sensitive fields stayed empty. The vision response separately transcribed the two values and reported each of those five fields blank. No submit action was taken.
- The dedicated live fixture passed after a RAM/residency preflight; the non-live form-preview suite passed four tests with seven live/browser tests ignored. Web Clippy and formatting passed with one build job. This is one synthetic static layout and a simulated grant, not a measurement of accuracy on real sites, difficult controls, other languages or peak resource use.

## Multilingual sensitive-form review — 2026-10-29

- The shared browser sensitivity check now includes common German, Spanish and French labels for work permission, compensation, protected characteristics, birth dates and consent. Common short English markers are matched as whole words so a name such as Grace is not marked sensitive solely because it contains `race`. These triggers do not cover every wording or language; sensitive and uncertain answers still require candidate review.
- A real local-browser fixture marked five fictional sensitive fields in German, Spanish and French while leaving German name and email fields available. A separate browser-plus-local-model test with `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context) confirmed that the raw model output mapped the two contact fields and the sensitive selectors were absent from the mapping prompt. The fixture did not fill or submit the form and does not prove accuracy on real employer sites or other languages.
- Application tests passed 30/30 before the final marker additions; the sensitive-marker test passed again afterwards. The focused suggestion suite passed 13 tests with two live tests ignored, and the browser and model fixtures were then run separately. Application, browser and web Clippy and formatting passed with one Cargo build job. Model/browser preflight checked available RAM and residency; peak resources were not measured. No application was submitted.

## Numeric quote context — 2026-10-29

- Preference comparison now also checks a bounded same-line context around a uniquely occurring citation when the citation contains a complete numeric bound. This catches a model citing only the upper or lower endpoint of a crossing interval; the outcome is unknown and requires review. The fallback does not inspect repeated citations, split-line ranges or citations without a whole bound, and the existing limits on number formats and semantic interpretation remain. Unrelated nearby numeric text may also require review. Saved assessments from the preceding scoring version become stale.
- Twelve focused fit tests passed with five live-model tests ignored, including fictional endpoint-only citations, repeated quotes, split lines, Unicode boundaries and the model-response integration. Web Clippy and formatting passed with one Cargo build job. No live model, real listing, browser session, peak-resource measurement or application submission was exercised for this change.

## Grouped numeric interval review — 2026-10-29

- The conservative preference check now recognizes comma- and dot-separated three-digit groups as candidate integers and downgrades both supported and contradicted model judgments when a cited interval straddles the preference's number. It does not resolve locale, units, decimal values, mixed separators or unrelated numbers; it only flags a potential ambiguity for manual review. Existing assessments become stale.
- The fictional local-model ambiguity fixture now includes a grouped salary case. In the initial run the model incorrectly marked the overlapping range as contradicted; after the guard covered both judgment directions, four of five cases matched expectations. The accessibility-auditing case still failed, so the live qualification test remains failing. This small test does not establish general accuracy; the guard depends on the model citing the full interval.
- Eleven focused fit tests passed with five live tests ignored. The qualification test was run separately with resident `qwen3.5:9b` after a RAM/residency preflight and failed as above. Web Clippy and formatting passed with one Cargo job; peak resource use and real employer listings were not measured. No application was submitted.

## Numeric preference interval guard — 2026-10-29

- A model-supported preference is downgraded to unknown when its single plain integer lies above the lower bound and at or below the upper bound of a plainly written two-integer interval in its cited listing excerpt. The review explanation identifies the numeric overlap. This generic conservative guard does not interpret units, signs, decimal/grouping formats, all interval expressions or semantic meaning; it cannot verify a model judgment and may require manual review even for unrelated quantities. Earlier saved fit assessments become stale.
- In the four-case fictional live-model ambiguity benchmark using resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context), the previously false 100% salary result became unknown; three cases matched expected outcomes, while the accessibility auditing case still incorrectly scored as supported. The benchmark therefore still failed at 3/4 and does not measure general accuracy. An additional counterexample prompting experiment also failed to reliably reason about the cases and was not integrated.
- Eleven focused fit tests passed with five live tests ignored, including interval boundaries and a fictional model-response integration test. Web Clippy and formatting passed with one Cargo build job. The live benchmark was run separately after checking available RAM and model residency; peak resources, other models, real listings and application submission were not measured.

## Salary minimum range boundary — 2026-10-29

- The deterministic salary constraint now reports a range that crosses the user's minimum as unknown, including when the range maximum equals the minimum. A comparable range passes only when its stated lower bound reaches the minimum; a stated maximum below the minimum fails. Missing or invalid bounds remain unknown where they do not establish the outcome. Even a stated matching range is not a guarantee of an eventual offer.
- Eleven search tests and 40 workspace tests passed, with two live network tests ignored; search Clippy and formatting checks passed with one Cargo build job. The current workspace ranking path still supplies no employer salary to this checker, and the separately generated model fit score is not corrected by this change. No live employer listing or application was exercised.

## Preference ambiguity and temporary score review — 2026-10-29

- The match detail now lets users explore changes to individual rubric judgments and recalculates the same fixed-weight midpoint and range. These changes are temporary: they do not save corrections, update the job badge, alter the stored assessment, or establish that other criteria were checked. The model output and its cited evidence remain visible.
- A new ignored live-model qualification test exercises four fictional ambiguous preference cases. On local `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context), only 2/4 judgments matched the expected result: the model incorrectly treated web accessibility auditing as accessible product development and the top of a salary range as a guaranteed minimum. Each erroneous single-criterion assessment displayed 100% with a 100–100% rubric range. The test failed as intended; an additional prompt attempt still scored 2/4 and was discarded. This is a small fictional benchmark, not a general accuracy estimate. The model's assessments remain unverified and users must check them.
- Ten focused fit tests passed with five live tests ignored; the new live qualification test was run separately and failed as described. JavaScript syntax and fit badge/control checks passed, including changing a temporary judgment and checking the recomputed range. No rendered browser session, real listing, application submission, or peak-resource measurement was performed.

## Listing-backed preference comparison — 2026-10-29

- Fit assessment now makes a separate local-model request for up to six extracted preference excerpts of at most 200 bytes, using at most the first 3,000 bytes of the listing. Its reported selection count, truncation and failure are visible. Every returned ID must be unique and complete, and every positive or conflicting judgment must cite an exact quote from the supplied listing excerpt. An unavailable or invalid response leaves the prior conservative assessment intact; unselected preferences remain unknown. The match detail shows cited listing text for both matches and conflicts. Older assessments become stale.
- A fictional four-preference test with locally resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context) returned the expected supported, contradicted, contradicted and unknown statuses. Three full-assessment repeats for a second fictional case each scored 87%, range 73–100%. In that case the model inferred accessible product development from an accessibility requirement even though the prompt requested explicit evidence; the cited text alone cannot validate that inference. Neither example proves general accuracy, stable results across models or runs, or that similar criteria are not double-counted. The score remains an estimate for review, and missing or truncated evidence can change it.
- Ten focused fit tests passed, plus the two separately run live fixtures. Web Clippy, JavaScript syntax and fit badge checks passed with one Cargo build job. An earlier concurrent rebuild failed because Windows had locked the running live-test executable; the sequential retry passed. Resource preflight required one fully resident model and 12 GiB free system memory; peak usage was not measured. No employer site or application submission was exercised.

## Fit evidence and repeatability checkpoint — 2026-10-29

- A preference is no longer credited or contradicted solely from a candidate fact: absent a valid exact listing citation, it becomes unknown. The current prompt does not request that citation, so preferences normally remain unknown and need manual review. The headline is derived from rubric counts instead of model-generated prose. Saved assessments with the prior input version are stale.
- A fictional repeatability fixture ran three times against locally resident `qwen3.5:9b` (digest `c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a`, 4096 context). Before the change all three runs scored 87%, but each incorrectly claimed in its summary that an unverified SQL requirement was met and used the candidate's own remote preference as evidence of the job. After the change all three scored 73%, with a 47–100% range and both unverified preferences marked unknown. This is one fictional case on one model, not a general repeatability or accuracy benchmark; criteria selection, weighting, duplicate concepts, model judgments, omitted evidence and unsupported explanations can still affect the score. A prompt requesting listing citations failed validation in a preliminary local run and is not enabled.
- Nine focused fit tests passed (three live tests ignored in that run), followed by the separately run three-repeat live fixture. Web Clippy, JavaScript syntax and fit badge checks passed with one Cargo build job. Preflight showed one model fully resident with approximately 5.13 GiB reported VRAM and at least 12 GiB free system memory before loading; peak memory was not measured. No application was submitted.

## Search-plan identifier check — 2026-10-29

- Locally generated search phrases are checked against normalized known profile identifiers, including the full name, email, address, links and phone number. Unsafe phrases are omitted with a visible count; if none remain, planning fails. The identifier list is not added separately to the model prompt, although user-provided qualifications or instructions may contain identifying text. This is not an exhaustive PII detector, and users are still asked to review phrases before searching.
- The plan digest includes the identifier list so a profile change invalidates old suggestions. Three focused planner/context tests, the planner-recovery UI test and web Clippy passed. A synthetic Edge fixture displayed the omission count and review reminder at 1200 and 375 pixels without issuing a planning request. The local Ollama inventory reported zero resident models and NVIDIA GPU telemetry was unavailable; no model was loaded and no live model or public provider was exercised. Peak memory was not measured.

## Visual label comparison checkpoint — 2026-10-29

- Visual field comparison now treats ASCII line breaks, repeated spacing and ASCII letter case consistently in both the backend and web UI. Duplicate normalized labels remain unobserved, and values must still match exactly. Non-ASCII formatting is not silently normalized.
- Eight focused visual-review tests, the UI comparison test and web Clippy passed with one build job. The model comparison was tested with fictional observations, not a live vision model or employer form; peak resource use was not measured.

## Direct upload-form inspection checkpoint — 2026-10-29

- Form inspection and application-page checking retain a page that already has a visible file-upload control instead of following another application link. This is a hint to review the current page, not proof that it accepts applications; pages without an upload continue through the existing link-selection path.
- Four focused form-task tests and three application-check tests passed, with their other local-browser tests ignored. A separate headless browser test passed against a fictional local upload form without submitting. Web Clippy passed with one build job; real employer layouts and peak resource use were not measured.

## Search planner input checkpoint — 2026-10-29

- Local search planning now reads at most 20 eligible reviewed facts through the existing bounded sealed-data path instead of loading the entire fact history. Proposed phrases still require user review before a search.
- Plans are bound to their selected facts and instructions. The status endpoint reports stale when those inputs changed, and the UI rechecks before copying a phrase into a search field. The web and Jobicy start endpoints check an unchanged copied phrase's binding again before starting a search. Editing the phrase makes it a manually authored search; it still requires user review.
- A pending-plan endpoint and a one-result in-memory replay let the UI recover an active or completed plan after a lost response or tab refresh. They do not survive an application restart and do not launch web searches automatically.
- The encrypted-workspace test passed with more than 20 fictional facts, a corrected fact, an unreviewed fact and an oversized fact; the selected 20 matched the prior full-read ordering. Three planner/context tests, planner recovery and binding UI tests, and web Clippy passed. Local handler tests rejected stale bindings for web and Jobicy starts before launching a provider. A synthetic headless Edge fixture displayed fictional recovered phrases and rejected a stale phrase at 1200 and 375 pixels without issuing a new planning request. No real local model or public provider was exercised for these changes. Peak memory was not measured, and other paths may still load larger data sets.

## Search coverage reporting checkpoint — 2026-10-29

- Search results now distinguish an omitted engine-availability report from an explicit zero and show examined versus provider-returned result counts. The denominator is one provider response, not global market coverage.
- The latest completed single-search result or failure remains available for repeat status reads, so a lost response need not discard its candidates. A pending-task endpoint lets the web UI resume a search whose start response was lost while its task remains active or unread. The tab retains its task ID across refreshes and prioritizes a newer active task. Only one terminal result is retained in memory; recovery does not survive an app restart.
- Four web-search tests, the search coverage and single-search recovery UI tests, and web Clippy passed; the existing visual-comparison UI test also passed. A local HTTP handler test recovered a fictional pending task, and a synthetic headless Edge page with mocked responses prioritized a newer task and restored its result across two reloads without another start request. A separate synthetic fixture displayed the unavailable-status wording at 1200 and 375 pixels. These checks did not test a running app or a public search provider. Hosted CI and peak resource use were not measured.

## Source-only listing capture checkpoint — 2026-10-29

- When screenshot capture fails, listing metadata can now be sealed separately using DPAPI and retained without claiming an image was captured. A later screenshot capture keeps its own metadata, while salary evidence continues to refer to the earlier retained source. The salary detail warns when the capture URL differs from, or cannot be compared with, the current job URL. Application records include the source-only metadata separately from screenshots.
- Store tests passed 46/46; a populated v24-to-v25 migration fixture also passed with SQLCipher enabled. The web suite passed 144 tests with 37 ignored, and web Clippy passed. Fictional local handler tests exercised both matching and different capture URLs, the no-screenshot result, source retention, salary retrieval and a later separately recorded screenshot. A synthetic Playwright fixture displayed the different-URL warning at 1200 and 375 pixels. A clean-host upgrade and real-site screenshot failure were not exercised; peak resource use was not measured.

## Confirmation scan boundary checkpoint — 2026-10-29

- The bounded application-reference scan now stops at a UTF-8 boundary and refuses an identifier token cut by the scan limit. This prevents a panic and avoids accepting a partial reference as a confirmation; it does not send an application.
- Five focused driver tests and `cargo clippy -p waypoint_applications --all-targets -- -D warnings` passed with one build job. The web-app suite subsequently passed 144 tests with 37 ignored, and web-app Clippy passed. No live employer response was tested; peak resource use was not measured.

## Listing metadata access checkpoint — 2026-10-29

- Existing listing-capture metadata can be read without loading or decrypting screenshot segments; repeated listing inspection uses this path. Structured source remains in the existing encrypted capture when capture succeeds. Job details can request bounded `baseSalary` evidence from one saved structured posting and display its stated fields and capture date for manual review. This is evidence from the original capture, not a salary comparison; filter wiring, employer-salary normalization and fresh-host testing remain incomplete.
- The DPAPI capture test passed, including metadata access when a fictional screenshot segment is corrupted. The web listing-inspection suite passed 8 tests with 3 local-browser tests ignored, including captured salary and ambiguous-posting cases. A local HTTP handler test passed with a fictional encrypted capture and checked returned salary evidence and timestamp. The real display function was exercised in a synthetic headless Edge page with a mocked response at 1200 and 375 pixels; it showed the evidence, date and verification caveat without horizontal overflow or console errors. This was not a running-app end-to-end test. One Cargo build job was used; peak resource use was not measured.

## Salary comparability checkpoint — 2026-10-29

- Salary thresholds now carry optional currency, gross/net basis, period and compensation category. If these are absent or disagree with a stated job amount, the comparison is explicitly unknown; a lower bound below the threshold no longer acts as an upper bound. Invalid inverted ranges are also unknown. Older settings without the new optional fields remain readable.
- `cargo test -p waypoint_search` passed 9 tests, including a legacy-settings compatibility check, and `cargo clippy -p waypoint_search --all-targets -- -D warnings` passed with one build job. The current workspace ranking path does not yet supply parsed employer salaries, and the UI does not yet collect these new comparison attributes; this change alone does not make salary filtering fully functional. Peak resource use was not measured.
- Ranking now carries unknown constraint reasons to each included result; the web detail and desktop row expose them instead of presenting them as confirmed passes. Workspace tests passed 40 with two ignored across unit and integration suites; desktop tests passed 13, the web suite passed 142 with 37 ignored, and the web lint check passed. The web interface still requests default constraints, so the new warning path was not exercised through its settings UI.

## Windows lint helper checkpoint — 2026-10-29

- `build-windows.ps1 -Clippy` uses the same OpenSSL discovery and one-job default as build and test. Conflicting `-Test`/`-Clippy` options and a filter without `-Test` are rejected before discovery.
- On this host, with OpenSSL environment variables cleared, the helper detected the existing OpenSSL installation and linted web-app targets successfully. `test-build-paths.ps1` passed. This does not install missing dependencies or verify a clean host; resource peaks were not measured.

## Required-field review checkpoint — 2026-10-29

- Required fields mapped to empty text or an unchecked checkbox remain in the review list both when preparing a preview and after autofill. When a user changes answers, generated required-field warnings are recalculated without removing unrelated review notes. This list is not browser validation; separate attachments and subsequent page changes still require inspection. The focused form-preview suite passed 3 tests with 4 local-browser tests ignored for the preview change. The required-field test passed again after warning refresh was added. A headless browser test passed against a fictional local form, checking offline suggestions and an unmapped required field. A separate local browser endpoint test passed after checking that filled fields lose stale warnings and unrelated warnings remain. These tests did not exercise the empty-value case in the browser or submit.
- With one Cargo build job and one test thread, the web-app suite passed 142 tests with 37 ignored. `cargo fmt --all -- --check`, the UI syntax and visual comparison checks, and the local build and package fixture checks passed. Direct `cargo clippy` without the helper's OpenSSL environment failed to find `OPENSSL_DIR`; the retry with the existing OpenSSL installation configured passed. This checkpoint did not exercise a live employer form or submit an application.

## Optional document conversion bundle — 2026-10-29

- The Windows packager can include a supplied LibreOffice distribution with `--libreoffice-dir`. It inventories the files by SHA-256, retains the distribution license and lets the installer verify the files before copying them into the existing private converter location. This is an optional packaging input, not automatic download or setup.
- `test-package-windows.py` verifies hashes and rejection of a missing license with a fictional three-file distribution. `test-install-package.ps1` copied fictional packages with and without a converter and verified that a damaged converter or absent license inventory is rejected before an installation directory is created; no executable was launched. Both scripts are registered in Windows CI but hosted execution has not been checked. On 2026-10-29 the local LibreOffice 26.2.5.2 distribution (13,930 files, 857,096,674 bytes) streamed through the bundler into a discard sink in 54.7 seconds. No archive, installer or conversion was run in that check. A full package, clean-host installation and bundled conversion remain untested. Resource peaks were not measured.
- Automatic dependency provisioning remains incomplete, including the OpenSSL development build prerequisite and automatic installation of LibreOffice, browser and Ollama. No applications were sent.

## Model inventory layout checkpoint — 2026-10-29

- The local model list has a bounded height, wraps long names and can receive keyboard focus for scrolling.
- `node --check apps/web_app/web/app.js` passed. A synthetic static fixture using the current stylesheet and 100 fictional entries was checked in headless Edge at 1200 and 375 pixels: the list remained bounded, had no horizontal overflow, and scrolled with Page Down. This did not exercise the running application or its model inventory endpoint.
- Automatic dependency provisioning and clean-host installation remain incomplete. The package includes OpenSSL and Python; the development build still requires an OpenSSL development installation, and browser, document conversion and model setup are not automatically provisioned. Peak resource use was not measured. No applications were sent.

## Visual comparison checkpoint — 2026-10-29

- Current revision: see `git log -1`. Visual comparison labels do not report a match when any capture changed or could not be checked for freshness; duplicate and missing observations remain unverified.
- With `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`, the eight `visual_review::tests` passed and `cargo clippy -p waypoint_web --all-targets -- -D warnings` passed. `node apps/web_app/tests/visual-review-ui.test.js` and `apps/web_app/scripts/test-build-paths.ps1` passed. The Windows CI workflow now runs both checks.
- A real vision-model review, rendered-browser verification, hosted CI, the full workspace suite and peak resource use were not measured for this checkpoint. No applications were sent. Automatic Windows dependency installation remains incomplete.

## Local model inventory checkpoint — 2026-10-29

- Revision: `2202d6c`. Settings can list up to 100 installed Ollama models and their reported disk sizes. Disk size is not a GPU memory or readiness measurement; the app does not install or load models.
- Validation with `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`: `build-windows.ps1 -Test` passed 141 tests with 37 ignored. `cargo test -p waypoint_web model_inventory_http_reads_local_ollama -- --ignored --test-threads=1` passed against the existing local service without loading a model. `node --check apps/web_app/web/app.js` passed.
- The full workspace suite, a clean-host installation and peak resource usage were not measured. No applications were sent. Automatic dependency and model provisioning remain incomplete.

## Local web prototype checkpoint — 2026-10-29

- Revision: `2f0cc0c`.
- The local fit assessment chooses up to 20 reviewed qualifications from the first 500 eligible facts using listing text overlap. The bounded fact read shares the full-history unsealing implementation. A fictional encrypted-workspace test checks corrected, unreviewed and oversized facts.
- `build-windows.ps1` keeps its OpenSSL search paths stable across repeated invocations. Automatic development dependency provisioning remains incomplete; the packaged runtime includes OpenSSL and Python but other tools still need setup.
- Validation on Windows with `CARGO_BUILD_JOBS=1` and `RUST_TEST_THREADS=1`: `build-windows.ps1 -Test` passed 140 tests with 36 ignored; `cargo test -p waypoint_store` passed 45; `cargo test -p waypoint_workspace` passed with one live journey ignored; `test-build-paths.ps1` and `node --check apps/web_app/web/app.js` passed.
- The full workspace suite, live model, third-party employer workflows, fresh-host installation and resource peaks were not measured for this checkpoint. No applications were sent.
- Next: complete automatic, verifiable Windows dependency provisioning and validate it on a clean host; continue end-to-end local-fixture application preparation and visual review.

## Planner validation checkpoint — 2026-10-29

- A generated plan retains usable phrases when individual suggestions are invalid or contain known personal identifiers. The interface reports omitted counts and still requires review; the identifier check is not exhaustive. Plans remain bounded to ten suggestions.
- With one build job and one test thread, three search-plan tests, eight visual-review tests and web-app Clippy passed. The search-plan recovery, binding and visual-review JavaScript checks passed. No model was loaded or application sent. A real model, installed-app startup, third-party applications and peak resource use were not measured for this checkpoint.

## Longer form visual comparisons — 2026-10-29

- Visual review compares up to 200 supported filled fields across captured segments, increased from 20. Further fields are explicitly reported as omitted. Each model response remains limited to 20 observations per segment, and review remains advisory; hidden content may be missed.
- With one build job and one test thread, nine visual-review tests passed (one unrelated PDF test ignored), web-app Clippy passed, and the visual comparison UI check passed. No live vision model or employer site was tested; peak resource use was not measured. No application was sent.

## Dense visual segments — 2026-10-29

- When a segment returns the maximum 20 model observations, the application is marked for manual review and the normal and recovered review flows warn that additional fields may have been missed. The saved visual result includes the limit indicator. Fewer observations also do not prove completeness.
- With one build job and one test thread, ten visual-review tests passed (one unrelated PDF test ignored), web-app Clippy passed, and the visual comparison UI check passed. No live model, real employer site or resource peaks were measured. No application was sent.

## Adaptive dense-segment review — 2026-10-29

- A visual segment reaching the 20-observation limit is rechecked sequentially in two non-overlapping halves; at most 18 captures are attempted for six segments. The job remains flagged for manual review because fields crossing the split or otherwise hidden can be missed. Saved captures identify their half and freshness recaptures use the same geometry.
- With one build job and one test thread, eleven visual-review tests passed (one unrelated PDF test ignored), web-app Clippy passed, and visual comparison and adaptive-retry UI fixture checks passed. A live model, employer site, full application startup and resource peaks were not measured. No application was sent.

## Conflicting form mappings — 2026-10-29

- When a model suggestion conflicts with an available exact-label or autocomplete suggestion, neither value is proposed for that field. Other mappings remain available; required fields are flagged for manual review. Matching values still require user review and do not establish correctness.
- With one build job and one test thread, seven suggestion tests, three non-browser form-preview tests, and two targeted conflict tests passed; four browser tests were ignored. Web-app Clippy passed before the final test-only addition. No live model, employer site or resource peaks were measured. No application was sent.

## Partial model form mappings — 2026-10-29

- Invalid, unknown or restricted individual model mappings no longer discard unrelated valid mappings. Duplicate or invalid known selectors remain unmapped, including from exact fallback; required unmapped fields remain review obligations. Model results and exact hints still need user review.
- With one build job and one test thread, eight suggestion tests and four non-browser form-preview tests passed (four browser tests ignored); web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Form mapping explanations — 2026-10-29

- The form preview explains which fields had conflicting, invalid or duplicate model suggestions and flags unidentifiable entries without exposing profile values. Field-specific warnings clear when all fields sharing the label receive reviewed mappings.
- With one build job and one test thread, eight suggestion tests and four non-browser form-preview tests passed (four browser tests ignored); web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Bounded mapping for longer forms — 2026-10-29

- Forms up to 200 fields can be mapped in sequential batches of at most 40 eligible fields with a 24,000-byte prompt limit. Batch results are validated against their own fields; failed batches are reported and retain exact-hint fallback. Larger forms remain available for manual review. Option interpretation is separately bounded.
- With one build job and one test thread, nine suggestion tests, four non-browser preview tests and the added failed-batch fixture passed; four browser tests were ignored. Web-app Clippy passed before the final test-only addition. No live model, employer site or resource peaks were measured. No application was sent.

## Bounded option interpretation — 2026-10-29

- Non-exact dropdown and radio options are interpreted in up to five sequential batches of ten fields, capped at 12,000 bytes of field data per batch. Unresolved choices are reported for manual review; duplicate selections for one field are discarded. Exact matches remain available beyond the batch limit.
- With one build job and one test thread, twelve suggestion tests and web-app Clippy passed. No live model, employer site or peak resources were measured. No application was sent.

## Long-form reviewed fill — 2026-10-29

- Reviewed fill accepts up to 200 distinct fields, matching the mapping limit. Its request body is separately capped at 1 MiB; other normal request limits remain unchanged. Larger answers can still exceed this byte limit and need manual handling. Oversized-form guidance recognizes the current mapping-limit message.
- With one build job and one test thread, ten reviewed-path tests passed (four ignored), including a 200/201-field boundary test; the ignored local-browser prefill fixture was run separately and passed without submitting. Web-app Clippy and JavaScript syntax checks passed. No live model, employer site or resource peaks were measured.

## Application link inspection failures — 2026-10-29

- Initial inspection and subsequent navigation now record failures to read likely or other page links as manual-review reasons instead of treating errors as empty results. The current job and saved inspection evidence carry the review state; navigation and submission behavior are unchanged.
- With one build job and one test thread, five link-navigation tests and web-app Clippy passed. No live employer site or resource peaks were measured; no application was sent.

## PDF export response recovery — 2026-10-29

- The most recent completed or failed reviewed PDF export can be polled again after a lost response without creating a second document. This replay is in memory, limited to the latest result and does not survive process restart.
- With one build job and one test thread, four letter-export tests passed (one local conversion test ignored); web-app Clippy passed before the final test-lock addition. No live model, employer site, installed-app startup or peak resources were measured. No application was sent.

## Pending PDF export recovery — 2026-10-29

- The local UI can discover an active reviewed PDF export when the start response is lost and recover the result without launching another export. The active task takes precedence over an older tab task. Recovery remains limited to the running process and its most recent completed result.
- With one build job and one test thread, four letter-export tests passed (one conversion test ignored), the recovery UI fixture passed, and web-app Clippy and JavaScript syntax checks passed. No live model, installed-app startup, employer site or resource peaks were measured. No application was sent.

## Job-linked template edit freshness — 2026-10-29

- A job-linked template conversion now checks a digest of its job-fit context and document instructions before saving; changed context stops the save. The fit context uses bounded, selected qualifications, so this does not prove that every possible candidate change is detected. Edits without a job retain their existing source-file check.
- With one build job and one test thread, the context-change fixture and existing template-record test passed; four conversion fixtures were ignored. Web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Template edit correction tracking — 2026-10-29

- Job-linked template conversion freshness now includes the workspace correction revision and the full stored job record and description, in addition to its bounded fit context and document instructions. A correction outside the selected fit facts changes the context. Adding a new fact does not itself advance the correction revision; the fit selection remains bounded.
- With one build job and one test thread, three template-edit tests passed (four conversion fixtures ignored), and web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Queued template suggestion freshness — 2026-10-29

- AI-suggested replacements carry their source, job and supporting-context digest into the edit queue. Before conversion, the server rejects a queued suggestion if that context or the selected job changed. Manual replacements remain available without a model binding. The digest tracks the workspace correction revision; newly added facts remain subject to bounded fit selection.
- With one build job and one test thread, the queued-context fixture and web-app Clippy passed, and JavaScript syntax was checked. No live model, rendered UI, employer site or resource peaks were measured. No application was sent.

## Template suggestion queue interaction — 2026-10-29

- A synthetic Playwright/Edge fixture exercised the actual document editor JavaScript at 1200 px and 375 px: changing the selected job after applying a suggestion was rejected, and restoring the original job produced a queued request carrying the suggestion digest and job identity. No page errors occurred. The fixture used mocked API responses and no application stylesheet, so it is not a visual-design or live-model qualification. Browser plugin not available; Playwright was the fallback.
- Screenshots and the fixture script are in the ignored `target/visual-checks/` directory. No employer site was contacted and no application was sent. Peak resource use was not measured.

## Template edit recovery — 2026-10-29

- An active template edit can be discovered after a lost start response, and its latest terminal result can be replayed without saving another DOCX/PDF pair. Recovery is in-memory and limited to the current process and most recent terminal result.
- With one build job and one test thread, four template-edit tests passed (four conversion fixtures ignored), the recovery UI fixture passed, and web-app Clippy and JavaScript syntax checks passed. No live model, employer site, installed-app startup or resource peaks were measured. No application was sent.

## Application page check recovery — 2026-10-29

- The interface discovers a running application-page check after a lost start response and resumes the correct job's check. A previously saved tab task remains usable when no active task exists. Checking still reports uncertain pages for manual review and never submits.
- With one build job and one test thread, three application-check tests passed (one browser fixture ignored), the recovery UI fixture passed, and web-app Clippy and JavaScript syntax checks passed. No live employer page or peak resources were measured. No application was sent.

## Application-page control evidence — 2026-10-29

- Application-page checks report detected file-upload, email and standard name-autocomplete controls alongside the field count. These signals can occur on unrelated forms and do not confirm an application or authorize any action; the result remains manual review.
- With one build job and one test thread, three application-check unit tests passed and the fictional local-browser link/form fixture passed without submitting. Web-app Clippy passed before the final fixture-only addition. No live employer page, model or peak resources were measured.

## Application check freshness — 2026-10-29

- Each application-page check and its replay retain the original listing URL. The status endpoint reports a changed URL as stale and the interface rechecks the displayed URL when jobs refresh. The result remains advisory and does not establish that an application is complete or open.
- With one build job and one test thread, four application-check tests passed (one browser fixture ignored), including a local endpoint/replay fixture for a changed URL. The recovery UI fixture, JavaScript syntax check and web-app Clippy passed. No live employer site or peak resources were measured. No application was sent.

## Form inference residency checks — 2026-10-29

- Form inspection and navigation recheck the selected model's reported residency before each model request, including sequential mapping and option batches. If the check fails or the selected resident model changes, that model call is not made and existing fallback/review behavior applies. This is a snapshot and cannot prevent external runtime changes between check and generation.
- With one build job and one test thread, the residency-change fixture, twelve suggestion tests and web-app Clippy passed. No live model or peak memory measurement was made, so this is not a no-freeze guarantee. No application was sent.

## Local inference residency across workflows — 2026-10-29

- The per-request resident-model snapshot now also covers local drafting, template suggestions, document excerpts, fit and duplicate analysis, link ranking, search planning and model-assisted listing extraction. Each guarded request stops if the selected model is no longer the eligible resident model. Visual-review workers already perform their own checks.
- With one build job and one test thread, the residency-change fixture and web-app Clippy passed. No live model or peak memory was measured; the runtime can still change between the snapshot and generation, so this is not a no-freeze guarantee. No application was sent.

## Bounded answer revision repair — 2026-10-29

- Application-answer and template-passage drafting retain the required style revision and allow one repair when its structured output or qualification citations are invalid. Runtime and residency failures are not retried. Citation validity does not prove factual support; user review remains necessary.
- With one build job and one test thread, three answer tests and one template-suggestion test passed. Web-app Clippy passed before the final test-only addition. No live model, employer site or peak resources were measured. No application was sent.

## Fit assessment repair — 2026-10-29

- A malformed fit assessment gets one bounded local repair attempt. Qualification indices and exact listing excerpts are still checked after repair; runtime failures are not retried. The resulting score remains subjective and citations do not establish correctness.
- With one build job and one test thread, six fit-assessment tests and web-app Clippy passed. No live model, employer site or peak resources were measured. No application was sent.

## Partial listing extraction — 2026-10-29

- Local-model listing extraction preserves a verified title and employer when optional location or description cannot be substantiated. Unsupported or missing optional fields stay empty, and the job is flagged for review. Exact excerpts still do not prove the model selected the correct posting.
- With one build job and one test thread, eight candidate tests passed (three browser fixtures initially ignored), and the fictional browser text-extraction fixture passed separately using a fake model. Web-app Clippy passed before the final test-only addition; JavaScript syntax passed. No live model, employer site or peak resources were measured. No application was sent.

## CV and cover-letter draft recovery — 2026-10-29

- An active draft can be discovered after a lost start response; the latest completed result can be replayed for ten minutes in the running process, with context rechecked on status reads. Expiration is applied on the next poll or a new draft, not by a background timer. Draft content is not persisted by this recovery cache.
- With one build job and one test thread, two draft tests, the recovery UI fixture and JavaScript syntax checks passed. Web-app Clippy passed before the final cache-clear and test additions. No live model, installed-app startup, employer site or peak resources were measured. No application was sent.

## Job-relevant drafting qualifications — 2026-10-29

- Drafting selects up to 18 reviewed qualifications for a CV or 20 for a cover letter from the first 500 eligible facts using word overlap with the listing. This can miss relevant facts without shared terms or beyond the limit; the interface states the limitation. The draft context digest still uses the full active fact read and is not a bounded-memory guarantee.
- With one build job and one test thread, the relevant-fact selection fixture, two draft tests, web-app Clippy and JavaScript syntax passed. A synthetic Playwright/Edge fixture with the application stylesheet displayed the notice at 1200 px and 375 px without page errors; Browser plugin not available. No live model, employer site or peak resources were measured. No application was sent.

## Incremental draft freshness digest — 2026-10-29

- Draft freshness now hashes active claims one row at a time through the shared sealed-text read path, rather than building a full active-fact vector. The digest includes reviewed and unreviewed claims and changes after additions or corrections. SQLite may still perform internal sorting, and other data such as the document and job description is read separately; peak memory has not been measured.
- With one build job and one test thread, 46 store tests, the encrypted-workspace fact/digest fixture, draft-context and draft tests, and web-app Clippy passed. No live model, employer site or resource peaks were measured. No application was sent.

## Active qualification reads — 2026-10-29

- Active-fact reads filter superseded rows in SQLite before unsealing, and single-claim lookups use an ID predicate, both through the shared sealed-text reader. This removes the previous full-history vector and repeated linear supersession scans from these paths. Active results still occupy memory and SQLite can use temporary storage; performance and peak memory were not measured.
- With one build job and one test thread, all 47 store tests passed before the final targeted-query adjustment; afterward six store work tests, the encrypted-workspace fixture and web-app Clippy passed. No live employer site or model was tested. No application was sent.

## Ranking normalization reuse — 2026-10-29

- A ranking run normalizes reviewed qualification strings once and reuses them for each listing. Existing token, phrase, duplicate and denominator behavior is preserved; job descriptions are still read one at a time. Runtime and peak memory were not measured, and the overall job result set is still accumulated.
- With one build job and one test thread, ten search tests, 40 workspace tests (two network tests ignored), and web-app Clippy passed. No live model or employer site was tested and no real application was sent.

## Dotted employer acronym duplicates — 2026-10-29

- Duplicate candidates now normalize dotted employer initials such as `I.B.M.` to `IBM`. When comparing a single compact employer token, the index also checks up to 200 older records marked as missing employer tokens. Matches remain pending review; they are not merged. Existing pairs are not retroactively rescanned by this change, and the bounded legacy scan can miss candidates.
- With one build job and one test thread, nine duplicate-detection tests and web-app Clippy passed. No live boards or peak resources were measured. No application was sent.

## Related employer requisition review — 2026-10-29

- Duplicate indexing now flags a matching requisition under moderately related employer names for user review, even if title overlap is low. The explanation includes employer, title and description overlap. Identifiers can be reused by unrelated organizations, so this is not an automatic merge; existing pairs are not retroactively rescanned and candidate scans remain bounded.
- With one build job and one test thread, ten duplicate tests and web-app Clippy passed. No live boards or peak resources were measured. No application was sent.

The table below records a historical checkpoint, not current release evidence.

Status: G0-G5 complete; G6 release-readiness executed (audit/SBOM/packaging checks); remaining release blockers recorded honestly.

| Field | Value |
|---|---|
| Latest commit | see `git log -1` |
| Work layer | facts → packet → one-use approval → DOCX export, with correction propagation; verified live end to end (152 real postings → shortlist → approve → export → correct → approval voided) |
| Submission stage | engine behind a `BrowserDriver` trait (preflight at the boundary, durable consumption, typed fills, qualified receipts, append-only journal, no auto-retry) **plus a real CDP driver** (`waypoint_browser`) — verified end to end against a real browser on local fixtures, including a change-detected form being refused and the approval voided. Not yet exercised on a live third-party site |
| Completed tasks | T001-T020, T021-T026, T027-T034, T035-T043 (all gates' engineering tasks); T044 (binary builds, unsigned), T045 (migration rollback tested; update channel pending), T046 (audit+deny+SBOM executed), T047 (defaults verified in code), T048 (skeleton + measured model baseline), T050 (limitations register) |
| Partial | T006 (keyboard done; Narrator/IME/DPI unobserved), T007 (trait+live probe; production adapter unwired), T049 (pilot not run by design) |
| Validation | `cargo fmt`; `cargo clippy --all-targets` -> 0 warnings; `cargo test --workspace` -> 183 pass (+5 real-browser tests, run in CI); `cargo audit --deny warnings` -> clean (yanked `yoke-derive` replaced by 0.8.4); live network + live model probes pass; `cargo deny check` -> licenses/bans/sources ok; release binary PE32+ 14.3 MB |
| Known blockers | signing cert + installer; SQLCipher; production inference adapter; browser CDP wiring; a11y observation session |
| Next smallest action | Wire `OllamaInference` into the drafting/interview flows, then SQLCipher integration |
