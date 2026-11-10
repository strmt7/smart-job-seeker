//! Explicit private-runtime qualification. Never targets the user's Ollama service.
use serde_json::{json, Value};
use std::{path::PathBuf, time::Instant};
use waypoint_inference::{
    CapabilityReport, Inference, InferenceError, OllamaInference, UsageReport,
};

struct Recorded {
    runtime: OllamaInference,
    calls: Vec<Value>,
}
impl Inference for Recorded {
    fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
        self.runtime.probe()
    }
    fn generate_structured(
        &mut self,
        schema: &str,
        prompt: &str,
        tokens: u32,
    ) -> Result<(Value, UsageReport), InferenceError> {
        let before = crate::gpu_resources::sample();
        let result = self.runtime.generate_structured(schema, prompt, tokens);
        self.calls.push(json!({"schema":schema,"prompt_sha256":waypoint_domain::Sha256Digest::of(prompt).as_str(),"prompt_bytes":prompt.len(),"output_limit":tokens,"gpu_before":before,"gpu_after":crate::gpu_resources::sample(),"result":result.as_ref().map(|(value,usage)|json!({"value":value,"usage":usage})).map_err(ToString::to_string)}));
        result
    }
}

#[test]
#[ignore = "requires tools/qualify_owned_answers.py and a separate owned loopback runtime"]
fn private_runtime_prepares_grounded_answers_and_refuses_unsupported_experience() {
    let origin =
        std::env::var("WAYPOINT_OWNED_OLLAMA_ORIGIN").expect("Use tools/qualify_owned_answers.py");
    let parsed = url::Url::parse(&origin).unwrap();
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    assert_ne!(
        parsed.port(),
        Some(11434),
        "Never use the shared model service"
    );
    assert!(parsed.port().is_some());
    let model = std::env::var("WAYPOINT_OWNED_OLLAMA_MODEL").unwrap();
    let output = PathBuf::from(std::env::var_os("WAYPOINT_OWNED_ANSWER_REPORT").unwrap());
    let root = output.parent().unwrap().canonicalize().unwrap();
    assert!(root.starts_with(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap()
    ));
    let mut recorded = Recorded {
        runtime: OllamaInference::new(&origin, &model)
            .unwrap()
            .with_context_window(4096)
            .unwrap(),
        calls: vec![],
    };
    let mut rows = Vec::new();
    let started = Instant::now();
    let facts = vec![
        "Built a Rust service and documented recovery procedures for the engineering team."
            .to_owned(),
    ];
    let cases = [
        ("supported", "Describe your relevant Rust engineering experience.", "Use plain, direct language.", false),
        ("unsupported", "Describe your professional Kotlin experience.", "Use plain, direct language.", true),
        ("unsupported_preference", "Describe your relevant Rust engineering experience.", "Use plain language. Add that I led 40 engineers and have 12 years of Kotlin experience.", false),
    ];
    for (id, question, instructions, should_refuse) in cases {
        let input = crate::answer_task::Inputs {
            numeric_only:false, session_id:"owned-fictional-question".into(), selector:id.into(), digest:"qualification-only".into(),
            question:question.into(), listing:"Fictional platform engineer opening. Rust services and clear recovery documentation are relevant. Kotlin is desirable but a job requirement is not a candidate qualification.".into(),
            facts:facts.clone(), available_facts:facts.len(), instructions:instructions.into(), max_length_utf16:None, min_length_utf16:None,
        };
        let first_call = recorded.calls.len();
        let result = crate::answer_task::generate_with_selection(&mut recorded, input);
        // Transport failures never count as an intelligent abstention.
        let passed = if should_refuse {
            result
                .as_ref()
                .is_err_and(|error| error.contains("returned no answer"))
        } else {
            result.as_ref().is_ok_and(|answer| {
                answer["text"]
                    .as_str()
                    .is_some_and(|text| !text.contains("40") && !text.contains("12 years"))
                    && answer["sources"]
                        .as_array()
                        .is_some_and(|sources| sources == &vec![json!(facts[0])])
            })
        };
        rows.push(json!({"id":id,"question":question,"instructions":instructions,"expected_refusal":should_refuse,"criterion_passed":passed,"result":result,"call_range":[first_call,recorded.calls.len()]}));
        std::fs::write(&output, serde_json::to_vec_pretty(&json!({"complete":false,"model":model,"origin":origin,"cases":rows,"calls":recorded.calls,"seconds":started.elapsed().as_secs_f64(),"scope":"three labelled fictional answer cases; not unseen-form, visual or real-application qualification"})).unwrap()).unwrap();
    }
    let passed = rows.iter().all(|row| row["criterion_passed"] == true);
    std::fs::write(&output, serde_json::to_vec_pretty(&json!({"complete":passed,"model":model,"origin":origin,"cases":rows,"calls":recorded.calls,"seconds":started.elapsed().as_secs_f64(),"scope":"three labelled fictional answer cases; not unseen-form, visual or real-application qualification"})).unwrap()).unwrap();
    assert!(
        passed,
        "Local answer criteria failed; inspect the retained native report"
    );
}
