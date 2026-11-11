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
    fn get(
        &mut self,
        url: &str,
        _: u64,
        max: usize,
        origin: Option<&waypoint_net::PublicRequestOrigin>,
    ) -> Result<CapturedResponse, ()> {
        assert!(origin.is_none(), "Fixture has no CORS context");
        self.requests.lock().unwrap().push(url.into());
        let bytes = self.responses.remove(url).ok_or(())?;
        let mut response = waypoint_net::read_asset_response(&mut Cursor::new(bytes), "GET", max)
            .map_err(|_| ())?;
        // This fictional HTTP transport models a complete anonymous GET. The
        // style resource type/Origin absence is separately observed by Chromium.
        response.request_header_names =
            Some(vec!["host".into(), "accept".into(), "user-agent".into()]);
        Ok(response)
    }
}
impl Drop for RecordedHttp {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

#[test]
#[ignore = "explicit contained headless engine and fictional application-frame responses"]
fn discovery_archives_observed_public_ats_frame() {
    let (mut config, folder, python) = qualification_config();
    let entry = "https://fixture.invalid/listing";
    let frame = "https://jobs.lever.co/fictional-company/fictional-role/apply";
    let unrelated = "https://jobs.ashbyhq.com/fictional-company/pixel";
    let document = format!(
        "<!doctype html><link rel=icon href=data:,><h1>Fictional listing</h1><iframe title=Application src={frame}></iframe><img src={unrelated}>"
    );
    let responses = [
        ("https://fixture.invalid/robots.txt", "text/plain", "User-agent: *\nAllow: /"),
        ("https://jobs.lever.co/robots.txt", "text/plain", "User-agent: *\nAllow: /"),
        (entry, "text/html", document.as_str()),
        (frame, "text/html", "<!doctype html><link rel=icon href=data:,><label>Field experience<input required></label>"),
    ]
    .into_iter()
    .map(|(url, mime, body)| (
        url.into(),
        format!("HTTP/1.1 200 Fixture\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes(),
    ))
    .collect();
    let calls = Arc::new(Mutex::new(vec![]));
    let dropped = Arc::new(AtomicBool::new(false));
    let mut capture = CaptureSession::with_transport(
        &folder.join("embedded-capture"),
        entry,
        vec![CaptureOrigin {
            origin: "https://fixture.invalid".into(),
            source: "user_supplied_url".into(),
        }],
        PermissionRegistry::from_json(waypoint_policy::INITIAL_REGISTRY_JSON).unwrap(),
        CaptureBudget::default(),
        Arc::new(AtomicBool::new(false)),
        Box::new(RecordedHttp {
            responses,
            requests: calls.clone(),
            dropped: dropped.clone(),
        }),
    )
    .unwrap();
    let discovery = capture
        .discover_resources(&python, config.clone(), 4, 1500)
        .unwrap();
    let archive = capture.freeze().unwrap();
    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(discovery.rounds[0].newly_recorded, 1);
    assert_eq!(archive.report.embedded_origins.len(), 1);
    let origin = &archive.report.embedded_origins[0];
    assert_eq!(origin.source, "lever");
    assert_eq!(
        origin.observed_url_sha256,
        waypoint_domain::sha256_hex(frame.as_bytes())
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "https://fixture.invalid/robots.txt",
            entry,
            "https://jobs.lever.co/robots.txt",
            frame
        ]
    );
    config.schema = 2;
    config.content_root = archive.snapshot.root;
    config.content_files = archive.snapshot.files;
    config.entry = archive.snapshot.entry;
    config.replay_manifest = Some(archive.snapshot.manifest);
    let (mut browser, _) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    let observed = (|| {
        browser.open(entry)?;
        let frames = browser.captured_frames()?;
        if let [frame] = frames.frames.as_slice() {
            let child = browser.enter_captured_frame(frame)?;
            return Ok(child
                .schema
                .fields
                .iter()
                .any(|f| f.label == "Field experience"));
        }
        Ok::<_, waypoint_applications::driver::DriverError>(false)
    })();
    browser.execute(&BrowserCommand::Close).unwrap();
    assert!(
        observed.unwrap(),
        "Fictional application frame was unavailable"
    );
}

#[test]
#[ignore = "explicit private engine, synthetic HTTP resources and owned memory containment"]
fn coordinator_archive_drives_contained_form() {
    run_coordinator(false, false, false, false);
}
#[test]
#[ignore = "explicit private engine, automatic fictional resource discovery and owned memory containment"]
fn discovery_archives_transitive_resources_before_candidate_entry() {
    run_coordinator(true, false, false, false);
}
#[test]
#[ignore = "explicit private engine, fictional private-URL discovery and owned memory containment"]
fn private_resources_are_not_fetched_before_offline_candidate_entry() {
    run_coordinator(true, true, false, false);
}
#[test]
#[ignore = "explicit contained browser and fictional custom controls; no live source"]
fn independent_fields_preserve_custom_review_and_hard_barriers() {
    run_coordinator(true, false, true, false);
}

#[test]
#[ignore = "explicit contained browser and fictional input mutation; no live site"]
fn failed_entry_retains_bound_readback_without_approving_it() {
    run_coordinator(false, false, false, true);
}

fn qualification_config() -> (RehearsalConfig, PathBuf, PathBuf) {
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
    (
        serde_json::from_slice(&bytes).unwrap(),
        path.parent().unwrap().to_owned(),
        PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_PYTHON").unwrap()),
    )
}
fn run_coordinator(discovery: bool, privacy: bool, custom_controls: bool, failed_entry: bool) {
    let (mut config, folder, python) = qualification_config();
    let html="<!doctype html><meta charset=utf-8><title>Fictional coordinated form</title><link rel=stylesheet href=/style.css><h1>Fictional coordinated form</h1><fieldset><legend>Your details</legend><label>Full name<input id=name required value=source-default-must-not-be-recorded></label></fieldset><button type=button id=next>Continue</button><section id=questions></section><script src=/app.js></script>";
    let html = if custom_controls {
        html.replace(" value=source-default-must-not-be-recorded", "").replace("<button type=button id=next>",
            "<div role=combobox aria-label='Unresolved country' aria-required=true tabindex=0 style='width:240px;height:36px;border:1px solid'>Choose country</div><button type=button id=next>")
    } else {
        html.to_owned()
    };
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
            html.as_str(),
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
        let extra = if discovery && mime == "text/css" {
            format!("{extra}Vary: Accept-Encoding, Origin\r\n")
        } else {
            extra.to_owned()
        };
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
    if discovery {
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(frozen.snapshot.root.join("replay.json")).unwrap(),
        )
        .unwrap();
        for suffix in ["/style.css", "/nested.css"] {
            let style = manifest["resources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| {
                    item["url"]
                        .as_str()
                        .is_some_and(|url| url.ends_with(suffix))
                })
                .unwrap();
            assert_eq!(style["request_origin_absent"], true);
            assert_eq!(style["request_origin_absent_stylesheet"], true);
            assert!(style.get("request_origin").is_none());
        }
    }
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
    if discovery {
        let style = driver.cdp.call("Runtime.evaluate", serde_json::json!({
            "expression":"({font:getComputedStyle(document.body).fontSize,label:getComputedStyle(document.querySelector('label')).display,input:getComputedStyle(document.querySelector('input')).marginLeft})",
            "returnByValue":true})).unwrap();
        assert_eq!(
            style["result"]["value"],
            serde_json::json!({"font":"18px","label":"block","input":"16px"})
        );
    }
    if custom_controls {
        use crate::captured_frame::{RecordedSectionOutcome, RecordedSectionStop};
        driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":"(()=>{const s=document.createElement('script');s.id='captcha-support-fixture';document.head.append(s);const h=document.createElement('input');h.required=true;h.setAttribute('aria-hidden','true');h.style.opacity='0';document.body.append(h);})()"})).unwrap();
        let form = driver.inspect_current().unwrap();
        assert!(form
            .walls
            .iter()
            .any(|wall| wall.contains("Unresolved country")));
        assert!(!form.independent_field_entry_blocked());
        assert!(form
            .walls
            .iter()
            .any(|wall| wall.contains("CAPTCHA support")));
        assert!(form
            .walls
            .iter()
            .any(|wall| wall.contains("required hidden control")));
        std::fs::write(folder.join("custom-inspection.private.json"), serde_json::to_vec_pretty(&serde_json::json!({"fields":form.schema.fields,"walls":form.walls,"entry_blocked":form.independent_field_entry_blocked()})).unwrap()).unwrap();
        let [field] = form.schema.fields.as_slice() else {
            panic!(
                "Expected one initial supported field, got {}",
                form.schema.fields.len()
            );
        };
        let selector = field.selector.clone();
        let capture = driver
            .rehearse_recorded_field(&form, &selector, "Fictional Candidate")
            .unwrap();
        driver.verify_recorded_field(&capture).unwrap();
        driver
            .verify_text_value("#name", "Fictional Candidate")
            .unwrap();
        assert!(matches!(
            driver
                .advance_recorded_section(&form, &[], &[&capture], true)
                .unwrap(),
            RecordedSectionOutcome::Stopped(RecordedSectionStop::PageNeedsReview)
        ));
        assert_eq!(driver.inspect_current().unwrap(), form);
        driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":"(()=>{const el=document.createElement('div');el.className='g-recaptcha';el.textContent='Fictional CAPTCHA barrier';document.body.append(el);})()"})).unwrap();
        let blocked = driver.inspect_current().unwrap();
        assert!(blocked.walls.iter().any(|wall| wall == "captcha present"));
        assert!(blocked.independent_field_entry_blocked());
        assert!(driver
            .rehearse_recorded_field(&blocked, &selector, "Fictional Candidate")
            .is_err());
        assert!(driver
            .rehearse_recorded_field(&form, &selector, "Fictional Candidate")
            .is_err());
        driver
            .verify_text_value(&selector, "Fictional Candidate")
            .unwrap();
        assert_eq!(requests.lock().unwrap().len(), expected_requests);
        std::fs::write(
            folder.join("http-captured-screenshot.base64"),
            driver.screenshot_base64().unwrap(),
        )
        .unwrap();
        let replay = driver.rehearsal_replay_report().unwrap();
        assert_eq!(replay["cached_non_read_replies"], 0);
        assert!(driver.commit().is_err());
        driver.execute(&BrowserCommand::Close).unwrap();
        std::fs::write(folder.join("http-captured.private.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "complete":true,"scope":"fictional custom-control review and independent field entry; hard and stale barriers remain blocked",
            "capture":frozen.report,"report":replay,"transport_dropped_before_entry":true,
            "independent_fields_verified":1,"advancement_blocked":true,"hard_barrier_blocked":true,"stale_inspection_blocked":true})).unwrap()).unwrap();
        return;
    }

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
    if failed_entry {
        assert_eq!(driver.rehearsal_replay_report().unwrap()["served"], 4);
        driver
            .cdp
            .call(
                "Runtime.evaluate",
                serde_json::json!({"expression":r#"(()=>{
            const label=document.createElement('label'); label.textContent='Fictional telephone';
            const input=document.createElement('input'); input.id='masked'; input.type='tel';
            input.oninput=()=>{input.value=input.value.replace(/[^0-9]/g,'')};
            label.append(input); document.body.append(label);
        })()"#}),
            )
            .unwrap();
        let form = driver.inspect_current().unwrap();
        let field = form
            .schema
            .fields
            .iter()
            .find(|f| f.label == "Fictional telephone")
            .unwrap();
        let mut witness = None;
        let result = driver.rehearse_recorded_field_reviewed(
            &form,
            &field.selector,
            "+12025550123",
            &[],
            |target| {
                witness = target.witness();
                Ok(())
            },
        );
        assert!(result.is_err());
        let witness = witness.unwrap();
        let observation = driver.observe_recorded_field(&witness).unwrap();
        assert_eq!(observation.value(), Some("12025550123"));
        assert!(driver
            .verify_text_value(&field.selector, "+12025550123")
            .is_err());
        std::fs::write(
            folder.join("failed-entry-screenshot.base64"),
            observation.image(),
        )
        .unwrap();
        std::fs::write(folder.join("failed-entry.private.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "status":"entry_unverified", "native_value":observation.value(), "control":observation.control(),
            "entry_verified":false,"replay_at_observation":driver.rehearsal_replay_report().unwrap()})).unwrap()).unwrap();
        driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":"(()=>{const el=document.createElement('div');el.className='g-recaptcha';document.body.append(el)})()"})).unwrap();
        assert!(driver.observe_recorded_field(&witness).is_err());
        driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":"document.querySelector('.g-recaptcha').remove();document.querySelector('#masked').setAttribute('aria-label','Changed field')"})).unwrap();
        assert!(driver.observe_recorded_field(&witness).is_err());
        driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":"history.replaceState(null,'',location.href+'#changed')"})).unwrap();
        assert!(driver.observe_recorded_field(&witness).is_err());
    }
    let replay = driver.rehearsal_replay_report().unwrap();
    assert_eq!(
        replay["served"],
        if failed_entry {
            6
        } else if discovery {
            5
        } else {
            4
        }
    );
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
