# Development pitfalls

Consult the entries relevant to the code or workflow under review. The linked checkpoints retain the original observations, test scope and limitations.

## Implementation evidence

- Mixed materials can outlive the browser worker even when native calls and
  inference succeed. Keep field, session and outer containment budgets coherent,
  preserve the original timeout and retain per-entry checkpoints independently
  of the final replay report. Those checkpoints are historical observations,
  never present/final-review or delivery proof. Field passes do not qualify
  navigation or whole workflows; profile observed latency before inferring a
  model/hardware cause. See `docs/g1/MIXED_MATERIALS_REHEARSAL_2026-11-14.md`.

- A top-level open does not independently prove that a cross-origin fixture's
  child form has parsed/rendered. Require an origin/source-bound owned fixture
  checkpoint and fresh native inspection, retaining the first observations and
  zero-field exposure checks. A fixture signal never clears a product barrier.
  Preserve failed CI source/results; do not replace a required specific native
  classification with a generic wall just to make a test pass. See
  `docs/g1/SAME_SITE_FRAME_FIXTURE_READINESS_2026-11-14.md`.

- A block file label can span the viewport while its rendered text and chooser
  fit in a small region. File-only detail capture uses bounded native text-range
  geometry, preserving existing text/select and original upload-feedback images.
  Keep native label association, clipping/visibility/generated-content refusals,
  before/after geometry and independent visual observations. A clearer filename
  image does not prove document truth, widget acceptance or server validation.
  See `docs/g1/NATIVE_FILE_LABEL_DETAILS_2026-11-14.md`.

- Runtime public research exposes only currently admitted tool actions in its
  native schema; stated budgets alone did not stop a local model from repeatedly
  selecting an exhausted search. Runtime checks still apply independently.
  Reuse the shared provider's actual `results` contract, test that adapter with
  native-shaped payloads, and retain failed requests for subsequent decisions.
  Anonymous research advice cannot clear a failed field or authorize a retry.
  Diagnose incomplete reasoning from native completion metadata, not empty final
  text alone; preserve the original failed run.

- An inline frame's DOM ranges use child viewport coordinates. Parent-page
  capture needs verified offsets through each owner, plus native hit testing.
  A crop with the right width can still show the page title instead of the
  field. Check the actual pixels and keep geometry checks on both sides of
  capture. The [scrollable application review](g1/SCROLLABLE_APPLICATION_REVIEW_2026-11-18.md)
  records the two paired same-origin observations and the remaining navigation gap.

- A short control crop can truncate a long question even when the control is
  fully visible. Preserve the original image and use current native-associated
  labels/headings for a separate bounded detail; do not supply expected text to
  the visual model. Section-separator spacing is structural label formatting,
  not an answer difference. Preserve exact values, section order, duplicate and
  conflict vetoes, and require independent agreement on native-bound detail.
  A fieldset's default minimum width can prevent a purported clipping fixture
  from clipping; prove its actual native geometry. CSS bounds and raster pixels
  differ when detail capture uses the existing 2× scale. See
  `docs/g1/CONDITIONAL_FORM_CONTINUATION_2026-11-14.md`.

- Inline-frame entry requires completed document inspections and current native
  source/origin/rendering evidence; identity alone cannot clear a frame barrier.
  Inherited `srcdoc`/`about:blank` frames may report `://` in the protocol origin.
  Preserve that observation and require positive effective-origin/parent-source
  evidence; never invent an HTTP success or infer an unknown network response.
  See `docs/g1/INLINE_FRAME_COMPLETENESS_2026-11-13.md`.
- Ordinary DOM reparenting can reload an iframe. An equal-schema stale-scope
  fixture must feature-detect a state-preserving move and prove the unchanged
  schema/review text on the actual engine. Do not weaken guards to make an
  incorrectly authored mutation satisfy its premise.

- Native frame identity uses backend owner/document IDs and loader identities,
  not transient frontend node handles. A parent session's `Page.getFrameTree`
  can omit an independently targeted child. Unavailable census evidence is not
  proof of a changed document or complete inspection; preserve existing review
  barriers and the separately leased target path. Never discharge a frame
  barrier from identity evidence alone. See `docs/g1/NATIVE_FRAME_IDENTITY_2026-11-13.md`.
- Build helpers reject undeclared arguments before dependency setup. Use the
  declared `-Workspace -Test -TestFilter` options for filtered workspace tests;
  verify actual native test counts, not just exit zero. PowerShell previously
  accepted an unknown flag and ran an ordinary build instead of the intended
  test. The routing regression is in `apps/web_app/scripts/test-build-paths.ps1`.

- A missing visual target differs from an ambiguous or contradicted target.
  Blind coverage rechecks must preserve the first observations, admit no expected
  label/value hints, and retain all native/image bindings. Compare singleton
  observations before retrying: repeated labels must not become a successful
  second guess. Conflicting overlapping observations veto entry. Keep the original
  image/fast path; store any native detail view separately. Preserve the original
  Country omission when a supplementary native-bound value read succeeds. A
  developer's reading alone never supplies a local-model observation. Apply exact
  value checks; only native telephone proof admits separator equivalence, never
  an inferred country prefix or normalization of another field.
- For literal readback, verify the native entered value and hidden placeholder
  around the bound detail capture. Keep the visible field label and ordered
  section headings separate in the model response. Retain the raw observations
  and compare the visible characters exactly. See
  [literal field readback](g1/LITERAL_FIELD_READBACK_2026-11-18.md).
- The form inspector includes section names in display labels. For final
  summary checks, derive the field label from the recorded section path and
  keep the path separate. Compare current native and blind visual observations
  with the prepared value. A matched filename does not establish file bytes.
  See [summary verification](g1/FINAL_SUMMARY_VERIFICATION_2026-11-18.md).
- Chromium's lower-page capture can hide the scrollbar and move centered
  content. Use consistent scrollbar rendering in the owned offline replay.
  Keep native geometry and scroll-state rechecks. If a heading requires a
  separate image, transcribe it independently and join it through the native
  section path. See [summary detail images](g1/SUMMARY_DETAIL_IMAGES_2026-11-18.md).
- Preserve the original native file control and form owner across review
  sections. Verify the prepared digest and current browser file together.
  Recheck the complete selection set after asynchronous hashing. A
  replacement input with identical file bytes cannot stand in for the
  original. See [terminal attachment readback](g1/TERMINAL_ATTACHMENT_READBACK_2026-11-18.md).
- An iframe's full-box center can be offscreen while its fields are visible.
  Owner availability uses a common ancestor-viewport intersection and native
  hit tests; each actual field still requires complete visual/native checks.
  Clipped or transparent content is not positive evidence of an absent frame.
  Preserve review even when its inspected field count is zero, and do not invent
  a control to claim action-guard coverage. See
  `docs/g1/FRAME_VIEWPORT_AVAILABILITY_2026-11-13.md`.
- A focused control's caret can change screenshot pixels without a form change.
  Native geometry/state regressions must not assume byte-identical timed captures.
  Preserve actual image digests for each observation. Debug test relinks can also
  exceed a 3 GiB job despite successful incremental runs; retain allocation failures
  and use sufficient RAM-admitted build capacity rather than diagnosing host RAM
  exhaustion or weakening product resource guards.

- Qualification job limits cover the model, browser and ownership helpers
  together. The fictional semantic-profile run exceeded 8 GiB committed memory;
  an 8 GiB test run stopped on an empty helper response, while the 10 GiB job
  reached planning and native/visual entry checks. Record native peak job commit
  and whole-host headroom; do not diagnose model intelligence from a worker or
  containment failure. Keep structured refusals and native exit/output diagnostics.
  Runtime API VRAM figures and whole-adapter WDDM usage are different scopes;
  neither alone proves model residency or safe host-memory use.

- Keep different document roles distinguishable in fixtures: a résumé and cover
  letter sharing PDF bytes cannot expose a swapped-document error. Freeze each
  role's actual expected digest before inference and version changed annotations.
  Inspect failed target images before blaming the model: short control bounds
  can crop distant labels even when the control itself is complete. Fix the
  bounded visual input and preserve raw observations; never invent the missing
  label or rewrite the model's answer to make a gate pass.

- Preserve commanded values, actual native values and raw visual transcriptions
  separately. A telephone control may reformat an explicit international number;
  separator equality is allowed only through the native telephone proof and an
  explicitly declared comparison mode. Never infer a country prefix, normalize
  other text, discard a visual difference or reuse an image after its native
  value changes. Corpus defaults remain exact; see
  `docs/g1/TELEPHONE_PRESENTATION_2026-11-13.md`.

- Native inspection snapshots include selector and section identities. Adding an
  embedded-document namespace must use `LiveForm::with_control_scope`; mutating
  those identities afterward makes unchanged forms stale. Public driver calls
  inside an embedded view must use the snapshot's scoped selectors; raw local
  selectors belong only in explicitly child-scoped native probes. Only a current native
  snapshot may survive that exact transformation. Preserve all original barriers,
  keep previously stale evidence invalid, and qualify both main-page and embedded
  fixtures after changes to this mechanism.

- Windows process creation metadata can remain inspectable after exit while a
  handle retains the process object. Creation time binds identity; it does not
  establish liveness. Verify owned job membership or a native process wait
  signal, drain the owned job, and inspect listener ownership separately. A
  short TCP probe timing out is not evidence of a remaining listener.
- Application-record directory names contain a 16-hex job prefix followed by
  the task identity. Bind temporary runtime ownership to the task portion.
  Exercise the compiled route as well as direct renderer fixtures when
  changing these contracts.
- When changing `app.js` startup or introducing a script global, run all
  `*-ui.test.js` scripts once. Some tests evaluate bounded source fragments;
  new initialization must stay in the page startup section rather than leak
  into unrelated fragments at the end of the file.
- Keep test listeners on loopback. Exercise wildcard listener-table entries
  with synthetic native records rather than opening an all-interface socket.
- A fully GPU-loaded model can still retain substantial host-memory prompt
  checkpoints. The owned Ollama qualifier uses a 512 MiB saved-prompt cache and
  one checkpoint per slot; verify backend acceptance in its native log. Retain
  incomplete native responses and distinguish runtime allocation failures from
  model mistakes or host RAM exhaustion. Never diagnose an output limit without
  its completion metadata. See `docs/LOCAL_VISION_QUALIFICATION.md`.
- Build contract fixtures must restore their caller's complete process
  environment on success and failure. Remove added variables through the Env
  provider; assigning null through the native environment API left present empty
  variables on the measured host. Do not let deleted fake SDK paths reach later
  builds or modify an externally owned toolchain update to work around them.
- Keep source citations in structured metadata, not newly added numeric markers
  in usable template text. Preserve references actually present in the original.
  Candidate history must not be rewritten to match the hiring employer or role.
  Retain original visual-review observations and unknowns; optional model advice
  is not evidence for new candidate facts. See `docs/TEMPLATE_TAILORING.md`.
- Larger context images can reduce local-model accuracy by introducing unrelated
  controls. Keep unaffected control crops stable, qualify native file-only
  context separately on main/embedded pages, and retain regressions rather than
  claiming that a larger image is inherently better. A page can catch an upload
  error without emitting an unhandled runtime exception; inspect its visible
  feedback and preserve server-dependent review outcomes.
- Native file selection and matching bytes do not prove that the page's widget
  accepted the attachment. Preserve visible filename failures and broker walls;
  do not normalize them into success. Keep requested waits separate from actual
  monotonic observation times, retaining earlier diagnostic reports unchanged.
- After interrupted browser or fixture tests, check for orphaned Waypoint-owned
  browser profiles and fixture servers before concluding that host memory is
  unavailable. Verify each process's identity and that its test parent is gone
  before stopping it; never close another active workload. Browser `Drop` cleanup
  does not run if its parent is forcibly terminated; Windows job ownership now
  covers that case for newly started browsers, but old processes and temp
  profiles may remain. Record measurements rather than assuming all memory
  pressure is due to the app.
- A global or earlier `Page.loadEventFired` is not multi-target readiness.
  Bind the flat session, actual current URL/document and native loader. Retain
  per-session load signals, keep wildcard interception/count assertions, and
  give controlled fixtures an inline icon to avoid unrelated favicon traffic.
  See `docs/g1/MULTI_SESSION_FIXTURE_READINESS_2026-11-14.md`.
- Candidate rehearsals must bind their input revision and never substitute demo
  contacts/documents for missing candidate material. Retain durable originals
  before changing a test profile and verify restoration: normal profile saves
  can normalize defaults/history. Startup logs contain workspace access tokens;
  retain them privately and extract/redact fields before displaying any output.
- Qualify research decisions separately from unchanged browser/vision stages.
  A fixed replay missing a changed query is missing input, not live-provider or
  model-intelligence evidence. Pass actual schemas in the prompt, preserve all
  completed read arguments and current budgets, and return precise validation
  issues. Citation repairs may offer bounded exact-source choices but must keep
  the original refusal and require fresh model output. Provenance-valid advice
  is not full semantic support, a tested repair or action authority. Preserve
  irrelevant-search stops and all earlier failures; see
  `docs/g1/PUBLIC_RESEARCH_DECISIONS_2026-11-14.md`.
- **Ollama `think:true` + strict JSON/schema format silently returns an empty
  `response`** while filling `thinking` (done_reason=length): structured generation
  in `ollama.rs` therefore pins `think:false`. Reasoning probes check the separate
  `thinking` field and verify a multi-step answer instead of requiring JSON.
- Tiny `num_predict` under `think:true` yields an empty final answer → fake
  "reasoning unavailable". Probe budgets are 768+ tokens.
- Greenhouse/Ashby wrap payloads in `{"jobs": [...]}`; Lever uses a bare list; one
  board (netflix) 404s — live tests use vercel/ashby/spotify.
- Zero/negative timestamps mean "unrecorded observation" (Unknown), never Fresh.
- MSYS/Git-Bash path quirks: pass `C:/...` forward-slash paths to native tools.
- Adapters expect the providers' real camelCase payloads (Lever `hostedUrl`/`workplaceType`,
  Ashby `jobUrl`/`isRemote`/`publishedAt`). Fixtures written in snake_case silently default
  to empty strings and get dropped as untrusted URLs — mirror real payloads.
- `Cargo.lock` moves with the graph: adding a dep re-resolved `url`→`idna`→ICU and pulled a
  yanked `yoke-derive 0.8.3`. Always re-run `cargo audit --deny warnings` after dependency
  changes; fix yanks by moving versions (`cargo update -p <crate> --precise X`), never by
  silencing the check.
- Application references must keep their original casing: `extract_application_ref`
  matches on an ASCII-lowercased copy but slices the *original* text. Slicing the
  lowercased text silently downcases identifiers the candidate has to quote back.
- The upload directory must contain the file name the fill plan asks for
  (`waypoint-packet.docx`); the driver resolves names against configured folders and
  reports a missing file rather than submitting without it.
- Candidate material is sealed, public data is not. Everything the candidate
  writes (claim text, packet body/doc_json) goes through `Store::seal` when a
  sealer is attached and its plaintext column is written empty; postings and
  journals stay plaintext so search and reporting keep working with no key.
  There is exactly **one** read path per table (`all_claims`,
  `row_to_packet_full`) so a sealed column cannot be missed through a side door —
  a new query that reads `text`, `body` or `doc_json` directly reintroduces the
  bug this design prevents (it happened once while building it).
- After sealing existing rows the store VACUUMs: SQLite keeps freed page contents
  until it rewrites the file, and "the plaintext is gone" must be true of the file
  on disk, not just of a query result.
- `unsafe_code` is `deny` workspace-wide rather than `forbid`, so exactly one
  module can carry a reviewed `#[allow(unsafe_code)]`. Any unsafe code outside
  `waypoint_seal`'s DPAPI module fails the build, which is the intent.
- `validate_claim_support` reports *positive* support (`Supported`) as well as problems, and
  a citation that no longer resolves is reported as unsupported. Do not "optimise" it back
  to problems-only: the approval gate counts supported spans, and a dangling citation must
  never read as supported.
- Entity-escaping text through write/patch tooling corrupts `&`-style strings;
  write such code via line-index scripts, not string-replace through the tool.

## Earlier planning checkpoints

For continuation of the active product-development goal, read
`docs/REPO_STATE_REVIEW_2026-11-11.md`, the detailed
`docs/COMPLETION_PLAN_2026-11-11.md` and `docs/ACTIVE_DEVELOPMENT_PLAN.json`.
The [2026-11-14 direction review](RELEASE_DIRECTION_REVIEW_2026-11-14.md)
adds current competitor research and prioritizes the actual reasoning/tool actor
and complete candidate journeys; it preserves every unfinished acceptance gate.
The [readiness assessment and next sequence](RELEASE_READINESS_AND_NEXT_PLAN_2026-11-14.md)
prioritizes the observed numeric/short-answer proof gap, then complete mixed
candidate journeys. Preserve the unchanged failed field-case denominators; do
not replace integrated work with more isolated geometry passes or relax numeric
units, source checks or unknown abstention to obtain a passing score.
Use that queue's prioritized `next_task` once its `start_requirements` are ready;
`depends_on` records completion prerequisites, not a ban on independent preparation.
Containment, ownership and interface obligations apply throughout. Keep all
unfinished criteria visible. Do not repeatedly
reconstruct a broad roadmap or treat the original T001-T050 `PLANNED` statuses
as a live progress ledger. The original handoff's contracts and all current
user/agent safety instructions remain binding. Update queue evidence/status
after meaningful checkpoints, distinguishing measured results from targets.
Keep work in the existing active goal; do not create another goal, branch,
release or agent delegation from the queue alone. A completed package does not
complete the overall goal. Historical plan/limitation text must not override
current-source evidence or silently remove unresolved acceptance work.

The 2026-09-26 gate results are historical, not evidence of the current checkout.
Read `IMPLEMENTATION_STATUS.md`, `docs/WEB_PROTOTYPE_CHECKPOINT.md` and current
source before selecting work. Preserve explicit unknowns and report measured
results separately from targets. Never submit a real application without explicit
authorization for that application; when instructed to stop before submission,
all live-site work must respect that boundary.
The product is a prototype; realistic employer-site behavior, accessibility,
installation and local-model quality need further verification.
