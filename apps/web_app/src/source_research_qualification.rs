//! Explicit owned-model diagnostics; no browser or candidate material.
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use waypoint_inference::{Inference, OllamaInference};
use waypoint_research::{
    tools::{PublicEvidence, PublicLink, PublicTools},
    ResearchBudget, ResearchPlan, Researcher,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default = "default_research")]
    run_research: bool,
    #[serde(default = "default_research")]
    run_schema_probes: bool,
    #[serde(default)]
    issue: Option<String>,
    #[serde(default)]
    live_public_research: bool,
    python: PathBuf,
    selection: crate::private_inference::Selection,
    context: String,
    searches: Vec<Search>,
    reads: Vec<Read>,
}
fn default_research() -> bool {
    true
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    links: Vec<PublicLink>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    requested: String,
    evidence: PublicEvidence,
}
struct Replay<'a> {
    config: &'a Config,
    calls: Vec<Value>,
}
impl PublicTools for Replay<'_> {
    fn search(&mut self, query: &str) -> Result<Vec<PublicLink>, String> {
        self.calls.push(json!({"action":"search","query":query}));
        self.config
            .searches
            .iter()
            .find(|s| s.query == query)
            .map(|s| s.links.clone())
            .ok_or("No recorded response for this query; replay did not access the internet".into())
    }
    fn read(&mut self, url: &str) -> Result<PublicEvidence, String> {
        self.calls.push(json!({"action":"read","url":url}));
        self.config
            .reads
            .iter()
            .find(|s| s.requested == url || s.evidence.url == url)
            .map(|s| s.evidence.clone())
            .ok_or("No recorded response for this URL; replay did not access the internet".into())
    }
    fn check(&self) -> Result<(), String> {
        crate::check_inference_memory()
    }
}
struct Trace<'a> {
    inner: &'a mut OllamaInference,
    calls: &'a mut Vec<Value>,
}
impl Inference for Trace<'_> {
    fn probe(
        &mut self,
    ) -> Result<waypoint_inference::CapabilityReport, waypoint_inference::InferenceError> {
        self.inner.probe()
    }
    fn generate_structured(
        &mut self,
        n: &str,
        p: &str,
        m: u32,
    ) -> Result<(Value, waypoint_inference::UsageReport), waypoint_inference::InferenceError> {
        self.calls
            .push(json!({"schema":n,"prompt":p,"output_limit":m}));
        self.inner.generate_structured(n, p, m)
    }
    fn generate_structured_with_schema(
        &mut self,
        n: &str,
        p: &str,
        m: u32,
        s: &Value,
    ) -> Result<(Value, waypoint_inference::UsageReport), waypoint_inference::InferenceError> {
        self.calls
            .push(json!({"schema":n,"prompt":p,"output_limit":m,"format":s}));
        self.inner.generate_structured_with_schema(n, p, m, s)
    }
    fn generate_advice(
        &mut self,
        n: &str,
        p: &str,
        m: u32,
    ) -> Result<Option<(String, waypoint_inference::UsageReport)>, waypoint_inference::InferenceError>
    {
        self.calls
            .push(json!({"schema":n,"prompt":p,"output_limit":m}));
        self.inner.generate_advice(n, p, m)
    }
}
#[test]
#[ignore = "explicit private model/cache and hash-bound replay inputs; no browser or internet"]
fn owned_runtime_evaluates_research_decisions_without_browser_work() {
    let config_path = PathBuf::from(
        std::env::var_os("WAYPOINT_RESEARCH_QUALIFICATION_CONFIG")
            .expect("Explicit qualification configuration"),
    );
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap();
    let config_path = config_path.canonicalize().unwrap();
    assert!(config_path.starts_with(&target));
    let bytes = std::fs::read(&config_path).unwrap();
    assert!(bytes.len() <= 64 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_RESEARCH_QUALIFICATION_SHA256").unwrap()
    );
    let config: Config = serde_json::from_slice(&bytes).unwrap();
    assert!(config.run_research || config.run_schema_probes);
    let context = match config.issue.as_deref() {
        None => config.context.clone(),
        Some("native_text") => {
            crate::form_research::technical_question("text input", "unidentified web form", false)
        }
        Some("native_attachment") => crate::form_research::technical_question(
            "file attachment",
            "unidentified web form",
            false,
        ),
        Some("visual_control") => {
            crate::form_research::technical_question("text input", "unidentified web form", true)
        }
        _ => panic!("Unknown qualification issue"),
    };
    assert!(context.len() <= 1200);
    assert!(config.searches.len() <= 4 && config.reads.len() <= 8);
    assert!(config
        .selection
        .cache_root
        .canonicalize()
        .unwrap()
        .starts_with(&target));
    let attempt = tempfile::Builder::new()
        .prefix("model-only-")
        .tempdir_in(config_path.parent().unwrap())
        .unwrap()
        .keep();
    let task = attempt.file_name().unwrap().to_str().unwrap();
    crate::private_inference::create_namespace(&attempt, task).unwrap();
    let screenshots = attempt.join("screenshots");
    std::fs::create_dir(&screenshots).unwrap();
    let started = Instant::now();
    assert!(config.python.is_absolute() && config.python.is_file());
    let python = config.python.canonicalize().unwrap();
    let mut session =
        crate::private_inference::Session::admit(&config.selection, &python, &screenshots).unwrap();
    let native = Arc::new(Mutex::new(Vec::<Value>::new()));
    let mut probes = Vec::new();
    let mut tool_calls = Vec::new();
    let mut model_inputs = Vec::new();
    let outcome = (|| -> Result<Value, String> {
        if session.metadata["mode"] != "private" {
            return Err(
                "Qualification requires its own runtime, never a borrowed shared model".into(),
            );
        }
        let mut inference = OllamaInference::new(
            &format!("http://{}", session.addr),
            &config.selection.choice.name,
        )
        .map_err(|e| e.to_string())?
        .with_resident_context();
        if let Some(guard) = &session.guard {
            inference.set_request_guard(guard.clone());
        }
        let observed = native.clone();
        inference.set_response_observer(move |schema, response| {
            let mut rows = observed.lock().unwrap();
            if rows.len() < 32 {
                rows.push(json!({"schema":schema,"native":response}));
            }
        });
        let prompt="Anonymous question: Which method returns the validation state? Choose the next tool action for this fixed diagnostic state. The documentation has already been read: 'Validation state is returned by checkValidity().' The source identifier is1. No other source is needed. Do not read that document again. Finish with a short supported conclusion and one exact quotation. You may stop if the evidence is insufficient. Return only the fields required for the chosen action, not empty fields for other actions.";
        let read = json!({"type":"object","properties":{"action":{"const":"read"},"url":{"type":"string"}},"required":["action","url"],"additionalProperties":false});
        let finish = json!({"type":"object","properties":{"action":{"const":"finish"},"advice":{"type":"string"},"citations":{"type":"array","items":{"type":"object","properties":{"source":{"type":"integer"},"quote":{"type":"string"}},"required":["source","quote"],"additionalProperties":false}}},"required":["action","advice","citations"],"additionalProperties":false});
        let stop = json!({"type":"object","properties":{"action":{"const":"stop"},"reason":{"type":"string"}},"required":["action","reason"],"additionalProperties":false});
        let flat = json!({"type":"object","properties":{"action":{"type":"string","enum":["read","finish","stop"]},"url":{"type":"string"},"advice":{"type":"string"},"citations":finish["properties"]["citations"],"reason":{"type":"string"}},"required":["action"],"additionalProperties":false});
        let mut grounded_passed = true;
        if config.run_schema_probes {
            for (name, schema) in [
                (
                    "read_first",
                    json!({"type":"object","oneOf":[read.clone(),finish.clone(),stop.clone()]}),
                ),
                (
                    "finish_first",
                    json!({"type":"object","oneOf":[finish,read,stop]}),
                ),
                ("flat_action", flat),
            ] {
                for grounded in [false, true] {
                    let id = format!(
                        "{name}_{}",
                        if grounded {
                            "grounded"
                        } else {
                            "constraint_only"
                        }
                    );
                    let actual_prompt = if grounded {
                        format!("{prompt}\nAllowed response JSON schema: {schema}")
                    } else {
                        prompt.to_owned()
                    };
                    let response = inference
                        .generate_structured_with_schema(&id, &actual_prompt, 768, &schema)
                        .map_err(|e| e.to_string());
                    let supported_finish = response.as_ref().is_ok_and(|(v, _)| {
                        v.as_object().is_some_and(|o| {
                            o.keys()
                                .all(|k| matches!(k.as_str(), "action" | "advice" | "citations"))
                        }) && v["action"] == "finish"
                            && v["citations"].as_array().is_some_and(|items| {
                                items.iter().any(|c| {
                                    c["source"] == 1
                                        && c["quote"].as_str().is_some_and(|q| {
                                            !q.is_empty()
                                            && "Validation state is returned by checkValidity()."
                                                .contains(q)
                                        })
                                })
                            })
                    });
                    if grounded {
                        grounded_passed &= supported_finish;
                    }
                    probes.push(json!({"id":id,"schema":schema,"prompt":actual_prompt,"response":response,"expected_action":"finish","supported_finish":supported_finish,"scope":"fictional diagnostic only, not research accuracy"}));
                }
            }
        }
        if !config.run_research {
            return Ok(
                json!({"scope":"schema_grounding_diagnostic_only","grounded_probes_passed":grounded_passed,"candidate_action_authority":false}),
            );
        }
        let mut tracing = Trace {
            inner: &mut inference,
            calls: &mut model_inputs,
        };
        if config.live_public_research {
            return crate::form_research::run(
                &mut tracing,
                &screenshots,
                &json!({}),
                &context,
                session.guard.clone(),
            )
            .map(|value| value["result"].clone());
        }
        let mut researcher = Researcher::new(&mut tracing);
        researcher.begin(ResearchPlan {
            employer: String::new(),
            unresolved_questions: vec![context.clone()],
            source_hierarchy: vec!["official_technical_documentation".into()],
            budget: ResearchBudget {
                max_queries: 2,
                max_fetches: 3,
                max_verification_passes: 2,
            },
        });
        let mut replay = Replay {
            config: &config,
            calls: vec![],
        };
        let report = researcher.investigate(&context, &mut replay);
        tool_calls = replay.calls;
        serde_json::to_value(report).map_err(|e| e.to_string())
    })();
    let stopped = session.stop();
    let cleanup = crate::private_inference::cleanup_namespace(&attempt, task);
    let passed = outcome.as_ref().is_ok_and(|report| {
        (if config.run_research {
            report["status"] == "advice_available"
        } else {
            report["grounded_probes_passed"] == true
        }) && report["candidate_action_authority"] == false
    }) && stopped.is_ok()
        && cleanup.is_ok();
    let report = json!({"criteria_passed":passed,"scope":if config.live_public_research{"Owned local model with anonymous public internet research; no browser, candidate material or application actions"}else{"Owned local model with fixed public-source replay; no browser, live discovery, candidate information or application actions"},"effective_context":context, "input_sha256":waypoint_domain::sha256_hex(&bytes),"model":session.metadata,"probes":probes,"research":outcome,"replay_calls":tool_calls,"model_inputs":model_inputs,"native":*native.lock().unwrap(),"stop":stopped,"namespace_cleanup":cleanup,"seconds":started.elapsed().as_secs_f64()});
    std::fs::write(
        config_path
            .parent()
            .unwrap()
            .join("research-decisions.private.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(
        passed,
        "Research decision criteria failed; inspect the native report"
    );
}
