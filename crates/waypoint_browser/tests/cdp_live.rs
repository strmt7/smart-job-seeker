//! Live CDP tests: a real browser, the real DOM, real input events — against
//! local fixture pages, so nothing ever touches a third-party site.
//!
//! Run with: `cargo test -p waypoint_browser --test cdp_live -- --ignored --nocapture`
//!
//! This is the evidence that the submission engine works against an actual
//! browser, not only against a scripted driver.

use std::path::PathBuf;

use waypoint_applications::driver::BrowserDriver;
use waypoint_applications::form_mapping::FormFieldType;
use waypoint_browser::CdpDriver;
use waypoint_domain::{ApplicationState, JobIdentityId, Sha256Digest};
use waypoint_store::StoredJob;
use waypoint_workspace::{live_form_schema_digest, FactOrigin, SubmitOutcome, Workspace};

const NOW: i64 = 1_758_900_000;
/// The name the fill plan asks for; the driver resolves it in the upload dir.
const PACKET_FILE: &str = "waypoint-packet.docx";

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A private directory for the packet file, so tests never write into the repo.
fn upload_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wp-uploads-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("upload dir");
    dir
}

fn file_url(name: &str) -> String {
    format!(
        "file:///{}",
        fixtures_dir()
            .join(name)
            .display()
            .to_string()
            .replace('\\', "/")
    )
}

fn job(id: &str, title: &str, employer: &str, url: &str) -> StoredJob {
    StoredJob {
        id: JobIdentityId::new(id).unwrap(),
        title: title.into(),
        employer: employer.into(),
        location: "Remote".into(),
        state: ApplicationState::Discovered,
        canonical_url: Some(url.to_string()),
        content_hash: Some(Sha256Digest::of(url).as_str().to_string()),
        first_seen: Some(NOW.to_string()),
        last_seen: Some(NOW.to_string()),
        remote: true,
        source: "user_supplied_url".into(),
        requisition_id: Some(id.to_string()),
        posted_at: None,
    }
}

/// Job → fact → shortlist → packet → approval bound to the live form.
fn prepared_for(url: &str, fact: &str) -> (Workspace, JobIdentityId) {
    let mut ws = Workspace::in_memory().unwrap();
    ws.import_job(job("fixture::1", "Backend Engineer", "acme", url))
        .unwrap();
    let id = JobIdentityId::new("fixture::1").unwrap();
    ws.add_fact(fact, FactOrigin::UserAttested).unwrap();
    ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
    let packet = ws.prepare_packet(&id, NOW).unwrap();
    assert!(packet.is_approvable(), "{:?}", packet.unsupported);
    (ws, id)
}

#[test]
#[ignore = "launches a real browser (Edge/Chrome)"]
fn the_schema_comes_from_the_real_page() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    println!("browser: {}", driver.browser_version().unwrap());

    let live = driver
        .open(&file_url("form_ok.html"))
        .expect("open fixture");
    let labels: Vec<&str> = live
        .schema
        .fields
        .iter()
        .map(|f| f.label.as_str())
        .collect();
    println!("fields discovered: {labels:?} walls: {:?}", live.walls);
    assert!(live.walls.is_empty(), "no walls expected: {:?}", live.walls);
    assert_eq!(live.schema.fields.len(), 2, "got {:?}", live.schema.fields);
    assert!(labels.contains(&"Cover letter"));
    assert!(labels.contains(&"Resume"));

    let cover = live
        .schema
        .fields
        .iter()
        .find(|f| f.label == "Cover letter")
        .unwrap();
    assert_eq!(cover.field_type, FormFieldType::TextArea);
    assert!(cover.required, "the fixture marks it required");
    assert!(!cover.sensitive, "a cover letter is not a sensitive field");

    let resume = live
        .schema
        .fields
        .iter()
        .find(|f| f.label == "Resume")
        .unwrap();
    assert_eq!(resume.field_type, FormFieldType::File);
    assert!(resume.required);

    // Schema identity must be stable across reads: approvals bind to it, so an
    // unstable hash would void every approval for no reason.
    let again = driver.open(&file_url("form_ok.html")).expect("re-open");
    assert_eq!(
        live_form_schema_digest(&live).as_str(),
        live_form_schema_digest(&again).as_str(),
        "reading the same page twice must produce the same schema identity"
    );
}

#[test]
#[ignore = "launches a real browser (Edge/Chrome)"]
fn a_real_browser_submits_the_packet_and_returns_a_qualified_receipt() {
    let url = file_url("form_ok.html");
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");

    let live = driver.open(&url).expect("open fixture");
    assert!(live.walls.is_empty());
    let (mut ws, id) = prepared_for(&url, "Built a Rust ingestion service at 12k msg/s");

    let digest = live_form_schema_digest(&live);
    ws.approve_packet(&id, &live.origin, &digest, NOW).unwrap();
    let export = upload_dir().join(PACKET_FILE);
    ws.export_packet(&id, &export).expect("export packet");
    assert!(export.is_file(), "the packet file must exist for upload");

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 1).unwrap();
    println!("outcome: {outcome:?}");
    println!("driver trace: {:#?}", driver.trace);
    for line in ws.submission_history(&id).unwrap() {
        println!(
            "journal: {} {:?} {}",
            line.timestamp, line.state, line.reason
        );
    }

    match outcome {
        SubmitOutcome::Confirmed { application_ref } => {
            assert_eq!(application_ref, "GH-44117", "the page's own reference")
        }
        other => panic!("expected a confirmed submission, got {other:?}"),
    }
    assert_eq!(
        ws.job(&id).unwrap().unwrap().state,
        ApplicationState::Confirmed
    );
    let _ = std::fs::remove_file(&export);
}

#[test]
#[ignore = "launches a real browser (Edge/Chrome)"]
fn the_typed_text_and_the_attached_file_actually_reach_the_page() {
    let url = file_url("form_echo.html");
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");

    let live = driver.open(&url).expect("open fixture");
    let (mut ws, id) = prepared_for(&url, "Mentored four engineers through promotion");
    let digest = live_form_schema_digest(&live);
    ws.approve_packet(&id, &live.origin, &digest, NOW).unwrap();
    let export = upload_dir().join(PACKET_FILE);
    ws.export_packet(&id, &export).unwrap();

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 1).unwrap();
    let _ = std::fs::remove_file(&export);
    println!("outcome: {outcome:?}");
    println!("driver trace: {:#?}", driver.trace);

    // The fixture derives its reference from the number of characters it
    // actually received, so a confirmed outcome proves the typed text arrived.
    match outcome {
        SubmitOutcome::Confirmed { application_ref } => {
            assert!(
                application_ref.starts_with("ECHO-"),
                "unexpected reference {application_ref}"
            );
            let received: usize = application_ref
                .trim_start_matches("ECHO-")
                .parse()
                .expect("numeric suffix");
            let expected = received - 10000;
            assert!(
                expected > 20,
                "the page must have received real text, got {expected} chars"
            );
            println!("page received {expected} characters of the packet");
        }
        other => panic!("expected a confirmed submission, got {other:?}"),
    }
}

#[test]
#[ignore = "launches a real browser (Edge/Chrome)"]
fn a_captcha_on_a_real_page_is_reported_as_a_wall() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let live = driver
        .open(&file_url("form_captcha.html"))
        .expect("open fixture");
    println!("walls: {:?}", live.walls);
    assert!(
        live.walls.iter().any(|w| w.contains("captcha")),
        "the captcha must be reported: {:?}",
        live.walls
    );
}

#[test]
#[ignore = "launches a real browser (Edge/Chrome)"]
fn a_changed_page_voids_the_approval_against_a_real_site() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    // Approve against the simple form...
    let original = driver.open(&file_url("form_ok.html")).expect("open v1");
    let digest = live_form_schema_digest(&original);

    let (mut ws, id) = prepared_for(&file_url("form_ok.html"), "Ran a 40-service migration");
    ws.approve_packet(&id, &original.origin, &digest, NOW)
        .unwrap();
    let export = upload_dir().join(PACKET_FILE);
    ws.export_packet(&id, &export).unwrap();

    // ...then the posting's page has grown an extra field.
    let mut changed = ws.job(&id).unwrap().unwrap();
    changed.canonical_url = Some(file_url("form_v2.html"));
    ws.import_job(changed).unwrap();

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 1).unwrap();
    let _ = std::fs::remove_file(&export);
    println!("outcome: {outcome:?}");
    match outcome {
        SubmitOutcome::Refused {
            issues,
            approval_voided,
        } => {
            assert!(approval_voided, "a changed form must void the approval");
            assert!(
                issues.iter().any(|i| i.contains("form changed")),
                "{issues:?}"
            );
        }
        other => panic!("expected a refusal against the changed form, got {other:?}"),
    }
    assert_eq!(
        ws.job(&id).unwrap().unwrap().state,
        ApplicationState::Prepared,
        "reopened for re-approval"
    );
    assert!(ws.grant(&id).unwrap().unwrap().expired);
}
