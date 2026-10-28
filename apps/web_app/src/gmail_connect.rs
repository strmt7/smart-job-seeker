//! One user-initiated OAuth consent flow. No message is sent.
use crate::gmail_oauth;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use waypoint_workspace::Workspace;
use zeroize::Zeroizing;

struct Task {
    id: String,
    cancel: Arc<AtomicBool>,
    result: Option<Result<gmail_oauth::Credentials, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

pub fn busy() -> bool {
    TASK.lock().map(|task| task.is_some()).unwrap_or(true)
}

pub fn start(id: String, client: Vec<u8>, ws: &Workspace) -> Result<(), String> {
    if !ws.can_store_gmail_credentials() {
        return Err("Gmail connection requires Windows DPAPI credential storage".into());
    }
    if client.is_empty() || client.len() > 32768 {
        return Err("OAuth client file must be 1 to 32768 bytes".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let mut task = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
    if task.is_some() {
        return Err("A Gmail connection is active or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        cancel: cancel.clone(),
        result: None,
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
            }
        }
    });
    Ok(())
}

pub fn poll(id: &str, ws: &Workspace) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Gmail connection unavailable")?;
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
