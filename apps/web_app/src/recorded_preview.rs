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

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn directory(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Saved page directory is unavailable")?;
    if !meta.is_dir() || linked(&meta) {
        return Err("Saved page directory is invalid".into());
    }
    Ok(())
}

fn read(path: &Path, max: usize) -> Result<Vec<u8>, String> {
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
            256 * 1024,
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
    } else {
        None
    };
    let mut model_session = if source.rehearse {
        selection
            .map(|selected| crate::private_inference::Session::admit(selected, python, screenshots))
            .transpose()?
    } else {
        None
    };
    let vision_addr = model_session
        .as_ref()
        .map(|session| session.addr)
        .unwrap_or(vision_addr);
    let runtime_guard = model_session
        .as_ref()
        .and_then(|session| session.guard.clone());
    let (mut driver, ready) =
        CdpDriver::launch_rehearsal(python, config).map_err(|error| error.to_string())?;
    let mut rehearsal = source.rehearse.then(|| {
        Rehearsal::new(
            vision_addr,
            selection.map(|value| &value.choice),
            runtime_guard,
            source.public_research,
            candidate,
        )
    });
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
            screenshots,
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
                                screenshots,
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
    if let Some(rehearsal) = rehearsal {
        let record = rehearsal.finish();
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
const REHEARSAL_LIMIT: usize = 8;
const VIEW_FIELD_LIMIT: usize = 40;
#[cfg(windows)]
#[cfg(windows)]
const SECTION_ADVANCE_LIMIT: usize = 2;
#[cfg(windows)]
const DIALOG_ACTION_LIMIT: usize = 2;
#[cfg(windows)]
const CONDITIONAL_REPLAN_LIMIT: usize = 4;

#[cfg(windows)]
fn record_scope(
    driver: &mut waypoint_browser::CdpDriver,
    directory: &Path,
    views: &mut Vec<Value>,
    label: &str,
    mut form: waypoint_applications::driver::LiveForm,
    embedded: bool,
    mut rehearsal: Option<&mut Rehearsal>,
) -> Result<(), String> {
    use waypoint_browser::{RecordedSectionOutcome, RecordedSectionStop};
    let first_view = views.len() + 1;
    let mut prior = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    let mut pending_dialog: Option<usize> = None;
    let mut section_capture_start = rehearsal.as_deref().map_or(0, |value| value.captures.len());
    loop {
        let view = views.len() + 1;
        let capture = capture_view(driver, directory, view, label, form.clone(), embedded);
        let mut capture = match capture {
            Ok(capture) => capture,
            Err(error) if pending.is_none() && pending_dialog.is_none() => return Err(error),
            Err(_) => {
                if pending_dialog.take().is_some() {
                    if let Some(rehearsal) = rehearsal.as_deref_mut() {
                        if let Some(record) = rehearsal.dialogs.last_mut() {
                            record["followup_capture"] = json!("unavailable");
                        }
                        rehearsal.halted = true;
                    }
                    return Ok(());
                }

                if let (Some(rehearsal), Some((from, _))) = (rehearsal.as_deref_mut(), pending) {
                    rehearsal.sections.push(
                        json!({"view":from,"status":"stopped","reason":"section_capture_failed"}),
                    );
                    rehearsal.halted = true;
                }
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
            capture["section"] = json!(prior.len() + 1);
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
        if form.independent_field_entry_blocked() || form.schema.fields.is_empty() {
            if let Some(next) = rehearsal.resolve_dialog(driver, directory, view, &form) {
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
        let continuation =
            rehearsal.exercise(driver, directory, view, &form, section_capture_start);
        if rehearsal.halted {
            return Ok(());
        }
        if let Some(next) = continuation {
            if rehearsal.conditionals >= CONDITIONAL_REPLAN_LIMIT {
                rehearsal.sections.push(json!({"view":view,"status":"stopped","reason":"conditional_replan_limit","unplanned_fields":next.schema.fields.len()}));
                rehearsal.halted = true;
                return Ok(());
            }
            rehearsal.conditionals += 1;
            rehearsal.sections.push(json!({"view":view,"status":"conditional_reinspection","next_view":views.len()+1,
                "previous_fields":form.schema.fields.len(),"inspected_fields":next.schema.fields.len(),
                "schema_sha256":waypoint_domain::sha256_hex(next.schema.schema_hash_input().as_bytes())}));
            form = next;
            continue;
        }
        let verified: Vec<_> = rehearsal.captures[section_capture_start..]
            .iter()
            .map(|(_, capture, _)| capture)
            .collect();
        let vision = &mut rehearsal.vision;
        let advance = driver.advance_recorded_section_reviewed(
            &form,
            &prior,
            &verified,
            rehearsal.advances < SECTION_ADVANCE_LIMIT && rehearsal.fields.len() < REHEARSAL_LIMIT,
            |target| vision.review_target(directory, view, "before_section", target),
        );
        match advance {
            Ok(RecordedSectionOutcome::Advanced {
                label: control,
                form: next,
            }) => {
                rehearsal.advances += 1;
                pending = Some((view, control));
                prior.push(form);
                form = next;
                section_capture_start = rehearsal.captures.len();
            }
            Ok(RecordedSectionOutcome::Stopped(reason)) => {
                if matches!(reason, RecordedSectionStop::FieldChanged) {
                    for (index, _, _) in rehearsal.captures.drain(section_capture_start..) {
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
    model: Option<crate::visual_review::VisionIdentityGuard>,
    identity: Value,
    targets: Vec<Value>,
}

#[cfg(windows)]
impl RehearsalVision {
    fn research_issue(
        &mut self,
        directory: &Path,
        profile: &Value,
        context: &str,
    ) -> Result<Value, String> {
        crate::check_inference_memory()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        crate::visual_review::research_recorded_issue(model, self.addr, directory, profile, context)
    }
    fn plan_profile(
        &mut self,
        form: &waypoint_applications::driver::LiveForm,
        profile: &Value,
    ) -> Result<crate::visual_review::RecordedFactPlan, String> {
        crate::check_inference_memory()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        Ok(crate::visual_review::propose_recorded_facts(
            model, self.addr, form, profile,
        ))
    }

    fn plan_attachments(
        &mut self,
        schema: &waypoint_applications::form_mapping::FormSchema,
    ) -> Result<crate::attachment_mapping::Plan, String> {
        use crate::attachment_mapping::DocumentOption;
        crate::check_inference_memory()?;
        let model = self.model.as_mut().ok_or("The local model needs review")?;
        crate::visual_review::propose_recorded_attachments(
            model,
            self.addr,
            schema,
            &[
                DocumentOption {
                    id: "resume",
                    description: "The fictional candidate's resume/CV PDF",
                },
                DocumentOption {
                    id: "cover_letter",
                    description: "The fictional candidate's cover letter PDF",
                },
            ],
        )
    }

    fn new(
        addr: std::net::SocketAddr,
        choice: Option<&crate::model_preferences::Choice>,
        guard: Option<crate::visual_review::RequestGuard>,
    ) -> Self {
        let identity = crate::check_inference_memory()
            .ok()
            .and_then(|()| crate::resident_model_identity_at(addr).ok().flatten())
            .filter(|identity| choice.is_none_or(|choice| choice.matches(identity)));
        let metadata = identity
            .as_ref()
            .map(|value| json!({"name":value.name,"digest":value.digest}));
        Self {
            addr,
            model: identity.map(|identity| {
                crate::visual_review::VisionIdentityGuard::from_identity(identity)
                    .with_request_guard(guard)
            }),
            identity: metadata.unwrap_or(Value::Null),
            targets: Vec::new(),
        }
    }

    fn review_target(
        &mut self,
        directory: &Path,
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
        if self.targets.len() >= REHEARSAL_LIMIT + SECTION_ADVANCE_LIMIT + DIALOG_ACTION_LIMIT {
            return Err(refused());
        }
        let mut record = json!({"view":view,"phase":phase,"label":bounded_text(target.label()),"kind":target.kind(),"status":"visual_unavailable"});
        if let Some(group) = target.file_group_label() {
            record["native_file_group_label"] = json!(group);
        }
        let saved = save_image(directory, 20 + self.targets.len(), target.image());
        if let Ok((image, digest)) = &saved {
            record["image"] = json!(image);
            record["sha256"] = json!(digest);
        }
        if let Some(error) = target.detail_error() {
            record["detail_capture_error"] = json!(bounded_text(error));
        }
        let detail_saved = match target.detail_image() {
            Some(image) => match save_image(directory, 60 + self.targets.len(), image) {
                Ok((image, digest)) => {
                    record["detail_image"] = json!(image);
                    record["detail_sha256"] = json!(digest);
                    true
                }
                Err(_) => {
                    record["detail_capture_error"] =
                        json!("The field detail image could not be saved");
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
            crate::check_inference_memory(),
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
    ) -> (&'static str, Option<Value>) {
        if crate::check_inference_memory().is_err() {
            return ("visual_unavailable", None);
        }
        let Some(model) = self.model.as_mut() else {
            return ("visual_unavailable", None);
        };
        let (outcome, transcription) =
            crate::visual_review::review_recorded_field(model, self.addr, capture);
        (recorded_vision_status(outcome), transcription)
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
struct Rehearsal {
    vision: RehearsalVision,
    fields: Vec<Value>,
    captures: Vec<(usize, waypoint_browser::RecordedFieldCapture, bool)>,
    halted: bool,
    sections: Vec<Value>,
    dialogs: Vec<Value>,
    advances: usize,
    conditionals: usize,
    profile: Result<Value, String>,
    candidate_metadata: Value,
    attachment_plans: Vec<Value>,
    fact_plans: Vec<Value>,
    public_research: bool,
    research_notes: Vec<Value>,
}

#[cfg(windows)]
impl Rehearsal {
    fn new(
        addr: std::net::SocketAddr,
        choice: Option<&crate::model_preferences::Choice>,
        guard: Option<crate::visual_review::RequestGuard>,
        public_research: bool,
        candidate: Option<(crate::rehearsal_candidate::Snapshot, String)>,
    ) -> Self {
        let vision = RehearsalVision::new(addr, choice, guard);
        let candidate_metadata = candidate.as_ref().map(|(snapshot,binding)|snapshot.metadata(binding)).unwrap_or_else(||json!({"scope":"bundled_fictional_contact_profile","profile_sha256":waypoint_domain::sha256_hex(include_bytes!("../data/rehearsal-profile.json")),"documents":"bundled_fictional_pdfs"}));
        let profile = match candidate {
            Some((snapshot, _)) => Ok(snapshot.contacts),
            None => serde_json::from_str(include_str!("../data/rehearsal-profile.json"))
                .map_err(|_| "The bundled fictional rehearsal profile is invalid".to_owned()),
        };
        let halted = vision.model.is_none() || profile.is_err();
        Self {
            vision,
            fields: Vec::new(),
            captures: Vec::new(),
            halted,
            sections: Vec::new(),
            dialogs: Vec::new(),
            advances: 0,
            conditionals: 0,
            profile,
            candidate_metadata,
            attachment_plans: Vec::new(),
            fact_plans: Vec::new(),
            public_research,
            research_notes: Vec::new(),
        }
    }

    fn exercise(
        &mut self,
        driver: &mut waypoint_browser::CdpDriver,
        directory: &Path,
        view: usize,
        form: &waypoint_applications::driver::LiveForm,
        carry_from: usize,
    ) -> Option<waypoint_applications::driver::LiveForm> {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::{FormFieldType, PacketValue};
        if self.halted
            || self.fields.len() >= REHEARSAL_LIMIT
            || form.independent_field_entry_blocked()
        {
            return None;
        }
        for (index, capture, _) in &self.captures[carry_from..] {
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
        let (mappings, review_selectors) = match plan {
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
        let mut attachments = std::collections::BTreeMap::new();
        if form
            .schema
            .fields
            .iter()
            .any(|field| field.field_type == FormFieldType::File)
        {
            let plan = if self.candidate_metadata["scope"] == "saved_candidate_contacts" {
                Err("Candidate documents have not been selected; demonstration files will not be attached".into())
            } else {
                self.vision.plan_attachments(&form.schema)
            };
            match plan {
                Ok(plan) => {
                    self.attachment_plans
                        .push(json!({"view":view,"status":"proposed","decisions":plan.decisions}));
                    for decision in plan.decisions {
                        let name = match decision.document_id.as_deref() {
                            Some("resume") => Some(SAMPLE_RESUME),
                            Some("cover_letter") => Some(SAMPLE_COVER),
                            _ => None,
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
        let field_limit = REHEARSAL_LIMIT;
        for field in &form.schema.fields {
            if self.captures[carry_from..]
                .iter()
                .any(|(_, capture, _)| capture.matches_field(field))
            {
                continue;
            }
            if self.fields.len() >= field_limit {
                break;
            }
            if field.sensitive
                || crate::suggestions::credential_autocomplete(field)
                || review_selectors.contains(&field.selector)
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
            if field.field_type != FormFieldType::File {
                result["candidate_source"] = json!({"scope":self.candidate_metadata["scope"],
                    "profile_sha256":profile_sha,"snapshot_sha256":self.candidate_metadata["snapshot_sha256"],"schema_sha256":schema_sha});
            }
            let mut target_refused = false;
            let mut witness = None;
            let verified: Vec<_> = self.captures[carry_from..]
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
                            .review_target(directory, view, "before_entry", target);
                    target_refused = reviewed.is_err();
                    reviewed
                },
            ) {
                Ok(capture) => {
                    if let Some(next) = capture.continuation() {
                        settled_form = next.clone();
                        continuation = Some(next.clone());
                    }
                    result["control"] = json!(capture.control());
                    result["kind"] = json!(capture.kind());
                    result["status"] = json!("visual_unavailable");
                    let mut image_saved = if let Ok((name, digest)) =
                        save_image(directory, 10 + index, capture.image())
                    {
                        result["image"] = json!(name);
                        result["sha256"] = json!(digest);
                        true
                    } else {
                        false
                    };
                    if let Some(error) = capture.detail_error() {
                        result["detail_capture_error"] = json!(bounded_text(error));
                    }
                    if let Some(image) = capture.detail_image() {
                        match save_image(directory, 80 + index, image) {
                            Ok((name, digest)) => {
                                result["detail_image"] = json!(name);
                                result["detail_sha256"] = json!(digest);
                            }
                            Err(_) => {
                                image_saved = false;
                                result["detail_capture_error"] =
                                    json!("The entered field detail image could not be saved");
                            }
                        }
                    }
                    if image_saved {
                        let (status, transcription) = self.vision.review_field(&capture);
                        result["status"] = json!(status);
                        if let Some(transcription) = transcription {
                            result["transcription"] = transcription;
                        }
                    }
                    if !image_saved || result["status"] != "visually_matched" {
                        self.halted = true;
                    }
                    if driver.verify_recorded_field(&capture).is_err() {
                        result["status"] = json!("changed_after_entry");
                        self.halted = true;
                    }
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
                            Some(observation) => {
                                match save_image(directory, 100 + index, observation.image()) {
                                    Ok((name, hash)) => {
                                        result["image"] = json!(name);
                                        result["sha256"] = json!(hash);
                                        json!({"status":"observed","scope":"current_same_document_and_schema",
                                        "control":observation.control(),"native_value":observation.value(),"entry_verified":false})
                                    }
                                    Err(_) => {
                                        json!({"status":"unavailable","reason":"image_not_retained","entry_verified":false})
                                    }
                                }
                            }
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
                        self.research_notes.push(match self.vision.research_issue(directory,profile,&context) {
                            Ok(value)=>value,Err(error)=>json!({"status":"needs_review","reason":bounded_text(&error),"form_actions_authorized":false}),
                        });
                    }
                }
            }
            let replay_after = driver.rehearsal_replay_report().ok();
            result["replay_activity"] =
                observed_replay_activity(replay_before.as_ref(), replay_after.as_ref());
            self.fields.push(result);
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
                        let Some(bytes) = sample_attachment_bytes(capture.value()) else {
                            self.fields[index]["status"] = json!("changed_after_entry");
                            self.fields[index]["native_verified"] = json!(false);
                            self.halted = true;
                            continue;
                        };
                        self.fields[index]["attachment"] = json!({
                            "name":capture.value(), "sha256":waypoint_domain::sha256_hex(bytes),
                        });
                    }
                    self.captures.push((index, capture, image_saved));
                } else {
                    self.fields[index]["status"] = json!("changed_after_entry");
                    self.fields[index]["native_verified"] = json!(false);
                    self.halted = true;
                }
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
        directory: &Path,
        view: usize,
        form: &waypoint_applications::driver::LiveForm,
    ) -> Option<waypoint_applications::driver::LiveForm> {
        use waypoint_browser::RecordedDialogOutcome;
        if !self.fields.is_empty() || self.dialogs.len() >= DIALOG_ACTION_LIMIT {
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
        let mut record = json!({"view":view,"status":"needs_review","scope":if self.candidate_metadata["scope"]=="saved_candidate_contacts"{"offline_saved_copy_only"}else{"fictional_saved_copy_only"},"description":dialog.description()});
        let outcome = (|| -> Result<Option<waypoint_applications::driver::LiveForm>, String> {
            crate::check_inference_memory()?;
            let model = self
                .vision
                .model
                .as_mut()
                .ok_or("The local model needs review")?;
            let plan = crate::visual_review::propose_recorded_dialog(
                model,
                self.vision.addr,
                &dialog.description(),
            )?;
            record["proposal"] = json!(plan);
            let Some(choice) = plan.control else {
                return Ok(None);
            };
            let result = driver
                .dismiss_recorded_dialog_reviewed(&dialog, &choice, |target| {
                    self.vision
                        .review_target(directory, view, "before_dialog", target)
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
                let (name, digest) = save_image(directory, 200 + self.dialogs.len(), &image)?;
                record["image"] = json!(name);
                record["sha256"] = json!(digest);
                if let Some(capture) = capture {
                    crate::check_inference_memory()?;
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

    fn finish(self) -> Value {
        json!({"scope":if self.candidate_metadata["scope"]=="saved_candidate_contacts"{"saved_candidate_contacts"}else{"fictional_saved_form"},"candidate_source":self.candidate_metadata,"profile_sha256":self.candidate_metadata["profile_sha256"],"profile_error":self.profile.as_ref().err(),"field_limit":REHEARSAL_LIMIT,"section_advance_limit":SECTION_ADVANCE_LIMIT,"halted":self.halted,"fields":self.fields,"fact_plans":self.fact_plans,"attachment_plans":self.attachment_plans,"sections":self.sections,"dialogs":self.dialogs,"dialog_action_limit":DIALOG_ACTION_LIMIT,"vision_model":self.vision.identity,
            "conditional_replans":self.conditionals,"conditional_replan_limit":CONDITIONAL_REPLAN_LIMIT,
            "public_research_enabled":self.public_research,"public_research":self.research_notes,
            "visual_gate":{"required":true,"available":self.vision.model.is_some(),"targets":self.vision.targets}})
    }
}

#[cfg(windows)]
fn capture_view(
    driver: &mut waypoint_browser::CdpDriver,
    directory: &Path,
    sequence: usize,
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
    let mut view = save_view(directory, sequence, label, form, image, embedded)?;
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
fn save_view(
    directory: &Path,
    sequence: usize,
    label: &str,
    form: waypoint_applications::driver::LiveForm,
    image: String,
    embedded: bool,
) -> Result<Value, String> {
    // Full-view captures have a separate range from field/target/diagnostic
    // evidence. Conditional replans can otherwise collide with field 0010.
    let image_sequence = sequence
        .checked_add(1000)
        .filter(|value| *value <= 9999)
        .ok_or("Saved page view limit exceeded")?;
    let (name, sha256) = save_image(directory, image_sequence, &image)?;
    let fields: Vec<_> = form
        .schema
        .fields
        .iter()
        .take(VIEW_FIELD_LIMIT)
        .map(|field| {
            use waypoint_applications::form_mapping::FormFieldType;
            let kind = match field.field_type {
                FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea => "text",
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
fn save_image(directory: &Path, sequence: usize, image: &str) -> Result<(String, String), String> {
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
    let name = format!("{sequence:04}.png");
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| "Cannot stage saved page image")?;
    file.write_all(&bytes)
        .and_then(|_| file.flush())
        .map_err(|_| "Cannot write saved page image")?;
    file.persist_noclobber(directory.join(&name))
        .map_err(|_| "Cannot preserve saved page image")?;
    Ok((name, waypoint_domain::sha256_hex(&bytes)))
}

#[cfg(test)]
mod tests {
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
            let capture = driver
                .rehearse_recorded_field(&form, &fixture.selector, &fixture.file_name)
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
            Ok(
                json!({"scope":"native file-widget diagnostics only; no local-model or whole-workflow credit","appcontainer":ready.appcontainer,"capabilities":ready.capabilities,"initial_replay":initial,"observations":observations}),
            )
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
        let result =
            Rehearsal::new("127.0.0.1:0".parse().unwrap(), None, None, false, None).finish();
        assert_eq!(result["halted"], true);
        assert_eq!(result["visual_gate"]["required"], true);
        assert_eq!(result["visual_gate"]["available"], false);
        assert_eq!(result["visual_gate"]["targets"], json!([]));
        assert_eq!(result["fields"], json!([]));
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
        assert_eq!(admission["model"], fixture.selection.choice.name);
        assert_eq!(admission["digest"], fixture.selection.choice.digest);
        let pid = admission["pid"].as_u64().unwrap() as u32;
        let legacy_presence =
            processkit::process_is_alive(pid, admission["creation_token"].as_u64()).unwrap();
        fs::write(folder.join("process-state.private.json"),serde_json::to_vec_pretty(&json!({"identity_metadata_visible":legacy_presence,"native_exit":admission["process_exit"]})).unwrap()).unwrap();
        assert_eq!(admission["process_exit"]["exited"], true);
        assert_eq!(admission["endpoint_after_stop"]["owners"], json!([]));
        assert_eq!(admission["connect_after_stop"]["connected"], false);
        assert_eq!(result["manifest_sha256"], fixture.manifest_sha256);
        assert_eq!(result["rehearsal"]["halted"], false);
        let fields = result["rehearsal"]["fields"].as_array().unwrap();
        assert_eq!(
            fields
                .iter()
                .map(|v| v["label"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["First name", "Email", "Country"]
        );
        assert!(fields.iter().all(|v| v["status"] == "visually_matched"));
        let targets = result["rehearsal"]["visual_gate"]["targets"]
            .as_array()
            .unwrap();
        assert_eq!(targets.len(), 5);
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
