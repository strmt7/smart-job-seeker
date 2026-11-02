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
    scroll: Option<bool>,
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
            if let Some(down) = scroll {
                session
                    .driver
                    .scroll_custom_choice_options(&selector, down)
                    .map(Some)
            } else if let Some(value) = option.as_deref() {
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
            (Some(record), Some(image)) => record.screenshot_with_name(image).and_then(|name| record.event(json!({"type":"custom_choice_action_result","time":now(),"selector":selector,"option":option,"scroll_down":scroll,"succeeded":action.is_ok(),"screenshot":format!("screenshots/{name}"),"note":"A new visual review is required before further action."}))).is_ok(),
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
        let attachments_verified = crate::choice_attachments_verified(&mut session);
        let previous_verified = previous_verified && attachments_verified;
        let result = match action {
            Ok(Some(choice)) => {
                json!({"status":"complete","opened":scroll.is_none(),"scrolled":scroll.is_some(),"choice":choice,"screenshot":screenshot,"record_saved":saved,"previous_fields_verified":previous_verified,"attachments_verified":attachments_verified,"visual_review_required":true})
            }
            Ok(None) => {
                json!({"status":"complete","selected":true,"option":option,"screenshot":screenshot,"record_saved":saved,"previous_fields_verified":previous_verified,"attachments_verified":attachments_verified,"visual_review_required":true})
            }
            Err(error) => {
                json!({"status":"uncertain","opened":false,"error":error,"screenshot":screenshot,"record_saved":saved,"previous_fields_verified":previous_verified,"attachments_verified":attachments_verified,"visual_review_required":true})
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
    #[ignore = "starts one browser on a fictional form that clears an upload"]
    fn choice_action_reports_upload_cleared_by_page() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_choice_clears_upload.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = form_preview::suggest_without_model(live, &json!({}), "fixture").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let job = JobIdentityId::new("cleared-upload-fixture").unwrap();
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
            crate::attempt_record::AttemptRecord::start(&ws, &job, "cleared-upload", now())
                .unwrap()
                .unwrap();
        let staged = dir.path().join("staged.pdf");
        std::fs::write(&staged, b"fictional document").unwrap();
        driver.attach_local_file("#upload", &staged).unwrap();
        let mut session = ReviewSession {
            id: "upload-session".into(),
            job_id: job.clone(),
            inspection_task_id: "fixture".into(),
            inspection_pending_revision: 0,
            url: url.to_string(),
            route_binding: None,
            profile_digest: Sha256Digest::of(&json!({}).to_string()),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: false,
            fill_attempted: false,
            completed_selectors: vec![],
            prior_sections: vec![],
            prior_sections_omitted: false,
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![(
                "#upload".into(),
                staged,
                "upload-session".into(),
                waypoint_domain::sha256_hex(b"fictional document"),
            )],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: Some(record),
        };
        let (inventory, _) = crate::custom_choice_snapshot(&mut session.driver).unwrap();
        let selector = inventory
            .choices
            .iter()
            .find(|choice| choice.label == "Location")
            .unwrap()
            .selector
            .clone();
        start("upload-choice".into(), session, selector, None, None, 1)
            .map_err(|(_, error)| error)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut returned_session = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let body = json!({"task_id":"upload-choice"}).to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client, "POST /api/form/custom-choice-action-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(
                &mut server,
                &mut ws,
                &mut returned_session,
                "fixture-token",
                port,
            )
            .unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            let result: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            if result["status"] == "running" {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            assert_eq!(result["opened"], true, "{}", result["error"]);
            assert_eq!(result["attachments_verified"], false);
            assert_eq!(result["previous_fields_verified"], false);
            assert!(ws
                .manual_review_reasons(&job)
                .unwrap()
                .iter()
                .any(|reason| reason.reason
                    == "Previously attached files changed or could not be verified"));
            break;
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "resource-gated visual review and actions on a fictional form"]
    fn resource_gated_reviewed_choice_workflow() {
        assert_eq!(
            std::env::var("WAYPOINT_CHOICE_LIVE_VISION").as_deref(),
            Ok("1")
        );
        crate::experience_inference::tests::with_resource_gated_model(
            reviewed_open_is_recorded_one_use_and_recoverable,
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "resource-gated local vision model and fictional multi-reference choice"]
    fn resource_gated_multi_reference_choice_workflow() {
        assert_eq!(
            std::env::var("WAYPOINT_CHOICE_LIVE_VISION").as_deref(),
            Ok("1")
        );
        crate::experience_inference::tests::with_resource_gated_model(
            reviewed_multi_reference_choice_is_recorded_and_recoverable,
        );
    }

    #[test]
    #[ignore = "starts one browser and opens only a fictional choice"]
    fn reviewed_open_is_recorded_one_use_and_recoverable() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_custom_choice_open.html");
        exercise_reviewed_choice(&fixture);
    }

    #[test]
    #[ignore = "starts one browser and reviews a fictional multi-reference choice"]
    fn reviewed_multi_reference_choice_is_recorded_and_recoverable() {
        let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_custom_choice_open.html");
        let html = std::fs::read_to_string(original).unwrap();
        assert_eq!(html.matches("aria-controls=\"locations\"").count(), 1);
        let html = html
            .replace(
                "aria-controls=\"locations\"",
                "aria-controls=\"location-help locations\"",
            )
            .replace(
                "<form onsubmit=",
                "<div id=\"location-help\">Choose a location.</div><form onsubmit=",
            );
        let dir = tempfile::tempdir().unwrap();
        let fixture = dir.path().join("choice.html");
        std::fs::write(&fixture, html).unwrap();
        exercise_reviewed_choice(&fixture);
    }

    fn exercise_reviewed_choice(fixture: &std::path::Path) {
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
        let mut cursor = String::new();
        for _ in 0..16 {
            if !ws.duplicate_index_pending().unwrap() {
                break;
            }
            cursor = ws
                .index_next_duplicate_step(&cursor)
                .unwrap()
                .cursor
                .unwrap_or_default();
        }
        assert!(
            !ws.duplicate_index_pending().unwrap(),
            "fixture duplicate indexing did not settle"
        );
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
            route_binding: None,
            profile_digest: Sha256Digest::of(&json!({}).to_string()),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: false,
            fill_attempted: false,
            completed_selectors: vec![],
            prior_sections: vec![],
            prior_sections_omitted: false,
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: Some(record),
        });
        // A later choice action must not silently accept an attachment that the
        // page replaced, even when the staged file itself is unchanged.
        let staged = dir.path().join("staged.pdf");
        let replacement = dir.path().join("replacement.pdf");
        std::fs::write(&staged, b"fixture original").unwrap();
        std::fs::write(&replacement, b"fixture replacement").unwrap();
        let current = session.as_mut().unwrap();
        current
            .driver
            .attach_local_file("#fixture-upload", &staged)
            .unwrap();
        current.attachments.push((
            "#fixture-upload".into(),
            staged.clone(),
            current.id.clone(),
            waypoint_domain::sha256_hex(b"fixture original"),
        ));
        assert!(crate::choice_attachments_verified(current));
        current
            .driver
            .attach_local_file("#fixture-upload", &replacement)
            .unwrap();
        assert!(!crate::choice_attachments_verified(current));
        current.attachments.clear();
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
        let visual_id = if std::env::var("WAYPOINT_CHOICE_LIVE_VISION").as_deref() == Ok("1") {
            request(
                &mut ws,
                &mut session,
                "/api/form/custom-choice-vision-start",
                json!({"session_id":"choice-session","selector":selector,"snapshot_digest":digest}),
            )
            .unwrap()["task_id"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
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
            "choice-visual".to_owned()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let evidence = loop {
            let result = request(
                &mut ws,
                &mut session,
                "/api/form/control-vision-status",
                json!({"task_id":visual_id}),
            )
            .unwrap();
            if result["status"] != "running" {
                break result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        println!(
            "choice visual expected={} observed={} comparison={}",
            evidence["label"], evidence["buttons"], evidence["comparison"]
        );
        assert_eq!(evidence["comparison"], "label_observed");
        assert_eq!(evidence["result_saved"], true);
        let unreviewed = json!({"session_id":"choice-session","visual_task_id":visual_id,"reviewed":false,"source_reviewed":true});
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-open",
            unreviewed
        )
        .is_err());
        let approved = json!({"session_id":"choice-session","visual_task_id":visual_id,"reviewed":true,"source_reviewed":true});
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
        let exact = request(&mut ws, &mut session, "/api/form/custom-choice-advice-start", json!({"session_id":"choice-session","selector":selector,"snapshot_digest":digest,"answer":" Berlin "})).unwrap();
        assert_eq!(exact["status"], "complete");
        assert_eq!(exact["method"], "exact");
        assert_eq!(exact["radio"], false);
        assert_eq!(exact["advice"]["option"], "Berlin");
        assert!(exact["task_id"].is_null());
        assert!(request(&mut ws, &mut session, "/api/form/custom-choice-advice-start", json!({"session_id":"choice-session","selector":selector,"snapshot_digest":digest,"answer":"Duplicate"})).is_err());
        assert!(request(&mut ws, &mut session, "/api/form/custom-choice-advice-start", json!({"session_id":"choice-session","selector":selector,"snapshot_digest":"0".repeat(64),"answer":"Berlin"})).is_err());
        let advice_task = if std::env::var("WAYPOINT_CHOICE_ADVICE_LIVE").as_deref() == Ok("1") {
            request(&mut ws, &mut session, "/api/form/custom-choice-advice-start", json!({"session_id":"choice-session","selector":selector,"snapshot_digest":digest,"answer":"I prefer Berlin"})).unwrap()["task_id"].as_str().unwrap().to_owned()
        } else {
            crate::choice_advice::set_fixture("choice-advice", "choice-session", &digest);
            "choice-advice".to_owned()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let advice = loop {
            let value = request(
                &mut ws,
                &mut session,
                "/api/form/custom-choice-advice-status",
                json!({"task_id":advice_task}),
            )
            .unwrap();
            if value["status"] != "running" {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(advice["advice"]["option"], "Berlin", "{advice}");
        crate::choice_advice::set_fixture("choice-advice-stale", "choice-session", "stale");
        let stale = request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-advice-status",
            json!({"task_id":"choice-advice-stale"}),
        )
        .unwrap();
        assert_eq!(stale["status"], "failed");
        assert!(stale["advice"].is_null());
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
            .screenshot_custom_choice_option(&selector, "Berlin")
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
        assert_eq!(
            visual["post_choice_review_finished"], true,
            "{:?}",
            visual["comparison"]
        );
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
            .screenshot_custom_choice_option(&selector, "Berlin")
            .unwrap();
        session
            .as_mut()
            .unwrap()
            .record
            .as_mut()
            .unwrap()
            .screenshot_with_name(&image)
            .unwrap();
        let option_task = if std::env::var("WAYPOINT_CHOICE_LIVE_VISION").as_deref() == Ok("1") {
            request(&mut ws, &mut session, "/api/form/custom-choice-option-vision-start", json!({"session_id":"choice-session","selector":selector,"option":"Berlin","snapshot_digest":digest})).unwrap()["task_id"].as_str().unwrap().to_owned()
        } else {
            let revision =
                crate::control_vision::begin_pending(&ws, &job, "option-select", now()).unwrap();
            crate::control_vision::set_option_fixture(
                "option-select",
                "choice-session",
                job.as_str(),
                revision,
                (&selector, &digest, &image, "Berlin"),
            );
            "option-select".to_owned()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let option_result = loop {
            let value = request(
                &mut ws,
                &mut session,
                "/api/form/control-vision-status",
                json!({"task_id":option_task}),
            )
            .unwrap();
            if value["status"] != "running" {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(option_result["comparison"], "label_observed");
        let approved = json!({"session_id":"choice-session","visual_task_id":option_task,"reviewed":true,"source_reviewed":true});
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
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                selector: "#name".into(),
                value: waypoint_applications::form_mapping::PacketValue::Text(
                    "Fictional applicant".into(),
                ),
            })
            .unwrap();
        session
            .as_mut()
            .unwrap()
            .completed_selectors
            .push("#name".into());
        let rescan = request(
            &mut ws,
            &mut session,
            "/api/form/rescan-start",
            json!({"session_id":"choice-session"}),
        )
        .unwrap();
        let rescan_id = rescan["task_id"].as_str().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        let rescanned = loop {
            let value = request(
                &mut ws,
                &mut session,
                "/api/form/open-link-status",
                json!({"task_id":rescan_id}),
            )
            .unwrap();
            if value["status"] != "running" {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert_eq!(rescanned["status"], "complete");
        let resumed_session = rescanned["session_id"].as_str().unwrap().to_owned();
        assert_ne!(resumed_session, "choice-session");
        assert!(session
            .as_ref()
            .unwrap()
            .preview
            .manual_review
            .iter()
            .any(|reason| reason == form_preview::POST_CHOICE_VISUAL_REQUIRED));
        assert!(session
            .as_ref()
            .unwrap()
            .preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("Fields filled before this inspection")));
        let inventory = request(
            &mut ws,
            &mut session,
            "/api/form/custom-choices",
            json!({"session_id":resumed_session}),
        )
        .unwrap();
        assert_eq!(inventory["can_open"], false);
        assert_eq!(inventory["pending_selected"]["option"], "Berlin");
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
            &resumed_session,
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
                json!({"session_id":resumed_session,"selector":selector,"snapshot_digest":digest}),
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
                &resumed_session,
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
            json!({"session_id":resumed_session}),
        )
        .unwrap();
        assert_eq!(inventory["can_open"], false);
        let error = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            json!({"session_id":resumed_session,"reviewed":true}),
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

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_custom_choice_scroll.html");
        let scroll_url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let scroll_job = JobIdentityId::new("scroll-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: scroll_job.clone(),
            title: "Different opportunity".into(),
            employer: "Another fixture".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(scroll_url.to_string()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let mut cursor = String::new();
        for _ in 0..16 {
            if !ws.duplicate_index_pending().unwrap() {
                break;
            }
            cursor = ws
                .index_next_duplicate_step(&cursor)
                .unwrap()
                .cursor
                .unwrap_or_default();
        }
        assert!(
            !ws.duplicate_index_pending().unwrap(),
            "fixture duplicate indexing did not settle"
        );
        let mut current = session.take().unwrap();
        let live = current.driver.open(scroll_url.as_str()).unwrap();
        current.id = "scroll-session".into();
        current.job_id = scroll_job.clone();
        current.url = scroll_url.to_string();
        current.preview = form_preview::suggest_without_model(live, &json!({}), "fixture").unwrap();
        current.selected_choices.clear();
        current.completed_selectors.clear();
        current.filled = false;
        current.pending_choice_visual = None;
        current.record =
            crate::attempt_record::AttemptRecord::start(&ws, &scroll_job, "scroll-fixture", now())
                .unwrap();
        session = Some(current);
        let (inventory, digest) =
            crate::custom_choice_snapshot(&mut session.as_mut().unwrap().driver).unwrap();
        let selector = inventory
            .choices
            .iter()
            .find(|choice| choice.label == "City")
            .unwrap()
            .selector
            .clone();
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
        let revision =
            crate::control_vision::begin_pending(&ws, &scroll_job, "scroll-visual", now()).unwrap();
        crate::control_vision::set_choice_fixture(
            "scroll-visual",
            "scroll-session",
            scroll_job.as_str(),
            revision,
            (&selector, &digest, &image, "City"),
            true,
        );
        request(
            &mut ws,
            &mut session,
            "/api/form/control-vision-status",
            json!({"task_id":"scroll-visual"}),
        )
        .unwrap();
        let boundary = json!({"session_id":"scroll-session","visual_task_id":"scroll-visual","direction":"up","reviewed":true,"source_reviewed":true});
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-scroll",
            boundary
        )
        .unwrap_err()
        .contains("boundary"));
        let approval = json!({"session_id":"scroll-session","visual_task_id":"scroll-visual","direction":"down","reviewed":true,"source_reviewed":true});
        let started = request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-scroll",
            approval.clone(),
        )
        .unwrap();
        let task = started["task_id"].as_str().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let scrolled = loop {
            let value = request(
                &mut ws,
                &mut session,
                "/api/form/custom-choice-action-status",
                json!({"task_id":task}),
            )
            .unwrap();
            if value["status"] != "running" {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(scrolled["scrolled"], true, "{:?}", scrolled["error"]);
        assert_eq!(scrolled["record_saved"], true);
        assert!(session.as_ref().unwrap().pending_choice_visual.is_some());
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/custom-choice-scroll",
            approval
        )
        .is_err());
        let (_, digest) =
            crate::custom_choice_snapshot(&mut session.as_mut().unwrap().driver).unwrap();
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
        let task = if std::env::var("WAYPOINT_CHOICE_LIVE_VISION").as_deref() == Ok("1") {
            request(
                &mut ws,
                &mut session,
                "/api/form/custom-choice-vision-start",
                json!({"session_id":"scroll-session","selector":selector,"snapshot_digest":digest}),
            )
            .unwrap()["task_id"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
            let revision =
                crate::control_vision::begin_pending(&ws, &scroll_job, "scroll-after", now())
                    .unwrap();
            crate::control_vision::set_choice_fixture(
                "scroll-after",
                "scroll-session",
                scroll_job.as_str(),
                revision,
                (&selector, &digest, &image, "City"),
                true,
            );
            "scroll-after".to_owned()
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
        if std::env::var("WAYPOINT_CHOICE_LIVE_VISION").as_deref() == Ok("1")
            && visual["comparison"] == "not_observed"
        {
            println!(
                "Live vision did not observe label {}; observed {}; review remains pending",
                visual["label"], visual["buttons"]
            );
            assert!(session.as_ref().unwrap().pending_choice_visual.is_some());
            return;
        }
        assert_eq!(
            visual["post_choice_review_finished"], true,
            "{:?}",
            visual["comparison"]
        );
        assert!(session.as_ref().unwrap().pending_choice_visual.is_none());
        println!(
            "Post-scroll visual confirmed label {} from {}",
            visual["label"], visual["buttons"]
        );
        let (text, _) = session
            .as_mut()
            .unwrap()
            .driver
            .page_text_bounded(5000)
            .unwrap();
        assert!(text.contains("NOT SUBMITTED"));
    }
}
