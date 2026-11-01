//! Local suggestion for a user-selected PDF or DOCX passage. No document is modified here.
use serde_json::{json, Value};
use std::sync::Mutex;
use waypoint_inference::{Inference, OllamaInference};
pub struct Inputs {
    pub document_id: String,
    pub document_name: String,
    pub job_id: String,
    pub digest: String,
    pub source: Vec<u8>,
    pub before: String,
    pub listing: String,
    pub facts: Vec<String>,
    pub instructions: String,
}
struct Task {
    id: String,
    document_id: String,
    job_id: String,
    before: String,
    digest: String,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
// Retain recent responses for lost HTTP responses or context-check retries.
// These candidate details stay in memory and expire; they are not persisted.
static COMPLETED: Mutex<Vec<(String, Value, std::time::Instant)>> = Mutex::new(Vec::new());

fn terminal_result(task: &Task) -> Option<Value> {
    task.result.as_ref().map(|result| match result {
        Ok(suggestion) => json!({"status":"complete","suggestion":suggestion,"document_id":task.document_id,"job_id":task.job_id,"before":task.before,"digest":task.digest}),
        Err(error) => json!({"status":"failed","error":error}),
    })
}

fn remember_result(
    completed: &mut Vec<(String, Value, std::time::Instant)>,
    id: &str,
    result: Value,
) {
    completed.retain(|(saved_id, _, at)| {
        saved_id != id && at.elapsed() <= std::time::Duration::from_secs(600)
    });
    if completed.len() >= 16 {
        completed.remove(0);
    }
    completed.push((id.to_owned(), result, std::time::Instant::now()));
}
pub fn start(id: String, model: String, input: Inputs) -> Result<(), String> {
    let lease = crate::doc_suggest::lease()?;
    let mut task = TASK
        .lock()
        .map_err(|_| "Template suggestions unavailable")?;
    if let Some(previous) = task.as_ref() {
        let response =
            terminal_result(previous).ok_or("A template suggestion is already running")?;
        let mut completed = COMPLETED
            .lock()
            .map_err(|_| "Template suggestions unavailable")?;
        remember_result(&mut completed, &previous.id, response);
    }
    *task = Some(Task {
        id: id.clone(),
        document_id: input.document_id.clone(),
        job_id: input.job_id.clone(),
        before: input.before.clone(),
        digest: input.digest.clone(),
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let _lease = lease;
        let result = std::panic::catch_unwind(|| {
            let llm = OllamaInference::new("http://127.0.0.1:11434", model.clone())
                .map_err(|e| e.to_string())?
                .with_resident_context();
            let mut model = crate::resident_inference::ResidentInference::new(
                llm,
                model,
                crate::resident_model,
            );
            suggest(&mut model, input)
        })
        .unwrap_or_else(|_| Err("Template suggestion stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|t| t.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}
fn suggest(model: &mut dyn Inference, input: Inputs) -> Result<Value, String> {
    let text = crate::document_extract::extract(&input.document_name, &input.source)?;
    let unique = text.find(&input.before).is_some_and(|at| {
        input
            .before
            .chars()
            .next()
            .is_some_and(|first| !text[at + first.len_utf8()..].contains(&input.before))
    });
    if input.before.is_empty()
        || input.before.len() > 1000
        || input.before.chars().any(char::is_control)
        || !unique
    {
        return Err("Select a unique single-line passage from the template".into());
    }
    // A passage deliberately selected by the user is source material for the
    // rewrite, even if it has not been added to the reviewed fact inventory.
    // Preserve its provenance: selecting text does not independently verify it.
    let selected_source = format!(
        "User-selected template passage (not independently verified): {}",
        input.before
    );
    let mut sources = vec![selected_source.clone()];
    sources.extend(input.facts);
    let request=crate::answer_task::Inputs{max_length_utf16:None,session_id:String::new(),selector:String::new(),digest:String::new(),question:"Rewrite the user-selected CV or cover letter passage for the job. Preserve the passage's meaning and supported facts; add other candidate facts only if the writing preferences request it. Return only the replacement passage.".into(),listing:input.listing,available_facts:sources.len(),facts:sources,instructions:input.instructions};
    let mut suggestion = crate::answer_task::generate_template_with_selection(model, request)?;
    if suggestion["text"]
        .as_str()
        .is_none_or(|text| text.chars().any(char::is_control))
    {
        return Err("Model returned a multi-line or invalid replacement; review manually".into());
    }
    let replacement = suggestion["text"].as_str().unwrap_or_default();
    suggestion["first_person_opening_changed"] =
        json!(first_person_opening(&input.before) != first_person_opening(replacement));
    suggestion["additional_sources_cited"] = json!(["sources", "initial_draft_sources"]
        .iter()
        .any(
            |key| suggestion[*key].as_array().is_some_and(|sources| sources
                .iter()
                .any(|source| source.as_str() != Some(selected_source.as_str())))
        ));
    qualify_unverified_support(&mut suggestion, &selected_source);
    Ok(suggestion)
}

/// Reviews cite sources for the whole suggestion, not individual claims. If
/// the selected passage is cited, no supported claim can be attributed solely
/// to reviewed facts. Keep the model's judgment and explain the uncertainty.
fn qualify_unverified_support(suggestion: &mut Value, selected_source: &str) {
    let selected_cited = suggestion["sources"].as_array().is_some_and(|sources| {
        sources
            .iter()
            .any(|source| source.as_str() == Some(selected_source))
    });
    if !selected_cited {
        return;
    }
    if let Some(claims) = suggestion["evidence_check"]["claims"].as_array_mut() {
        for claim in claims {
            if claim["status"] == "supported" {
                claim["status"] = json!("uncertain");
                claim["model_status"] = json!("supported");
                claim["provenance_guard"] = json!(true);
                let explanation = claim["explanation"].as_str().unwrap_or_default();
                claim["explanation"] = json!(format!("The model judged this supported, but the suggestion cites the independently unverified template passage and does not attribute sources to individual claims. Review the original evidence. Model explanation: {explanation}"));
            }
        }
    }
}

fn first_person_opening(text: &str) -> bool {
    text.split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("i") || word.eq_ignore_ascii_case("we"))
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK
        .lock()
        .map_err(|_| "Template suggestions unavailable")?;
    let mut completed = COMPLETED
        .lock()
        .map_err(|_| "Template suggestions unavailable")?;
    completed.retain(|(_, _, at)| at.elapsed() <= std::time::Duration::from_secs(600));
    if let Some((_, result, _)) = completed.iter().find(|(task_id, _, _)| task_id == id) {
        return Ok(result.clone());
    }
    let task = tasks
        .as_ref()
        .filter(|t| t.id == id)
        .ok_or("Template suggestion not found")?;
    let Some(response) = terminal_result(task) else {
        return Ok(json!({"status":"running"}));
    };
    remember_result(&mut completed, id, response.clone());
    *tasks = None;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use waypoint_inference::{CapabilityReport, InferenceError, UsageReport};
    static TASK_TEST_LOCK: Mutex<()> = Mutex::new(());
    struct Fake {
        calls: usize,
    }
    impl Inference for Fake {
        fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
            unreachable!()
        }
        fn generate_structured(
            &mut self,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<(Value, UsageReport), InferenceError> {
            self.calls += 1;
            Ok((
                json!({"text":"Applied research","facts":[1]}),
                UsageReport {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    wall_time_ms: 0,
                },
            ))
        }
    }
    fn input_with_passage(before: &str, document_text: &str) -> Inputs {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(format!("<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'><w:p><w:r><w:t>{document_text}</w:t></w:r></w:p></w:document>").as_bytes()).unwrap();
        Inputs {
            document_id: "d".into(),
            document_name: "source.docx".into(),
            job_id: "j".into(),
            digest: "x".into(),
            source: writer.finish().unwrap().into_inner(),
            before: before.into(),
            listing: "Research role".into(),
            facts: vec!["Research experience".into()],
            instructions: String::new(),
        }
    }
    fn input(before: &str) -> Inputs {
        input_with_passage(before, "Fictional research")
    }
    #[test]
    fn suggestion_uses_existing_passage_and_checks_evidence() {
        let mut model = Fake { calls: 0 };
        let result = suggest(&mut model, input("Fictional research")).unwrap();
        assert_eq!(result["text"], "Applied research");
        assert_eq!(
            result["sources"][0],
            "User-selected template passage (not independently verified): Fictional research"
        );
        assert_eq!(model.calls, 4);
        assert_eq!(result["evidence_check"]["status"], "invalid");
        assert_eq!(result["selection"]["template_context_omitted"], 1);
        assert_eq!(result["first_person_opening_changed"], false);
        let changed_voice = suggest(
            &mut model,
            input_with_passage("I researched", "I researched"),
        )
        .unwrap();
        assert_eq!(changed_voice["first_person_opening_changed"], true);
        let mut without_reviewed_facts = input("Fictional research");
        without_reviewed_facts.facts.clear();
        let result = suggest(&mut model, without_reviewed_facts).unwrap();
        assert!(result["sources"][0]
            .as_str()
            .unwrap()
            .contains("not independently verified"));
        assert!(suggest(&mut model, input("Not present")).is_err());
        assert!(suggest(&mut model, input_with_passage("aaa", "aaaa")).is_err());
        assert!(suggest(&mut model, input_with_passage("éé", "ééé")).is_err());
    }

    #[test]
    fn template_support_requires_review_when_unverified_source_is_cited() {
        let source = "User-selected template passage (not independently verified): Wrote code";
        let mut mixed = json!({"sources":[source,"Reviewed Python experience"],"evidence_check":{"status":"completed","claims":[
            {"excerpt":"Wrote code","status":"supported","explanation":"Matches source"},
            {"excerpt":"Led a team","status":"unsupported","explanation":"Missing"},
            {"excerpt":"Five years","status":"uncertain","explanation":"Not clear"}
        ]}});
        qualify_unverified_support(&mut mixed, source);
        assert_eq!(mixed["evidence_check"]["claims"][0]["status"], "uncertain");
        assert_eq!(
            mixed["evidence_check"]["claims"][0]["model_status"],
            "supported"
        );
        assert_eq!(
            mixed["evidence_check"]["claims"][0]["provenance_guard"],
            true
        );
        assert!(mixed["evidence_check"]["claims"][0]["explanation"]
            .as_str()
            .unwrap()
            .contains("Matches source"));
        assert_eq!(
            mixed["evidence_check"]["claims"][1]["status"],
            "unsupported"
        );
        assert_eq!(mixed["evidence_check"]["claims"][2]["status"], "uncertain");
        let mut reviewed = json!({"sources":["Reviewed Python experience"],"evidence_check":{"status":"completed","claims":[{"excerpt":"Python","status":"supported","explanation":"Matches"}]}});
        qualify_unverified_support(&mut reviewed, source);
        assert_eq!(
            reviewed["evidence_check"]["claims"][0]["status"],
            "supported"
        );
        let mut failed = json!({"sources":[source],"evidence_check":{"status":"invalid"}});
        qualify_unverified_support(&mut failed, source);
        assert_eq!(failed["evidence_check"]["status"], "invalid");
    }

    #[test]
    fn template_suggestion_reports_citations_beyond_selected_passage() {
        struct ExtraSource;
        impl Inference for ExtraSource {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                unreachable!()
            }
            fn generate_structured(
                &mut self,
                schema: &str,
                _: &str,
                _: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                let value = if schema == "application_answer_evidence_check" {
                    json!({"claims":[]})
                } else if schema == "application_answer_style" {
                    json!({"text":"Built websites and maintained Python services.","facts":[1]})
                } else {
                    json!({"text":"Built websites and maintained Python services.","facts":[1,2]})
                };
                Ok((
                    value,
                    UsageReport {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        wall_time_ms: 0,
                    },
                ))
            }
        }
        let mut input = input_with_passage("Built websites.", "Built websites.");
        input.facts = vec!["Maintained Python services.".into()];
        input.instructions = "Include the Python experience.".into();
        let result = suggest(&mut ExtraSource, input).unwrap();
        assert_eq!(result["additional_sources_cited"], true);
        assert_eq!(result["sources"].as_array().unwrap().len(), 1);
        assert_eq!(result["initial_draft_sources"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn completed_suggestion_can_be_retried_and_is_revalidated_against_current_inputs() {
        use std::io::{Read, Write};
        let _guard = TASK_TEST_LOCK.lock().unwrap();
        let mut ws = waypoint_workspace::Workspace::in_memory().unwrap();
        let added = crate::manual_job::add(
            &mut ws,
            &json!({"url":"https://example.org/role","title":"Engineer","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let job = waypoint_domain::JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        ws.add_document("fixture-document", "fixture.docx", b"fictional", 1)
            .unwrap();
        let input =
            crate::template_suggestion_inputs(&ws, "fixture-document", &job, "Original").unwrap();
        *TASK.lock().unwrap() = Some(Task {
            id: "retry-fixture".into(),
            document_id: input.document_id,
            job_id: input.job_id,
            before: input.before,
            digest: input.digest,
            result: Some(Ok(json!({"text":"Revised","sources":[]}))),
        });
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let poll_api = |ws: &mut waypoint_workspace::Workspace| {
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = json!({"task_id":"retry-fixture"}).to_string();
            write!(client,"POST /api/documents/template-suggest-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(&mut server, ws, &mut None, "fixture", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        let first = poll_api(&mut ws);
        assert_eq!(first["status"], "complete");
        assert_eq!(poll_api(&mut ws), first);
        ws.add_fact(
            "A changed fictional qualification",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        let changed = poll_api(&mut ws);
        assert_eq!(changed["status"], "failed");
        assert!(changed["error"].as_str().unwrap().contains("changed"));
    }

    #[test]
    fn retained_suggestions_are_bounded_and_expire() {
        let mut saved = vec![(
            "expired".into(),
            json!({}),
            std::time::Instant::now() - std::time::Duration::from_secs(601),
        )];
        for i in 0..17 {
            remember_result(
                &mut saved,
                &format!("task-{i}"),
                json!({"status":"complete"}),
            );
        }
        assert_eq!(saved.len(), 16);
        assert!(!saved
            .iter()
            .any(|(id, _, _)| id == "expired" || id == "task-0"));
        assert!(saved.iter().any(|(id, _, _)| id == "task-16"));
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_long_template_context_retains_selected_passage() {
        let model = OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_context_window(4096)
            .unwrap();
        let mut model = crate::resident_inference::ResidentInference::new(
            model,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let passage = "Designed accessible interfaces for screen-reader users.";
        let mut request = input_with_passage(passage, passage);
        request.listing = "Frontend role: create accessible web interfaces. ".repeat(160);
        request.instructions = "Write concisely and avoid embellishment. ".repeat(50);
        request.facts = (0..25)
            .map(|i| format!("Fictional task {i}: maintained a web interface."))
            .collect();
        let result = suggest(&mut model, request).unwrap();
        println!("fictional long-template suggestion: {result}");
        assert_eq!(result["selection"]["selected_passage_retained"], true);
        assert_eq!(result["selection"]["draft_listing_truncated"], true);
        assert_eq!(result["selection"]["draft_instructions_truncated"], true);
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_template_suggestion_preserves_selected_fictional_experience() {
        struct Recording<I>(I);
        impl<I: Inference> Inference for Recording<I> {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                self.0.probe()
            }
            fn generate_structured(
                &mut self,
                schema: &str,
                prompt: &str,
                max: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                let result = self.0.generate_structured(schema, prompt, max)?;
                println!("Fictional {schema} response: {}", result.0);
                Ok(result)
            }
        }
        let passage = "Designed accessible interfaces for screen-reader users.";
        let mut input = input_with_passage(passage, passage);
        input.facts = vec!["Maintained Python services.".into()];
        input.listing = "Frontend developer: build accessible web interfaces.".into();
        let model = OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_context_window(4096)
            .unwrap();
        let model = crate::resident_inference::ResidentInference::new(
            model,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let mut model = Recording(model);
        let result = suggest(&mut model, input).unwrap();
        println!("Fictional template suggestion: {result}");
        let text = result["text"].as_str().unwrap().to_lowercase();
        assert!(
            text.contains("accessib"),
            "the selected accessibility experience was omitted: {text}"
        );
        assert!(
            text.contains("screen reader") || text.contains("screen-reader"),
            "the selected screen-reader detail was omitted: {text}"
        );
        assert!(
            !text.contains("ten years") && !text.contains("10 years"),
            "unsupported duration: {text}"
        );
        if text.contains("python") {
            assert_eq!(
                result["additional_sources_cited"], true,
                "the added qualification should be flagged"
            );
        }
        assert!(
            !text.contains("python"),
            "unrelated qualification added to selected passage: {text}"
        );
        let evidence = &result["evidence_check"];
        assert_eq!(
            evidence["status"], "completed",
            "evidence check: {evidence}"
        );
        assert!(
            evidence["claims"]
                .as_array()
                .is_some_and(|claims| claims.iter().any(|claim| {
                    claim["status"] == "uncertain"
                        && claim["excerpt"].as_str().is_some_and(|excerpt| {
                            !excerpt.is_empty() && text.contains(&excerpt.to_lowercase())
                        })
                })),
            "unverified template evidence should require review: {evidence}"
        );
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_template_rewrite_can_add_explicitly_requested_fact() {
        let passage = "Designed accessible interfaces for screen-reader users.";
        let mut input = input_with_passage(passage, passage);
        input.facts = vec!["Maintained Python services.".into()];
        input.listing =
            "Developer: build accessible interfaces and maintain Python services.".into();
        input.instructions =
            "Include my Python service maintenance experience in this passage.".into();
        let model = OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_context_window(4096)
            .unwrap();
        let mut model = crate::resident_inference::ResidentInference::new(
            model,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let result = suggest(&mut model, input).unwrap();
        println!("Fictional requested addition: {result}");
        let text = result["text"].as_str().unwrap().to_lowercase();
        assert!(
            text.contains("python"),
            "requested experience omitted: {text}"
        );
        assert!(
            text.contains("accessib"),
            "original experience omitted: {text}"
        );
        assert_eq!(result["additional_sources_cited"], true);
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_template_suggestion_rewrites_wordy_fictional_passage() {
        let passage = "Responsible for the development of accessible web interfaces for users of screen readers.";
        let mut input = input_with_passage(passage, passage);
        input.facts.clear();
        input.listing = "Frontend developer: build accessible web interfaces.".into();
        input.instructions = "Use active voice and concise language.".into();
        let model = OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_context_window(4096)
            .unwrap();
        let mut model = crate::resident_inference::ResidentInference::new(
            model,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let result = suggest(&mut model, input).unwrap();
        println!("Fictional wordy template suggestion: {result}");
        let text = result["text"].as_str().unwrap();
        let lower = text.to_lowercase();
        assert_ne!(text, passage, "the passage was not revised");
        assert_eq!(
            result["first_person_opening_changed"],
            lower.starts_with("i ")
        );
        assert!(
            text.len() < passage.len(),
            "the revision was not concise: {text}"
        );
        assert!(
            lower.contains("accessib"),
            "accessibility was omitted: {text}"
        );
        assert!(
            lower.contains("screen reader") || lower.contains("screen-reader"),
            "screen-reader detail was omitted: {text}"
        );
        assert_eq!(result["evidence_check"]["status"], "completed");
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_template_rewrite_preserves_first_person_past_tense() {
        let passage =
            "I collaborated with designers to improve the accessibility of web interfaces.";
        let mut input = input_with_passage(passage, passage);
        input.facts.clear();
        input.listing = "Frontend developer: work with designers on accessibility.".into();
        input.instructions = "Be concise.".into();
        let model = OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_context_window(4096)
            .unwrap();
        let mut model = crate::resident_inference::ResidentInference::new(
            model,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let result = suggest(&mut model, input).unwrap();
        println!("Fictional cover letter suggestion: {result}");
        let text = result["text"].as_str().unwrap().to_lowercase();
        assert!(text.starts_with("i "), "first person was lost: {text}");
        assert!(
            text.contains("collaborated") || text.contains("worked") || text.contains("partnered"),
            "past-tense collaboration was lost: {text}"
        );
        assert!(
            text.contains("design"),
            "collaborators were omitted: {text}"
        );
        assert!(
            text.contains("accessib"),
            "accessibility was omitted: {text}"
        );
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_two_template_passages_are_drafted_sequentially() {
        let first = "Designed accessible interfaces for screen-reader users.";
        let second = "Maintained Python services for three years.";
        let source = format!("{first}\n{second}");
        let model = OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_context_window(4096)
            .unwrap();
        let mut model = crate::resident_inference::ResidentInference::new(
            model,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let mut outputs = Vec::new();
        for passage in [first, second] {
            let mut input = input_with_passage(passage, &source);
            input.facts.clear();
            input.listing = "Build accessible web interfaces and maintain Python services.".into();
            outputs.push(suggest(&mut model, input).unwrap());
        }
        println!("Fictional sequential template suggestions: {outputs:?}");
        let first_text = outputs[0]["text"].as_str().unwrap().to_lowercase();
        let second_text = outputs[1]["text"].as_str().unwrap().to_lowercase();
        assert!(first_text.contains("accessib"));
        assert!(second_text.contains("python"));
        assert!(!second_text.contains("five years") && !second_text.contains("5 years"));
        assert!(outputs
            .iter()
            .all(|output| output["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|source| source
                    .as_str()
                    .unwrap()
                    .contains("not independently verified"))));
    }
}
