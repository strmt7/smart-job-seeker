//! Production fill endpoint and plaintext attempt snapshots, fictional only.
use super::*;
use waypoint_applications::driver::BrowserDriver;

fn call(
    ws: &mut Workspace,
    session: &mut Option<ReviewSession>,
    body: &Value,
) -> (Result<(), String>, Value) {
    call_path(ws, session, body, "/api/form/fill")
}
pub(crate) fn call_path(
    ws: &mut Workspace,
    session: &mut Option<ReviewSession>,
    body: &Value,
    path: &str,
) -> (Result<(), String>, Value) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let raw = body.to_string();
    write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fictional-lineage-token\r\nContent-Length: {}\r\n\r\n{raw}",raw.len()).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    let result = serve(&mut server, ws, session, "fictional-lineage-token", port);
    if let Err(error) = &result {
        json_reply(&mut server, "400 Bad Request", json!({"error":error}));
    }
    drop(server);
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    let body = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    (result, body)
}

#[test]
#[ignore = "one headless fictional browser, production fill endpoint, source guards and disk archive; no model or external site"]
fn history_sources_survive_reviewed_fill_and_archive_without_stale_reuse() {
    let root = tempfile::Builder::new()
        .prefix("history-answer-sources-")
        .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .unwrap()
        .keep();
    let mut ws = Workspace::open(&root).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/form-history-mapping.html")
        .canonicalize()
        .unwrap();
    let url = url::Url::from_file_path(fixture).unwrap().to_string();
    let job = JobIdentityId::new("fictional-history-answer-sources").unwrap();
    ws.import_job(waypoint_store::StoredJob {
        id: job.clone(),
        title: "Fictional role".into(),
        employer: "Fictional Destination".into(),
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
    assert_eq!(
        ws.index_next_duplicate_job("").unwrap(),
        Some(job.as_str().into())
    );
    require_duplicate_index_ready(&ws).unwrap();
    let raw=json!({"version":1,"employment":[{"id":"a".repeat(32),"employer":"Fictional Source Co","title":"Engineer","start_date":"2020-06","reviewed":true},{"id":"b".repeat(32),"employer":"Unrelated Private Fictional","title":"Other","reviewed":true}],"education":[]}).to_string();
    let profile = json!({"career_history":raw});
    ws.save_candidate_profile(&profile.to_string()).unwrap();
    let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&url).unwrap();
    let pending = form_task::begin_pending(&ws, &job, "fictional-history-inspection", 1).unwrap();
    let mut session = Some(ReviewSession {
        id: "fictional-history-answer-session".into(),
        job_id: job.clone(),
        inspection_task_id: "fictional-history-inspection".into(),
        inspection_pending_revision: pending,
        experience_revisions: vec![],
        url,
        route_binding: None,
        profile_digest: Sha256Digest::of(&profile.to_string()),
        preview: form_preview::FormPreview {
            schema: live.schema,
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        },
        driver,
        created: Instant::now(),
        ttl: Duration::from_secs(120),
        filled: false,
        fill_attempted: false,
        completed_selectors: vec![],
        prior_sections: vec![],
        prior_sections_omitted: false,
        selected_choices: vec![],
        answer_drafts: Default::default(),
        visible: false,
        staged_files: vec![],
        attachments: vec![],
        pending_choice_visual: None,
        last_visual_freshness_key: None,
        record: None,
    });
    let selectors = vec!["#organization".into(), "#month".into()];
    let (batch_input, batch_digest) =
        history_inputs(&ws, session.as_ref().unwrap(), "all", &selectors).unwrap();
    assert!(
        matches!(batch_input.source,history_advice::HistorySource::AllFields(ref records) if records.len()==2)
    );
    assert!(history_inputs(
        &ws,
        session.as_ref().unwrap(),
        "all",
        &["#organization".into(), "#organization".into()]
    )
    .is_err());
    history_advice::TASK.set_completed_fixture("fictional-batch-binding",json!({"session_id":"fictional-history-answer-session","entry_id":"all","selectors":selectors,"digest":batch_digest}),json!({"mode":"form","proposals":[],"manual_review":[]}));
    assert_eq!(
        history_advice_status(&ws, session.as_ref(), "fictional-batch-binding").unwrap()["status"],
        "complete"
    );
    ws.save_candidate_profile(&json!({"career_history":""}).to_string())
        .unwrap();
    assert_eq!(
        history_advice_status(&ws, session.as_ref(), "fictional-batch-binding").unwrap()["status"],
        "failed"
    );
    ws.save_candidate_profile(&profile.to_string()).unwrap();
    assert_eq!(
        history_advice_status(&ws, session.as_ref(), "fictional-batch-binding").unwrap()["status"],
        "failed",
        "reverting profile data must not restore stale advice"
    );
    let record = candidate_history::selected(&raw, &"a".repeat(32)).unwrap();
    let answer = |selector: &str, component: &str| json!({"selector":selector,"value":record.values[component],"history_source":{"entry_id":record.id,"component":component,"source_sha256":history_answer_source::source_digest(&record,component).unwrap().as_str()}});
    let good = json!({"session_id":"fictional-history-answer-session","reviewed":true,"selectors":[],"answers":[answer("#organization","employer"),answer("#month","start_year_month")]});
    let (result, resolved) = call_path(
        &mut ws,
        &mut session,
        &json!({"session_id":"fictional-history-answer-session","entry_id":record.id,"selector":"#organization","component":"employer"}),
        "/api/form/history-value",
    );
    result.unwrap();
    assert_eq!(resolved["value"]["text"], "Fictional Source Co");
    assert_eq!(
        resolved["history_source"],
        good["answers"][0]["history_source"]
    );
    let (result, _) = call_path(
        &mut ws,
        &mut session,
        &json!({"session_id":"fictional-history-answer-session","entry_id":record.id,"selector":"#referee","component":"employer"}),
        "/api/form/history-value",
    );
    assert!(result.is_err());
    let mut forged = good.clone();
    forged["answers"][1]["history_source"]["source_sha256"] = json!("0".repeat(64));
    let (result, _) = call(&mut ws, &mut session, &forged);
    assert!(result.unwrap_err().contains("source changed"));
    let mut edited = good.clone();
    edited["answers"][0]["value"] = json!("Edited organization");
    let (result, _) = call(&mut ws, &mut session, &edited);
    assert!(result.unwrap_err().contains("answer was edited"));
    ws.save_candidate_profile(&json!({"career_history":raw,"first_name":"Corrected"}).to_string())
        .unwrap();
    let (result, _) = call(&mut ws, &mut session, &good);
    assert!(result.unwrap_err().contains("Profile changed"));
    ws.save_candidate_profile(&profile.to_string()).unwrap();
    let current = session.as_mut().unwrap();
    assert!(!current.fill_attempted);
    assert!(current.record.is_none());
    current
        .driver
        .verify_text_value("#organization", "")
        .unwrap();
    current.driver.verify_text_value("#month", "").unwrap();
    let (result, response) = call(&mut ws, &mut session, &good);
    result.unwrap();
    assert_eq!(response["filled_fields"].as_array().unwrap().len(), 2);
    let current = session.as_mut().unwrap();
    assert!(current.filled);
    current
        .driver
        .verify_text_value("#organization", "Fictional Source Co")
        .unwrap();
    current
        .driver
        .verify_text_value("#month", "2020-06")
        .unwrap();
    assert!(current
        .driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
    let attempts = attempt_record::list(&ws, &job).unwrap();
    let attempt = attempts["attempts"][0]["attempt_id"].as_str().unwrap();
    let history = attempt_record::answer_history(&ws, &job, attempt, 0).unwrap();
    let requested = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["data"]["type"] == "fields_requested")
        .unwrap();
    let fields = requested["data"]["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 2);
    for field in fields {
        assert_eq!(field["source_career_history"]["entry_id"], record.id);
        assert_eq!(field["source_career_history"]["checked"], true);
        assert_eq!(
            field["source_career_history"]["profile_sha256"],
            Sha256Digest::of(&profile.to_string()).as_str()
        );
    }
    assert!(!requested
        .to_string()
        .contains("Unrelated Private Fictional"));
    let folder = std::path::Path::new(attempts["attempts"][0]["folder"].as_str().unwrap());
    let saved_file = folder.join(requested["file"].as_str().unwrap());
    let before = std::fs::read(&saved_file).unwrap();
    assert_eq!(
        fields[0]["source_career_history"]["current_entry_status"],
        "entry_unchanged"
    );
    ws.save_candidate_profile(&json!({"career_history":""}).to_string())
        .unwrap();
    let after = attempt_record::answer_history(&ws, &job, attempt, 0).unwrap();
    let preserved = after["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["data"]["type"] == "fields_requested")
        .unwrap();
    assert_eq!(
        preserved["data"]["fields"][0]["source_career_history"]["current_entry_status"],
        "entry_changed_or_unchecked"
    );
    assert_eq!(before, std::fs::read(saved_file).unwrap());
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["data"]["type"] == "fill_commands_verified"));
    let folder = std::path::Path::new(attempts["attempts"][0]["folder"].as_str().unwrap());
    assert!(std::fs::read_dir(folder.join("screenshots"))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|s| s == "png")));
    std::fs::write(root.join("qualification.private.json"),json!({"case":"fictional_history_source_endpoint","sources":{"main":waypoint_domain::sha256_hex(include_str!("main.rs").as_bytes()),"validator":waypoint_domain::sha256_hex(include_str!("history_answer_source.rs").as_bytes()),"recorder":waypoint_domain::sha256_hex(include_str!("attempt_record.rs").as_bytes()),"qualification":waypoint_domain::sha256_hex(include_str!("history_answer_source_browser_tests.rs").as_bytes())},"filled":2,"history_before_correction":history,"history_after_correction":after,"archive_unchanged":true,"no_submit_command":true}).to_string()).unwrap();
    println!("History source endpoint evidence: {}", root.display());
}
