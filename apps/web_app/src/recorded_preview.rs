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
    mut config: waypoint_browser::rehearsal::RehearsalConfig,
    screenshots: &Path,
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
    let (mut driver, ready) =
        CdpDriver::launch_rehearsal(python, config).map_err(|error| error.to_string())?;
    let result = (|| {
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
        let mut views = vec![capture_view(
            &mut driver,
            screenshots,
            1,
            "Page",
            parent,
            false,
        )?];
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
                            let captured = capture_view(
                                &mut driver,
                                screenshots,
                                views.len() + 1,
                                frame.label(),
                                form,
                                true,
                            );
                            driver
                                .leave_captured_frame()
                                .map_err(|error| error.to_string())?;
                            captured
                        }
                        Err(error) => Err(error.to_string()),
                    };
                    match view {
                        Ok(view) => views.push(view),
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
    result
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
    save_view(directory, sequence, label, form, image)
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
) -> Result<Value, String> {
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
        json!({"kind":if sequence==1{"page"}else{"frame"},"label":bounded_text(label),"origin":bounded_text(&form.origin),
        "image":name,"sha256":waypoint_domain::sha256_hex(&bytes),"fields":fields,"field_count":form.schema.fields.len(),"walls":walls}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preview_requires_the_current_job_and_completed_archive() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("saved-copy");
        fs::create_dir(&path).unwrap();
        let source = Source {
            directory: path,
            attempt_id: "copy".into(),
            job_id: "fixture-job".into(),
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
