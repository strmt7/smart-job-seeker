//! Opt-in qualification of actual contact -> history -> reviewed-fill integration.
//! Every identity and field is fictional; no external form or submit control.
use super::*;
use std::{io::Write, path::Path};
use waypoint_applications::{driver::BrowserDriver, form_mapping::ProposedMapping};
use waypoint_inference::{CapabilityReport, Inference, InferenceError, UsageReport};

struct Trace<'a> {
    inner: &'a mut dyn Inference,
    file: std::fs::File,
    calls: usize,
}
impl Inference for Trace<'_> {
    fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
        self.inner.probe()
    }
    fn set_context_tags(&mut self, tags: &[&str]) {
        self.inner.set_context_tags(tags);
    }
    fn generate_structured(
        &mut self,
        schema: &str,
        prompt: &str,
        limit: u32,
    ) -> Result<(Value, UsageReport), InferenceError> {
        assert!(self.calls < 12, "bounded qualification model-call budget");
        self.calls += 1;
        let result = self.inner.generate_structured(schema, prompt, limit);
        let captured = match &result {
            Ok((response, usage)) => {
                json!({"response":response,"usage":{"prompt":usage.prompt_tokens,"completion":usage.completion_tokens,"milliseconds":usage.wall_time_ms}})
            }
            Err(error) => json!({"error":error.to_string()}),
        };
        writeln!(
            self.file,
            "{}",
            json!({"schema":schema,"prompt":prompt,"limit":limit,"result":captured})
        )
        .unwrap();
        result
    }
}

#[test]
#[ignore = "resource-gated installed local model, fictional native form and reviewed-fill coordinator; no external site"]
fn resource_gated_history_pipeline_keeps_subjects_separate() {
    let model = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL")
        .unwrap_or_else(|_| "ministral-3:14b".into());
    crate::experience_inference::tests::with_resource_gated_named_model(&model, || {
        let directory = tempfile::Builder::new()
            .prefix("history-pipeline-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap()
            .keep();
        let layout =
            std::env::var("WAYPOINT_HISTORY_PIPELINE_LAYOUT").unwrap_or_else(|_| "grouped".into());
        let fixture_name = match layout.as_str() {
            "grouped" => "tests/form-career-subjects.html",
            "headings" => "tests/form-career-subjects-headings.html",
            _ => panic!("Choose only an owned fictional layout"),
        };
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(fixture_name)
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(&fixture).unwrap().to_string();
        let mut browser = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = browser.open(&url).unwrap();
        assert!(live.walls.is_empty(), "{:?}", live.walls);
        assert_eq!(live.schema.fields.len(), 11);
        let (_, resident) = waypoint_inference::ollama::http_request(
            "127.0.0.1:11434".parse().unwrap(),
            "GET",
            "/api/ps",
            None,
            3000,
        )
        .unwrap();
        std::fs::write(directory.join("metadata.private.json"),json!({"model":model,"resident":serde_json::from_slice::<Value>(&resident).unwrap(),"layout":layout,"sources":{"normalizer":waypoint_domain::sha256_hex(include_str!("native_month.rs").as_bytes()),"visual_review":waypoint_domain::sha256_hex(include_str!("visual_review.rs").as_bytes()),"qualification":waypoint_domain::sha256_hex(include_str!("history_pipeline_tests.rs").as_bytes()),"lock":waypoint_domain::sha256_hex(include_str!("../../../Cargo.lock").as_bytes()),"classifier":waypoint_domain::sha256_hex(include_str!("suggestions.rs").as_bytes()),"history":waypoint_domain::sha256_hex(include_str!("history_advice.rs").as_bytes()),"fixture":waypoint_domain::sha256_hex(&std::fs::read(fixture).unwrap())},"schema":live.schema}).to_string()).unwrap();
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", &model)
            .unwrap()
            .with_resident_context();
        let mut guarded = crate::resident_inference::ResidentInference::new(
            llm,
            model.clone(),
            crate::resident_model_identity,
        );
        let mut traced = Trace {
            inner: &mut guarded,
            file: std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join("model.private.jsonl"))
                .unwrap(),
            calls: 0,
        };
        let history=json!({"version":1,"employment":[{"id":"a".repeat(32),"employer":"Fictional Engineering","title":"Systems Engineer","start_date":"2020-06","reviewed":true}],"education":[{"id":"b".repeat(32),"institution":"Fictional Institute","qualification":"BSc Engineering","reviewed":true}]}).to_string();
        let profile = json!({"first_name":"Avery","email":"avery@example.invalid","address":"10 Candidate Street","career_history":history});
        let mut preview = form_preview::suggest_for_live_form(live, &mut traced, &profile).unwrap();
        std::fs::write(directory.join("preview.private.json"),json!({"mappings":preview.suggestions,"manual":preview.manual_review,"restricted":preview.model_review_selectors}).to_string()).unwrap();
        println!("Career pipeline evidence: {}", directory.display());
        assert_eq!(
            preview.suggestions.len(),
            2,
            "contact mapper must propose only candidate name and email"
        );
        assert!(preview.suggestions.iter().all(|mapping| matches!(
            mapping.field_selector.as_str(),
            "#candidate-name" | "#candidate-email"
        )));
        for selector in [
            "#work-company",
            "#work-title",
            "#work-month",
            "#school-name",
            "#qualification",
        ] {
            assert!(
                !preview.model_review_selectors.contains(selector),
                "ordinary career field was blocked: {selector}; {:?}",
                preview.manual_review
            );
        }
        for selector in [
            "#referee-name",
            "#referee-company",
            "#office-address",
            "#salary",
        ] {
            assert!(
                preview.model_review_selectors.contains(selector)
                    || preview
                        .schema
                        .fields
                        .iter()
                        .any(|field| field.selector == selector && field.sensitive),
                "restricted subject not flagged: {selector}"
            );
        }
        for (entry, selectors) in [
            (
                "a".repeat(32),
                vec!["#work-company", "#work-title", "#work-month"],
            ),
            ("b".repeat(32), vec!["#school-name", "#qualification"]),
        ] {
            let fields = selectors
                .iter()
                .map(|selector| {
                    preview
                        .schema
                        .fields
                        .iter()
                        .find(|field| field.selector == *selector)
                        .unwrap()
                        .clone()
                })
                .collect();
            let input = history_advice::Inputs {
                record: candidate_history::selected(&history, &entry).unwrap(),
                fields,
            };
            let proposed = history_advice::propose(&mut traced, &input).unwrap();
            std::fs::write(
                directory.join(format!("history-{entry}.private.json")),
                proposed.to_string(),
            )
            .unwrap();
            let items = proposed["proposals"].as_array().unwrap();
            assert_eq!(items.len(), selectors.len(), "history completeness");
            for item in items {
                preview.suggestions.push(ProposedMapping {
                    field_selector: item["selector"].as_str().unwrap().into(),
                    value: serde_json::from_value(item["value"].clone()).unwrap(),
                });
            }
        }
        let expected = [
            ("#candidate-name", "Avery"),
            ("#candidate-email", "avery@example.invalid"),
            ("#work-company", "Fictional Engineering"),
            ("#work-title", "Systems Engineer"),
            ("#work-month", "2020-06"),
            ("#school-name", "Fictional Institute"),
            ("#qualification", "BSc Engineering"),
        ];
        assert_eq!(preview.suggestions.len(), expected.len());
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("fictional-career-pipeline").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Fictional role".into(),
            employer: "Fictional destination".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.clone()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("fictional-career-pipeline-intent").unwrap();
        let packet = Sha256Digest::of(&profile.to_string());
        let mut grant = ApprovalGrant {
            id: waypoint_domain::GrantId::new("fictional-career-pipeline-grant").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: waypoint_domain::GrantOperation::Prefill,
            nonce: "fictional".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        assert_eq!(
            form_preview::apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut browser,
                &url,
                &preview,
                &mut grant,
                form_preview::FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut completed
                }
            )
            .unwrap(),
            expected.len()
        );
        for (selector, value) in expected {
            browser.verify_text_value(selector, value).unwrap();
        }
        for selector in [
            "#referee-name",
            "#referee-company",
            "#office-address",
            "#salary",
        ] {
            browser.verify_text_value(selector, "").unwrap();
        }
        let image = browser.screenshot_base64().unwrap();
        std::fs::write(
            directory.join("filled.png"),
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &image).unwrap(),
        )
        .unwrap();
        let visual_expected: Vec<_> = expected
            .iter()
            .map(|(selector, value)| {
                (
                    preview
                        .schema
                        .fields
                        .iter()
                        .find(|field| field.selector == *selector)
                        .unwrap()
                        .label
                        .clone(),
                    (*value).into(),
                )
            })
            .collect();
        let mut visual =
            visual_review::benchmark_analyze(&model, &image, &visual_expected).unwrap();
        let completed_selectors: Vec<_> = expected
            .iter()
            .map(|(selector, _)| (*selector).to_owned())
            .collect();
        let (verified, unconfirmed) = visual_review::crosscheck_live_values(
            &mut browser,
            &preview,
            &completed_selectors,
            &mut visual,
        );
        std::fs::write(directory.join("visual.private.json"), visual.to_string()).unwrap();
        assert_eq!((verified, unconfirmed), (expected.len(), 0));
        assert!(
            visual["comparisons"]
                .as_array()
                .is_some_and(|items| items.len() == expected.len()
                    && items
                        .iter()
                        .all(|item| item["status"] == "matches_transcription")),
            "visual readback: {visual}"
        );
        assert!(browser
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    });
}
