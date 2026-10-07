//! Submission-stage tests. The scripted driver replaces the browser, so the
//! whole supervised-apply flow is exercised without a real site — and every
//! refusal path is asserted to send *nothing*.

use super::*;
use waypoint_applications::form_mapping::{
    BrowserCommand, FormField, FormFieldType, FormSchema, PacketValue,
};
use waypoint_applications::receipt::ReceiptKind;
use waypoint_domain::{ApplicationState, JobIdentityId, Sha256Digest};

const NOW: i64 = 1_758_900_000;
const ORIGIN: &str = "https://boards.greenhouse.io";

/// A form the packet can genuinely answer: a cover-letter prompt and a résumé
/// upload, neither sensitive.
fn answerable_schema() -> FormSchema {
    FormSchema {
        origin: ORIGIN.into(),
        fields: vec![
            FormField {
                selector: "#cover".into(),
                label: "Cover letter".into(),
                field_type: FormFieldType::TextArea,
                required: true,
                sensitive: false,
            },
            FormField {
                selector: "#resume".into(),
                label: "Resume".into(),
                field_type: FormFieldType::File,
                required: true,
                sensitive: false,
            },
        ],
        multi_step: false,
        steps: 1,
    }
}

fn live(schema: FormSchema) -> LiveForm {
    LiveForm {
        origin: schema.origin.clone(),
        schema,
        walls: vec![],
    }
}

#[derive(Default)]
struct ScriptedDriver {
    live: Option<LiveForm>,
    ack: Option<Result<SiteAck, DriverError>>,
    executed: Vec<BrowserCommand>,
    commits: usize,
    opened: Vec<String>,
}

impl ScriptedDriver {
    fn new(form: LiveForm, ack: Result<SiteAck, DriverError>) -> Self {
        Self {
            live: Some(form),
            ack: Some(ack),
            ..Default::default()
        }
    }
}

impl BrowserDriver for ScriptedDriver {
    fn open(&mut self, url: &str) -> Result<LiveForm, DriverError> {
        self.opened.push(url.to_string());
        self.live
            .clone()
            .ok_or_else(|| DriverError("no page scripted".into()))
    }

    fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        self.executed.push(command.clone());
        Ok(())
    }

    fn commit(&mut self) -> Result<SiteAck, DriverError> {
        self.commits += 1;
        self.ack
            .clone()
            .unwrap_or_else(|| Err(DriverError("no ack scripted".into())))
    }
}

fn qualified_ack() -> SiteAck {
    SiteAck {
        receipt_kind: ReceiptKind::ConfirmationPageWithId,
        payload: "GH-APP-88301".into(),
    }
}

/// A workspace with one job, one attested fact, a prepared packet and an
/// approval bound to `schema`.
fn approved_for(schema: &FormSchema) -> (Workspace, JobIdentityId) {
    let mut ws = Workspace::in_memory().unwrap();
    ws.test_insert_job("greenhouse::req::77", "Backend Engineer", "acme");
    let id = JobIdentityId::new("greenhouse::req::77").unwrap();
    ws.add_fact(
        "Shipped a Rust service at 20k req/s",
        FactOrigin::UserAttested,
    )
    .unwrap();
    ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
    ws.prepare_packet(&id, NOW).unwrap();
    let digest = Sha256Digest::of(&schema.schema_hash_input());
    ws.approve_packet(&id, ORIGIN, &digest, NOW).unwrap();
    (ws, id)
}

fn state(ws: &Workspace, id: &JobIdentityId) -> ApplicationState {
    ws.job(id).unwrap().unwrap().state
}

#[test]
fn a_qualified_submission_confirms_once_and_records_the_whole_history() {
    let schema = answerable_schema();
    let (mut ws, id) = approved_for(&schema);
    let mut driver = ScriptedDriver::new(live(schema), Ok(qualified_ack()));

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 10).unwrap();
    match outcome {
        SubmitOutcome::Confirmed { application_ref } => assert_eq!(application_ref, "GH-APP-88301"),
        other => panic!("expected Confirmed, got {other:?}"),
    }

    // The write happened exactly once, and the page was actually inspected.
    assert_eq!(driver.commits, 1);
    assert_eq!(driver.opened.len(), 1);
    assert!(driver.opened[0].contains("greenhouse"));

    // Fields were filled with typed commands only: the packet body into the
    // cover-letter prompt, the packet file into the résumé upload.
    assert_eq!(driver.executed.len(), 2);
    let has_upload = driver
        .executed
        .iter()
        .any(|c| matches!(c, BrowserCommand::UploadFile { selector, .. } if selector == "#resume"));
    assert!(has_upload, "{:?}", driver.executed);
    let cover = driver.executed.iter().find_map(|c| match c {
        BrowserCommand::Fill { value, .. } => Some(value.clone()),
        _ => None,
    });
    assert!(
        matches!(cover, Some(PacketValue::Text(t)) if t.contains("20k req/s")),
        "the packet body must be what gets filled"
    );

    // State and history are exactly what happened.
    assert_eq!(state(&ws, &id), ApplicationState::Confirmed);
    let history = ws.submission_history(&id).unwrap();
    let reasons: Vec<&str> = history.iter().map(|r| r.reason.as_str()).collect();
    assert_eq!(
        reasons,
        vec!["preflight_ok", "write_started", "receipt_confirmed"]
    );
    assert!(ws.grant(&id).unwrap().unwrap().consumed);

    // A second attempt cannot reuse the moment: the job is no longer Approved.
    let mut again = ScriptedDriver::new(live(answerable_schema()), Ok(qualified_ack()));
    assert!(matches!(
        ws.submit_packet(&id, &mut again, NOW + 20).unwrap_err(),
        WorkspaceError::IllegalTransition { .. }
    ));
    assert_eq!(again.commits, 0, "a refused attempt must not write");
}

#[test]
fn a_changed_form_voids_the_approval_and_sends_nothing() {
    let granted = answerable_schema();
    let (mut ws, id) = approved_for(&granted);

    // The live form differs (the site added a field).
    let mut changed = answerable_schema();
    changed.fields.push(FormField {
        selector: "#portfolio".into(),
        label: "Portfolio URL".into(),
        field_type: FormFieldType::Text,
        required: false,
        sensitive: false,
    });
    let mut driver = ScriptedDriver::new(live(changed), Ok(qualified_ack()));

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 10).unwrap();
    match outcome {
        SubmitOutcome::Refused {
            issues,
            approval_voided,
        } => {
            assert!(approval_voided, "a schema mismatch must void the approval");
            assert!(
                issues.iter().any(|i| i.contains("form changed")),
                "{issues:?}"
            );
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(driver.commits, 0, "nothing may be sent");
    assert!(driver.executed.is_empty(), "nothing may even be typed in");
    assert_eq!(state(&ws, &id), ApplicationState::Prepared, "reopened");
    assert!(ws.grant(&id).unwrap().unwrap().expired);
}

#[test]
fn a_sensitive_field_is_never_auto_answered_and_does_not_burn_the_approval() {
    let mut schema = answerable_schema();
    schema.fields.push(FormField {
        selector: "#salary".into(),
        label: "Salary expectation".into(),
        field_type: FormFieldType::Text,
        required: true,
        sensitive: true,
    });
    let (mut ws, id) = approved_for(&schema);
    let mut driver = ScriptedDriver::new(live(schema), Ok(qualified_ack()));

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 10).unwrap();
    match outcome {
        SubmitOutcome::Refused {
            issues,
            approval_voided,
        } => {
            assert!(
                !approval_voided,
                "the approval must survive: nothing changed"
            );
            assert!(
                issues.iter().any(|i| i.contains("yourself")),
                "the refusal must say the candidate owns this field: {issues:?}"
            );
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(driver.commits, 0);
    assert!(
        !ws.grant(&id).unwrap().unwrap().consumed,
        "approval still live"
    );
    assert_eq!(state(&ws, &id), ApplicationState::Approved);
}

#[test]
fn a_required_field_the_packet_cannot_answer_blocks_before_any_write() {
    let mut schema = answerable_schema();
    schema.fields.push(FormField {
        selector: "#first_name".into(),
        label: "First name".into(),
        field_type: FormFieldType::Text,
        required: true,
        sensitive: false,
    });
    let (mut ws, id) = approved_for(&schema);
    let mut driver = ScriptedDriver::new(live(schema), Ok(qualified_ack()));

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 10).unwrap();
    match outcome {
        SubmitOutcome::Refused {
            issues,
            approval_voided,
        } => {
            assert!(!approval_voided);
            assert!(
                issues
                    .iter()
                    .any(|i| i.contains("first_name") || i.contains("does not contain")),
                "the app must name what it cannot answer: {issues:?}"
            );
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    // Crucially: the app did not invent a name, and did not write.
    assert_eq!(driver.commits, 0);
    assert!(driver.executed.is_empty());
    assert!(!ws.grant(&id).unwrap().unwrap().consumed);
}

#[test]
fn a_generic_success_page_is_not_a_confirmation() {
    let schema = answerable_schema();
    let (mut ws, id) = approved_for(&schema);
    let mut driver = ScriptedDriver::new(
        live(schema),
        Ok(SiteAck {
            receipt_kind: ReceiptKind::GenericSuccessText,
            payload: "Your application was submitted successfully!".into(),
        }),
    );

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 10).unwrap();
    match outcome {
        SubmitOutcome::Uncertain { why } => {
            assert!(why.contains("qualifying"), "{why}")
        }
        other => panic!("a marketing page must not count as Confirmed: {other:?}"),
    }
    assert_eq!(state(&ws, &id), ApplicationState::Uncertain);
    let history = ws.submission_history(&id).unwrap();
    assert_eq!(
        history.last().unwrap().reason,
        "receipt_not_qualifying",
        "the reason must be recorded verbatim"
    );
}

#[test]
fn an_ambiguous_write_is_uncertain_and_never_retried_automatically() {
    let schema = answerable_schema();
    let (mut ws, id) = approved_for(&schema);
    let mut driver = ScriptedDriver::new(
        live(answerable_schema()),
        Err(DriverError("connection reset".into())),
    );

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 10).unwrap();
    match outcome {
        SubmitOutcome::Uncertain { why } => {
            assert!(
                why.contains("automatic retry frozen"),
                "the message must state that retry is frozen: {why}"
            );
            assert!(
                why.contains("connection reset"),
                "the underlying failure must be reported, not hidden: {why}"
            );
        }
        other => panic!("expected Uncertain, got {other:?}"),
    }
    assert_eq!(driver.commits, 1, "the write was attempted once");
    assert_eq!(state(&ws, &id), ApplicationState::Uncertain);

    // No automatic retry: the state machine itself refuses to go again.
    let mut second = ScriptedDriver::new(live(answerable_schema()), Ok(qualified_ack()));
    assert!(matches!(
        ws.submit_packet(&id, &mut second, NOW + 20).unwrap_err(),
        WorkspaceError::IllegalTransition { .. }
    ));
    assert_eq!(second.commits, 0);

    // Only a deliberate, journalled user action moves it back.
    assert!(ws.retry_after_uncertain(&id, NOW + 30).is_ok());
    assert_eq!(state(&ws, &id), ApplicationState::Approved);
    let history = ws.submission_history(&id).unwrap();
    assert_eq!(
        history.last().unwrap().reason,
        "user_deliberate_retry_after_uncertain"
    );
}

#[test]
fn a_stale_posting_blocks_the_submission_and_voids_the_approval() {
    let schema = answerable_schema();
    // The posting was last observed ten days before the attempt.
    let mut ws = Workspace::in_memory().unwrap();
    ws.test_insert_job_at(
        "greenhouse::req::99",
        "Backend Engineer",
        "acme",
        &(NOW - 10 * 24 * 3600).to_string(),
    );
    let id = JobIdentityId::new("greenhouse::req::99").unwrap();
    ws.add_fact(
        "Shipped a Rust service at 20k req/s",
        FactOrigin::UserAttested,
    )
    .unwrap();
    ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
    ws.prepare_packet(&id, NOW).unwrap();
    let digest = Sha256Digest::of(&schema.schema_hash_input());
    ws.approve_packet(&id, ORIGIN, &digest, NOW).unwrap();

    let mut driver = ScriptedDriver::new(live(schema), Ok(qualified_ack()));
    let outcome = ws.submit_packet(&id, &mut driver, NOW).unwrap();
    match outcome {
        SubmitOutcome::Refused { issues, .. } => {
            assert!(issues.iter().any(|i| i.contains("stale")), "{issues:?}")
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(driver.commits, 0);
}

#[test]
fn a_captcha_or_login_wall_is_refused_and_never_climbed() {
    let schema = answerable_schema();
    let (mut ws, id) = approved_for(&schema);
    let mut form = live(answerable_schema());
    form.walls = vec!["captcha present".into()];
    let mut driver = ScriptedDriver::new(form, Ok(qualified_ack()));

    let outcome = ws.submit_packet(&id, &mut driver, NOW + 10).unwrap();
    match outcome {
        SubmitOutcome::Refused {
            issues,
            approval_voided,
        } => {
            assert!(!approval_voided, "a wall is not a binding mismatch");
            assert!(issues[0].contains("captcha"), "{issues:?}");
            assert!(issues[0].contains("left to you"), "hand it back honestly");
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(driver.commits, 0);
    assert!(
        driver.executed.is_empty(),
        "not even a fill may be attempted"
    );
    assert_eq!(
        state(&ws, &id),
        ApplicationState::Approved,
        "approval intact"
    );
}

#[test]
fn the_fill_plan_only_claims_what_the_packet_really_answers() {
    let schema = answerable_schema();
    let plan = plan_fill(&schema, "body text from the packet");
    assert_eq!(plan.proposals.len(), 2);
    assert!(plan.issues.is_empty());
    assert!(plan.commands().iter().any(
        |c| matches!(c, BrowserCommand::UploadFile { file_name, .. } if file_name.ends_with(".docx"))
    ));

    // A sensitive field is offered to the candidate, never filled.
    let mut with_sensitive = answerable_schema();
    with_sensitive.fields.push(FormField {
        selector: "#auth".into(),
        label: "Work authorization".into(),
        field_type: FormFieldType::Select {
            options: vec!["Yes".into(), "No".into()],
        },
        required: false,
        sensitive: true,
    });
    let plan = plan_fill(&with_sensitive, "body");
    assert_eq!(plan.needs_user, vec!["#auth".to_string()]);
    assert!(!plan
        .commands()
        .iter()
        .any(|c| matches!(c, BrowserCommand::Fill { selector, .. } if selector == "#auth")));
}

#[test]
fn the_submission_journal_cannot_be_rewritten() {
    // The append-only guarantee is enforced by the database itself, so a
    // future code path cannot quietly edit history.
    let mut ws = Workspace::in_memory().unwrap();
    ws.test_insert_job("greenhouse::req::1", "T", "E");
    let id = JobIdentityId::new("greenhouse::req::1").unwrap();
    ws.journal(&id, ApplicationState::Approved, NOW, "preflight_ok")
        .unwrap();
    assert_eq!(ws.submission_history(&id).unwrap().len(), 1);

    // Reusing the same identity must append, never overwrite.
    ws.journal(&id, ApplicationState::Submitting, NOW + 1, "write_started")
        .unwrap();
    let history = ws.submission_history(&id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].reason, "preflight_ok");
    assert_eq!(history[1].reason, "write_started");
}
