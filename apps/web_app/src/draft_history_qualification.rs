//! Explicit local-model qualification. Fictional histories and jobs only.
use super::*;

#[test]
#[ignore = "loads the explicitly selected local model through resource admission; fictional drafts only"]
fn resource_gated_history_backed_document_drafts() {
    assert_eq!(
        std::env::var("WAYPOINT_HISTORY_DRAFT_CONTAINED").as_deref(),
        Ok("1"),
        "Use tools/qualify_history_drafts.py"
    );
    let name = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL").expect("explicit model name");
    let output = tempfile::Builder::new()
        .prefix("history-document-native-")
        .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .unwrap()
        .keep();
    println!("History document evidence: {}", output.display());
    crate::experience_inference::tests::with_resource_gated_named_model(&name, || {
        struct Recording<I> {
            inner: I,
            calls: Vec<Value>,
        }
        impl<I: Inference> Inference for Recording<I> {
            fn probe(
                &mut self,
            ) -> Result<waypoint_inference::CapabilityReport, waypoint_inference::InferenceError>
            {
                self.inner.probe()
            }
            fn generate_structured(
                &mut self,
                task: &str,
                prompt: &str,
                max: u32,
            ) -> Result<(Value, waypoint_inference::UsageReport), waypoint_inference::InferenceError>
            {
                let result = self.inner.generate_structured(task, prompt, max);
                self.calls.push(json!({"task":task,"prompt":prompt,"output":result.as_ref().ok().map(|v|&v.0),"error":result.as_ref().err().map(ToString::to_string)}));
                result
            }
        }
        let identity = crate::resident_model_identity()
            .unwrap()
            .expect("resident identity");
        let identity = json!({"name":identity.name,"digest":identity.digest});
        let inference = OllamaInference::new("http://127.0.0.1:11434", &name)
            .unwrap()
            .with_resident_context();
        let guarded = crate::resident_inference::ResidentInference::new(
            inference,
            name.clone(),
            crate::resident_model_identity,
        );
        let mut model = Recording {
            inner: guarded,
            calls: Vec::new(),
        };
        let profile = json!({"career_history":json!({"version":1,"employment":[
            {"id":"a".repeat(32),"employer":"Fictional Signal Lab","title":"Systems engineer","start_date":"2019","end_date":"2023","description":"Built measurement tools in Rust and wrote operating guides.","reviewed":true},
            {"id":"b".repeat(32),"employer":"Unreviewed Organization","title":"Director","description":"Supervised 200 people.","reviewed":false}],
            "education":[{"id":"c".repeat(32),"institution":"Fictional Learning Institute","qualification":"Master of Data Science","start_date":"2025-09","current":true,"description":"Studies in progress. No degree has been awarded.","reviewed":true}]}).to_string()});
        let mut results = Vec::new();
        for kind in ["cv", "cover_letter"] {
            let mut facts = Vec::new();
            let history_sources = crate::draft_history::augment(&mut facts, &profile).unwrap();
            let input = Inputs {
                job_id:"fictional-history-document".into(), digest:"fictional-context".into(),
                job:json!({"title":"Research software engineer","employer":"Fictional Research Centre","description":"Build reliable measurement tools, maintain software and write clear guides."}),
                kind:kind.into(), available_facts:facts.len(), facts, history_sources,
                signature: Default::default(),
                contact_facts:Vec::new(), priority_facts:Vec::new(), selection_listing:"Measurement software and operating guides".into(),
                instructions:"Use concise, concrete language. Preserve unknowns and the exact precision of dates.".into(),
                template:None, reviewed_template:None, template_style_focus:None, template_scan_page:None,
            };
            let generated = generate_with_selection(&mut model, &input, "", None);
            let row = json!({"kind":kind,"draft":generated.as_ref().ok(),"error":generated.as_ref().err()});
            results.push(row);
            std::fs::write(
                output.join("results.private.json"),
                serde_json::to_vec_pretty(
                    &json!({"model":identity,"results":results,"calls":model.calls}),
                )
                .unwrap(),
            )
            .unwrap();
            let draft = generated.expect("history-backed draft generation");
            assert_eq!(draft["selection"]["history_sources_available"], 2);
            assert_eq!(
                draft["selection"]["history_sources_used"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(draft["selection"]["draft_fact_budget_omitted"], 0);
            assert_eq!(draft["evidence_check"]["status"], "completed");
            assert!(
                draft["evidence_check"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|v| v["status"] != "unsupported"),
                "Local evidence checker found unsupported statements"
            );
            assert!(model.calls.iter().all(|row| !row["prompt"]
                .as_str()
                .unwrap()
                .contains("Supervised 200 people")));
        }
    });
}
