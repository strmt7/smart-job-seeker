//! Opt-in fictional HTTPS-origin replay, inside the actual AppContainer transport.
#![cfg(windows)]
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use waypoint_applications::{
    driver::BrowserDriver,
    form_mapping::{BrowserCommand, PacketValue},
};
use waypoint_browser::{rehearsal::RehearsalConfig, CdpDriver};

fn wait_value(driver: &mut CdpDriver, selector: &str, value: &str) {
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        match driver.verify_text_value(selector, value) {
            Ok(()) => return,
            Err(error) if Instant::now() >= until => panic!("{selector}: {error}"),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[test]
#[ignore = "explicit memory-contained fictional replay archive and selected headless engine"]
fn isolated_replay_preserves_origin_scripts_steps_and_blocks_uncaptured_requests() {
    let path = std::fs::canonicalize(
        std::env::var_os("WAYPOINT_REHEARSAL_CONFIG").expect("explicit replay config"),
    )
    .unwrap();
    let root =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
    assert!(path.starts_with(root));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_REHEARSAL_CONFIG_SHA256").unwrap()
    );
    let config: RehearsalConfig = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(config.schema, 2);
    let artifact = path.parent().unwrap();
    let python =
        PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_PYTHON").expect("explicit Python"));
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    ready.validate().unwrap();
    assert!(ready.replay.is_some());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let form = driver.open(&ready.entry_url).unwrap();
        assert_eq!(form.origin, "https://fixture.invalid");
        assert_eq!(
            driver.current_url().unwrap(),
            "https://fixture.invalid/form#application"
        );
        wait_value(&mut driver, "#bootstrap-status", "ready");
        wait_value(&mut driver, "#frame-status", "ready");
        wait_value(&mut driver, "#worker-status", "recorded-worker");
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#name".into(),
                value: PacketValue::Text("Fictional Candidate — 資格".into()),
            })
            .unwrap();
        driver
            .execute(&BrowserCommand::Click {
                selector: "#next".into(),
            })
            .unwrap();
        wait_value(&mut driver, "#step-status", "questions loaded");
        driver
            .execute(&BrowserCommand::Fill {
                selector: "#email".into(),
                value: PacketValue::Text("fictional@example.invalid".into()),
            })
            .unwrap();
        driver
            .verify_text_value("#email", "fictional@example.invalid")
            .unwrap();
        driver
            .execute(&BrowserCommand::UploadFile {
                selector: "#resume".into(),
                file_name: "fictional.pdf".into(),
            })
            .unwrap();
        driver
            .verify_local_file("#resume", &ready.content_root.join("fictional.pdf"))
            .unwrap();
        let refusal = driver
            .execute(&BrowserCommand::Fill {
                selector: "#sideeffect".into(),
                value: PacketValue::Text("Fictional side effect".into()),
            })
            .unwrap_err();
        assert!(
            refusal.to_string().contains("request")
                && refusal.to_string().contains("text entry stopped")
        );
        wait_value(&mut driver, "#blocked-status", "blocked");
        let report = driver.rehearsal_replay_report().unwrap();
        assert!(report["served"].as_u64().unwrap() >= 7, "{report}");
        assert!(
            report["cached_non_read_replies"].as_u64().unwrap() >= 1,
            "{report}"
        );
        assert!(report["blocked"].as_u64().unwrap() >= 1, "{report}");
        assert_eq!(report["server_validation"], "not_performed");
        assert!(!report.to_string().contains("Fictional Candidate"));
        assert!(!report.to_string().contains("fictional@example.invalid"));
        let screenshot = driver.screenshot_base64().unwrap();
        assert!(screenshot.starts_with("iVBOR"));
        std::fs::write(artifact.join("replay-screenshot.base64"), screenshot).unwrap();
        assert!(driver
            .commit()
            .unwrap_err()
            .to_string()
            .contains("unavailable in isolated rehearsal"));
        report
    }));
    let report = match outcome {
        Ok(report) => report,
        Err(error) => {
            if let Ok(report) = driver.rehearsal_replay_report() {
                let _ = std::fs::write(
                    artifact.join("failure-report.private.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
            }
            if let Ok(screenshot) = driver.screenshot_base64() {
                let _ = std::fs::write(artifact.join("failure-screenshot.base64"), screenshot);
            }
            std::panic::resume_unwind(error);
        }
    };
    let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
        .join("Packages")
        .join(&ready.profile_name);
    driver.execute(&BrowserCommand::Close).unwrap();
    assert!(!profile.join("AC").exists());
    std::fs::write(artifact.join("native.private.json"),serde_json::to_vec_pretty(&serde_json::json!({"complete":true,"scope":"fictional recorded-response replay; no live employer interaction or general no-egress qualification","report":report,"profile_removed":true})).unwrap()).unwrap();
}
