//! Offline review of a job's archived public page.
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use serde_json::json;
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) directory: PathBuf,
    pub(crate) attempt_id: String,
    pub(crate) job_id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) rehearse: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) public_research: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) candidate_snapshot_sha256: Option<String>,
}

pub(crate) fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

pub(crate) fn directory(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Saved page directory is unavailable")?;
    if !meta.is_dir() || linked(&meta) {
        return Err("Saved page directory is invalid".into());
    }
    Ok(())
}

pub(crate) fn read(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Saved page evidence is unavailable")?;
    if !meta.is_file() || linked(&meta) || meta.len() > max as u64 {
        return Err("Saved page evidence is invalid".into());
    }
    let mut data = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(max as u64 + 1).read_to_end(&mut data))
        .map_err(|_| "Cannot read saved page evidence")?;
    if data.len() > max {
        return Err("Saved page evidence exceeds its limit".into());
    }
    Ok(data)
}

impl Source {
    pub(crate) fn validate(&self, url: &str) -> Result<(), String> {
        if let Some(digest) = &self.candidate_snapshot_sha256 {
            if !self.rehearse
                || digest.len() != 64
                || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Candidate contact details require a bound offline rehearsal".into());
            }
        }
        if self.public_research && !self.rehearse {
            return Err("Public research requires an autofill rehearsal".into());
        }
        directory(
            self.directory
                .parent()
                .ok_or("Saved page parent is unavailable")?,
        )?;
        directory(&self.directory)?;
        let summary: Value = serde_json::from_slice(&read(
            &self.directory.join("source-page-summary.json"),
            crate::job_capture::SOURCE_SUMMARY_BYTES,
        )?)
        .map_err(|_| "Invalid saved page record")?;
        let digest = waypoint_domain::sha256_hex(url.as_bytes());
        if summary["status"] != "complete"
            || summary["result"]["archive_written"] != true
            || summary["attempt_id"] != self.attempt_id
            || summary["task_id"] != self.attempt_id
            || summary["job_id"] != self.job_id
            || summary["source_url_sha256"] != digest
        {
            return Err("Save a page copy for the current job link first".into());
        }
        Ok(())
    }
}

#[cfg(windows)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    schema: u32,
    task_id: String,
    files: std::collections::BTreeMap<String, String>,
}

#[cfg(windows)]
pub(crate) fn render(
    source: &Source,
    url: &str,
    python: &Path,
    config: waypoint_browser::rehearsal::RehearsalConfig,
    screenshots: &Path,
    selection: Option<&crate::private_inference::Selection>,
) -> Result<Value, String> {
    render_at(
        source,
        url,
        python,
        config,
        screenshots,
        std::net::SocketAddr::from(([127, 0, 0, 1], 11434)),
        selection,
    )
}

#[cfg(windows)]
fn render_at(
    source: &Source,
    url: &str,
    python: &Path,
    mut config: waypoint_browser::rehearsal::RehearsalConfig,
    screenshots: &Path,
    vision_addr: std::net::SocketAddr,
    selection: Option<&crate::private_inference::Selection>,
) -> Result<Value, String> {
    use waypoint_applications::{driver::BrowserDriver, form_mapping::BrowserCommand};
    use waypoint_browser::CdpDriver;
    source.validate(url)?;
    let capture = source.directory.join("source-page");
    let archive = capture.join("archive");
    directory(&capture)?;
    directory(&archive)?;
    directory(screenshots)?;
    let inventory: Inventory =
        serde_json::from_slice(&read(&capture.join("inventory.json"), 128 * 1024)?)
            .map_err(|_| "Invalid saved page inventory")?;
    if inventory.schema != 1
        || inventory.task_id != source.attempt_id
        || inventory.files.len() > 516
    {
        return Err("Saved page inventory does not match its record".into());
    }
    let manifest_bytes = read(&archive.join("replay.json"), 1024 * 1024)?;
    let manifest_sha256 = waypoint_domain::sha256_hex(&manifest_bytes);
    if inventory.files.get("replay.json") != Some(&manifest_sha256) {
        return Err("Saved page manifest changed".into());
    }
    let manifest: Value =
        serde_json::from_slice(&manifest_bytes).map_err(|_| "Invalid saved page manifest")?;
    let entry = manifest["entry_url"]
        .as_str()
        .ok_or("Saved page has no entry URL")?;
    let expected = url::Url::parse(url).map_err(|_| "Invalid original page URL")?;
    let page = url::Url::parse(entry).map_err(|_| "Invalid saved page URL")?;
    if page.origin() != expected.origin()
        || waypoint_browser::capture_privacy::checked_reference(entry).is_err()
    {
        return Err("Saved page entry needs review".into());
    }
    config.schema = 2;
    config.content_root = archive;
    config.content_files = inventory.files;
    config.replay_manifest = Some("replay.json".into());
    let candidate = source
        .candidate_snapshot_sha256
        .as_deref()
        .map(|binding| {
            crate::rehearsal_candidate::load(screenshots, binding)
                .map(|snapshot| (snapshot, binding.to_owned()))
        })
        .transpose()?;
    config.attachments = if source.rehearse && candidate.is_none() {
        Some(stage_sample_attachments(screenshots)?)
    } else if let Some((snapshot, _)) = &candidate {
        snapshot.attachments(screenshots, &source.job_id)?
    } else {
        None
    };
    let model_session = if source.rehearse {
        selection
            .map(|selected| crate::private_inference::Session::admit(selected, python, screenshots))
            .transpose()
    } else {
        Ok(None)
    };
    let (mut model_session, model_refusal) = match model_session {
        Ok(session) => (session, None),
        Err(crate::private_inference::AdmissionError::Unavailable(reason)) => {
            (None, Some(bounded_text(&reason)))
        }
        Err(crate::private_inference::AdmissionError::CleanupFailed(reason)) => return Err(reason),
    };
    let vision_addr = model_session
        .as_ref()
        .map(|session| session.addr)
        .unwrap_or(vision_addr);
    let runtime = model_session
        .as_ref()
        .and_then(|session| session.runtime.clone());
    let session_duration = std::time::Duration::from_secs(config.seconds);
    let deadline = std::time::Instant::now()
        .checked_add(session_duration)
        .ok_or("Invalid rehearsal duration")?;
    let (mut driver, ready) =
        CdpDriver::launch_rehearsal(python, config).map_err(|error| error.to_string())?;
    let mut rehearsal = source.rehearse.then(|| {
        Rehearsal::new(
            vision_addr,
            selection.and_then(|value| value.choice.as_ref()),
            runtime,
            model_refusal.is_none(),
            source.public_research,
            candidate,
            (deadline, session_duration),
        )
    });
    let mut evidence = ScreenshotArchive::new(screenshots);
    let result: Result<Value, String> = (|| {
        if ready.entry_url != entry
            || ready
                .replay
                .as_ref()
                .map(|proof| proof.manifest_sha256.as_str())
                != Some(manifest_sha256.as_str())
        {
            return Err("Saved page entry changed during setup".into());
        }
        let parent = driver.open(entry).map_err(|error| error.to_string())?;
        let mut views = Vec::new();
        record_scope(
            &mut driver,
            &mut evidence,
            &mut views,
            "Page",
            parent,
            false,
            rehearsal.as_mut(),
        )?;
        let mut uninspected = 0;
        let truncated;
        let mut notices = Vec::new();
        match driver.captured_frames() {
            Ok(frames) => {
                uninspected = frames.uninspected;
                truncated = frames.truncated;
                for frame in frames.frames {
                    let view = match driver.enter_captured_frame(&frame) {
                        Ok(form) => {
                            let captured = record_scope(
                                &mut driver,
                                &mut evidence,
                                &mut views,
                                frame.label(),
                                form,
                                true,
                                rehearsal.as_mut(),
                            );
                            driver
                                .leave_captured_frame()
                                .map_err(|error| error.to_string())?;
                            captured
                        }
                        Err(error) => Err(error.to_string()),
                    };
                    match view {
                        Ok(()) => {}
                        Err(error) => {
                            uninspected += 1;
                            notices.push(bounded_text(&error));
                        }
                    }
                }
            }
            Err(error) => {
                truncated = true;
                notices.push(bounded_text(&error.to_string()));
            }
        }
        let report = driver
            .rehearsal_replay_report()
            .map_err(|error| error.to_string())?;
        Ok(
            json!({"source_attempt":source.attempt_id,"manifest_sha256":manifest_sha256,
            "views":views,"uninspected_frames":uninspected,"truncated":truncated,"notices":notices,
            "replay":{"served":report["served"],"blocked":report["blocked"],"blocked_reasons":report["blocked_reasons"],
                "refused_request_samples":report["refused_request_samples"],"omitted_refusal_samples":report["omitted_refusal_samples"],
                "cached_non_read_replies":report["cached_non_read_replies"],"unreplayed_transports":report["unreplayed_transports"],
                "unmanaged_child_sessions":report["unmanaged_child_sessions"],"unsupported_targets":report["unsupported_targets"],
                "server_validation":report["server_validation"],"external_delivery":report["external_delivery"]}}),
        )
    })();
    let close = driver
        .execute(&BrowserCommand::Close)
        .map_err(|error| error.to_string());
    let mut result = result;
    if let (Ok(result), Some(reason)) = (&mut result, model_refusal) {
        result["model_admission"] = json!({"mode":"unavailable","reason":reason});
    }
    if let Some(rehearsal) = rehearsal {
        let record = rehearsal.finish(evidence.bytes);
        if let Ok(result) = &mut result {
            result["rehearsal"] = record;
        }
    }
    let mut cleanup = Vec::new();
    if let Err(error) = close {
        cleanup.push(error);
    }
    if let Some(session) = model_session.as_mut() {
        let stopped = session.stop();
        if let Ok(result) = &mut result {
            result["model_admission"] = session.metadata.clone();
        }
        match stopped {
            Err(error) => cleanup.push(error),
            Ok(()) => {
                if let Ok(result) = &mut result {
                    result["model_admission"]["session_closed"] = json!(true);
                }
            }
        }
    }
    if !cleanup.is_empty() {
        let cleanup = cleanup.join("; ");
        return Err(match result {
            Ok(_) => cleanup,
            Err(error) => format!("{error}; cleanup: {cleanup}"),
        });
    }
    result
}

#[cfg(windows)]
const REVIEW_MEMORY_WAIT: std::time::Duration = std::time::Duration::from_secs(45);
#[cfg(all(windows, test))]
thread_local! {
    static TEST_FIELD_MEMORY_REFUSALS: std::cell::RefCell<std::collections::BTreeSet<usize>> =
        const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
}
/// Shared by launch and accounting. Larger local reasoning models need enough
/// time to complete mixed materials; RAM, cancellation and action guards remain.
#[cfg(windows)]
pub(crate) const REHEARSAL_SECONDS: u64 = 2700;
const VIEW_FIELD_LIMIT: usize = 40;

#[cfg(windows)]
fn record_scope(
    driver: &mut waypoint_browser::CdpDriver,
    evidence: &mut ScreenshotArchive<'_>,
    views: &mut Vec<Value>,
    label: &str,
    mut form: waypoint_applications::driver::LiveForm,
    embedded: bool,
    mut rehearsal: Option<&mut Rehearsal>,
) -> Result<(), String> {
    use waypoint_browser::{RecordedSectionOutcome, RecordedSectionStop};
    let first_view = views.len() + 1;
    let first_entry = rehearsal.as_ref().map_or(0, |run| run.fields.len());
    let mut summary_attachments = std::collections::BTreeMap::new();
    let mut prior = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    let mut pending_dialog: Option<usize> = None;
    let mut section = 1;
    let mut section_dialogs = crate::dialog_planning::SectionDialogs::default();
    if let Some(rehearsal) = rehearsal.as_deref_mut() {
        rehearsal.captures.clear();
    }
    loop {
        let view = views.len() + 1;
        let capture = capture_view(driver, evidence, label, form.clone(), embedded);
        let mut capture = match capture {
            Ok(capture) => capture,
            Err(error) => {
                let Some(rehearsal) = rehearsal.as_deref_mut().filter(|_| view > first_view) else {
                    return Err(error);
                };
                let capture_reason = bounded_text(&error);
                if pending_dialog.take().is_some() {
                    if let Some(record) = rehearsal.dialogs.last_mut() {
                        record["followup_capture"] = json!("unavailable");
                        record["capture_reason"] = json!(capture_reason);
                    }
                } else {
                    rehearsal.sections.push(json!({
                        "view":view-1,"status":"stopped","reason":"section_capture_failed",
                        "capture_reason":capture_reason
                    }));
                }
                rehearsal.halted = true;
                return Ok(());
            }
        };
        if let Some(rehearsal) = rehearsal.as_deref_mut() {
            if pending_dialog.take().is_some() {
                capture["after_dialog"] = json!(true);
                if let Some(record) = rehearsal.dialogs.last_mut() {
                    record["followup_capture"] = json!("saved");
                }
            }
            capture["scope_view"] = json!(first_view);
            capture["section"] = json!(section);
            if let Some((from, control)) = pending.take() {
                rehearsal.sections.push(json!({"view":from,"status":"advanced","to":view,"control":bounded_text(&control)}));
            }
        }
        views.push(capture);
        let Some(rehearsal) = rehearsal.as_deref_mut() else {
            return Ok(());
        };
        if rehearsal.halted {
            return Ok(());
        }
        if rehearsal.stop_at_deadline(view) {
            return Ok(());
        }
        if form.independent_field_entry_blocked() || form.schema.fields.is_empty() {
            if let Some(next) =
                rehearsal.resolve_dialog(driver, evidence, view, &form, &mut section_dialogs)
            {
                pending_dialog = Some(view);
                form = next;
                continue;
            }
            if form.schema.fields.is_empty() {
                return Ok(());
            }
            rehearsal.halted = true;
            rehearsal.sections.push(json!({"view":view,"status":"stopped","reason":"page_needs_review","barriers":form.independent_field_barriers()}));
            return Ok(());
        }
        let continuation = rehearsal.exercise(driver, evidence, view, &form);
        if rehearsal.halted {
            return Ok(());
        }
        if let Some(next) = continuation {
            rehearsal.conditionals += 1;
            rehearsal.sections.push(json!({"view":view,"status":"conditional_reinspection","next_view":views.len()+1,
                "previous_fields":form.schema.fields.len(),"inspected_fields":next.schema.fields.len(),
                "schema_sha256":waypoint_domain::sha256_hex(next.schema.schema_hash_input().as_bytes())}));
            prior.push(form);
            form = next;
            continue;
        }
        if rehearsal.stop_at_deadline(view) {
            return Ok(());
        }
        for (index, capture, _) in &rehearsal.captures {
            let entry = &rehearsal.fields[*index];
            if entry["status"] == "visually_matched" {
                if let Some(sha256) = entry["attachment"]["sha256"].as_str() {
                    summary_attachments
                        .entry(*index - first_entry)
                        .or_insert_with(|| {
                            driver
                                .capture_recorded_attachment_witness(capture, sha256)
                                .map_err(|error| bounded_text(&error.to_string()))
                        });
                }
            }
        }
        let verified: Vec<_> = rehearsal
            .captures
            .iter()
            .map(|(_, capture, _)| capture)
            .collect();
        let vision = &mut rehearsal.vision;
        let advance =
            driver.advance_recorded_section_reviewed(&form, &prior, &verified, true, |target| {
                vision.review_target(evidence, view, "before_section", target)
            });
        match advance {
            Ok(RecordedSectionOutcome::Advanced {
                label: control,
                form: next,
            }) => {
                section += 1;
                pending = Some((view, control));
                prior.push(form);
                form = next;
                rehearsal.captures.clear();
                section_dialogs = crate::dialog_planning::SectionDialogs::default();
            }
            Ok(RecordedSectionOutcome::ReviewRequired {
                reason,
                form,
                image,
                summary,
            }) => {
                let mut transition = json!({"view":view,"status":"stopped","reason":reason});
                match save_review_observation(
                    evidence,
                    label,
                    &form,
                    &image,
                    embedded,
                    (first_view, section, view),
                ) {
                    Ok(mut captured) => {
                        if let Some(summary) = summary {
                            let report = crate::recorded_summary::inspect(
                                driver,
                                &form,
                                &image,
                                summary,
                                crate::recorded_summary::PreparedSummary {
                                    entries: &rehearsal.fields[first_entry..],
                                    attachments: &summary_attachments,
                                },
                                |image| evidence.save(image),
                                |input| rehearsal.vision.read_summary(input),
                            );
                            captured["final_review"] =
                                json!(report["status"] == "answers_verified");
                            captured["final_summary"] = report;
                        }
                        views.push(captured);
                        transition["review_view"] = json!(views.len());
                    }
                    Err(error) => transition["capture_reason"] = json!(bounded_text(&error)),
                }
                rehearsal.sections.push(transition);
                rehearsal.halted = true;
                return Ok(());
            }
            Ok(RecordedSectionOutcome::Stopped(reason)) => {
                if matches!(reason, RecordedSectionStop::FieldChanged) {
                    for (index, _, _) in rehearsal.captures.drain(..) {
                        rehearsal.fields[index]["status"] = json!("changed_after_entry");
                        rehearsal.fields[index]
                            .as_object_mut()
                            .expect("field record")
                            .remove("attachment");
                    }
                    rehearsal.halted = true;
                }
                if matches!(
                    reason,
                    RecordedSectionStop::ReplayGap
                        | RecordedSectionStop::DocumentChanged
                        | RecordedSectionStop::PageNeedsReview
                        | RecordedSectionStop::NoNewSection
                ) {
                    rehearsal.halted = true;
                }
                rehearsal
                    .sections
                    .push(json!({"view":view,"status":"stopped","reason":reason}));
                return Ok(());
            }
            Err(_) => {
                rehearsal.sections.push(
                    json!({"view":view,"status":"stopped","reason":"verification_unavailable"}),
                );
                rehearsal.halted = true;
                return Ok(());
            }
        }
    }
}

#[cfg(windows)]
const SAMPLE_PDF: &[u8] = include_bytes!("rehearsal-sample.pdf");
#[cfg(windows)]
const SAMPLE_COVER_PDF: &[u8] = include_bytes!("rehearsal-cover-letter.pdf");
#[cfg(windows)]
const SAMPLE_RESUME: &str = "sample-resume.pdf";
#[cfg(windows)]
const SAMPLE_COVER: &str = "sample-cover-letter.pdf";

#[cfg(windows)]
fn sample_attachment_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        SAMPLE_RESUME => Some(SAMPLE_PDF),
        SAMPLE_COVER => Some(SAMPLE_COVER_PDF),
        _ => None,
    }
}

#[cfg(windows)]
fn stage_sample_attachments(
    screenshots: &Path,
) -> Result<waypoint_browser::rehearsal::RehearsalAttachments, String> {
    let attempt = screenshots
        .parent()
        .ok_or("Rehearsal record is unavailable")?;
    directory(attempt)?;
    let root = attempt.join("rehearsal-attachments");
    fs::create_dir(&root).map_err(|_| "Cannot create sample attachment folder")?;
    let mut files = std::collections::BTreeMap::new();
    for (name, bytes) in [
        (SAMPLE_RESUME, SAMPLE_PDF),
        (SAMPLE_COVER, SAMPLE_COVER_PDF),
    ] {
        use std::io::Write;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(name))
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|_| "Cannot preserve sample attachment")?;
        files.insert(name.to_owned(), waypoint_domain::sha256_hex(bytes));
    }
    Ok(waypoint_browser::rehearsal::RehearsalAttachments { root, files })
}

#[cfg(windows)]
struct RehearsalVision {
    addr: std::net::SocketAddr,
    memory: crate::InferenceMemoryPolicy,
    model: Option<crate::visual_review::VisionIdentityGuard>,
    identity: Value,
    targets: Vec<Value>,
    deadline: std::time::Instant,
}

#[cfg(windows)]
impl RehearsalVision {
    fn read_summary(
        &mut self,
        input: crate::recorded_summary::SummaryVisual<'_>,
    ) -> Result<Value, String> {
        self.memory.check()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        crate::visual_review::inspect_recorded_summary_image(model, self.addr, input)
    }

    fn research_issue(
        &mut self,
        directory: &Path,
        profile: &Value,
        context: &str,
    ) -> Result<Value, String> {
        self.memory.check()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        crate::visual_review::research_recorded_issue(model, self.addr, directory, profile, context)
    }
    fn plan_history(
        &mut self,
        fields: &[waypoint_applications::form_mapping::FormField],
        records: &[crate::candidate_history::SelectedRecord],
    ) -> Result<Value, String> {
        self.memory.check()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        crate::visual_review::propose_recorded_history(model, self.addr, fields, records)
    }
    fn plan_profile(
        &mut self,
        form: &waypoint_applications::driver::LiveForm,
        profile: &Value,
    ) -> Result<crate::visual_review::RecordedFactPlan, String> {
        self.memory.check()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        Ok(crate::visual_review::propose_recorded_facts(
            model, self.addr, form, profile,
        ))
    }

    fn plan_qualifications(
        &mut self,
        fields: &[waypoint_applications::form_mapping::FormField],
        facts: &crate::rehearsal_facts::Snapshot,
    ) -> Result<Value, String> {
        self.memory.check()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        crate::visual_review::propose_recorded_qualifications(model, self.addr, fields, facts)
    }

    fn plan_attachments(
        &mut self,
        schema: &waypoint_applications::form_mapping::FormSchema,
        documents: &[crate::attachment_mapping::DocumentOption<'_>],
    ) -> Result<crate::attachment_mapping::Plan, String> {
        self.memory.check()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        crate::visual_review::propose_recorded_attachments(model, self.addr, schema, documents)
    }

    fn new(
        addr: std::net::SocketAddr,
        choice: Option<&crate::model_preferences::Choice>,
        runtime: Option<crate::private_inference::RuntimeBinding>,
        allow_inference: bool,
        deadline: std::time::Instant,
    ) -> Self {
        let memory = if runtime.is_some() {
            crate::InferenceMemoryPolicy::Owned
        } else {
            crate::InferenceMemoryPolicy::Shared
        };
        let identity = if allow_inference {
            memory
                .check()
                .ok()
                .and_then(|()| crate::resident_model_identity_at(addr).ok().flatten())
                .filter(|identity| choice.is_none_or(|choice| choice.matches(identity)))
        } else {
            None
        };
        let metadata = identity
            .as_ref()
            .map(|value| json!({"name":value.name,"digest":value.digest}));
        let runner = runtime.as_ref().map(|runtime| runtime.runner);
        let request_guard: crate::visual_review::RequestGuard = std::sync::Arc::new(move || {
            if std::time::Instant::now() >= deadline {
                return Err("The rehearsal time budget was reached".into());
            }
            runtime
                .as_ref()
                .map_or_else(|| memory.check(), |runtime| (runtime.guard)())
        });
        Self {
            addr,
            memory,
            model: identity.map(|identity| {
                crate::visual_review::VisionIdentityGuard::from_identity(identity)
                    .with_rehearsal_runtime(request_guard, runner)
            }),
            identity: metadata.unwrap_or(Value::Null),
            targets: Vec::new(),
            deadline,
        }
    }

    fn review_target(
        &mut self,
        evidence: &mut ScreenshotArchive<'_>,
        view: usize,
        phase: &str,
        target: &waypoint_browser::RecordedTargetCapture,
    ) -> Result<(), waypoint_applications::driver::DriverError> {
        use crate::visual_review::RecordedVisionOutcome;
        let refused = || {
            waypoint_applications::driver::DriverError(
                "Visual inspection needs review before this action".into(),
            )
        };
        if std::time::Instant::now() >= self.deadline {
            return Err(refused());
        }
        let mut record = json!({"view":view,"phase":phase,"label":bounded_text(target.label()),"kind":target.kind(),"status":"visual_unavailable"});
        if let Some(group) = target.file_group_label() {
            record["native_file_group_label"] = json!(group);
        }
        let saved = evidence.save(target.image());
        match &saved {
            Ok((image, digest)) => {
                record["image"] = json!(image);
                record["sha256"] = json!(digest);
            }
            Err(reason) => record["retention_reason"] = json!(bounded_text(reason)),
        }
        if let Some(error) = target.detail_error() {
            record["detail_capture_error"] = json!(bounded_text(error));
        }
        let detail_saved = match target.detail_image() {
            Some(image) => match evidence.save(image) {
                Ok((image, digest)) => {
                    record["detail_image"] = json!(image);
                    record["detail_sha256"] = json!(digest);
                    true
                }
                Err(reason) => {
                    record["detail_capture_error"] = json!(bounded_text(&reason));
                    false
                }
            },
            None => true,
        };
        let (outcome, transcription) = if let (Some(model), Ok(_), Ok(())) = (
            self.model.as_mut(),
            if detail_saved {
                saved
            } else {
                Err("Detail evidence unavailable".into())
            },
            self.memory.check(),
        ) {
            crate::visual_review::review_recorded_target(model, self.addr, target)
        } else {
            (RecordedVisionOutcome::Stopped, None)
        };
        let accepted = matches!(outcome, RecordedVisionOutcome::Matched);
        record["status"] = json!(recorded_vision_status(outcome));
        if let Some(transcription) = transcription {
            record["transcription"] = transcription;
        }
        self.targets.push(record);
        if accepted {
            Ok(())
        } else {
            Err(refused())
        }
    }

    fn review_field(
        &mut self,
        capture: &waypoint_browser::RecordedFieldCapture,
    ) -> Result<(&'static str, Option<Value>), String> {
        self.memory.check()?;
        let model = self
            .model
            .as_mut()
            .ok_or("Local vision model unavailable")?;
        let (outcome, transcription) =
            crate::visual_review::review_recorded_field(model, self.addr, capture)?;
        Ok((recorded_vision_status(outcome), transcription))
    }

    fn review_entered_field(
        &mut self,
        driver: &mut waypoint_browser::CdpDriver,
        capture: &mut waypoint_browser::RecordedFieldCapture,
        evidence: &mut ScreenshotArchive<'_>,
        index: usize,
        replay_before: Option<&Value>,
        result: &mut Value,
    ) {
        let mut recovered = false;
        #[cfg(not(test))]
        let admission = self.memory.sample();
        #[cfg(test)]
        let admission = if TEST_FIELD_MEMORY_REFUSALS
            .with(|entries| entries.borrow_mut().remove(&(index + 1)))
        {
            Err(crate::InferenceMemoryRefusal::Pressure {
                available: 3 * 1024_u64.pow(3),
                required: crate::InferenceMemoryPolicy::OWNED_RESERVE_BYTES,
            })
        } else {
            self.memory.sample()
        };
        if let Err(refusal) = admission {
            result["review_reason"] = json!(bounded_text(&refusal.to_string()));
            // A request that was already issued can still be running. Only
            // a known admission refusal before review permits this recovery.
            if !matches!(refusal, crate::InferenceMemoryRefusal::Pressure { .. })
                || self.model.is_none()
            {
                return;
            }
            // Each entry has its own immutable initial record and recapture.
            // The remaining workflow deadline limits every recovery.
            let initial = json!({
                "scope": "entered_field_before_visual_review",
                "field": result,
                "reason": "memory_pressure",
            });
            let initial_archive = match archive_candidate_answers(
                evidence.directory,
                "recovery",
                index + 1,
                "entries",
                &initial,
            ) {
                Ok(value) => value,
                Err(reason) => {
                    result["visual_recovery"] =
                        json!({"status":"evidence_unavailable","reason":reason});
                    return;
                }
            };
            let started = std::time::Instant::now();
            let wait = REVIEW_MEMORY_WAIT.min(self.deadline.saturating_duration_since(started));
            let outcome = recover_field_observation(driver, capture, replay_before, || {
                wait_for_review_memory(|| self.memory.sample(), wait)
            })
            .and_then(|()| save_field_images(evidence, capture, result));
            result["visual_recovery"] = json!({
                "status": "stopped",
                "initial_archive": initial_archive,
                "elapsed_ms": started.elapsed().as_millis(),
            });
            match outcome {
                Ok(()) => {
                    if let Some(field) = result.as_object_mut() {
                        field.remove("review_reason");
                    }
                    recovered = true;
                }
                Err(reason) => {
                    result["visual_recovery"]["reason"] = json!(bounded_text(&reason));
                    result["review_reason"] = json!(bounded_text(&reason));
                    return;
                }
            }
        }
        match self.review_field(capture) {
            Ok((status, transcription)) => {
                result["status"] = json!(status);
                if let Some(transcription) = transcription {
                    result["transcription"] = transcription;
                }
            }
            Err(reason) => result["review_reason"] = json!(bounded_text(&reason)),
        }
        if recovered {
            let current = driver
                .verify_recorded_field_observation(capture)
                .map_err(|error| error.to_string())
                .and_then(|()| verify_recovery_replay(driver, replay_before));
            if let Err(reason) = current {
                result["status"] = json!("changed_after_entry");
                result["review_reason"] = json!(bounded_text(&reason));
            }
            result["visual_recovery"]["status"] =
                json!(if result["status"] == "visually_matched" {
                    "verified"
                } else {
                    "stopped"
                });
        }
    }
}

#[cfg(windows)]
fn recorded_vision_status(outcome: crate::visual_review::RecordedVisionOutcome) -> &'static str {
    use crate::visual_review::RecordedVisionOutcome;
    match outcome {
        RecordedVisionOutcome::Matched => "visually_matched",
        RecordedVisionOutcome::Difference => "visual_difference",
        RecordedVisionOutcome::Unobserved => "visual_unobserved",
        RecordedVisionOutcome::Failed => "visual_review_failed",
        RecordedVisionOutcome::Stopped => "visual_unavailable",
    }
}

/// Activity observed during entry and its visual/native review, not causality,
/// server acceptance, or a complete inventory of browser/network channels.
#[cfg(any(windows, test))]
fn observed_replay_activity(before: Option<&Value>, after: Option<&Value>) -> Value {
    let unavailable = || serde_json::json!({"status":"unavailable","reason":"replay_counters_unavailable_or_changed"});
    let (Some(before), Some(after)) = (before, after) else {
        return unavailable();
    };
    if [before, after]
        .iter()
        .any(|report| report["schema"] != 1 || report["mode"] != "offline_response_replay")
    {
        return unavailable();
    }
    let mut delta = serde_json::Map::new();
    for key in [
        "served",
        "blocked",
        "cached_non_read_replies",
        "unreplayed_transports",
        "unmanaged_child_sessions",
    ] {
        let Some(value) = before[key]
            .as_u64()
            .zip(after[key].as_u64())
            .and_then(|(before, after)| after.checked_sub(before))
        else {
            return unavailable();
        };
        delta.insert(key.to_owned(), value.into());
    }
    serde_json::json!({"status":"observed","scope":"entry_and_review_interval","delta":delta})
}

#[cfg(windows)]
fn verify_recovery_replay(
    driver: &mut waypoint_browser::CdpDriver,
    before: Option<&Value>,
) -> Result<(), String> {
    let current = driver
        .rehearsal_replay_report()
        .map_err(|e| e.to_string())?;
    let activity = observed_replay_activity(before, Some(&current));
    if activity["status"] != "observed"
        || ![
            "blocked",
            "cached_non_read_replies",
            "unreplayed_transports",
            "unmanaged_child_sessions",
        ]
        .iter()
        .all(|key| activity["delta"][key] == 0)
        || !current["unsupported_targets"].is_object()
        || !before
            .is_some_and(|report| report["unsupported_targets"] == current["unsupported_targets"])
    {
        return Err("Saved-page request activity changed or could not be verified".into());
    }
    Ok(())
}

#[cfg(windows)]
fn recover_field_observation(
    driver: &mut waypoint_browser::CdpDriver,
    capture: &mut waypoint_browser::RecordedFieldCapture,
    replay_before: Option<&Value>,
    wait: impl FnOnce() -> Result<(), crate::InferenceMemoryRefusal>,
) -> Result<(), String> {
    verify_recovery_replay(driver, replay_before)?;
    wait().map_err(|error| error.to_string())?;
    verify_recovery_replay(driver, replay_before)?;
    driver
        .refresh_recorded_field_observation(capture)
        .map_err(|error| error.to_string())?;
    verify_recovery_replay(driver, replay_before)
}

#[cfg(any(windows, test))]
fn wait_for_review_memory(
    mut sample: impl FnMut() -> Result<(), crate::InferenceMemoryRefusal>,
    limit: std::time::Duration,
) -> Result<(), crate::InferenceMemoryRefusal> {
    use crate::InferenceMemoryRefusal;
    use std::time::{Duration, Instant};
    let started = Instant::now();
    loop {
        let result = sample();
        let remaining = limit.saturating_sub(started.elapsed());
        if !matches!(result, Err(InferenceMemoryRefusal::Pressure { .. })) || remaining.is_zero() {
            return result;
        }
        std::thread::sleep(remaining.min(Duration::from_secs(1)));
    }
}

#[cfg(windows)]
fn archive_history(
    screenshots: &Path,
    index: usize,
    kind: &str,
    value: &Value,
) -> Result<Value, String> {
    archive_candidate_answers(screenshots, "history", index, kind, value)
}

#[cfg(windows)]
fn archive_candidate_answers(
    screenshots: &Path,
    category: &str,
    index: usize,
    kind: &str,
    value: &Value,
) -> Result<Value, String> {
    use std::io::Write;
    if !matches!(category, "history" | "facts" | "progress" | "recovery")
        || !matches!(kind, "plan" | "entries")
        || index == 0
    {
        return Err("Invalid history archive identity".into());
    }
    let parent = screenshots
        .parent()
        .ok_or("History record directory is unavailable")?;
    directory(parent)?;
    let folder = parent.join("answers");
    if !folder.exists() {
        fs::create_dir(&folder).map_err(|_| "Cannot create history answers folder")?;
    }
    directory(&folder)?;
    let bytes = serde_json::to_vec(value).map_err(|_| "Cannot encode history answer evidence")?;
    if bytes.len() > 512 * 1024 {
        return Err("History answer evidence exceeds its limit".into());
    }
    let name = format!("{category}-{kind}-{index:03}.json");
    let mut file = tempfile::NamedTempFile::new_in(&folder)
        .map_err(|_| "Cannot stage history answer evidence")?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "Cannot retain history answer evidence")?;
    file.persist_noclobber(folder.join(&name))
        .map_err(|_| "Cannot create immutable history answer evidence")?;
    Ok(json!({"file":format!("answers/{name}"),"sha256":waypoint_domain::sha256_hex(&bytes)}))
}

#[cfg(windows)]
struct Rehearsal {
    vision: RehearsalVision,
    fields: Vec<Value>,
    captures: Vec<(usize, waypoint_browser::RecordedFieldCapture, bool)>,
    halted: bool,
    sections: Vec<Value>,
    dialogs: Vec<Value>,
    conditionals: usize,
    session_duration: std::time::Duration,
    profile: Result<Value, String>,
    candidate_metadata: Value,
    history: Result<Vec<crate::candidate_history::SelectedRecord>, String>,
    history_plans: Vec<Value>,
    qualifications: Option<crate::rehearsal_facts::Snapshot>,
    qualification_plans: Vec<Value>,
    privacy_profile: Value,
    candidate_documents: Vec<crate::rehearsal_candidate::SnapshotDocument>,
    candidate_selected: bool,
    attachment_plans: Vec<Value>,
    fact_plans: Vec<Value>,
    progress_checkpoints: Vec<Value>,
    public_research: bool,
    research_notes: Vec<Value>,
}

#[cfg(windows)]
impl Rehearsal {
    fn new(
        addr: std::net::SocketAddr,
        choice: Option<&crate::model_preferences::Choice>,
        runtime: Option<crate::private_inference::RuntimeBinding>,
        allow_inference: bool,
        public_research: bool,
        candidate: Option<(crate::rehearsal_candidate::Snapshot, String)>,
        session: (std::time::Instant, std::time::Duration),
    ) -> Self {
        let (deadline, session_duration) = session;
        let vision = RehearsalVision::new(addr, choice, runtime, allow_inference, deadline);
        let candidate_metadata = candidate.as_ref().map(|(snapshot,binding)|snapshot.metadata(binding)).unwrap_or_else(||json!({"scope":"bundled_fictional_contact_profile","profile_sha256":waypoint_domain::sha256_hex(include_bytes!("../data/rehearsal-profile.json")),"documents":"bundled_fictional_pdfs"}));
        let candidate_selected = candidate.is_some();
        let candidate_documents = candidate
            .as_ref()
            .map(|(snapshot, _)| snapshot.documents.clone())
            .unwrap_or_default();
        let qualifications = candidate
            .as_ref()
            .and_then(|(snapshot, _)| snapshot.facts.clone());
        let history_raw = candidate
            .as_ref()
            .and_then(|(snapshot, _)| snapshot.history.clone());
        let history = history_raw
            .as_deref()
            .map(crate::candidate_history::reviewed)
            .transpose()
            .map(Option::unwrap_or_default);
        let profile = match candidate {
            Some((snapshot, _)) => Ok(snapshot.contacts),
            None => serde_json::from_str(include_str!("../data/rehearsal-profile.json"))
                .map_err(|_| "The bundled fictional rehearsal profile is invalid".to_owned()),
        };
        let mut privacy_profile = profile.as_ref().cloned().unwrap_or_else(|_| json!({}));
        if let Some(raw) = history_raw {
            privacy_profile["career_history"] = json!(raw);
        }
        if let Some(facts) = &qualifications {
            privacy_profile["reviewed_qualifications"] = json!(facts
                .facts
                .iter()
                .map(|f| f.text.clone())
                .collect::<Vec<_>>());
        }
        let halted = vision.model.is_none() || profile.is_err();
        Self {
            vision,
            fields: Vec::new(),
            captures: Vec::new(),
            halted,
            sections: Vec::new(),
            dialogs: Vec::new(),
            conditionals: 0,
            session_duration,
            profile,
            candidate_metadata,
            history,
            history_plans: Vec::new(),
            qualifications,
            qualification_plans: Vec::new(),
            privacy_profile,
            candidate_documents,
            candidate_selected,
            attachment_plans: Vec::new(),
            fact_plans: Vec::new(),
            progress_checkpoints: Vec::new(),
            public_research,
            research_notes: Vec::new(),
        }
    }

    fn stop_at_deadline(&mut self, view: usize) -> bool {
        if std::time::Instant::now() < self.vision.deadline {
            return false;
        }
        self.sections
            .push(json!({"view":view,"status":"stopped","reason":"time_budget_reached"}));
        self.halted = true;
        true
    }

    fn exercise(
        &mut self,
        driver: &mut waypoint_browser::CdpDriver,
        evidence: &mut ScreenshotArchive<'_>,
        view: usize,
        form: &waypoint_applications::driver::LiveForm,
    ) -> Option<waypoint_applications::driver::LiveForm> {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::{FormFieldType, PacketValue};
        if self.halted || form.independent_field_entry_blocked() {
            return None;
        }
        let directory = evidence.directory;
        for (index, capture, _) in &self.captures {
            if driver.verify_recorded_field(capture).is_err() {
                self.fields[*index]["status"] = json!("changed_after_entry");
                self.fields[*index]["native_verified"] = json!(false);
                self.halted = true;
                return None;
            }
        }
        let Ok(profile) = self.profile.as_ref() else {
            return None;
        };
        let profile_sha = self.candidate_metadata["profile_sha256"].clone();
        let schema_sha = waypoint_domain::sha256_hex(form.schema.schema_hash_input().as_bytes());
        let (plan, diagnostics) = match self.vision.plan_profile(form, profile) {
            Ok(result) => (result.plan, Some(result.diagnostics)),
            Err(error) => (Err(error), None),
        };
        let (mut mappings, review_selectors) = match plan {
            Ok(plan) if plan.schema == form.schema => {
                let warnings: Vec<_> = plan
                    .manual_review
                    .iter()
                    .take(12)
                    .map(|warning| bounded_text(warning))
                    .collect();
                self.fact_plans.push(json!({"view":view,"status":"proposed","source":self.candidate_metadata["scope"],
                    "profile_sha256":profile_sha,"schema_sha256":schema_sha,"proposed_fields":plan.suggestions.len(),
                    "manual_fields":plan.model_review_selectors.len(),"warnings":warnings,
                    "omitted_warnings":plan.manual_review.len().saturating_sub(12),"diagnostics":diagnostics}));
                (plan.suggestions, plan.model_review_selectors)
            }
            result => {
                let reason = result
                    .err()
                    .unwrap_or_else(|| "The form changed during profile matching".into());
                self.fact_plans.push(json!({"view":view,"status":"needs_review","source":self.candidate_metadata["scope"],
                    "profile_sha256":profile_sha,"schema_sha256":schema_sha,"reason":bounded_text(&reason),
                    "diagnostic":reason.chars().take(512).collect::<String>(),"diagnostics":diagnostics}));
                // Other actors can still propose independent documents. No
                // exact-label fallback becomes a contact-entry plan here.
                (Vec::new(), std::collections::HashSet::new())
            }
        };
        let history_start = self.fields.len();
        let mut history_sources = std::collections::BTreeMap::new();
        if let Ok(records) = &self.history {
            if !records.is_empty() {
                let eligible: Vec<_> = form
                    .schema
                    .fields
                    .iter()
                    .filter(|field| {
                        crate::history_advice::eligible(field)
                            && !review_selectors.contains(&field.selector)
                            && !mappings
                                .iter()
                                .any(|mapping| mapping.field_selector == field.selector)
                            && !self
                                .captures
                                .iter()
                                .any(|(_, capture, _)| capture.matches_field(field))
                    })
                    .cloned()
                    .collect();
                let offered = &eligible;
                if !offered.is_empty() {
                    let index = self.history_plans.len() + 1;
                    let mut report = json!({"view":view,"source":self.candidate_metadata,"eligible_fields":eligible.len(),"offered_fields":offered.len(),"omitted_eligible_fields":eligible.len()-offered.len(),"status":"needs_review"});
                    match self.vision.plan_history(offered, records).and_then(|plan| {
                        crate::history_batch::resolve_checked_proposals(offered, records, &plan)
                            .map(|resolved| (plan, resolved))
                    }) {
                        Ok((plan, resolved)) => {
                            report["status"] = json!("proposed");
                            report["plan"] = plan;
                            let mut unsupported = Vec::new();
                            for (mapping, source) in resolved {
                                let field = offered
                                    .iter()
                                    .find(|field| field.selector == mapping.field_selector)
                                    .expect("resolved field exists");
                                let supported = matches!(
                                    field.field_type,
                                    FormFieldType::Text
                                        | FormFieldType::TextArea
                                        | FormFieldType::Select { .. }
                                        | FormFieldType::Radio { .. }
                                ) && matches!(&mapping.value,PacketValue::Text(value) if !value.trim().is_empty() && value.len()<=1000);
                                if supported {
                                    history_sources.insert(mapping.field_selector.clone(), source);
                                    mappings.push(mapping);
                                } else {
                                    unsupported.push(json!({"selector":mapping.field_selector,"reason":"rehearsal_control_or_value_not_supported"}));
                                }
                            }
                            report["entry_deferred"] = json!(unsupported);
                        }
                        Err(error) => report["reason"] = json!(bounded_text(&error)),
                    }
                    match archive_history(directory, index, "plan", &report) {
                        Ok(archive) => report["archive"] = archive,
                        Err(error) => {
                            report["status"] = json!("needs_review");
                            report["archive_error"] = json!(error);
                            self.halted = true;
                        }
                    }
                    self.history_plans.push(report);
                    if self.halted {
                        return None;
                    }
                }
            }
        }
        let mut qualification_sources = std::collections::BTreeMap::new();
        if let Some(facts) = &self.qualifications {
            let eligible: Vec<_> = form
                .schema
                .fields
                .iter()
                .filter(|field| {
                    crate::rehearsal_facts::eligible(field)
                        && !review_selectors.contains(&field.selector)
                        && !mappings
                            .iter()
                            .any(|mapping| mapping.field_selector == field.selector)
                        && !self
                            .captures
                            .iter()
                            .any(|(_, capture, _)| capture.matches_field(field))
                        && form
                            .schema
                            .fields
                            .iter()
                            .filter(|f| f.label.trim().eq_ignore_ascii_case(field.label.trim()))
                            .count()
                            == 1
                })
                .cloned()
                .collect();
            let offered: Vec<_> = eligible
                .iter()
                .take(crate::rehearsal_facts::MAX_FIELDS)
                .cloned()
                .collect();
            if !offered.is_empty() {
                let mut report = json!({"view":view,"source":facts.metadata(),"eligible_fields":eligible.len(),"offered_fields":offered.len(),"omitted_eligible_fields":eligible.len()-offered.len(),"status":"needs_review"});
                match self
                    .vision
                    .plan_qualifications(&offered, facts)
                    .and_then(|plan| {
                        crate::rehearsal_facts::resolve(&offered, facts, &plan)
                            .map(|resolved| (plan, resolved))
                    }) {
                    Ok((plan, resolved)) => {
                        report["status"] = json!("proposed");
                        report["plan"] = plan;
                        for (mapping, source) in resolved {
                            qualification_sources.insert(mapping.field_selector.clone(), source);
                            mappings.push(mapping);
                        }
                    }
                    Err(error) => report["reason"] = json!(bounded_text(&error)),
                }
                match archive_candidate_answers(
                    directory,
                    "facts",
                    self.qualification_plans.len() + 1,
                    "plan",
                    &report,
                ) {
                    Ok(archive) => report["archive"] = archive,
                    Err(error) => {
                        report["archive_error"] = json!(error);
                        self.halted = true;
                    }
                }
                self.qualification_plans.push(report);
                if self.halted {
                    return None;
                }
            }
        }
        let mut attachments = std::collections::BTreeMap::new();
        if form
            .schema
            .fields
            .iter()
            .any(|field| field.field_type == FormFieldType::File)
        {
            let descriptions: Vec<_> = self
                .candidate_documents
                .iter()
                .map(|doc| doc.description())
                .collect();
            let documents: Vec<_> = if self.candidate_selected {
                self.candidate_documents
                    .iter()
                    .zip(&descriptions)
                    .map(
                        |(doc, description)| crate::attachment_mapping::DocumentOption {
                            id: &doc.id,
                            description,
                        },
                    )
                    .collect()
            } else {
                vec![
                    crate::attachment_mapping::DocumentOption {
                        id: "resume",
                        description: "The fictional candidate's resume/CV PDF",
                    },
                    crate::attachment_mapping::DocumentOption {
                        id: "cover_letter",
                        description: "The fictional candidate's cover letter PDF",
                    },
                ]
            };
            let plan = self.vision.plan_attachments(&form.schema, &documents);
            match plan {
                Ok(plan) => {
                    self.attachment_plans
                        .push(json!({"view":view,"status":"proposed","decisions":plan.decisions}));
                    for decision in plan.decisions {
                        let name = if self.candidate_selected {
                            self.candidate_documents
                                .iter()
                                .find(|doc| {
                                    Some(doc.id.as_str()) == decision.document_id.as_deref()
                                })
                                .map(|doc| doc.filename.as_str())
                        } else {
                            match decision.document_id.as_deref() {
                                Some("resume") => Some(SAMPLE_RESUME),
                                Some("cover_letter") => Some(SAMPLE_COVER),
                                _ => None,
                            }
                        };
                        if let Some(name) = name {
                            attachments.insert(decision.selector, name);
                        }
                    }
                }
                Err(reason) => self.attachment_plans.push(
                    json!({"view":view,"status":"needs_review","reason":bounded_text(&reason)}),
                ),
            }
        }
        let mut captures = Vec::new();
        let mut continuation = None;
        let mut settled_form = form.clone();
        for field in &form.schema.fields {
            if self
                .captures
                .iter()
                .any(|(_, capture, _)| capture.matches_field(field))
            {
                continue;
            }
            if std::time::Instant::now() >= self.vision.deadline {
                self.sections
                    .push(json!({"view":view,"status":"stopped","reason":"time_budget_reached"}));
                self.halted = true;
                break;
            }
            if field.sensitive
                || crate::suggestions::credential_autocomplete(field)
                || (field.field_type != FormFieldType::File
                    && review_selectors.contains(&field.selector))
            {
                continue;
            }
            if form
                .schema
                .fields
                .iter()
                .filter(|f| f.label.trim().eq_ignore_ascii_case(field.label.trim()))
                .count()
                != 1
            {
                continue;
            }
            let value = if field.field_type == FormFieldType::File {
                let Some(name) = attachments.get(&field.selector) else {
                    continue;
                };
                *name
            } else {
                let Some(value) = mappings.iter().find_map(|mapping| match &mapping.value {
                    PacketValue::Text(value) if mapping.field_selector == field.selector => {
                        Some(value.as_str())
                    }
                    _ => None,
                }) else {
                    continue;
                };
                value
            };
            let index = self.fields.len();
            let mut result =
                json!({"view":view,"label":bounded_text(&field.label),"status":"entry_unverified"});
            result["summary_key"] = json!(waypoint_browser::SummaryKey::from_field(field));
            if field.field_type != FormFieldType::File {
                result["candidate_source"] = json!({"scope":self.candidate_metadata["scope"],
                    "profile_sha256":profile_sha,"snapshot_sha256":self.candidate_metadata["snapshot_sha256"],"schema_sha256":schema_sha});
            }
            if let Some(source) = history_sources.get(&field.selector) {
                result["candidate_history"] = json!({"snapshot_sha256":self.candidate_metadata["snapshot_sha256"],"history_sha256":self.candidate_metadata["history_sha256"],"source_profile_sha256":self.candidate_metadata["source_profile_sha256"],"schema_sha256":schema_sha,"proposal":source,"note":"Resolved from checked saved career history; experience and delivery are not independently verified."});
            }
            if let Some(source) = qualification_sources.get(&field.selector) {
                result["candidate_facts"] = json!({"snapshot_sha256":self.candidate_metadata["snapshot_sha256"],"source_facts_sha256":self.candidate_metadata["source_facts_sha256"],"schema_sha256":schema_sha,"proposal":source,"note":"Grounded in reviewed saved qualifications; candidate facts and delivery are not independently verified."});
            }
            if field.field_type == FormFieldType::File && self.candidate_selected {
                let document = self
                    .candidate_documents
                    .iter()
                    .find(|doc| doc.filename == value)?;
                result["candidate_document"] = json!({"snapshot_sha256":self.candidate_metadata["snapshot_sha256"],"document":document,"schema_sha256":schema_sha});
            }
            let mut target_refused = false;
            let mut witness = None;
            let verified: Vec<_> = self
                .captures
                .iter()
                .chain(captures.iter())
                .map(|(_, capture, _)| capture)
                .collect();
            let replay_before = driver.rehearsal_replay_report().ok();
            match driver.rehearse_recorded_field_progressive(
                form,
                &field.selector,
                value,
                &verified,
                |target| {
                    result["control"] = json!(target.control());
                    witness = target.witness();
                    let reviewed =
                        self.vision
                            .review_target(evidence, view, "before_entry", target);
                    target_refused = reviewed.is_err();
                    reviewed
                },
            ) {
                Ok(mut capture) => {
                    if let Some(next) = capture.continuation() {
                        settled_form = next.clone();
                        continuation = Some(next.clone());
                    }
                    result["control"] = json!(capture.control());
                    result["kind"] = json!(capture.kind());
                    result["status"] = json!("visual_unavailable");
                    let image_saved = match save_field_images(evidence, &capture, &mut result) {
                        Ok(()) => {
                            self.vision.review_entered_field(
                                driver,
                                &mut capture,
                                evidence,
                                index,
                                replay_before.as_ref(),
                                &mut result,
                            );
                            true
                        }
                        Err(reason) => {
                            result["review_reason"] = json!(bounded_text(&reason));
                            false
                        }
                    };
                    if !image_saved || result["status"] != "visually_matched" {
                        self.halted = true;
                    }
                    if driver.verify_recorded_field(&capture).is_err() {
                        result["status"] = json!("changed_after_entry");
                        result["native_verified"] = json!(false);
                        self.halted = true;
                    } else {
                        result["native_verified"] = json!(true);
                        result["native_value"] = json!(capture.observed_value());
                        result["intended_value"] = json!(capture.value());
                        result["value_comparison"] = json!(capture.value_comparison());
                    }
                    capture.release_archived_pixels();
                    captures.push((index, capture, image_saved));
                }
                Err(error) => {
                    if target_refused {
                        result["status"] = json!("before_entry_review_required");
                    }
                    result["review_reason"] = json!(bounded_text(&error.to_string()));
                    if !target_refused {
                        result["native_verified"] = json!(false);
                        result["failure_observation"] = match witness
                            .as_ref()
                            .and_then(|witness| driver.observe_recorded_field(witness).ok())
                        {
                            Some(observation) => match evidence.save(observation.image()) {
                                Ok((name, hash)) => {
                                    result["image"] = json!(name);
                                    result["sha256"] = json!(hash);
                                    json!({"status":"observed","scope":"current_same_document_and_schema",
                                        "control":observation.control(),"native_value":observation.value(),"entry_verified":false})
                                }
                                Err(_) => {
                                    json!({"status":"unavailable","reason":"image_not_retained","entry_verified":false})
                                }
                            },
                            None => {
                                json!({"status":"unavailable","reason":"current_bound_evidence_unavailable","entry_verified":false})
                            }
                        };
                    }
                    self.halted = true;
                    if self.public_research && self.research_notes.is_empty() {
                        let kind = match field.field_type {
                            FormFieldType::File => "file attachment",
                            FormFieldType::Select { .. } | FormFieldType::Radio { .. } => {
                                "selection"
                            }
                            _ => "text input",
                        };
                        let family = url::Url::parse(&form.schema.origin)
                            .ok()
                            .and_then(|u| u.host_str().map(waypoint_policy::source_for_host))
                            .unwrap_or("user_supplied_url");
                        let platform = if family == "user_supplied_url" {
                            "unidentified web form"
                        } else {
                            family
                        };
                        let context = crate::form_research::technical_question(
                            kind,
                            platform,
                            target_refused,
                        );
                        self.research_notes.push(match self.vision.research_issue(directory,&self.privacy_profile,&context) {
                            Ok(value)=>value,Err(error)=>json!({"status":"needs_review","reason":bounded_text(&error),"form_actions_authorized":false}),
                        });
                    }
                }
            }
            let replay_after = driver.rehearsal_replay_report().ok();
            result["replay_activity"] =
                observed_replay_activity(replay_before.as_ref(), replay_after.as_ref());
            self.fields.push(result);
            // Preserve observed entries even if a later native call or the final
            // replay report fails. These immutable snapshots never establish
            // current/final review, continued validity or delivery.
            let progress = json!({"schema":2,"scope":"partial_entry_checkpoint","final_review":false,"view":view,
                "candidate_source":self.candidate_metadata,"entry_index":index+1,"fields":[self.fields[index]],"halted_at_capture":self.halted});
            match archive_candidate_answers(
                directory,
                "progress",
                self.fields.len(),
                "entries",
                &progress,
            ) {
                Ok(archive) => self.progress_checkpoints.push(archive),
                Err(error) => {
                    self.progress_checkpoints
                        .push(json!({"status":"needs_review","archive_error":error}));
                    self.halted = true;
                }
            }
            if self.halted || continuation.is_some() {
                break;
            }
        }
        if !captures.is_empty() {
            let unchanged = driver
                .inspect_current()
                .is_ok_and(|current| current == settled_form);
            for (index, capture, image_saved) in captures {
                if unchanged && driver.verify_recorded_field(&capture).is_ok() {
                    self.fields[index]["native_verified"] = json!(true);
                    self.fields[index]["native_value"] = json!(capture.observed_value());
                    self.fields[index]["intended_value"] = json!(capture.value());
                    self.fields[index]["value_comparison"] = json!(capture.value_comparison());
                    if matches!(capture.kind(), waypoint_browser::RecordedFieldKind::File) {
                        let sha256 = if self.candidate_selected {
                            self.candidate_documents
                                .iter()
                                .find(|doc| doc.filename == capture.value())
                                .map(|doc| doc.sha256.clone())
                        } else {
                            sample_attachment_bytes(capture.value())
                                .map(waypoint_domain::sha256_hex)
                        };
                        let Some(sha256) = sha256 else {
                            self.fields[index]["status"] = json!("changed_after_entry");
                            self.fields[index]["native_verified"] = json!(false);
                            self.halted = true;
                            continue;
                        };
                        self.fields[index]["attachment"] =
                            json!({"name":capture.value(), "sha256":sha256});
                    }
                    self.captures.push((index, capture, image_saved));
                } else {
                    self.fields[index]["status"] = json!("changed_after_entry");
                    self.fields[index]["native_verified"] = json!(false);
                    self.halted = true;
                }
            }
        }
        if self.fields[history_start..]
            .iter()
            .any(|field| field.get("candidate_history").is_some())
        {
            let report = json!({"view":view,"snapshot_sha256":self.candidate_metadata["snapshot_sha256"],"fields":self.fields[history_start..].iter().filter(|field|field.get("candidate_history").is_some()).collect::<Vec<_>>()});
            if let Err(error) =
                archive_history(directory, self.history_plans.len(), "entries", &report)
            {
                self.history_plans
                    .push(json!({"view":view,"status":"needs_review","archive_error":error}));
                self.halted = true;
            }
        }
        if self.fields[history_start..]
            .iter()
            .any(|field| field.get("candidate_facts").is_some())
        {
            let report = json!({"view":view,"snapshot_sha256":self.candidate_metadata["snapshot_sha256"],"fields":self.fields[history_start..].iter().filter(|field|field.get("candidate_facts").is_some()).collect::<Vec<_>>()});
            if let Err(error) = archive_candidate_answers(
                directory,
                "facts",
                self.qualification_plans.len(),
                "entries",
                &report,
            ) {
                self.qualification_plans
                    .push(json!({"view":view,"status":"needs_review","archive_error":error}));
                self.halted = true;
            }
        }
        if self.halted {
            None
        } else {
            continuation
        }
    }

    fn resolve_dialog(
        &mut self,
        driver: &mut waypoint_browser::CdpDriver,
        evidence: &mut ScreenshotArchive<'_>,
        view: usize,
        form: &waypoint_applications::driver::LiveForm,
        section_dialogs: &mut crate::dialog_planning::SectionDialogs,
    ) -> Option<waypoint_applications::driver::LiveForm> {
        use waypoint_browser::RecordedDialogOutcome;
        // Active captures are bound to their original form. Earlier sections
        // retain their archived evidence and do not block this observation.
        if !self.captures.is_empty() {
            return None;
        }
        let dialog = match driver.recorded_dialog(form) {
            Ok(Some(dialog)) => dialog,
            Ok(None) => return None,
            Err(_) => {
                self.dialogs.push(json!({"view":view,"status":"needs_review","reason":"Dialog context could not be verified"}));
                self.halted = true;
                return None;
            }
        };
        let description = dialog.description();
        let mut record = json!({"view":view,"status":"needs_review","scope":if self.candidate_selected{"offline_saved_copy_only"}else{"fictional_saved_copy_only"},"description":description});
        if !section_dialogs.record(&record["description"]) {
            record["reason"] = json!("The same dialog returned before the section advanced");
            self.dialogs.push(record);
            self.halted = true;
            return None;
        }
        let outcome = (|| -> Result<Option<waypoint_applications::driver::LiveForm>, String> {
            self.vision.memory.check()?;
            let model = self
                .vision
                .model
                .as_mut()
                .ok_or("The local model needs review")?;
            let plan = crate::visual_review::propose_recorded_dialog(
                model,
                self.vision.addr,
                &description,
            )?;
            record["proposal"] = json!(plan);
            let Some(choice) = plan.control else {
                return Ok(None);
            };
            let result = driver
                .dismiss_recorded_dialog_reviewed(&dialog, &choice, |target| {
                    self.vision
                        .review_target(evidence, view, "before_dialog", target)
                })
                .map_err(|error| error.to_string())?;
            let (capture, image) = match result {
                RecordedDialogOutcome::NativeClosed { capture } => {
                    record["native_closed"] = json!(true);
                    let image = capture.image().to_owned();
                    (Some(capture), Some(image))
                }
                RecordedDialogOutcome::Stopped { reason, image } => {
                    record["reason"] = json!(reason);
                    (None, image)
                }
            };
            if let Some(image) = image {
                let (name, digest) = evidence.save(&image)?;
                record["image"] = json!(name);
                record["sha256"] = json!(digest);
                if let Some(capture) = capture {
                    self.vision.memory.check()?;
                    let model = self
                        .vision
                        .model
                        .as_mut()
                        .ok_or("The local model needs review")?;
                    let observed = crate::visual_review::inspect_recorded_dialog_image(
                        model,
                        self.vision.addr,
                        &image,
                    )?;
                    let accepted = crate::dialog_planning::visual_closed(&observed);
                    record["post_visual"] = observed;
                    let fresh = driver.verify_recorded_dialog_closed(&capture).is_ok();
                    record["closure_reverified"] = json!(fresh);
                    if accepted && fresh {
                        record["status"] = json!("closed_in_saved_copy");
                        record["to"] = json!(view + 1);
                        return Ok(Some(capture.form().clone()));
                    }
                    record["reason"] =
                        json!("The image check did not verify that the dialog closed");
                }
            }
            Ok(None)
        })();
        match outcome {
            Ok(Some(form)) => {
                self.dialogs.push(record);
                Some(form)
            }
            Ok(None) => {
                self.dialogs.push(record);
                self.halted = true;
                None
            }
            Err(error) => {
                record["reason"] = json!(bounded_text(&error));
                self.dialogs.push(record);
                self.halted = true;
                None
            }
        }
    }

    fn finish(self, screenshot_bytes: usize) -> Value {
        json!({"scope":if self.candidate_selected{self.candidate_metadata["scope"].as_str().unwrap_or("saved_candidate_materials")}else{"fictional_saved_form"},"candidate_source":self.candidate_metadata,"profile_sha256":self.candidate_metadata["profile_sha256"],"profile_error":self.profile.as_ref().err(),"time_limit_seconds":self.session_duration.as_secs(),"progress_checkpoints":self.progress_checkpoints,"halted":self.halted,"fields":self.fields,"fact_plans":self.fact_plans,"history_plans":self.history_plans,"qualification_plans":self.qualification_plans,"history_error":self.history.as_ref().err(),"attachment_plans":self.attachment_plans,"sections":self.sections,"dialogs":self.dialogs,"vision_model":self.vision.identity,
            "conditional_replans":self.conditionals,"screenshot_bytes":screenshot_bytes,"screenshot_budget_bytes":SCREENSHOT_BUDGET_BYTES,
            "public_research_enabled":self.public_research,"public_research":self.research_notes,
            "visual_gate":{"required":true,"available":self.vision.model.is_some(),"targets":self.vision.targets}})
    }
}

#[cfg(windows)]
fn capture_view(
    driver: &mut waypoint_browser::CdpDriver,
    evidence: &mut ScreenshotArchive<'_>,
    label: &str,
    mut form: waypoint_applications::driver::LiveForm,
    embedded: bool,
) -> Result<Value, String> {
    use waypoint_applications::driver::BrowserDriver;
    let url = driver.current_url().map_err(|error| error.to_string())?;
    let image = if embedded {
        driver.screenshot_captured_frame()
    } else {
        driver.screenshot_base64()
    }
    .map_err(|error| error.to_string())?;
    let current = driver
        .inspect_current()
        .map_err(|error| error.to_string())?;
    if current.origin != form.origin
        || current.schema != form.schema
        || driver.current_url().map_err(|error| error.to_string())? != url
    {
        return Err("The saved page changed during the capture. Create another preview.".into());
    }
    for wall in current.walls {
        if !form.walls.contains(&wall) {
            form.walls.push(wall);
        }
    }
    let controls = form
        .schema
        .fields
        .iter()
        .take(VIEW_FIELD_LIMIT)
        .map(|field| driver.recorded_control_identity(&form, &field.selector))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut view = save_view(evidence, label, &form, &image, embedded)?;
    for (field, control) in view["fields"]
        .as_array_mut()
        .ok_or("Saved control inventory is unavailable")?
        .iter_mut()
        .zip(controls)
    {
        field["control"] = json!(control);
    }
    Ok(view)
}

#[cfg(windows)]
fn bounded_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

#[cfg(windows)]
fn save_review_observation(
    evidence: &mut ScreenshotArchive<'_>,
    label: &str,
    form: &waypoint_applications::driver::LiveForm,
    image: &str,
    embedded: bool,
    (scope_view, section, from): (usize, usize, usize),
) -> Result<Value, String> {
    let mut saved = save_view(evidence, label, form, image, embedded)?;
    saved["scope_view"] = json!(scope_view);
    saved["section"] = json!(section);
    saved["review_observation"] = json!(true);
    saved["observed_after_view"] = json!(from);
    saved["final_review"] = json!(false);
    Ok(saved)
}

#[cfg(windows)]
fn save_view(
    evidence: &mut ScreenshotArchive<'_>,
    label: &str,
    form: &waypoint_applications::driver::LiveForm,
    image: &str,
    embedded: bool,
) -> Result<Value, String> {
    let (name, sha256) = evidence.save(image)?;
    let fields: Vec<_> = form
        .schema
        .fields
        .iter()
        .take(VIEW_FIELD_LIMIT)
        .map(|field| {
            use waypoint_applications::form_mapping::FormFieldType;
            let kind = match field.field_type {
                FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea => "text",
                FormFieldType::Number { .. } => "number",
                FormFieldType::Select { .. } => "select",
                FormFieldType::Radio { .. } => "radio",
                FormFieldType::File => "file",
                _ => "unsupported",
            };
            let label_truncated = field
                .label
                .chars()
                .filter(|character| !character.is_control())
                .nth(160)
                .is_some();
            json!({"label":bounded_text(&field.label),"label_truncated":label_truncated,"required":field.required,"kind":kind})
        })
        .collect();
    let walls: Vec<_> = form
        .walls
        .iter()
        .take(8)
        .map(|wall| bounded_text(wall))
        .collect();
    Ok(
        json!({"kind":if embedded{"frame"}else{"page"},"label":bounded_text(label),"origin":bounded_text(&form.origin),
        "image":name,"sha256":sha256,"fields":fields,"field_count":form.schema.fields.len(),"omitted_fields":form.schema.fields.len().saturating_sub(VIEW_FIELD_LIMIT),"walls":walls,"wall_count":form.walls.len(),"omitted_walls":form.walls.len().saturating_sub(walls.len()),"independent_entry":{"blocked":form.independent_field_entry_blocked(),"barriers":form.independent_field_barriers().iter().take(8).map(|value|bounded_text(value)).collect::<Vec<_>>(),"barrier_count":form.independent_field_barriers().len()}}),
    )
}

#[cfg(windows)]
const SCREENSHOT_BUDGET_BYTES: usize = 256 * 1024 * 1024;

#[cfg(windows)]
struct ScreenshotArchive<'a> {
    directory: &'a Path,
    count: u32,
    bytes: usize,
}

#[cfg(windows)]
impl<'a> ScreenshotArchive<'a> {
    fn new(directory: &'a Path) -> Self {
        Self {
            directory,
            count: 0,
            bytes: 0,
        }
    }

    fn save(&mut self, image: &str) -> Result<(String, String), String> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use std::io::Write;
        if image.len() > 8 * 1024 * 1024 {
            return Err("Saved page image exceeds its limit".into());
        }
        let bytes = STANDARD
            .decode(image)
            .map_err(|_| "Invalid saved page image")?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.len() > 6 * 1024 * 1024 {
            return Err("Invalid saved page image".into());
        }
        if bytes.len() > SCREENSHOT_BUDGET_BYTES.saturating_sub(self.bytes) {
            return Err("The saved page reached its screenshot storage budget".into());
        }
        let sequence = self
            .count
            .checked_add(1)
            .ok_or("Screenshot sequence exhausted")?;
        let name = format!("{sequence:04}.png");
        let mut file = tempfile::NamedTempFile::new_in(self.directory)
            .map_err(|_| "Cannot stage saved page image")?;
        file.write_all(&bytes)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| "Cannot write saved page image")?;
        file.persist_noclobber(self.directory.join(&name))
            .map_err(|_| "Cannot preserve saved page image")?;
        self.count = sequence;
        self.bytes += bytes.len();
        Ok((name, waypoint_domain::sha256_hex(&bytes)))
    }
}

#[cfg(windows)]
fn save_field_images(
    evidence: &mut ScreenshotArchive<'_>,
    capture: &waypoint_browser::RecordedFieldCapture,
    field: &mut Value,
) -> Result<(), String> {
    let (name, digest) = evidence.save(capture.image())?;
    let field = field
        .as_object_mut()
        .ok_or("Invalid field evidence record")?;
    field.insert("image".into(), json!(name));
    field.insert("sha256".into(), json!(digest));
    for key in ["detail_image", "detail_sha256", "detail_capture_error"] {
        field.remove(key);
    }
    if let Some(reason) = capture.detail_error() {
        field.insert("detail_capture_error".into(), json!(bounded_text(reason)));
    }
    if let Some(image) = capture.detail_image() {
        let (name, digest) = evidence.save(image)?;
        field.insert("detail_image".into(), json!(name));
        field.insert("detail_sha256".into(), json!(digest));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn review_memory_wait_requires_a_valid_sample_within_its_budget() {
        use crate::InferenceMemoryRefusal;
        use std::time::Duration;
        let pressure = || InferenceMemoryRefusal::Pressure {
            available: 7 * 1024_u64.pow(3),
            required: 8 * 1024_u64.pow(3),
        };
        assert_eq!(
            super::wait_for_review_memory(|| Err(pressure()), Duration::ZERO),
            Err(pressure())
        );
        let mut samples = [Err(InferenceMemoryRefusal::Unavailable), Ok(())].into_iter();
        assert_eq!(
            super::wait_for_review_memory(|| samples.next().unwrap(), Duration::from_millis(1)),
            Err(InferenceMemoryRefusal::Unavailable)
        );
        assert_eq!(samples.next(), Some(Ok(())));
        let mut samples = [Err(pressure()), Ok(())].into_iter();
        assert!(super::wait_for_review_memory(
            || samples.next().unwrap(),
            Duration::from_millis(20)
        )
        .is_ok());
        assert_eq!(samples.next(), None);
    }

    #[cfg(windows)]
    #[test]
    fn history_answer_archives_are_bounded_distinct_and_never_overwritten() {
        let folder = tempfile::tempdir().unwrap();
        let screenshots = folder.path().join("screenshots");
        fs::create_dir(&screenshots).unwrap();
        let value = serde_json::json!({"question":"Current employer","answer":"Fictional company"});
        let first = super::archive_history(&screenshots, 1, "plan", &value).unwrap();
        let path = folder.path().join(first["file"].as_str().unwrap());
        let bytes = fs::read(&path).unwrap();
        assert_eq!(first["sha256"], waypoint_domain::sha256_hex(&bytes));
        assert!(super::archive_history(
            &screenshots,
            1,
            "plan",
            &serde_json::json!({"changed":true})
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(super::archive_history(&screenshots, 1, "entries", &value).is_ok());
        let checkpoint = serde_json::json!({"scope":"partial_entry_checkpoint","final_review":false,"fields":[value.clone()]});
        let progress =
            super::archive_candidate_answers(&screenshots, "progress", 1, "entries", &checkpoint)
                .unwrap();
        let progress_path = folder.path().join(progress["file"].as_str().unwrap());
        let progress_bytes = fs::read(&progress_path).unwrap();
        assert_eq!(
            progress["sha256"],
            waypoint_domain::sha256_hex(&progress_bytes)
        );
        assert!(
            super::archive_candidate_answers(&screenshots, "progress", 1, "entries", &value)
                .is_err()
        );
        assert_eq!(fs::read(progress_path).unwrap(), progress_bytes);
        assert!(super::archive_candidate_answers(
            &screenshots,
            "progress",
            65,
            "entries",
            &checkpoint
        )
        .is_ok());
        assert!(super::archive_candidate_answers(
            &screenshots,
            "../progress",
            2,
            "entries",
            &value
        )
        .is_err());
        assert!(super::archive_history(&screenshots, 2, "../outside", &value).is_err());
        assert!(super::archive_history(
            &screenshots,
            2,
            "plan",
            &serde_json::json!({"value":"x".repeat(512*1024)})
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn screenshot_archive_counts_committed_bytes_and_keeps_prior_images() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let folder = tempfile::tempdir().unwrap();
        let mut archive = ScreenshotArchive::new(folder.path());
        let bytes = b"\x89PNG\r\n\x1a\nFictional image data";
        let image = STANDARD.encode(bytes);
        for number in 1..=85 {
            let (name, digest) = archive.save(&image).unwrap();
            assert_eq!(name, format!("{number:04}.png"));
            assert_eq!(digest, waypoint_domain::sha256_hex(bytes));
        }
        assert_eq!(archive.bytes, 85 * bytes.len());
        archive.bytes = SCREENSHOT_BUDGET_BYTES - bytes.len() + 1;
        assert!(archive.save(&image).unwrap_err().contains("storage budget"));
        assert_eq!(archive.count, 85);
        assert!(!folder.path().join("0086.png").exists());
        assert_eq!(fs::read(folder.path().join("0001.png")).unwrap(), bytes);
    }

    use super::*;
    use serde_json::json;

    #[cfg(windows)]
    #[test]
    #[ignore = "explicit private offline fixture and headless engine; native inspection only, no field actions or model"]
    fn recorded_profile_inspection_probe() {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::BrowserCommand;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fixture {
            python: PathBuf,
            config: waypoint_browser::rehearsal::RehearsalConfig,
            url: String,
        }
        let path = PathBuf::from(
            std::env::var_os("WAYPOINT_PROFILE_INSPECTION_PROBE").expect("private fixture"),
        )
        .canonicalize()
        .unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert!(path.starts_with(&root));
        let bytes = read(&path, 128 * 1024).unwrap();
        assert_eq!(
            waypoint_domain::sha256_hex(&bytes),
            std::env::var("WAYPOINT_PROFILE_INSPECTION_PROBE_SHA256").unwrap()
        );
        let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(fixture.config.schema, 2);
        assert!(fixture.config.replay_manifest.is_some() && fixture.config.attachments.is_none());
        assert!(fixture
            .config
            .content_root
            .canonicalize()
            .unwrap()
            .starts_with(root));
        let folder = path.parent().unwrap();
        assert!(!folder.join("inspection.private.json").exists());
        let (mut driver, ready) =
            waypoint_browser::CdpDriver::launch_rehearsal(&fixture.python, fixture.config).unwrap();
        assert!(ready.appcontainer && ready.capabilities == 0 && ready.entry_url == fixture.url);
        let result = (|| -> Result<Value, String> {
            let started = std::time::Instant::now();
            let form = driver.open(&fixture.url).map_err(|e| e.to_string())?;
            let mut observations = vec![
                json!({"phase":"open","elapsed_ms":started.elapsed().as_millis(),"schema":form.schema,"walls":form.walls,
                "barriers":form.independent_field_barriers()}),
            ];
            for _ in 0..3 {
                let started = std::time::Instant::now();
                let current = driver.inspect_current().map_err(|e| e.to_string())?;
                observations.push(json!({"phase":"inspect","elapsed_ms":started.elapsed().as_millis(),"schema":current.schema,"walls":current.walls,
                    "barriers":current.independent_field_barriers()}));
            }
            let trace: Vec<_> = driver
                .trace
                .iter()
                .filter(|entry| {
                    entry.starts_with("loading check:") || entry.starts_with("loading timing:")
                })
                .cloned()
                .collect();
            let report = driver
                .rehearsal_replay_report()
                .map_err(|e| e.to_string())?;
            assert_eq!(report["cached_non_read_replies"], 0);
            assert_eq!(report["unmanaged_child_sessions"], 0);
            Ok(
                json!({"scope":"native open/reinspection only; no field entry or local-model credit","observations":observations,"loading_trace":trace,"replay":report}),
            )
        })();
        let close = driver.execute(&BrowserCommand::Close);
        fs::write(
            folder.join("inspection.private.json"),
            serde_json::to_vec_pretty(&json!({"result":result,"browser_closed":close.is_ok()}))
                .unwrap(),
        )
        .unwrap();
        assert!(close.is_ok());
        assert!(result.is_ok(), "native failure retained");
    }

    #[test]
    fn replay_activity_preserves_unknowns_and_does_not_claim_causality() {
        let before = json!({"schema":1,"mode":"offline_response_replay","served":10,"blocked":3,
            "cached_non_read_replies":0,"unreplayed_transports":0,"unmanaged_child_sessions":0});
        let mut after = before.clone();
        after["served"] = json!(12);
        after["blocked"] = json!(4);
        let activity = observed_replay_activity(Some(&before), Some(&after));
        assert_eq!(activity["status"], "observed");
        assert_eq!(activity["scope"], "entry_and_review_interval");
        assert_eq!(activity["delta"]["blocked"], 1);
        assert_eq!(activity["delta"]["served"], 2);
        assert!(activity.get("entry_verified").is_none());
        for key in [
            "served",
            "blocked",
            "cached_non_read_replies",
            "unreplayed_transports",
            "unmanaged_child_sessions",
        ] {
            for value in [Value::Null, json!(-1), json!(1.5), json!("0")] {
                let mut invalid = after.clone();
                invalid[key] = value;
                assert_eq!(
                    observed_replay_activity(Some(&before), Some(&invalid))["status"],
                    "unavailable"
                );
            }
        }
        let mut regressed = before.clone();
        regressed["served"] = json!(9);
        assert_eq!(
            observed_replay_activity(Some(&before), Some(&regressed))["status"],
            "unavailable"
        );
        for key in ["schema", "mode"] {
            let mut invalid = before.clone();
            invalid[key] = Value::Null;
            assert_eq!(
                observed_replay_activity(Some(&invalid), Some(&after))["status"],
                "unavailable"
            );
        }
        assert_eq!(
            observed_replay_activity(None, Some(&after))["status"],
            "unavailable"
        );
        assert_eq!(
            observed_replay_activity(Some(&before), None)["status"],
            "unavailable"
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "native actuator probe of an explicitly bound private offline archive; no model-quality claim"]
    fn recorded_upload_widget_probe() {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::BrowserCommand;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fixture {
            python: PathBuf,
            config: waypoint_browser::rehearsal::RehearsalConfig,
            url: String,
            selector: String,
            file_name: String,
            #[serde(default)]
            check_review_recovery: bool,
        }
        let path =
            PathBuf::from(std::env::var_os("WAYPOINT_UPLOAD_PROBE").expect("private fixture"));
        let path = path.canonicalize().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert!(path.starts_with(&root));
        let bytes = read(&path, 128 * 1024).unwrap();
        assert_eq!(
            waypoint_domain::sha256_hex(&bytes),
            std::env::var("WAYPOINT_UPLOAD_PROBE_SHA256").unwrap()
        );
        let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(fixture.config.schema, 2);
        assert!(fixture.config.replay_manifest.is_some() && fixture.config.attachments.is_some());
        let folder = path.parent().unwrap();
        assert!(fixture
            .config
            .content_root
            .canonicalize()
            .unwrap()
            .starts_with(&root));
        assert!(fixture
            .config
            .attachments
            .as_ref()
            .unwrap()
            .root
            .canonicalize()
            .unwrap()
            .starts_with(folder));
        for name in [
            "widget-probe.private.json",
            "widget-0.png",
            "widget-1.png",
            "widget-2.png",
        ] {
            assert!(
                !folder.join(name).exists(),
                "preserve earlier probe observations"
            );
        }
        let (mut driver, ready) =
            waypoint_browser::CdpDriver::launch_rehearsal(&fixture.python, fixture.config).unwrap();
        assert!(ready.replay.is_some());
        assert!(ready.appcontainer);
        assert_eq!(ready.capabilities, 0);
        assert_eq!(ready.entry_url, fixture.url);
        let result = (|| -> Result<Value, String> {
            let form = driver.open(&fixture.url).map_err(|e| e.to_string())?;
            let initial = driver
                .rehearsal_replay_report()
                .map_err(|e| e.to_string())?;
            let mut capture = if fixture.check_review_recovery {
                driver.rehearse_recorded_field_progressive(
                    &form,
                    &fixture.selector,
                    &fixture.file_name,
                    &[],
                    |_| Ok(()),
                )
            } else {
                driver.rehearse_recorded_field(&form, &fixture.selector, &fixture.file_name)
            }
            .map_err(|e| e.to_string())?;
            let mut observations = Vec::new();
            let started = std::time::Instant::now();
            for (index, delay) in [0, 300, 1700].iter().enumerate() {
                std::thread::sleep(std::time::Duration::from_millis(*delay));
                let verified = driver
                    .verify_recorded_field(&capture)
                    .map_err(|e| e.to_string());
                let image = driver
                    .screenshot_field_feedback(&fixture.selector)
                    .map_err(|e| e.to_string())?;
                let name = format!("widget-{index}.png");
                let png =
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &image)
                        .map_err(|_| "Invalid screenshot")?;
                fs::write(folder.join(&name), &png).map_err(|e| e.to_string())?;
                let replay = driver
                    .rehearsal_replay_report()
                    .map_err(|e| e.to_string())?;
                let activity = observed_replay_activity(Some(&initial), Some(&replay));
                assert_eq!(activity["status"], "observed");
                observations.push(json!({"elapsed_ms":started.elapsed().as_millis(),"requested_delay_ms":delay,"native_verified":verified.is_ok(),"error":verified.err(),"image":name,"sha256":waypoint_domain::sha256_hex(&png),"replay":replay,"replay_activity":activity}));
            }
            let mut report = json!({"scope":"native file-widget diagnostics only; no local-model or whole-workflow credit","appcontainer":ready.appcontainer,"capabilities":ready.capabilities,"initial_replay":initial,"observations":observations});
            if fixture.check_review_recovery {
                recover_field_observation(&mut driver, &mut capture, Some(&initial), || Ok(()))?;
                driver
                    .verify_recorded_field_observation(&capture)
                    .map_err(|e| e.to_string())?;
                let refreshed = ScreenshotArchive::new(folder).save(capture.image())?;
                // Even the same URL and schema can be a new document. Use a
                // fresh replay baseline so that document/value validation
                // independently refuses the old capture after reopening.
                driver.open(&fixture.url).map_err(|e| e.to_string())?;
                let after_reopen = driver
                    .rehearsal_replay_report()
                    .map_err(|e| e.to_string())?;
                verify_recovery_replay(&mut driver, Some(&after_reopen))?;
                let refusal = recover_field_observation(
                    &mut driver,
                    &mut capture,
                    Some(&after_reopen),
                    || Ok(()),
                )
                .expect_err("A reopened document cannot reuse a previous entry");
                report["review_recovery"] = json!({
                    "refreshed_attachment_verified":true,
                    "image":refreshed.0,
                    "sha256":refreshed.1,
                    "reopened_document_refused":refusal,
                    "replay":driver.rehearsal_replay_report().map_err(|e| e.to_string())?,
                });
            }
            Ok(report)
        })();
        let close = driver.execute(&BrowserCommand::Close);
        let report = json!({"result":result,"browser_closed":close.is_ok()});
        fs::write(
            folder.join("widget-probe.private.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        assert!(close.is_ok());
        assert!(result.is_ok(), "private probe output retained");
        for observation in report["result"]["Ok"]["observations"].as_array().unwrap() {
            assert_eq!(observation["replay"]["cached_non_read_replies"], 0);
            assert_eq!(observation["replay"]["unmanaged_child_sessions"], 0);
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "owned model and private fictional saved-copy fixture"]
    fn owned_runtime_reviews_saved_copy_rehearsal() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fixture {
            source: Source,
            python: PathBuf,
            config: waypoint_browser::rehearsal::RehearsalConfig,
            manifest_sha256: String,
            #[serde(default)]
            mixed_controls: bool,
            #[serde(default)]
            upload_controls: bool,
            #[serde(default)]
            section_controls: bool,
        }
        const URL: &str = "https://example.org/fictional-rehearsal";
        let (addr, model, output) = crate::source_vision_qualification::owned_vision_endpoint();
        let folder = output.parent().unwrap();
        let bytes = read(
            &folder.join("recorded-rehearsal-input.private.json"),
            128 * 1024,
        )
        .unwrap();
        assert_eq!(
            waypoint_domain::sha256_hex(&bytes),
            std::env::var("WAYPOINT_OWNED_REHEARSAL_SHA256").unwrap()
        );
        let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert!(fixture
            .source
            .directory
            .canonicalize()
            .unwrap()
            .starts_with(target));
        assert!(fixture.source.rehearse);
        fixture.source.validate(URL).unwrap();
        let manifest = fixture
            .source
            .directory
            .join("source-page/archive/replay.json");
        assert_eq!(
            waypoint_domain::sha256_hex(&read(&manifest, 1024 * 1024).unwrap()),
            fixture.manifest_sha256
        );
        let screenshots = folder.join("rehearsal-screenshots");
        fs::create_dir(&screenshots).unwrap();
        let load = serde_json::to_vec(&json!({"model":model,"prompt":"","stream":false,
            "keep_alive":"5m","options":{"num_ctx":4096}}))
        .unwrap();
        let (code, _) = waypoint_inference::ollama::http_request(
            addr,
            "POST",
            "/api/generate",
            Some(&load),
            120_000,
        )
        .unwrap();
        assert_eq!(code, 200);
        let identity = crate::resident_model_identity_at(addr)
            .unwrap()
            .expect("The private model must meet the product resident budget");
        assert_eq!(identity.name, model);
        // A visual refusal must occur before native entry or section clicking.
        // This separate replay is never used as model-quality evidence.
        {
            use waypoint_applications::{
                driver::{BrowserDriver, DriverError},
                form_mapping::BrowserCommand,
            };
            let mut refusal_config = fixture.config.clone();
            refusal_config.schema = 2;
            refusal_config.content_root = fixture.source.directory.join("source-page/archive");
            let inventory: Inventory = serde_json::from_slice(
                &read(
                    &fixture.source.directory.join("source-page/inventory.json"),
                    128 * 1024,
                )
                .unwrap(),
            )
            .unwrap();
            refusal_config.content_files = inventory.files;
            refusal_config.replay_manifest = Some("replay.json".into());
            let (mut browser, ready) =
                waypoint_browser::CdpDriver::launch_rehearsal(&fixture.python, refusal_config)
                    .unwrap();
            browser.open(&ready.entry_url).unwrap();
            let frame = browser
                .captured_frames()
                .unwrap()
                .frames
                .into_iter()
                .next()
                .unwrap();
            let form = browser.enter_captured_frame(&frame).unwrap();
            let first = form
                .schema
                .fields
                .iter()
                .find(|field| field.label == "First name")
                .unwrap();
            let mut asked = false;
            let refused = browser.rehearse_recorded_field_reviewed(
                &form,
                &first.selector,
                "Avery",
                &[],
                |target| {
                    asked = true;
                    assert_eq!(target.label(), "First name");
                    assert_eq!(target.kind(), "text");
                    Err(DriverError("Intentional visual refusal".into()))
                },
            );
            assert!(asked && refused.is_err());
            assert_eq!(
                browser.text_value_snapshot(&first.selector).unwrap().value,
                ""
            );
            if fixture.section_controls {
                let entered = browser
                    .rehearse_recorded_field(&form, &first.selector, "Avery")
                    .unwrap();
                let mut asked = false;
                let refused = browser.advance_recorded_section_reviewed(
                    &form,
                    &[],
                    &[&entered],
                    true,
                    |target| {
                        asked = true;
                        assert_eq!(target.kind(), "button");
                        Err(DriverError("Intentional visual refusal".into()))
                    },
                );
                assert!(asked && refused.is_err());
                assert_eq!(browser.inspect_current().unwrap(), form);
            }
            browser.leave_captured_frame().unwrap();
            browser.execute(&BrowserCommand::Close).unwrap();
        }
        let result = render_at(
            &fixture.source,
            URL,
            &fixture.python,
            fixture.config,
            &screenshots,
            addr,
            None,
        )
        .unwrap();
        fs::write(&output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        assert_eq!(result["manifest_sha256"], fixture.manifest_sha256);
        assert_eq!(
            result["rehearsal"]["vision_model"]["digest"],
            identity.digest
        );
        assert_eq!(result["rehearsal"]["halted"], false);
        assert_eq!(result["rehearsal"]["visual_gate"]["required"], true);
        let targets = result["rehearsal"]["visual_gate"]["targets"]
            .as_array()
            .unwrap();
        assert_eq!(
            targets.len(),
            if fixture.section_controls {
                5
            } else if fixture.mixed_controls || fixture.upload_controls {
                4
            } else {
                2
            }
        );
        assert!(targets
            .iter()
            .all(|target| target["status"] == "visually_matched"
                && target["sha256"].as_str().is_some_and(|s| s.len() == 64)));

        let fields = result["rehearsal"]["fields"].as_array().unwrap();
        let controls: Vec<_> = fields
            .iter()
            .map(|field| {
                (
                    field["label"].as_str().unwrap(),
                    field["kind"].as_str().unwrap(),
                )
            })
            .collect();
        let text = [("First name", "text"), ("Email", "text")];
        let mixed = [text[0], text[1], ("Country", "select"), ("City", "radio")];
        let uploads = [
            text[0],
            mixed[2],
            ("Resume", "file"),
            ("Cover letter", "file"),
        ];
        let staged = [text[0], text[1], mixed[2]];
        assert!(
            [
                fixture.mixed_controls,
                fixture.upload_controls,
                fixture.section_controls
            ]
            .into_iter()
            .filter(|value| *value)
            .count()
                <= 1
        );
        let expected: &[_] = if fixture.section_controls {
            &staged
        } else if fixture.upload_controls {
            &uploads
        } else if fixture.mixed_controls {
            &mixed
        } else {
            &text
        };
        assert_eq!(controls, expected);
        for (index, field) in fields.iter().enumerate() {
            let expected_view = if fixture.section_controls {
                index + 2
            } else {
                2
            };
            assert_eq!(field["view"], expected_view);
            assert_eq!(field["status"], "visually_matched", "{}", field["label"]);
            if field["kind"] == "file" {
                let (name, bytes) = match field["label"].as_str() {
                    Some("Resume") => (SAMPLE_RESUME, SAMPLE_PDF),
                    Some("Cover letter") => (SAMPLE_COVER, SAMPLE_COVER_PDF),
                    _ => panic!("Unexpected fixture upload control"),
                };
                assert_eq!(field["attachment"]["name"], name);
                assert_eq!(
                    field["attachment"]["sha256"],
                    waypoint_domain::sha256_hex(bytes)
                );
            }
        }
        if fixture.section_controls {
            let transitions = result["rehearsal"]["sections"].as_array().unwrap();
            assert_eq!(transitions.len(), 3);
            assert_eq!(transitions[0]["to"], 3);
            assert_eq!(transitions[1]["to"], 4);
            assert_eq!(transitions[2]["reason"], "no_forward_control");
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "explicit hashed fictional replay and owned headless browser"]
    fn recorded_terminal_view_is_saved_for_manual_review() {
        use waypoint_applications::{driver::BrowserDriver, form_mapping::BrowserCommand};
        use waypoint_browser::{
            rehearsal::RehearsalConfig, CdpDriver, RecordedSectionOutcome, RecordedSectionStop,
        };

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fixture {
            python: PathBuf,
            config: RehearsalConfig,
            sections: Vec<Vec<(String, String)>>,
            #[serde(default)]
            measure_first_section: bool,
            summary: Option<SummaryQualification>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SummaryQualification {
            expected_status: String,
            comparison_statuses: Option<Vec<String>>,
            test_visual_observation: Option<Value>,
            #[serde(default)]
            test_detail_observations: Vec<Value>,
            expected_detail_images: Option<Vec<usize>>,
            expected_attachment_statuses: Option<Vec<Option<String>>>,
            #[serde(default)]
            check_scroll_sentinel: bool,
            model: Option<crate::private_inference::Selection>,
            replacement_url: Option<String>,
        }
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        let input = PathBuf::from(std::env::var_os("WAYPOINT_REVIEW_VIEW_FIXTURE").unwrap())
            .canonicalize()
            .unwrap();
        assert!(input.starts_with(&target));
        let bytes = read(&input, 128 * 1024).unwrap();
        assert_eq!(
            waypoint_domain::sha256_hex(&bytes),
            std::env::var("WAYPOINT_REVIEW_VIEW_FIXTURE_SHA256").unwrap()
        );
        let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(fixture.config.schema, 2);
        assert!(fixture.config.replay_manifest.is_some());
        assert!(fixture
            .config
            .content_root
            .canonicalize()
            .unwrap()
            .starts_with(&target));
        let output = input.parent().unwrap().join("review-observation");
        fs::create_dir(&output).unwrap();
        let attachment_manifest = fixture
            .config
            .attachments
            .as_ref()
            .map(|files| files.files.clone())
            .unwrap_or_default();
        let (mut driver, ready) =
            CdpDriver::launch_rehearsal(&fixture.python, fixture.config).unwrap();
        driver.begin_recorded_call_timing().unwrap();
        let started = std::time::Instant::now();
        let mut entry_timings = Vec::new();
        let observed = (|| -> Result<Value, String> {
            let mut form = driver
                .open(&ready.entry_url)
                .map_err(|error| error.to_string())?;
            let mut prior = Vec::new();
            let mut verified_entries = 0;
            let mut prepared = Vec::new();
            let mut summary_attachments = std::collections::BTreeMap::new();
            for (index, expected) in fixture.sections.iter().enumerate() {
                assert_eq!(form.schema.fields.len(), expected.len());
                let mut entries = Vec::new();
                for (field, (label, value)) in form.schema.fields.iter().zip(expected) {
                    assert_eq!(&field.label, label);
                    let previous: Vec<_> = entries.iter().collect();
                    let entry_started = std::time::Instant::now();
                    let entry = driver.rehearse_recorded_field_progressive(
                        &form,
                        &field.selector,
                        value,
                        &previous,
                        |target| {
                            assert_eq!(target.label(), label);
                            Ok(())
                        },
                    );
                    entry_timings.push(json!({"section":index+1,"field":entries.len()+1,
                        "seconds":entry_started.elapsed().as_secs_f64(),"verified":entry.is_ok()}));
                    let entry = entry.map_err(|error| error.to_string())?;
                    assert_eq!(entry.observed_value(), value);
                    assert!(entry.continuation().is_none());
                    if fixture.summary.is_some() {
                        let key = waypoint_browser::SummaryKey::from_field(field);
                        let mut record = json!({"summary_key":key,"intended_value":value,
                            "status":"visually_matched","native_verified":true,"kind":entry.kind()});
                        if matches!(entry.kind(), waypoint_browser::RecordedFieldKind::File) {
                            let sha256 = attachment_manifest
                                .get(value)
                                .expect("fixture file manifest");
                            record["attachment"] = json!({"name":value,"sha256":sha256});
                            summary_attachments.insert(
                                prepared.len(),
                                driver
                                    .capture_recorded_attachment_witness(&entry, sha256)
                                    .map_err(|error| error.to_string()),
                            );
                        }
                        prepared.push(record);
                    }
                    entries.push(entry);
                    verified_entries += 1;
                }
                eprintln!(
                    "Verified all {} fields in fictional section {} after {:.1}s",
                    entries.len(),
                    index + 1,
                    started.elapsed().as_secs_f64(),
                );
                let fields: Vec<_> = entries.iter().collect();
                let outcome = driver
                    .advance_recorded_section_reviewed(&form, &prior, &fields, true, |target| {
                        assert_eq!(target.label(), "Continue");
                        assert_eq!(target.kind(), "button");
                        Ok(())
                    })
                    .map_err(|error| error.to_string())?;
                if index + 1 < fixture.sections.len() {
                    let RecordedSectionOutcome::Advanced { form: next, .. } = outcome else {
                        return Err(format!(
                            "The replay did not advance from section {}",
                            index + 1
                        ));
                    };
                    if fixture.measure_first_section {
                        return Ok(
                            json!({"scope":"first_section_only","entry_fields":verified_entries,"sections":1,"advanced":true}),
                        );
                    }
                    prior.push(form);
                    form = next;
                    continue;
                }
                let RecordedSectionOutcome::ReviewRequired {
                    reason: RecordedSectionStop::NoEditableFields,
                    form,
                    image,
                    summary,
                } = outcome
                else {
                    return Err("The terminal observation was unavailable".into());
                };
                let controls = driver.page_controls().map_err(|error| error.to_string())?;
                assert!(!controls.incomplete);
                assert_eq!(controls.controls.len(), 1);
                assert_eq!(controls.controls[0].label, "Submit application");
                let replay = driver
                    .rehearsal_replay_report()
                    .map_err(|error| error.to_string())?;
                for key in [
                    "blocked",
                    "cached_non_read_replies",
                    "unreplayed_transports",
                    "unmanaged_child_sessions",
                ] {
                    assert_eq!(replay[key], 0, "{key}");
                }
                assert_eq!(replay["unsupported_targets"], json!({}));
                let mut evidence = ScreenshotArchive::new(&output);
                let mut view = save_review_observation(
                    &mut evidence,
                    "Page",
                    &form,
                    &image,
                    false,
                    (1, index + 1, index + 1),
                )?;
                assert_eq!(view["field_count"], 0);
                assert_eq!(view["review_observation"], true);
                assert_eq!(view["final_review"], false);
                let path = output.join(view["image"].as_str().unwrap());
                assert_eq!(
                    waypoint_domain::sha256_hex(&fs::read(path).unwrap()),
                    view["sha256"]
                );
                let mut model_admission = Value::Null;
                let mut summary_scroll_events = Value::Null;
                if let Some(qualification) = &fixture.summary {
                    let summary = summary.ok_or("The native review summary was missing")?;
                    if let Some(replacement) = &qualification.replacement_url {
                        let replacement_form = driver
                            .open(replacement)
                            .map_err(|error| error.to_string())?;
                        assert_eq!(replacement_form.schema.fields, form.schema.fields);
                        assert_eq!(replacement_form.walls, form.walls);
                    }
                    let report = if let Some(selection) = &qualification.model {
                        assert!(qualification.test_visual_observation.is_none());
                        let _lease = crate::doc_suggest::lease()?;
                        let task = "terminal-summary-qualification";
                        let attempt = output.join(task);
                        let screenshots = attempt.join("screenshots");
                        fs::create_dir(&attempt).map_err(|error| error.to_string())?;
                        fs::create_dir(&screenshots).map_err(|error| error.to_string())?;
                        crate::private_inference::create_namespace(&attempt, task)?;
                        let mut session = crate::private_inference::Session::admit(
                            selection,
                            &fixture.python,
                            &screenshots,
                        )
                        .map_err(|error| match error {
                            crate::private_inference::AdmissionError::Unavailable(reason)
                            | crate::private_inference::AdmissionError::CleanupFailed(reason) => {
                                reason
                            }
                        })?;
                        assert_eq!(session.metadata["mode"], "private");
                        let mut vision = RehearsalVision::new(
                            session.addr,
                            selection.choice.as_ref(),
                            session.runtime.clone(),
                            true,
                            std::time::Instant::now() + std::time::Duration::from_secs(150),
                        );
                        let report = crate::recorded_summary::inspect(
                            &mut driver,
                            &form,
                            &image,
                            summary,
                            crate::recorded_summary::PreparedSummary {
                                entries: &prepared,
                                attachments: &summary_attachments,
                            },
                            |image| evidence.save(image),
                            |input| vision.read_summary(input),
                        );
                        let stopped = session.stop();
                        let drained = crate::private_inference::cleanup_namespace(&attempt, task);
                        model_admission = session.metadata.clone();
                        stopped?;
                        drained?;
                        report
                    } else {
                        let mut details = qualification.test_detail_observations.iter();
                        crate::recorded_summary::inspect(
                            &mut driver,
                            &form,
                            &image,
                            summary,
                            crate::recorded_summary::PreparedSummary {
                                entries: &prepared,
                                attachments: &summary_attachments,
                            },
                            |image| evidence.save(image),
                            |input| match input {
                                crate::recorded_summary::SummaryVisual::Viewport(_) => {
                                    qualification
                                        .test_visual_observation
                                        .clone()
                                        .ok_or("Visual test input missing".into())
                                }
                                crate::recorded_summary::SummaryVisual::Region(_)
                                | crate::recorded_summary::SummaryVisual::Heading(_)
                                | crate::recorded_summary::SummaryVisual::SeparateAnswer(_) => {
                                    details
                                        .next()
                                        .cloned()
                                        .ok_or("Detail test input missing".into())
                                }
                            },
                        )
                    };
                    view["final_review"] = json!(report["status"] == "answers_verified");
                    view["final_summary"] = report;
                    assert_eq!(
                        driver.page_controls().map_err(|error| error.to_string())?,
                        controls
                    );
                    if qualification.check_scroll_sentinel {
                        let events = driver
                            .read_attribute("#submit", "data-scroll-events")
                            .map_err(|error| error.to_string())?;
                        assert_eq!(events.as_deref(), Some("0"));
                        summary_scroll_events = json!(0);
                    }
                }
                return Ok(
                    json!({"entry_fields":verified_entries,"sections":index+1,"view":view,"replay":replay,
                        "delivery_control_unchanged":true,"summary_scroll_events":summary_scroll_events,
                        "model_admission":model_admission}),
                );
            }
            Err("The fictional fixture supplied no terminal section".into())
        })();
        let profile = json!({"scope":if fixture.measure_first_section{"first_section_only"}else{"full_fixture"},
            "entries":entry_timings,"elapsed_seconds":started.elapsed().as_secs_f64(),
            "calls":driver.take_recorded_call_timing()});
        let close = driver.execute(&BrowserCommand::Close);
        fs::write(
            output.join("timing.private.json"),
            serde_json::to_vec_pretty(&profile).unwrap(),
        )
        .unwrap();
        let mut saved = observed.unwrap_or_else(|error| panic!("{error}; close: {close:?}"));
        close.unwrap();
        saved["browser_closed"] = json!(true);
        fs::write(
            output.join("report.private.json"),
            serde_json::to_vec_pretty(&saved).unwrap(),
        )
        .unwrap();
        if let Some(summary) = fixture.summary {
            let report = &saved["view"]["final_summary"];
            assert_eq!(report["status"], summary.expected_status);
            if let Some(comparisons) = summary.comparison_statuses {
                let checks: Vec<_> = report["comparisons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|check| check["status"].as_str().unwrap())
                    .collect();
                assert_eq!(checks, comparisons);
            }
            if summary.replacement_url.is_some() {
                assert_eq!(report["native_revalidated"], false);
            }
            if let Some(expected) = summary.expected_attachment_statuses {
                let comparisons = report["comparisons"].as_array().unwrap();
                let statuses: Vec<_> = comparisons
                    .iter()
                    .map(|check| check["attachment"]["status"].as_str().map(str::to_owned))
                    .collect();
                assert_eq!(statuses, expected);
                let mut verified = 0;
                for (check, (_, filename)) in
                    comparisons.iter().zip(fixture.sections.iter().flatten())
                {
                    if check["attachment"]["status"] == "retained" {
                        verified += 1;
                        assert_eq!(check["attachment"]["sha256"], attachment_manifest[filename]);
                    }
                }
                assert_eq!(report["attachment_matched_count"], verified);
            }
            if let Some(expected) = summary.expected_detail_images {
                let details = report["detail_reads"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let recorded: Vec<_> = details
                    .iter()
                    .map(|detail| {
                        let images = detail["images"].as_array().unwrap();
                        for image in images {
                            let bytes =
                                fs::read(output.join(image["image"].as_str().unwrap())).unwrap();
                            assert_eq!(image["sha256"], waypoint_domain::sha256_hex(&bytes));
                        }
                        images.len()
                    })
                    .collect();
                assert_eq!(recorded, expected);
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn staged_document_roles_have_distinct_bytes_and_never_overwrite() {
        let folder = tempfile::tempdir().unwrap();
        let screenshots = folder.path().join("screenshots");
        fs::create_dir(&screenshots).unwrap();
        let documents = stage_sample_attachments(&screenshots).unwrap();
        assert_ne!(
            documents.files[SAMPLE_RESUME],
            documents.files[SAMPLE_COVER]
        );
        assert_eq!(
            fs::read(documents.root.join(SAMPLE_RESUME)).unwrap(),
            SAMPLE_PDF
        );
        assert_eq!(
            fs::read(documents.root.join(SAMPLE_COVER)).unwrap(),
            SAMPLE_COVER_PDF
        );
        assert!(sample_attachment_bytes("unknown.pdf").is_none());
        assert!(stage_sample_attachments(&screenshots).is_err());
        assert_eq!(
            fs::read(documents.root.join(SAMPLE_COVER)).unwrap(),
            SAMPLE_COVER_PDF
        );
    }

    #[cfg(windows)]
    #[test]
    fn rehearsal_without_a_qualified_visual_model_stops_before_entry() {
        let result = Rehearsal::new(
            "127.0.0.1:0".parse().unwrap(),
            None,
            None,
            true,
            false,
            None,
            (std::time::Instant::now(), std::time::Duration::ZERO),
        )
        .finish(0);
        assert_eq!(result["halted"], true);
        assert_eq!(result["visual_gate"]["required"], true);
        assert_eq!(result["visual_gate"]["available"], false);
        assert_eq!(result["visual_gate"]["targets"], json!([]));
        assert_eq!(result["fields"], json!([]));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "explicit hashed fictional replay and owned headless browser"]
    fn refused_model_keeps_a_read_only_saved_view() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fixture {
            source: Source,
            url: String,
            python: PathBuf,
            config: waypoint_browser::rehearsal::RehearsalConfig,
        }
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        let input = PathBuf::from(std::env::var_os("WAYPOINT_MODEL_REFUSAL_FIXTURE").unwrap())
            .canonicalize()
            .unwrap();
        assert!(input.starts_with(&target));
        let bytes = read(&input, 128 * 1024).unwrap();
        assert_eq!(
            waypoint_domain::sha256_hex(&bytes),
            std::env::var("WAYPOINT_MODEL_REFUSAL_FIXTURE_SHA256").unwrap()
        );
        let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
        assert!(fixture.source.rehearse);
        assert!(fixture
            .source
            .directory
            .canonicalize()
            .unwrap()
            .starts_with(&target));
        let folder = input.parent().unwrap();
        let screenshots = folder.join("model-refusal-screenshots");
        fs::create_dir(&screenshots).unwrap();
        let fallback = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        fallback.set_nonblocking(true).unwrap();
        let selected = crate::private_inference::Selection {
            choice: Some(crate::model_preferences::Choice {
                name: "invalid-fixture-selection".into(),
                digest: String::new(),
                size_bytes: 0,
                directory: None,
            }),
            cache_root: folder.join("unused-model-cache"),
        };
        let result = render_at(
            &fixture.source,
            &fixture.url,
            &fixture.python,
            fixture.config,
            &screenshots,
            fallback.local_addr().unwrap(),
            Some(&selected),
        )
        .unwrap();
        assert_eq!(
            result["model_admission"],
            json!({"mode":"unavailable","reason":"Saved model settings are invalid"})
        );
        assert_eq!(result["rehearsal"]["halted"], true);
        assert_eq!(result["rehearsal"]["fields"], json!([]));
        assert_eq!(result["rehearsal"]["visual_gate"]["targets"], json!([]));
        assert_eq!(result["rehearsal"]["visual_gate"]["available"], false);
        assert_eq!(
            fallback.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        let view = &result["views"][0];
        assert!(view["field_count"].as_u64().unwrap() > 0);
        let png = read(
            &screenshots.join(view["image"].as_str().unwrap()),
            6 * 1024 * 1024,
        )
        .unwrap();
        assert_eq!(view["sha256"], waypoint_domain::sha256_hex(&png));
        assert_eq!(result["replay"]["blocked"], 0);
        assert_eq!(result["replay"]["cached_non_read_replies"], 0);
        fs::write(
            folder.join("model-refusal.private.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "loads one selected local vision model into an owned private runtime; fictional archive only"]
    fn automatic_private_model_rehearsal() {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fixture {
            source: Source,
            python: PathBuf,
            config: waypoint_browser::rehearsal::RehearsalConfig,
            manifest_sha256: String,
            url: String,
            selection: crate::private_inference::Selection,
            #[serde(default)]
            simulate_initial_review_pressure: bool,
            #[serde(default)]
            simulate_review_pressure_entries: Vec<usize>,
            journey: Option<Journey>,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Journey {
            sections: Vec<Vec<(String, String)>>,
            stops_at_repeated_section: bool,
        }
        let path = PathBuf::from(
            std::env::var_os("WAYPOINT_AUTO_MODEL_FIXTURE")
                .expect("Hash-bound private fixture required"),
        );
        let target = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert!(path.canonicalize().unwrap().starts_with(&target));
        let bytes = read(&path, 128 * 1024).unwrap();
        assert_eq!(
            waypoint_domain::sha256_hex(&bytes),
            std::env::var("WAYPOINT_AUTO_MODEL_FIXTURE_SHA256").unwrap()
        );
        let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
        assert!(fixture.source.rehearse);
        let folder = path.parent().unwrap();
        let attempt = folder.join("automatic-rehearsal");
        fs::create_dir(&attempt).unwrap();
        let screenshots = attempt.join("screenshots");
        fs::create_dir(&screenshots).unwrap();
        let lease = crate::doc_suggest::lease().unwrap();
        crate::private_inference::create_namespace(&attempt, "automatic-rehearsal").unwrap();
        let existing = crate::resident_model_status().unwrap();
        assert_eq!(existing["state"], "no_model");
        let mut simulated: std::collections::BTreeSet<_> = fixture
            .simulate_review_pressure_entries
            .iter()
            .copied()
            .collect();
        assert_eq!(
            simulated.len(),
            fixture.simulate_review_pressure_entries.len()
        );
        assert!(!simulated.contains(&0));
        if fixture.simulate_initial_review_pressure {
            simulated.insert(1);
        }
        TEST_FIELD_MEMORY_REFUSALS.with(|entries| *entries.borrow_mut() = simulated.clone());
        let result = render(
            &fixture.source,
            &fixture.url,
            &fixture.python,
            fixture.config,
            &screenshots,
            Some(&fixture.selection),
        );
        let cleanup = crate::private_inference::cleanup_namespace(&attempt, "automatic-rehearsal");
        if let Ok(value) = &result {
            fs::write(
                folder.join("automatic-rehearsal.private.json"),
                serde_json::to_vec_pretty(value).unwrap(),
            )
            .unwrap();
        }
        cleanup.unwrap();
        let result = result.unwrap();
        let admission = &result["model_admission"];
        assert_eq!(admission["mode"], "private");
        assert_eq!(admission["session_closed"], true);
        assert_eq!(
            admission["model"],
            fixture.selection.choice.as_ref().unwrap().name
        );
        assert_eq!(
            admission["digest"],
            fixture.selection.choice.as_ref().unwrap().digest
        );
        let pid = admission["pid"].as_u64().unwrap() as u32;
        let legacy_presence =
            processkit::process_is_alive(pid, admission["creation_token"].as_u64()).unwrap();
        fs::write(folder.join("process-state.private.json"),serde_json::to_vec_pretty(&json!({"identity_metadata_visible":legacy_presence,"native_exit":admission["process_exit"]})).unwrap()).unwrap();
        assert_eq!(admission["process_exit"]["exited"], true);
        assert_eq!(admission["endpoint_after_stop"]["owners"], json!([]));
        assert_eq!(admission["connect_after_stop"]["connected"], false);
        assert_eq!(result["manifest_sha256"], fixture.manifest_sha256);
        let fields = result["rehearsal"]["fields"].as_array().unwrap();
        let targets = result["rehearsal"]["visual_gate"]["targets"]
            .as_array()
            .unwrap();
        if let Some(journey) = &fixture.journey {
            assert!(!journey.sections.is_empty());
            let expected: Vec<_> = journey.sections.iter().flatten().collect();
            assert_eq!(fields.len(), expected.len());
            assert_eq!(
                result["rehearsal"]["halted"],
                journey.stops_at_repeated_section
            );
            let sections = result["rehearsal"]["sections"].as_array().unwrap();
            assert_eq!(sections.len(), journey.sections.len());
            assert!(sections[..sections.len() - 1]
                .iter()
                .all(|section| section["status"] == "advanced"));
            let last_reason = if journey.stops_at_repeated_section {
                "no_new_section"
            } else {
                "no_forward_control"
            };
            assert_eq!(sections.last().unwrap()["reason"], last_reason);
            assert_eq!(
                targets.len(),
                fields.len() + journey.sections.len()
                    - usize::from(!journey.stops_at_repeated_section)
            );
            let checkpoints = result["rehearsal"]["progress_checkpoints"]
                .as_array()
                .unwrap();
            assert_eq!(checkpoints.len(), fields.len());
            let mut image_names = std::collections::HashSet::new();
            for (index, ((field, (label, value)), checkpoint)) in
                fields.iter().zip(expected).zip(checkpoints).enumerate()
            {
                assert_eq!(field["label"], *label);
                assert_eq!(field["native_value"], *value);
                assert_eq!(field["native_verified"], true);
                let name = field["image"].as_str().unwrap();
                assert!(image_names.insert(name));
                let image = read(&screenshots.join(name), 6 * 1024 * 1024).unwrap();
                assert_eq!(field["sha256"], waypoint_domain::sha256_hex(&image));
                let saved = read(
                    &attempt.join(checkpoint["file"].as_str().unwrap()),
                    512 * 1024,
                )
                .unwrap();
                assert_eq!(checkpoint["sha256"], waypoint_domain::sha256_hex(&saved));
                let saved: Value = serde_json::from_slice(&saved).unwrap();
                assert_eq!(saved["schema"], 2);
                assert_eq!(saved["entry_index"], index + 1);
                assert_eq!(saved["fields"].as_array().unwrap().len(), 1);
                assert_eq!(saved["fields"][0]["native_value"], *value);
            }
        } else {
            assert_eq!(result["rehearsal"]["halted"], false);
            assert_eq!(
                fields
                    .iter()
                    .map(|v| v["label"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["First name", "Email", "Country"]
            );
            assert_eq!(targets.len(), 5);
        }
        assert!(fields.iter().all(|v| v["status"] == "visually_matched"));
        if !simulated.is_empty() {
            assert!(TEST_FIELD_MEMORY_REFUSALS.with(|entries| entries.borrow().is_empty()));
            for (index, field) in fields.iter().enumerate() {
                if !simulated.contains(&(index + 1)) {
                    assert!(field.get("visual_recovery").is_none());
                    continue;
                }
                assert_eq!(field["visual_recovery"]["status"], "verified");
                assert_eq!(field["native_verified"], true);
                let note = &field["visual_recovery"]["initial_archive"];
                let original =
                    read(&attempt.join(note["file"].as_str().unwrap()), 512 * 1024).unwrap();
                assert_eq!(note["sha256"], waypoint_domain::sha256_hex(&original));
                let original: Value = serde_json::from_slice(&original).unwrap();
                assert_eq!(original["reason"], "memory_pressure");
                assert_eq!(original["field"]["status"], "visual_unavailable");
                assert_eq!(original["field"]["control"], field["control"]);
                assert_ne!(original["field"]["image"], field["image"]);
                for observed in [&original["field"], field] {
                    let image = read(
                        &screenshots.join(observed["image"].as_str().unwrap()),
                        6 * 1024 * 1024,
                    )
                    .unwrap();
                    assert_eq!(observed["sha256"], waypoint_domain::sha256_hex(&image));
                }
            }
            assert_eq!(result["replay"]["cached_non_read_replies"], 0);
            assert_eq!(result["replay"]["unmanaged_child_sessions"], 0);
        }
        for counter in [
            "blocked",
            "cached_non_read_replies",
            "unreplayed_transports",
            "unmanaged_child_sessions",
        ] {
            assert_eq!(result["replay"][counter], 0, "{counter}");
        }
        assert_eq!(result["replay"]["unsupported_targets"], json!({}));
        assert!(targets.iter().all(|v| v["status"] == "visually_matched"));
        assert_eq!(crate::resident_model_status().unwrap(), existing);
        assert!(!attempt.join("model-runtime").exists());
        assert!(!fs::read_dir(&attempt).unwrap().any(|v| v
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("local-model-runtime-")));
        drop(lease);
    }

    #[test]
    fn preview_requires_the_current_job_and_completed_archive() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("saved-copy");
        fs::create_dir(&path).unwrap();
        let source = Source {
            directory: path,
            attempt_id: "copy".into(),
            job_id: "fixture-job".into(),
            rehearse: false,
            public_research: false,
            candidate_snapshot_sha256: None,
        };
        let url = "https://example.org/fictional";
        let summary_path = source.directory.join("source-page-summary.json");
        let mut summary = json!({"status":"complete","result":{"archive_written":true},"task_id":"copy","attempt_id":"copy","job_id":"fixture-job","source_url_sha256":waypoint_domain::sha256_hex(url.as_bytes())});
        let write =
            |value: &Value| fs::write(&summary_path, serde_json::to_vec(value).unwrap()).unwrap();
        write(&summary);
        assert!(source.validate(url).is_ok());
        for (rehearse, digest, valid) in [
            (false, "a".repeat(64), false),
            (true, "a".repeat(64), true),
            (true, "../candidate.json".into(), false),
            (true, "z".repeat(64), false),
        ] {
            let mut selected: Source =
                serde_json::from_value(serde_json::to_value(&source).unwrap()).unwrap();
            selected.rehearse = rehearse;
            selected.candidate_snapshot_sha256 = Some(digest);
            assert_eq!(selected.validate(url).is_ok(), valid);
        }
        assert!(source.validate("https://example.org/different").is_err());
        summary["job_id"] = json!("other-job");
        write(&summary);
        assert!(source.validate(url).is_err());
        summary["job_id"] = json!("fixture-job");
        summary["result"]["archive_written"] = json!(false);
        write(&summary);
        assert!(source.validate(url).is_err());
    }
}
