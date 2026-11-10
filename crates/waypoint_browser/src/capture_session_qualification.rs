//! Explicit fictional coordinator -> archive -> actual contained browser check.
use super::*;
use crate::{rehearsal::RehearsalConfig, CdpDriver};
use std::{io::Cursor, sync::Mutex};
use waypoint_applications::{
    driver::BrowserDriver,
    form_mapping::{BrowserCommand, PacketValue},
};

struct RecordedHttp {
    responses: BTreeMap<String, Vec<u8>>,
    requests: Arc<Mutex<Vec<String>>>,
    dropped: Arc<AtomicBool>,
}
impl Transport for RecordedHttp {
    fn get(&mut self, url: &str, _: u64, max: usize) -> Result<CapturedResponse, ()> {
        self.requests.lock().unwrap().push(url.into());
        let bytes = self.responses.remove(url).ok_or(())?;
        waypoint_net::read_asset_response(&mut Cursor::new(bytes), "GET", max).map_err(|_| ())
    }
}
impl Drop for RecordedHttp {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

#[test]
#[ignore = "explicit private engine, synthetic HTTP resources and owned memory containment"]
fn coordinator_archive_drives_contained_form() {
    run_coordinator(false, false);
}
#[test]
#[ignore = "explicit private engine, automatic fictional resource discovery and owned memory containment"]
fn discovery_archives_transitive_resources_before_candidate_entry() {
    run_coordinator(true, false);
}
#[test]
#[ignore = "explicit private engine, fictional private-URL discovery and owned memory containment"]
fn private_resources_are_not_fetched_before_offline_candidate_entry() {
    run_coordinator(true, true);
}
fn run_coordinator(discovery: bool, privacy: bool) {
    let target =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let path = std::fs::canonicalize(
        std::env::var_os("WAYPOINT_REHEARSAL_CONFIG").expect("explicit config"),
    )
    .unwrap();
    assert!(path.starts_with(target));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_REHEARSAL_CONFIG_SHA256").unwrap()
    );
    let mut config: RehearsalConfig = serde_json::from_slice(&bytes).unwrap();
    let folder = path.parent().unwrap();
    let html="<!doctype html><meta charset=utf-8><title>Fictional coordinated form</title><link rel=stylesheet href=/style.css><h1>Fictional coordinated form</h1><fieldset><legend>Your details</legend><label>Full name<input id=name required value=source-default-must-not-be-recorded></label></fieldset><button type=button id=next>Continue</button><section id=questions></section><script src=/app.js></script>";
    let script = if discovery {
        "fetch('/questions').then(r=>r.json()).then(v=>{const label=document.createElement('label');label.textContent=v.label;const input=document.createElement('input');input.id='email';input.type='email';label.append(input);const section=document.getElementById('questions');section.append(label);section.hidden=true;document.getElementById('next').onclick=()=>section.hidden=false;});"
    } else {
        "document.getElementById('next').onclick=()=>fetch('/questions').then(r=>r.json()).then(v=>{const label=document.createElement('label');label.textContent=v.label;const input=document.createElement('input');input.id='email';input.type='email';label.append(input);document.getElementById('questions').append(label);});"
    };
    let script = if privacy {
        format!("{script} fetch('/bootstrap?next=HTTPS%3A%2F%2Ffixture.invalid%2F%3Faccess_token%3Dfictional-private-marker').catch(()=>{{}}); fetch('/who?email=fictional%40fixture.invalid').catch(()=>{{}}); fetch('/account-link').catch(()=>{{}});")
    } else {
        script.to_owned()
    };
    let style = if discovery {
        "@import url('/nested.css'); body{font:18px Arial;margin:30px}"
    } else {
        "body{font:18px Arial;margin:30px}label{display:block;margin:16px 0}input{margin-left:16px}"
    };
    let mut responses = BTreeMap::new();
    for (url, body, mime, extra) in [
        (
            "https://fixture.invalid/robots.txt",
            "User-agent: *\nAllow: /",
            "text/plain",
            "",
        ),
        (
            "https://fixture.invalid/form",
            "redirect",
            "text/plain",
            "Location: /flow\r\n",
        ),
        (
            "https://fixture.invalid/flow",
            html,
            "text/html; charset=utf-8",
            "Set-Cookie: fictional=discarded\r\n",
        ),
        (
            "https://fixture.invalid/app.js",
            script.as_str(),
            "text/javascript",
            "",
        ),
        ("https://fixture.invalid/style.css", style, "text/css", ""),
        (
            "https://fixture.invalid/nested.css",
            "label{display:block;margin:16px 0}input{margin-left:16px}",
            "text/css",
            "",
        ),
        (
            "https://fixture.invalid/questions",
            "{\"label\":\"Email\"}",
            "application/json",
            "",
        ),
    ] {
        let status = if url.ends_with("/form") { 302 } else { 200 };
        let raw=format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{extra}\r\n{body}",body.len());
        responses.insert(url.to_owned(), raw.into_bytes());
    }
    if privacy {
        responses.insert("https://fixture.invalid/account-link".into(),
            b"HTTP/1.1 302 Fixture\r\nContent-Type: text/plain\r\nContent-Length: 0\r\nLocation: /private?session_id=fictional-private-marker\r\n\r\n".to_vec());
    }
    let requests = Arc::new(Mutex::new(vec![]));
    let dropped = Arc::new(AtomicBool::new(false));
    let mut capture = CaptureSession::with_transport(
        &folder.join("http-captured"),
        "https://fixture.invalid/form#application",
        vec![CaptureOrigin {
            origin: "https://fixture.invalid".into(),
            source: "user_supplied_url".into(),
        }],
        PermissionRegistry::from_json(waypoint_policy::INITIAL_REGISTRY_JSON).unwrap(),
        CaptureBudget::default(),
        Arc::new(AtomicBool::new(false)),
        Box::new(RecordedHttp {
            responses,
            requests: requests.clone(),
            dropped: dropped.clone(),
        }),
    )
    .unwrap();
    assert!(matches!(
        capture.capture_entry(),
        CaptureOutcome::Recorded { status: 200, .. }
    ));
    let python = PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_PYTHON").unwrap());
    if discovery {
        let summary = capture
            .discover_resources(&python, config.clone(), 4, 1500)
            .unwrap();
        assert_eq!(
            summary.stopped,
            if privacy {
                crate::capture_discovery::DiscoveryStop::ReviewRequired
            } else {
                crate::capture_discovery::DiscoveryStop::NoNewRequests
            }
        );
        assert_eq!(summary.rounds.len(), 3);
        assert_eq!(
            summary
                .rounds
                .iter()
                .map(|r| r.newly_recorded)
                .sum::<usize>(),
            4
        );
    } else {
        for url in [
            "https://fixture.invalid/app.js",
            "https://fixture.invalid/style.css",
            "https://fixture.invalid/questions",
        ] {
            assert!(matches!(
                capture.capture(url),
                CaptureOutcome::Recorded { .. }
            ));
        }
    }
    if discovery {
        let report = capture.report();
        let summary = report.discovery.as_ref().unwrap();
        let outline = summary
            .form_outline
            .as_ref()
            .expect("Public control inventory missing");
        assert!(outline.valid());
        let name = outline
            .controls
            .iter()
            .find(|c| c.label.as_deref() == Some("Full name"))
            .expect("Label not observed");
        assert_eq!(name.required, Some(true));
        assert!(name.context.iter().any(|label| label == "Your details"));
        assert!(outline
            .controls
            .iter()
            .any(|c| c.role == "button" && c.label.as_deref() == Some("Continue")));
        assert!(!serde_json::to_string(outline)
            .unwrap()
            .contains("source-default-must-not-be-recorded"));
        assert_eq!(summary.form_outline_round, Some(3));
        assert_eq!(
            summary.form_outline_snapshot_sha256.as_ref().unwrap().len(),
            64
        );
    }
    let frozen = capture.freeze().unwrap();
    assert!(dropped.load(Ordering::Acquire));
    let expected_requests = if privacy {
        8
    } else if discovery {
        7
    } else {
        6
    };
    assert_eq!(requests.lock().unwrap().len(), expected_requests);
    assert_eq!(
        frozen.report.recorded_resources,
        if discovery { 6 } else { 5 }
    );
    assert_eq!(frozen.report.robots.len(), 1);
    if privacy {
        assert!(requests
            .lock()
            .unwrap()
            .iter()
            .all(|url| !url.contains("fictional-private-marker") && !url.contains("/who?")));
        assert_eq!(
            frozen
                .report
                .records
                .iter()
                .filter(|r| r.outcome
                    == CaptureOutcome::NeedsReview {
                        reason: CaptureGap::PrivateUrl
                    })
                .count(),
            3
        );
        assert!(!serde_json::to_string(&frozen.report)
            .unwrap()
            .contains("fictional-private-marker"));
    }
    config.schema = 2;
    config.content_root = frozen.snapshot.root;
    config.content_files = frozen.snapshot.files;
    config.entry = frozen.snapshot.entry;
    config.replay_manifest = Some(frozen.snapshot.manifest);
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    assert_eq!(ready.entry_url, "https://fixture.invalid/flow#application");
    driver.open(&ready.entry_url).unwrap();
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#name".into(),
            value: PacketValue::Text("Fictional Candidate — 資格".into()),
        })
        .unwrap();
    driver
        .verify_text_value("#name", "Fictional Candidate — 資格")
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#next".into(),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while driver.text_value_snapshot("#email").is_err() {
        assert!(
            Instant::now() < deadline,
            "Recorded next section did not appear"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#email".into(),
            value: PacketValue::Text("fictional@example.invalid".into()),
        })
        .unwrap();
    driver
        .verify_text_value("#email", "fictional@example.invalid")
        .unwrap();
    let replay = driver.rehearsal_replay_report().unwrap();
    assert_eq!(replay["served"], if discovery { 5 } else { 4 });
    assert_eq!(replay["cached_non_read_replies"], 0);
    assert_eq!(
        requests.lock().unwrap().len(),
        expected_requests,
        "Collection transport was used after freeze"
    );
    std::fs::write(
        folder.join("http-captured-screenshot.base64"),
        driver.screenshot_base64().unwrap(),
    )
    .unwrap();
    assert!(driver.commit().is_err());
    driver.execute(&BrowserCommand::Close).unwrap();
    std::fs::write(folder.join("http-captured.private.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "complete":true,"scope":"fictional transport responses through policy/robots coordinator, frozen archive and contained browser; no public form capture",
        "capture":frozen.report,"report":replay,"privacy_guard_exercised":privacy,"transport_dropped_before_entry":true})).unwrap()).unwrap();
}
