//! User-selected navigation to an application link in the current page.
use crate::{form_preview, manual_job, ReviewSession};
use std::sync::{mpsc, Mutex};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::Sha256Digest;
use waypoint_inference::OllamaInference;
use waypoint_policy::Operation;

struct Task {
    id: String,
    rx: mpsc::Receiver<Result<ReviewSession, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

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
    let current = url::Url::parse(&driver.current_url().map_err(|e| e.to_string())?)
        .map_err(|_| "Invalid current URL")?;
    let mut result = Vec::new();
    for href in driver
        .application_links()
        .map_err(|e| e.to_string())?
        .into_iter()
        .take(50)
    {
        let Ok(mut url) = current.join(&href) else {
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
        if !result.contains(&target) {
            result.push(target);
        }
        if result.len() == 10 {
            break;
        }
    }
    Ok(result)
}

pub fn start(
    id: String,
    mut session: ReviewSession,
    target: Option<String>,
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
    *tasks = Some(Task { id, rx });
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
                    if !candidates(&mut session.driver)?.contains(&target) {
                        return Err("The application link changed; inspect again".into());
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
                    let _lease = crate::doc_suggest::lease()?;
                    let mut llm = OllamaInference::new("http://127.0.0.1:11434", model)
                        .map_err(|e| e.to_string())?;
                    form_preview::suggest_for_live_form(live, &mut llm, &profile)
                } else {
                    let mut review = live.walls;
                    review.push("No eligible local model is loaded; continue manually".into());
                    Ok(form_preview::FormPreview {
                        schema: live.schema,
                        suggestions: vec![],
                        manual_review: review,
                    })
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
        let result = crate::random_id().map(|new_id| {
            session.id = new_id;
            session.profile_digest = Sha256Digest::of(&profile.to_string());
            session.preview = preview;
            session.filled = false;
            session.created = std::time::Instant::now();
            session
        });
        let _ = tx.send(result);
    });
    Ok(())
}

pub fn poll(id: &str) -> Result<Option<Result<ReviewSession, String>>, String> {
    let mut tasks = TASK.lock().map_err(|_| "Navigation unavailable")?;
    let task = tasks
        .as_ref()
        .filter(|task| task.id == id)
        .ok_or("Navigation task not found")?;
    match task.rx.try_recv() {
        Ok(result) => {
            *tasks = None;
            Ok(Some(result))
        }
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => {
            *tasks = None;
            Err("Navigation stopped unexpectedly".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_applications::driver::{DriverError, LiveForm, SiteAck};
    use waypoint_applications::form_mapping::BrowserCommand;

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
        fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
            panic!("candidate discovery must not fill")
        }
        fn commit(&mut self) -> Result<SiteAck, DriverError> {
            panic!("candidate discovery must not submit")
        }
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
