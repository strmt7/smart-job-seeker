//! Bounded background recapture of an existing application-review session.
use crate::{attempt_record, form_preview, now, ReviewSession};
use serde_json::{json, Value};
use std::sync::{mpsc, Mutex};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::JobIdentityId;
use waypoint_workspace::Workspace;

struct Task {
    id: String,
    session_id: String,
    job: JobIdentityId,
    inspection: String,
    revision: i64,
    rx: mpsc::Receiver<(ReviewSession, Value)>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

fn reason(id: &str) -> String {
    format!("Application recapture pending: {id}")
}
pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}
pub fn pending() -> Result<Value, String> {
    let task = TASK.lock().map_err(|_| "Recapture unavailable")?;
    if let Some(task) = task.as_ref() {
        return Ok(
            json!({"active":true,"task_id":task.id,"session_id":task.session_id,"job_id":task.job.as_str()}),
        );
    }
    let completed = COMPLETED.lock().map_err(|_| "Recapture unavailable")?;
    Ok(completed.as_ref().map_or_else(|| json!({"active":false}), |(id, value)| json!({"active":false,"last":{"task_id":id,"session_id":value["session_id"],"job_id":value["job_id"]}})))
}
pub fn protected_reasons(job: &JobIdentityId) -> Result<Vec<String>, String> {
    let task = TASK.lock().map_err(|_| "Recapture unavailable")?;
    Ok(task
        .as_ref()
        .filter(|task| &task.job == job)
        .map_or_else(Vec::new, |task| {
            vec![
                reason(&task.id),
                crate::form_task::pending_reason(&task.inspection),
            ]
        }))
}
pub fn begin_pending(ws: &Workspace, job: &JobIdentityId, id: &str) -> Result<i64, String> {
    let reason = reason(id);
    ws.mark_manual_review(job, &reason, now())
        .map_err(|e| e.to_string())?;
    ws.manual_review_reasons(job)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|entry| entry.reason == reason)
        .map(|entry| entry.revision)
        .ok_or("Pending capture was not recorded".into())
}
pub fn finish_pending(
    ws: &Workspace,
    job: &JobIdentityId,
    id: &str,
    revision: i64,
) -> Result<(), String> {
    ws.clear_manual_review_reason(job, &reason(id), revision)
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn start(
    id: String,
    mut session: ReviewSession,
    revision: i64,
) -> Result<(), (Box<ReviewSession>, String)> {
    // Keep ownership with the caller on lock failure or an already-active task.
    let mut task = match TASK.lock() {
        Ok(task) => task,
        Err(_) => return Err((Box::new(session), "Recapture unavailable".into())),
    };
    if task.is_some() {
        return Err((Box::new(session), "A recapture is already running".into()));
    }
    let (tx, rx) = mpsc::channel();
    *task = Some(Task {
        id: id.clone(),
        session_id: session.id.clone(),
        job: session.job_id.clone(),
        inspection: session.inspection_task_id.clone(),
        revision,
        rx,
    });
    drop(task);
    std::thread::spawn(move || {
        let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let record = session
                .record
                .as_mut()
                .ok_or("Application record unavailable".to_owned())?;
            session
                .driver
                .with_review_timeout(|driver| attempt_record::capture_page(driver, record, now()))
        }))
        .unwrap_or_else(|_| Err("Recapture stopped unexpectedly".into()));
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
        let fields_ok = check.as_ref().is_none_or(|check| {
            check.selected_retained && check.failed_labels.is_empty() && check.unverified == 0
        });
        let attachments_ok = crate::choice_attachments_verified(&mut session);
        let page_ok = session
            .driver
            .with_review_timeout(|driver| driver.inspect_current())
            .is_ok_and(|live| {
                form_preview::same_reviewable_walls(&session.preview, &live.walls)
                    && live.schema == session.preview.schema
                    && live.origin == session.preview.schema.origin
            })
            && crate::verify_selected_choices(&mut session).is_ok();
        let captured_ok = captured.is_ok();
        let event_saved = session.record.as_mut().is_some_and(|record| {
            let failure_saved = captured.as_ref().err().is_none_or(|error| record.event(json!({"type":"capture_incomplete","time":now(),"error":error})).is_ok());
            let result_saved = record.event(json!({"type":"user_recapture_result","time":now(),"captured":captured_ok,"fields_verified":fields_ok,"attachments_verified":attachments_ok,"page_verified":page_ok,"note":"Capture and checks do not prove application correctness or submission."})).is_ok();
            let status_saved = record.status("manual_review", now()).is_ok();
            failure_saved && result_saved && status_saved
        });
        let guidance = captured
            .as_ref()
            .err()
            .and_then(|error| attempt_record::capture_failure_guidance(error));
        if !captured_ok
            && !session
                .preview
                .manual_review
                .iter()
                .any(|reason| reason == "Attempt screenshot capture was incomplete")
        {
            session
                .preview
                .manual_review
                .push("Attempt screenshot capture was incomplete".into());
        }
        if (!fields_ok || !attachments_ok || !page_ok || !event_saved)
            && !session
                .preview
                .manual_review
                .iter()
                .any(|reason| reason == "Recapture checks need manual review")
        {
            session
                .preview
                .manual_review
                .push("Recapture checks need manual review".into());
        }
        session.created = std::time::Instant::now();
        let result = json!({"status":"complete","session_id":session.id,"job_id":session.job_id.as_str(),"captured":captured_ok,"fields_verified":fields_ok,"fields_checked":check.as_ref().map_or(0, |item| item.verified),"attachments_verified":attachments_ok,"page_verified":page_ok,"event_saved":event_saved,"guidance":guidance,"manual_review_required":true});
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
    let mut tasks = TASK.lock().map_err(|_| "Recapture unavailable")?;
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Recapture unavailable")?
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, value)| Poll::Cached(value.clone()))
            .ok_or("Recapture not found".into());
    };
    match task.rx.try_recv() {
        Ok((session, value)) => {
            let revision = task.revision;
            *tasks = None;
            Ok(Poll::Complete(Box::new(session), value, revision))
        }
        Err(mpsc::TryRecvError::Empty) => Ok(Poll::Running),
        Err(mpsc::TryRecvError::Disconnected) => {
            let job = task.job.clone();
            let revision = task.revision;
            *tasks = None;
            Ok(Poll::Lost(job, revision))
        }
    }
}
pub fn remember(id: &str, value: Value) -> Result<(), String> {
    *COMPLETED.lock().map_err(|_| "Recapture unavailable")? = Some((id.into(), value));
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
    #[ignore = "starts one browser on fictional stable and changing forms"]
    fn recapture_restores_session_and_reports_unstable_page() {
        for (fixture, expected_capture, drift) in [
            (
                "../../crates/waypoint_browser/tests/fixtures/form_address_lines.html",
                true,
                false,
            ),
            (
                "../../crates/waypoint_browser/tests/fixtures/form_address_lines.html",
                true,
                true,
            ),
            ("tests/form-changing-capture.html", false, false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut ws = Workspace::open(dir.path()).unwrap();
            let job = JobIdentityId::new("recapture-fixture").unwrap();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(fixture)
                .canonicalize()
                .unwrap();
            let url = url::Url::from_file_path(path).unwrap();
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
            let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
            let live = driver.open(url.as_str()).unwrap();
            let preview = form_preview::suggest_without_model(live, &json!({}), "fixture").unwrap();
            let record =
                attempt_record::AttemptRecord::start(&ws, &job, "recapture-fixture", now())
                    .unwrap()
                    .unwrap();
            let mut session = Some(ReviewSession {
                id: "recapture-session".into(),
                job_id: job.clone(),
                inspection_task_id: "fixture".into(),
                inspection_pending_revision: 0,
                experience_revisions: vec![],
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
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let request = |ws: &mut Workspace,
                           session: &mut Option<ReviewSession>,
                           path: &str,
                           body: Value|
             -> Result<Value, String> {
                let body = body.to_string();
                let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
                let (mut server, _) = listener.accept().unwrap();
                crate::serve(&mut server, ws, session, "fixture", port)?;
                drop(server);
                let mut response = String::new();
                client.read_to_string(&mut response).unwrap();
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1)
                    .map_err(|error| error.to_string())
            };
            if expected_capture {
                crate::answer_task::set_completed_fixture(
                    "pending-answer",
                    crate::answer_task::Inputs {
                        numeric_only: false,
                        session_id: "recapture-session".into(),
                        selector: "#fixture".into(),
                        digest: "fixture".into(),
                        question: "Question".into(),
                        listing: "Listing".into(),
                        facts: vec!["Fictional fact".into()],
                        available_facts: 1,
                        instructions: String::new(),
                        max_length_utf16: None,
                        min_length_utf16: None,
                    },
                    json!({"text":"Fictional","sources":["Fictional fact"]}),
                );
                assert!(request(
                    &mut ws,
                    &mut session,
                    "/api/form/recapture-start",
                    json!({"session_id":"recapture-session"})
                )
                .is_err());
                assert!(session.is_some());
                crate::answer_task::complete("pending-answer", json!({"status":"failed"})).unwrap();
            }
            let started = request(
                &mut ws,
                &mut session,
                "/api/form/recapture-start",
                json!({"session_id":"recapture-session"}),
            )
            .unwrap();
            assert!(session.is_none());
            let task = started["task_id"].as_str().unwrap();
            assert_eq!(
                request(
                    &mut ws,
                    &mut session,
                    "/api/form/recapture-pending",
                    json!({})
                )
                .unwrap()["task_id"],
                task
            );
            assert!(request(
                &mut ws,
                &mut session,
                "/api/form/recapture-start",
                json!({"session_id":"recapture-session"})
            )
            .is_err());
            if drift {
                ws.save_candidate_profile(&json!({"city":"Changed fictional city"}).to_string())
                    .unwrap();
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let result = loop {
                let value = if expected_capture {
                    // The server's background collector uses this same path
                    // even when the web page is closed.
                    crate::settle_form_recapture(&ws, &mut session, task).unwrap()
                } else {
                    request(
                        &mut ws,
                        &mut session,
                        "/api/form/recapture-status",
                        json!({"task_id":task}),
                    )
                    .unwrap()
                };
                if value["status"] != "running" {
                    break value;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(50));
            };
            assert_eq!(result["captured"], expected_capture, "{result}");
            assert_eq!(result["event_saved"], true);
            assert_eq!(result["context_current"], !drift);
            assert_eq!(result["manual_review_required"], true);
            assert!(session.is_some());
            assert_eq!(
                request(
                    &mut ws,
                    &mut session,
                    "/api/form/recapture-pending",
                    json!({})
                )
                .unwrap()["last"]["task_id"],
                task
            );
            assert_eq!(
                request(
                    &mut ws,
                    &mut session,
                    "/api/form/recapture-status",
                    json!({"task_id":task})
                )
                .unwrap(),
                result
            );
            assert!(ws
                .manual_review_reasons(&job)
                .unwrap()
                .iter()
                .any(|reason| reason.reason == "Application needs manual review"));
            assert!(session
                .unwrap()
                .driver
                .trace
                .iter()
                .all(|event| !event.contains("submitted")));
        }
    }
}
