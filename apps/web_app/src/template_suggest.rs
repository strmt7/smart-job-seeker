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
pub fn start(id: String, model: String, input: Inputs) -> Result<(), String> {
    let lease = crate::doc_suggest::lease()?;
    let mut task = TASK
        .lock()
        .map_err(|_| "Template suggestions unavailable")?;
    if task.as_ref().is_some_and(|t| t.result.is_none()) {
        return Err("A template suggestion is already running".into());
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
    if input.before.is_empty()
        || input.before.len() > 1000
        || input.before.chars().any(char::is_control)
        || text.match_indices(&input.before).count() != 1
    {
        return Err("Select a unique single-line passage from the template".into());
    }
    // A passage deliberately selected by the user is source material for the
    // rewrite, even if it has not been added to the reviewed fact inventory.
    // Preserve its provenance: selecting text does not independently verify it.
    let mut sources = vec![format!(
        "User-selected template passage (not independently verified): {}",
        input.before
    )];
    sources.extend(input.facts);
    let request=crate::answer_task::Inputs{session_id:String::new(),selector:String::new(),digest:String::new(),question:format!("Rewrite this original CV or cover letter passage for the job. Preserve the meaning of the user-selected template passage and supported facts; do not add qualifications or details. Return only the replacement passage: {}",input.before),listing:input.listing,facts:sources,instructions:input.instructions};
    let suggestion = crate::answer_task::generate(model, &request)?;
    if suggestion["text"]
        .as_str()
        .is_none_or(|text| text.chars().any(char::is_control))
    {
        return Err("Model returned a multi-line or invalid replacement; review manually".into());
    }
    Ok(suggestion)
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK
        .lock()
        .map_err(|_| "Template suggestions unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|t| t.id == id)
        .ok_or("Template suggestion not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    let (document_id, job_id, before, digest) = (
        task.document_id.clone(),
        task.job_id.clone(),
        task.before.clone(),
        task.digest.clone(),
    );
    *tasks = None;
    Ok(match result {
        Ok(suggestion) => {
            json!({"status":"complete","suggestion":suggestion,"document_id":document_id,"job_id":job_id,"before":before,"digest":digest})
        }
        Err(error) => json!({"status":"failed","error":error}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use waypoint_inference::{CapabilityReport, InferenceError, UsageReport};
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
    fn suggestion_uses_existing_passage_and_two_reviewable_passes() {
        let mut model = Fake { calls: 0 };
        let result = suggest(&mut model, input("Fictional research")).unwrap();
        assert_eq!(result["text"], "Applied research");
        assert_eq!(
            result["sources"][0],
            "User-selected template passage (not independently verified): Fictional research"
        );
        assert_eq!(model.calls, 2);
        let mut without_reviewed_facts = input("Fictional research");
        without_reviewed_facts.facts.clear();
        let result = suggest(&mut model, without_reviewed_facts).unwrap();
        assert!(result["sources"][0]
            .as_str()
            .unwrap()
            .contains("not independently verified"));
        assert!(suggest(&mut model, input("Not present")).is_err());
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_template_suggestion_preserves_selected_fictional_experience() {
        let passage = "Designed accessible interfaces for screen-reader users.";
        let mut input = input_with_passage(passage, passage);
        input.facts = vec!["Maintained Python services.".into()];
        input.listing = "Frontend developer: build accessible web interfaces.".into();
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
    }
}
