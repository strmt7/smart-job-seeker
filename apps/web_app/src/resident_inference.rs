//! Recheck the shared local-model budget before each guarded inference request.
use waypoint_inference::{CapabilityReport, Inference, InferenceError, UsageReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentIdentity {
    pub name: String,
    pub digest: String,
}

pub struct ResidentInference<I, F> {
    inner: I,
    model: String,
    resident: F,
    bound_identity: Option<ResidentIdentity>,
}
impl<I, F> ResidentInference<I, F>
where
    I: Inference,
    F: FnMut() -> Result<Option<ResidentIdentity>, String>,
{
    pub fn new(mut inner: I, model: String, resident: F) -> Self {
        #[cfg(windows)]
        inner.set_request_guard(std::sync::Arc::new(crate::check_inference_memory));
        #[cfg(not(windows))]
        let _ = &mut inner;
        Self {
            inner,
            model,
            resident,
            bound_identity: None,
        }
    }
    pub fn with_identity(mut self, identity: ResidentIdentity) -> Result<Self, String> {
        if identity.name != self.model {
            return Err("The selected resident model changed".into());
        }
        self.bound_identity = Some(identity);
        Ok(self)
    }

    fn check(&mut self) -> Result<(), InferenceError> {
        match (self.resident)() {
            Ok(Some(identity)) if identity.name == self.model => match &self.bound_identity {
                Some(bound) if bound != &identity => Err(InferenceError::RuntimeUnreachable(
                    "The resident model digest changed; inference stopped".into(),
                )),
                Some(_) => Ok(()),
                None => {
                    self.bound_identity = Some(identity);
                    Ok(())
                }
            },
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
    F: FnMut() -> Result<Option<ResidentIdentity>, String>,
{
    fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
        self.check()?;
        let result = self.inner.probe()?;
        self.check()?;
        Ok(result)
    }
    fn generate_structured(
        &mut self,
        schema: &str,
        prompt: &str,
        max: u32,
    ) -> Result<(serde_json::Value, UsageReport), InferenceError> {
        self.check()?;
        let result = self.inner.generate_structured(schema, prompt, max)?;
        self.check()?;
        Ok(result)
    }
    fn cancel(&mut self) -> Result<(), InferenceError> {
        self.inner.cancel()
    }

    fn generate_advice(
        &mut self,
        purpose: &str,
        prompt: &str,
        max: u32,
    ) -> Result<Option<(String, UsageReport)>, InferenceError> {
        self.check()?;
        let result = self.inner.generate_advice(purpose, prompt, max)?;
        self.check()?;
        Ok(result)
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
        fn generate_advice(
            &mut self,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<Option<(String, UsageReport)>, InferenceError> {
            let (_, usage) = self.generate_structured("fixture", "fixture", 768)?;
            Ok(Some(("Fictional assessment".into(), usage)))
        }
    }
    #[test]
    fn advisory_generation_checks_identity_before_and_after_request() {
        for changed_before in [false, true] {
            let calls = Cell::new(0);
            let checks = Cell::new(0);
            let mut guarded = ResidentInference::new(Fake(&calls), "fictional".into(), || {
                let n = checks.get();
                checks.set(n + 1);
                Ok(Some(ResidentIdentity {
                    name: "fictional".into(),
                    digest: if changed_before || n > 0 { "b" } else { "a" }.repeat(64),
                }))
            })
            .with_identity(ResidentIdentity {
                name: "fictional".into(),
                digest: "a".repeat(64),
            })
            .unwrap();
            assert!(guarded
                .generate_advice("fixture", "Fictional", 1536)
                .is_err());
            assert_eq!(calls.get(), usize::from(!changed_before));
        }
    }

    #[test]
    fn prebound_identity_refuses_a_different_digest_before_inference() {
        let calls = Cell::new(0);
        let original = ResidentIdentity {
            name: "fictional".into(),
            digest: "a".repeat(64),
        };
        let mut guarded = ResidentInference::new(Fake(&calls), "fictional".into(), || {
            Ok(Some(ResidentIdentity {
                name: "fictional".into(),
                digest: "b".repeat(64),
            }))
        })
        .with_identity(original)
        .unwrap();
        assert!(guarded.generate_structured("test", "fictional", 1).is_err());
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn changed_digest_during_request_discards_the_response() {
        let calls = Cell::new(0);
        let checks = Cell::new(0);
        let mut guarded = ResidentInference::new(Fake(&calls), "fictional".into(), || {
            let n = checks.get();
            checks.set(n + 1);
            Ok(Some(ResidentIdentity {
                name: "fictional".into(),
                digest: if n == 0 {
                    "a".repeat(64)
                } else {
                    "b".repeat(64)
                },
            }))
        });
        assert!(guarded.generate_structured("test", "fictional", 1).is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn same_name_with_changed_digest_stops_the_next_request() {
        let calls = Cell::new(0);
        let checks = Cell::new(0);
        let mut guarded = ResidentInference::new(Fake(&calls), "fictional".into(), || {
            let n = checks.get();
            checks.set(n + 1);
            Ok(Some(ResidentIdentity {
                name: "fictional".into(),
                digest: if n < 2 {
                    "a".repeat(64)
                } else {
                    "b".repeat(64)
                },
            }))
        });
        assert!(guarded.generate_structured("test", "fictional", 1).is_ok());
        assert!(guarded.generate_structured("test", "fictional", 1).is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn changed_residency_stops_the_next_model_request() {
        let calls = Cell::new(0);
        let checks = Cell::new(0);
        let mut guarded = ResidentInference::new(Fake(&calls), "fictional".into(), || {
            let n = checks.get();
            checks.set(n + 1);
            Ok((n < 2).then(|| ResidentIdentity {
                name: "fictional".into(),
                digest: "a".repeat(64),
            }))
        });
        assert!(guarded.generate_structured("test", "fictional", 1).is_ok());
        assert!(guarded.generate_structured("test", "fictional", 1).is_err());
        assert_eq!(calls.get(), 1);
    }
}
