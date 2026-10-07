//! Live end-to-end journey through the public API only: discover real postings,
//! shortlist one, prepare an evidence-checked packet from the candidate's own
//! facts, approve it once, export a real DOCX, then correct a fact and confirm
//! the approval is voided while nothing submitted is touched.
//!
//! Run with: `cargo test -p waypoint_workspace --test live_journey -- --ignored --nocapture`

use waypoint_domain::{ApplicationState, JobIdentityId, Sha256Digest};
use waypoint_workspace::{DiscoveryTarget, FactOrigin, LiveFetcher, Workspace};

fn now() -> i64 {
    1_758_900_000
}

#[test]
#[ignore = "requires network access (public ATS boards)"]
fn live_full_journey_from_discovery_to_voided_approval() {
    let mut ws = Workspace::in_memory().expect("workspace");

    // 1. Real discovery over public boards.
    let outcome = ws.discover(
        &[
            DiscoveryTarget::new("greenhouse", "vercel"),
            DiscoveryTarget::new("ashby", "ashby"),
        ],
        &LiveFetcher::default(),
        now(),
    );
    println!("discovery: {}", outcome.summary());
    assert!(outcome.inserted > 10, "expected real postings");

    let job = ws
        .jobs()
        .expect("jobs")
        .into_iter()
        .next()
        .expect("at least one tracked job");
    let id = job.id.clone();
    println!("journey job: {} — {}", job.title, job.employer);

    // 2. Materials need facts; nothing can be approved from guesses.
    ws.set_state(&id, ApplicationState::Shortlisted)
        .expect("shortlist");
    let empty = ws.prepare_packet(&id, now()).expect("prepare");
    assert_eq!(empty.supported_spans, 0);
    assert!(!empty.is_approvable());
    assert!(
        ws.approve_packet(&id, "https://boards.greenhouse.io", &schema(), now())
            .is_err(),
        "an evidence-free packet must never be approvable"
    );

    // 3. The candidate states two facts.
    let first = ws
        .add_fact(
            "Built a Rust ingestion service handling 12k msg/s",
            FactOrigin::UserAttested,
        )
        .expect("fact 1");
    ws.add_fact(
        "Mentored four engineers through promotion",
        FactOrigin::UserAttested,
    )
    .expect("fact 2");

    let packet = ws.prepare_packet(&id, now() + 1).expect("prepare");
    println!(
        "packet {} — {} supported span(s), {} unsupported",
        &packet.packet_sha256[..12],
        packet.supported_spans,
        packet.unsupported.len()
    );
    assert!(packet.is_approvable(), "{:?}", packet.unsupported);
    assert!(packet.body.contains("Rust ingestion service"));

    // 4. Explicit approval, bound to packet + schema + origin.
    let grant = ws
        .approve_packet(&id, "https://boards.greenhouse.io", &schema(), now() + 2)
        .expect("approve");
    assert_eq!(
        ws.job(&id).unwrap().unwrap().state,
        ApplicationState::Approved
    );
    println!("approval {} live, unconsumed={}", grant.id, !grant.consumed);

    // 5. A real DOCX on disk.
    let dir = std::env::temp_dir().join(format!("wp-journey-{}", std::process::id()));
    let file = dir.join("journey.docx");
    let bytes = ws.export_packet(&id, &file).expect("export");
    let written = std::fs::read(&file).expect("read back");
    assert_eq!(&written[..2], b"PK");
    println!("exported {bytes} bytes to {}", file.display());
    let _ = std::fs::remove_dir_all(&dir);

    // 6. The candidate corrects a fact. Everything downstream must react.
    let report = ws
        .correct_fact(
            &first,
            "Built a Rust ingestion service at 18k msg/s",
            now() + 3,
        )
        .expect("correct");
    println!(
        "correction: revision {}, {} stale packet(s), {} voided approval(s), {} reopened, {} submitted untouched",
        report.new_profile_revision,
        report.invalidated_packets.len(),
        report.expired_grants.len(),
        report.reopened_to_prepared.len(),
        report.submitted_untouched.len()
    );
    assert_eq!(report.invalidated_packets, vec![id.as_str().to_string()]);
    assert_eq!(report.expired_grants.len(), 1);
    assert_eq!(
        ws.job(&id).unwrap().unwrap().state,
        ApplicationState::Prepared
    );

    let stale = ws.packet_readiness(&id).unwrap().unwrap();
    assert!(stale.invalidated_at.is_some(), "packet must read as stale");
    assert!(!stale.is_approvable());

    // The voided approval cannot be spent, even with perfect bindings.
    let payload = Sha256Digest::new(packet.packet_sha256.clone()).unwrap();
    assert!(
        ws.consume_grant(&id, &payload, &schema(), "https://boards.greenhouse.io")
            .is_err(),
        "a voided approval may never be spent"
    );

    // 7. Re-preparing picks up the corrected fact and becomes approvable again.
    let fresh = ws.prepare_packet(&id, now() + 4).expect("re-prepare");
    assert_ne!(fresh.packet_sha256, packet.packet_sha256);
    assert!(fresh.body.contains("18k msg/s"));
    assert!(!fresh.body.contains("12k msg/s"));
    assert!(ws
        .approve_packet(&id, "https://boards.greenhouse.io", &schema(), now() + 5)
        .is_ok());

    // 8. The alias graph and coverage survived the whole journey.
    assert!(ws.coverage().all_fresh_and_ok());
    let id_again = JobIdentityId::new(id.as_str().to_string()).unwrap();
    assert!(ws.job(&id_again).unwrap().is_some());
}

fn schema() -> Sha256Digest {
    Sha256Digest::of("waypoint-form-schema:greenhouse:v1")
}
