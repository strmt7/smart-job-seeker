//! Recheck the shared local-model budget before each guarded inference request.
use waypoint_inference::{CapabilityReport, Inference, InferenceError, UsageReport};

pub struct ResidentInference<I, F> {
    inner: I,
    model: String,
    resident: F,
}
impl<I, F> ResidentInference<I, F>
where
    F: FnMut() -> Result<Option<String>, String>,
{
    pub fn new(inner: I, model: String, resident: F) -> Self {
        Self {
            inner,
            model,
            resident,
        }
    }
    fn check(&mut self) -> Result<(), InferenceError> {
        match (self.resident)() {
            Ok(Some(model)) if model == self.model => Ok(()),
            Ok(_) => Err(InferenceError::RuntimeUnreachable(
                "The eligible resident model changed; form interpretation stopped".into(),
            )),
            Err(error) => Err(InferenceError::RuntimeUnreachable(format!(
                "Could not verify resident model: {error}"
            ))),
        }
    }
}
impl<I, F> Inference for ResidentInference<I, F>
where
    I: Inference,
    F: FnMut() -> Result<Option<String>, String>,
{
    fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
        self.check()?;
        self.inner.probe()
    }
    fn generate_structured(
        &mut self,
        schema: &str,
        prompt: &str,
        max: u32,
    ) -> Result<(serde_json::Value, UsageReport), InferenceError> {
        self.check()?;
        self.inner.generate_structured(schema, prompt, max)
    }
    fn cancel(&mut self) -> Result<(), InferenceError> {
        self.inner.cancel()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Fake<'a>(&'a Cell<usize>);
    impl Inference for Fake<'_> {
        fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
            unreachable!()
        }
        fn generate_structured(
            &mut self,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<(serde_json::Value, UsageReport), InferenceError> {
            self.0.set(self.0.get() + 1);
            Ok((
                serde_json::json!({}),
                UsageReport {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    wall_time_ms: 0,
                },
            ))
        }
    }
    #[test]
    fn changed_residency_stops_the_next_model_request() {
        let calls = Cell::new(0);
        let checks = Cell::new(0);
        let mut guarded = ResidentInference::new(Fake(&calls), "fictional".into(), || {
            let n = checks.get();
            checks.set(n + 1);
            Ok((n == 0).then(|| "fictional".into()))
        });
        assert!(guarded.generate_structured("test", "fictional", 1).is_ok());
        assert!(guarded.generate_structured("test", "fictional", 1).is_err());
        assert_eq!(calls.get(), 1);
    }
}
