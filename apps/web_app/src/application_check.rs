//! A single, read-only browser check. Finding controls is not proof that the
//! page is an application or that an application can be completed.
use std::sync::{mpsc, Mutex};
use url::Url;
use waypoint_applications::driver::BrowserDriver;

struct Task {
    id: String,
    job_id: String,
    url: String,
    receiver: mpsc::Receiver<(String, bool)>,
    result: Option<(String, bool)>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
// Retain only the most recently saved result for a lost response retry.
static COMPLETED: Mutex<Option<(String, String, String, String)>> = Mutex::new(None);

/// Only display a destination that is a public HTTPS URL on the original
/// origin and has no query, fragment or credentials. Query strings can contain
/// private or one-use tokens and removing them would misrepresent the URL.
fn displayable_inspected_url(original: &str, current: &str) -> Option<String> {
    let source = Url::parse(original).ok()?;
    let observed = Url::parse(current).ok()?;
    if observed.scheme() != "https"
        || observed.origin() != source.origin()
        || observed.query().is_some()
        || observed.fragment().is_some()
        || !observed.username().is_empty()
        || observed.password().is_some()
        || observed.as_str().len() > 2048
        || !waypoint_connectors::is_public_host_url(observed.as_str())
    {
        return None;
    }
    Some(observed.to_string())
}

fn report_inspected_page(mut message: String, inspected_url: Option<String>) -> String {
    if let Some(inspected_url) = inspected_url {
        message.push_str(&format!(" Inspected page: {inspected_url}"));
    }
    message
}

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

fn classify_fields(
    fields: &[waypoint_applications::form_mapping::FormField],
    followed: bool,
) -> (String, bool) {
    use waypoint_applications::form_mapping::FormFieldType;
    if fields.is_empty() {
        return (
            "No form fields detected on this page; find or open the application manually.".into(),
            true,
        );
    }
    // These are evidence only: contact or upload controls also appear on
    // unrelated forms and do not establish that an application can be sent.
    let upload = fields
        .iter()
        .any(|field| matches!(field.field_type, FormFieldType::File));
    let email = fields.iter().any(|field| {
        matches!(field.field_type, FormFieldType::Email)
            || field.autocomplete.as_deref().is_some_and(|hint| {
                hint.split_ascii_whitespace()
                    .last()
                    .is_some_and(|part| part.eq_ignore_ascii_case("email"))
            })
    });
    let name = fields.iter().any(|field| {
        field.autocomplete.as_deref().is_some_and(|hint| {
            hint.split_ascii_whitespace().last().is_some_and(|part| {
                ["name", "given-name", "family-name"]
                    .iter()
                    .any(|token| part.eq_ignore_ascii_case(token))
            })
        })
    });
    let mut signals = Vec::new();
    if upload {
        signals.push("file upload");
    }
    if email {
        signals.push("email");
    }
    if name {
        signals.push("name autocomplete");
    }
    let evidence = if signals.is_empty() {
        "No application-specific signals identified".to_owned()
    } else {
        format!("Visible control signals: {}", signals.join(", "))
    };
    (format!("{} form fields detected{}; {}. The page is not confirmed as an application. Manual review needed.", fields.len(), if followed { " after opening an application link" } else { "" }, evidence), true)
}

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

pub fn pending() -> Result<Option<(String, String)>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Application check unavailable")?
        .as_ref()
        .map(|task| (task.id.clone(), task.job_id.clone())))
}

pub fn start(id: String, job_id: String, url: String) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "Application check unavailable")?;
    if task.is_some() {
        return Err("An application check is already active or awaiting its result".into());
    }
    let (sender, receiver) = mpsc::channel();
    let inspected_url = url.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(String, bool), String> {
            let mut browser = waypoint_browser::CdpDriver::launch(vec![])
                .map_err(|_| "The local browser could not start.")?;
            let mut page = browser
                .open(&url)
                .map_err(|_| "The listing page could not be opened or inspected.")?;
            let mut followed = false;
            let mut inspected_url = browser
                .current_url()
                .ok()
                .and_then(|current| displayable_inspected_url(&url, &current));
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
                    inspected_url = displayable_inspected_url(&url, &final_url);
                } else if !links.is_empty() {
                    return Ok((
                        "Application links need manual review before opening.".into(),
                        true,
                    ));
                }
            }
            let (message, review) = if !page.walls.is_empty() {
                (
                    "Sign-in, CAPTCHA or another obstacle detected; manual review needed.".into(),
                    true,
                )
            } else {
                classify_fields(&page.schema.fields, followed)
            };
            Ok((report_inspected_page(message, inspected_url), review))
        })();
        let message =
            result.unwrap_or_else(|error| (format!("{error} Manual review needed."), true));
        let _ = sender.send(message);
    });
    *task = Some(Task {
        id,
        job_id,
        url: inspected_url,
        receiver,
        result: None,
    });
    Ok(())
}

pub fn poll(id: &str) -> Result<Option<(String, String, String, bool)>, String> {
    let mut task = TASK.lock().map_err(|_| "Application check unavailable")?;
    let Some(current) = task.as_mut().filter(|task| task.id == id) else {
        let completed = COMPLETED
            .lock()
            .map_err(|_| "Application check unavailable")?;
        return completed
            .as_ref()
            .filter(|(task_id, _, _, _)| task_id == id)
            .map(|(_, job_id, url, result)| {
                Some((job_id.clone(), url.clone(), result.clone(), false))
            })
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
    Ok(current.result.as_ref().map(|(result, review)| {
        (
            current.job_id.clone(),
            current.url.clone(),
            result.clone(),
            *review,
        )
    }))
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
            .is_some_and(|(task_id, _, _, _)| task_id == id)
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
        .map_err(|_| "Application check unavailable")? = Some((
        current.id.clone(),
        current.job_id.clone(),
        current.url.clone(),
        result.clone(),
    ));
    *task = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        acknowledge, application_destination, classify_fields, displayable_inspected_url, poll,
        report_inspected_page, Task, TASK,
    };
    static TASK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    #[test]
    fn inspected_page_is_only_reported_when_it_is_safe_and_unambiguous() {
        let original = "https://jobs.example.org/listing";
        assert_eq!(
            displayable_inspected_url(original, "https://jobs.example.org/listing/apply")
                .as_deref(),
            Some("https://jobs.example.org/listing/apply")
        );
        let message = report_inspected_page(
            "Manual review needed.".into(),
            displayable_inspected_url(original, "https://jobs.example.org/listing/apply"),
        );
        assert!(message.contains("Inspected page: https://jobs.example.org/listing/apply"));
        for destination in [
            "https://jobs.example.org/apply?token=fictional",
            "https://jobs.example.org/apply#section",
            "https://person@jobs.example.org/apply",
            "https://other.example.org/apply",
            "http://jobs.example.org/apply",
            "https://127.0.0.1/apply",
        ] {
            assert!(
                displayable_inspected_url(original, destination).is_none(),
                "{destination}"
            );
            assert_eq!(
                report_inspected_page(
                    "Manual review needed.".into(),
                    displayable_inspected_url(original, destination)
                ),
                "Manual review needed."
            );
        }
    }
    #[test]
    fn interrupted_check_returns_a_reviewable_result() {
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "fictional-interrupted-check".into(),
            job_id: "fictional-job".into(),
            url: "https://example.org/job".into(),
            receiver,
            result: None,
        });
        assert_eq!(
            super::pending().unwrap(),
            Some(("fictional-interrupted-check".into(), "fictional-job".into()))
        );
        let (job, url, result, review) = poll("fictional-interrupted-check").unwrap().unwrap();
        assert_eq!(job, "fictional-job");
        assert!(review);
        assert!(result.contains("stopped unexpectedly"));
        assert_eq!(
            poll("fictional-interrupted-check").unwrap().unwrap(),
            (job, url, result, review)
        );
        assert!(acknowledge("wrong-task").is_err());
        acknowledge("fictional-interrupted-check").unwrap();
        assert!(TASK.lock().unwrap().is_none());
        let (saved_job, saved_url, saved_result, needs_save) =
            poll("fictional-interrupted-check").unwrap().unwrap();
        assert_eq!(saved_job, "fictional-job");
        assert_eq!(saved_url, "https://example.org/job");
        assert!(saved_result.contains("stopped unexpectedly"));
        assert!(!needs_save);
        acknowledge("fictional-interrupted-check").unwrap();
    }
    #[test]
    fn changed_listing_is_reported_stale_on_status_and_replay() {
        use std::io::{Read, Write};
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let job = crate::manual_job::add(&mut ws, &serde_json::json!({"url":"https://example.org/new","title":"Role","employer":"Fixture"}), 1).unwrap();
        let job_id = job["id"].as_str().unwrap().to_owned();
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "stale-check".into(),
            job_id,
            url: "https://example.org/old".into(),
            receiver,
            result: Some(("3 fields detected on old page".into(), true)),
        });
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        for _ in 0..2 {
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = serde_json::json!({"task_id":"stale-check"}).to_string();
            write!(client,"POST /api/application-check/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(&mut server, &mut ws, &mut None, "fixture", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            let result: serde_json::Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(result["current"], false);
            assert!(result["result"]
                .as_str()
                .unwrap()
                .contains("listing changed"));
            assert!(!result["result"].as_str().unwrap().contains("3 fields"));
        }
    }
    #[test]
    fn detected_fields_report_signals_without_confirming_an_application() {
        use waypoint_applications::form_mapping::{FormField, FormFieldType};
        let field = |selector: &str, field_type, autocomplete: Option<&str>| FormField {
            selector: selector.into(),
            label: "Fictional field".into(),
            field_type,
            required: false,
            sensitive: false,
            autocomplete: autocomplete.map(str::to_owned),
        };
        let fields = vec![
            field("#email", FormFieldType::Email, None),
            field("#upload", FormFieldType::File, None),
            field(
                "#name",
                FormFieldType::Text,
                Some("section-user given-name"),
            ),
        ];
        for followed in [false, true] {
            let (message, manual_review) = classify_fields(&fields, followed);
            assert!(manual_review);
            assert!(message.contains("not confirmed"));
            assert!(message.contains("file upload, email, name autocomplete"));
        }
        assert!(classify_fields(&[], false).1);
        let (generic, review) =
            classify_fields(&[field("#search", FormFieldType::Text, None)], false);
        assert!(review && generic.contains("No application-specific signals"));
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
        assert_eq!(page.schema.fields.len(), 3);
        let (message, review) = classify_fields(&page.schema.fields, true);
        assert!(review && message.contains("file upload, email, name autocomplete"));
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }
}
