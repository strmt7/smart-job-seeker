//! Tests for the "act on a job" workflow. These assert the product's D1/D2
//! contracts end to end: correction propagation and single-use approvals,
//! through persistence, not just in isolated helpers.

use super::*;
use waypoint_domain::{ApplicationState, Sha256Digest};

const NOW: i64 = 1_758_900_000;

fn digest(byte: char) -> Sha256Digest {
    Sha256Digest::new(byte.to_string().repeat(64)).unwrap()
}

/// A workspace with two tracked jobs, inserted directly (this suite is about
/// the work layer, not discovery).
fn workspace_with_two_jobs() -> (Workspace, JobIdentityId, JobIdentityId) {
    let mut ws = Workspace::in_memory().unwrap();
    ws.test_insert_job("greenhouse::req::1", "Backend Engineer", "acme");
    ws.test_insert_job("greenhouse::req::2", "Data Engineer", "bcorp");
    let j1 = JobIdentityId::new("greenhouse::req::1").unwrap();
    let j2 = JobIdentityId::new("greenhouse::req::2").unwrap();
    (ws, j1, j2)
}

fn job_state(ws: &Workspace, id: &JobIdentityId) -> ApplicationState {
    ws.job(id).unwrap().unwrap().state
}

#[test]
fn prepare_requires_shortlisting_first() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.add_fact(
        "Nine years building distributed systems",
        FactOrigin::UserAttested,
    )
    .unwrap();

    // Discovered -> Prepared is not a legal step.
    let err = ws.prepare_packet(&j1, NOW).unwrap_err();
    assert!(
        matches!(err, WorkspaceError::IllegalTransition { .. }),
        "{err:?}"
    );

    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    let packet = ws.prepare_packet(&j1, NOW).unwrap();
    assert_eq!(job_state(&ws, &j1), ApplicationState::Prepared);
    assert_eq!(packet.supported_spans, 1);
    assert!(packet.unsupported.is_empty());
    assert!(packet
        .body
        .contains("Nine years building distributed systems"));
    assert!(packet.body.contains("Backend Engineer"));
}

#[test]
fn packet_is_content_addressed_so_a_fact_change_produces_a_new_packet() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    let first_id = ws
        .add_fact("Eight years of Rust", FactOrigin::UserAttested)
        .unwrap();
    let first = ws.prepare_packet(&j1, NOW).unwrap();

    let report = ws
        .correct_fact(&first_id, "Nine years of Rust", NOW + 10)
        .unwrap();
    assert_eq!(report.invalidated_packets, vec![j1.as_str().to_string()]);
    assert!(report.new_profile_revision > 1);

    let second = ws.prepare_packet(&j1, NOW + 20).unwrap();
    assert_ne!(first.packet_sha256, second.packet_sha256, "content changed");
    assert!(second.body.contains("Nine years of Rust"));
    assert!(!second.body.contains("Eight years of Rust"));
    assert!(second.cited_claims.contains(&report.new_claim_id));
    assert!(
        !second.cited_claims.contains(&first_id),
        "old fact no longer cited"
    );

    // Re-preparing unchanged content is idempotent.
    let third = ws.prepare_packet(&j1, NOW + 30).unwrap();
    assert_eq!(second.packet_sha256, third.packet_sha256);
}

#[test]
fn an_empty_packet_can_never_be_approved() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    let packet = ws.prepare_packet(&j1, NOW).unwrap();
    assert_eq!(packet.supported_spans, 0);
    assert!(!packet.is_approvable());

    let err = ws
        .approve_packet(&j1, "https://boards.greenhouse.io", &digest('a'), NOW)
        .unwrap_err();
    match err {
        WorkspaceError::UnsupportedClaims(reasons) => {
            assert!(reasons[0].contains("no attested facts"), "{reasons:?}")
        }
        other => panic!("expected unsupported-claims refusal, got {other:?}"),
    }
    assert_eq!(job_state(&ws, &j1), ApplicationState::Prepared);
    assert!(ws.grant(&j1).unwrap().is_none(), "no grant may be created");
}

#[test]
fn a_model_inferred_fact_cannot_support_an_approval_until_confirmed() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    let inferred = ws
        .add_fact("Probably knows Kubernetes", FactOrigin::ModelInferred)
        .unwrap();

    let packet = ws.prepare_packet(&j1, NOW).unwrap();
    assert_eq!(packet.supported_spans, 0);
    assert!(
        packet
            .unsupported
            .iter()
            .any(|r| r.to_lowercase().contains("infer")),
        "the reason must name the inferred status: {:?}",
        packet.unsupported
    );
    assert!(ws
        .approve_packet(&j1, "https://boards.greenhouse.io", &digest('a'), NOW)
        .is_err());

    // The candidate confirms it: the inferred claim is superseded by an
    // attested one, and only then does approval become possible.
    ws.correct_fact(&inferred, "Knows Kubernetes (confirmed)", NOW + 5)
        .unwrap();
    let confirmed = ws.prepare_packet(&j1, NOW + 10).unwrap();
    assert_eq!(
        confirmed.supported_spans,
        1,
        "{}",
        confirmed.unsupported.join("; ")
    );
    assert!(ws
        .approve_packet(&j1, "https://boards.greenhouse.io", &digest('a'), NOW + 20)
        .is_ok());
}

#[test]
fn approval_is_bound_to_packet_schema_and_origin_and_is_single_use() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    ws.add_fact("Led migration of 40 services", FactOrigin::UserAttested)
        .unwrap();
    let packet = ws.prepare_packet(&j1, NOW).unwrap();
    let grant = ws
        .approve_packet(&j1, "https://boards.greenhouse.io", &digest('a'), NOW)
        .unwrap();
    assert_eq!(grant.packet_sha256, packet.packet_sha256);
    assert!(grant.explicit_user_gesture);
    assert_eq!(job_state(&ws, &j1), ApplicationState::Approved);

    let payload = Sha256Digest::new(packet.packet_sha256.clone()).unwrap();

    // Wrong origin: refused, and crucially the refusal does not spend it.
    let err = ws
        .consume_grant(&j1, &payload, &digest('a'), "https://evil.example")
        .unwrap_err();
    assert!(matches!(err, WorkspaceError::Approval(_)), "{err:?}");
    assert!(
        !ws.grant(&j1).unwrap().unwrap().consumed,
        "refusal keeps it live"
    );

    // Correct binding: spent exactly once.
    ws.consume_grant(&j1, &payload, &digest('a'), "https://boards.greenhouse.io")
        .unwrap();
    assert!(ws.grant(&j1).unwrap().unwrap().consumed);

    let second = ws
        .consume_grant(&j1, &payload, &digest('a'), "https://boards.greenhouse.io")
        .unwrap_err();
    assert!(matches!(second, WorkspaceError::Approval(_)), "{second:?}");
}

#[test]
fn a_stale_form_schema_is_refused_even_with_a_perfect_payload() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    ws.add_fact(
        "Ten years in platform engineering",
        FactOrigin::UserAttested,
    )
    .unwrap();
    let packet = ws.prepare_packet(&j1, NOW).unwrap();
    ws.approve_packet(&j1, "https://boards.greenhouse.io", &digest('a'), NOW)
        .unwrap();

    let payload = Sha256Digest::new(packet.packet_sha256.clone()).unwrap();
    // The form changed since approval: the grant no longer matches.
    let err = ws
        .consume_grant(&j1, &payload, &digest('b'), "https://boards.greenhouse.io")
        .unwrap_err();
    assert!(matches!(err, WorkspaceError::Approval(_)), "{err:?}");
    assert!(!ws.grant(&j1).unwrap().unwrap().consumed);
}

/// The flagship: correcting one fact must expire exactly the approvals that
/// depended on it, reopen exactly those jobs, and leave everything else alone.
#[test]
fn correcting_a_fact_expires_only_the_affected_approval_and_reopens_that_job() {
    let (mut ws, j1, j2) = workspace_with_two_jobs();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    ws.set_state(&j2, ApplicationState::Shortlisted).unwrap();

    // A fact both jobs will cite...
    let shared = ws
        .add_fact("Fluent in German (C1)", FactOrigin::UserAttested)
        .unwrap();
    // ...and a second job prepared/approved while only that fact existed.
    ws.prepare_packet(&j2, NOW).unwrap();
    ws.approve_packet(&j2, "https://boards.greenhouse.io", &digest('a'), NOW)
        .unwrap();

    // A later fact that only job j1's packet will cite.
    let only_j1 = ws
        .add_fact("Built a payments ledger", FactOrigin::UserAttested)
        .unwrap();
    ws.prepare_packet(&j1, NOW + 5).unwrap();
    ws.approve_packet(&j1, "https://boards.greenhouse.io", &digest('a'), NOW + 5)
        .unwrap();

    // Correcting a fact cited only by j1 must not disturb j2 at all.
    let report = ws
        .correct_fact(&only_j1, "Built a payments ledger at scale", NOW + 100)
        .unwrap();
    assert_eq!(report.invalidated_packets, vec![j1.as_str().to_string()]);
    assert_eq!(report.expired_grants.len(), 1);
    assert_eq!(report.reopened_to_prepared, vec![j1.as_str().to_string()]);
    assert!(report.submitted_untouched.is_empty());
    assert_eq!(job_state(&ws, &j1), ApplicationState::Prepared, "reopened");
    assert!(ws.grant(&j1).unwrap().unwrap().expired, "approval voided");
    assert_eq!(job_state(&ws, &j2), ApplicationState::Approved, "untouched");
    assert!(!ws.grant(&j2).unwrap().unwrap().expired);

    // The voided approval cannot be spent even with perfect bindings.
    let old_payload = ws.packet(&j1).unwrap().unwrap().packet_sha256;
    let err = ws
        .consume_grant(
            &j1,
            &Sha256Digest::new(old_payload).unwrap(),
            &digest('a'),
            "https://boards.greenhouse.io",
        )
        .unwrap_err();
    assert!(matches!(err, WorkspaceError::Approval(_)), "{err:?}");

    // Correcting the *shared* fact must now reach both jobs: j1 is re-prepared
    // (no live grant) and j2's live approval is voided and reopened.
    let second = ws
        .correct_fact(&shared, "Fluent in German (C2)", NOW + 200)
        .unwrap();
    // j1's packet was already stale from the first correction, so only j2's is
    // newly invalidated — re-reporting a known-stale packet would be noise.
    assert_eq!(second.invalidated_packets, vec![j2.as_str().to_string()]);
    assert!(ws.packet(&j1).unwrap().unwrap().invalidated_at.is_some());
    assert!(ws.packet(&j2).unwrap().unwrap().invalidated_at.is_some());
    assert_eq!(
        second.expired_grants.len(),
        1,
        "only j2 still held a live grant"
    );
    assert_eq!(second.reopened_to_prepared, vec![j2.as_str().to_string()]);
    assert_eq!(job_state(&ws, &j2), ApplicationState::Prepared);
}

#[test]
fn submitted_applications_are_recorded_but_never_modified() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    let fact = ws
        .add_fact(
            "Certified Kubernetes administrator",
            FactOrigin::UserAttested,
        )
        .unwrap();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    ws.prepare_packet(&j1, NOW).unwrap();
    ws.approve_packet(&j1, "https://boards.greenhouse.io", &digest('a'), NOW)
        .unwrap();
    ws.set_state(&j1, ApplicationState::Submitting).unwrap();
    ws.set_state(&j1, ApplicationState::Confirmed).unwrap();
    let packet_before = ws.packet(&j1).unwrap().unwrap();

    let report = ws
        .correct_fact(&fact, "Certified Kubernetes administrator (CKAD)", NOW + 50)
        .unwrap();

    assert_eq!(report.submitted_untouched, vec![j1.as_str().to_string()]);
    assert!(
        report.invalidated_packets.is_empty(),
        "a sent packet is not rewritten"
    );
    assert!(report.expired_grants.is_empty());
    assert_eq!(
        job_state(&ws, &j1),
        ApplicationState::Confirmed,
        "state untouched"
    );
    assert_eq!(ws.packet(&j1).unwrap().unwrap(), packet_before);
    assert!(!ws.grant(&j1).unwrap().unwrap().expired);
}

#[test]
fn docx_export_is_real_ooxml_and_deterministic() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    ws.add_fact(
        "Shipped a Rust service at 20k req/s",
        FactOrigin::UserAttested,
    )
    .unwrap();
    ws.prepare_packet(&j1, NOW).unwrap();

    let dir = std::env::temp_dir().join(format!("wp-export-{}", std::process::id()));
    let out = dir.join("packet.docx");
    let bytes = ws.export_packet(&j1, &out).unwrap();
    assert!(bytes > 500, "a real docx is not tiny: {bytes} bytes");
    let written = std::fs::read(&out).unwrap();
    assert_eq!(&written[..2], b"PK", "OOXML is a zip container");
    assert_eq!(written.len(), bytes);

    // Deterministic: exporting twice yields identical bytes.
    let out2 = dir.join("packet2.docx");
    ws.export_packet(&j1, &out2).unwrap();
    assert_eq!(written, std::fs::read(&out2).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_requires_a_prepared_packet_but_works_without_approval() {
    let (mut ws, j1, _) = workspace_with_two_jobs();
    ws.add_fact("Something true", FactOrigin::UserAttested)
        .unwrap();
    let err = ws
        .export_packet(&j1, &std::env::temp_dir().join("nope.docx"))
        .unwrap_err();
    assert!(matches!(err, WorkspaceError::PacketRequired(_)), "{err:?}");

    ws.set_state(&j1, ApplicationState::Shortlisted).unwrap();
    ws.prepare_packet(&j1, NOW).unwrap();
    let dir = std::env::temp_dir().join(format!("wp-export-b-{}", std::process::id()));
    let out = dir.join("p.docx");
    assert!(
        ws.export_packet(&j1, &out).is_ok(),
        "export needs no approval"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn facts_must_have_text_and_unknown_facts_are_rejected() {
    let (mut ws, _, _) = workspace_with_two_jobs();
    assert!(matches!(
        ws.add_fact("   ", FactOrigin::UserAttested).unwrap_err(),
        WorkspaceError::EmptyFact
    ));
    let err = ws.correct_fact("c-does-not-exist", "x", NOW).unwrap_err();
    assert!(matches!(err, WorkspaceError::UnknownFact(_)), "{err:?}");
}

#[test]
fn fact_history_keeps_the_superseded_revision() {
    let (mut ws, _, _) = workspace_with_two_jobs();
    let first = ws
        .add_fact("Seven years of Kotlin", FactOrigin::UserAttested)
        .unwrap();
    ws.correct_fact(&first, "Eight years of Kotlin", NOW)
        .unwrap();

    assert_eq!(ws.facts().unwrap().len(), 1, "one fact in force");
    let history = ws.fact_history().unwrap();
    assert_eq!(history.len(), 2, "the superseded revision is retained");
    assert!(history.iter().any(|c| c.text == "Seven years of Kotlin"));
    assert!(history
        .iter()
        .any(|c| c.supersedes.as_deref() == Some(first.as_str())));
}
