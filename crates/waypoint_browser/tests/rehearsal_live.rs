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

#[path = "support/rehearsal_relay.rs"]
pub mod relay;

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

fn assert_matching_control_engine(config: &RehearsalConfig) {
    use std::io::Read;
    let executable = std::fs::canonicalize(
        std::env::var_os("WAYPOINT_BROWSER").expect("explicit matched control engine"),
    )
    .unwrap();
    let root = executable.parent().unwrap();
    assert_eq!(executable.file_name().unwrap(), "chrome-headless-shell.exe");
    let mut total = 0u64;
    for (name, expected) in &config.engine_files {
        let relative = std::path::Path::new(name);
        assert!(
            !name.contains(['\\', ':'])
                && relative
                    .components()
                    .all(|p| matches!(p, std::path::Component::Normal(_)))
        );
        let file = std::fs::canonicalize(root.join(relative)).unwrap();
        assert!(file.starts_with(root));
        let size = std::fs::metadata(&file).unwrap().len();
        total += size;
        assert!(size <= 300 * 1024 * 1024 && total <= 600 * 1024 * 1024);
        let mut bytes = Vec::with_capacity(size as usize + 1);
        std::fs::File::open(file)
            .unwrap()
            .take(size + 1)
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes.len() as u64, size);
        assert_eq!(
            &waypoint_domain::sha256_hex(&bytes),
            expected,
            "control engine bytes differ: {name}"
        );
    }
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
    scripted_channel_qualification("127.0.0.1:0".parse().unwrap());
}

#[test]
#[ignore = "requires an explicitly supplied engine and IPv6 loopback; no external traffic"]
fn isolated_ipv6_channels_have_a_matching_unrestricted_baseline() {
    scripted_channel_qualification("[::1]:0".parse().unwrap());
}

fn scripted_channel_qualification(address: std::net::SocketAddr) {
    let (python, mut config, local) = setup();
    assert_matching_control_engine(&config);
    let server = network::Receiver::start_on(address);
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

fn start_relay_attempt(driver: &mut CdpDriver) {
    let _ = driver.execute(&BrowserCommand::Fill {
        selector: "#name".into(),
        value: PacketValue::Text("Fictional".into()),
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    loop {
        if driver
            .read_attribute("#status", "data-started")
            .unwrap()
            .as_deref()
            == Some("true")
        {
            return;
        }
        let error = driver.read_attribute("#status", "data-error").unwrap();
        assert!(
            error.is_none() && std::time::Instant::now() < deadline,
            "WebRTC relay attempt did not start: {error:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
#[ignore = "explicit matched headless engine; fictional IPv4/IPv6 UDP/TCP relay receivers only"]
fn isolated_webrtc_relay_channels_have_matching_unrestricted_baselines() {
    let (python, config, local) = setup();
    assert_matching_control_engine(&config);
    let root = config.content_root.parent().unwrap().to_owned();
    let mut observations = Vec::new();
    for address in ["127.0.0.1:0", "[::1]:0"] {
        for transport in [relay::Transport::Udp, relay::Transport::Tcp] {
            let receiver = relay::RelayFixture::start(address.parse().unwrap(), transport);
            let label = format!("{address}-{transport:?}");
            let folder = root.join(format!(
                "relay-snapshot-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&folder).unwrap();
            let source = relay::page(&receiver.url);
            std::fs::write(folder.join("index.html"), &source).unwrap();
            let mut selected = config.clone();
            selected.content_root = folder.clone();
            selected.content_files = [(
                "index.html".into(),
                waypoint_domain::sha256_hex(source.as_bytes()),
            )]
            .into();
            let mut normal = CdpDriver::launch(vec![]).unwrap();
            let version = normal.browser_version().unwrap();
            let display = folder.to_string_lossy();
            let display = display
                .strip_prefix(r"\\?\")
                .unwrap_or(&display)
                .replace('\\', "/");
            normal
                .open(&format!("file:///{display}/index.html"))
                .unwrap();
            start_relay_attempt(&mut normal);
            assert!(
                receiver.wait_for_baseline(),
                "missing unrestricted TURN allocation baseline: {label}; {:?}",
                receiver.counts()
            );
            normal.execute(&BrowserCommand::Close).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(200));
            let baseline = receiver.counts();
            let (mut driver, evidence) = CdpDriver::launch_rehearsal(&python, selected).unwrap();
            assert_eq!(version, driver.browser_version().unwrap());
            driver.open(&evidence.entry_url).unwrap();
            start_relay_attempt(&mut driver);
            std::thread::sleep(std::time::Duration::from_millis(1500));
            driver.execute(&BrowserCommand::Close).unwrap();
            let after = receiver.counts();
            assert_eq!(
                baseline, after,
                "isolated relay traffic reached loopback: {label}"
            );
            assert!(!local
                .join("Packages")
                .join(evidence.profile_name)
                .join("AC")
                .exists());
            observations.push(serde_json::json!({"case":label,"control_version":version,"baseline":baseline,"after_isolation":after}));
            receiver.finish();
        }
    }
    std::fs::write(
        root.join("relay-observations.private.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
}

/// Collect observations only. The opt-in Python controller must independently
/// verify receiver history and both controls before claiming protection.
#[test]
#[ignore = "requires an explicitly created temporary public receiver; no employer site or real data"]
fn public_https_canary_observations() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Canary {
        schema: u32,
        root_url: String,
        nonce: String,
    }
    let (python, config, local) = setup();
    assert_matching_control_engine(&config);
    let root = config.content_root.parent().unwrap();
    let spec = std::fs::canonicalize(
        std::env::var_os("WAYPOINT_INTERNET_CANARY_CONFIG").expect("explicit temporary receiver"),
    )
    .unwrap();
    assert!(spec.starts_with(std::fs::canonicalize(root).unwrap()));
    let raw = std::fs::read(spec).unwrap();
    assert!(raw.len() <= 4096);
    assert_eq!(
        waypoint_domain::sha256_hex(&raw),
        std::env::var("WAYPOINT_INTERNET_CANARY_SHA256").unwrap()
    );
    let canary: Canary = serde_json::from_slice(&raw).unwrap();
    assert_eq!(canary.schema, 1);
    let id = canary
        .root_url
        .strip_prefix("https://webhook.site/")
        .expect("fixed public receiver service");
    let parts: Vec<_> = id.split('-').collect();
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12]
    );
    assert!(parts
        .iter()
        .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit())));
    assert!(canary.nonce.len() == 32 && canary.nonce.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut observations = Vec::new();
    let mut control_version = None;
    for stage in ["control_before", "isolated", "control_after"] {
        let url = format!("{}?stage={stage}&probe={}", canary.root_url, canary.nonce);
        let body = format!("fictional-qualification:{stage}:{}", canary.nonce);
        let source = format!(
            r#"<!doctype html><meta charset="utf-8"><label for="name">Fictional name</label><input id="name"><span id="status"></span>
<script>document.querySelector('#name').addEventListener('input',()=>{{const status=document.querySelector('#status');if(status.dataset.invoked)return;status.dataset.invoked='true';fetch({},{{method:'POST',mode:'cors',body:{}}}).then(response=>status.dataset.result=response.ok?'delivered':'failed').catch(()=>status.dataset.result='failed');}});</script>"#,
            serde_json::to_string(&url).unwrap(),
            serde_json::to_string(&body).unwrap()
        );
        let folder = root.join(format!(
            "internet-{stage}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("index.html"), &source).unwrap();
        let mut selected = config.clone();
        selected.content_root = folder.clone();
        selected.content_files = [(
            "index.html".into(),
            waypoint_domain::sha256_hex(source.as_bytes()),
        )]
        .into();
        let (mut driver, evidence) = if stage == "isolated" {
            let (driver, evidence) = CdpDriver::launch_rehearsal(&python, selected).unwrap();
            (driver, Some(evidence))
        } else {
            (CdpDriver::launch(vec![]).unwrap(), None)
        };
        let version = driver.browser_version().unwrap();
        if let Some(expected) = &control_version {
            assert_eq!(&version, expected);
        } else {
            control_version = Some(version.clone());
        }
        let display = folder.to_string_lossy();
        let display = display
            .strip_prefix(r"\\?\")
            .unwrap_or(&display)
            .replace('\\', "/");
        let entry = evidence
            .as_ref()
            .map(|e| e.entry_url.clone())
            .unwrap_or_else(|| format!("file:///{display}/index.html"));
        driver.open(&entry).unwrap();
        let _ = driver.execute(&BrowserCommand::Fill {
            selector: "#name".into(),
            value: PacketValue::Text("Fictional".into()),
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let result = loop {
            if let Some(result) = driver.read_attribute("#status", "data-result").unwrap() {
                break result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "public probe did not complete: {stage}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        driver.execute(&BrowserCommand::Close).unwrap();
        if let Some(evidence) = evidence {
            assert!(!local
                .join("Packages")
                .join(evidence.profile_name)
                .join("AC")
                .exists());
        }
        observations.push(serde_json::json!({"stage":stage,"result":result,"version":version}));
        std::fs::write(
            root.join("internet-browser.private.json"),
            serde_json::to_vec_pretty(&observations).unwrap(),
        )
        .unwrap();
    }
}
