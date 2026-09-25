//! Local inference boundary (T007 + T009, gate G1).
//!
//! The domain sees a trait-like interface only: capability probe, tokenize,
//! budget estimate, structured generate, cancel, usage reporting. llama.cpp /
//! managed-server specifics live behind adapters, never in prompts.

use serde::{Deserialize, Serialize};

/// What a backend reports about itself after a successful startup probe.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityReport {
    pub backend: String,
    pub model_name: String,
    pub quantization: String,
    pub context_window_tokens: u32,
    pub supports_reasoning: bool,
    pub supports_structured_output: bool,
    /// Zero means fully GPU-resident per backend report; any CPU offload > 0.
    pub cpu_offload_bytes: u64,
    /// Reserved GPU (dedicated) memory in bytes at probe time, if known.
    pub gpu_reserved_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageReport {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub wall_time_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceError {
    /// Reasoning control could not be enabled reliably; the model is
    /// unqualified for core AI tasks (MASTER_PLAN §7).
    ReasoningUnavailable,
    StructuredOutputFailed(String),
    BudgetExceeded,
    Cancelled,
    /// Hard refusal to ever fall back to a paid cloud model silently.
    CloudFallbackForbidden,
}

impl std::fmt::Display for InferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferenceError::ReasoningUnavailable => {
                write!(f, "model runtime cannot enable reasoning controls reliably")
            }
            InferenceError::StructuredOutputFailed(m) => write!(f, "structured output: {m}"),
            InferenceError::BudgetExceeded => write!(f, "token/VRAM budget exceeded"),
            InferenceError::Cancelled => write!(f, "generation cancelled"),
            InferenceError::CloudFallbackForbidden => {
                write!(f, "silent paid-cloud fallback is forbidden")
            }
        }
    }
}

impl std::error::Error for InferenceError {}

/// Trait-like boundary used by the domain. Implementations: managed llama.cpp
/// server over private pipe / authenticated loopback (T007 runtime), and a
/// deterministic test stub used in fixtures.
pub trait Inference {
    /// Startup probe: structured output + bounded reasoning request must both
    /// succeed for the model to be marked qualified.
    fn probe(&mut self) -> Result<CapabilityReport, InferenceError>;

    /// Generate JSON conforming to `schema_name` within `max_output_tokens`.
    fn generate_structured(
        &mut self,
        schema_name: &str,
        prompt: &str,
        max_output_tokens: u32,
    ) -> Result<(serde_json::Value, UsageReport), InferenceError>;

    fn cancel(&mut self) -> Result<(), InferenceError> {
        Err(InferenceError::Cancelled)
    }
}

/* --------------------------- resource scheduling ---------------------------- */

/// T009: bounded default concurrency per MASTER_PLAN §13.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudgets {
    /// Max public fetch tasks globally.
    pub global_fetch: u8,
    /// Max fetches per origin (source policy may be stricter).
    pub per_origin_fetch: u8,
    /// Max simultaneous browser application sessions.
    pub browser_sessions: u8,
    /// Max simultaneous local reasoning generations.
    pub reasoning_slots: u8,
    /// Aggregate dedicated-GPU ceiling in MiB on the 16 GiB reference card.
    pub gpu_budget_mib: u32,
}

impl Default for ResourceBudgets {
    fn default() -> Self {
        Self {
            global_fetch: 4,
            per_origin_fetch: 2,
            browser_sessions: 1,
            reasoning_slots: 1,
            gpu_budget_mib: 14 * 1024,
        }
    }
}

/// Simple counted scheduler: acquire returns false instead of exceeding the
/// bound so callers queue rather than oversubscribe.
#[derive(Debug, Default)]
pub struct ResourceScheduler {
    budgets: ResourceBudgets,
    active_fetch: u8,
    active_browser: u8,
    active_reasoning: u8,
}

impl ResourceScheduler {
    pub fn new(budgets: ResourceBudgets) -> Self {
        Self {
            budgets,
            ..Default::default()
        }
    }

    pub fn acquire_fetch(&mut self) -> bool {
        if self.active_fetch < self.budgets.global_fetch {
            self.active_fetch += 1;
            true
        } else {
            false
        }
    }

    pub fn release_fetch(&mut self) {
        self.active_fetch = self.active_fetch.saturating_sub(1);
    }

    pub fn acquire_browser(&mut self) -> bool {
        if self.active_browser < self.budgets.browser_sessions {
            self.active_browser += 1;
            true
        } else {
            false
        }
    }

    pub fn release_browser(&mut self) {
        self.active_browser = self.active_browser.saturating_sub(1);
    }

    pub fn acquire_reasoning(&mut self) -> bool {
        if self.active_reasoning < self.budgets.reasoning_slots {
            self.active_reasoning += 1;
            true
        } else {
            false
        }
    }

    pub fn release_reasoning(&mut self) {
        self.active_reasoning = self.active_reasoning.saturating_sub(1);
    }
}

/// Deterministic stub used for qualification tests without a live model.
pub struct StubInference {
    pub reasoning_ok: bool,
}

impl StubInference {
    pub fn qualified() -> Self {
        Self { reasoning_ok: true }
    }
}

impl Inference for StubInference {
    fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
        if !self.reasoning_ok {
            return Err(InferenceError::ReasoningUnavailable);
        }
        Ok(CapabilityReport {
            backend: "stub".into(),
            model_name: "qwen3.5-9b (stubbed)".into(),
            quantization: "Q4_K_M".into(),
            context_window_tokens: 16_384,
            supports_reasoning: true,
            supports_structured_output: true,
            cpu_offload_bytes: 0,
            gpu_reserved_bytes: None,
        })
    }

    fn generate_structured(
        &mut self,
        schema_name: &str,
        _prompt: &str,
        max_output_tokens: u32,
    ) -> Result<(serde_json::Value, UsageReport), InferenceError> {
        if max_output_tokens == 0 {
            return Err(InferenceError::BudgetExceeded);
        }
        Ok((
            serde_json::json!({"schema": schema_name, "ok": true}),
            UsageReport {
                prompt_tokens: 0,
                completion_tokens: 4,
                wall_time_ms: 1,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unqualified_model_fails_probe_closed() {
        let mut bad = StubInference {
            reasoning_ok: false,
        };
        assert_eq!(bad.probe(), Err(InferenceError::ReasoningUnavailable));
    }

    #[test]
    fn qualified_stub_reports_capability() {
        let mut s = StubInference::qualified();
        let r = s.probe().unwrap();
        assert!(r.supports_reasoning);
        assert_eq!(r.cpu_offload_bytes, 0);
    }

    #[test]
    fn scheduler_never_exceeds_budgets() {
        let b = ResourceBudgets::default();
        assert_eq!(b.global_fetch, 4);
        assert_eq!(b.per_origin_fetch, 2);
        assert_eq!(b.browser_sessions, 1);
        assert_eq!(b.reasoning_slots, 1);
        assert_eq!(b.gpu_budget_mib, 14 * 1024);

        let mut s = ResourceScheduler::new(b);
        assert!(s.acquire_fetch());
        assert!(s.acquire_fetch());
        assert!(s.acquire_fetch());
        assert!(s.acquire_fetch());
        assert!(!s.acquire_fetch()); // 5th refused
        s.release_fetch();
        assert!(s.acquire_fetch());

        assert!(s.acquire_reasoning());
        assert!(!s.acquire_reasoning()); // single reasoning slot

        assert!(s.acquire_browser());
        assert!(!s.acquire_browser());
    }

    #[test]
    fn zero_token_budget_refused() {
        let mut s = StubInference::qualified();
        assert!(matches!(
            s.generate_structured("x", "y", 0),
            Err(InferenceError::BudgetExceeded)
        ));
    }
}
