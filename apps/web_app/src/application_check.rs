//! A single, read-only browser check. Finding controls is not proof that the
//! page is an application or that an application can be completed.
use std::sync::{mpsc, Mutex};
use url::Url;
use waypoint_applications::driver::BrowserDriver;

struct Task {
    id: String,
    job_id: String,
    receiver: mpsc::Receiver<(String, bool)>,
    result: Option<(String, bool)>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
// Retain only the most recently saved result for a lost response retry.
static COMPLETED: Mutex<Option<(String, String, String)>> = Mutex::new(None);

// Follow only one unambiguous same-origin link. In particular, never navigate
// a URL that looks like a final submit action or contains query parameters.
pub(crate) fn application_destination(current: &str, links: &[String]) -> Option<String> {
    let base = Url::parse(current).ok()?;
    if base.scheme() != "https" || !waypoint_connectors::is_public_host_url(current) {
        return None;
    }
    let mut candidates = Vec::new();
    let mut rejected = false;
    for href in links {
        let candidate = match base.join(href) {
            Ok(value) => value,
            Err(_) => {
                rejected = true;
                continue;
            }
        };
        if candidate.scheme() != "https"
            || candidate.origin() != base.origin()
            || candidate.query().is_some()
            || candidate.fragment().is_some()
            || !candidate.username().is_empty()
            || candidate.password().is_some()
            || candidate == base
            || !waypoint_connectors::is_public_host_url(candidate.as_str())
            || ["submit", "complete", "confirm", "finish", "send"]
                .iter()
                .any(|term| candidate.path().to_ascii_lowercase().contains(term))
        {
            rejected = true;
            continue;
        }
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    if candidates.len() == 1 && !rejected {
        Some(candidates.remove(0).to_string())
    } else {
        None
    }
}

fn classify_fields(field_count: usize, followed: bool) -> (String, bool) {
    if field_count == 0 {
        (
            "No form fields detected on this page; find or open the application manually.".into(),
            true,
        )
    } else {
        (format!(
            "{} form fields detected{}; the page is not confirmed as an application. Manual review needed.",
            field_count,
            if followed { " after opening an application link" } else { "" }
        ), true)
    }
}

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

pub fn start(id: String, job_id: String, url: String) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "Application check unavailable")?;
    if task.is_some() {
        return Err("An application check is already active or awaiting its result".into());
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<(String, bool), String> {
            let mut browser = waypoint_browser::CdpDriver::launch(vec![])
                .map_err(|_| "The local browser could not start.")?;
            let mut page = browser
                .open(&url)
                .map_err(|_| "The listing page could not be opened or inspected.")?;
            let mut followed = false;
            if page.walls.is_empty() {
                let current = browser
                    .current_url()
                    .map_err(|_| "The listing destination could not be checked.")?;
                let original = Url::parse(&url).map_err(|e| e.to_string())?;
                let reached = Url::parse(&current).map_err(|e| e.to_string())?;
                if original.origin() != reached.origin() {
                    return Ok((
                        "The listing redirected to another site; review the destination manually."
                            .into(),
                        true,
                    ));
                }
                let links = if crate::form_task::has_upload_control(&page.schema.fields) {
                    Vec::new()
                } else {
                    browser
                        .application_links()
                        .map_err(|_| "Application links could not be inspected.")?
                };
                if let Some(destination) = application_destination(&current, &links) {
                    page = browser
                        .open(&destination)
                        .map_err(|_| "The application link could not be opened or inspected.")?;
                    followed = true;
                    let final_url = browser
                        .current_url()
                        .map_err(|_| "The application destination could not be checked.")?;
                    let final_origin = Url::parse(&final_url).map_err(|e| e.to_string())?.origin();
                    if final_origin != original.origin() {
                        return Ok(("The application link redirected to another site; review the destination manually.".into(), true));
                    }
                } else if !links.is_empty() {
                    return Ok((
                        "Application links need manual review before opening.".into(),
                        true,
                    ));
                }
            }
            if !page.walls.is_empty() {
                Ok((
                    "Sign-in, CAPTCHA or another obstacle detected; manual review needed.".into(),
                    true,
                ))
            } else {
                Ok(classify_fields(page.schema.fields.len(), followed))
            }
        })();
        let message =
            result.unwrap_or_else(|error| (format!("{error} Manual review needed."), true));
        let _ = sender.send(message);
    });
    *task = Some(Task {
        id,
        job_id,
        receiver,
        result: None,
    });
    Ok(())
}

pub fn poll(id: &str) -> Result<Option<(String, String, bool)>, String> {
    let mut task = TASK.lock().map_err(|_| "Application check unavailable")?;
    let Some(current) = task.as_mut().filter(|task| task.id == id) else {
        let completed = COMPLETED
            .lock()
            .map_err(|_| "Application check unavailable")?;
        return completed
            .as_ref()
            .filter(|(task_id, _, _)| task_id == id)
            .map(|(_, job_id, result)| Some((job_id.clone(), result.clone(), false)))
            .ok_or_else(|| "Application check not found".into());
    };
    if current.result.is_none() {
        current.result = match current.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some((
                "Application check stopped unexpectedly; manual review needed.".into(),
                true,
            )),
        };
    }
    Ok(current
        .result
        .as_ref()
        .map(|(result, review)| (current.job_id.clone(), result.clone(), *review)))
}

/// Release a completed result only after its review entry has been saved.
pub fn acknowledge(id: &str) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "Application check unavailable")?;
    let Some(current) = task.as_ref().filter(|task| task.id == id) else {
        let completed = COMPLETED
            .lock()
            .map_err(|_| "Application check unavailable")?;
        return if completed
            .as_ref()
            .is_some_and(|(task_id, _, _)| task_id == id)
        {
            Ok(())
        } else {
            Err("Application check not found".into())
        };
    };
    let (result, _) = current
        .result
        .as_ref()
        .ok_or("Application check is still running")?;
    *COMPLETED
        .lock()
        .map_err(|_| "Application check unavailable")? =
        Some((current.id.clone(), current.job_id.clone(), result.clone()));
    *task = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{acknowledge, application_destination, classify_fields, poll, Task, TASK};
    #[test]
    fn interrupted_check_returns_a_reviewable_result() {
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "fictional-interrupted-check".into(),
            job_id: "fictional-job".into(),
            receiver,
            result: None,
        });
        let (job, result, review) = poll("fictional-interrupted-check").unwrap().unwrap();
        assert_eq!(job, "fictional-job");
        assert!(review);
        assert!(result.contains("stopped unexpectedly"));
        assert_eq!(
            poll("fictional-interrupted-check").unwrap().unwrap(),
            (job, result, review)
        );
        assert!(acknowledge("wrong-task").is_err());
        acknowledge("fictional-interrupted-check").unwrap();
        assert!(TASK.lock().unwrap().is_none());
        let (saved_job, saved_result, needs_save) =
            poll("fictional-interrupted-check").unwrap().unwrap();
        assert_eq!(saved_job, "fictional-job");
        assert!(saved_result.contains("stopped unexpectedly"));
        assert!(!needs_save);
        acknowledge("fictional-interrupted-check").unwrap();
    }
    #[test]
    fn detected_fields_do_not_confirm_an_application_page() {
        for followed in [false, true] {
            let (message, manual_review) = classify_fields(1, followed);
            assert!(manual_review);
            assert!(message.contains("not confirmed"));
        }
        assert!(classify_fields(0, false).1);
    }
    use waypoint_applications::driver::BrowserDriver;
    #[test]
    fn follows_only_a_single_safe_same_origin_application_link() {
        let base = "https://jobs.example.org/role";
        assert_eq!(
            application_destination(base, &["/role/apply".into()]).as_deref(),
            Some("https://jobs.example.org/role/apply")
        );
        for href in [
            "https://other.example.org/apply",
            "/submit",
            "/apply?submit=true",
            "#apply",
            "javascript:submit()",
        ] {
            assert!(
                application_destination(base, &[href.into()]).is_none(),
                "{href}"
            );
        }
        assert_eq!(
            application_destination(
                base,
                &[
                    "/role/apply".into(),
                    "https://jobs.example.org/role/apply".into()
                ]
            )
            .as_deref(),
            Some("https://jobs.example.org/role/apply")
        );
        assert!(application_destination(base, &["/apply".into(), "/apply/other".into()]).is_none());
        assert!(application_destination(
            base,
            &["/apply".into(), "https://other.example.org/apply".into()]
        )
        .is_none());
        assert!(application_destination(base, &["/apply".into(), "/submit".into()]).is_none());
    }

    #[test]
    #[ignore = "starts one browser using fictional local fixture files"]
    fn reads_application_link_and_form_without_submitting() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/application-check-listing.html");
        let listing = url::Url::from_file_path(&fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        assert!(driver
            .open(listing.as_str())
            .unwrap()
            .schema
            .fields
            .is_empty());
        let links = driver.application_links().unwrap();
        assert_eq!(links, ["application-check-form.html"]);
        let target = listing.join(&links[0]).unwrap();
        let page = driver.open(target.as_str()).unwrap();
        assert_eq!(page.schema.fields.len(), 1);
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }
}
