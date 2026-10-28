//! One bounded application-email worker. No UI or HTTP route starts it yet.
use crate::application_email::PreparedApplicationEmail;
use std::sync::{mpsc, Mutex};
use waypoint_domain::JobIdentityId;
use waypoint_store::email_handoff::EmailHandoffOutcome;
use waypoint_workspace::Workspace;

struct Task {
    id: String,
    receiver: mpsc::Receiver<EmailHandoffOutcome>,
    result: Option<EmailHandoffOutcome>,
    record: Option<(JobIdentityId, String)>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

/// Reconciliation must wait for any live or uncollected worker result.
pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

#[derive(Debug, PartialEq, Eq)]
pub enum WorkerStatus {
    Running,
    Finished {
        outcome: EmailHandoffOutcome,
        record_saved: Option<bool>,
    },
}

/// No network action starts until the one-use approval and reservation are
/// committed. The UI must also preserve the packet and ask for submission.
pub fn start_gmail(
    ws: &mut Workspace,
    job: &JobIdentityId,
    approval_id: &str,
    attempt_id: String,
    now: i64,
) -> Result<(), String> {
    let (_, packet) = crate::attempt_record::load_gmail_review(ws, job, &attempt_id)?;
    let credentials = packet.connected_credentials(ws)?;
    start_with(
        ws,
        job,
        approval_id,
        attempt_id.clone(),
        packet,
        now,
        move |packet| PreparedApplicationEmail::transmit(&packet.reviewed(), &credentials),
    )
}

fn start_with<F>(
    ws: &mut Workspace,
    job: &JobIdentityId,
    approval_id: &str,
    handoff_id: String,
    packet: PreparedApplicationEmail,
    now: i64,
    transport: F,
) -> Result<(), String>
where
    F: FnOnce(PreparedApplicationEmail) -> EmailHandoffOutcome + Send + 'static,
{
    let mut task = TASK.lock().map_err(|_| "Email worker unavailable")?;
    if task.is_some() {
        return Err("An email handoff is running or awaiting its result".into());
    }
    let record = ws
        .db_path
        .is_some()
        .then(|| (job.clone(), handoff_id.clone()));
    ws.reserve_reviewed_email(job, approval_id, &handoff_id, &packet.reviewed(), now)?;
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("waypoint-email-handoff".into())
        .spawn(move || {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| transport(packet)))
                    .unwrap_or(EmailHandoffOutcome::Uncertain);
            let _ = sender.send(outcome);
        });
    if spawned.is_err() {
        ws.finish_reviewed_email(&handoff_id, EmailHandoffOutcome::NotSent)?;
        return Err("Email worker could not start; no message was sent".into());
    }
    *task = Some(Task {
        id: handoff_id,
        receiver,
        result: None,
        record,
    });
    Ok(())
}

/// Polling records a completed result once. If recording fails, retain the
/// result for another database write attempt, never another SMTP handoff.
pub fn poll(ws: &mut Workspace, id: &str) -> Result<WorkerStatus, String> {
    let mut tasks = TASK.lock().map_err(|_| "Email worker unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Email worker not found; check the saved handoff before any retry")?;
    if task.result.is_none() {
        match task.receiver.try_recv() {
            Ok(outcome) => task.result = Some(outcome),
            Err(mpsc::TryRecvError::Empty) => return Ok(WorkerStatus::Running),
            Err(mpsc::TryRecvError::Disconnected) => {
                task.result = Some(EmailHandoffOutcome::Uncertain)
            }
        }
    }
    let Some(outcome) = task.result else {
        return Err("Email worker result unavailable; check the saved handoff".into());
    };
    ws.finish_reviewed_email(id, outcome)?;
    let record_saved = task.record.as_ref().map(|(job, attempt)| {
        crate::attempt_record::record_email_outcome(ws, job, attempt, outcome, crate::now()).is_ok()
    });
    *tasks = None;
    Ok(WorkerStatus::Finished {
        outcome,
        record_saved,
    })
}

/// Collect a completed worker independently of an open browser page. This
/// writes only the result; it never starts or retries a transport.
pub fn collect_finished(ws: &mut Workspace) -> Result<Option<WorkerStatus>, String> {
    let id = TASK
        .lock()
        .map_err(|_| "Email worker unavailable")?
        .as_ref()
        .map(|task| task.id.clone());
    match id {
        Some(id) => poll(ws, &id).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::ApplicationState;
    #[test]
    fn worker_reserves_before_transport_and_records_one_outcome() {
        let directory = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let job = JobIdentityId::new("worker-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job.clone(),
            title: "Role".into(),
            employer: "Fixture".into(),
            location: "".into(),
            state: ApplicationState::Shortlisted,
            canonical_url: None,
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "fixture".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let attempt = "00000000000000000000000000000001";
        crate::attempt_record::AttemptRecord::start(&ws, &job, attempt, 1)
            .unwrap()
            .unwrap();
        let packet = PreparedApplicationEmail::prepare(
            "sender@example.invalid",
            "recipient@example.invalid",
            "Role",
            "Fictional",
            &[],
            "abc123",
        )
        .unwrap();
        packet.approve(&mut ws, &job, "approval", 1, true).unwrap();
        let (release, wait) = mpsc::channel();
        start_with(
            &mut ws,
            &job,
            "approval",
            attempt.into(),
            packet,
            2,
            move |_| {
                wait.recv().unwrap();
                EmailHandoffOutcome::Accepted
            },
        )
        .unwrap();
        assert!(busy());
        assert_eq!(
            ws.email_handoff_history(&job).unwrap().0[0].state,
            "reserved"
        );
        assert_eq!(
            collect_finished(&mut ws).unwrap(),
            Some(WorkerStatus::Running)
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_worker = stop.clone();
        let server = std::thread::spawn(move || {
            crate::serve_loop(ws, listener, "fixture-token".into(), port, stop_worker)
        });
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while busy() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let collected = !busy();
        stop.store(true, std::sync::atomic::Ordering::Release);
        server.join().unwrap();
        assert!(collected, "server did not collect the worker outcome");
        let mut ws = Workspace::open(directory.path()).unwrap();
        assert_eq!(
            ws.email_handoff_history(&job).unwrap().0[0].state,
            "accepted"
        );
        assert!(poll(&mut ws, attempt).is_err());
        assert!(!busy());
        let folder = std::fs::read_dir(directory.path().join("application-records"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let outcome: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.join("email-handoff.json")).unwrap())
                .unwrap();
        assert_eq!(outcome["state"], "accepted");
    }
}
