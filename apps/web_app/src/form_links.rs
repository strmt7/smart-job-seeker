//! User-selected navigation to an application link in the current page.
use crate::{form_preview, manual_job, ReviewSession};
use std::sync::{mpsc, Mutex};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::Sha256Digest;
use waypoint_inference::OllamaInference;
use waypoint_policy::Operation;

struct Task {
    id: String,
    job_id: waypoint_domain::JobIdentityId,
    pending_revision: i64,
    rx: mpsc::Receiver<Result<ReviewSession, NavigationFailure>>,
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

pub fn start(
    id: String,
    mut session: ReviewSession,
    target: Option<String>,
    allow_other: bool,
    reviewed_label: Option<String>,
    profile: serde_json::Value,
    pending_revision: i64,
) -> Result<(), (Box<ReviewSession>, String)> {
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
        pending_revision,
        rx,
        failure: None,
    });
    drop(tasks);
    std::thread::spawn(move || {
        let mut navigation_capture_warning = None;
        let navigation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<form_preview::FormPreview, String> {
                let fresh = session
                    .driver
                    .inspect_current()
                    .map_err(|e| e.to_string())?;
                let live = if let Some(target) = target {
                    if !fresh.walls.is_empty() {
                        return Err("The current page requires manual review".into());
                    }
                    let found = if allow_other {
                        other_candidates(&mut session.driver)?.iter().any(|link| {
                            reviewed_label
                                .as_deref()
                                .is_some_and(|label| reviewed_link_matches(link, &target, label))
                        })
                    } else {
                        candidates(&mut session.driver)?.contains(&target)
                    };
                    if !found {
                        return Err("The selected link changed; inspect again".into());
                    }
                    let parsed = waypoint_net::parse_url(&target).map_err(|e| e.to_string())?;
                    waypoint_net::resolve_public(&parsed).map_err(|e| e.to_string())?;
                    match session.record.as_mut() {
                        Some(record) => {
                            if let Err(error) = crate::attempt_record::capture_before_navigation(&mut session.driver, record, crate::now()) {
                                navigation_capture_warning = Some(format!("The previous section could not be fully captured before navigation: {error}"));
                            }
                        }
                        None => navigation_capture_warning = Some("The previous section could not be captured because the application record is unavailable".into()),
                    }
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
                    check_destination(&fresh.origin, Operation::Read)?;
                    fresh
                };
                let model = crate::resident_model().ok().flatten();
                if let Some(model) = model {
                    match crate::doc_suggest::lease() {
                        Ok(_lease) => {
                            let llm = OllamaInference::new("http://127.0.0.1:11434", model.clone())
                                .map_err(|e| e.to_string())?
                                .with_resident_context();
                            let mut llm = crate::resident_inference::ResidentInference::new(
                                llm,
                                model,
                                crate::resident_model,
                            );
                            form_preview::suggest_for_live_form(live, &mut llm, &profile)
                        }
                        Err(error) => form_preview::suggest_without_model(live, &profile, &error),
                    }
                } else {
                    form_preview::suggest_without_model(
                        live,
                        &profile,
                        "No eligible local model is loaded",
                    )
                }
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
        if let Some(warning) = navigation_capture_warning {
            preview.manual_review.push(warning);
        }
        let result = crate::random_id()
            .map_err(|reason| NavigationFailure {
                job_id: session.job_id.clone(),
                reason,
                review_saved: false,
                pending_revision,
            })
            .map(|new_id| {
                if !session.attachments.is_empty() {
                    preview.manual_review.push("Attachments from an earlier inspection cannot be verified on this section; check them in the application before submitting".into());
                }
                session.id = new_id;
                session.answer_drafts.clear();
                session.profile_digest = Sha256Digest::of(&profile.to_string());
                session.preview = preview;
                session.filled = false;
                session.completed_selectors.clear();
                session.created = std::time::Instant::now();
                session
            });
        let _ = tx.send(result);
    });
    Ok(())
}

pub type NavigationOutcome = Result<(ReviewSession, i64), NavigationFailure>;

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
        Ok(Ok(result)) => {
            let revision = task.pending_revision;
            *tasks = None;
            Ok(Some(Ok((result, revision))))
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
    use super::*;
    use waypoint_applications::driver::{DriverError, LiveForm, SiteAck};
    use waypoint_applications::form_mapping::BrowserCommand;

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
            pending_revision: revision,
            rx,
            failure: None,
        });
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
