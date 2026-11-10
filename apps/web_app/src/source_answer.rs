//! Advisory drafts from saved public questions. No browser, fill plan or grant.
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};
use waypoint_browser::capture_discovery::PublicFormOutline;
use waypoint_domain::{ApplicationState, JobIdentityId, Sha256Digest};
use waypoint_workspace::Workspace;

fn read_record(path: &Path) -> Result<Value, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Saved question record not found")?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if !metadata.is_file() || linked || metadata.len() > 256 * 1024 {
        return Err("Invalid saved question record".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(256 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|_| "Cannot read saved question record")?;
    if bytes.len() > 256 * 1024 {
        return Err("Saved question record exceeds its limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid saved question record".into())
}

pub fn inputs(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    number: usize,
) -> Result<crate::answer_task::Inputs, String> {
    let selected = ws
        .job(job)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    if !matches!(
        selected.state,
        ApplicationState::Shortlisted | ApplicationState::Prepared
    ) {
        return Err("Shortlist this job before preparing answers".into());
    }
    if ws.has_duplicate_hold(job).map_err(|e| e.to_string())? {
        return Err("Review possible duplicates before preparing answers".into());
    }
    let root = crate::attempt_record::attempt_directory(ws, job, attempt)?;
    let record = read_record(&root.join("source-page-summary.json"))?;
    let url = selected
        .canonical_url
        .as_deref()
        .ok_or("Job has no source URL")?;
    let url = url::Url::parse(url).map_err(|_| "Invalid job source URL")?;
    if record["status"] != "complete"
        || record["job_id"] != job.as_str()
        || record["attempt_id"] != attempt
        || record["result"]["archive_written"] != true
        || record["source_url_sha256"].as_str()
            != Some(waypoint_domain::sha256_hex(url.as_str().as_bytes()).as_str())
    {
        return Err("Save a new copy of the current job page before preparing answers".into());
    }
    let discovery = &record["result"]["report"]["discovery"];
    let snapshot = discovery["form_outline_snapshot_sha256"]
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("Saved questions have no rendering checkpoint; save a new page copy")?;
    let outline: PublicFormOutline = serde_json::from_value(discovery["form_outline"].clone())
        .map_err(|_| "Saved page questions are unavailable")?;
    if !outline.valid() || !matches!(outline.status.as_str(), "observed" | "partial") {
        return Err("Saved page questions are incomplete or invalid".into());
    }
    let control = outline
        .controls
        .iter()
        .find(|c| c.number == number)
        .ok_or("Choose a question from this page copy")?;
    if control.role != "textbox" || control.disabled == Some(true) || control.context_incomplete {
        return Err(
            "This control needs manual review; choose a text question with its saved context"
                .into(),
        );
    }
    let label = control
        .label
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or("The saved question has no complete label")?;
    let question = [
        control.context.join(" · "),
        label.to_owned(),
        control.description.clone().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join("\n");
    if crate::answer_task::json_prefix(&question, 1000) != question
        || waypoint_applications::driver::is_sensitive_label(&question)
    {
        return Err("This question needs your own answer or manual review".into());
    }
    let (all_facts, search_instructions) = crate::fit_candidate_context(ws)?;
    if all_facts.is_empty() {
        return Err("Add reviewed qualifications before drafting an answer".into());
    }
    let (_, listing, listing_digest) =
        crate::fit_listing_digest(ws, job, &all_facts, &search_instructions)?;
    let full_description = ws
        .job_description(job)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let description_digest = Sha256Digest::of(&full_description);
    let profile: Value = ws
        .candidate_profile()
        .map_err(|e| e.to_string())?
        .map(|text| serde_json::from_str(&text))
        .transpose()
        .map_err(|_| "Stored profile is invalid")?
        .unwrap_or(json!({}));
    let instructions = profile["document_instructions"].as_str().unwrap_or("");
    if crate::answer_task::json_prefix(instructions, 1000) != instructions {
        return Err(
            "Writing instructions are too long for this answer; shorten them in settings".into(),
        );
    }
    let facts = crate::fit_semantic_pool(&all_facts, &format!("{question}\n{listing}"));
    let digest = Sha256Digest::of(&json!({"version":"source-answer-v1","job":job.as_str(),"attempt":attempt,"record":record,"snapshot":snapshot,"control":control,"listing_digest":listing_digest,"description_digest":description_digest.as_str(),"all_facts":all_facts,"facts":facts,"profile":profile}).to_string()).as_str().to_owned();
    Ok(crate::answer_task::Inputs {
        numeric_only: false,
        session_id: format!("source:{}:{attempt}", job.as_str()),
        selector: format!("control:{number}"),
        digest,
        question,
        listing,
        available_facts: all_facts.len(),
        facts,
        instructions: instructions.to_owned(),
        max_length_utf16: None,
        min_length_utf16: None,
    })
}

pub fn context(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    number: usize,
    input: &crate::answer_task::Inputs,
) -> Result<Value, String> {
    let root = crate::attempt_record::attempt_directory(ws, job, attempt)?;
    let mut drafts = Vec::new();
    let mut partial = false;
    let mut files_read = 0;
    for (index, entry) in fs::read_dir(root)
        .map_err(|_| "Cannot inspect saved drafts")?
        .enumerate()
    {
        if index >= 2000 || drafts.len() >= 8 {
            partial = true;
            break;
        }
        let entry = entry.map_err(|_| "Cannot inspect saved drafts")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("question-draft-") && name.ends_with(".json") {
            if files_read >= 8 {
                partial = true;
                break;
            }
            files_read += 1;
            let value = read_record(&entry.path())?;
            if value["type"] == "saved_question_draft"
                && value["control_number"] == number
                && value["source_attempt"] == attempt
            {
                drafts.push(json!({"file":name,"current":value["context_digest"] == input.digest,"question":value["question"],"answer":value["answer"]}));
            }
        }
    }
    Ok(
        json!({"question":input.question,"digest":input.digest,"drafts":drafts,"drafts_partial":partial,"notice":"This is a saved-page observation. Review the full question and current form before using any draft. Field limits and later steps were not assessed."}),
    )
}

pub fn finish(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    number: usize,
    task: &str,
) -> Result<Value, String> {
    let session = format!("source:{}:{attempt}", job.as_str());
    let selector = format!("control:{number}");
    let mut result = crate::answer_task::poll_bound(task, &session, &selector)?;
    if result["status"] == "running" {
        return Ok(result);
    }
    if result["status"] == "complete" {
        let current = inputs(ws, job, attempt, number);
        if current.as_ref().ok().map(|i| i.digest.as_str()) != result["digest"].as_str() {
            result = json!({"status":"failed","error":"The saved question, job or qualifications changed; prepare the answer again"});
        } else {
            let input = current?;
            let root = crate::attempt_record::attempt_directory(ws, job, attempt)?;
            if task.len() > 64
                || task.is_empty()
                || !task.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err("Invalid answer draft identity".into());
            }
            // Each task has an immutable sidecar. A lost response can retry;
            // it cannot overwrite a different draft or authorize filling.
            let filename = format!("question-draft-{task}.json");
            let value = json!({"schema":1,"type":"saved_question_draft","question":input.question,"source_attempt":attempt,"control_number":number,"context_digest":input.digest,"answer":result["answer"],"filled":false,"submitted":false});
            let destination = root.join(&filename);
            if destination.exists() {
                if read_record(&destination)? != value {
                    return Err("Saved answer record changed".into());
                }
            } else {
                use std::io::Write;
                let mut file = tempfile::NamedTempFile::new_in(&root)
                    .map_err(|_| "Cannot save answer draft")?;
                serde_json::to_writer_pretty(&mut file, &value)
                    .map_err(|_| "Cannot encode answer draft")?;
                file.flush().map_err(|_| "Cannot save answer draft")?;
                if file
                    .as_file()
                    .metadata()
                    .map_err(|_| "Cannot inspect answer draft")?
                    .len()
                    > 256 * 1024
                {
                    return Err("Answer draft exceeds the record limit".into());
                }
                file.persist_noclobber(destination)
                    .map_err(|_| "Cannot preserve answer draft")?;
            }
            result["saved_file"] = json!(filename);
            result["question"] = json!(input.question);
        }
    }
    result["session_id"] = json!(session);
    result["selector"] = json!(selector);
    crate::answer_task::complete(task, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ATTEMPT: &str = "source-question-fixture";
    fn post(ws: &mut Workspace, path: &str, body: &Value) -> Result<Value, String> {
        use std::{
            io::{Read, Write},
            net::{TcpListener, TcpStream},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let data = body.to_string();
        write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{data}",data.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let result = crate::serve(&mut server, ws, &mut None, "fixture-token", port);
        drop(server);
        result?;
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).map_err(|e| e.to_string())
    }
    fn fixture() -> (tempfile::TempDir, Workspace, JobIdentityId, Value) {
        fixture_with_shortlist(true)
    }
    fn fixture_with_shortlist(
        shortlisted: bool,
    ) -> (tempfile::TempDir, Workspace, JobIdentityId, Value) {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added = crate::manual_job::add(&mut ws, &json!({"url":"https://example.org/fictional-question","title":"Fictional engineer","employer":"Fictional organization","description":"Build reliable software using Rust."}), 1).unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        if shortlisted {
            ws.set_state(&job, ApplicationState::Shortlisted).unwrap();
        }
        ws.add_fact(
            "Built a fictional Rust service.",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        let record = crate::attempt_record::AttemptRecord::start(&ws, &job, ATTEMPT, 2)
            .unwrap()
            .unwrap();
        let value = json!({"status":"complete","job_id":job.as_str(),"attempt_id":ATTEMPT,"source_url_sha256":waypoint_domain::sha256_hex(b"https://example.org/fictional-question"),"result":{"archive_written":true,"report":{"discovery":{"form_outline_snapshot_sha256":"a".repeat(64),"form_outline":{"schema":1,"scope":"root_frame_accessibility_tree","status":"observed","reason":null,"nodes":3,"omitted_controls":0,"unlabelled_controls":0,"omitted_texts":0,"controls":[{"number":1,"role":"textbox","label":"Describe relevant experience","description":"Use a concrete example","context":["Engineering"],"context_incomplete":false,"required":true,"disabled":null}]}}}}});
        record.source_capture_result(&value).unwrap();
        (dir, ws, job, value)
    }
    fn rewrite(ws: &Workspace, job: &JobIdentityId, value: &Value) {
        let root = crate::attempt_record::attempt_directory(ws, job, ATTEMPT).unwrap();
        fs::write(
            root.join("source-page-summary.json"),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn saved_question_uses_complete_context_and_reviewed_facts_only() {
        let (_dir, ws, job, _) = fixture();
        let input = inputs(&ws, &job, ATTEMPT, 1).unwrap();
        assert_eq!(
            input.question,
            "Engineering\nDescribe relevant experience\nUse a concrete example"
        );
        assert!(input
            .facts
            .iter()
            .any(|s| s.contains("fictional Rust service")));
        assert_eq!(input.max_length_utf16, None);
        assert_eq!(inputs(&ws, &job, ATTEMPT, 1).unwrap().digest, input.digest);
        assert_eq!(
            context(&ws, &job, ATTEMPT, 1, &input).unwrap()["drafts"],
            json!([])
        );
    }
    #[test]
    fn question_endpoint_binds_the_selection_and_rejects_stale_start_before_inference() {
        let (_dir, mut ws, job, _) = fixture();
        let body = json!({"id":job.as_str(),"attempt_id":ATTEMPT,"number":1});
        let preview = post(&mut ws, "/api/job-source/question", &body).unwrap();
        assert!(preview["question"]
            .as_str()
            .unwrap()
            .contains("Engineering"));
        let mut stale = body.clone();
        stale["digest"] = json!("old");
        assert!(post(&mut ws, "/api/job-source/answer-start", &stale)
            .unwrap_err()
            .contains("changed"));
        let mut wrong = body;
        wrong["number"] = json!(257);
        assert!(post(&mut ws, "/api/job-source/question", &wrong).is_err());
    }
    #[test]
    fn unavailable_ambiguous_sensitive_and_action_controls_do_not_become_drafts() {
        let (_dir, ws, job, original) = fixture();
        for (key, replacement) in [
            ("role", json!("button")),
            ("role", json!("searchbox")),
            ("disabled", json!(true)),
            ("context_incomplete", json!(true)),
            ("label", Value::Null),
            ("label", json!("Salary expectations")),
            ("context", json!(["Work authorization"])),
            ("description", json!("State your citizenship")),
            ("value", json!("must-not-be-used")),
        ] {
            let mut value = original.clone();
            value["result"]["report"]["discovery"]["form_outline"]["controls"][0][key] =
                replacement;
            rewrite(&ws, &job, &value);
            assert!(inputs(&ws, &job, ATTEMPT, 1).is_err(), "{key}");
        }
        rewrite(&ws, &job, &original);
        assert!(inputs(&ws, &job, ATTEMPT, 2).is_err());
        assert!(inputs(&ws, &job, "../elsewhere", 1).is_err());
    }
    #[test]
    fn old_or_mismatched_captures_require_another_copy() {
        let (_dir, ws, job, original) = fixture();
        for (key, replacement) in [
            ("source_url_sha256", Value::Null),
            ("source_url_sha256", json!("b".repeat(64))),
            ("job_id", json!("other-job")),
            ("status", json!("failed")),
            ("attempt_id", json!("other-attempt")),
        ] {
            let mut value = original.clone();
            value[key] = replacement;
            rewrite(&ws, &job, &value);
            assert!(inputs(&ws, &job, ATTEMPT, 1).is_err(), "{key}");
        }
        let mut value = original;
        value["result"]["archive_written"] = json!(false);
        rewrite(&ws, &job, &value);
        assert!(inputs(&ws, &job, ATTEMPT, 1).is_err());
    }
    #[test]
    fn facts_instructions_and_source_changes_invalidate_context() {
        let (_dir, mut ws, job, mut record) = fixture();
        let first = inputs(&ws, &job, ATTEMPT, 1).unwrap().digest;
        ws.add_fact(
            "Documented fictional service recovery.",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        let second = inputs(&ws, &job, ATTEMPT, 1).unwrap().digest;
        assert_ne!(first, second);
        ws.save_candidate_profile(
            &json!({"document_instructions":"Use plain wording."}).to_string(),
        )
        .unwrap();
        let third = inputs(&ws, &job, ATTEMPT, 1).unwrap().digest;
        assert_ne!(second, third);
        record["result"]["report"]["discovery"]["form_outline_snapshot_sha256"] =
            json!("c".repeat(64));
        rewrite(&ws, &job, &record);
        assert_ne!(third, inputs(&ws, &job, ATTEMPT, 1).unwrap().digest);
    }
    #[test]
    fn preparation_requires_shortlisting() {
        let (_dir, ws, job, _) = fixture_with_shortlist(false);
        assert!(inputs(&ws, &job, ATTEMPT, 1)
            .err()
            .unwrap()
            .contains("Shortlist"));
    }
    #[test]
    fn drafts_archive_once_replay_and_stale_results_cannot_revive() {
        let _guard = crate::answer_task::TEST_LOCK.lock().unwrap();
        let (_dir, mut ws, job, _) = fixture();
        let input = inputs(&ws, &job, ATTEMPT, 1).unwrap();
        crate::answer_task::set_completed_fixture(
            "question-task",
            input.clone(),
            json!({"text":"I built a fictional Rust service.","sources":input.facts,"evidence_check":{"status":"supported"}}),
        );
        assert!(finish(&ws, &job, ATTEMPT, 2, "question-task").is_err());
        let result = finish(&ws, &job, ATTEMPT, 1, "question-task").unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(
            finish(&ws, &job, ATTEMPT, 1, "question-task").unwrap(),
            result
        );
        let root = crate::attempt_record::attempt_directory(&ws, &job, ATTEMPT).unwrap();
        let saved = fs::read(root.join(result["saved_file"].as_str().unwrap())).unwrap();
        assert_eq!(
            context(&ws, &job, ATTEMPT, 1, &input).unwrap()["drafts"][0]["current"],
            true
        );
        ws.add_fact(
            "New fictional qualification.",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        assert_eq!(
            finish(&ws, &job, ATTEMPT, 1, "question-task").unwrap()["status"],
            "failed"
        );
        assert_eq!(
            saved,
            fs::read(root.join(result["saved_file"].as_str().unwrap())).unwrap()
        );
        assert_eq!(
            context(
                &ws,
                &job,
                ATTEMPT,
                1,
                &inputs(&ws, &job, ATTEMPT, 1).unwrap()
            )
            .unwrap()["drafts"][0]["current"],
            false
        );
    }
}
