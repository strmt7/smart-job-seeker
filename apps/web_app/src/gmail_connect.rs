//! One user-initiated OAuth consent flow. No message is sent.
use crate::gmail_oauth;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use waypoint_workspace::Workspace;
use zeroize::Zeroizing;

struct Task {
    id: String,
    cancel: Arc<AtomicBool>,
    result: Option<Result<gmail_oauth::Credentials, String>>,
    finished_at: Option<Instant>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
const RESULT_LIFETIME: Duration = Duration::from_secs(600);

fn expire_finished(task: &mut Option<Task>, now: Instant) {
    if task
        .as_ref()
        .and_then(|task| task.finished_at)
        .is_some_and(|finished| now.saturating_duration_since(finished) >= RESULT_LIFETIME)
    {
        *task = None;
    }
}

pub fn busy() -> bool {
    TASK.lock()
        .map(|mut task| {
            expire_finished(&mut task, Instant::now());
            task.is_some()
        })
        .unwrap_or(true)
}

pub fn start(id: String, client: Vec<u8>, ws: &Workspace) -> Result<(), String> {
    if crate::gmail_password::busy() {
        return Err("A Gmail connection is already running".into());
    }
    if !ws.can_store_gmail_credentials() {
        return Err("Gmail connection requires Windows DPAPI credential storage".into());
    }
    if client.is_empty() || client.len() > 32768 {
        return Err("OAuth client file must be 1 to 32768 bytes".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let mut task = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    expire_finished(&mut task, Instant::now());
    if task.is_some() {
        return Err("A Gmail connection is active or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        cancel: cancel.clone(),
        result: None,
        finished_at: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            gmail_oauth::login(client, true, &cancel).map_err(|_| {
                "Gmail authorization did not complete; check your OAuth client and consent"
                    .to_owned()
            })
        })
        .unwrap_or_else(|_| Err("Gmail authorization stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
                task.finished_at = Some(Instant::now());
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str, ws: &Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    expire_finished(&mut tasks, Instant::now());
    let task = tasks
        .as_mut()
        .filter(|task| task.id == id)
        .ok_or("Gmail connection not found")?;
    match task.result.take() {
        None => Ok(json!({"status":"running"})),
        Some(Err(error)) => {
            *tasks = None;
            Ok(json!({"status":"failed","error":error}))
        }
        Some(Ok(credentials)) => {
            if task.cancel.load(Ordering::SeqCst) {
                *tasks = None;
                return Ok(json!({"status":"failed","error":"Authorization cancelled"}));
            }
            let encoded = Zeroizing::new(
                serde_json::to_vec(&credentials).map_err(|_| "Cannot encode credentials")?,
            );
            *tasks = None;
            ws.save_gmail_credentials(&encoded)
                .map_err(|_| "Cannot save Gmail credentials securely")?;
            Ok(json!({"status":"complete"}))
        }
    }
}

pub fn cancel(id: &str) -> Result<(), String> {
    let tasks = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    tasks
        .as_ref()
        .filter(|task| task.id == id)
        .ok_or("Gmail connection not found")?
        .cancel
        .store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_expired_finished_results_are_discarded() {
        let now = Instant::now();
        let make_task = |finished_at: Option<Instant>| {
            Some(Task {
                id: "fixture".into(),
                cancel: Arc::new(AtomicBool::new(false)),
                result: finished_at.map(|_| Err("fixture".into())),
                finished_at,
            })
        };
        let mut running = make_task(None);
        expire_finished(&mut running, now + RESULT_LIFETIME);
        assert!(running.is_some());
        let mut completed = make_task(Some(now));
        expire_finished(
            &mut completed,
            now + RESULT_LIFETIME - Duration::from_secs(1),
        );
        assert!(completed.is_some());
        expire_finished(&mut completed, now + RESULT_LIFETIME);
        assert!(completed.is_none());
    }
}
