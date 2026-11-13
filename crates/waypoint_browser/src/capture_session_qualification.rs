//! Explicit fictional coordinator -> archive -> actual contained browser check.
use super::*;
use crate::{rehearsal::RehearsalConfig, CdpDriver};
use serde_json::Value;
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

#[test]
#[ignore = "explicit zero-capability engine; fictional dialog transitions and adversarial changes"]
fn recorded_dialog_transitions_preserve_values_and_refuse_uncertainty() {
    use crate::RecordedDialogOutcome;
    let (template, folder, python) = qualification_config();
    let (mut successful, mut refused, mut guarded, mut stale) = (0, 0, 0, 0);
    for (index, behavior, embedded) in [
        (0,"document.getElementById('notice').remove()",false),
        (1,"document.getElementById('notice').remove()",true),
        (2,"document.getElementById('email').value='changed';document.getElementById('notice').remove()",false),
        (3,"document.getElementById('application').requestSubmit()",false),
        (4,"document.getElementById('notice').innerHTML='<p>Another dialog</p><button type=button>Review</button>'",false),
        (5,"document.getElementById('notice').remove()",false),
        (6,"document.getElementById('notice').remove()",false),
    ] {
        let entry="https://fixture.invalid/dialog";
        let frame="https://frame.invalid/dialog";
        let body=format!("<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{{font:18px Arial;margin:20px}}#notice{{position:fixed;inset:80px 25px auto;background:white;border:2px solid #124;padding:20px;z-index:10}}button{{font:18px Arial;padding:12px;margin:8px}}</style><form id=application method=post action=/send><label>Email<input id=email name=email value=original></label></form><div role=dialog aria-label='Privacy choices' id=notice><p>Optional tracking is your choice.</p><button id=decline onclick=\"{behavior}\">Keep optional tracking off</button><button type=button>Allow optional tracking</button><button type=submit form=application>Send application</button></div>");
        let body=if index==5 {body.replace("id=decline onclick=", "id=decline onmouseenter=\"this.textContent='Changed target'\" onclick=")} else {body};
        let mut builder=SnapshotBuilder::start(&folder.join(format!("dialog-{index}")),entry).unwrap();
        let response=|html:&str| {
            let wire=format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{html}",html.len());
            waypoint_net::read_asset_response(&mut Cursor::new(wire.into_bytes()),"GET",1024*1024).unwrap()
        };
        if embedded {
            builder.add(entry,response(&format!("<!doctype html><link rel=icon href=data:,><iframe title='Fictional preferences' width=1000 height=650 src={frame}></iframe>"))).unwrap();
            builder.add(frame,response(&body)).unwrap();
        } else {builder.add(entry,response(&body)).unwrap();}
        let snapshot=builder.finish().unwrap();let mut config=template.clone();
        config.schema=2;config.content_root=snapshot.root;config.content_files=snapshot.files;config.entry=snapshot.entry;config.replay_manifest=Some(snapshot.manifest);
        let (mut browser,ready)=CdpDriver::launch_rehearsal(&python,config).unwrap();assert!(ready.appcontainer && ready.capabilities==0);
        let mut form=browser.open(entry).unwrap();
        if embedded {let frames=browser.captured_frames().unwrap();assert_eq!(frames.frames.len(),1);form=browser.enter_captured_frame(&frames.frames[0]).unwrap();}
        let dialog=browser.recorded_dialog(&form).unwrap().unwrap();let description=dialog.description();
        assert_eq!(description.controls.len(),3);assert!(description.controls[0].available);assert!(!description.controls[2].available);
        assert!(browser.dismiss_recorded_dialog_reviewed(&dialog,"control-2",|_|panic!("unsafe target reached vision")).is_err());
        assert!(browser.dismiss_recorded_dialog_reviewed(&dialog,"control-99",|_|panic!("unknown target reached vision")).is_err());
        assert!(browser.dismiss_recorded_dialog_reviewed(&dialog,"control-0",|_|Err(waypoint_applications::driver::DriverError("fictional visual refusal".into()))).is_err());
        assert!(browser.trace.iter().all(|event|!event.starts_with("clicked")));
        if index==6 {
            let selector=browser.captured_selector("#email");
            browser.execute(&BrowserCommand::Fill {selector,value:PacketValue::Text("changed-before-action".into())}).unwrap();
        }
        let clicks_before=browser.trace.iter().filter(|event|event.starts_with("clicked")).count();
        let outcome=browser.dismiss_recorded_dialog_reviewed(&dialog,"control-0",|target| {
            assert_eq!(target.label(),"Keep optional tracking off");assert_eq!(target.kind(),"button");
            if index<2 {std::fs::write(folder.join(format!("dialog-{index}-before.base64")),target.image()).unwrap();}
            Ok(())
        }).unwrap();
        match (index,outcome) {
            (0|1,RecordedDialogOutcome::NativeClosed {capture})=>{
                assert!(browser.recorded_dialog(capture.form()).unwrap().is_none());
                browser.verify_recorded_dialog_closed(&capture).unwrap();successful+=1;
                std::fs::write(folder.join(format!("dialog-{index}-after.base64")),capture.image()).unwrap();
                let selector=browser.captured_selector("#email");
                browser.execute(&BrowserCommand::Fill {selector,value:PacketValue::Text("changed-after-evidence".into())}).unwrap();
                assert!(browser.verify_recorded_dialog_closed(&capture).is_err());stale+=1;
            }
            (2,RecordedDialogOutcome::Stopped {reason:"field_changed",..}) |
            (3,RecordedDialogOutcome::Stopped {reason:"replay_gap",..}) |
            (4,RecordedDialogOutcome::Stopped {reason:"dialog_remains",..})=>refused+=1,
            (5,RecordedDialogOutcome::Stopped {reason:"interaction_unverified",..}) |
            (6,RecordedDialogOutcome::Stopped {reason:"dialog_changed",..})=>{
                assert_eq!(browser.trace.iter().filter(|event|event.starts_with("clicked")).count(),clicks_before);guarded+=1;
            }
            _=>panic!("Unexpected dialog outcome for fixture {index}"),
        }
        if embedded {browser.leave_captured_frame().unwrap();}
        browser.execute(&BrowserCommand::Close).unwrap();
    }
    assert_eq!((successful, refused, guarded, stale), (2, 3, 2, 2));
    std::fs::write(folder.join("dialog-qualification.private.json"),serde_json::to_vec_pretty(&serde_json::json!({"native_closed":successful,"post_action_refusals":refused,"pre_action_refusals":21,"guarded_refusals":guarded,"freshness_refusals":stale,"external_delivery":"not_performed"})).unwrap()).unwrap();
}
impl Transport for RecordedHttp {
    fn get(
        &mut self,
        url: &str,
        _: u64,
        max: usize,
        origin: Option<&waypoint_net::PublicRequestOrigin>,
        accept: Option<&waypoint_net::PublicDocumentAccept>,
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
        response.request_accept = Some(
            accept
                .map(waypoint_net::PublicDocumentAccept::as_str)
                .unwrap_or("*/*")
                .into(),
        );
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
        "<!doctype html><link rel=icon href=data:,><h1>Fictional listing</h1><iframe title=Application width=720 height=480 src={frame}></iframe><img src={unrelated}>"
    );
    let responses = [
        ("https://fixture.invalid/robots.txt", "text/plain", "User-agent: *\nAllow: /"),
        ("https://jobs.lever.co/robots.txt", "text/plain", "User-agent: *\nAllow: /"),
        ("https://jobs.ashbyhq.com/robots.txt", "text/plain", "User-agent: *\nDisallow: /"),
        (entry, "text/html", document.as_str()),
        (frame, "text/html", "<!doctype html><link rel=icon href=data:,><label>Field experience<input required></label><div role=group aria-label='Supporting documents'><label for=document>Candidate document</label><input id=document type=file><p aria-live=polite style='margin-top:170px;color:maroon'>Fictional upload needs review</p></div>"),
    ]
    .into_iter()
    .map(|(url, mime, body)| {
        let vary = if mime == "text/html" {
            "Vary: Accept-Encoding\r\nVary: Accept, X-Requested-With, Origin\r\n"
        } else {
            ""
        };
        (url.into(), format!("HTTP/1.1 200 Fixture\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{vary}\r\n{body}", body.len()).into_bytes())
    })
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
    let accept = crate::capture_discovery::document_request_accept(
        &python,
        config.clone(),
        &folder.join("document-request-probe"),
    )
    .unwrap();
    capture.set_document_accept(accept.clone()).unwrap();
    let discovery = capture
        .discover_resources(&python, config.clone(), 4, 1500)
        .unwrap();
    let archive = capture.freeze().unwrap();
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(archive.snapshot.root.join(&archive.snapshot.manifest)).unwrap(),
    )
    .unwrap();
    for url in [entry, frame] {
        let resource = manifest["resources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["url"] == url)
            .unwrap();
        assert_eq!(resource["request_accept"], accept.as_str());
        assert_eq!(resource["request_x_requested_with_absent"], true);
        assert_eq!(resource["request_origin_absent"], true);
    }
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
            frame,
            "https://jobs.ashbyhq.com/robots.txt"
        ]
    );
    config.schema = 2;
    config.content_root = archive.snapshot.root;
    config.content_files = archive.snapshot.files;
    config.entry = archive.snapshot.entry;
    config.replay_manifest = Some(archive.snapshot.manifest);
    let (mut browser, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    assert!(ready.appcontainer && ready.capabilities == 0);
    let observed = (|| {
        browser.open(entry)?;
        let frames = browser.captured_frames()?;
        if let [frame] = frames.frames.as_slice() {
            let child = browser.enter_captured_frame(frame)?;
            let contact = child
                .schema
                .fields
                .iter()
                .find(|f| f.label == "Field experience")
                .unwrap();
            std::fs::write(
                folder.join("embedded-contact-context.base64"),
                browser.screenshot_field_context(&contact.selector)?,
            )
            .unwrap();
            let file = child
                .schema
                .fields
                .iter()
                .find(|f| f.field_type == waypoint_applications::form_mapping::FormFieldType::File)
                .unwrap();
            std::fs::write(
                folder.join("embedded-file-context.base64"),
                browser.screenshot_field_feedback(&file.selector)?,
            )
            .unwrap();
            assert_eq!(
                browser.read_attribute(&file.selector, "type")?.as_deref(),
                Some("file")
            );
            let files = browser.cdp.call("Runtime.evaluate",serde_json::json!({"expression":"document.querySelector('#document').files.length","returnByValue":true})).unwrap();
            assert_eq!(files["result"]["value"], 0);
            std::fs::write(folder.join("embedded-context.private.json"),serde_json::to_vec_pretty(&serde_json::json!({"file_and_contact_images":true,"files_selected":0,"external_delivery":"not_performed"})).unwrap()).unwrap();
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

#[test]
#[ignore = "explicit zero-capability engine; fictional six-layer dependency discovery"]
fn deep_discovery_renders_transitive_form_with_unchanged_session_budgets() {
    use crate::capture_discovery::{DiscoveryStop, MAX_DISCOVERY_ROUNDS};
    let (mut config, folder, python) = qualification_config();
    let mut responses = BTreeMap::new();
    let wire = |body: &str, mime: &str| {
        format!(
            "HTTP/1.1 200 Fictional\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    };
    let entry = "https://fixture.invalid/deep";
    responses.insert(
        "https://fixture.invalid/robots.txt".into(),
        wire("User-agent: *\nAllow: /", "text/plain"),
    );
    responses.insert(entry.into(),wire("<!doctype html><html lang=en><head><meta charset=utf-8><link rel=icon href=data:,><title>Fictional deep form</title></head><body><h1>Fictional deep form</h1><script src=/layer1.js></script></body></html>","text/html"));
    for index in 1..=6 {
        let script = if index < 6 {
            format!("(()=>{{const next=document.createElement('script');next.src='/layer{}.js';next.async=false;document.head.append(next);}})();",index+1)
        } else {
            "document.body.insertAdjacentHTML('beforeend','<label>Contact email<input id=deepemail type=email required></label>');".into()
        };
        responses.insert(
            format!("https://fixture.invalid/layer{index}.js"),
            wire(&script, "text/javascript"),
        );
    }
    let requests = Arc::new(Mutex::new(vec![]));
    let dropped = Arc::new(AtomicBool::new(false));
    let mut capture = CaptureSession::with_transport(
        &folder.join("deep-capture"),
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
            requests: requests.clone(),
            dropped: dropped.clone(),
        }),
    )
    .unwrap();
    let discovery = capture
        .discover_resources(&python, config.clone(), MAX_DISCOVERY_ROUNDS, 500)
        .unwrap();
    assert_eq!(discovery.stopped, DiscoveryStop::NoNewRequests);
    assert_eq!(discovery.rounds.len(), 7);
    assert!(discovery.rounds.iter().all(|r| r.review_outcomes == 0
        && r.ineligible_requests == 0
        && r.omitted_requests == 0
        && r.pending_commands == 0));
    let frozen = capture.freeze().unwrap();
    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(frozen.report.recorded_resources, 7);
    assert_eq!(requests.lock().unwrap().len(), 8);
    config.schema = 2;
    config.content_root = frozen.snapshot.root;
    config.content_files = frozen.snapshot.files;
    config.entry = frozen.snapshot.entry;
    config.replay_manifest = Some(frozen.snapshot.manifest);
    let (mut browser, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    assert!(ready.appcontainer && ready.capabilities == 0);
    let form = browser.open(entry).unwrap();
    assert_eq!(form.schema.fields.len(), 1);
    assert_eq!(form.schema.fields[0].label, "Contact email");
    assert_eq!(form.schema.fields[0].selector, "#deepemail");
    assert!(!form.independent_field_entry_blocked());
    let replay = browser.rehearsal_replay_report().unwrap();
    assert_eq!(replay["blocked"], 0);
    std::fs::write(
        folder.join("deep-discovery-screenshot.base64"),
        browser.screenshot_base64().unwrap(),
    )
    .unwrap();
    browser.execute(&BrowserCommand::Close).unwrap();
    std::fs::write(folder.join("deep-discovery.private.json"),serde_json::to_vec_pretty(&serde_json::json!({"complete":true,"rounds":7,"recorded_resources":7,"transport_requests":8,"native_fields":1,"fields_entered":0,"external_delivery":"not_performed"})).unwrap()).unwrap();
}

type VariantRequest = (String, Option<String>);
struct VariantHttp {
    responses: BTreeMap<VariantRequest, Vec<u8>>,
    requests: Arc<Mutex<Vec<VariantRequest>>>,
    dropped: Arc<AtomicBool>,
}
impl Transport for VariantHttp {
    fn get(
        &mut self,
        url: &str,
        _: u64,
        max: usize,
        origin: Option<&waypoint_net::PublicRequestOrigin>,
        accept: Option<&waypoint_net::PublicDocumentAccept>,
    ) -> Result<CapturedResponse, ()> {
        let key = (url.to_owned(), origin.map(|o| o.as_str().to_owned()));
        self.requests.lock().unwrap().push(key.clone());
        let bytes = self.responses.remove(&key).ok_or(())?;
        let mut value = waypoint_net::read_asset_response(&mut Cursor::new(bytes), "GET", max)
            .map_err(|_| ())?;
        value.request_header_names = Some(vec!["host".into(), "accept".into()]);
        value.request_accept = Some(
            accept
                .map(waypoint_net::PublicDocumentAccept::as_str)
                .unwrap_or("*/*")
                .into(),
        );
        value.request_origin = origin.cloned();
        if origin.is_some() {
            value
                .request_header_names
                .as_mut()
                .unwrap()
                .push("origin".into());
        }
        Ok(value)
    }
}
impl Drop for VariantHttp {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}
#[test]
#[ignore = "explicit zero-capability engine; independently captured Origin stylesheet variants"]
fn observed_request_variants_collect_and_render_independent_styles() {
    use crate::capture_discovery::{DiscoveryStop, MAX_DISCOVERY_ROUNDS};
    let (mut config, folder, python) = qualification_config();
    let entry = "https://fixture.invalid/variants";
    let style = "https://cdn.fixture.invalid/shared.css";
    let wire = |body: &str, mime: &str, extra: &str| {
        format!("HTTP/1.1 200 Fictional\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{extra}\r\n{body}",body.len()).into_bytes()
    };
    let mut responses = BTreeMap::new();
    for origin in ["https://fixture.invalid", "https://cdn.fixture.invalid"] {
        responses.insert(
            (format!("{origin}/robots.txt"), None),
            wire("User-agent: *\nAllow: /", "text/plain", ""),
        );
    }
    let html=format!("<!doctype html><html lang=en><head><meta charset=utf-8><link rel=icon href=data:,><title>Fictional request variants</title><link rel=stylesheet crossorigin=anonymous href={style}><link rel=stylesheet href={style}></head><body><h1>Fictional request variants</h1><label>Email<input type=email id=email></label></body></html>");
    responses.insert((entry.into(), None), wire(&html, "text/html", ""));
    responses.insert(
        (style.into(), Some("https://fixture.invalid".into())),
        wire(
            "input{border:3px solid rgb(1,2,3);font:18px Arial}",
            "text/css",
            "Vary: Origin\r\nAccess-Control-Allow-Origin: *\r\n",
        ),
    );
    responses.insert(
        (style.into(), None),
        wire(
            "input{background:rgb(4,5,6);color:white;padding:8px}",
            "text/css",
            "Vary: Origin\r\nAccess-Control-Allow-Origin: *\r\n",
        ),
    );
    let requests = Arc::new(Mutex::new(vec![]));
    let dropped = Arc::new(AtomicBool::new(false));
    let mut capture = CaptureSession::with_transport(
        &folder.join("variant-capture"),
        entry,
        vec![
            CaptureOrigin {
                origin: "https://fixture.invalid".into(),
                source: "user_supplied_url".into(),
            },
            CaptureOrigin {
                origin: "https://cdn.fixture.invalid".into(),
                source: "user_supplied_url".into(),
            },
        ],
        PermissionRegistry::from_json(waypoint_policy::INITIAL_REGISTRY_JSON).unwrap(),
        CaptureBudget::default(),
        Arc::new(AtomicBool::new(false)),
        Box::new(VariantHttp {
            responses,
            requests: requests.clone(),
            dropped: dropped.clone(),
        }),
    )
    .unwrap();
    let discovery = capture
        .discover_resources(&python, config.clone(), MAX_DISCOVERY_ROUNDS, 700)
        .unwrap();
    assert_eq!(discovery.stopped, DiscoveryStop::NoNewRequests);
    assert_eq!(discovery.rounds.len(), 2);
    assert_eq!(discovery.rounds[0].proposed, 2);
    assert_eq!(discovery.rounds[0].newly_recorded, 2);
    let frozen = capture.freeze().unwrap();
    assert!(dropped.load(Ordering::Acquire));
    assert_eq!(frozen.report.recorded_resources, 3);
    assert_eq!(frozen.report.recorded_urls, 2);
    assert_eq!(requests.lock().unwrap().len(), 5);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(frozen.snapshot.root.join("replay.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["schema"], 2);
    config.schema = 2;
    config.content_root = frozen.snapshot.root;
    config.content_files = frozen.snapshot.files;
    config.entry = frozen.snapshot.entry;
    config.replay_manifest = Some(frozen.snapshot.manifest);
    let (mut browser, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    assert!(ready.appcontainer && ready.capabilities == 0);
    let form = browser.open(entry).unwrap();
    assert_eq!(form.schema.fields.len(), 1);
    assert_eq!(form.schema.fields[0].label, "Email");
    assert!(!form.independent_field_entry_blocked());
    let observed=browser.cdp.call("Runtime.evaluate",serde_json::json!({"expression":"JSON.stringify({border:getComputedStyle(document.getElementById('email')).borderTopColor,background:getComputedStyle(document.getElementById('email')).backgroundColor})","returnByValue":true})).unwrap();
    let styles: Value =
        serde_json::from_str(observed["result"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(styles["border"], "rgb(1, 2, 3)");
    assert_eq!(styles["background"], "rgb(4, 5, 6)");
    let replay = browser.rehearsal_replay_report().unwrap();
    assert_eq!(replay["blocked"], 0);
    std::fs::write(
        folder.join("request-variants-screenshot.base64"),
        browser.screenshot_base64().unwrap(),
    )
    .unwrap();
    browser.execute(&BrowserCommand::Close).unwrap();
    std::fs::write(folder.join("request-variants.private.json"),serde_json::to_vec_pretty(&serde_json::json!({"complete":true,"manifest_schema":2,"rounds":2,"recorded_resources":3,"recorded_urls":2,"transport_requests":5,"native_fields":1,"styles":styles,"fields_entered":0,"external_delivery":"not_performed"})).unwrap()).unwrap();
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
        // Finish the original witness refusals before adding any new fields;
        // otherwise the extra schema alone would make the barrier test vacuous.
        driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":format!("history.replaceState(null,'',{});document.querySelector('#masked').removeAttribute('aria-label')", serde_json::to_string(&ready.entry_url).unwrap())})).unwrap();
        qualify_telephone_presentation(&mut driver, &folder);
        qualify_file_group_target(&mut driver, &folder);
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

fn qualify_telephone_presentation(driver: &mut CdpDriver, folder: &Path) {
    driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":r#"(()=>{
        const label=document.createElement('label'); label.textContent='International contact';
        const input=document.createElement('input'); input.id='international-contact'; input.type='tel';
        input.oninput=()=>{if(input.value.replace(/[^0-9]/g,'')==='12025550123') input.value='+1 202-555-0123'};
        label.append(input); document.body.append(label);
    })()"#})).unwrap();
    let form = driver.inspect_current().unwrap();
    let field = form
        .schema
        .fields
        .iter()
        .find(|field| field.label == "International contact")
        .unwrap();
    let capture = driver
        .rehearse_recorded_field(&form, &field.selector, "+12025550123")
        .unwrap();
    assert_eq!(capture.value(), "+12025550123");
    assert_eq!(capture.observed_value(), "+1 202-555-0123");
    assert_eq!(
        capture.value_comparison(),
        "international_telephone_presentation"
    );
    driver.verify_recorded_field(&capture).unwrap();
    std::fs::write(
        folder.join("telephone-presentation-screenshot.base64"),
        capture.image(),
    )
    .unwrap();
    let report = serde_json::json!({"intended":capture.value(),"observed":capture.observed_value(),
        "comparison":capture.value_comparison(),"control":capture.control()});
    driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":"document.querySelector('#international-contact').value='+1 (202) 555-0123'"})).unwrap();
    driver
        .verify_text_value(&field.selector, "+12025550123")
        .unwrap();
    // Even equivalent reformatting invalidates the older image/value snapshot.
    assert!(driver.verify_recorded_field(&capture).is_err());
    let mut denied = 0;
    for (kind, actual, attribute, attr_value) in [
        ("text", "+1 202-555-0123", "", ""),
        ("tel", "+44 202-555-0123", "", ""),
        ("tel", "202-555-0123", "", ""),
        ("tel", "+1 202-555-0123 x2", "", ""),
        ("tel", "+1 202-555-0123", "pattern", r"\+[0-9]{11}"),
        ("tel", "+1 202-555-0123", "aria-invalid", "true"),
        ("tel", "+1 202-555-0123", "readonly", ""),
        ("tel", "+1 202-555-0123", "disabled", ""),
        ("tel", "+1 202-555-0123", "role", "combobox"),
        ("tel", "+1 202-555-0123", "aria-autocomplete", "list"),
        ("tel", "+1 202-555-0123", "style", "display:none"),
    ] {
        let setup = format!("(()=>{{const old=document.querySelector('#negative-contact');if(old)old.remove();const input=document.createElement('input');input.id='negative-contact';input.type={};input.value={};if({})input.setAttribute({},{});document.body.append(input)}})()",
            serde_json::to_string(kind).unwrap(),serde_json::to_string(actual).unwrap(),!attribute.is_empty(),
            serde_json::to_string(attribute).unwrap(),serde_json::to_string(attr_value).unwrap());
        driver
            .cdp
            .call("Runtime.evaluate", serde_json::json!({"expression":setup}))
            .unwrap();
        assert!(
            driver
                .verify_text_value("#negative-contact", "+12025550123")
                .is_err(),
            "{kind} {attribute}"
        );
        denied += 1;
    }
    driver.cdp.call("Runtime.evaluate", serde_json::json!({"expression":"const el=document.querySelector('#negative-contact');el.removeAttribute('style');el.setCustomValidity('Fictional constraint')"})).unwrap();
    assert!(driver
        .verify_text_value("#negative-contact", "+12025550123")
        .is_err());
    std::fs::write(folder.join("telephone-presentation.private.json"),serde_json::to_vec_pretty(&serde_json::json!({"capture":report,"negative_cases":denied+1,"stale_image_refused":true})).unwrap()).unwrap();
}

fn qualify_file_group_target(driver: &mut CdpDriver, folder: &Path) {
    driver.cdp.call("Runtime.evaluate",serde_json::json!({"expression":r#"(()=>{
        document.querySelector('#international-contact')?.remove();
        document.querySelector('#negative-contact')?.remove();
        const group=document.createElement('div');group.id='file-review-group';group.role='group';group.setAttribute('aria-labelledby','file-review-heading');
        const heading=document.createElement('h3');heading.id='file-review-heading';heading.textContent='Materials';group.append(heading);
        const label=document.createElement('label');label.htmlFor='group-file';label.textContent='Select a document';group.append(label);
        const input=document.createElement('input');input.id='group-file';input.type='file';group.append(input);document.body.append(group);
    })()"#})).unwrap();
    let form = driver.inspect_current().unwrap();
    let field = form
        .schema
        .fields
        .iter()
        .find(|f| f.selector == "#group-file")
        .unwrap();
    let mut reviewed = false;
    let refused = driver.rehearse_recorded_field_reviewed(
        &form,
        &field.selector,
        "unused.pdf",
        &[],
        |target| {
            reviewed = true;
            assert_eq!(target.kind(), "file");
            assert_eq!(target.file_group_label(), Some("Materials"));
            std::fs::write(folder.join("file-group-screenshot.base64"), target.image()).unwrap();
            Err(waypoint_applications::driver::DriverError(
                "Intentional review refusal".into(),
            ))
        },
    );
    assert!(reviewed && refused.is_err());
    let mut refusals = 0;
    for change in [
        "const other=document.createElement('input');other.id='other-group-file';other.type='file';document.querySelector('#file-review-group').append(other)",
        "document.querySelector('#other-group-file').remove();document.querySelector('#file-review-group').removeAttribute('aria-labelledby');document.querySelector('#file-review-group').removeAttribute('role')",
    ] {
        driver.cdp.call("Runtime.evaluate",serde_json::json!({"expression":change})).unwrap();
        let form=driver.inspect_current().unwrap();
        let mut asked=false;
        let refused=driver.rehearse_recorded_field_reviewed(&form,"#group-file","unused.pdf",&[],|target|{
            asked=true;assert_eq!(target.file_group_label(),None);
            Err(waypoint_applications::driver::DriverError("Intentional review refusal".into()))
        });
        assert!(asked && refused.is_err());refusals+=1;
    }
    let files=driver.cdp.call("Runtime.evaluate",serde_json::json!({"expression":"document.querySelector('#group-file').files.length","returnByValue":true})).unwrap();
    assert_eq!(files["result"]["value"], 0);
    std::fs::write(folder.join("file-group-identity.private.json"),serde_json::to_vec_pretty(&serde_json::json!({"explicit_group_verified":true,"negative_cases":refusals,"review_refusals":3,"files_attached":0})).unwrap()).unwrap();
}
