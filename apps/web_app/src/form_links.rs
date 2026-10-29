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
    rx: mpsc::Receiver<Result<ReviewSession, NavigationFailure>>,
    failure: Option<NavigationFailure>,
}
#[derive(Clone)]
pub struct NavigationFailure {
    pub job_id: waypoint_domain::JobIdentityId,
    pub reason: String,
    pub review_saved: bool,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED_FAILURE: Mutex<Option<(String, NavigationFailure)>> = Mutex::new(None);

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
        Ok(links) => links,
        Err(_) => {
            review
                .push("Likely application links could not be inspected; navigate manually".into());
            Vec::new()
        }
    };
    let (other, available) = match other_candidates(driver) {
        Ok(links) => (links, true),
        Err(_) => {
            review.push("Other page links could not be inspected; navigate manually".into());
            (Vec::new(), false)
        }
    };
    (likely, other, available)
}

fn filtered_candidates(
    driver: &mut dyn BrowserDriver,
    other: bool,
) -> Result<Vec<waypoint_applications::driver::PageLink>, String> {
    let current = url::Url::parse(&driver.current_url().map_err(|e| e.to_string())?)
        .map_err(|_| "Invalid current URL")?;
    let mut result = Vec::new();
    let hrefs = if other {
        driver.page_links().map_err(|e| e.to_string())?
    } else {
        driver
            .application_links()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|url| waypoint_applications::driver::PageLink {
                url,
                label: String::new(),
            })
            .collect()
    };
    for link in hrefs.into_iter().take(200) {
        let Ok(mut url) = current.join(&link.url) else {
            continue;
        };
        url.set_fragment(None);
        let route = format!("{}?{}", url.path(), url.query().unwrap_or("")).to_ascii_lowercase();
        if ["submit", "confirm", "complete", "finish", "send"]
            .iter()
            .any(|word| route.contains(word))
            || permitted(&url, Operation::Read).is_err()
        {
            continue;
        }
        let target = url.to_string();
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
        rx,
        failure: None,
    });
    drop(tasks);
    std::thread::spawn(move || {
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
        let preview = match navigation {
            Ok(preview) => preview,
            Err(error) => form_preview::FormPreview {
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
        let result = crate::random_id()
            .map_err(|reason| NavigationFailure {
                job_id: session.job_id.clone(),
                reason,
                review_saved: false,
            })
            .map(|new_id| {
                session.id = new_id;
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

pub fn poll(id: &str) -> Result<Option<Result<ReviewSession, NavigationFailure>>, String> {
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
            *tasks = None;
            Ok(Some(Ok(result)))
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
        let (sender, rx) = mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "fictional-navigation".into(),
            job_id: job_id.clone(),
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
