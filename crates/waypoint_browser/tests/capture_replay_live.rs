//! Explicit fictional HTTP-byte capture -> manifest -> contained browser bridge.
#![cfg(windows)]
use std::path::{Path, PathBuf};
use waypoint_applications::{
    driver::BrowserDriver,
    form_mapping::{BrowserCommand, PacketValue},
};
use waypoint_browser::{rehearsal::RehearsalConfig, rehearsal_capture::SnapshotBuilder, CdpDriver};
#[test]
#[ignore = "explicit checksum-bound engine config, fictional content, owned memory containment"]
fn captured_http_resources_render_and_fill_in_isolated_replay() {
    let root =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let path = std::fs::canonicalize(
        std::env::var_os("WAYPOINT_REHEARSAL_CONFIG").expect("explicit fixture config"),
    )
    .unwrap();
    assert!(path.starts_with(root));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_REHEARSAL_CONFIG_SHA256").unwrap()
    );
    let mut config: RehearsalConfig = serde_json::from_slice(&bytes).unwrap();
    let folder = path.parent().unwrap();
    let destination = folder.join("http-captured");
    let mut builder =
        SnapshotBuilder::start(&destination, "https://fixture.invalid/form#application").unwrap();
    let html="<!doctype html><meta charset=utf-8><title>Fictional captured form</title><link rel=stylesheet href=/style.css><h1>Fictional captured form</h1><label>Name<input id=name></label><button type=button id=next>Continue</button><section id=questions></section><script src=/app.js></script>";
    let script="document.getElementById('next').onclick=()=>fetch('/questions').then(r=>r.json()).then(v=>{const label=document.createElement('label');label.textContent=v.label;const input=document.createElement('input');input.id='email';input.type='email';label.append(input);document.getElementById('questions').append(label);});";
    for (url,body,mime,chunked) in [("https://fixture.invalid/form",html,"text/html; charset=utf-8",false),("https://fixture.invalid/app.js",script,"text/javascript",true),("https://fixture.invalid/style.css","body{font:18px Arial;margin:30px}label{display:block;margin:16px 0}input{margin-left:16px}","text/css",false),("https://fixture.invalid/questions","{\"label\":\"Email\"}","application/json",true)] {
        let raw=if chunked {format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",body.len())}
            else {format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nSet-Cookie: fictional=discarded\r\n\r\n{body}",body.len())};
        let response=waypoint_net::read_asset_response(&mut std::io::Cursor::new(raw.into_bytes()),"GET",1024*1024).unwrap();builder.add(url,response).unwrap();
    }
    let snapshot = builder.finish().unwrap();
    assert_eq!(snapshot.warnings, ["response_cookies_not_replayed"]);
    let source = std::fs::read_to_string(snapshot.root.join(&snapshot.manifest)).unwrap();
    assert!(!source.contains("fictional=discarded") && !source.contains("Set-Cookie"));
    config.schema = 2;
    config.content_root = snapshot.root;
    config.content_files = snapshot.files;
    config.entry = snapshot.entry;
    config.replay_manifest = Some(snapshot.manifest);
    let python =
        PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_PYTHON").expect("explicit Python"));
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while driver.text_value_snapshot("#email").is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "Recorded next section did not appear"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
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
    let report = driver.rehearsal_replay_report().unwrap();
    assert_eq!(report["served"], 4);
    assert_eq!(report["cached_non_read_replies"], 0);
    std::fs::write(
        folder.join("http-captured-screenshot.base64"),
        driver.screenshot_base64().unwrap(),
    )
    .unwrap();
    assert!(driver.commit().is_err());
    driver.execute(&BrowserCommand::Close).unwrap();
    let evidence = serde_json::json!({"complete":true,"scope":"fictional HTTP response framing to offline archive and contained browser; no network source capture","report":report,"cookie_headers_discarded":true});
    std::fs::write(
        folder.join("http-captured.private.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
