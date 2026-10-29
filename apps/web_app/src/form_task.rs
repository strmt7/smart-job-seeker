//! One background form inspection; the server thread owns the resulting session.
use crate::{doc_suggest, form_preview};
use std::sync::{mpsc, Mutex};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::{JobIdentityId, Sha256Digest};
use waypoint_inference::OllamaInference;

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

/// Open the listing, then at most one unambiguous same-origin application link.
/// No field action or submit action is issued here.
fn open_application_page(
    driver: &mut dyn BrowserDriver,
    url: &str,
) -> Result<waypoint_applications::driver::LiveForm, String> {
    let mut live = driver.open(url).map_err(|e| e.to_string())?;
    if !live.walls.is_empty() {
        return Ok(live);
    }
    let current = driver.current_url().map_err(|e| e.to_string())?;
    let original = url::Url::parse(url).map_err(|_| "Invalid listing URL")?;
    let reached = url::Url::parse(&current).map_err(|_| "Invalid current URL")?;
    if original.scheme() == "https" && original.origin() != reached.origin() {
        live.walls
            .push("The listing redirected to another site; continue manually".into());
        return Ok(live);
    }
    // A file upload is evidence that this may already be an application form.
    // Keep the current page for user review rather than navigating away from it;
    // this is not proof that the page accepts applications.
    if has_upload_control(&live.schema.fields) {
        return Ok(live);
    }
    let links = driver.application_links().map_err(|e| e.to_string())?;
    if let Some(destination) = crate::application_check::application_destination(&current, &links) {
        live = driver.open(&destination).map_err(|e| e.to_string())?;
        let final_url = driver.current_url().map_err(|e| e.to_string())?;
        let final_url = url::Url::parse(&final_url).map_err(|_| "Invalid application URL")?;
        if original.origin() != final_url.origin() {
            live.walls
                .push("The application link redirected to another site; continue manually".into());
        }
    } else if !links.is_empty() {
        live.walls
            .push("Application links are ambiguous or require manual navigation".into());
    }
    Ok(live)
}

pub struct Finished {
    pub job_id: JobIdentityId,
    pub url: String,
    pub profile_digest: Sha256Digest,
    pub visible: bool,
    pub result: Result<(waypoint_browser::CdpDriver, form_preview::FormPreview), String>,
}
struct Task {
    id: String,
    receiver: mpsc::Receiver<Finished>,
    job_id: JobIdentityId,
    url: String,
    profile_digest: Sha256Digest,
    visible: bool,
    failure: Option<String>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED_FAILURE: Mutex<Option<(String, String)>> = Mutex::new(None);

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

pub fn pending() -> Result<Option<(String, String)>, String> {
    let task = TASK.lock().map_err(|_| "Form inspection unavailable")?;
    Ok(task
        .as_ref()
        .map(|task| (task.id.clone(), task.job_id.as_str().to_owned())))
}

pub fn start(
    id: String,
    job_id: JobIdentityId,
    url: String,
    profile: serde_json::Value,
    model: Option<String>,
    visible: bool,
) -> Result<(), String> {
    let (model, lease, no_model_reason) = match model {
        Some(model) => match doc_suggest::lease() {
            Ok(lease) => (Some(model), Some(lease), String::new()),
            Err(error) => (None, None, error),
        },
        None => (None, None, "No eligible local model is loaded".into()),
    };
    let mut task = TASK.lock().map_err(|_| "Form inspection unavailable")?;
    if task.is_some() {
        return Err("A form inspection is running or awaiting its result".into());
    }
    let (sender, receiver) = mpsc::channel();
    let profile_digest = Sha256Digest::of(&profile.to_string());
    *task = Some(Task {
        id,
        receiver,
        job_id: job_id.clone(),
        url: url.clone(),
        profile_digest: profile_digest.clone(),
        visible,
        failure: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let _lease = lease;
        let result = std::panic::catch_unwind(|| -> Result<_, String> {
            let mut driver = waypoint_browser::CdpDriver::launch_with_mode(vec![], visible)
                .map_err(|e| e.to_string())?;
            let live = open_application_page(&mut driver, &url)?;
            let preview = if let Some(model) = model {
                let mut llm = OllamaInference::new("http://127.0.0.1:11434", model)
                    .map_err(|e| e.to_string())?;
                form_preview::suggest_for_live_form(live, &mut llm, &profile)?
            } else {
                form_preview::suggest_without_model(live, &profile, &no_model_reason)?
            };
            Ok((driver, preview))
        })
        .unwrap_or_else(|_| Err("Form inspection stopped unexpectedly".into()));
        let _ = sender.send(Finished {
            job_id,
            url,
            profile_digest,
            visible,
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
            url: task.url.clone(),
            profile_digest: task.profile_digest.clone(),
            visible: task.visible,
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
                url: task.url.clone(),
                profile_digest: task.profile_digest.clone(),
                visible: task.visible,
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
    static TEST_LOCK: Mutex<()> = Mutex::new(());

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
            url: "https://example.org/temporary".into(),
            profile_digest: Sha256Digest::of("fixture"),
            visible: false,
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
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        *TASK.lock().unwrap() = Some(Task {
            id: "fictional-task".into(),
            receiver,
            job_id: job_id.clone(),
            url: "https://example.org/job".into(),
            profile_digest: Sha256Digest::of("fictional-profile"),
            visible: false,
            failure: None,
        });
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
        let _guard = TEST_LOCK.lock().unwrap();
        let _lease = crate::doc_suggest::lease().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        start(
            "busy-fixture".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({"country":"Germany"}),
            Some("fictional-model".into()),
            false,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(done) = poll("busy-fixture").unwrap() {
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
                    autocomplete: None,
                }]
            } else if url.ends_with("/apply") {
                vec![FormField {
                    selector: "#name".into(),
                    label: "Name".into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: false,
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
            Ok(self.links.clone())
        }
        fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
            panic!("inspection must not execute field actions")
        }
        fn commit(&mut self) -> Result<SiteAck, DriverError> {
            panic!("inspection must not submit")
        }
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
        let form = open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
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
        let form = open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
        assert_eq!(driver.opened.len(), 1);
        assert!(!form.walls.is_empty());
        let mut driver = FakeDriver {
            current: String::new(),
            links: vec![],
            opened: vec![],
            redirect: Some("https://another.example/job".into()),
            direct_upload: false,
        };
        let form = open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
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
        let form = open_application_page(&mut driver, "https://jobs.example.org/role").unwrap();
        assert_eq!(driver.opened, ["https://jobs.example.org/role"]);
        assert_eq!(form.schema.fields[0].field_type, FormFieldType::File);
    }

    #[test]
    #[ignore = "starts one headless browser against a fictional local form"]
    fn retains_a_live_upload_form_for_review() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let form = open_application_page(&mut driver, &url).unwrap();
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
    #[ignore = "starts one headless browser against a fictional local fixture"]
    fn background_inspection_returns_the_live_form_without_filling() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(path).unwrap().to_string();
        start(
            "inspection-fixture".into(),
            JobIdentityId::new("fixture-job").unwrap(),
            url,
            serde_json::json!({}),
            None,
            false,
        )
        .unwrap();
        assert!(busy());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(finished) = poll("inspection-fixture").unwrap() {
                let (driver, preview) = finished.result.unwrap();
                assert!(!preview.schema.fields.is_empty());
                assert!(preview.suggestions.is_empty());
                assert!(preview
                    .manual_review
                    .iter()
                    .any(|reason| reason.contains("No eligible local model")));
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
