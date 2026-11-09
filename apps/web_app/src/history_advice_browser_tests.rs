//! Fictional browser/session binding checks; no model or external form.
use super::*;
use waypoint_applications::driver::BrowserDriver;
#[test]
#[ignore = "one headless fictional browser; verifies history advice session binding and stale-result refusal"]
fn history_advice_binding_rejects_changed_information_and_late_results() {
    let mut ws = Workspace::in_memory().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/form-history-mapping.html")
        .canonicalize()
        .unwrap();
    let url = url::Url::from_file_path(fixture).unwrap().to_string();
    let id = JobIdentityId::new("fictional-history-binding").unwrap();
    ws.import_job(waypoint_store::StoredJob {
        id: id.clone(),
        title: "Fictional role".into(),
        employer: "Fictional Co".into(),
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
    let entry = "a".repeat(32);
    let raw=json!({"version":1,"employment":[{"id":entry,"employer":"Fictional Co","title":"Engineer","reviewed":true}],"education":[]}).to_string();
    let profile = json!({"career_history":raw});
    ws.save_candidate_profile(&profile.to_string()).unwrap();
    let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&url).unwrap();
    let mut session = ReviewSession {
        id: "fictional-history-session".into(),
        job_id: id,
        inspection_task_id: "fixture".into(),
        inspection_pending_revision: 0,
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
        ttl: Duration::from_secs(600),
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
    };
    let selectors = vec!["#organization".into()];
    let (_, digest) = history_inputs(&ws, &session, &entry, &selectors).unwrap();
    let binding =
        json!({"session_id":session.id,"entry_id":entry,"selectors":selectors,"digest":digest});
    assert!(history_inputs(&ws, &session, &entry, &["#missing".into()]).is_err());
    assert!(history_inputs(
        &ws,
        &session,
        &entry,
        &["#organization".into(), "#organization".into()]
    )
    .is_err());
    session
        .preview
        .model_review_selectors
        .insert("#organization".into());
    assert!(history_inputs(&ws, &session, &entry, &selectors).is_err());
    session.preview.model_review_selectors.clear();
    history_advice::TASK.set_completed_fixture(
        "binding-case",
        binding.clone(),
        json!({"proposals":[{"value":"fictional previous answer"}]}),
    );
    assert_eq!(
        history_advice_status(&ws, Some(&session), "binding-case").unwrap()["status"],
        "complete"
    );
    ws.save_candidate_profile(
        &json!({"career_history":profile["career_history"],"first_name":"Changed"}).to_string(),
    )
    .unwrap();
    let stale = history_advice_status(&ws, Some(&session), "binding-case").unwrap();
    assert_eq!(stale["status"], "failed");
    assert!(stale.get("advice").is_none());
    ws.save_candidate_profile(&profile.to_string()).unwrap();
    assert_eq!(
        history_advice_status(&ws, Some(&session), "binding-case").unwrap()["status"],
        "failed"
    );
    history_advice::TASK.set_completed_fixture(
        "late-case",
        binding.clone(),
        json!({"proposals":[]}),
    );
    session.filled = true;
    assert_eq!(
        history_advice_status(&ws, Some(&session), "late-case").unwrap()["status"],
        "failed"
    );
    history_advice::TASK.set_completed_fixture("closed-case", binding, json!({"proposals":[]}));
    assert_eq!(
        history_advice_status(&ws, None, "closed-case").unwrap()["status"],
        "failed"
    );
    session
        .driver
        .verify_text_value("#organization", "")
        .unwrap();
    assert!(session
        .driver
        .trace
        .iter()
        .all(|event| !event.contains("typed") && !event.contains("submitted")));
}
