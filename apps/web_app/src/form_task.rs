//! One background form inspection; the server thread owns the resulting session.
use crate::{doc_suggest, form_preview};
use std::sync::{mpsc, Mutex};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::{JobIdentityId, Sha256Digest};
use waypoint_inference::OllamaInference;
use waypoint_workspace::form_experience::ExperienceMatch;

#[cfg(test)]
pub(crate) struct CrossOriginFixture {
    pub url: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

#[cfg(test)]
impl Drop for CrossOriginFixture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
pub(crate) fn cross_origin_form_fixture() -> CrossOriginFixture {
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let child = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let parent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let child_port = child.local_addr().unwrap().port();
    let parent_port = parent.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let mut threads = vec![];
    for (listener, body) in [(child, "<form><label>Name <input name='name'></label></form>".to_owned()), (parent, format!("<iframe title='Fictional form' src='http://127.0.0.1:{child_port}/'></iframe><a href='#apply'>Apply elsewhere</a>"))] {
        listener.set_nonblocking(true).unwrap();
        let stop = stop.clone();
        threads.push(std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
                        let mut request = [0u8; 2048]; let _ = stream.read(&mut request);
                        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(10)),
                    Err(_) => break,
                }
            }
        }));
    }
    CrossOriginFixture {
        url: format!("http://127.0.0.1:{parent_port}/"),
        stop,
        threads,
    }
}

pub(crate) fn has_upload_control(
    fields: &[waypoint_applications::form_mapping::FormField],
) -> bool {
    fields.iter().any(|field| {
        matches!(
            field.field_type,
            waypoint_applications::form_mapping::FormFieldType::File
        )
    })
}

/// Multiple required data-entry controls suggest a form may already be open.
/// This is only a navigation hint, not proof that the form is an application.
fn has_required_data_entry_form(fields: &[waypoint_applications::form_mapping::FormField]) -> bool {
    use waypoint_applications::form_mapping::FormFieldType;
    fields
        .iter()
        .filter(|field| {
            field.required
                && matches!(
                    field.field_type,
                    FormFieldType::Text
                        | FormFieldType::Email
                        | FormFieldType::TextArea
                        | FormFieldType::Select { .. }
                        | FormFieldType::MultiSelect { .. }
                        | FormFieldType::Date
                        | FormFieldType::Month
                        | FormFieldType::Time
                        | FormFieldType::DateTimeLocal
                )
        })
        .take(2)
        .count()
        >= 2
}

pub(crate) fn has_main_fields_and_embedded_content(
    live: &waypoint_applications::driver::LiveForm,
) -> bool {
    !live.schema.fields.is_empty()
        && live.walls.iter().any(|wall| {
            wall == waypoint_browser::UNINSPECTED_FRAME_REVIEW
                || wall == waypoint_browser::SHADOW_FORM_REVIEW
        })
}

/// Open the listing, then at most one unambiguous same-origin application link.
/// No field action or submit action is issued here.
fn open_application_page(
    driver: &mut dyn BrowserDriver,
    url: &str,
) -> Result<(waypoint_applications::driver::LiveForm, Option<String>), String> {
    let mut live = driver.open(url).map_err(|e| e.to_string())?;
    if live.walls.iter().any(|wall| {
        wall != waypoint_browser::UNINSPECTED_FRAME_REVIEW
            && wall != waypoint_browser::SHADOW_FORM_REVIEW
    }) {
        return Ok((live, None));
    }
    let current = driver.current_url().map_err(|e| e.to_string())?;
    let original = url::Url::parse(url).map_err(|_| "Invalid listing URL")?;
    let reached = url::Url::parse(&current).map_err(|_| "Invalid current URL")?;
    if original.scheme() == "https" && original.origin() != reached.origin() {
        live.walls
            .push("The listing redirected to another site; continue manually".into());
        return Ok((live, None));
    }
    // A file upload is evidence that this may already be an application form.
    // Keep the current page for user review rather than navigating away from it;
    // this is not proof that the page accepts applications.
    if has_upload_control(&live.schema.fields) {
        return Ok((live, None));
    }
    if has_required_data_entry_form(&live.schema.fields) {
        return Ok((live, Some("This page has required data-entry fields; review it before following another application link.".into())));
    }
    if has_main_fields_and_embedded_content(&live) {
        return Ok((live, Some("The current page has visible fields and embedded content; review it before navigating away.".into())));
    }
    let links = match driver.application_links() {
        Ok(links) => links,
        Err(_) => {
            return Ok((
                live,
                Some(
                    "Application links could not be inspected; review navigation manually.".into(),
                ),
            ))
        }
    };
    if let Some(destination) = crate::application_check::application_destination(&current, &links) {
        live = driver.open(&destination).map_err(|e| e.to_string())?;
        let final_url = driver.current_url().map_err(|e| e.to_string())?;
        let final_url = url::Url::parse(&final_url).map_err(|_| "Invalid application URL")?;
        if original.origin() != final_url.origin() {
            live.walls
                .push("The application link redirected to another site; continue manually".into());
        }
    } else if !links.is_empty() {
        return Ok((
            live,
            Some("Application links are ambiguous or require manual navigation".into()),
        ));
    }
    Ok((live, None))
}

pub struct Finished {
    pub job_id: JobIdentityId,
    pub pending_revision: i64,
    pub url: String,
    pub profile_digest: Sha256Digest,
    pub visible: bool,
    pub experience_revisions: Vec<(String, u64)>,
    pub result: Result<(waypoint_browser::CdpDriver, form_preview::FormPreview), String>,
}
struct Task {
    id: String,
    receiver: mpsc::Receiver<Finished>,
    job_id: JobIdentityId,
    pending_revision: i64,
    url: String,
    profile_digest: Sha256Digest,
    visible: bool,
    experience_revisions: Vec<(String, u64)>,
    failure: Option<String>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());
static COMPLETED_FAILURE: Mutex<Option<(String, String)>> = Mutex::new(None);

pub(crate) fn pending_reason(task_id: &str) -> String {
    format!("Application review in progress or interrupted: {task_id}")
}

pub fn begin_pending(
    ws: &waypoint_workspace::Workspace,
    job: &JobIdentityId,
    task_id: &str,
    timestamp: i64,
) -> Result<i64, String> {
    let reason = pending_reason(task_id);
    ws.mark_manual_review(job, &reason, timestamp)
        .map_err(|e| format!("Could not save pending inspection: {e}"))?;
    ws.manual_review_reasons(job)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|entry| entry.reason == reason)
        .map(|entry| entry.revision)
        .ok_or_else(|| "Pending inspection was not found after saving".into())
}

pub fn finish_pending(
    ws: &waypoint_workspace::Workspace,
    job: &JobIdentityId,
    task_id: &str,
    revision: i64,
) -> Result<bool, String> {
    ws.clear_manual_review_reason(job, &pending_reason(task_id), revision)
        .map_err(|e| format!("Could not clear pending inspection: {e}"))
}

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

pub fn pending() -> Result<Option<(String, String)>, String> {
    let task = TASK.lock().map_err(|_| "Form inspection unavailable")?;
    Ok(task
        .as_ref()
        .map(|task| (task.id.clone(), task.job_id.as_str().to_owned())))
}

/// Lessons describe classes of controls, not selectors or field values.
/// Only pass a class when a non-sensitive field of that class is present.
fn observed_platform(origin: &str) -> Option<waypoint_workspace::form_experience::Platform> {
    use waypoint_workspace::form_experience::Platform;
    let parsed = url::Url::parse(origin).ok()?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let host = parsed.host_str()?;
    match host {
        "boards.greenhouse.io" | "job-boards.greenhouse.io" => Some(Platform::Greenhouse),
        "jobs.lever.co" => Some(Platform::Lever),
        "jobs.ashbyhq.com" => Some(Platform::Ashby),
        "apply.workable.com" => Some(Platform::Workable),
        _ if host.ends_with(".myworkdayjobs.com") => Some(Platform::Workday),
        _ => None,
    }
}

/// Keep newly observed walls even if the first inspection missed them. A
/// different schema cannot safely reuse mappings made for the earlier page.
fn merge_final_inspection(
    preview: &mut form_preview::FormPreview,
    fresh: &waypoint_applications::driver::LiveForm,
) -> Result<(), String> {
    if fresh.origin != preview.schema.origin || fresh.schema != preview.schema {
        return Err("The application changed while preparing its review; inspect it again".into());
    }
    for wall in &fresh.walls {
        if !preview.manual_review.contains(wall) {
            preview.manual_review.push(wall.clone());
        }
    }
    Ok(())
}

pub(crate) fn relevant_experiences(
    schema: &waypoint_applications::form_mapping::FormSchema,
    experiences: &[ExperienceMatch],
) -> Vec<ExperienceMatch> {
    use waypoint_applications::form_mapping::FormFieldType as Field;
    use waypoint_workspace::form_experience::{Control, Stage};
    let has_text = schema.fields.iter().any(|field| {
        !field.sensitive
            && matches!(
                field.field_type,
                Field::Text
                    | Field::Email
                    | Field::TextArea
                    | Field::Date
                    | Field::Month
                    | Field::Time
                    | Field::DateTimeLocal
            )
    });
    let has_choice = schema.fields.iter().any(|field| {
        !field.sensitive
            && matches!(
                field.field_type,
                Field::Select { .. }
                    | Field::MultiSelect { .. }
                    | Field::Checkbox
                    | Field::Radio { .. }
            )
    });
    let platform = observed_platform(&schema.origin);
    let mut selected = Vec::new();
    for (control, present) in [(Control::Text, has_text), (Control::Choice, has_choice)] {
        if !present {
            continue;
        }
        let mut candidates: Vec<_> = experiences
            .iter()
            .filter(|item| {
                item.experience.enabled
                    && item.experience.reviewed
                    && item.experience.stage == Stage::EnterInformation
                    && item.experience.control == control
            })
            .cloned()
            .collect();
        for item in &mut candidates {
            item.same_platform = platform.is_some() && item.experience.platform == platform;
        }
        candidates.sort_by(|a, b| {
            b.same_platform
                .cmp(&a.same_platform)
                .then_with(|| b.experience.observed_at.cmp(&a.experience.observed_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut group: Vec<ExperienceMatch> = Vec::new();
        for item in &candidates {
            if !group.iter().any(|prior| {
                prior.experience.observation == item.experience.observation
                    && prior.experience.response == item.experience.response
                    && prior.experience.outcome == item.experience.outcome
            }) {
                group.push(item.clone());
                if group.len() == 5 {
                    break;
                }
            }
        }
        for item in candidates {
            if group.len() == 5 {
                break;
            }
            if !group.iter().any(|prior| prior.id == item.id) {
                group.push(item);
            }
        }
        selected.extend(group);
    }
    selected
}

pub(crate) fn used_experience_revisions(
    experiences: &[ExperienceMatch],
    attempted: bool,
) -> Vec<(String, u64)> {
    if !attempted {
        return Vec::new();
    }
    experiences
        .iter()
        .map(|item| (item.id.clone(), item.revision))
        .collect()
}

fn discovery_anchor_selector(current: &str, links: &[String]) -> Option<String> {
    let target = crate::application_check::application_destination(current, links)?;
    let from = url::Url::parse(current).ok()?;
    let to = url::Url::parse(&target).ok()?;
    let fragment = to.fragment()?;
    if from.origin() != to.origin()
        || from.path() != to.path()
        || from.query() != to.query()
        || fragment.contains('/')
    {
        return None;
    }
    Some(format!("[id=\"{fragment}\"]"))
}

fn should_seek_obscured_form(live: &waypoint_applications::driver::LiveForm) -> bool {
    live.schema.fields.is_empty()
        && live
            .walls
            .iter()
            .any(|wall| wall.contains("A field may be obscured, clipped or transparent"))
        && live
            .walls
            .iter()
            .all(|wall| form_preview::is_custom_review(wall))
}

fn warn_missing_observed_fields(
    previous: &waypoint_applications::driver::LiveForm,
    current: &mut waypoint_applications::driver::LiveForm,
) {
    let missing: Vec<_> = previous
        .schema
        .fields
        .iter()
        .filter(|field| {
            !current
                .schema
                .fields
                .iter()
                .any(|next| next.selector == field.selector)
        })
        .collect();
    if !missing.is_empty() {
        let labels: Vec<String> = missing
            .iter()
            .take(3)
            .map(|field| {
                let mut chars = field.label.chars().filter(|ch| !ch.is_control());
                let mut label: String = chars.by_ref().take(80).collect();
                if chars.next().is_some() {
                    label.push('…');
                }
                if label.trim().is_empty() {
                    "Unlabelled field".into()
                } else {
                    label
                }
            })
            .collect();
        current.walls.push(format!("a custom control needs manual review: {} field(s) seen earlier are absent from this inspection ({}{}). Scroll and inspect again; the form may be incomplete.", missing.len(), labels.join("; "), if missing.len() > labels.len() { "; more omitted" } else { "" }));
    }
}

pub fn start(
    id: String,
    job_id: JobIdentityId,
    url: String,
    profile: serde_json::Value,
    model: Result<Option<String>, String>,
    visible: bool,
    review: (i64, Vec<ExperienceMatch>),
) -> Result<(), String> {
    let (pending_revision, experiences) = review;
    let (model, lease, no_model_reason) = match model {
        Ok(Some(model)) => match doc_suggest::lease() {
            Ok(lease) => (Some(model), Some(lease), String::new()),
            Err(error) => (None, None, error),
        },
        Ok(None) => (None, None, "No eligible local model is loaded".into()),
        Err(error) => (None, None, format!("Local model unavailable: {error}")),
    };
    let mut task = TASK.lock().map_err(|_| "Form inspection unavailable")?;
    if task.is_some() {
        return Err("A form inspection is running or awaiting its result".into());
    }
    let (sender, receiver) = mpsc::channel();
    let profile_digest = Sha256Digest::of(&profile.to_string());
    let experience_revisions: Vec<_> = experiences
        .iter()
        .map(|item| (item.id.clone(), item.revision))
        .collect();
    *task = Some(Task {
        id,
        receiver,
        job_id: job_id.clone(),
        pending_revision,
        url: url.clone(),
        profile_digest: profile_digest.clone(),
        visible,
        experience_revisions: experience_revisions.clone(),
        failure: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let _lease = lease;
        let mut used_revisions = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<_, String> {
            let mut driver = waypoint_browser::CdpDriver::launch_with_mode(vec![], visible)
                .map_err(|e| e.to_string())?;
            let (mut live, navigation_warning) = open_application_page(&mut driver, &url)?;
            if should_seek_obscured_form(&live) {
                let current = driver.current_url().map_err(|e| e.to_string())?;
                let selector = driver.application_links().ok().and_then(|links| discovery_anchor_selector(&current, &links));
                if let Some(selector) = selector {
                    if driver.with_review_timeout(|browser| browser.scroll_field_into_view(&selector)).is_ok() {
                        std::thread::sleep(std::time::Duration::from_millis(600));
                        if driver.current_url().ok().as_deref() == Some(current.as_str()) {
                            if let Ok(mut refreshed) = driver.with_review_timeout(|browser| browser.inspect_current()) {
                                if refreshed.walls.iter().all(|wall| form_preview::is_custom_review(wall)) {
                                    match driver.with_review_timeout(|browser| browser.reveal_section_controls(&selector)) {
                                        Ok((count, truncated)) => {
                                            std::thread::sleep(std::time::Duration::from_millis(500));
                                            if let Ok(mut after) = driver.with_review_timeout(|browser| browser.inspect_current()) {
                                                warn_missing_observed_fields(&refreshed, &mut after);
                                                for wall in &refreshed.walls { if !after.walls.contains(wall) { after.walls.push(wall.clone()); } }
                                                after.walls.push(format!("a custom control needs manual review: Scrolled through {count} controls in the application section{}; inspect the entire form.", if truncated { " (the 12-control limit was reached)" } else { "" }));
                                                refreshed = after;
                                            } else {
                                                refreshed.walls.push("Could not inspect the form after scrolling; review manually.".into());
                                            }
                                        }
                                        Err(_) => refreshed.walls.push("Could not safely inspect all controls while scrolling; review manually.".into()),
                                    }
                                }
                                for wall in &live.walls { if !refreshed.walls.contains(wall) { refreshed.walls.push(wall.clone()); } }
                                refreshed.walls.push("a custom control needs manual review: Scrolled to a possible application section; other fields may be outside the inspected area.".into());
                                live = refreshed;
                            }
                        } else {
                            live.walls.push("The page changed while locating the application section; inspect manually.".into());
                        }
                    }
                }
                if driver.current_url().ok().as_deref() != Some(current.as_str()) {
                    live.schema.fields.clear();
                    live.walls.push("The page changed during form discovery; inspect manually.".into());
                }
            }
            let observed_url = driver.current_url().map_err(|e| e.to_string())?;
            let relevant = relevant_experiences(&live.schema, &experiences);
            let mut preview = if let Some(model) = model {
                let llm = OllamaInference::new("http://127.0.0.1:11434", model.clone())
                    .map_err(|e| e.to_string())?
                    .with_resident_context();
                let mut llm = crate::resident_inference::ResidentInference::new(
                    llm,
                    model,
                    crate::resident_model,
                );
                let mut informed = crate::experience_inference::ExperienceInference::new(&mut llm, &relevant);
                let mut preview = form_preview::suggest_for_live_form(live, &mut informed, &profile)?;
                used_revisions = used_experience_revisions(&relevant, informed.attempted_with_hints());
                if informed.attempted_with_hints() {
                    preview.manual_review.push("A model mapping request included reviewed lessons as fallible context; verify every suggestion against this form.".into());
                    let (included, omitted) = informed.note_attempts();
                    if included + omitted > 0 {
                        preview.manual_review.push(format!("Across attempted mapping requests, opted-in note inclusions prepared: {included}; omitted by the context budget: {omitted}. A note can be counted more than once. This does not prove model receipt or use."));
                    }
                }
                preview
            } else {
                form_preview::suggest_without_model(live, &profile, &no_model_reason)?
            };
            if let Some(warning) = navigation_warning {
                preview.manual_review.push(warning);
            }
            let fresh = driver.with_review_timeout(|browser| browser.inspect_current())
                .map_err(|e| format!("Final form check failed; inspect it again: {e}"))?;
            if driver.current_url().map_err(|e| e.to_string())? != observed_url {
                return Err("The application changed while preparing its review; inspect it again".into());
            }
            merge_final_inspection(&mut preview, &fresh)?;
            Ok((driver, preview))
        }))
        .unwrap_or_else(|_| Err("Form inspection stopped unexpectedly".into()));
        let _ = sender.send(Finished {
            job_id,
            pending_revision,
            url,
            profile_digest,
            visible,
            experience_revisions: used_revisions,
            result,
        });
    });
    Ok(())
}

pub fn poll(id: &str) -> Result<Option<Finished>, String> {
    let mut tasks = TASK.lock().map_err(|_| "Form inspection unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Form inspection not found")?;
    if let Some(error) = &task.failure {
        return Ok(Some(Finished {
            job_id: task.job_id.clone(),
            pending_revision: task.pending_revision,
            url: task.url.clone(),
            profile_digest: task.profile_digest.clone(),
            visible: task.visible,
            experience_revisions: task.experience_revisions.clone(),
            result: Err(error.clone()),
        }));
    }
    match task.receiver.try_recv() {
        Ok(result) => {
            if let Err(error) = &result.result {
                task.failure = Some(error.clone());
            } else {
                *tasks = None;
            }
            Ok(Some(result))
        }
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => {
            let failed = Finished {
                job_id: task.job_id.clone(),
                pending_revision: task.pending_revision,
                url: task.url.clone(),
                profile_digest: task.profile_digest.clone(),
                visible: task.visible,
                experience_revisions: task.experience_revisions.clone(),
                result: Err("Form inspection stopped unexpectedly".into()),
            };
            task.failure = Some("Form inspection stopped unexpectedly".into());
            Ok(Some(failed))
        }
    }
}

pub fn completed_failure(id: &str) -> Result<Option<String>, String> {
    let completed = COMPLETED_FAILURE
        .lock()
        .map_err(|_| "Form inspection unavailable")?;
    Ok(completed
        .as_ref()
        .filter(|(task_id, _)| task_id == id)
        .map(|(_, error)| error.clone()))
}

pub fn acknowledge_failure(id: &str, error: String) -> Result<(), String> {
    let mut tasks = TASK.lock().map_err(|_| "Form inspection unavailable")?;
    if !tasks
        .as_ref()
        .is_some_and(|task| task.id == id && task.failure.is_some())
    {
        return Err("Form inspection failure not found".into());
    }
    let mut completed = COMPLETED_FAILURE
        .lock()
        .map_err(|_| "Form inspection unavailable")?;
    *completed = Some((id.to_owned(), error));
    *tasks = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_applications::driver::{DriverError, LiveForm, SiteAck};
    use waypoint_applications::form_mapping::{
        BrowserCommand, FormField, FormFieldType, FormSchema,
    };

    #[test]
    fn discovery_scroll_target_is_one_safe_same_page_anchor() {
        let base = "https://example.org/careers";
        assert_eq!(
            discovery_anchor_selector(base, &["#apply".into(), "#apply".into()]).as_deref(),
            Some("[id=\"apply\"]")
        );
        for links in [
            vec!["/apply"],
            vec!["https://other.example/apply#apply"],
            vec!["#submit"],
            vec!["#apply?x"],
            vec!["#apply", "#other"],
        ] {
            assert!(discovery_anchor_selector(
                base,
                &links.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_none());
        }
    }

    #[test]
    fn discovery_scroll_never_bypasses_a_captcha_or_other_wall() {
        let mut live = LiveForm { origin: "https://example.org".into(), schema: FormSchema { origin: "https://example.org".into(), fields: vec![], multi_step: false, steps: 1 }, walls: vec!["a custom control needs manual review: A field may be obscured, clipped or transparent; check overlays and the visible browser".into()] };
        assert!(should_seek_obscured_form(&live));
        live.walls.push("captcha present".into());
        assert!(!should_seek_obscured_form(&live));
        live.walls.pop();
        live.walls.push("login required".into());
        assert!(!should_seek_obscured_form(&live));
    }

    #[test]
    fn discovery_warns_when_previously_observed_fields_disappear() {
        let field = FormField {
            selector: "#previous".into(),
            label: "Earlier field".into(),
            field_type: FormFieldType::Text,
            required: false,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        };
        let previous = LiveForm {
            origin: "https://example.org".into(),
            schema: FormSchema {
                origin: "https://example.org".into(),
                fields: vec![field.clone()],
                multi_step: false,
                steps: 1,
            },
            walls: vec![],
        };
        let mut current = previous.clone();
        warn_missing_observed_fields(&previous, &mut current);
        assert!(current.walls.is_empty());
        current.schema.fields.clear();
        warn_missing_observed_fields(&previous, &mut current);
        assert!(
            current
                .walls
                .iter()
                .any(|wall| wall.contains("1 field(s) seen earlier")
                    && wall.contains("Earlier field"))
        );
        assert!(form_preview::is_custom_review(&current.walls[0]));
    }

    #[test]
    #[ignore = "read-only public form; requires a browser and an explicitly supplied field selector"]
    fn live_scroll_reveals_deferred_form_fields_without_filling() {
        let url = std::env::var("WAYPOINT_SCROLL_TEST_URL").unwrap();
        let selector = std::env::var("WAYPOINT_SCROLL_TEST_SELECTOR").unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let source = crate::manual_job::source(parsed.host_str().unwrap());
        crate::form_links::check_job_and_destination(
            source,
            &parsed.origin().ascii_serialization(),
            waypoint_policy::Operation::Read,
        )
        .unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let initial = driver.open(&url).unwrap();
        assert!(
            initial.schema.fields.is_empty(),
            "initial form state changed"
        );
        if let Ok(section) = std::env::var("WAYPOINT_SCROLL_TEST_SECTION") {
            let (count, truncated) = driver.reveal_section_controls(&section).unwrap();
            assert!(count > 0);
            println!("section controls scrolled: {count}; truncated: {truncated}");
        }
        driver.scroll_field_into_view(&selector).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let refreshed = driver.inspect_current().unwrap();
        assert!(
            !refreshed.schema.fields.is_empty(),
            "scroll did not reveal form fields"
        );
        println!(
            "fields after scroll: {}; walls: {:?}",
            refreshed.schema.fields.len(),
            refreshed.walls
        );
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "read-only public form; captures bounded screenshots with a browser"]
    fn live_scroll_capture_reports_dimension_changes() {
        let url = std::env::var("WAYPOINT_SCROLL_TEST_URL").unwrap();
        let section = std::env::var("WAYPOINT_SCROLL_TEST_SECTION").unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let source = crate::manual_job::source(parsed.host_str().unwrap());
        crate::form_links::check_job_and_destination(
            source,
            &parsed.origin().ascii_serialization(),
            waypoint_policy::Operation::Read,
        )
        .unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(&url).unwrap();
        driver.scroll_field_into_view(&section).unwrap();
        driver.reveal_section_controls(&section).unwrap();
        let before = driver.page_dimensions().unwrap();
        let width = before.0.min(2048.0);
        println!("initial dimensions: {before:?}");
        for index in 0..6 {
            let y = f64::from(index) * 2000.0;
            if y >= before.1 {
                break;
            }
            let image = driver
                .screenshot_tile(0.0, y, width, (before.1 - y).min(2000.0))
                .unwrap();
            assert!(!image.is_empty());
            drop(image);
            let after = driver.page_dimensions().unwrap();
            println!("after tile {index}: {after:?}");
            if after != before {
                for delay in [100, 500, 1000] {
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    println!("after {delay}ms: {:?}", driver.page_dimensions().unwrap());
                }
            }
        }
        if before.1 > 2000.0 {
            let image = driver
                .screenshot_tile(0.0, before.1 - 2000.0, width, 2000.0)
                .unwrap();
            drop(image);
            println!(
                "after overlapping bottom tile: {:?}",
                driver.page_dimensions().unwrap()
            );
        }
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    fn final_inspection_preserves_late_walls_and_rejects_schema_changes() {
        use waypoint_applications::{driver::LiveForm, form_mapping::FormSchema};
        let schema = FormSchema {
            origin: "https://example.invalid".into(),
            fields: vec![],
            multi_step: false,
            steps: 1,
        };
        let mut preview = form_preview::FormPreview {
            schema: schema.clone(),
            suggestions: vec![],
            manual_review: vec!["earlier wall".into()],
            model_review_selectors: Default::default(),
        };
        let mut fresh = LiveForm {
            origin: schema.origin.clone(),
            schema,
            walls: vec!["captcha present".into()],
        };
        merge_final_inspection(&mut preview, &fresh).unwrap();
        assert!(preview
            .manual_review
            .iter()
            .any(|wall| wall == "captcha present"));
        assert!(preview
            .manual_review
            .iter()
            .any(|wall| wall == "earlier wall"));
        merge_final_inspection(&mut preview, &fresh).unwrap();
        assert_eq!(
            preview
                .manual_review
                .iter()
                .filter(|wall| *wall == "captcha present")
                .count(),
            1
        );
        fresh.schema.multi_step = true;
        assert!(merge_final_inspection(&mut preview, &fresh).is_err());
        fresh.schema = preview.schema.clone();
        fresh.origin = "https://other.invalid".into();
        assert!(merge_final_inspection(&mut preview, &fresh).is_err());
    }

    #[test]
    fn lesson_selection_uses_live_non_sensitive_control_types() {
        use waypoint_applications::form_mapping::{FormField, FormFieldType as Field, FormSchema};
        use waypoint_workspace::form_experience::{
            Control, Experience, Observation, Outcome, Response, Stage,
        };
        let make = |control| ExperienceMatch {
            id: format!("{control:?}"),
            revision: 1,
            same_platform: false,
            experience: Experience {
                version: 1,
                reviewed: true,
                enabled: true,
                observed_at: 1,
                platform: None,
                stage: Stage::EnterInformation,
                control,
                observation: Observation::AmbiguousLabel,
                response: Response::InspectContext,
                outcome: Outcome::Unknown,
                note: None,
                use_note_in_model: false,
            },
        };
        let experiences = vec![
            make(Control::Text),
            make(Control::Choice),
            make(Control::Upload),
        ];
        let mut schema = FormSchema {
            origin: "https://example.invalid".into(),
            fields: vec![],
            multi_step: false,
            steps: 1,
        };
        assert!(relevant_experiences(&schema, &experiences).is_empty());
        let field = |field_type, sensitive| FormField {
            selector: "#fixture".into(),
            label: "Fixture".into(),
            field_type,
            required: false,
            sensitive,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        };
        schema.fields.push(field(Field::Email, false));
        schema.fields.push(field(Field::Checkbox, true));
        assert_eq!(
            relevant_experiences(&schema, &experiences)
                .iter()
                .map(|x| x.experience.control)
                .collect::<Vec<_>>(),
            vec![Control::Text]
        );
        schema.fields[1].sensitive = false;
        assert_eq!(relevant_experiences(&schema, &experiences).len(), 2);
        schema.fields.clear();
        schema.fields.push(field(Field::File, false));
        assert!(relevant_experiences(&schema, &experiences).is_empty());
    }

    #[test]
    fn lesson_ranking_uses_observed_host_and_preserves_control_budgets() {
        use waypoint_applications::form_mapping::{FormField, FormFieldType as Field, FormSchema};
        use waypoint_workspace::form_experience::{
            Control, Experience, Observation, Outcome, Platform, Response, Stage,
        };
        assert_eq!(
            observed_platform("https://jobs.lever.co"),
            Some(Platform::Lever)
        );
        assert_eq!(
            observed_platform("https://jobs.lever.co.attacker.invalid"),
            None
        );
        assert_eq!(observed_platform("http://jobs.lever.co"), None);
        assert_eq!(
            observed_platform("https://apply.workable.com"),
            Some(Platform::Workable)
        );
        assert_eq!(
            observed_platform("https://apply.workable.com.attacker.invalid"),
            None
        );
        let field = |field_type| FormField {
            selector: "#fixture".into(),
            label: "Fixture".into(),
            field_type,
            required: false,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        };
        let schema = FormSchema {
            origin: "https://jobs.lever.co".into(),
            fields: vec![field(Field::Text), field(Field::Checkbox)],
            multi_step: false,
            steps: 1,
        };
        let item = |id: usize, control, platform, observed_at| ExperienceMatch {
            id: format!("{id:032x}"),
            revision: 1,
            same_platform: false,
            experience: Experience {
                version: 1,
                reviewed: true,
                enabled: true,
                observed_at,
                platform: Some(platform),
                stage: Stage::EnterInformation,
                control,
                observation: Observation::AmbiguousLabel,
                response: Response::InspectContext,
                outcome: Outcome::Unknown,
                note: None,
                use_note_in_model: false,
            },
        };
        let mut items: Vec<_> = (0..6)
            .map(|i| item(i, Control::Text, Platform::Greenhouse, 100 + i as i64))
            .collect();
        items.push(item(6, Control::Text, Platform::Lever, 1));
        items.push(item(7, Control::Choice, Platform::Lever, 1));
        let selected = relevant_experiences(&schema, &items);
        assert_eq!(selected.len(), 6);
        assert_eq!(selected[0].experience.platform, Some(Platform::Lever));
        assert_eq!(
            selected
                .iter()
                .filter(|value| value.experience.control == Control::Text)
                .count(),
            5
        );
        assert_eq!(
            selected
                .iter()
                .filter(|value| value.experience.control == Control::Choice)
                .count(),
            1
        );
        let mut unrelated = schema.clone();
        unrelated.origin = "https://example.invalid".into();
        assert!(relevant_experiences(&unrelated, &items)
            .iter()
            .all(|value| !value.same_platform));
        let mut workable_schema = schema.clone();
        workable_schema.origin = "https://apply.workable.com".into();
        let mut workable_lesson = item(8, Control::Text, Platform::Workable, 1);
        workable_lesson.experience.observation = Observation::ValidationError;
        items.push(workable_lesson);
        let workable_selected = relevant_experiences(&workable_schema, &items);
        assert_eq!(
            workable_selected[0].experience.platform,
            Some(Platform::Workable)
        );
        assert!(workable_selected[0].same_platform);
        let mut full: Vec<_> = (0..500)
            .map(|i| {
                item(
                    i,
                    if i < 250 {
                        Control::Text
                    } else {
                        Control::Choice
                    },
                    Platform::Greenhouse,
                    i as i64,
                )
            })
            .collect();
        full[0].experience.platform = Some(Platform::Lever);
        full[250].experience.platform = Some(Platform::Lever);
        let selected = relevant_experiences(&schema, &full);
        assert_eq!(selected.len(), 10);
        assert!(selected[0].same_platform && selected[5].same_platform);
    }

    #[cfg(all(windows, feature = "sqlcipher"))]
    #[test]
    #[ignore = "starts a fictional local browser and needs a preloaded local model and resource preflight"]
    fn live_inspection_worker_with_reviewed_note() {
        use waypoint_workspace::form_experience::{
            Control, Experience, Observation, Outcome, Response, Stage,
        };
        let _guard = TEST_LOCK.lock().unwrap();
        let model = crate::resident_model()
            .unwrap()
            .expect("eligible resident model");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-ambiguous-contact.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        let id = "00000000000000000000000000000001";
        let dir = tempfile::tempdir().unwrap();
        let workspace = waypoint_workspace::Workspace::open_encrypted(dir.path()).unwrap();
        let experience = Experience {
            version: 1,
            reviewed: true,
            enabled: true,
            observed_at: 1,
            platform: None,
            stage: Stage::EnterInformation,
            control: Control::Text,
            observation: Observation::AmbiguousLabel,
            response: Response::InspectContext,
            outcome: Outcome::Helped,
            note: Some("Review contact questions that do not name a specific channel".into()),
            use_note_in_model: true,
        };
        assert_eq!(
            workspace.save_form_experience(id, 0, &experience).unwrap(),
            1
        );
        let experiences = workspace
            .related_form_experiences(Stage::EnterInformation, Control::Text, None)
            .unwrap();
        assert_eq!(experiences.len(), 1);
        assert_eq!(experiences[0].experience.note, experience.note);
        start(
            "fixture-note-inspection".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({"email":"alex@example.invalid"}),
            Ok(Some(model)),
            false,
            (0, experiences),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        loop {
            if let Some(done) = poll("fixture-note-inspection").unwrap() {
                assert_eq!(done.experience_revisions, vec![(id.to_owned(), 1)]);
                let (driver, preview) = done.result.unwrap();
                println!(
                    "fictional note inspection warnings: {:?}, mappings: {:?}",
                    preview.manual_review, preview.suggestions
                );
                assert!(preview.manual_review.iter().any(
                    |reason| reason.contains("model mapping request included reviewed lessons")
                ));
                assert!(preview.manual_review.iter().any(|reason| reason.contains(
                    "opted-in note inclusions prepared: 1; omitted by the context budget: 0"
                )));
                assert!(!preview
                    .manual_review
                    .iter()
                    .any(|reason| reason
                        .contains("only exact labels or standard autocomplete hints")));
                assert!(preview
                    .suggestions
                    .iter()
                    .any(|m| m.field_selector == "#email"));
                assert!(!preview
                    .suggestions
                    .iter()
                    .any(|m| m.field_selector == "#contact" || m.field_selector == "#reach"));
                assert!(driver
                    .trace
                    .iter()
                    .all(|entry| !entry.contains("submitted")));
                assert!(workspace.revoke_form_experience(id, 1).unwrap());
                assert!(workspace
                    .related_form_experiences(Stage::EnterInformation, Control::Text, None)
                    .unwrap()
                    .is_empty());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "inspection did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[cfg(windows)]
    #[test]
    fn new_experience_can_be_saved_during_inspection_but_not_revoked() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut ws = waypoint_workspace::Workspace::open(dir.path()).unwrap();
        let (sender, receiver) = mpsc::channel();
        *TASK.lock().unwrap() = Some(Task {
            id: "fixture-pending".into(),
            receiver,
            job_id: JobIdentityId::new("fixture-job").unwrap(),
            pending_revision: 0,
            url: "https://example.invalid/job".into(),
            profile_digest: Sha256Digest::of("fixture"),
            visible: false,
            experience_revisions: vec![],
            failure: None,
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut request = |path: &str, body: serde_json::Value| {
            let body = body.to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            let served = crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port);
            if served.is_err() {
                return (served, serde_json::Value::Null);
            }
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            (
                served,
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap(),
            )
        };
        let (saved, value) = request(
            "/api/form-experiences/save",
            serde_json::json!({"confirmed":true,"experience":{"stage":"enter_information","control":"text","platform":null,"observation":"value_changed","response":"verify_value_again","outcome":"unknown"}}),
        );
        assert!(saved.is_ok());
        let (blocked, _) = request(
            "/api/form-experiences/revoke",
            serde_json::json!({"confirmed":true,"id":value["id"],"revision":value["revision"]}),
        );
        assert!(blocked
            .unwrap_err()
            .contains("Close the active form review"));
        let (blocked, _) = request(
            "/api/form-experiences/set-enabled",
            serde_json::json!({"confirmed":true,"id":value["id"],"revision":value["revision"],"enabled":false}),
        );
        assert!(blocked
            .unwrap_err()
            .contains("Close the active form review"));
        *TASK.lock().unwrap() = None;
        drop(sender);
        let (removed, _) = request(
            "/api/form-experiences/revoke",
            serde_json::json!({"confirmed":true,"id":value["id"],"revision":value["revision"]}),
        );
        assert!(removed.is_ok());
    }

    #[test]
    fn pending_inspection_survives_reopen_and_preserves_other_reasons() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(base).unwrap();
        let (job, revision) = {
            let mut ws = waypoint_workspace::Workspace::open(dir.path()).unwrap();
            let added = crate::manual_job::add(&mut ws, &serde_json::json!({"url":"https://example.org/inspection-fixture","title":"Role","employer":"Fixture"}), 1).unwrap();
            let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
            let revision = begin_pending(&ws, &job, "inspection-task", 2).unwrap();
            ws.mark_manual_review(&job, "Another review issue", 3)
                .unwrap();
            (job, revision)
        };
        let ws = waypoint_workspace::Workspace::open(dir.path()).unwrap();
        assert!(ws
            .manual_review_reasons(&job)
            .unwrap()
            .iter()
            .any(|entry| entry.reason == pending_reason("inspection-task")));
        assert!(!finish_pending(&ws, &job, "inspection-task", revision + 1).unwrap());
        assert!(finish_pending(&ws, &job, "inspection-task", revision).unwrap());
        let reasons = ws.manual_review_reasons(&job).unwrap();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].reason, "Another review issue");
    }

    #[test]
    fn inspection_failure_survives_review_flag_error() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let _guard = TEST_LOCK.lock().unwrap();
        let posting = serde_json::json!({"url":"https://example.org/temporary","title":"Engineer","employer":"Fixture"});
        let mut source = waypoint_workspace::Workspace::in_memory().unwrap();
        let added = crate::manual_job::add(&mut source, &posting, 1).unwrap();
        let job_id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "fixture-write-error".into(),
            receiver,
            job_id: job_id.clone(),
            pending_revision: 0,
            url: "https://example.org/temporary".into(),
            profile_digest: Sha256Digest::of("fixture"),
            visible: false,
            experience_revisions: vec![],
            failure: None,
        });
        assert_eq!(
            pending().unwrap(),
            Some(("fixture-write-error".into(), job_id.as_str().into()))
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = serde_json::json!({"task_id":"fixture-write-error"}).to_string();
        let make_client = || {
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST /api/form/inspect-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            client
        };
        let _first_client = make_client();
        let (mut server, _) = listener.accept().unwrap();
        assert!(crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).is_err());
        assert!(busy());
        assert!(poll("fixture-write-error")
            .unwrap()
            .unwrap()
            .result
            .is_err());
        assert_eq!(
            crate::manual_job::add(&mut ws, &posting, 1).unwrap()["id"],
            job_id.as_str()
        );
        let mut client = make_client();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(ws.manual_reviews().unwrap().len(), 1);
        assert!(!busy());
    }

    #[test]
    fn disconnected_worker_keeps_job_context_for_manual_review() {
        let _guard = TEST_LOCK.lock().unwrap();
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        #[cfg(all(windows, feature = "sqlcipher"))]
        let dir = tempfile::tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        )
        .unwrap();
        #[cfg(all(windows, feature = "sqlcipher"))]
        let mut ws = waypoint_workspace::Workspace::open_encrypted(dir.path()).unwrap();
        #[cfg(not(all(windows, feature = "sqlcipher")))]
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let added = crate::manual_job::add(
            &mut ws,
            &serde_json::json!({
                "url":"https://example.org/job", "title":"Engineer", "employer":"Fixture", "description":"Fictional role description"
            }),
            1,
        )
        .unwrap();
        let job_id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let pending_revision = begin_pending(&ws, &job_id, "fictional-task", 2).unwrap();
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "fictional-task".into(),
            receiver,
            job_id: job_id.clone(),
            pending_revision,
            url: "https://example.org/job".into(),
            profile_digest: Sha256Digest::of("fictional-profile"),
            visible: false,
            experience_revisions: vec![],
            failure: None,
        });
        assert!(crate::active_form_review_reasons(&job_id, &None)
            .unwrap()
            .contains(&pending_reason("fictional-task")));
        assert!(poll("fictional-task").unwrap().unwrap().result.is_err());
        assert!(poll("fictional-task").unwrap().unwrap().result.is_err());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = serde_json::json!({"task_id":"fictional-task"}).to_string();
        write!(client, "POST /api/form/inspect-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["status"], "failed");
        let reviews = ws.manual_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].job_id, job_id.as_str());
        assert!(reviews[0]
            .reason
            .contains("Form inspection stopped unexpectedly"));
        let revision = ws.manual_review_reasons(&job_id).unwrap()[0].revision;
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/form/inspect-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let repeated: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(repeated, result);
        assert_eq!(
            ws.manual_review_reasons(&job_id).unwrap()[0].revision,
            revision
        );
        #[cfg(all(windows, feature = "sqlcipher"))]
        {
            let records: Vec<_> = std::fs::read_dir(dir.path().join("application-records"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(records.len(), 1);
            let record = &records[0];
            let original: serde_json::Value =
                serde_json::from_slice(&std::fs::read(record.join("original-job.json")).unwrap())
                    .unwrap();
            assert_eq!(original["description"], "Fictional role description");
            assert_eq!(original["source_url"], "https://example.org/job");
            let attempt: serde_json::Value =
                serde_json::from_slice(&std::fs::read(record.join("attempt.json")).unwrap())
                    .unwrap();
            assert_eq!(attempt["status"], "manual_review");
            let events = std::fs::read_to_string(record.join("events.jsonl")).unwrap();
            let inspections: Vec<serde_json::Value> = events
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .filter(|event: &serde_json::Value| event["type"] == "page_inspected")
                .collect();
            assert_eq!(inspections.len(), 1);
            assert_eq!(inspections[0]["screenshot_available"], false);
            assert!(inspections[0]["manual_review"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason
                    .as_str()
                    .is_some_and(|reason| reason.contains("stopped unexpectedly"))));
        }
        assert!(!busy());
    }

    #[test]
    #[ignore = "starts a local browser while holding the model lease"]
    fn busy_model_does_not_block_form_inspection() {
        use waypoint_workspace::form_experience::{
            Control, Experience, Observation, Outcome, Response, Stage,
        };
        let _guard = TEST_LOCK.lock().unwrap();
        let _lease = crate::doc_suggest::lease().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        let lesson = ExperienceMatch {
            id: "fictional-lesson".into(),
            revision: 1,
            same_platform: false,
            experience: Experience {
                version: 1,
                reviewed: true,
                enabled: true,
                observed_at: 1,
                platform: None,
                stage: Stage::EnterInformation,
                control: Control::Text,
                observation: Observation::AmbiguousLabel,
                response: Response::InspectContext,
                outcome: Outcome::Unknown,
                note: None,
                use_note_in_model: false,
            },
        };
        start(
            "busy-fixture".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({"country":"Germany"}),
            Ok(Some("fictional-model".into())),
            false,
            (0, vec![lesson]),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(done) = poll("busy-fixture").unwrap() {
                assert!(done.experience_revisions.is_empty());
                let (_, preview) = done.result.unwrap();
                assert!(preview
                    .suggestions
                    .iter()
                    .any(|item| item.field_selector == "#country"));
                assert!(preview
                    .manual_review
                    .iter()
                    .any(|reason| reason.contains("already running")));
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    struct FakeDriver {
        current: String,
        links: Vec<String>,
        opened: Vec<String>,
        redirect: Option<String>,
        direct_upload: bool,
    }
    impl BrowserDriver for FakeDriver {
        fn open(&mut self, url: &str) -> Result<LiveForm, DriverError> {
            self.opened.push(url.into());
            self.current = self.redirect.take().unwrap_or_else(|| url.into());
            let fields = if self.direct_upload && !url.ends_with("/apply") {
                vec![FormField {
                    selector: "#resume".into(),
                    label: "Resume".into(),
                    field_type: FormFieldType::File,
                    required: false,
                    sensitive: false,
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: None,
                }]
            } else if url.ends_with("/apply") || url.ends_with("/form") {
                vec![FormField {
                    selector: "#name".into(),
                    label: if url.ends_with("/form") {
                        "First name"
                    } else {
                        "Name"
                    }
                    .into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: false,
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: None,
                }]
            } else {
                vec![]
            };
            Ok(LiveForm {
                origin: "https://jobs.example.org".into(),
                schema: FormSchema {
                    origin: "https://jobs.example.org".into(),
                    fields,
                    multi_step: false,
                    steps: 1,
                },
                walls: vec![],
            })
        }
        fn current_url(&mut self) -> Result<String, DriverError> {
            Ok(self.current.clone())
        }
        fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
            if self.links == ["fixture-link-inspection-error"] {
                Err(DriverError("fictional link read failure".into()))
            } else {
                Ok(self.links.clone())
            }
        }
        fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
            panic!("inspection must not execute field actions")
        }
        fn commit(&mut self) -> Result<SiteAck, DriverError> {
            panic!("inspection must not submit")
        }
    }

    struct EmbeddedDriver(FakeDriver);
    impl BrowserDriver for EmbeddedDriver {
        fn open(&mut self, url: &str) -> Result<LiveForm, DriverError> {
            let mut page = self.0.open(url)?;
            page.walls
                .push(waypoint_browser::UNINSPECTED_FRAME_REVIEW.into());
            Ok(page)
        }
        fn current_url(&mut self) -> Result<String, DriverError> {
            self.0.current_url()
        }
        fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
            self.0.application_links()
        }
        fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
            panic!("inspection must not execute actions")
        }
        fn commit(&mut self) -> Result<SiteAck, DriverError> {
            panic!("inspection must not submit")
        }
    }

    #[test]
    fn embedded_form_with_main_fields_stays_on_current_page() {
        let mut driver = EmbeddedDriver(FakeDriver {
            current: String::new(),
            links: vec!["/other/apply".into()],
            opened: vec![],
            redirect: None,
            direct_upload: false,
        });
        let (page, warning) =
            open_application_page(&mut driver, "https://jobs.example.org/form").unwrap();
        assert_eq!(driver.0.opened, ["https://jobs.example.org/form"]);
        assert!(has_main_fields_and_embedded_content(&page));
        assert!(warning.unwrap().contains("review it before navigating"));

        let mut listing = EmbeddedDriver(FakeDriver {
            current: String::new(),
            links: vec!["/role/apply".into()],
            opened: vec![],
            redirect: None,
            direct_upload: false,
        });
        let (page, _) =
            open_application_page(&mut listing, "https://jobs.example.org/role").unwrap();
        assert_eq!(
            listing.0.opened,
            [
                "https://jobs.example.org/role",
                "https://jobs.example.org/role/apply"
            ]
        );
        assert!(has_main_fields_and_embedded_content(&page));
    }

    #[test]
    #[ignore = "starts one local browser against a fictional process-isolated form"]
    fn isolated_embedded_form_does_not_follow_another_link() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_inaccessible_frame.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap())
            .unwrap()
            .to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let (page, _) = open_application_page(&mut driver, &url).unwrap();
        assert!(page.schema.fields.is_empty());
        assert!(page
            .walls
            .iter()
            .any(|wall| wall == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW));
        assert_eq!(driver.current_url().unwrap(), url);
        assert!(driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser against a fictional inaccessible form"]
    fn inaccessible_embedded_form_does_not_follow_another_link() {
        let fixture = cross_origin_form_fixture();
        let url = fixture.url.clone();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let (page, _) = open_application_page(&mut driver, &url).unwrap();
        assert!(page.schema.fields.is_empty());
        assert!(page
            .walls
            .iter()
            .any(|wall| wall == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW));
        assert_eq!(driver.current_url().unwrap(), url);
        assert!(driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a local browser against a fictional embedded form"]
    fn embedded_form_inspection_does_not_follow_another_link() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-embedded-listing.html");
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let (page, warning) = open_application_page(&mut driver, &url).unwrap();
        assert_eq!(driver.current_url().unwrap(), url);
        assert!(has_main_fields_and_embedded_content(&page));
        assert!(warning.unwrap().contains("review it before navigating"));
        assert!(driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }

    #[test]
    fn follows_clear_application_link_and_hands_off_ambiguous_destinations() {
        let mut driver = FakeDriver {
            current: String::new(),
            links: vec!["/role/apply".into()],
            opened: vec![],
            redirect: None,
            direct_upload: false,
        };
        let (form, warning) =
            open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
        assert!(warning.is_none());
        assert_eq!(
            driver.opened,
            [
                "https://jobs.example.org/role",
                "https://jobs.example.org/role/apply"
            ]
        );
        assert_eq!(form.schema.fields.len(), 1);
        let mut driver = FakeDriver {
            current: String::new(),
            links: vec!["https://another.example/apply".into()],
            opened: vec![],
            redirect: None,
            direct_upload: false,
        };
        let (form, warning) =
            open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
        assert_eq!(driver.opened.len(), 1);
        assert!(form.walls.is_empty());
        assert!(warning.unwrap().contains("ambiguous"));
        let mut driver = FakeDriver {
            current: String::new(),
            links: vec![],
            opened: vec![],
            redirect: Some("https://another.example/job".into()),
            direct_upload: false,
        };
        let (form, _) =
            open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
        assert!(!form.walls.is_empty());
    }

    #[test]
    fn keeps_existing_upload_form_instead_of_following_another_link() {
        let mut driver = FakeDriver {
            current: String::new(),
            links: vec!["/other/apply".into()],
            opened: vec![],
            redirect: None,
            direct_upload: true,
        };
        let (form, _) =
            open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
        assert_eq!(driver.opened, ["https://jobs.example.org/role"]);
        assert_eq!(form.schema.fields[0].field_type, FormFieldType::File);
    }

    #[test]
    fn recognizes_multiple_required_data_entries_without_field_name_rules() {
        let field = |required, field_type| FormField {
            selector: "#example".into(),
            label: "Response".into(),
            field_type,
            required,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        };
        assert!(has_required_data_entry_form(&[
            field(true, FormFieldType::Text),
            field(true, FormFieldType::TextArea)
        ]));
        assert!(!has_required_data_entry_form(&[
            field(true, FormFieldType::Text),
            field(false, FormFieldType::Email)
        ]));
        assert!(!has_required_data_entry_form(&[
            field(true, FormFieldType::Checkbox),
            field(true, FormFieldType::Text)
        ]));
    }

    #[test]
    #[ignore = "starts a local browser against a fictional form"]
    fn keeps_current_form_when_another_application_link_is_present() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-current-step-with-application-link.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap())
            .unwrap()
            .to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let (page, warning) = open_application_page(&mut driver, &url).unwrap();
        assert_eq!(driver.current_url().unwrap(), url);
        assert!(has_required_data_entry_form(&page.schema.fields));
        assert!(warning.unwrap().contains("required data-entry"));
        assert!(driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }

    #[test]
    fn ambiguous_links_do_not_block_existing_form_suggestions() {
        let mut driver = FakeDriver {
            current: String::new(),
            links: vec!["https://other.example/apply".into()],
            opened: vec![],
            redirect: None,
            direct_upload: false,
        };
        let (form, warning) =
            open_application_page(&mut driver, "https://jobs.example.org/form").unwrap();
        assert!(warning.unwrap().contains("ambiguous"));
        assert!(form.walls.is_empty());
        assert_eq!(driver.opened.len(), 1);
        let preview = form_preview::suggest_without_model(
            form,
            &serde_json::json!({"first_name":"Alex"}),
            "Model unavailable",
        )
        .unwrap();
        assert!(preview
            .suggestions
            .iter()
            .any(|mapping| mapping.field_selector == "#name"));
    }

    #[test]
    fn failed_link_read_keeps_existing_form_available() {
        let mut driver = FakeDriver {
            current: String::new(),
            links: vec!["fixture-link-inspection-error".into()],
            opened: vec![],
            redirect: None,
            direct_upload: false,
        };
        let (form, warning) =
            open_application_page(&mut driver, "https://jobs.example.org/form").unwrap();
        assert!(warning.unwrap().contains("could not be inspected"));
        assert!(form.walls.is_empty());
        let preview = form_preview::suggest_without_model(
            form,
            &serde_json::json!({"first_name":"Alex"}),
            "Model unavailable",
        )
        .unwrap();
        assert!(preview
            .suggestions
            .iter()
            .any(|mapping| mapping.field_selector == "#name"));
        assert_eq!(driver.opened.len(), 1);
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional local form"]
    fn background_inspection_keeps_form_and_ambiguous_link_warning() {
        let _guard = TEST_LOCK.lock().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/application-check-ambiguous-links.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        start(
            "ambiguous-inspection-fixture".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({"email":"alex@example.invalid"}),
            Ok(None),
            false,
            (0, vec![]),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(done) = poll("ambiguous-inspection-fixture").unwrap() {
                let (driver, preview) = done.result.unwrap();
                assert!(preview
                    .manual_review
                    .iter()
                    .any(|reason| reason.contains("Application links are ambiguous")));
                assert!(preview
                    .suggestions
                    .iter()
                    .any(|mapping| mapping.field_selector == "#email"));
                assert!(driver
                    .trace
                    .iter()
                    .all(|entry| !entry.contains("submitted")));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "inspection did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    #[ignore = "starts one local browser and requires a preloaded local model and resource preflight"]
    fn local_model_inspects_form_with_ambiguous_links() {
        let _guard = TEST_LOCK.lock().unwrap();
        let model = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL")
            .unwrap_or_else(|_| "qwen3.5:9b".into());
        assert_eq!(
            crate::resident_model().unwrap().as_deref(),
            Some(model.as_str())
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-ambiguous-contact.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        start(
            "model-ambiguous-fixture".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({"email":"alex@example.invalid"}),
            Ok(Some(model)),
            false,
            (0, vec![]),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        loop {
            if let Some(done) = poll("model-ambiguous-fixture").unwrap() {
                let (driver, preview) = done.result.unwrap();
                println!(
                    "fictional model inspection warnings: {:?}, mappings: {:?}",
                    preview.manual_review, preview.suggestions
                );
                assert!(preview
                    .manual_review
                    .iter()
                    .any(|reason| reason.contains("Application links are ambiguous")));
                assert!(
                    !preview.manual_review.iter().any(|reason| reason.contains(
                        "only exact labels or standard autocomplete hints are suggested"
                    )),
                    "the model pass fell back to exact labels"
                );
                assert!(
                    !preview
                        .suggestions
                        .iter()
                        .any(|mapping| mapping.field_selector == "#contact"),
                    "an ambiguous contact method must not be guessed"
                );
                assert!(
                    preview
                        .manual_review
                        .iter()
                        .any(|reason| reason
                            .contains("Where should our recruiting team send updates?")),
                    "the ambiguous field must be reported for review"
                );
                assert!(
                    !preview
                        .suggestions
                        .iter()
                        .any(|mapping| mapping.field_selector == "#reach"),
                    "a second ambiguous contact field must not be guessed"
                );
                assert!(
                    preview
                        .manual_review
                        .iter()
                        .any(|reason| reason.contains("How can we reach you?")),
                    "the second ambiguous field must be reported"
                );
                assert!(
                    preview
                        .suggestions
                        .iter()
                        .any(|mapping| mapping.field_selector == "#email"),
                    "an explicitly identified email field should remain usable"
                );
                assert!(driver
                    .trace
                    .iter()
                    .all(|entry| !entry.contains("submitted")));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "inspection did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional local form"]
    fn retains_a_live_upload_form_for_review() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let (form, _) = open_application_page(&mut driver, &url).unwrap();
        assert!(form
            .schema
            .fields
            .iter()
            .any(|field| matches!(field.field_type, FormFieldType::File)));
        assert_eq!(driver.current_url().unwrap(), url);
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }
    #[test]
    #[ignore = "read-only public application worker inspection"]
    fn live_public_worker_rechecks_before_presenting_review() {
        let _guard = TEST_LOCK.lock().unwrap();
        let url =
            std::env::var("WAYPOINT_PUBLIC_APPLICATION_URL").expect("set public application URL");
        let parsed = url::Url::parse(&url).unwrap();
        let source = crate::manual_job::source(parsed.host_str().unwrap());
        crate::form_links::check_job_and_destination(
            source,
            &parsed.origin().ascii_serialization(),
            waypoint_policy::Operation::Read,
        )
        .unwrap();
        start(
            "public-worker-fixture".into(),
            JobIdentityId::new("fictional-job").unwrap(),
            url,
            serde_json::json!({}),
            Err("model intentionally not started".into()),
            false,
            (0, vec![]),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(finished) = poll("public-worker-fixture").unwrap() {
                match finished.result {
                    Ok((driver, preview)) => {
                        println!(
                            "review returned: fields={} warnings={} captcha={}",
                            preview.schema.fields.len(),
                            preview.manual_review.len(),
                            preview
                                .manual_review
                                .iter()
                                .any(|wall| wall == "captcha present")
                        );
                        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
                            && !entry.contains("clicked")
                            && !entry.contains("submitted")));
                    }
                    Err(error) => {
                        println!("review not returned: {error}");
                        assert!(
                            error.contains("changed") || error.contains("Final form check failed"),
                            "unexpected failure: {error}"
                        );
                    }
                }
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "inspection did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional local fixture"]
    fn background_inspection_returns_the_live_form_without_filling() {
        let _guard = TEST_LOCK.lock().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        start(
            "inspection-fixture".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({}),
            Err("fictional memory guard failure".into()),
            false,
            (0, vec![]),
        )
        .unwrap();
        assert!(busy());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(finished) = poll("inspection-fixture").unwrap() {
                let (driver, preview) = finished.result.unwrap();
                assert!(!preview.schema.fields.is_empty());
                assert!(preview.suggestions.is_empty());
                assert!(preview.manual_review.iter().any(|reason| reason
                    .contains("Local model unavailable: fictional memory guard failure")));
                assert!(preview
                    .manual_review
                    .iter()
                    .any(|reason| reason.contains("Required field needs review")));
                assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!busy());
    }
}
