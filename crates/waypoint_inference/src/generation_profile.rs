//! Explicit local presets; unknown models retain the generic adapter contract.
use serde_json::{json, Value};

pub const DEFAULT_MODEL: &str = "smart-jobseeker/qwen3.8:27b-ud-q3_k_xl";
/// Adapter ceiling, not a claim that a model or host can serve this context.
/// Each caller must verify native capacity, actual allocation and host headroom.
pub const MAX_CONTEXT_TOKENS: u32 = 262_144;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationProfile {
    Generic,
    Qwen38,
}

/// Allocation options for a runner launched and guarded by the caller.
/// Generation must carry the same options as preload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct OwnedRunnerOptions {
    num_ctx: u32,
    num_batch: u32,
    num_thread: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_gpu: Option<u32>,
}

impl OwnedRunnerOptions {
    pub fn for_profile(profile: GenerationProfile) -> Self {
        Self {
            num_ctx: profile.context_tokens(),
            num_batch: 128,
            num_thread: 2,
            num_gpu: (profile == GenerationProfile::Generic).then_some(999),
        }
    }

    pub fn bind(self, options: &mut Value, resident_context: u32) -> Result<(), String> {
        if resident_context != self.num_ctx {
            return Err("The private model context differs from the admitted runner".into());
        }
        let options = options
            .as_object_mut()
            .ok_or("Invalid generation options")?;
        options.insert("num_ctx".into(), json!(self.num_ctx));
        options.insert("num_batch".into(), json!(self.num_batch));
        options.insert("num_thread".into(), json!(self.num_thread));
        if let Some(layers) = self.num_gpu {
            options.insert("num_gpu".into(), json!(layers));
        } else {
            options.remove("num_gpu");
        }
        Ok(())
    }
}

impl GenerationProfile {
    pub fn for_model(name: &str) -> Self {
        if name == DEFAULT_MODEL {
            Self::Qwen38
        } else {
            Self::Generic
        }
    }

    pub fn context_tokens(self) -> u32 {
        match self {
            Self::Generic => 4096,
            Self::Qwen38 => 16384,
        }
    }

    pub fn max_cpu_offload_bytes(self) -> u64 {
        (match self {
            Self::Generic => 2,
            // Allow automatic placement to accommodate a busy 16 GiB GPU.
            // Host admission, aggregate process limits and live RAM checks
            // remain independent; this ceiling is not a RAM reservation.
            Self::Qwen38 => 6,
        }) * 1024u64.pow(3)
    }

    pub fn startup_overhead_percent(self) -> u64 {
        match self {
            Self::Generic => 20,
            Self::Qwen38 => 10,
        }
    }

    pub fn thinking(self) -> Value {
        match self {
            Self::Generic => json!(true),
            Self::Qwen38 => json!("xhigh"),
        }
    }

    /// `budget` remains the complete native generation limit, never an
    /// unreported extra reasoning allowance on top of a caller's token budget.
    pub fn options(self, thinking: bool, budget: u32) -> Value {
        match self {
            Self::Generic => json!({"num_predict":budget,"temperature":0.0}),
            Self::Qwen38 => json!({"num_predict":budget,"temperature":if thinking {1.0} else {0.7},
                "top_p":if thinking {0.95} else {0.8},"top_k":20,"min_p":0.0,
                "presence_penalty":if thinking {0.0} else {1.5},"repeat_penalty":1.0}),
        }
    }

    pub fn validate_details(self, details: &Value) -> Result<(), String> {
        if self == Self::Generic {
            return Ok(());
        }
        let capabilities = details["capabilities"].as_array();
        if details["renderer"] != "qwen3.8"
            || details["parser"] != "qwen3.5"
            || !capabilities.is_some_and(|items| {
                ["vision", "thinking", "tools"]
                    .iter()
                    .all(|required| items.iter().any(|item| item.as_str() == Some(*required)))
            })
            || !details["thinking"]["values"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item == "xhigh"))
        {
            return Err("The default Qwen model needs its native vision, tool and xhigh reasoning configuration; install or repair it in Settings".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_profile_keeps_other_models_generic_and_bounds_generation() {
        assert_eq!(
            GenerationProfile::for_model(DEFAULT_MODEL),
            GenerationProfile::Qwen38
        );
        assert_eq!(
            GenerationProfile::for_model("future-model:vision"),
            GenerationProfile::Generic
        );
        let qwen = GenerationProfile::Qwen38;
        assert_eq!(qwen.thinking(), "xhigh");
        assert_eq!(qwen.context_tokens(), 16384);
        assert_eq!(qwen.max_cpu_offload_bytes(), 6 * 1024u64.pow(3));
        assert_eq!(
            GenerationProfile::Generic.max_cpu_offload_bytes(),
            2 * 1024u64.pow(3)
        );
        assert_eq!(qwen.options(true, 4096)["num_predict"], 4096);
        assert_eq!(qwen.options(true, 4096)["temperature"], 1.0);
        assert_eq!(qwen.options(false, 256)["num_predict"], 256);
        assert_eq!(qwen.options(false, 256)["presence_penalty"], 1.5);
    }
    #[test]
    fn advertised_capabilities_without_native_controls_do_not_qualify_default() {
        let valid = json!({"renderer":"qwen3.8","parser":"qwen3.5","capabilities":["vision","thinking","tools"],"thinking":{"values":[false,"low","medium","xhigh"]}});
        assert!(GenerationProfile::Qwen38.validate_details(&valid).is_ok());
        for key in ["renderer", "parser", "capabilities", "thinking"] {
            let mut wrong = valid.clone();
            wrong.as_object_mut().unwrap().remove(key);
            assert!(GenerationProfile::Qwen38.validate_details(&wrong).is_err());
        }
        assert!(GenerationProfile::Generic
            .validate_details(&json!({}))
            .is_ok());
    }
}
