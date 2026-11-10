//! Private, native control-plane qualification. No product debug API.
use super::CdpDriver;
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use waypoint_applications::{
    driver::BrowserDriver,
    form_mapping::{BrowserCommand, PacketValue},
};

#[test]
#[ignore = "explicit frozen fictional replay, selected engine, owned Windows qualification"]
fn closed_interceptor_releases_aliases_before_reattachment() {
    let path = PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_CONFIG").unwrap())
        .canonicalize()
        .unwrap();
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap();
    assert!(path.starts_with(target));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_REHEARSAL_CONFIG_SHA256").unwrap()
    );
    let config: super::rehearsal::RehearsalConfig = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(config.schema, 2);
    let python = PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_PYTHON").unwrap());
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    ready.validate().unwrap();
    driver.open(&ready.entry_url).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while driver
        .verify_text_value("#bootstrap-status", "ready")
        .is_err()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#name".into(),
            value: PacketValue::Text("Fictional retained name".into()),
        })
        .unwrap();
    let targets = driver
        .cdp
        .call_browser("Target.getTargets", json!({}))
        .unwrap();
    let matching: Vec<_> = targets["targetInfos"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|info| info["type"] == "page" && info["url"] == ready.entry_url)
        .collect();
    assert_eq!(matching.len(), 1);
    let target_id = matching[0]["targetId"].as_str().unwrap();
    let extra = driver
        .cdp
        .call_browser(
            "Target.attachToTarget",
            json!({"targetId":target_id,"flatten":true}),
        )
        .unwrap();
    let extra = extra["sessionId"].as_str().unwrap().to_string();
    let primary = driver.cdp.session.clone();
    driver
        .cdp
        .call_browser("Target.detachFromTarget", json!({"sessionId":primary}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let retired = loop {
        let report = driver.rehearsal_replay_report().unwrap();
        if report["closed_aliases"].as_u64().unwrap() >= 1 && report["pending_detachments"] == 0 {
            break report;
        }
        assert!(
            Instant::now() < deadline,
            "Related sessions were not closed: {report}"
        );
        driver.cdp.pump(Duration::from_millis(50));
    };
    let absent = driver
        .cdp
        .call_browser("Target.detachFromTarget", json!({"sessionId":extra}))
        .unwrap_err();
    assert!(
        absent.to_string().contains("No session with given id"),
        "{absent}"
    );
    let fresh = driver
        .cdp
        .call_browser(
            "Target.attachToTarget",
            json!({"targetId":target_id,"flatten":true}),
        )
        .unwrap();
    driver.cdp.session = fresh["sessionId"].as_str().unwrap().to_string();
    for method in ["Page.enable", "Network.enable", "DOM.enable"] {
        driver.cdp.call(method, json!({})).unwrap();
    }
    driver
        .verify_text_value("#name", "Fictional retained name")
        .unwrap();
    let after = driver.rehearsal_replay_report().unwrap();
    driver.execute(&BrowserCommand::Close).unwrap();
    let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
        .join("Packages")
        .join(ready.profile_name);
    assert!(!profile.join("AC").exists());
    std::fs::write(path.parent().unwrap().join("alias-lifecycle.private.json"),serde_json::to_vec_pretty(&json!({"complete":true,"scope":"fictional native CDP aliases; not employer application entry","retired":retired,"after":after,"alias_physically_absent":true,"value_retained":true,"profile_removed":true})).unwrap()).unwrap();
}
