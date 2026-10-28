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
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

#[derive(Debug, PartialEq, Eq)]
pub enum WorkerStatus {
    Running,
    Finished(EmailHandoffOutcome),
}

/// No network action starts until the one-use approval and reservation are
/// committed. The UI must also preserve the packet and ask for submission.
pub fn start_gmail(
    ws: &mut Workspace,
    job: &JobIdentityId,
    approval_id: &str,
    handoff_id: String,
    packet: PreparedApplicationEmail,
    now: i64,
) -> Result<(), String> {
    let credentials = packet.connected_credentials(ws)?;
    start_with(
        ws,
        job,
        approval_id,
        handoff_id,
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
    *tasks = None;
    Ok(WorkerStatus::Finished(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_domain::ApplicationState;
    #[test]
    fn worker_reserves_before_transport_and_records_one_outcome() {
        let mut ws = Workspace::in_memory().unwrap();
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
            "handoff".into(),
            packet,
            2,
            move |_| {
                wait.recv().unwrap();
                EmailHandoffOutcome::Accepted
            },
        )
        .unwrap();
        assert_eq!(
            ws.email_handoff_history(&job).unwrap().0[0].state,
            "reserved"
        );
        assert_eq!(poll(&mut ws, "handoff").unwrap(), WorkerStatus::Running);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match poll(&mut ws, "handoff").unwrap() {
                WorkerStatus::Finished(outcome) => {
                    assert_eq!(outcome, EmailHandoffOutcome::Accepted);
                    break;
                }
                WorkerStatus::Running if std::time::Instant::now() < deadline => {
                    std::thread::yield_now()
                }
                _ => panic!("fixture worker timed out"),
            }
        }
        assert_eq!(
            ws.email_handoff_history(&job).unwrap().0[0].state,
            "accepted"
        );
        assert!(poll(&mut ws, "handoff").is_err());
    }
}
