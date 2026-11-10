//! Explicit anonymous public-page collection only; no browser or candidate data.
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::{SystemTime, UNIX_EPOCH},
};
use waypoint_browser::capture_session::{
    CaptureBudget, CaptureOrigin, CaptureOutcome, CaptureSession,
};
use waypoint_policy::PermissionRegistry;

#[test]
#[ignore = "explicit permitted public reference/listing URL; performs bounded anonymous GETs"]
fn public_page_capture_obeys_policy_robots_and_freezes_original_bytes() {
    let url = std::env::var("WAYPOINT_CAPTURE_PUBLIC_URL").expect("explicit public URL");
    // This qualification never targets an employer's application endpoint.
    let source = match url.as_str() {
        "https://www.rfc-editor.org/rfc/rfc9309.html" => "user_supplied_url",
        "https://jobs.lever.co/spotify" => "lever",
        _ => panic!("Only the reviewed public reference and listing URLs qualify here"),
    };
    let target =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let folder = target.join(format!("public-capture-{}-{nonce}", std::process::id()));
    let origin = url::Url::parse(&url)
        .unwrap()
        .origin()
        .ascii_serialization();
    let budget = CaptureBudget {
        resources: 16,
        requests: 16,
        body_bytes: 2 * 1024 * 1024,
        received_bytes: 4 * 1024 * 1024,
        seconds: 45,
    };
    let mut capture = CaptureSession::start(
        &folder,
        &url,
        vec![CaptureOrigin {
            origin,
            source: source.into(),
        }],
        PermissionRegistry::from_json(waypoint_policy::INITIAL_REGISTRY_JSON).unwrap(),
        budget,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let result = capture.capture_entry();
    let recorded = matches!(result, CaptureOutcome::Recorded { status: 200, .. });
    let frozen = capture.freeze();
    assert!(
        recorded,
        "Public capture needs review: {result:?}; private report: {}",
        folder.display()
    );
    let frozen = frozen.expect("Complete public archive");
    assert!(!frozen.report.robots.is_empty());
    let entry = std::fs::read(folder.join("index.html")).unwrap();
    assert!(!entry.is_empty());
    assert_eq!(
        waypoint_domain::sha256_hex(&entry),
        frozen.snapshot.files["index.html"]
    );
    assert_eq!(
        frozen.report.coverage,
        "unassessed; missing resources and server-dependent steps require review"
    );
    println!("Public read-only capture: {}; {} request(s), {} recorded resource(s), {} entry bytes; no page scripts executed",folder.display(),frozen.report.requests,frozen.report.recorded_resources,entry.len());
}
