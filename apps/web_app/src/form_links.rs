//! User-selected navigation to an application link in the current page.
use crate::{form_preview, manual_job, ReviewSession};
use std::sync::{mpsc, Mutex};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::Sha256Digest;
use waypoint_inference::OllamaInference;
use waypoint_policy::Operation;

type NavigationResult = Result<(ReviewSession, Vec<(String, u64)>), NavigationFailure>;

struct Task {
    id: String,
    job_id: waypoint_domain::JobIdentityId,
    inspection_task_id: String,
    pending_revision: i64,
    rx: mpsc::Receiver<NavigationResult>,
    failure: Option<NavigationFailure>,
}
#[derive(Clone)]
pub struct NavigationFailure {
    pub job_id: waypoint_domain::JobIdentityId,
    pub reason: String,
    pub review_saved: bool,
    pub pending_revision: i64,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED_FAILURE: Mutex<Option<(String, NavigationFailure)>> = Mutex::new(None);

fn model_unavailable_reason(status: Result<Option<String>, String>) -> String {
    match status {
        Err(error) => format!("Local model unavailable: {error}"),
        Ok(None) => "No eligible local model is loaded".into(),
        Ok(Some(_)) => unreachable!("a resident model is not an unavailable status"),
    }
}

pub fn pending_reason(task_id: &str) -> String {
    format!("Application navigation pending: {task_id}")
}

pub fn begin_pending(
    ws: &waypoint_workspace::Workspace,
    job: &waypoint_domain::JobIdentityId,
    task_id: &str,
    timestamp: i64,
) -> Result<i64, String> {
    let reason = pending_reason(task_id);
    ws.mark_manual_review(job, &reason, timestamp)
        .map_err(|e| format!("Could not save pending navigation: {e}"))?;
    ws.manual_review_reasons(job)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|entry| entry.reason == reason)
        .map(|entry| entry.revision)
        .ok_or_else(|| "Pending navigation was not found after saving".into())
}

pub fn finish_pending(
    ws: &waypoint_workspace::Workspace,
    job: &waypoint_domain::JobIdentityId,
    task_id: &str,
    revision: i64,
) -> Result<bool, String> {
    ws.clear_manual_review_reason(job, &pending_reason(task_id), revision)
        .map_err(|e| format!("Could not clear pending navigation: {e}"))
}

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

/// Recover a navigation whose start response was lost while its session is
/// temporarily owned by the worker.
pub fn pending() -> Result<Option<(String, String)>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Navigation unavailable")?
        .as_ref()
        .map(|task| (task.id.clone(), task.job_id.as_str().to_owned())))
}

pub(crate) fn protected_reasons(
    job: &waypoint_domain::JobIdentityId,
) -> Result<Vec<String>, String> {
    let task = TASK.lock().map_err(|_| "Navigation unavailable")?;
    Ok(task
        .as_ref()
        .filter(|task| &task.job_id == job)
        .map_or_else(Vec::new, |task| {
            vec![
                pending_reason(&task.id),
                crate::form_task::pending_reason(&task.inspection_task_id),
            ]
        }))
}

fn permitted(url: &url::Url, operation: Operation) -> Result<(), String> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !waypoint_connectors::is_public_host_url(url.as_str())
    {
        return Err("Destination must be public HTTPS".into());
    }
    let source = manual_job::source(url.host_str().ok_or("Destination has no host")?);
    waypoint_workspace::default_registry()
        .check_with_review(source, operation, true)
        .map_err(|e| e.to_string())
}

pub fn check_destination(origin: &str, operation: Operation) -> Result<(), String> {
    let parsed = url::Url::parse(origin).map_err(|_| "Invalid destination")?;
    permitted(&parsed, operation)
}

pub fn check_job_and_destination(
    source: &str,
    origin: &str,
    operation: Operation,
) -> Result<(), String> {
    if !origin.starts_with("https://") {
        return waypoint_workspace::default_registry()
            .check_with_review(source, operation, true)
            .map_err(|e| e.to_string());
    }
    let destination = url::Url::parse(origin).map_err(|_| "Invalid destination")?;
    let destination_source =
        manual_job::source(destination.host_str().ok_or("Destination has no host")?);
    let original_operation = if source == "jobicy" && destination_source != "jobicy" {
        Operation::Read
    } else {
        operation
    };
    waypoint_workspace::default_registry()
        .check_with_review(source, original_operation, true)
        .map_err(|e| e.to_string())?;
    permitted(&destination, operation)
}

pub fn candidates(driver: &mut dyn BrowserDriver) -> Result<Vec<String>, String> {
    Ok(filtered_candidates(driver, false)?
        .into_iter()
        .map(|link| link.url)
        .collect())
}

pub fn other_candidates(
    driver: &mut dyn BrowserDriver,
) -> Result<Vec<waypoint_applications::driver::PageLink>, String> {
    filtered_candidates(driver, true)
}

/// Preserve link-read failures as review obligations instead of treating them as empty pages.
pub fn links_for_review(
    driver: &mut dyn BrowserDriver,
    review: &mut Vec<String>,
) -> (
    Vec<String>,
    Vec<waypoint_applications::driver::PageLink>,
    bool,
) {
    let likely = match candidates(driver) {
        Ok(links) => {
            if links.len() == 10 {
                review.push("The likely application-link limit was reached; additional links may be missing".into());
            }
            links
        }
        Err(error) if error == LINK_SCAN_LIMIT => {
            review.push("The page link-inspection limit was reached; navigate manually".into());
            Vec::new()
        }
        Err(_) => {
            review
                .push("Likely application links could not be inspected; navigate manually".into());
            Vec::new()
        }
    };
    let (other, available) = match other_candidates(driver) {
        Ok(links) => {
            if links.len() == 30 {
                review.push(
                    "The other-link limit was reached; additional links may be missing".into(),
                );
            }
            (links, true)
        }
        Err(error) if error == LINK_SCAN_LIMIT => {
            if !review.iter().any(|reason| {
                reason == "The page link-inspection limit was reached; navigate manually"
            }) {
                review.push("The page link-inspection limit was reached; navigate manually".into());
            }
            (Vec::new(), false)
        }
        Err(_) => {
            review.push("Other page links could not be inspected; navigate manually".into());
            (Vec::new(), false)
        }
    };
    (likely, other, available)
}

const LINK_SCAN_LIMIT: &str = "The page link-inspection limit was reached";

fn filtered_candidates(
    driver: &mut dyn BrowserDriver,
    other: bool,
) -> Result<Vec<waypoint_applications::driver::PageLink>, String> {
    let current = url::Url::parse(&driver.current_url().map_err(|e| e.to_string())?)
        .map_err(|_| "Invalid current URL")?;
    let mut result = Vec::new();
    let page_links = driver.page_links().map_err(|e| e.to_string())?;
    // The browser caps its inventory at 200. At the cap, a later anchor may
    // share a destination and carry a final-action label we did not observe.
    // Refuse reviewed navigation instead of presenting an incomplete filter.
    if page_links.len() >= waypoint_applications::driver::PAGE_LINK_LIMIT {
        return Err(LINK_SCAN_LIMIT.into());
    }
    let likely = if other {
        None
    } else {
        Some(driver.application_links().map_err(|e| e.to_string())?)
    };
    // Multiple visible anchors may share a destination with different labels.
    // If any label suggests a final action, exclude that destination entirely.
    let blocked: std::collections::HashSet<String> = page_links
        .iter()
        .filter(|link| final_action_label(&link.label))
        .filter_map(|link| current.join(&link.url).ok().map(|url| url.to_string()))
        .collect();
    let hrefs = page_links.into_iter().filter(|link| {
        likely
            .as_ref()
            .is_none_or(|items| items.contains(&link.url))
    });
    for link in hrefs.take(waypoint_applications::driver::PAGE_LINK_LIMIT) {
        let Ok(url) = current.join(&link.url) else {
            continue;
        };
        // Hash-based applications use the fragment as a route. Removing it
        // changes the reviewed destination and can reopen the wrong section.
        // Include it when excluding obvious final-action links as well.
        let route = format!(
            "{}?{}#{}",
            url.path(),
            url.query().unwrap_or(""),
            url.fragment().unwrap_or("")
        )
        .to_ascii_lowercase();
        if ["submit", "confirm", "complete", "finish", "send"]
            .iter()
            .any(|word| route.contains(word))
            || permitted(&url, Operation::Read).is_err()
        {
            continue;
        }
        let target = url.to_string();
        if blocked.contains(&target) {
            continue;
        }
        if !result
            .iter()
            .any(|link: &waypoint_applications::driver::PageLink| link.url == target)
        {
            result.push(waypoint_applications::driver::PageLink {
                url: target,
                label: link.label,
            });
        }
        if result.len() == if other { 30 } else { 10 } {
            break;
        }
    }
    Ok(result)
}

fn final_action_label(label: &str) -> bool {
    let text = label.to_lowercase();
    [
        "submit",
        "send application",
        "finish application",
        "confirm application",
        "complete application",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
}

fn reviewed_link_matches(
    link: &waypoint_applications::driver::PageLink,
    target: &str,
    label: &str,
) -> bool {
    link.url == target && link.label == label
}

const PRIOR_FIELDS_REVIEW: &str = "Fields filled before this inspection must be reviewed again; their earlier verification does not apply to the new inspection";
const PRIOR_MISSING_FIELDS_REVIEW: &str = "a custom control needs manual review: An earlier inspection found fields that disappeared while scrolling. Review the entire application; this inspection may still be incomplete.";
const PRIOR_CHOICES_REVIEW: &str = "Custom choices from an earlier inspection cannot be rechecked here; verify them in the application before submitting";
const PRIOR_CAPTURE_REVIEW: &str =
    "An earlier application capture was incomplete; the application record may be missing evidence";

fn mark_prior_capture_for_review(
    preview: &mut form_preview::FormPreview,
    previous_review: &[String],
) {
    let incomplete = previous_review.iter().any(|reason| {
        reason == PRIOR_CAPTURE_REVIEW
            || reason.starts_with("Inspection evidence incomplete:")
            || reason.starts_with("The previous section could not be fully captured before navigation:")
            || reason == "The previous section could not be captured because the application record is unavailable"
            || reason == "Attempt screenshot capture was incomplete"
    });
    if incomplete
        && !preview
            .manual_review
            .iter()
            .any(|reason| reason == PRIOR_CAPTURE_REVIEW)
    {
        preview.manual_review.push(PRIOR_CAPTURE_REVIEW.into());
    }
}

fn mark_prior_choices_for_review(
    preview: &mut form_preview::FormPreview,
    choices: &mut Vec<(String, String)>,
    previous_review: &[String],
) {
    if (!choices.is_empty()
        || previous_review
            .iter()
            .any(|reason| reason == PRIOR_CHOICES_REVIEW))
        && !preview
            .manual_review
            .iter()
            .any(|reason| reason == PRIOR_CHOICES_REVIEW)
    {
        preview.manual_review.push(PRIOR_CHOICES_REVIEW.into());
    }
    choices.clear();
}

fn mark_prior_fills_for_review(
    preview: &mut form_preview::FormPreview,
    completed: &[String],
    previous_review: &[String],
) {
    if (!completed.is_empty()
        || previous_review
            .iter()
            .any(|reason| reason == PRIOR_FIELDS_REVIEW))
        && !preview
            .manual_review
            .iter()
            .any(|reason| reason == PRIOR_FIELDS_REVIEW)
    {
        preview.manual_review.push(PRIOR_FIELDS_REVIEW.into());
    }
    if previous_review.iter().any(|reason| {
        reason == PRIOR_MISSING_FIELDS_REVIEW
            || (reason.starts_with("a custom control needs manual review: ")
                && reason.contains("field(s) seen earlier are absent from this inspection"))
    }) && !preview
        .manual_review
        .iter()
        .any(|reason| reason == PRIOR_MISSING_FIELDS_REVIEW)
    {
        preview
            .manual_review
            .push(PRIOR_MISSING_FIELDS_REVIEW.into());
    }
}

fn recheck_navigation_preview(
    driver: &mut waypoint_browser::CdpDriver,
    preview: &mut form_preview::FormPreview,
    profile: &serde_json::Value,
    observed_url: &str,
    initial_walls: &[String],
) -> Result<bool, String> {
    let fresh = driver
        .with_review_timeout(|browser| browser.inspect_current())
        .map_err(|e| format!("Final form check failed; inspect it again: {e}"))?;
    if driver.current_url().map_err(|e| e.to_string())? != observed_url {
        return Err("The application changed while preparing its review; inspect it again".into());
    }
    let changed =
        crate::form_task::merge_final_inspection(preview, &fresh, profile, initial_walls)?;
    match driver.with_review_timeout(|browser| browser.page_text_bounded(32_000)) {
        Ok((text, truncated)) => {
            if crate::form_task::possible_no_ai_instruction(&text) {
                preview.manual_review.push("The page may ask candidates to answer without AI assistance. Read the original instructions and answer the affected questions yourself; this notice does not identify their scope.".into());
            }
            if truncated {
                preview.manual_review.push("Only part of the page text was checked for employer instructions; review the full page.".into());
            }
        }
        Err(_) => preview.manual_review.push(
            "Page text could not be checked for employer instructions; review the original page."
                .into(),
        ),
    }
    if driver.current_url().map_err(|e| e.to_string())? != observed_url {
        return Err(
            "The application changed while checking page instructions; inspect it again".into(),
        );
    }
    Ok(changed)
}

// Each section is separately capped before it is retained. Sixteen sections
// support longer wizards while keeping the serialized input to at most 4 MiB;
// in-memory object overhead can be higher.
const MAX_PRIOR_SECTIONS: usize = 16;
const MAX_PRIOR_SECTION_BYTES: usize = 256 * 1024;

fn remember_prior_section(session: &mut ReviewSession) {
    let attachment_groups =
        crate::current_attachment_groups(&session.attachments, &session.id).len();
    if session.completed_selectors.is_empty() && attachment_groups == 0 {
        return;
    }
    let suggestions: Vec<_> = session
        .preview
        .suggestions
        .iter()
        .filter(|mapping| {
            session
                .completed_selectors
                .contains(&mapping.field_selector)
        })
        .cloned()
        .collect();
    let size = serde_json::to_vec(&serde_json::json!({"schema":session.preview.schema,"suggestions":suggestions,"completed":session.completed_selectors})).map(|bytes| bytes.len());
    if size.is_err() || size.is_ok_and(|bytes| bytes > MAX_PRIOR_SECTION_BYTES) {
        session.prior_sections_omitted = true;
        return;
    }
    if session.prior_sections.len() >= MAX_PRIOR_SECTIONS {
        session.prior_sections.remove(0);
        session.prior_sections_omitted = true;
    }
    session.prior_sections.push(crate::PriorSection {
        preview: form_preview::FormPreview {
            schema: session.preview.schema.clone(),
            suggestions,
            manual_review: vec![],
            model_review_selectors: Default::default(),
        },
        completed: session.completed_selectors.clone(),
        profile_digest: session.profile_digest.clone(),
        inspection_id: session.id.clone(),
        attachment_groups,
    });
}

fn prior_field_number(
    schema: &waypoint_applications::form_mapping::FormSchema,
    selector: &str,
) -> Option<(usize, usize)> {
    let mut matches = schema
        .fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.selector == selector);
    let (index, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some((index + 1, schema.fields.len()))
}

fn review_prior_section(
    session: &mut ReviewSession,
    preview: &mut form_preview::FormPreview,
    profile: &serde_json::Value,
) {
    if session.prior_sections_omitted {
        preview.manual_review.push("Some earlier filled sections exceeded the in-memory review limit; inspect them manually.".into());
    }
    let matches: Vec<_> = session
        .prior_sections
        .iter()
        .filter(|section| section.preview.schema == preview.schema)
        .collect();
    let [section] = matches.as_slice() else {
        if matches.len() > 1 {
            preview.manual_review.push(
                "Multiple earlier filled sections have this same layout; inspect them manually."
                    .into(),
            );
        }
        return;
    };
    if section.profile_digest != waypoint_domain::Sha256Digest::of(&profile.to_string()) {
        preview.manual_review.push("The profile changed after an earlier section was filled; review its answers against the current profile.".into());
    }
    let before = session.driver.current_url().ok();
    let check = section.completed.first().map(|first| {
        session.driver.with_review_timeout(|driver| {
            form_preview::verify_completed_fields(
                driver,
                &section.preview,
                &section.completed,
                first,
            )
        })
    });
    let groups = crate::current_attachment_groups(&session.attachments, &section.inspection_id);
    let mut verified_attachments = 0usize;
    if groups.len() == section.attachment_groups && groups.len() <= 8 {
        let started = std::time::Instant::now();
        for (selector, paths) in &groups {
            if started.elapsed() >= std::time::Duration::from_secs(4)
                || !crate::staged_attachments_unchanged(
                    &session.attachments,
                    &section.inspection_id,
                    selector,
                )
                || session
                    .driver
                    .with_review_timeout(|driver| driver.verify_local_files(selector, paths))
                    .is_err()
            {
                break;
            }
            verified_attachments += 1;
        }
    }
    let schema_current = session
        .driver
        .with_review_timeout(|driver| driver.inspect_current())
        .is_ok_and(|current| current.schema == preview.schema);
    let after = session.driver.current_url().ok();
    let label = if before.is_none() || before != after || !schema_current {
        "A possible earlier section changed or could not be rechecked during verification; inspect its answers manually.".to_owned()
    } else if check
        .as_ref()
        .is_none_or(|check| check.failed_labels.is_empty() && check.unverified == 0)
    {
        format!("A possible earlier section has the same form layout; browser readback matched {} recorded field value(s) from this review session. This is not a visual check and does not prove it is the same section or that the values remain correct; review them manually.", check.as_ref().map_or(0, |check| check.verified))
    } else {
        let verified = check.as_ref().map_or(0, |check| check.verified);
        format!("A possible earlier section has the same form layout; {} field(s) matched, {} could not be verified or changed. Inspect the answers manually.", verified, section.completed.len().saturating_sub(verified))
    };
    if !section.completed.is_empty() || before.is_none() || before != after || !schema_current {
        preview.manual_review.push(label);
    }
    if before.is_some() && before == after && schema_current {
        if let Some(check) = &check {
            if !check.failed_labels.is_empty() || check.unverified > 0 {
                if let Some((index, total)) = section
                    .completed
                    .get(check.verified)
                    .and_then(|selector| prior_field_number(&preview.schema, selector))
                {
                    preview.manual_review.push(format!("Start reviewing at field {index} of {total} in this section's field list; later fields may not have been checked."));
                }
            }
        }
    }
    if section.attachment_groups > 0 {
        if before.is_none() || before != after || !schema_current {
            preview.manual_review.push("Earlier attachments could not be rechecked on a stable page; inspect them manually.".into());
        } else {
            preview.manual_review.push(format!("For this possible earlier section, {verified_attachments} of {} attachment field(s) matched their staged files. Review the section and attachments manually; this does not establish delivery.", section.attachment_groups));
            if verified_attachments < section.attachment_groups
                && groups.len() == section.attachment_groups
            {
                if let Some((index, total)) = groups
                    .get(verified_attachments)
                    .and_then(|(selector, _)| prior_field_number(&preview.schema, selector))
                {
                    preview.manual_review.push(format!("Start reviewing attachments at field {index} of {total} in this section's field list; later attachments may not have been checked."));
                }
            }
        }
    }
}

pub fn start(
    id: String,
    session: ReviewSession,
    target: Option<String>,
    allow_other: bool,
    reviewed_label: Option<String>,
    profile: serde_json::Value,
    review: (
        i64,
        Vec<waypoint_workspace::form_experience::ExperienceMatch>,
    ),
) -> Result<(), (Box<ReviewSession>, String)> {
    start_with_destination_check(
        id,
        session,
        target,
        allow_other,
        reviewed_label,
        profile,
        review,
        check_destination,
    )
}

fn require_checked_previous_answers(
    check: &form_preview::CompletedFieldCheck,
) -> Result<(), String> {
    if !check.failed_labels.is_empty() {
        return Err("A previously filled answer changed or could not be verified. Review the current section before navigating.".into());
    }
    if check.unverified != 0 {
        return Err("Not all previously filled answers could be checked before navigation. Review the current section before continuing.".into());
    }
    Ok(())
}

fn verify_navigation_state(session: &mut ReviewSession) -> Result<(), String> {
    crate::check_answer_page(session)?;
    if let Some(first) = session.completed_selectors.first() {
        let check = session.driver.with_review_timeout(|driver| {
            form_preview::verify_completed_fields(
                driver,
                &session.preview,
                &session.completed_selectors,
                first,
            )
        });
        require_checked_previous_answers(&check)?;
    }
    if !crate::choice_attachments_verified(session) {
        return Err("A previously attached file changed or could not be verified. Review the current section before navigating.".into());
    }
    Ok(())
}

fn reviewed_target_present(
    session: &mut ReviewSession,
    target: &str,
    allow_other: bool,
    reviewed_label: Option<&str>,
) -> Result<bool, String> {
    if allow_other {
        Ok(other_candidates(&mut session.driver)?.iter().any(|link| {
            reviewed_label.is_some_and(|label| reviewed_link_matches(link, target, label))
        }))
    } else {
        Ok(candidates(&mut session.driver)?
            .iter()
            .any(|candidate| candidate == target))
    }
}

fn capture_then_recheck(
    session: &mut ReviewSession,
    target: &str,
    allow_other: bool,
    reviewed_label: Option<&str>,
    warning: &mut Option<String>,
    capture: impl FnOnce(&mut ReviewSession) -> Option<String>,
) -> Result<(), String> {
    *warning = capture(session);
    verify_navigation_state(session)?;
    if !reviewed_target_present(session, target, allow_other, reviewed_label)? {
        return Err("The selected link changed during capture; inspect again".into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn start_with_destination_check(
    id: String,
    mut session: ReviewSession,
    target: Option<String>,
    allow_other: bool,
    reviewed_label: Option<String>,
    profile: serde_json::Value,
    review: (
        i64,
        Vec<waypoint_workspace::form_experience::ExperienceMatch>,
    ),
    destination_check: fn(&str, Operation) -> Result<(), String>,
) -> Result<(), (Box<ReviewSession>, String)> {
    let (pending_revision, experiences) = review;
    let mut tasks = match TASK.lock() {
        Ok(tasks) => tasks,
        Err(_) => return Err((Box::new(session), "Navigation unavailable".into())),
    };
    if tasks.is_some() {
        return Err((
            Box::new(session),
            "Application navigation is already running".into(),
        ));
    }
    let (tx, rx) = mpsc::channel();
    let job_id = session.job_id.clone();
    *tasks = Some(Task {
        id,
        job_id,
        inspection_task_id: session.inspection_task_id.clone(),
        pending_revision,
        rx,
        failure: None,
    });
    drop(tasks);
    std::thread::spawn(move || {
        let navigating = target.is_some();
        let mut navigation_capture_warning = None;
        let mut used_revisions = Vec::new();
        let navigation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<form_preview::FormPreview, String> {
                if navigating {
                    verify_navigation_state(&mut session)?;
                }
                let fresh = session
                    .driver
                    .inspect_current()
                    .map_err(|e| e.to_string())?;
                let live = if let Some(target) = target {
                    if !fresh.walls.is_empty() {
                        return Err("The current page requires manual review".into());
                    }
                    let found = reviewed_target_present(&mut session, &target, allow_other, reviewed_label.as_deref())?;
                    if !found {
                        return Err("The selected link changed; inspect again".into());
                    }
                    let parsed = waypoint_net::parse_url(&target).map_err(|e| e.to_string())?;
                    waypoint_net::resolve_public(&parsed).map_err(|e| e.to_string())?;
                    capture_then_recheck(&mut session, &target, allow_other, reviewed_label.as_deref(), &mut navigation_capture_warning, |session| {
                        match session.record.as_mut() {
                            Some(record) => crate::attempt_record::capture_before_navigation(&mut session.driver, record, crate::now()).err().map(|error| format!("The previous section could not be fully captured before navigation: {error}")),
                            None => Some("The previous section could not be captured because the application record is unavailable".into()),
                        }
                    })?;
                    let live = session.driver.open(&target).map_err(|e| e.to_string())?;
                    let actual = session.driver.current_url().map_err(|e| e.to_string())?;
                    let actual =
                        url::Url::parse(&actual).map_err(|_| "Invalid final destination")?;
                    let target_url =
                        url::Url::parse(&target).map_err(|_| "Invalid selected destination")?;
                    if actual.origin() != target_url.origin() {
                        return Err(
                            "The application redirected to another site; review it manually".into(),
                        );
                    }
                    permitted(&actual, Operation::Read)?;
                    live
                } else {
                    destination_check(&fresh.origin, Operation::Read)?;
                    fresh
                };
                let observed_url = session.driver.current_url().map_err(|e| e.to_string())?;
                let initial_walls = live.walls.clone();
                let model = crate::form_resident_model();
                let mut preview = if let Ok(Some(model)) = model {
                    match crate::doc_suggest::lease() {
                        Ok(_lease) => {
                            let llm = OllamaInference::new("http://127.0.0.1:11434", model.clone())
                                .map_err(|e| e.to_string())?
                                .with_resident_context();
                            let mut llm = crate::resident_inference::ResidentInference::new(
                                llm,
                                model,
                                crate::resident_model_identity,
                            );
                            let relevant = crate::form_task::relevant_experiences(&live.schema, &experiences);
                            let mut informed = crate::experience_inference::ExperienceInference::new(&mut llm, &relevant);
                            let mut preview = form_preview::suggest_for_live_form(live, &mut informed, &profile)?;
                            used_revisions = crate::form_task::used_experience_revisions(&relevant, informed.attempted_with_hints());
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
                            Ok(preview)
                        }
                        Err(error) => form_preview::suggest_without_model(live, &profile, &error),
                    }
                } else {
                    let reason = model_unavailable_reason(model);
                    form_preview::suggest_without_model(live, &profile, &reason)
                }?;
                if recheck_navigation_preview(&mut session.driver, &mut preview, &profile, &observed_url, &initial_walls)? {
                    used_revisions.clear();
                }
                Ok(preview)
            },
        ))
        .unwrap_or_else(|_| Err("Application navigation stopped unexpectedly".into()));
        let mut preview = match navigation {
            Ok(preview) => preview,
            Err(error) => form_preview::FormPreview {
                model_review_selectors: Default::default(),
                schema: waypoint_applications::form_mapping::FormSchema {
                    origin: String::new(),
                    fields: vec![],
                    multi_step: false,
                    steps: 1,
                },
                suggestions: vec![],
                manual_review: vec![error],
            },
        };
        if form_preview::has_phone_mapping(&preview) {
            let before = session.driver.current_url().ok();
            let controls = session.driver.page_controls();
            if before.is_none() || session.driver.current_url().ok() != before {
                preview.suggestions.clear();
                preview
                    .manual_review
                    .push(form_preview::CHANGED_INFORMATIONAL_PREVIEW.into());
            } else {
                form_preview::guard_phone_country_picker(&mut preview, controls);
            }
        }
        if let Some(warning) = navigation_capture_warning {
            preview.manual_review.push(warning);
        }
        remember_prior_section(&mut session);
        review_prior_section(&mut session, &mut preview, &profile);
        let result = crate::random_id()
            .map_err(|reason| NavigationFailure {
                job_id: session.job_id.clone(),
                reason,
                review_saved: false,
                pending_revision,
            })
            .map(|new_id| {
                if session.pending_choice_visual.is_some()
                    && !preview.manual_review.iter().any(|reason| reason == form_preview::POST_CHOICE_VISUAL_REQUIRED)
                {
                    preview.manual_review.push(form_preview::POST_CHOICE_VISUAL_REQUIRED.into());
                }
                mark_prior_fills_for_review(&mut preview, &session.completed_selectors, &session.preview.manual_review);
                mark_prior_capture_for_review(&mut preview, &session.preview.manual_review);
                // A rescan can follow manual navigation too. Selectors and
                // their verified choices belong to the old inspection even
                // when the page happens to reuse the same selector.
                mark_prior_choices_for_review(&mut preview, &mut session.selected_choices, &session.preview.manual_review);
                if !session.attachments.is_empty() {
                    preview.manual_review.push("Attachments from earlier inspections require review; a matching-layout check is only advisory. Check them in the application before submitting".into());
                }
                session.id = new_id;
                session.answer_drafts.clear();
                session.profile_digest = Sha256Digest::of(&profile.to_string());
                session.preview = preview;
                session.filled = false;
                session.completed_selectors.clear();
                session.created = std::time::Instant::now();
                (session, used_revisions)
            });
        let _ = tx.send(result);
    });
    Ok(())
}

pub type NavigationOutcome = Result<(ReviewSession, i64, Vec<(String, u64)>), NavigationFailure>;

pub fn poll(id: &str) -> Result<Option<NavigationOutcome>, String> {
    let mut tasks = TASK.lock().map_err(|_| "Navigation unavailable")?;
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        let completed = COMPLETED_FAILURE
            .lock()
            .map_err(|_| "Navigation unavailable")?;
        return completed
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, failure)| Some(Err(failure.clone())))
            .ok_or_else(|| "Navigation task not found".into());
    };
    if let Some(failure) = &task.failure {
        return Ok(Some(Err(failure.clone())));
    }
    match task.rx.try_recv() {
        Ok(Ok((result, experience_revisions))) => {
            let revision = task.pending_revision;
            *tasks = None;
            Ok(Some(Ok((result, revision, experience_revisions))))
        }
        Ok(Err(failure)) => {
            task.failure = Some(failure.clone());
            Ok(Some(Err(failure)))
        }
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => {
            let failure = NavigationFailure {
                job_id: task.job_id.clone(),
                reason: "Navigation stopped unexpectedly".into(),
                review_saved: false,
                pending_revision: task.pending_revision,
            };
            task.failure = Some(failure.clone());
            Ok(Some(Err(failure)))
        }
    }
}

/// Release a failed task only after its manual-review flag has been saved.
pub fn acknowledge_failure(id: &str) -> Result<(), String> {
    let mut tasks = TASK.lock().map_err(|_| "Navigation unavailable")?;
    let mut completed = COMPLETED_FAILURE
        .lock()
        .map_err(|_| "Navigation unavailable")?;
    let Some(task) = tasks.as_ref().filter(|task| task.id == id) else {
        return if completed.as_ref().is_some_and(|(task_id, _)| task_id == id) {
            Ok(())
        } else {
            Err("Navigation task not found".into())
        };
    };
    let mut failure = task
        .failure
        .clone()
        .ok_or("Navigation failure is not ready")?;
    failure.review_saved = true;
    *completed = Some((id.to_owned(), failure));
    *tasks = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn navigation_requires_previous_answers_to_be_checked() {
        let mut check = crate::form_preview::CompletedFieldCheck {
            selected_retained: true,
            verified: 2,
            unverified: 0,
            failed_labels: vec![],
            failed_details: vec![],
        };
        assert!(super::require_checked_previous_answers(&check).is_ok());
        check.unverified = 1;
        assert!(super::require_checked_previous_answers(&check).is_err());
        check.unverified = 0;
        check.failed_labels.push("First name".into());
        assert!(super::require_checked_previous_answers(&check).is_err());
    }

    #[test]
    #[ignore = "starts a local browser and tests navigation preflight"]
    fn changed_answer_or_attachment_prevents_worker_opening_a_reviewed_link() {
        use waypoint_applications::{
            driver::BrowserDriver,
            form_mapping::{BrowserCommand, PacketValue, ProposedMapping},
        };
        use waypoint_domain::{JobIdentityId, Sha256Digest};
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-local-model-wizard.html");
        let temp = tempfile::tempdir().unwrap();
        let source = std::fs::read_to_string(fixture).unwrap().replace(
            "<button type=\"button\" id=\"next\"",
            "<label for=\"resume\">Resume</label><input type=\"file\" id=\"resume\"><button type=\"button\" id=\"clear-resume\" onclick=\"document.getElementById('resume').value=''\">Clear attachment</button><button type=\"button\" id=\"next\"",
        );
        let fixture = temp.path().join("wizard.html");
        std::fs::write(&fixture, source).unwrap();
        let staged = temp.path().join("fictional.pdf");
        let original = b"%PDF-fictional";
        std::fs::write(&staged, original).unwrap();
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap())
            .unwrap()
            .to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let profile = serde_json::json!({"first_name":"Avery"});
        let mut preview = form_preview::suggest_without_model(live, &profile, "fixture").unwrap();
        preview
            .suggestions
            .retain(|mapping| mapping.field_selector != "#given");
        preview.suggestions.push(ProposedMapping {
            field_selector: "#given".into(),
            value: PacketValue::Text("Avery".into()),
        });
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#given".into(),
                value: PacketValue::Text("Changed".into()),
            })
            .unwrap();
        driver.attach_local_file("#resume", &staged).unwrap();
        let mut session = crate::ReviewSession {
            id: "preflight-session".into(),
            job_id: JobIdentityId::new("preflight-job").unwrap(),
            inspection_task_id: "preflight-inspection".into(),
            inspection_pending_revision: 0,
            experience_revisions: vec![],
            url: url.clone(),
            route_binding: None,
            profile_digest: Sha256Digest::of(&profile.to_string()),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: true,
            fill_attempted: true,
            completed_selectors: vec!["#given".into()],
            prior_sections: vec![],
            prior_sections_omitted: false,
            selected_choices: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            last_visual_freshness_key: None,
            record: None,
        };
        session
            .driver
            .execute(&BrowserCommand::Fill {
                selector: "#given".into(),
                value: PacketValue::Text("Avery".into()),
            })
            .unwrap();
        assert!(super::verify_navigation_state(&mut session).is_ok());
        let mut warning = None;
        let result = super::capture_then_recheck(
            &mut session,
            "https://example.invalid/next",
            false,
            None,
            &mut warning,
            |session| {
                session
                    .driver
                    .execute(&BrowserCommand::Fill {
                        selector: "#given".into(),
                        value: PacketValue::Text("Changed".into()),
                    })
                    .unwrap();
                Some("fictional capture warning".into())
            },
        );
        assert!(result
            .unwrap_err()
            .contains("previously filled answer changed"));
        assert_eq!(warning.as_deref(), Some("fictional capture warning"));
        assert_eq!(session.driver.current_url().unwrap(), url);
        assert!(super::verify_navigation_state(&mut session).is_err());
        fn fixture_check(_: &str, _: waypoint_policy::Operation) -> Result<(), String> {
            Ok(())
        }
        assert!(super::start_with_destination_check(
            "preflight-task".into(),
            session,
            Some("https://example.invalid/next".into()),
            false,
            None,
            profile.clone(),
            (0, vec![]),
            fixture_check
        )
        .is_ok());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut returned = loop {
            if let Some(result) = super::poll("preflight-task").unwrap() {
                break match result {
                    Ok((session, _, _)) => session,
                    Err(failure) => panic!("navigation failed: {}", failure.reason),
                };
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(returned.driver.current_url().unwrap(), url);
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("previously filled answer changed")));
        // Reinspect the same page, then change the staged file after attachment.
        let live = returned.driver.inspect_current().unwrap();
        returned.preview = form_preview::suggest_without_model(live, &profile, "fixture").unwrap();
        returned.completed_selectors.clear();
        returned.attachments = vec![(
            "#resume".into(),
            staged.clone(),
            returned.id.clone(),
            waypoint_domain::sha256_hex(original),
        )];
        std::fs::write(&staged, b"%PDF-changed").unwrap();
        assert!(super::start_with_destination_check(
            "attachment-preflight".into(),
            returned,
            Some("https://example.invalid/next".into()),
            false,
            None,
            profile.clone(),
            (0, vec![]),
            fixture_check
        )
        .is_ok());
        let mut returned = loop {
            if let Some(result) = super::poll("attachment-preflight").unwrap() {
                break match result {
                    Ok((session, _, _)) => session,
                    Err(failure) => panic!("navigation failed: {}", failure.reason),
                };
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(returned.driver.current_url().unwrap(), url);
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("previously attached file changed")));
        std::fs::write(&staged, original).unwrap();
        returned
            .driver
            .attach_local_file("#resume", &staged)
            .unwrap();
        returned
            .driver
            .execute(&BrowserCommand::Click {
                selector: "#clear-resume".into(),
            })
            .unwrap();
        let live = returned.driver.inspect_current().unwrap();
        returned.preview = form_preview::suggest_without_model(live, &profile, "fixture").unwrap();
        returned.attachments = vec![(
            "#resume".into(),
            staged.clone(),
            returned.id.clone(),
            waypoint_domain::sha256_hex(original),
        )];
        assert!(super::start_with_destination_check(
            "cleared-attachment".into(),
            returned,
            Some("https://example.invalid/next".into()),
            false,
            None,
            profile,
            (0, vec![]),
            fixture_check
        )
        .is_ok());
        let mut returned = loop {
            if let Some(result) = super::poll("cleared-attachment").unwrap() {
                break match result {
                    Ok((session, _, _)) => session,
                    Err(failure) => panic!("navigation failed: {}", failure.reason),
                };
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(returned.driver.current_url().unwrap(), url);
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("previously attached file changed")));
    }
    use super::*;
    use waypoint_applications::driver::{DriverError, LiveForm, SiteAck};
    use waypoint_applications::form_mapping::BrowserCommand;

    #[test]
    fn unavailable_model_status_preserves_error_details() {
        assert_eq!(
            model_unavailable_reason(Ok(None)),
            "No eligible local model is loaded"
        );
        assert_eq!(
            model_unavailable_reason(Err("memory guard paused inference".into())),
            "Local model unavailable: memory guard paused inference"
        );
    }

    #[test]
    fn prior_fills_remain_a_review_obligation_after_navigation_or_rescan() {
        let mut preview = form_preview::FormPreview {
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: true,
                steps: 2,
            },
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        mark_prior_fills_for_review(&mut preview, &[], &[]);
        assert!(preview.manual_review.is_empty());
        mark_prior_fills_for_review(&mut preview, &["#earlier-answer".into()], &[]);
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason
                .contains("Fields filled before this inspection must be reviewed again")));
        let previous = preview.manual_review.clone();
        preview.manual_review.clear();
        mark_prior_fills_for_review(&mut preview, &[], &previous);
        mark_prior_fills_for_review(&mut preview, &[], &previous);
        assert_eq!(
            preview
                .manual_review
                .iter()
                .filter(|reason| *reason == PRIOR_FIELDS_REVIEW)
                .count(),
            1
        );
    }

    #[test]
    #[ignore = "opens a local fictional form to recheck a changed navigation preview"]
    fn navigation_preview_rechecks_changed_fields_and_page_instructions() {
        use waypoint_applications::{
            driver::BrowserDriver,
            form_mapping::{BrowserCommand, PacketValue, ProposedMapping},
        };
        let temp = tempfile::tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        )
        .unwrap();
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/form-controlled-change.html"),
        )
        .unwrap();
        let fixture = temp.path().join("fictional.html");
        std::fs::write(
            &fixture,
            source.replace("</body>", "<p>Do not use AI tools.</p></body>"),
        )
        .unwrap();
        let url = url::Url::from_file_path(&fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let observed_url = driver.current_url().unwrap();
        let walls = live.walls.clone();
        let mut preview = form_preview::FormPreview {
            schema: live.schema,
            suggestions: vec![ProposedMapping {
                field_selector: "#first".into(),
                value: PacketValue::Text("Fictional".into()),
            }],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        assert!(!super::recheck_navigation_preview(
            &mut driver,
            &mut preview,
            &serde_json::json!({"first_name":"Fictional"}),
            &observed_url,
            &walls
        )
        .unwrap());
        assert_eq!(preview.suggestions.len(), 1);
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("without AI assistance")));
        preview.manual_review.clear();
        driver
            .execute(&BrowserCommand::Click {
                selector: "#change".into(),
            })
            .unwrap();
        assert!(super::recheck_navigation_preview(
            &mut driver,
            &mut preview,
            &serde_json::json!({"first_name":"Fictional"}),
            &observed_url,
            &walls
        )
        .unwrap());
        assert!(preview.suggestions.is_empty());
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("form changed during inspection")));
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("without AI assistance")));
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "captcha present"));
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("typed") && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "runs the background navigation worker on a fictional local wizard"]
    fn worker_reinspects_manually_advanced_wizard_without_submitting() {
        use waypoint_applications::{
            driver::BrowserDriver,
            form_mapping::{BrowserCommand, PacketValue},
        };
        use waypoint_domain::{JobIdentityId, Sha256Digest};
        fn local_fixture_only(origin: &str, _: waypoint_policy::Operation) -> Result<(), String> {
            if origin.starts_with("http://127.0.0.1:") {
                Ok(())
            } else {
                Err("Unexpected fixture origin".into())
            }
        }
        let _lease = crate::doc_suggest::lease().unwrap();
        let temp = tempfile::Builder::new()
            .prefix("waypoint-wizard-fixture-")
            .tempdir()
            .unwrap();
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/form-local-model-wizard.html"),
        )
        .unwrap();
        let source = source.replace("<label for=\"email\">", "<p>Do not use AI tools.</p><button type=\"button\" id=\"back\" onclick=\"document.getElementById('step-two').hidden=true;document.getElementById('step-one').hidden=false\">Back</button><label for=\"email\">").replace("<button type=\"button\" id=\"next\"", "<label for=\"resume\">Resume</label><input type=\"file\" id=\"resume\" accept=\".pdf\"><button type=\"button\" id=\"clear-file\" onclick=\"document.getElementById('resume').value=''\">Clear file</button><button type=\"button\" id=\"mutate\" onclick=\"document.getElementById('given').value='Changed'\">Change saved answer</button><button type=\"button\" id=\"next\"");
        use std::io::{Read, Write};
        use std::sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = stop.clone();
        let non_get_requests = Arc::new(AtomicUsize::new(0));
        let observed_writes = non_get_requests.clone();
        let server = std::thread::spawn(move || {
            while !stop_worker.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                        let mut request = [0u8; 4096];
                        if let Ok(count) = stream.read(&mut request) {
                            if count > 0 && !request[..count].starts_with(b"GET ") {
                                observed_writes.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", source.len(), source);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        let staged = temp.path().join("fictional.pdf");
        let file_bytes = b"%PDF-fictional attachment";
        std::fs::write(&staged, file_bytes).unwrap();
        let url = format!("http://127.0.0.1:{port}/wizard");
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let profile = serde_json::json!({"first_name":"Avery","email":"avery@example.invalid"});
        let mut preview =
            form_preview::suggest_without_model(live, &profile, "Fixture model unavailable")
                .unwrap();
        preview
            .suggestions
            .push(waypoint_applications::form_mapping::ProposedMapping {
                field_selector: "#given".into(),
                value: PacketValue::Text("Avery".into()),
            });
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#given".into(),
                value: PacketValue::Text("Avery".into()),
            })
            .unwrap();
        let completed = vec!["#given".to_string()];
        let check =
            form_preview::verify_completed_fields(&mut driver, &preview, &completed, "#given");
        assert!(super::require_checked_previous_answers(&check).is_ok());
        driver
            .execute(&BrowserCommand::Click {
                selector: "#mutate".into(),
            })
            .unwrap();
        let check =
            form_preview::verify_completed_fields(&mut driver, &preview, &completed, "#given");
        assert!(super::require_checked_previous_answers(&check).is_err());
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#given".into(),
                value: PacketValue::Text("Avery".into()),
            })
            .unwrap();
        driver.attach_local_file("#resume", &staged).unwrap();
        driver
            .execute(&BrowserCommand::Click {
                selector: "#next".into(),
            })
            .unwrap();
        let session = crate::ReviewSession {
            id: "wizard-session".into(),
            job_id: JobIdentityId::new("wizard-job").unwrap(),
            inspection_task_id: "wizard-inspection".into(),
            inspection_pending_revision: 0,
            experience_revisions: vec![],
            url,
            route_binding: None,
            profile_digest: Sha256Digest::of(&profile.to_string()),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: true,
            fill_attempted: true,
            completed_selectors: vec!["#given".into()],
            prior_sections: vec![],
            prior_sections_omitted: false,
            selected_choices: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![(
                "#resume".into(),
                staged.clone(),
                "wizard-session".into(),
                waypoint_domain::sha256_hex(file_bytes),
            )],
            pending_choice_visual: None,
            last_visual_freshness_key: None,
            record: None,
        };
        assert!(super::start_with_destination_check(
            "wizard-task".into(),
            session,
            None,
            false,
            None,
            profile.clone(),
            (0, vec![]),
            local_fixture_only
        )
        .is_ok());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut returned = loop {
            if let Some(outcome) = super::poll("wizard-task").unwrap() {
                break match outcome {
                    Ok((session, _, _)) => session,
                    Err(failure) => panic!("navigation failed: {}", failure.reason),
                };
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_ne!(returned.id, "wizard-session");
        assert!(returned
            .preview
            .schema
            .fields
            .iter()
            .any(|field| field.selector == "#email"));
        assert!(returned
            .preview
            .suggestions
            .iter()
            .any(|mapping| mapping.field_selector == "#email"));
        assert!(!returned
            .preview
            .suggestions
            .iter()
            .any(|mapping| mapping.field_selector == "#permit"));
        assert_eq!(
            returned.driver.text_value_snapshot("#email").unwrap().value,
            ""
        );
        assert_eq!(
            returned
                .driver
                .text_value_snapshot("#permit")
                .unwrap()
                .value,
            ""
        );
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("without AI assistance")));
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("Fields filled before this inspection")));
        assert!(returned.completed_selectors.is_empty());
        returned
            .driver
            .execute(&BrowserCommand::Click {
                selector: "#back".into(),
            })
            .unwrap();
        returned
            .driver
            .verify_text_value("#given", "Avery")
            .unwrap();
        assert!(super::start_with_destination_check(
            "wizard-return".into(),
            returned,
            None,
            false,
            None,
            profile.clone(),
            (0, vec![]),
            local_fixture_only
        )
        .is_ok());
        let mut returned = loop {
            if let Some(outcome) = super::poll("wizard-return").unwrap() {
                break match outcome {
                    Ok((session, _, _)) => session,
                    Err(failure) => panic!("navigation failed: {}", failure.reason),
                };
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("browser readback matched 1 recorded field")));
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("does not prove it is the same section")));
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("1 of 1 attachment field(s) matched")));
        std::fs::write(&staged, b"%PDF-changed attachment").unwrap();
        let mut changed_file_view = returned.preview.clone();
        changed_file_view.manual_review.clear();
        super::review_prior_section(&mut returned, &mut changed_file_view, &profile);
        assert!(changed_file_view
            .manual_review
            .iter()
            .any(|reason| reason.contains("0 of 1 attachment field(s) matched")));
        std::fs::write(&staged, file_bytes).unwrap();
        let mut revised_profile_view = returned.preview.clone();
        revised_profile_view.manual_review.clear();
        super::review_prior_section(
            &mut returned,
            &mut revised_profile_view,
            &serde_json::json!({"first_name":"Revised"}),
        );
        assert!(revised_profile_view
            .manual_review
            .iter()
            .any(|reason| reason.contains("profile changed after an earlier section")));
        returned
            .driver
            .execute(&BrowserCommand::Click {
                selector: "#mutate".into(),
            })
            .unwrap();
        returned
            .driver
            .execute(&BrowserCommand::Click {
                selector: "#clear-file".into(),
            })
            .unwrap();
        assert!(super::start_with_destination_check(
            "wizard-changed".into(),
            returned,
            None,
            false,
            None,
            profile,
            (0, vec![]),
            local_fixture_only
        )
        .is_ok());
        let mut returned = loop {
            if let Some(outcome) = super::poll("wizard-changed").unwrap() {
                break match outcome {
                    Ok((session, _, _)) => session,
                    Err(failure) => panic!("navigation failed: {}", failure.reason),
                };
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("could not be verified or changed")));
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("0 of 1 attachment field(s) matched")));
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("Start reviewing at field 1 of 2")));
        assert!(returned
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("Start reviewing attachments at field 2 of 2")));
        assert!(returned
            .preview
            .manual_review
            .iter()
            .all(|reason| !reason.contains("Avery")));
        returned.preview = returned.prior_sections[0].preview.clone();
        returned.completed_selectors = vec!["#given".into()];
        for _ in 0..5 {
            super::remember_prior_section(&mut returned);
        }
        assert!(returned.prior_sections.len() > 4);
        assert!(!returned.prior_sections_omitted);
        for _ in 0..super::MAX_PRIOR_SECTIONS {
            super::remember_prior_section(&mut returned);
        }
        assert_eq!(returned.prior_sections.len(), super::MAX_PRIOR_SECTIONS);
        assert!(returned.prior_sections_omitted);
        let mut ambiguous = returned.preview.clone();
        ambiguous.manual_review.clear();
        super::review_prior_section(
            &mut returned,
            &mut ambiguous,
            &serde_json::json!({"first_name":"Avery","email":"avery@example.invalid"}),
        );
        assert!(ambiguous
            .manual_review
            .iter()
            .any(|reason| reason.contains("same layout")));
        assert!(ambiguous
            .manual_review
            .iter()
            .any(|reason| reason.contains("exceeded the in-memory review limit")));
        returned.prior_sections.clear();
        returned.prior_sections_omitted = false;
        returned.completed_selectors.clear();
        returned.attachments[0].2 = returned.id.clone();
        super::remember_prior_section(&mut returned);
        assert_eq!(returned.prior_sections.len(), 1);
        assert!(returned.prior_sections[0].completed.is_empty());
        let mut attachment_only = returned.preview.clone();
        attachment_only.manual_review.clear();
        super::review_prior_section(
            &mut returned,
            &mut attachment_only,
            &serde_json::json!({"first_name":"Avery","email":"avery@example.invalid"}),
        );
        assert!(attachment_only
            .manual_review
            .iter()
            .any(|reason| reason.contains("0 of 1 attachment field(s) matched")));
        assert!(returned
            .driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
        stop.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert_eq!(
            non_get_requests.load(Ordering::Relaxed),
            0,
            "fixture received a non-GET request"
        );
    }

    #[test]
    fn earlier_custom_choices_remain_a_review_obligation_across_rescans() {
        let mut preview = form_preview::FormPreview {
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: true,
                steps: 2,
            },
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let mut choices = vec![];
        mark_prior_choices_for_review(&mut preview, &mut choices, &[]);
        assert!(preview.manual_review.is_empty());
        choices.push(("#shared-selector".into(), "earlier choice".into()));
        mark_prior_choices_for_review(&mut preview, &mut choices, &[]);
        assert!(
            choices.is_empty(),
            "an old selector must not carry into the next inspection"
        );
        let previous = preview.manual_review.clone();
        preview.manual_review.clear();
        mark_prior_choices_for_review(&mut preview, &mut choices, &previous);
        mark_prior_choices_for_review(&mut preview, &mut choices, &previous);
        assert_eq!(preview.manual_review, vec![PRIOR_CHOICES_REVIEW]);
    }

    #[test]
    fn incomplete_capture_remains_visible_across_inspections() {
        let mut preview = form_preview::FormPreview {
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: true,
                steps: 2,
            },
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        for failure in [
            "Inspection evidence incomplete: write failed",
            "The previous section could not be fully captured before navigation: timed out",
            "The previous section could not be captured because the application record is unavailable",
            "Attempt screenshot capture was incomplete",
        ] {
            preview.manual_review.clear();
            mark_prior_capture_for_review(&mut preview, &[failure.into()]);
            assert_eq!(preview.manual_review, vec![PRIOR_CAPTURE_REVIEW]);
            let previous = preview.manual_review.clone();
            preview.manual_review.clear();
            mark_prior_capture_for_review(&mut preview, &previous);
            mark_prior_capture_for_review(&mut preview, &previous);
            assert_eq!(preview.manual_review, vec![PRIOR_CAPTURE_REVIEW]);
        }
        preview.manual_review.clear();
        mark_prior_capture_for_review(&mut preview, &["unrelated review".into()]);
        assert!(preview.manual_review.is_empty());
    }

    #[test]
    fn missing_field_observation_remains_a_historical_review_obligation() {
        let mut preview = form_preview::FormPreview {
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let first = vec!["a custom control needs manual review: 1 field(s) seen earlier are absent from this inspection (Question). Scroll and inspect again; the form may be incomplete.".into()];
        mark_prior_fills_for_review(&mut preview, &[], &first);
        assert_eq!(preview.manual_review, vec![PRIOR_MISSING_FIELDS_REVIEW]);
        let prior = preview.manual_review.clone();
        preview.manual_review.clear();
        mark_prior_fills_for_review(&mut preview, &[], &prior);
        mark_prior_fills_for_review(&mut preview, &[], &prior);
        assert_eq!(preview.manual_review, vec![PRIOR_MISSING_FIELDS_REVIEW]);
    }

    #[test]
    fn disconnected_navigation_retains_the_job_for_review() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let added = crate::manual_job::add(
            &mut ws,
            &serde_json::json!({
                "url":"https://example.org/job", "title":"Engineer", "employer":"Fixture"
            }),
            1,
        )
        .unwrap();
        let job_id = waypoint_domain::JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let revision = begin_pending(&ws, &job_id, "fictional-navigation", 1).unwrap();
        let (sender, rx) = mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "fictional-navigation".into(),
            job_id: job_id.clone(),
            inspection_task_id: "fictional-inspection".into(),
            pending_revision: revision,
            rx,
            failure: None,
        });
        assert_eq!(
            pending().unwrap(),
            Some(("fictional-navigation".into(), job_id.as_str().into()))
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = "{}";
        write!(client, "POST /api/form/session-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let state: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(state["active"], false);
        assert_eq!(state["navigation"]["task"], "fictional-navigation");
        assert_eq!(state["navigation"]["job"], job_id.as_str());
        assert_eq!(
            protected_reasons(&job_id).unwrap(),
            vec![
                pending_reason("fictional-navigation"),
                crate::form_task::pending_reason("fictional-inspection"),
            ]
        );
        let first = poll("fictional-navigation")
            .unwrap()
            .unwrap()
            .err()
            .unwrap();
        let retry = poll("fictional-navigation")
            .unwrap()
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(first.job_id, retry.job_id);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = serde_json::json!({"task_id":"fictional-navigation"}).to_string();
        write!(client, "POST /api/form/open-link-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["error"], "Navigation stopped unexpectedly");
        let reviews = ws.manual_reviews().unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].job_id, job_id.as_str());
        assert!(reviews[0].reason.contains("navigation stopped"));
        assert!(!ws
            .manual_review_reasons(&job_id)
            .unwrap()
            .iter()
            .any(|entry| entry.reason == pending_reason("fictional-navigation")));
        let revision = ws.manual_review_reasons(&job_id).unwrap()[0].revision;
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/form/open-link-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let repeated: serde_json::Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result, repeated);
        assert_eq!(
            ws.manual_review_reasons(&job_id).unwrap()[0].revision,
            revision
        );
        assert!(!busy());
    }

    #[test]
    fn pending_navigation_survives_reopen_and_preserves_other_reasons() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(base).unwrap();
        let (job, revision) = {
            let mut ws = waypoint_workspace::Workspace::open(dir.path()).unwrap();
            let added = crate::manual_job::add(&mut ws, &serde_json::json!({"url":"https://example.org/navigation-fixture","title":"Role","employer":"Fixture"}), 1).unwrap();
            let job = waypoint_domain::JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
            let revision = begin_pending(&ws, &job, "navigation-task", 2).unwrap();
            ws.mark_manual_review(&job, "Another review issue", 3)
                .unwrap();
            (job, revision)
        };
        let ws = waypoint_workspace::Workspace::open(dir.path()).unwrap();
        assert!(ws
            .manual_review_reasons(&job)
            .unwrap()
            .iter()
            .any(|entry| entry.reason == pending_reason("navigation-task")));
        assert!(finish_pending(&ws, &job, "navigation-task", revision).unwrap());
        let reasons = ws.manual_review_reasons(&job).unwrap();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].reason, "Another review issue");
    }

    struct Links(Vec<String>);
    impl BrowserDriver for Links {
        fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
            panic!("candidate discovery must not navigate")
        }
        fn current_url(&mut self) -> Result<String, DriverError> {
            Ok("https://jobs.example.org/role".into())
        }
        fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
            Ok(self.0.clone())
        }
        fn page_links(
            &mut self,
        ) -> Result<Vec<waypoint_applications::driver::PageLink>, DriverError> {
            Ok(self
                .0
                .iter()
                .map(|url| waypoint_applications::driver::PageLink {
                    url: url.clone(),
                    label: "Example".into(),
                })
                .collect())
        }
        fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
            panic!("candidate discovery must not fill")
        }
        fn commit(&mut self) -> Result<SiteAck, DriverError> {
            panic!("candidate discovery must not submit")
        }
    }
    #[test]
    fn link_read_failures_are_recorded_without_conflating_empty_results() {
        struct FailingLinks;
        impl BrowserDriver for FailingLinks {
            fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
                panic!("no navigation")
            }
            fn current_url(&mut self) -> Result<String, DriverError> {
                Ok("https://jobs.example.org/role".into())
            }
            fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
                Err(DriverError("fixture error".into()))
            }
            fn page_links(
                &mut self,
            ) -> Result<Vec<waypoint_applications::driver::PageLink>, DriverError> {
                Err(DriverError("fixture error".into()))
            }
            fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
                panic!("no fill")
            }
            fn commit(&mut self) -> Result<SiteAck, DriverError> {
                panic!("no submit")
            }
        }
        let mut review = Vec::new();
        let (likely, other, available) = links_for_review(&mut FailingLinks, &mut review);
        assert!(likely.is_empty() && other.is_empty() && !available);
        assert_eq!(review.len(), 2);
        assert!(review[0].contains("could not be inspected"));
        let mut review = Vec::new();
        let (likely, other, available) = links_for_review(&mut Links(vec![]), &mut review);
        assert!(likely.is_empty() && other.is_empty() && available && review.is_empty());
    }
    #[test]
    fn reviewed_link_requires_the_same_label_and_destination() {
        let link = waypoint_applications::driver::PageLink {
            url: "https://jobs.example.org/apply".into(),
            label: "Continue".into(),
        };
        assert!(reviewed_link_matches(&link, &link.url, "Continue"));
        assert!(!reviewed_link_matches(&link, &link.url, "Submit"));
        assert!(!reviewed_link_matches(
            &link,
            "https://other.example.org/apply",
            "Continue"
        ));
    }
    #[test]
    fn other_links_remain_bounded_and_exclude_final_action_routes() {
        let mut driver = Links(vec![
            "/unfamiliar-application".into(),
            "/submit".into(),
            "https://127.0.0.1/apply".into(),
        ]);
        let found = other_candidates(&mut driver).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].url,
            "https://jobs.example.org/unfamiliar-application"
        );
        assert_eq!(found[0].label, "Example");
        let mut many = Links((0..80).map(|i| format!("/page/{i}")).collect());
        assert_eq!(other_candidates(&mut many).unwrap().len(), 30);
        let mut review = Vec::new();
        let (_, links, available) = links_for_review(&mut many, &mut review);
        assert!(available);
        assert_eq!(links.len(), 30);
        assert!(review
            .iter()
            .any(|reason| reason.contains("other-link limit")));
        assert!(review
            .iter()
            .any(|reason| reason.contains("application-link limit")));
    }

    #[test]
    fn full_link_inventory_requires_manual_navigation() {
        let mut driver = Links((0..200).map(|i| format!("/page/{i}")).collect());
        assert!(candidates(&mut driver).unwrap_err().contains("limit"));
        assert!(other_candidates(&mut driver).unwrap_err().contains("limit"));
        let mut review = Vec::new();
        let (likely, other, available) = links_for_review(&mut driver, &mut review);
        assert!(likely.is_empty() && other.is_empty() && !available);
        assert_eq!(review.len(), 1);
        assert!(review[0].contains("navigate manually"));
    }

    #[test]
    fn final_action_labels_block_shared_destinations_in_both_lists() {
        struct Labelled;
        impl BrowserDriver for Labelled {
            fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
                panic!("no navigation")
            }
            fn current_url(&mut self) -> Result<String, DriverError> {
                Ok("https://jobs.example.org/role".into())
            }
            fn application_links(&mut self) -> Result<Vec<String>, DriverError> {
                Ok(vec!["/next".into(), "/apply".into()])
            }
            fn page_links(
                &mut self,
            ) -> Result<Vec<waypoint_applications::driver::PageLink>, DriverError> {
                Ok(vec![
                    waypoint_applications::driver::PageLink {
                        url: "/next".into(),
                        label: "Continue".into(),
                    },
                    waypoint_applications::driver::PageLink {
                        url: "/next".into(),
                        label: "Submit application".into(),
                    },
                    waypoint_applications::driver::PageLink {
                        url: "/apply".into(),
                        label: "Apply now".into(),
                    },
                ])
            }
            fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
                panic!("no fill")
            }
            fn commit(&mut self) -> Result<SiteAck, DriverError> {
                panic!("no submit")
            }
        }
        let mut driver = Labelled;
        assert_eq!(
            candidates(&mut driver).unwrap(),
            vec!["https://jobs.example.org/apply"]
        );
        assert_eq!(
            other_candidates(&mut driver)
                .unwrap()
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            vec!["https://jobs.example.org/apply"]
        );
    }

    #[test]
    #[ignore = "starts one headless browser against a local fixture"]
    fn browser_reports_distinct_labels_for_the_same_link() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-shared-link-labels.html");
        let url = format!(
            "file:///{}",
            fixture.display().to_string().replace('\\', "/")
        );
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(&url).unwrap();
        let links = driver.page_links().unwrap();
        assert!(links
            .iter()
            .any(|link| link.url == "/next" && link.label == "Continue"));
        assert!(links
            .iter()
            .any(|link| link.url == "/next" && link.label == "Submit application"));
    }

    #[test]
    #[ignore = "starts one headless browser against a local fixture"]
    fn browser_full_link_inventory_is_reported_for_manual_review() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(base).unwrap();
        let path = dir.path().join("many-links.html");
        let anchors: String = (0..200)
            .map(|i| format!("<a href='/page/{i}'>Page {i}</a>\n"))
            .collect();
        std::fs::write(
            &path,
            format!("<!doctype html><html><body>{anchors}</body></html>"),
        )
        .unwrap();
        let url = format!("file:///{}", path.display().to_string().replace('\\', "/"));
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(&url).unwrap();
        let mut review = Vec::new();
        let (likely, other, available) = links_for_review(&mut driver, &mut review);
        assert!(likely.is_empty() && other.is_empty() && !available);
        assert!(review
            .iter()
            .any(|reason| reason.contains("link-inspection limit")));
    }

    #[test]
    fn hash_routes_preserve_the_reviewed_destination_and_exclude_final_actions() {
        let mut driver = Links(vec![
            "#/application".into(),
            "#/profile".into(),
            "#/submit".into(),
        ]);
        let links = other_candidates(&mut driver).unwrap();
        assert_eq!(
            links
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            [
                "https://jobs.example.org/role#/application",
                "https://jobs.example.org/role#/profile"
            ]
        );
        assert!(reviewed_link_matches(
            &links[0],
            "https://jobs.example.org/role#/application",
            "Example"
        ));
        assert!(!reviewed_link_matches(
            &links[0],
            "https://jobs.example.org/role",
            "Example"
        ));
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional local page"]
    fn browser_inspects_a_hash_routed_section_without_submitting() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-hash-navigation.html");
        let mut url = url::Url::from_file_path(path).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let first = driver.open(url.as_str()).unwrap();
        assert!(first
            .schema
            .fields
            .iter()
            .any(|field| field.selector == "#name"));
        url.set_fragment(Some("/profile"));
        let started = std::time::Instant::now();
        let next = driver.open(url.as_str()).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "same-document navigation waited for a full page load"
        );
        assert_eq!(driver.current_url().unwrap(), url.as_str());
        assert!(
            next.schema
                .fields
                .iter()
                .any(|field| field.selector == "#email"),
            "profile section was not inspected: {:?}",
            next.schema.fields
        );
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }
    #[test]
    fn cross_origin_candidates_require_a_public_permitted_non_submit_route() {
        let mut driver = Links(vec![
            "https://boards.greenhouse.io/example/jobs/1?gh_jid=1".into(),
            "https://linkedin.com/jobs/1".into(),
            "https://127.0.0.1/apply".into(),
            "/apply?submit=true".into(),
        ]);
        assert_eq!(
            candidates(&mut driver).unwrap(),
            ["https://boards.greenhouse.io/example/jobs/1?gh_jid=1"]
        );
        assert!(check_job_and_destination(
            "jobicy",
            "https://boards.greenhouse.io",
            Operation::Prefill
        )
        .is_ok());
        assert!(
            check_job_and_destination("jobicy", "https://jobicy.com", Operation::Prefill).is_err()
        );
        assert!(check_job_and_destination(
            "user_supplied_url",
            "https://linkedin.com",
            Operation::Prefill
        )
        .is_err());
    }
}
