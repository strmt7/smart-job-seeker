//! Bounded, single-result background slot for local advisory inference.
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use waypoint_inference::{Inference, OllamaInference};

struct Work {
    id: String,
    binding: Value,
    result: Option<Result<Value, String>>,
    completed: Option<Instant>,
}
#[derive(Default)]
struct State {
    active: Option<Work>,
    cached: Option<(String, Value, Instant)>,
}
pub struct Slot(Mutex<State>);
impl Slot {
    #[cfg(test)]
    pub(crate) fn set_completed_fixture(&self, id: &str, binding: Value, value: Value) {
        let mut state = self.0.lock().unwrap();
        state.cached = None;
        state.active = Some(Work {
            id: id.into(),
            binding,
            result: Some(Ok(value)),
            completed: Some(Instant::now()),
        });
    }
    pub const fn new() -> Self {
        Self(Mutex::new(State {
            active: None,
            cached: None,
        }))
    }
    fn expire(state: &mut State) {
        if state
            .active
            .as_ref()
            .and_then(|work| work.completed)
            .is_some_and(|at| at.elapsed() > Duration::from_secs(600))
        {
            state.active = None;
        }
        if state
            .cached
            .as_ref()
            .is_some_and(|(_, _, at)| at.elapsed() > Duration::from_secs(600))
        {
            state.cached = None;
        }
    }
    pub fn pending(&self) -> Result<Value, String> {
        let mut state = self.0.lock().map_err(|_| "Local advice unavailable")?;
        Self::expire(&mut state);
        Ok(state.active.as_ref().map_or_else(
            || json!({"active":false}),
            |work| json!({"active":true,"task_id":work.id,"binding":work.binding}),
        ))
    }
    pub fn start(
        &'static self,
        id: String,
        binding: Value,
        model: String,
        action: impl FnOnce(&mut dyn Inference) -> Result<Value, String> + Send + 'static,
    ) -> Result<(), String> {
        let lease = crate::doc_suggest::lease()?;
        let mut state = self.0.lock().map_err(|_| "Local advice unavailable")?;
        Self::expire(&mut state);
        if state.active.is_some() {
            return Err("Local advice is running or awaiting collection".into());
        }
        state.active = Some(Work {
            id: id.clone(),
            binding,
            result: None,
            completed: None,
        });
        drop(state);
        let failed_id = id.clone();
        if std::thread::Builder::new()
            .name("waypoint-history-advice".into())
            .spawn(move || {
                let _lease = lease;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let llm = OllamaInference::new("http://127.0.0.1:11434", model.clone())
                        .map_err(|error| error.to_string())?
                        .with_resident_context();
                    let mut llm = crate::resident_inference::ResidentInference::new(
                        llm,
                        model,
                        crate::resident_model_identity,
                    );
                    action(&mut llm)
                }))
                .unwrap_or_else(|_| Err("Local advice stopped unexpectedly".into()));
                if let Ok(mut state) = self.0.lock() {
                    if let Some(work) = state.active.as_mut().filter(|work| work.id == id) {
                        work.result = Some(result);
                        work.completed = Some(Instant::now());
                    }
                }
            })
            .is_err()
        {
            if let Ok(mut state) = self.0.lock() {
                if state
                    .active
                    .as_ref()
                    .is_some_and(|work| work.id == failed_id)
                {
                    state.active = None;
                }
            }
            return Err("Could not start local advice".into());
        }
        Ok(())
    }
    pub fn poll(&self, id: &str) -> Result<Value, String> {
        let mut state = self.0.lock().map_err(|_| "Local advice unavailable")?;
        Self::expire(&mut state);
        if let Some(work) = state.active.as_ref().filter(|work| work.id == id) {
            return Ok(match &work.result {
                None => json!({"status":"running","binding":work.binding}),
                Some(Ok(value)) => {
                    json!({"status":"complete","binding":work.binding,"advice":value})
                }
                Some(Err(error)) => json!({"status":"failed","binding":work.binding,"error":error}),
            });
        }
        state
            .cached
            .as_ref()
            .filter(|(key, _, _)| key == id)
            .map(|(_, value, _)| value.clone())
            .ok_or("Local advice not found".into())
    }
    pub fn complete(&self, id: &str, value: Value) -> Result<Value, String> {
        let mut state = self.0.lock().map_err(|_| "Local advice unavailable")?;
        Self::expire(&mut state);
        if state
            .active
            .as_ref()
            .is_some_and(|work| work.id == id && work.result.is_some())
        {
            state.active = None;
            state.cached = Some((id.into(), value.clone(), Instant::now()));
        } else if let Some((_, cached, _)) = state.cached.as_mut().filter(|(key, _, _)| key == id) {
            *cached = value.clone();
        } else {
            return Err("Local advice is not ready for collection".into());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_errors_and_stale_tasks_cannot_resurrect_private_results() {
        let slot = Slot::new();
        slot.0.lock().unwrap().active = Some(Work {
            id: "old".into(),
            binding: json!({"session":"original"}),
            result: Some(Err("fixture failure".into())),
            completed: Some(Instant::now()),
        });
        assert_eq!(slot.poll("old").unwrap()["status"], "failed");
        slot.complete("old", json!({"status":"failed","error":"stale context"}))
            .unwrap();
        assert_eq!(slot.poll("old").unwrap()["error"], "stale context");
        assert_eq!(slot.pending().unwrap()["active"], false);
        assert!(slot
            .complete("unknown", json!({"private":"must not persist"}))
            .is_err());
        slot.0.lock().unwrap().cached.as_mut().unwrap().2 =
            Instant::now() - Duration::from_secs(601);
        assert!(slot.poll("old").is_err());
    }
    #[test]
    fn running_task_cannot_be_collected_as_complete() {
        let slot = Slot::new();
        slot.0.lock().unwrap().active = Some(Work {
            id: "current".into(),
            binding: json!({}),
            result: None,
            completed: None,
        });
        assert_eq!(slot.poll("current").unwrap()["status"], "running");
        assert!(slot
            .complete("current", json!({"status":"complete"}))
            .is_err());
        assert_eq!(slot.pending().unwrap()["active"], true);
    }
}
