//! Explicit, headless Windows qualification. Only checksum-bound fictional
//! snapshots inside the ignored target directory may be supplied to this test.
#![cfg(windows)]

use std::path::PathBuf;
use waypoint_applications::driver::BrowserDriver;
use waypoint_applications::form_mapping::{BrowserCommand, PacketValue};
use waypoint_browser::{rehearsal::RehearsalConfig, CdpDriver};

#[path = "support/rehearsal_network.rs"]
mod network;

#[path = "support/loopback.rs"]
pub mod loopback;

fn setup() -> (PathBuf, RehearsalConfig, PathBuf) {
    let python =
        PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_PYTHON").expect("explicit Python"));
    let config = std::fs::canonicalize(
        std::env::var_os("WAYPOINT_REHEARSAL_CONFIG").expect("explicit fictional configuration"),
    )
    .unwrap();
    let root =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
    assert!(config.starts_with(&root));
    let bytes = std::fs::read(&config).unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_REHEARSAL_CONFIG_SHA256").unwrap()
    );
    let config: RehearsalConfig = serde_json::from_slice(&bytes).unwrap();
    assert!(std::fs::canonicalize(&config.content_root)
        .unwrap()
        .starts_with(root));
    assert_eq!(config.entry, "index.html");
    (
        python,
        config,
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()),
    )
}

#[test]
#[ignore = "requires an explicitly supplied headless engine and fictional snapshot"]
fn isolated_driver_fills_navigates_captures_and_removes_profile() {
    let (python, config, local) = setup();
    let artifact = config
        .content_root
        .parent()
        .unwrap()
        .join("driver-screenshot.base64");
    let (mut driver, evidence) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    evidence.validate().unwrap();
    let profile = local.join("Packages").join(&evidence.profile_name);
    assert!(profile.join("AC").is_dir());
    let form = driver.open(&evidence.entry_url).unwrap();
    assert!(form
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#name"));
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
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#email".into(),
            value: PacketValue::Text("fictional@example.invalid".into()),
        })
        .unwrap();
    driver
        .verify_text_value("#email", "fictional@example.invalid")
        .unwrap();
    // The debugger cannot make the contained browser read a host-only file.
    // This is fictional data in the newly protected recovery directory.
    let private = local
        .join("SmartJobseeker/rehearsal-state-v1")
        .join(format!("privacy-probe-{}.pdf", std::process::id()));
    struct PrivateProbe(PathBuf);
    impl Drop for PrivateProbe {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut probe = std::fs::File::create_new(&private).unwrap();
    let _private = PrivateProbe(private.clone());
    use std::io::Write;
    probe
        .write_all(&std::fs::read(evidence.content_root.join("fictional.pdf")).unwrap())
        .unwrap();
    drop(probe);
    assert!(
        driver.attach_local_file("#resume", &private).is_err(),
        "contained browser read a host-only private file"
    );
    assert!(driver
        .execute(&BrowserCommand::UploadFile {
            selector: "#resume".into(),
            file_name: "missing.pdf".into(),
        })
        .is_err());
    driver
        .execute(&BrowserCommand::UploadFile {
            selector: "#resume".into(),
            file_name: "fictional.pdf".into(),
        })
        .unwrap();
    driver
        .verify_local_file("#resume", &evidence.content_root.join("fictional.pdf"))
        .unwrap();
    let screenshot = driver.screenshot_base64().unwrap();
    assert!(screenshot.starts_with("iVBOR") && screenshot.len() > 1000);
    std::fs::write(artifact, screenshot).unwrap();
    assert!(driver
        .commit()
        .unwrap_err()
        .to_string()
        .contains("unavailable in isolated rehearsal"));
    assert!(driver
        .trace
        .iter()
        .all(|line| !line.contains("Fictional Candidate")
            && !line.contains("fictional@example.invalid")));
    driver.execute(&BrowserCommand::Close).unwrap();
    assert!(
        !profile.join("AC").exists(),
        "AppContainer data was retained"
    );
}

#[test]
#[ignore = "requires an explicitly supplied engine; fictional loopback traffic only"]
fn isolated_scripted_channels_have_a_matching_unrestricted_baseline() {
    let (python, mut config, local) = setup();
    assert_eq!(
        std::fs::canonicalize(
            std::env::var_os("WAYPOINT_BROWSER").expect("explicit matched headless engine")
        )
        .unwrap(),
        std::fs::canonicalize(config.engine_root.join("chrome-headless-shell.exe")).unwrap(),
        "control must use the same engine as the isolated session"
    );
    let server = network::Receiver::start();
    let folder = config.content_root.parent().unwrap().join(format!(
        "channel-snapshot-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&folder).unwrap();
    let source = network::page(&server.origin);
    std::fs::write(folder.join("index.html"), &source).unwrap();
    config.content_root = folder.clone();
    config.content_files = [(
        "index.html".into(),
        waypoint_domain::sha256_hex(source.as_bytes()),
    )]
    .into();

    // Exact same scripts and URL endpoints must transmit in the ordinary
    // headless browser. Otherwise a broken fixture could look like isolation.
    let mut normal = CdpDriver::launch(vec![]).unwrap();
    let control_version = normal.browser_version().unwrap();
    let path = folder.to_string_lossy();
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(&path)
        .replace('\\', "/");
    normal.open(&format!("file:///{path}/index.html")).unwrap();
    let _ = normal.execute(&BrowserCommand::Fill {
        selector: "#name".into(),
        value: PacketValue::Text("Fictional".into()),
    });
    assert!(
        server.wait_for_baseline(),
        "incomplete unrestricted baseline: {:?}; browser={}; worker={:?}; worker_error={:?}",
        server.counts(),
        normal.browser_version().unwrap(),
        normal.read_attribute("#status", "data-worker").unwrap(),
        normal
            .read_attribute("#status", "data-worker-error")
            .unwrap()
    );
    normal.execute(&BrowserCommand::Close).unwrap();
    let before = server.counts();

    let (mut driver, evidence) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    assert_eq!(
        control_version,
        driver.browser_version().unwrap(),
        "control and isolated engines differ"
    );
    driver.open(&evidence.entry_url).unwrap();
    // Observing an attempted POST can itself stop filling. Delivery and
    // observation are different: only the receiving server decides this check.
    let _ = driver.execute(&BrowserCommand::Fill {
        selector: "#name".into(),
        value: PacketValue::Text("Fictional".into()),
    });
    assert_eq!(
        driver
            .read_attribute("#status", "data-fired")
            .unwrap()
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        driver
            .read_attribute("#status", "data-completed")
            .unwrap()
            .as_deref(),
        Some("true"),
        "isolated fixture did not attempt every scripted channel"
    );
    std::thread::sleep(std::time::Duration::from_millis(1500));
    driver.execute(&BrowserCommand::Close).unwrap();
    assert!(!local
        .join("Packages")
        .join(evidence.profile_name)
        .join("AC")
        .exists());
    assert_eq!(
        before,
        server.counts(),
        "isolated script traffic reached loopback"
    );
    server.finish();
}
