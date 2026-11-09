//! Actual rendered client, production handlers and local model; fictional only.
use super::*;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
use waypoint_applications::driver::BrowserDriver;

struct UiChild(Child);
impl Drop for UiChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
#[ignore = "explicit memory-contained qualifier, installed local model and two headless fictional browsers"]
fn resource_gated_rendered_history_fill_and_visual_review() {
    assert_eq!(
        std::env::var("WAYPOINT_HISTORY_UI_CONTAINED").as_deref(),
        Ok("1"),
        "Use tools/qualify_history_ui.py for process-tree containment"
    );
    let node = PathBuf::from(std::env::var_os("WAYPOINT_HISTORY_UI_NODE").expect("explicit Node"));
    let engine =
        PathBuf::from(std::env::var_os("WAYPOINT_HISTORY_UI_BROWSER").expect("explicit browser"));
    assert!(node.is_absolute() && node.is_file() && engine.is_absolute() && engine.is_file());
    let model = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL")
        .unwrap_or_else(|_| "gemma4:12b-it-q4_K_M".into());
    crate::experience_inference::tests::with_resource_gated_named_model(&model, || {
        let root = tempfile::Builder::new()
            .prefix("history-rendered-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap()
            .keep();
        println!("Rendered history evidence: {}", root.display());
        let layout =
            std::env::var("WAYPOINT_HISTORY_PIPELINE_LAYOUT").unwrap_or_else(|_| "repeated".into());
        let name = match layout.as_str() {
            "repeated" => "tests/form-career-repeated.html",
            "flat" => "tests/form-career-flat.html",
            _ => panic!("Choose a supported owned fictional fixture"),
        };
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(name)
            .canonicalize()
            .unwrap();
        let source = std::fs::read_to_string(&fixture).unwrap();
        assert!(!source.contains("<script") && !source.contains("type=\"submit\""));
        let url = url::Url::from_file_path(&fixture).unwrap().to_string();
        let baseline = std::env::var("WAYPOINT_HISTORY_UI_PROFILE").as_deref() == Ok("baseline");
        let profile = if baseline {
            json!({"first_name":"Avery","email":"avery@example.invalid","career_history":json!({"version":1,"employment":[
            {"id":"e".repeat(32),"employer":"Fictional Early Works","title":"Technician","start_date":"2011-01","end_date":"2015","reviewed":true},
            {"id":"a".repeat(32),"employer":"Fictional Engineering","title":"Systems Engineer","start_date":"2020-06","current":true,"reviewed":true},
            {"id":"b".repeat(32),"employer":"Fictional Prior Works","title":"Design Engineer","start_date":"2016-02","end_date":"2019-10","reviewed":true},
            {"id":"d".repeat(32),"employer":"Unchecked Fictional","title":"Unverified","current":true,"reviewed":false}
        ],"education":[{"id":"c".repeat(32),"institution":"Fictional Institute","qualification":"BSc Engineering","reviewed":true}]}).to_string()})
        } else {
            json!({"first_name":"Jordan","email":"jordan@example.invalid","career_history":json!({"version":1,"employment":[
            {"id":"a".repeat(32),"employer":"Fictional North Studio","title":"Process Analyst","start_date":"2019-03","current":true,"reviewed":true},
            {"id":"b".repeat(32),"employer":"Fictional Prior Studio","title":"Systems Specialist","start_date":"2014-08","end_date":"2018-06","reviewed":true},
            {"id":"d".repeat(32),"employer":"Unchecked Fictional Studio","title":"Unverified","current":true,"reviewed":false}
        ],"education":[{"id":"c".repeat(32),"institution":"Fictional Research College","qualification":"Applied Physics Diploma","reviewed":true}]}).to_string()})
        };
        let mut ws = Workspace::open(&root.join("workspace")).unwrap();
        ws.save_candidate_profile(&profile.to_string()).unwrap();
        let job_id = JobIdentityId::new("fictional-rendered-history").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Fictional systems role".into(),
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
            Some(job_id.as_str().into())
        );
        require_duplicate_index_ready(&ws).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        assert!(live.walls.is_empty());
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", &model)
            .unwrap()
            .with_resident_context();
        let mut llm = crate::resident_inference::ResidentInference::new(
            llm,
            model.clone(),
            crate::resident_model_identity,
        );
        let preview = form_preview::suggest_for_live_form(live, &mut llm, &profile).unwrap();
        assert_eq!(preview.suggestions.len(), 2);
        assert!(preview.suggestions.iter().all(|item| matches!(
            item.field_selector.as_str(),
            "#candidate-name" | "#candidate-email"
        )));
        let image = driver.screenshot_base64().unwrap();
        let session_id = "fictional-rendered-history-session";
        let pending =
            form_task::begin_pending(&ws, &job_id, "fictional-rendered-inspection", now()).unwrap();
        let mut record =
            attempt_record::AttemptRecord::start(&ws, &job_id, "fictional-rendered-attempt", now())
                .unwrap()
                .unwrap();
        attempt_record::capture_inspection(
            &mut driver,
            &mut record,
            &preview.schema.origin,
            &preview.manual_review,
            &preview.fields_for_review(),
            Some(&image),
            now(),
        )
        .unwrap();
        let seed = json!({"status":"complete","fields":preview.fields_for_review(),"model_review_selectors":preview.model_review_selectors,"repeated_label_selectors":preview.repeated_label_selectors(),"suggestions":preview.suggestions,"manual_review":preview.manual_review,"session_id":session_id,"origin":preview.schema.origin,"expires_in_seconds":600,"visible":false,"source_route":false,"fill_attempted":false,"screenshot":image,"links":[],"other_links":[],"other_links_available":false});
        let mut session = Some(ReviewSession {
            id: session_id.into(),
            job_id: job_id.clone(),
            inspection_task_id: "fictional-rendered-inspection".into(),
            inspection_pending_revision: pending,
            experience_revisions: vec![],
            url,
            route_binding: None,
            profile_digest: Sha256Digest::of(&profile.to_string()),
            preview,
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
            record: Some(record),
        });
        let expected = if baseline {
            json!({"#candidate-name":"Avery","#candidate-email":"avery@example.invalid","#work-company":"Fictional Engineering","#work-title":"Systems Engineer","#work-month":"2020-06","#past-company":"Fictional Prior Works","#past-title":"Design Engineer","#past-month":"2016-02","#school-name":"Fictional Institute","#qualification":"BSc Engineering"})
        } else {
            json!({"#candidate-name":"Jordan","#candidate-email":"jordan@example.invalid","#work-company":"Fictional North Studio","#work-title":"Process Analyst","#work-month":"2019-03","#past-company":"Fictional Prior Studio","#past-title":"Systems Specialist","#past-month":"2014-08","#school-name":"Fictional Research College","#qualification":"Applied Physics Diploma"})
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = "7".repeat(64);
        let native_browser = session.as_mut().unwrap().driver.browser_version().unwrap();
        let config = root.join("client.private.json");
        std::fs::write(&config,json!({"url":format!("http://127.0.0.1:{port}/#{token}"),"preview":seed,"job_id":job_id.as_str(),"browser":engine,"output":root,"expected":expected,"sources":{"main":waypoint_domain::sha256_hex(include_str!("main.rs").as_bytes()),"test":waypoint_domain::sha256_hex(include_str!("history_ui_qualification.rs").as_bytes()),"fixture":waypoint_domain::sha256_hex(source.as_bytes()),"app_js":waypoint_domain::sha256_hex(include_str!("../web/app.js").as_bytes()),"history_mapping":waypoint_domain::sha256_hex(include_str!("../web/history-mapping.js").as_bytes()),"theme":waypoint_domain::sha256_hex(include_str!("../web/workspace-theme.css").as_bytes()),"model_start":waypoint_domain::sha256_hex(include_str!("model_start.rs").as_bytes()),"history_batch":waypoint_domain::sha256_hex(include_str!("history_batch.rs").as_bytes()),"answer_source":waypoint_domain::sha256_hex(include_str!("history_answer_source.rs").as_bytes()),"browser":waypoint_domain::sha256_hex(include_str!("../../../crates/waypoint_browser/src/lib.rs").as_bytes()),"lock":waypoint_domain::sha256_hex(include_str!("../../../Cargo.lock").as_bytes()),"client":waypoint_domain::sha256_hex(include_str!("../scripts/qualify-history-ui.cjs").as_bytes())},"model":model,"native_browser":native_browser}).to_string()).unwrap();
        use std::os::windows::process::CommandExt;
        let log = std::fs::File::create(root.join("client.private.log")).unwrap();
        let mut child = UiChild(
            Command::new(node)
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/qualify-history-ui.cjs"))
                .arg("--config")
                .arg(&config)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut denied = Vec::new();
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "bounded rendered qualification; inspect client log"
            );
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut peek = [0; 2048];
                    let bytes = stream.peek(&mut peek).unwrap_or(0);
                    if bytes == 0 {
                        continue;
                    }
                    let first = String::from_utf8_lossy(&peek[..bytes]);
                    let mut request = first.split_whitespace();
                    let method = request.next().unwrap_or("");
                    let path = request.next().unwrap_or("");
                    if method != "GET"
                        && (!allowed_mutation(path)
                            || (path == "/api/web-search/selection"
                                && !selection_load_only(&first)))
                    {
                        denied.push(path.to_owned());
                        json_reply(
                            &mut stream,
                            "403 Forbidden",
                            json!({"error":"Mutation outside fictional qualification scope"}),
                        );
                        continue;
                    }
                    if let Err(error) = serve(&mut stream, &mut ws, &mut session, &token, port) {
                        json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("Qualification listener failed: {error}"),
            }
        };
        let mut values = serde_json::Map::new();
        let current = session.as_mut().expect("review stays available");
        for selector in expected.as_object().unwrap().keys() {
            let value = current
                .driver
                .text_value_snapshot(selector)
                .map(|snapshot| snapshot.value)
                .map_err(|error| error.to_string());
            values.insert(selector.clone(), serde_json::to_value(value).unwrap());
        }
        std::fs::write(root.join("server.private.json"),json!({"client_exit_code":status.code(),"expected":expected,"observed":values,"denied":denied,"submitted":false}).to_string()).unwrap();
        assert!(
            status.success(),
            "Rendered client failed; inspect {}",
            root.join("client.private.log").display()
        );
        for (selector, value) in expected.as_object().unwrap() {
            current
                .driver
                .verify_text_value(selector, value.as_str().unwrap())
                .unwrap();
        }
        for selector in [
            "#referee-name",
            "#referee-company",
            "#office-address",
            "#salary",
        ] {
            current.driver.verify_text_value(selector, "").unwrap();
        }
        assert!(current
            .driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
        assert!(
            denied.is_empty(),
            "Unexpected mutation was refused: {denied:?}"
        );
        assert!(
            status.success(),
            "Rendered client failed; inspect {}",
            root.join("client.private.log").display()
        );
    });
}
fn allowed_mutation(path: &str) -> bool {
    matches!(
        path,
        "/api/form/history-advice-start"
            | "/api/form/history-advice-status"
            | "/api/form/history-advice-pending"
            | "/api/form/history-advice-cancel"
            | "/api/form/fill"
            | "/api/form/visual-focus"
            | "/api/form/visual-review-start"
            | "/api/form/visual-review-status"
            | "/api/form/session-status"
            | "/api/form/recapture-pending"
            | "/api/letter/export-pending"
            | "/api/search-plan/pending"
            | "/api/discover/pending"
            | "/api/application-check/pending"
            | "/api/gmail/status"
            | "/api/storage/status"
            | "/api/documents/convert-pdf-pending"
            | "/api/web-search/selection"
            | "/api/web-search/pending"
            | "/api/web-search/batch-pending"
            | "/api/listing-inspect/pending"
            | "/api/documents/edit-template-pending"
            | "/api/draft/pending"
            | "/api/fit/batch"
    )
}
fn selection_load_only(request: &str) -> bool {
    request
        .split_once("\r\n\r\n")
        .and_then(|(_, body)| serde_json::from_str::<Value>(body).ok())
        .is_some_and(|body| body["action"] == "load")
}
#[test]
fn rendered_qualification_never_allows_application_or_email_mutations() {
    for path in [
        "/api/form/submit",
        "/api/email/send",
        "/api/email/test",
        "/api/profile",
        "/api/form/rescan-start",
        "/api/form/custom-choice-select",
    ] {
        assert!(!allowed_mutation(path));
    }
    assert!(allowed_mutation("/api/form/fill"));
    assert!(selection_load_only(
        "POST /api/web-search/selection HTTP/1.1\r\n\r\n{\"action\":\"load\"}"
    ));
    assert!(!selection_load_only(
        "POST /api/web-search/selection HTTP/1.1\r\n\r\n{\"action\":\"save\"}"
    ));
}
