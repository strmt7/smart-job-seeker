//! One bounded, user-reviewed choice-opening action on a background worker.
use crate::{form_preview, now, ReviewSession};
use serde_json::{json, Value};
use std::sync::{mpsc, Mutex};
use waypoint_domain::JobIdentityId;
use waypoint_workspace::Workspace;

struct Task {
    id: String,
    session_id: String,
    job_id: JobIdentityId,
    inspection_id: String,
    revision: i64,
    rx: mpsc::Receiver<(ReviewSession, Value)>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

fn reason(id: &str) -> String {
    format!("Custom choice opening pending: {id}")
}

pub fn begin_pending(ws: &Workspace, job: &JobIdentityId, id: &str) -> Result<i64, String> {
    let reason = reason(id);
    ws.mark_manual_review(job, &reason, now())
        .map_err(|error| error.to_string())?;
    ws.manual_review_reasons(job)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|entry| entry.reason == reason)
        .map(|entry| entry.revision)
        .ok_or_else(|| "Pending choice action was not recorded".into())
}

pub fn finish_pending(
    ws: &Workspace,
    job: &JobIdentityId,
    id: &str,
    revision: i64,
) -> Result<(), String> {
    ws.clear_manual_review_reason(job, &reason(id), revision)
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn protected_reasons(job: &JobIdentityId) -> Result<Vec<String>, String> {
    let task = TASK.lock().map_err(|_| "Choice action unavailable")?;
    Ok(task
        .as_ref()
        .filter(|task| &task.job_id == job)
        .map_or_else(Vec::new, |task| {
            vec![
                reason(&task.id),
                crate::form_task::pending_reason(&task.inspection_id),
            ]
        }))
}

pub fn pending() -> Result<Value, String> {
    let task = TASK.lock().map_err(|_| "Choice action unavailable")?;
    Ok(task.as_ref().map_or_else(
        || json!({"active":false}),
        |task| json!({"active":true,"task_id":task.id,"session_id":task.session_id}),
    ))
}

pub fn start(
    id: String,
    mut session: ReviewSession,
    selector: String,
    option: Option<String>,
    revision: i64,
) -> Result<(), (Box<ReviewSession>, String)> {
    let mut task = match TASK.lock() {
        Ok(task) => task,
        Err(_) => return Err((Box::new(session), "Choice action unavailable".into())),
    };
    if task.is_some() {
        return Err((
            Box::new(session),
            "A choice action is already running".into(),
        ));
    }
    let (tx, rx) = mpsc::channel();
    *task = Some(Task {
        id: id.clone(),
        session_id: session.id.clone(),
        job_id: session.job_id.clone(),
        inspection_id: session.inspection_task_id.clone(),
        revision,
        rx,
    });
    drop(task);
    std::thread::spawn(move || {
        let action = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(value) = option.as_deref() {
                session
                    .driver
                    .select_custom_choice_option(&selector, value)
                    .map(|()| None)
            } else {
                session.driver.open_custom_choice(&selector).map(Some)
            }
        }))
        .map_err(|_| "Choice opening stopped unexpectedly".to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));
        let screenshot = session
            .driver
            .with_review_timeout(|driver| driver.screenshot_base64())
            .ok();
        let saved = match (&mut session.record, &screenshot) {
            (Some(record), Some(image)) => record.screenshot_with_name(image).and_then(|name| record.event(json!({"type":"custom_choice_action_result","time":now(),"selector":selector,"option":option,"succeeded":action.is_ok(),"screenshot":format!("screenshots/{name}"),"note":"A new visual review is required before further action."}))).is_ok(),
            _ => false,
        };
        let check = session.completed_selectors.first().cloned().map(|first| {
            session.driver.with_review_timeout(|driver| {
                form_preview::verify_completed_fields(
                    driver,
                    &session.preview,
                    &session.completed_selectors,
                    &first,
                )
            })
        });
        let previous_verified = check.as_ref().is_none_or(|check| {
            check.selected_retained && check.failed_labels.is_empty() && check.unverified == 0
        }) && crate::verify_selected_choices_except(
            &mut session,
            option.as_ref().map(|_| selector.as_str()),
        )
        .is_ok();
        let result = match action {
            Ok(Some(choice)) => {
                json!({"status":"complete","opened":true,"choice":choice,"screenshot":screenshot,"record_saved":saved,"previous_fields_verified":previous_verified,"visual_review_required":true})
            }
            Ok(None) => {
                json!({"status":"complete","selected":true,"option":option,"screenshot":screenshot,"record_saved":saved,"previous_fields_verified":previous_verified,"visual_review_required":true})
            }
            Err(error) => {
                json!({"status":"uncertain","opened":false,"error":error,"screenshot":screenshot,"record_saved":saved,"previous_fields_verified":previous_verified,"visual_review_required":true})
            }
        };
        session.pending_choice_visual = Some((selector, id, revision, option));
        session
            .preview
            .manual_review
            .push(form_preview::POST_CHOICE_VISUAL_REQUIRED.into());
        session.created = std::time::Instant::now();
        let _ = tx.send((session, result));
    });
    Ok(())
}

pub enum Poll {
    Running,
    Complete(Box<ReviewSession>, Value, i64),
    Lost(JobIdentityId, i64),
    Cached(Value),
}
pub fn poll(id: &str) -> Result<Poll, String> {
    let mut task = TASK.lock().map_err(|_| "Choice action unavailable")?;
    let Some(active) = task.as_mut().filter(|task| task.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Choice action unavailable")?
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, value)| Poll::Cached(value.clone()))
            .ok_or_else(|| "Choice action not found".into());
    };
    match active.rx.try_recv() {
        Ok((session, result)) => {
            let revision = active.revision;
            *task = None;
            Ok(Poll::Complete(Box::new(session), result, revision))
        }
        Err(mpsc::TryRecvError::Empty) => Ok(Poll::Running),
        Err(mpsc::TryRecvError::Disconnected) => {
            let job = active.job_id.clone();
            let revision = active.revision;
            *task = None;
            Ok(Poll::Lost(job, revision))
        }
    }
}
pub fn remember(id: &str, value: Value) -> Result<(), String> {
    *COMPLETED.lock().map_err(|_| "Choice action unavailable")? = Some((id.into(), value));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use waypoint_applications::driver::BrowserDriver;
    use waypoint_domain::{ApplicationState, Sha256Digest};

    #[test]
    #[ignore = "starts one browser and opens only a fictional choice"]
    fn reviewed_open_is_recorded_one_use_and_recoverable() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_custom_choice_open.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = form_preview::suggest_without_model(live, &json!({}), "fixture").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let job = JobIdentityId::new("choice-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job.clone(),
            title: "Role".into(),
            employer: "Fixture".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.to_string()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let record =
            crate::attempt_record::AttemptRecord::start(&ws, &job, "choice-fixture", now())
                .unwrap()
                .unwrap();
        let mut session = Some(ReviewSession {
            id: "choice-session".into(),
            job_id: job.clone(),
            inspection_task_id: "fixture".into(),
            inspection_pending_revision: 0,
            url: url.to_string(),
            profile_digest: Sha256Digest::of(&json!({}).to_string()),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: false,
            completed_selectors: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: Some(record),
        });
        let (inventory, digest) =
            crate::custom_choice_snapshot(&mut session.as_mut().unwrap().driver).unwrap();
        let choice = inventory
            .choices
            .iter()
            .find(|choice| choice.label == "Location")
            .unwrap();
        let selector = choice.selector.clone();
        let image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_field_context(&selector)
            .unwrap();
        let record = session.as_mut().unwrap().record.as_mut().unwrap();
        let name = record.screenshot_with_name(&image).unwrap();
        record
            .event(json!({"type":"fixture_capture","screenshot":name}))
            .unwrap();
        let revision =
            crate::control_vision::begin_pending(&ws, &job, "choice-visual", now()).unwrap();
        crate::control_vision::set_choice_fixture(
            "choice-visual",
            "choice-session",
            job.as_str(),
            revision,
            (&selector, &digest, &image, "Location"),
            true,
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let request = |ws: &mut Workspace,
                       session: &mut Option<ReviewSession>,
                       path: &str,
                       body: Value|
         -> Result<Value, String> {
            let body = body.to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            let served = crate::serve(&mut server, ws, session, "fixture-token", port);
            drop(server);
            served?;
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1)
                .map_err(|error| error.to_string())
        };
        let evidence = request(
            &mut ws,
            &mut session,
            "/api/form/control-vision-status",
            json!({"task_id":"choice-visual"}),
        )
        .unwrap();
        assert_eq!(evidence["comparison"], "label_observed");
        assert_eq!(evidence["result_saved"], true);
        let unreviewed = json!({"session_id":"choice-session","visual_task_id":"choice-visual","reviewed":false,"source_reviewed":true});
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-open",
            unreviewed
        )
        .is_err());
        let approved = json!({"session_id":"choice-session","visual_task_id":"choice-visual","reviewed":true,"source_reviewed":true});
        let started = request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-open",
            approved.clone(),
        )
        .unwrap();
        let task_id = started["task_id"].as_str().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let result = loop {
            let result = request(
                &mut ws,
                &mut session,
                "/api/form/custom-choice-action-status",
                json!({"task_id":task_id}),
            )
            .unwrap();
            if result["status"] != "running" {
                break result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(result["status"], "complete", "{result}");
        assert_eq!(result["opened"], true);
        assert_eq!(result["record_saved"], true);
        assert_eq!(result["visual_review_required"], true);
        assert!(session.as_ref().unwrap().pending_choice_visual.is_some());
        assert!(!form_preview::same_reviewable_walls(
            &session.as_ref().unwrap().preview,
            &[]
        ));
        let clicks = session
            .as_ref()
            .unwrap()
            .driver
            .trace
            .iter()
            .filter(|event| event.contains("clicked"))
            .count();
        let replay = request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-action-status",
            json!({"task_id":task_id}),
        )
        .unwrap();
        assert_eq!(replay["opened"], true);
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-open",
            approved
        )
        .is_err());
        let (inventory, digest) =
            crate::custom_choice_snapshot(&mut session.as_mut().unwrap().driver).unwrap();
        assert!(inventory
            .choices
            .iter()
            .any(|choice| choice.selector == selector && choice.expanded));
        for option in ["Missing", "Duplicate"] {
            assert!(request(&mut ws, &mut session, "/api/form/custom-choice-option-vision-start", json!({"session_id":"choice-session","selector":selector,"option":option,"snapshot_digest":digest})).is_err());
        }
        let image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_field_context(&selector)
            .unwrap();
        session
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .screenshot_with_name(&image)
            .unwrap();
        let option_revision =
            crate::control_vision::begin_pending(&ws, &job, "option-visual", now()).unwrap();
        let option_image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_base64()
            .unwrap();
        session
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .screenshot_with_name(&option_image)
            .unwrap();
        crate::control_vision::set_option_fixture(
            "option-visual",
            "choice-session",
            job.as_str(),
            option_revision,
            (&selector, &digest, &option_image, "Berlin"),
        );
        let option_review = request(
            &mut ws,
            &mut session,
            "/api/form/control-vision-status",
            json!({"task_id":"option-visual"}),
        )
        .unwrap();
        assert_eq!(option_review["comparison"], "label_observed");
        assert!(session.as_ref().unwrap().pending_choice_visual.is_some());
        assert!(request(&mut ws, &mut session, "/api/form/custom-choice-open", json!({"session_id":"choice-session","visual_task_id":"option-visual","reviewed":true,"source_reviewed":true})).is_err());
        let revision =
            crate::control_vision::begin_pending(&ws, &job, "choice-after", now()).unwrap();
        crate::control_vision::set_choice_fixture(
            "choice-after",
            "choice-session",
            job.as_str(),
            revision,
            (&selector, &digest, &image, "Location"),
            true,
        );
        let visual = request(
            &mut ws,
            &mut session,
            "/api/form/control-vision-status",
            json!({"task_id":"choice-after"}),
        )
        .unwrap();
        assert_eq!(visual["post_choice_review_finished"], true);
        assert!(session.as_ref().unwrap().pending_choice_visual.is_none());
        let current = session.as_mut().unwrap();
        assert_eq!(
            current
                .driver
                .trace
                .iter()
                .filter(|event| event.contains("clicked"))
                .count(),
            clicks
        );
        let (text, _) = current.driver.page_text_bounded(2000).unwrap();
        assert!(text.contains("NOT SUBMITTED"), "{text}");
        let (_, digest) =
            crate::custom_choice_snapshot(&mut session.as_mut().unwrap().driver).unwrap();
        let image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_base64()
            .unwrap();
        session
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .screenshot_with_name(&image)
            .unwrap();
        let revision =
            crate::control_vision::begin_pending(&ws, &job, "option-select", now()).unwrap();
        crate::control_vision::set_option_fixture(
            "option-select",
            "choice-session",
            job.as_str(),
            revision,
            (&selector, &digest, &image, "Berlin"),
        );
        request(
            &mut ws,
            &mut session,
            "/api/form/control-vision-status",
            json!({"task_id":"option-select"}),
        )
        .unwrap();
        let approved = json!({"session_id":"choice-session","visual_task_id":"option-select","reviewed":true,"source_reviewed":true});
        let started = request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-select",
            approved.clone(),
        )
        .unwrap();
        let action_id = started["task_id"].as_str().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let selected = loop {
            let value = request(
                &mut ws,
                &mut session,
                "/api/form/custom-choice-action-status",
                json!({"task_id":action_id}),
            )
            .unwrap();
            if value["status"] != "running" {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(selected["selected"], true, "{:?}", selected["error"]);
        assert_eq!(selected["option"], "Berlin");
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-select",
            approved
        )
        .is_err());
        assert!(session.as_ref().unwrap().pending_choice_visual.is_some());
        let (_, digest) =
            crate::custom_choice_snapshot(&mut session.as_mut().unwrap().driver).unwrap();
        let field_image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_field_context(&selector)
            .unwrap();
        session
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .screenshot_with_name(&field_image)
            .unwrap();
        let revision =
            crate::control_vision::begin_pending(&ws, &job, "wrong-visual", now()).unwrap();
        crate::control_vision::set_choice_fixture(
            "wrong-visual",
            "choice-session",
            job.as_str(),
            revision,
            (&selector, &digest, &field_image, "Location"),
            true,
        );
        let wrong = request(
            &mut ws,
            &mut session,
            "/api/form/control-vision-status",
            json!({"task_id":"wrong-visual"}),
        )
        .unwrap();
        assert_eq!(wrong["comparison"], "label_observed");
        assert!(session.as_ref().unwrap().pending_choice_visual.is_some());
        let image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_control(&selector)
            .unwrap();
        session
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .screenshot_with_name(&image)
            .unwrap();
        let task = if std::env::var("WAYPOINT_CHOICE_LIVE_VISION").as_deref() == Ok("1") {
            request(
                &mut ws,
                &mut session,
                "/api/form/custom-choice-selected-vision-start",
                json!({"session_id":"choice-session","selector":selector,"snapshot_digest":digest}),
            )
            .unwrap()["task_id"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
            let revision =
                crate::control_vision::begin_pending(&ws, &job, "selected-visual", now()).unwrap();
            crate::control_vision::set_selected_fixture(
                "selected-visual",
                "choice-session",
                job.as_str(),
                revision,
                (&selector, &digest, &image, "Berlin"),
            );
            "selected-visual".to_owned()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let visual = loop {
            let value = request(
                &mut ws,
                &mut session,
                "/api/form/control-vision-status",
                json!({"task_id":task}),
            )
            .unwrap();
            if value["status"] != "running" {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(visual["post_choice_review_finished"], true);
        assert!(session.as_ref().unwrap().pending_choice_visual.is_none());
        assert_eq!(
            session.as_ref().unwrap().selected_choices,
            vec![(selector.clone(), "Berlin".into())]
        );
        assert!(crate::verify_selected_choices(session.as_mut().unwrap()).is_ok());
        session
            .as_mut()
            .unwrap()
            .driver
            .select_custom_choice_option(&selector, "Lisbon")
            .unwrap();
        assert!(crate::verify_selected_choices(session.as_mut().unwrap()).is_err());
        let inventory = request(
            &mut ws,
            &mut session,
            "/api/form/custom-choices",
            json!({"session_id":"choice-session"}),
        )
        .unwrap();
        assert_eq!(inventory["can_open"], false);
        let error = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            json!({"session_id":"choice-session","reviewed":true}),
        )
        .unwrap_err();
        assert!(error.contains("previously selected choice"), "{error}");
        let (text, _) = session
            .as_mut()
            .unwrap()
            .driver
            .page_text_bounded(2000)
            .unwrap();
        assert!(text.contains("NOT SUBMITTED"), "{text}");
    }
}
