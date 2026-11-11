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
) -> Result<Value, String> {
    render_at(
        source,
        url,
        python,
        config,
        screenshots,
        std::net::SocketAddr::from(([127, 0, 0, 1], 11434)),
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
    if source.rehearse {
        config.attachments = Some(stage_sample_attachments(screenshots)?);
    }
    let (mut driver, ready) =
        CdpDriver::launch_rehearsal(python, config).map_err(|error| error.to_string())?;
    let mut rehearsal = source.rehearse.then(Rehearsal::default);
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
            "replay":{"served":report["served"],"blocked":report["blocked"],"unsupported_targets":report["unsupported_targets"]}}),
        )
    })();
    driver
        .execute(&BrowserCommand::Close)
        .map_err(|error| error.to_string())?;
    let mut result = result?;
    if let Some(rehearsal) = rehearsal {
        result["rehearsal"] = rehearsal.review(vision_addr);
    }
    Ok(result)
}

#[cfg(windows)]
const REHEARSAL_LIMIT: usize = 8;
#[cfg(windows)]
const FIELDS_PER_SECTION: usize = 4;
#[cfg(windows)]
const SECTION_ADVANCE_LIMIT: usize = 2;

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
    loop {
        let view = views.len() + 1;
        let capture = capture_view(driver, directory, view, label, form.clone(), embedded);
        let mut capture = match capture {
            Ok(capture) => capture,
            Err(error) if pending.is_none() => return Err(error),
            Err(_) => {
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
        if rehearsal.halted || form.schema.fields.is_empty() || !form.walls.is_empty() {
            return Ok(());
        }
        let first_capture = rehearsal.captures.len();
        rehearsal.exercise(driver, directory, view, &form);
        if rehearsal.halted {
            return Ok(());
        }
        let verified: Vec<_> = rehearsal.captures[first_capture..]
            .iter()
            .map(|(_, capture, _)| capture)
            .collect();
        let advance = driver.advance_recorded_section(
            &form,
            &prior,
            &verified,
            rehearsal.advances < SECTION_ADVANCE_LIMIT && rehearsal.fields.len() < REHEARSAL_LIMIT,
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
            }
            Ok(RecordedSectionOutcome::Stopped(reason)) => {
                if matches!(reason, RecordedSectionStop::FieldChanged) {
                    for (index, _, _) in rehearsal.captures.drain(first_capture..) {
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
const SAMPLE_RESUME: &str = "sample-resume.pdf";
#[cfg(windows)]
const SAMPLE_COVER: &str = "sample-cover-letter.pdf";

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
    let digest = waypoint_domain::sha256_hex(SAMPLE_PDF);
    let mut files = std::collections::BTreeMap::new();
    for name in [SAMPLE_RESUME, SAMPLE_COVER] {
        use std::io::Write;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(name))
            .and_then(|mut file| file.write_all(SAMPLE_PDF))
            .map_err(|_| "Cannot preserve sample attachment")?;
        files.insert(name.to_owned(), digest.clone());
    }
    Ok(waypoint_browser::rehearsal::RehearsalAttachments { root, files })
}

#[cfg(windows)]
fn sample_attachment(
    field: &waypoint_applications::form_mapping::FormField,
) -> Option<&'static str> {
    use waypoint_applications::form_mapping::FormFieldType;
    if field.sensitive || field.field_type != FormFieldType::File {
        return None;
    }
    match field
        .label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .as_str()
    {
        "resume" | "résumé" | "cv" | "curriculum vitae" => Some(SAMPLE_RESUME),
        "cover letter" => Some(SAMPLE_COVER),
        _ => None,
    }
}

#[cfg(windows)]
#[derive(Default)]
struct Rehearsal {
    fields: Vec<Value>,
    captures: Vec<(usize, waypoint_browser::RecordedFieldCapture, bool)>,
    halted: bool,
    sections: Vec<Value>,
    advances: usize,
}

#[cfg(windows)]
impl Rehearsal {
    fn exercise(
        &mut self,
        driver: &mut waypoint_browser::CdpDriver,
        directory: &Path,
        view: usize,
        form: &waypoint_applications::driver::LiveForm,
    ) {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::{FormFieldType, PacketValue};
        if self.halted || self.fields.len() >= REHEARSAL_LIMIT || !form.walls.is_empty() {
            return;
        }
        let profile = json!({
            "full_name":"Avery Example", "first_name":"Avery", "last_name":"Example",
            "email":"avery@example.invalid", "phone":"+12025550123",
            "city":"Example City", "country":"Canada",
        });
        let Ok(mappings) = crate::suggestions::suggest_exact(&form.schema, &profile) else {
            return;
        };
        let mut captures = Vec::new();
        let field_limit = (self.fields.len() + FIELDS_PER_SECTION).min(REHEARSAL_LIMIT);
        for field in &form.schema.fields {
            if self.fields.len() >= field_limit {
                break;
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
                let Some(name) = sample_attachment(field) else {
                    continue;
                };
                if form
                    .schema
                    .fields
                    .iter()
                    .filter(|other| sample_attachment(other) == Some(name))
                    .count()
                    != 1
                {
                    continue;
                }
                name
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
            let mut result = json!({"view":view,"label":bounded_text(&field.label),"status":"readback_unavailable"});
            match driver.rehearse_recorded_field(form, &field.selector, value) {
                Ok(capture) => {
                    result["kind"] = json!(capture.kind());
                    result["status"] = json!("visual_unavailable");
                    let image_saved = if let Ok((name, digest)) =
                        save_image(directory, 10 + index, capture.image())
                    {
                        result["image"] = json!(name);
                        result["sha256"] = json!(digest);
                        true
                    } else {
                        false
                    };
                    captures.push((index, capture, image_saved));
                }
                Err(_) => self.halted = true,
            }
            self.fields.push(result);
            if self.halted {
                break;
            }
        }
        if !captures.is_empty() {
            let unchanged = driver
                .inspect_current()
                .is_ok_and(|current| current == *form);
            for (index, capture, image_saved) in captures {
                if unchanged && driver.verify_recorded_field(&capture).is_ok() {
                    if matches!(capture.kind(), waypoint_browser::RecordedFieldKind::File) {
                        self.fields[index]["attachment"] = json!({
                            "name":capture.value(), "sha256":waypoint_domain::sha256_hex(SAMPLE_PDF),
                        });
                    }
                    self.captures.push((index, capture, image_saved));
                } else {
                    self.fields[index]["status"] = json!("changed_after_entry");
                    self.halted = true;
                }
            }
        }
    }

    fn review(mut self, addr: std::net::SocketAddr) -> Value {
        use crate::visual_review::RecordedVisionOutcome;
        let mut vision_model = None;
        if !self.captures.is_empty() && crate::check_inference_memory().is_ok() {
            if let Ok(Some(identity)) = crate::resident_model_identity_at(addr) {
                vision_model = Some(json!({"name":identity.name,"digest":identity.digest}));
                let mut model = crate::visual_review::VisionIdentityGuard::from_identity(identity);
                for (index, capture, _) in self
                    .captures
                    .iter()
                    .filter(|(_, _, image_saved)| *image_saved)
                {
                    if crate::check_inference_memory().is_err() {
                        break;
                    }
                    let status = match crate::visual_review::review_recorded_field(
                        &mut model, addr, capture,
                    ) {
                        RecordedVisionOutcome::Matched => "visually_matched",
                        RecordedVisionOutcome::Difference => "visual_difference",
                        RecordedVisionOutcome::Unobserved => "visual_unobserved",
                        RecordedVisionOutcome::Failed => "visual_review_failed",
                        RecordedVisionOutcome::Stopped => break,
                    };
                    self.fields[*index]["status"] = json!(status);
                }
            }
        }
        json!({"scope":"fictional_saved_form","field_limit":REHEARSAL_LIMIT,"section_advance_limit":SECTION_ADVANCE_LIMIT,"halted":self.halted,"fields":self.fields,"sections":self.sections,"vision_model":vision_model})
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
    save_view(directory, sequence, label, form, image, embedded)
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
    let (name, sha256) = save_image(directory, sequence, &image)?;
    let fields: Vec<_> = form
        .schema
        .fields
        .iter()
        .take(40)
        .map(|field| json!({"label":bounded_text(&field.label),"required":field.required}))
        .collect();
    let walls: Vec<_> = form
        .walls
        .iter()
        .take(8)
        .map(|wall| bounded_text(wall))
        .collect();
    Ok(
        json!({"kind":if embedded{"frame"}else{"page"},"label":bounded_text(label),"origin":bounded_text(&form.origin),
        "image":name,"sha256":sha256,"fields":fields,"field_count":form.schema.fields.len(),"walls":walls}),
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
        let result = render_at(
            &fixture.source,
            URL,
            &fixture.python,
            fixture.config,
            &screenshots,
            addr,
        )
        .unwrap();
        fs::write(&output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        assert_eq!(result["manifest_sha256"], fixture.manifest_sha256);
        assert_eq!(
            result["rehearsal"]["vision_model"]["digest"],
            identity.digest
        );
        assert_eq!(result["rehearsal"]["halted"], false);
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
                assert_eq!(
                    field["attachment"]["sha256"],
                    waypoint_domain::sha256_hex(SAMPLE_PDF)
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
        };
        let url = "https://example.org/fictional";
        let summary_path = source.directory.join("source-page-summary.json");
        let mut summary = json!({"status":"complete","result":{"archive_written":true},"task_id":"copy","attempt_id":"copy","job_id":"fixture-job","source_url_sha256":waypoint_domain::sha256_hex(url.as_bytes())});
        let write =
            |value: &Value| fs::write(&summary_path, serde_json::to_vec(value).unwrap()).unwrap();
        write(&summary);
        assert!(source.validate(url).is_ok());
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
