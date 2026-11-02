//! One background form inspection; the server thread owns the resulting session.
use crate::{doc_suggest, form_preview};
use std::sync::{mpsc, Mutex};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::{JobIdentityId, Sha256Digest};
use waypoint_inference::OllamaInference;
use waypoint_workspace::form_experience::ExperienceMatch;

// A narrow advisory for explicit instructions in untrusted page text. This is
// not a comprehensive policy classifier and DOM text may include hidden text.
pub(crate) fn possible_no_ai_instruction(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<_> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let phrases: &[&[&str]] = &[
        &["without", "ai"],
        &["without", "artificial", "intelligence"],
        &["without", "the", "use", "of", "ai"],
        &["without", "the", "use", "of", "artificial", "intelligence"],
        &["do", "not", "use", "ai"],
        &["no", "ai", "assistance"],
        &["no", "ai", "tools"],
    ];
    phrases
        .iter()
        .any(|phrase| words.windows(phrase.len()).any(|window| window == *phrase))
}

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
    pub route_binding: Option<(String, String)>,
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
    route_binding: Option<(String, String)>,
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
fn schema_change_summary(
    before: &waypoint_applications::form_mapping::FormSchema,
    after: &waypoint_applications::form_mapping::FormSchema,
) -> String {
    use std::collections::HashMap;
    let before_fields: HashMap<_, _> = before.fields.iter().map(|f| (&f.selector, f)).collect();
    let after_fields: HashMap<_, _> = after.fields.iter().map(|f| (&f.selector, f)).collect();
    if before_fields.len() != before.fields.len() || after_fields.len() != after.fields.len() {
        return "The field inventory changed or contained repeated identifiers.".into();
    }
    let added = after_fields
        .keys()
        .filter(|key| !before_fields.contains_key(*key))
        .count();
    let removed = before_fields
        .keys()
        .filter(|key| !after_fields.contains_key(*key))
        .count();
    let changed = before_fields
        .iter()
        .filter(|(key, field)| after_fields.get(*key).is_some_and(|next| **field != *next))
        .count();
    let order_changed = before
        .fields
        .iter()
        .map(|f| &f.selector)
        .ne(after.fields.iter().map(|f| &f.selector));
    let structure_changed = before.multi_step != after.multi_step || before.steps != after.steps;
    format!("Inspection field changes: added {added}, removed {removed}, modified {changed}; order {}, step structure {}. No entered values are included.", if order_changed { "changed" } else { "unchanged" }, if structure_changed { "changed" } else { "unchanged" })
}

fn merge_final_inspection(
    preview: &mut form_preview::FormPreview,
    fresh: &waypoint_applications::driver::LiveForm,
    profile: &serde_json::Value,
    initial_walls: &[String],
) -> Result<bool, String> {
    if fresh.origin != preview.schema.origin {
        return Err("The application changed while preparing its review; inspect it again".into());
    }
    if fresh.schema != preview.schema {
        let reason =
            "The form changed during inspection; previous model suggestions were discarded";
        let detail = schema_change_summary(&preview.schema, &fresh.schema);
        let mut updated = form_preview::suggest_without_model(fresh.clone(), profile, reason)?;
        updated.suggestions.clear();
        updated.manual_review.retain(|item| {
            item != &format!(
                "{reason}; only exact labels or standard autocomplete hints are suggested"
            )
        });
        updated.manual_review.push(reason.into());
        updated.manual_review.push(detail);
        form_preview::refresh_required_review(&mut updated);
        updated
            .manual_review
            .push(form_preview::CHANGED_INFORMATIONAL_PREVIEW.into());
        for wall in initial_walls {
            if !updated.manual_review.contains(wall) {
                updated.manual_review.push(wall.clone());
            }
        }
        *preview = updated;
        return Ok(true);
    }
    for wall in &fresh.walls {
        if !preview.manual_review.contains(wall) {
            preview.manual_review.push(wall.clone());
        }
    }
    Ok(false)
}

pub(crate) fn relevant_experiences(
    schema: &waypoint_applications::form_mapping::FormSchema,
    experiences: &[ExperienceMatch],
) -> Vec<ExperienceMatch> {
    use waypoint_applications::form_mapping::FormFieldType as Field;
    use waypoint_workspace::form_experience::{Control, Stage};
    let field_control = |field: &waypoint_applications::form_mapping::FormField| {
        if field.sensitive {
            return None;
        }
        match field.field_type {
            Field::Text
            | Field::Email
            | Field::TextArea
            | Field::Date
            | Field::Month
            | Field::Time
            | Field::DateTimeLocal => Some(Control::Text),
            Field::Select { .. }
            | Field::MultiSelect { .. }
            | Field::Checkbox
            | Field::Radio { .. } => Some(Control::Choice),
            _ => None,
        }
    };
    let mut selected = Vec::new();
    for control in [Control::Text, Control::Choice] {
        let mut fields = schema
            .fields
            .iter()
            .filter(|field| field_control(field) == Some(control))
            .peekable();
        if fields.peek().is_none() {
            continue;
        }
        // Hints are labelled by control type. The top-page origin can rank
        // one type's hints only if none of that type's mapped fields is framed.
        let platform = if fields.any(|field| field.selector.contains("waypoint-frame:")) {
            None
        } else {
            observed_platform(&schema.origin)
        };
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
            // The model sees only the platform relation and opted-in notes.
            // Normalize the rest before selecting so indistinguishable hints
            // cannot consume slots needed by other observations.
            if !item.same_platform {
                item.experience.platform = None;
            }
            if !item.experience.use_note_in_model {
                item.experience.note = None;
            }
        }
        for hint in waypoint_workspace::form_experience::select_experience_hints(candidates) {
            if let Some(original) = experiences
                .iter()
                .find(|item| item.id == hint.id && item.revision == hint.revision)
            {
                let mut original = original.clone();
                original.same_platform = hint.same_platform;
                selected.push(original);
            }
        }
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

#[cfg(test)]
pub fn start(
    id: String,
    job_id: JobIdentityId,
    url: String,
    profile: serde_json::Value,
    model: Result<Option<String>, String>,
    visible: bool,
    review: (i64, Vec<ExperienceMatch>),
) -> Result<(), String> {
    start_bound(
        id,
        job_id,
        url,
        profile,
        model,
        visible,
        (review.0, review.1, None),
    )
}

pub fn start_bound(
    id: String,
    job_id: JobIdentityId,
    url: String,
    profile: serde_json::Value,
    model: Result<Option<String>, String>,
    visible: bool,
    review: (i64, Vec<ExperienceMatch>, Option<(String, String)>),
) -> Result<(), String> {
    let (pending_revision, experiences, route_binding) = review;
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
        route_binding: route_binding.clone(),
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
            let initial_walls = live.walls.clone();
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
                    if informed.conflicting_reports_attempted() {
                        preview.manual_review.push("Reviewed lessons report conflicting outcomes for a similar form issue; verify this form independently.".into());
                    }
                    let (included, omitted) = informed.note_attempts();
                    if included + omitted > 0 {
                        preview.manual_review.push(format!("Across attempted mapping requests, opted-in note inclusions prepared: {included}; omitted by the context budget: {omitted}. A note can be counted more than once. This does not prove model receipt or use."));
                    }
                }
                preview
            } else {
                form_preview::suggest_without_model(live, &profile, &no_model_reason)?
            };
            let fresh = driver.with_review_timeout(|browser| browser.inspect_current())
                .map_err(|e| format!("Final form check failed; inspect it again: {e}"))?;
            if driver.current_url().map_err(|e| e.to_string())? != observed_url {
                return Err("The application changed while preparing its review; inspect it again".into());
            }
            if merge_final_inspection(&mut preview, &fresh, &profile, &initial_walls)? {
                used_revisions.clear();
            }
            if let Some(warning) = navigation_warning {
                preview.manual_review.push(warning);
            }
            match driver.with_review_timeout(|browser| browser.page_text_bounded(32_000)) {
                Ok((text, truncated)) => {
                    if possible_no_ai_instruction(&text) {
                        preview.manual_review.push("The page may ask candidates to answer without AI assistance. Read the original instructions and answer the affected questions yourself; this notice does not identify their scope.".into());
                    }
                    if truncated { preview.manual_review.push("Only part of the page text was checked for employer instructions; review the full page.".into()); }
                }
                Err(_) => preview.manual_review.push("Page text could not be checked for employer instructions; review the original page.".into()),
            }
            if driver.current_url().map_err(|e| e.to_string())? != observed_url {
                return Err("The application changed while checking page instructions; inspect it again".into());
            }
            Ok((driver, preview))
        }))
        .unwrap_or_else(|_| Err("Form inspection stopped unexpectedly".into()));
        let _ = sender.send(Finished {
            job_id,
            pending_revision,
            url,
            route_binding,
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
            route_binding: task.route_binding.clone(),
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
                route_binding: task.route_binding.clone(),
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
    #[ignore = "read-only public application inspection; requires an explicitly supplied URL"]
    fn live_public_form_reports_no_ai_instruction() {
        let url = std::env::var("WAYPOINT_PUBLIC_APPLICATION_URL").unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        crate::form_links::check_job_and_destination(
            crate::manual_job::source(parsed.host_str().unwrap()),
            &parsed.origin().ascii_serialization(),
            waypoint_policy::Operation::Read,
        )
        .unwrap();
        start(
            "notice-check".into(),
            JobIdentityId::new("fictional-notice-check").unwrap(),
            url,
            serde_json::json!({}),
            Ok(None),
            false,
            (0, vec![]),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let finished = loop {
            if let Some(result) = poll("notice-check").unwrap() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let (driver, preview) = finished.result.unwrap();
        assert!(
            preview
                .manual_review
                .iter()
                .any(|warning| warning.contains("without AI assistance")),
            "{:?}",
            preview.manual_review
        );
        assert!(driver.trace.iter().all(|item| !item.contains("typed")
            && !item.contains("clicked")
            && !item.contains("submitted")));
        println!(
            "read-only form fields={} advisory=true",
            preview.schema.fields.len()
        );
    }

    #[test]
    fn no_ai_instruction_advisory_handles_word_boundaries() {
        assert!(possible_no_ai_instruction(
            "Please answer yourself, without AI assistance."
        ));
        assert!(possible_no_ai_instruction("Do not use AI tools."));
        assert!(possible_no_ai_instruction(
            "WITHOUT the use of artificial intelligence"
        ));
        assert!(!possible_no_ai_instruction(
            "We use AI tools to review applications."
        ));
        assert!(!possible_no_ai_instruction(
            "This role requires maintaining data pipelines."
        ));
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
    #[ignore = "starts a fictional browser and requires a resource-gated resident local model"]
    fn live_model_preview_rechecks_controlled_form_change() {
        use waypoint_applications::form_mapping::BrowserCommand;
        use waypoint_inference::{CapabilityReport, Inference, InferenceError, UsageReport};
        struct Counting<I> {
            inner: I,
            completed: usize,
        }
        impl<I: Inference> Inference for Counting<I> {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                self.inner.probe()
            }
            fn set_context_tags(&mut self, tags: &[&str]) {
                self.inner.set_context_tags(tags);
            }
            fn generate_structured(
                &mut self,
                schema: &str,
                prompt: &str,
                max: u32,
            ) -> Result<(serde_json::Value, UsageReport), InferenceError> {
                let result = self.inner.generate_structured(schema, prompt, max)?;
                self.completed += 1;
                Ok(result)
            }
        }
        let model = crate::resident_model()
            .unwrap()
            .expect("eligible resident model");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-controlled-change.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(path).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let initial = driver.open(url.as_str()).unwrap();
        assert!(!initial.walls.iter().any(|wall| wall == "captcha present"));
        let original_walls = initial.walls.clone();
        let llm = OllamaInference::new("http://127.0.0.1:11434", &model)
            .unwrap()
            .with_resident_context();
        let mut llm = Counting {
            inner: crate::resident_inference::ResidentInference::new(
                llm,
                model,
                crate::resident_model,
            ),
            completed: 0,
        };
        let profile = serde_json::json!({"first_name":"Avery","email":"avery@example.invalid"});
        let mut preview = form_preview::suggest_for_live_form(initial, &mut llm, &profile).unwrap();
        assert!(llm.completed > 0, "a local model response is required");
        driver
            .execute(&BrowserCommand::Click {
                selector: "#change".into(),
            })
            .unwrap();
        let fresh = driver.inspect_current().unwrap();
        assert!(merge_final_inspection(&mut preview, &fresh, &profile, &original_walls).unwrap());
        assert!(preview
            .schema
            .fields
            .iter()
            .any(|field| field.label == "Additional question"));
        assert!(preview.suggestions.is_empty());
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "captcha present"));
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == form_preview::CHANGED_INFORMATIONAL_PREVIEW));
        for label in ["First name", "Email", "Additional question"] {
            assert!(preview
                .manual_review
                .iter()
                .any(|reason| reason == &format!("Required field needs review: {label}")));
        }
        assert!(!form_preview::same_reviewable_walls(&preview, &[]));
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("typed") && !entry.contains("submitted")));
    }

    #[test]
    fn final_inspection_preserves_late_walls_and_blocks_changed_schema() {
        use waypoint_applications::{
            driver::LiveForm,
            form_mapping::{FormField, FormFieldType, FormSchema},
        };
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
        let profile = serde_json::json!({"first_name":"Avery"});
        let initial = vec!["earlier wall".into()];
        assert!(!merge_final_inspection(&mut preview, &fresh, &profile, &initial).unwrap());
        assert!(preview
            .manual_review
            .iter()
            .any(|wall| wall == "captcha present"));
        assert!(preview
            .manual_review
            .iter()
            .any(|wall| wall == "earlier wall"));
        merge_final_inspection(&mut preview, &fresh, &profile, &initial).unwrap();
        assert_eq!(
            preview
                .manual_review
                .iter()
                .filter(|wall| *wall == "captcha present")
                .count(),
            1
        );
        fresh.schema.multi_step = true;
        fresh.schema.fields.push(FormField {
            selector: "#first".into(),
            label: "First name".into(),
            field_type: FormFieldType::Text,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: Some("given-name".into()),
        });
        assert!(merge_final_inspection(&mut preview, &fresh, &profile, &initial).unwrap());
        assert_eq!(preview.schema, fresh.schema);
        assert!(preview.suggestions.is_empty());
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "Required field needs review: First name"));
        assert!(!preview.manual_review.iter().any(|reason| reason
            .contains("only exact labels or standard autocomplete hints are suggested")));
        assert!(preview
            .manual_review
            .iter()
            .any(|wall| wall == form_preview::CHANGED_INFORMATIONAL_PREVIEW));
        assert!(preview
            .manual_review
            .iter()
            .any(|wall| wall.contains("added 1, removed 0, modified 0")
                && wall.contains("step structure changed")));
        let mut altered = fresh.schema.clone();
        altered.fields[0].label = "Confidential example".into();
        let detail = schema_change_summary(&fresh.schema, &altered);
        assert!(detail.contains("added 0, removed 0, modified 1"));
        assert!(!detail.contains("Confidential example"));
        assert!(preview
            .manual_review
            .iter()
            .any(|wall| wall == "earlier wall"));
        assert!(!form_preview::same_reviewable_walls(&preview, &[]));
        fresh.schema = preview.schema.clone();
        fresh.origin = "https://other.invalid".into();
        assert!(merge_final_inspection(&mut preview, &fresh, &profile, &initial).is_err());
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
            .map(|i| {
                let mut entry = item(i, Control::Text, Platform::Greenhouse, 100 + i as i64);
                entry.experience.observation = match i % 5 {
                    0 => Observation::AmbiguousLabel,
                    1 => Observation::ValueChanged,
                    2 => Observation::ValidationError,
                    3 => Observation::HiddenControl,
                    _ => Observation::UnexpectedNavigation,
                };
                entry
            })
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
        let mut embedded = schema.clone();
        embedded.fields[0].selector = "waypoint-frame:12:34:input[name=given]".into();
        let mixed = relevant_experiences(&embedded, &items);
        assert!(mixed
            .iter()
            .filter(|value| value.experience.control == Control::Text)
            .all(|value| !value.same_platform));
        assert!(mixed
            .iter()
            .any(|value| value.experience.control == Control::Choice && value.same_platform));
        embedded.fields[0].selector =
            "waypoint-shadow:7:8:waypoint-frame:12:34:input[name=given]".into();
        assert!(relevant_experiences(&embedded, &items)
            .iter()
            .filter(|value| value.experience.control == Control::Text)
            .all(|value| !value.same_platform));
        embedded.fields[0].selector = "waypoint-shadow:7:8:input[name=given]".into();
        assert!(relevant_experiences(&embedded, &items)
            .iter()
            .any(|value| value.same_platform));
        embedded.fields[1].selector = "waypoint-frame:12:34:input[type=checkbox]".into();
        let mixed = relevant_experiences(&embedded, &items);
        assert!(mixed
            .iter()
            .any(|value| value.experience.control == Control::Text && value.same_platform));
        assert!(mixed
            .iter()
            .filter(|value| value.experience.control == Control::Choice)
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
                let mut entry = item(
                    i,
                    if i < 250 {
                        Control::Text
                    } else {
                        Control::Choice
                    },
                    Platform::Greenhouse,
                    i as i64,
                );
                entry.experience.observation = match i % 5 {
                    0 => Observation::AmbiguousLabel,
                    1 => Observation::ValueChanged,
                    2 => Observation::ValidationError,
                    3 => Observation::HiddenControl,
                    _ => Observation::UnexpectedNavigation,
                };
                entry
            })
            .collect();
        full[0].experience.platform = Some(Platform::Lever);
        full[250].experience.platform = Some(Platform::Lever);
        let selected = relevant_experiences(&schema, &full);
        assert_eq!(selected.len(), 10);
        assert!(selected[0].same_platform && selected[5].same_platform);
        let mut reports: Vec<_> = (0..7)
            .map(|i| item(i, Control::Text, Platform::Lever, i as i64 + 1))
            .collect();
        reports[0].experience.outcome = Outcome::DidNotHelp;
        reports[1].experience.outcome = Outcome::Helped;
        for (index, report) in reports.iter_mut().enumerate().skip(2) {
            report.experience.observation = match index {
                2 => Observation::ValueChanged,
                3 => Observation::ValidationError,
                4 => Observation::HiddenControl,
                5 => Observation::UnexpectedNavigation,
                _ => Observation::AmbiguousLabel,
            };
            report.experience.response = Response::StopAndReview;
        }
        let selected = relevant_experiences(&schema, &reports);
        assert!(selected
            .iter()
            .any(|item| item.experience.outcome == Outcome::Helped));
        assert!(selected
            .iter()
            .any(|item| item.experience.outcome == Outcome::DidNotHelp));
        let mut private_duplicates: Vec<_> = (0..7)
            .map(|i| {
                let mut entry = item(
                    i,
                    Control::Text,
                    if i % 2 == 0 {
                        Platform::Ashby
                    } else {
                        Platform::Greenhouse
                    },
                    100 + i as i64,
                );
                entry.experience.note = Some(format!("private context {i}"));
                entry
            })
            .collect();
        for i in 7..9 {
            let mut entry = item(i, Control::Text, Platform::Ashby, i as i64);
            entry.experience.note = Some(format!("permitted context {i}"));
            entry.experience.use_note_in_model = true;
            private_duplicates.push(entry);
        }
        let selected = relevant_experiences(&schema, &private_duplicates);
        assert_eq!(selected.len(), 3);
        assert_eq!(
            selected
                .iter()
                .filter(|item| item.experience.use_note_in_model)
                .count(),
            2
        );
        assert!(selected
            .iter()
            .any(|item| item.experience.note.as_deref() == Some("private context 6")));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "loads a local model after the resource gate using fictional fields"]
    fn local_model_maps_embedded_fields_with_unestablished_lesson_platform() {
        use waypoint_applications::form_mapping::{
            FormField, FormFieldType, FormSchema, PacketValue,
        };
        use waypoint_workspace::form_experience::{
            Control, Experience, ExperienceMatch, Observation, Outcome, Platform, Response, Stage,
        };
        crate::experience_inference::tests::with_resource_gated_model(|| {
            let field = |selector: &str, label: &str, kind| FormField {
                selector: selector.into(),
                label: label.into(),
                field_type: kind,
                required: true,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                min_length_utf16: None,
                pattern: None,
                autocomplete: None,
            };
            let schema = FormSchema {
                origin: "https://jobs.lever.co".into(),
                fields: vec![
                    field("#given", "Your given name", FormFieldType::Text),
                    field(
                        "waypoint-frame:12:34:input[name=email]",
                        "Email address",
                        FormFieldType::Email,
                    ),
                    field(
                        "#country",
                        "Country",
                        FormFieldType::Select {
                            options: vec!["Germany".into(), "France".into()],
                        },
                    ),
                ],
                multi_step: false,
                steps: 1,
            };
            let lesson = ExperienceMatch {
                id: "00000000000000000000000000000001".into(),
                revision: 1,
                same_platform: false,
                experience: Experience {
                    version: 1,
                    reviewed: true,
                    enabled: true,
                    observed_at: 1,
                    platform: Some(Platform::Lever),
                    stage: Stage::EnterInformation,
                    control: Control::Text,
                    observation: Observation::AmbiguousLabel,
                    response: Response::InspectContext,
                    outcome: Outcome::Unknown,
                    note: None,
                    use_note_in_model: false,
                },
            };
            let mut choice_lesson = lesson.clone();
            choice_lesson.id = "00000000000000000000000000000002".into();
            choice_lesson.experience.control = Control::Choice;
            let selected = relevant_experiences(&schema, &[lesson, choice_lesson]);
            assert_eq!(selected.len(), 2);
            assert!(!selected[0].same_platform);
            assert!(selected[1].same_platform);
            let model = crate::resident_model().unwrap().unwrap();
            let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", &model)
                .unwrap()
                .with_resident_context();
            let mut llm = crate::resident_inference::ResidentInference::new(
                llm,
                model,
                crate::resident_model,
            );
            let mut experienced =
                crate::experience_inference::ExperienceInference::new(&mut llm, &selected);
            let (mappings, _, _) = crate::suggestions::suggest_with_review(
                &mut experienced,
                &schema,
                &serde_json::json!({"first_name":"Avery","email":"avery@example.invalid","country":"Germany"}),
            )
            .unwrap();
            assert!(experienced.attempted_with_hints());
            assert!(mappings
                .iter()
                .any(|m| m.field_selector == "#given"
                    && m.value == PacketValue::Text("Avery".into())));
            assert!(mappings.iter().any(|m| m.field_selector
                == "waypoint-frame:12:34:input[name=email]"
                && m.value == PacketValue::Text("avery@example.invalid".into())));
            assert!(mappings.iter().any(|m| m.field_selector == "#country"
                && m.value == PacketValue::Text("Germany".into())));
        });
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
            route_binding: None,
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
            route_binding: None,
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
            route_binding: None,
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
        public_worker_recheck(
            Err("model intentionally not started".into()),
            serde_json::json!({}),
        );
    }

    fn public_worker_recheck(model: Result<Option<String>, String>, profile: serde_json::Value) {
        let _guard = TEST_LOCK.lock().unwrap();
        let expected_model = matches!(&model, Ok(Some(_)));
        let expected_preferred = profile
            .get("preferred_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
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
            profile,
            model,
            false,
            (0, vec![]),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(150);
        loop {
            if let Some(finished) = poll("public-worker-fixture").unwrap() {
                match finished.result {
                    Ok((driver, preview)) => {
                        println!(
                            "review returned: fields={} required={} suggestions={} warnings={} captcha={} informational={}",
                            preview.schema.fields.len(),
                            preview.schema.fields.iter().filter(|field| field.required).count(),
                            preview.suggestions.len(),
                            preview.manual_review.len(),
                            preview
                                .manual_review
                                .iter()
                                .any(|wall| wall == "captcha present"),
                            preview
                                .manual_review
                                .iter()
                                .any(|wall| wall == form_preview::CHANGED_INFORMATIONAL_PREVIEW)
                        );
                        println!("review warnings: {:?}", preview.manual_review);
                        if expected_model
                            && !preview
                                .manual_review
                                .iter()
                                .any(|reason| reason == form_preview::CHANGED_INFORMATIONAL_PREVIEW)
                        {
                            assert!(
                                !preview.suggestions.is_empty(),
                                "no local-model mapping was returned"
                            );
                            assert!(!preview.manual_review.iter().any(|reason| reason.contains("only exact labels or standard autocomplete hints are suggested")), "mapping used the deterministic fallback");
                        }
                        for field in &preview.schema.fields {
                            println!(
                                "field: required={} sensitive={} label={:?}",
                                field.required, field.sensitive, field.label
                            );
                        }
                        for mapping in &preview.suggestions {
                            let field = preview
                                .schema
                                .fields
                                .iter()
                                .find(|field| field.selector == mapping.field_selector)
                                .expect("suggestion references a visible field");
                            assert!(!field.sensitive, "sensitive field was suggested");
                            println!("suggested field: {:?}", field.label);
                            if expected_model {
                                use waypoint_applications::form_mapping::PacketValue;
                                let expected = if field.label.starts_with("Full name")
                                    || field.label == "Full legal name"
                                {
                                    "Avery Example"
                                } else if field.label.starts_with("Preferred name") {
                                    expected_preferred
                                        .as_deref()
                                        .expect("preferred name must be provided")
                                } else if field.label.starts_with("Email") {
                                    "avery@example.invalid"
                                } else if field.label.starts_with("Phone") {
                                    "+1 202 555 0101"
                                } else {
                                    panic!("unexpected suggestion for {:?}", field.label);
                                };
                                assert_eq!(
                                    mapping.value,
                                    PacketValue::Text(expected.into()),
                                    "incorrect fictional value for {:?}",
                                    field.label
                                );
                            }
                        }
                        if expected_preferred.is_some() {
                            assert!(
                                preview.suggestions.iter().any(|mapping| preview
                                    .schema
                                    .fields
                                    .iter()
                                    .any(|field| field.selector == mapping.field_selector
                                        && field.label.starts_with("Preferred name"))),
                                "preferred name was not suggested"
                            );
                        }
                        if preview
                            .manual_review
                            .iter()
                            .any(|wall| wall == form_preview::CHANGED_INFORMATIONAL_PREVIEW)
                        {
                            assert!(preview.suggestions.is_empty());
                            assert!(!form_preview::same_reviewable_walls(&preview, &[]));
                        }
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

    #[cfg(windows)]
    #[test]
    #[ignore = "resource-gated local model and read-only public application"]
    fn live_public_worker_with_resource_gated_model() {
        let id = "fictional-public-model-evaluation";
        crate::model_start::start(id.into(), "qwen3.5:9b".into()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(150);
        let result = loop {
            let result = crate::model_start::poll(id).unwrap();
            if result["status"] != "running" {
                break result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "startup timed out; inspect runtime"
            );
            std::thread::sleep(std::time::Duration::from_millis(250));
        };
        assert_eq!(result["status"], "complete", "{result}");
        struct Unload(bool);
        impl Drop for Unload {
            fn drop(&mut self) {
                if self.0 {
                    let _ = waypoint_inference::ollama::http_request(
                        "127.0.0.1:11434".parse().unwrap(),
                        "POST",
                        "/api/generate",
                        Some(br#"{"model":"qwen3.5:9b","keep_alive":0}"#),
                        10_000,
                    );
                }
            }
        }
        let _unload = Unload(result["model"]["already_running"] != true);
        let mut profile = serde_json::json!({"full_name":"Avery Example","email":"avery@example.invalid","phone":"+1 202 555 0101","city":"Fictional City"});
        if std::env::var("WAYPOINT_TEST_PREFERRED_NAME").as_deref() == Ok("1") {
            profile["preferred_name"] = serde_json::json!("Ace");
        }
        public_worker_recheck(Ok(Some("qwen3.5:9b".into())), profile);
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional local fixture"]
    fn background_inspection_returns_the_live_form_without_filling() {
        let _guard = TEST_LOCK.lock().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        start_bound(
            "inspection-fixture".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({}),
            Err("fictional memory guard failure".into()),
            false,
            (
                0,
                vec![],
                Some(("ashby".into(), "https://example.invalid/listing".into())),
            ),
        )
        .unwrap();
        assert!(busy());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(finished) = poll("inspection-fixture").unwrap() {
                assert_eq!(
                    finished.route_binding,
                    Some(("ashby".into(), "https://example.invalid/listing".into()))
                );
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

    #[test]
    #[ignore = "read-only live Ashby form inspection; requires a reviewed URL"]
    fn source_route_live_inspection_does_not_fill_or_submit() {
        let _guard = TEST_LOCK.lock().unwrap();
        let route = std::env::var("WAYPOINT_LIVE_CHECK_URL").unwrap();
        let parsed = url::Url::parse(&route).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert_eq!(parsed.host_str(), Some("jobs.ashbyhq.com"));
        waypoint_net::resolve_public(&waypoint_net::parse_url(&route).unwrap()).unwrap();
        let listing = route
            .strip_suffix("/application")
            .expect("listing route")
            .to_owned();
        start_bound(
            "live-source-form".into(),
            JobIdentityId::new("fictional-job").unwrap(),
            route,
            serde_json::json!({}),
            Ok(None),
            false,
            (0, vec![], Some(("ashby".into(), listing.clone()))),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let finished = loop {
            if let Some(done) = poll("live-source-form").unwrap() {
                break done;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(finished.route_binding, Some(("ashby".into(), listing)));
        let (driver, preview) = finished.result.unwrap();
        println!(
            "Fields: {}; suggestions: {}; review reasons: {}",
            preview.schema.fields.len(),
            preview.suggestions.len(),
            preview.manual_review.len()
        );
        assert!(driver.trace.iter().all(|line| !line.contains("typed")
            && !line.contains("clicked")
            && !line.contains("submitted")));
    }
}
