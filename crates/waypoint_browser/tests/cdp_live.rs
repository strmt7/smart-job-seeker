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
use waypoint_applications::form_mapping::{BrowserCommand, PacketValue};
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

#[test]
#[ignore = "launches one browser against a local fixture without navigating or submitting"]
fn flags_a_possible_next_step_without_clicking_it() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver.open(&file_url("form_possible_steps.html")).unwrap();
    assert!(live.schema.multi_step);
    assert_eq!(
        driver.read_attribute("#next", "data-clicked").unwrap(),
        None
    );
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser without navigating or submitting"]
fn flags_accessibly_named_custom_next_control_without_clicking() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver
        .open(&file_url("form_accessible_steps.html"))
        .unwrap();
    assert!(live.schema.multi_step);
    assert_eq!(
        driver.read_attribute("#next", "data-clicked").unwrap(),
        None
    );
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser and immediately exercises startup timeout cleanup"]
fn timed_out_browser_start_cleans_its_private_profile() {
    let temp = std::env::temp_dir();
    let prefix = format!("waypoint-profile-{}-", std::process::id());
    let profiles = || -> std::collections::HashSet<_> {
        std::fs::read_dir(&temp)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .map(|entry| entry.path())
            .collect()
    };
    let before = profiles();
    let result = waypoint_browser::cdp::BrowserProcess::launch(std::time::Duration::ZERO);
    assert!(result.is_err(), "zero startup time must fail");
    assert_eq!(profiles(), before, "failed startup must remove its profile");
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
#[ignore = "launches one browser against a local fixture without submitting"]
fn text_fill_checks_that_the_site_retains_the_complete_value() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    driver.open(&file_url("form_value_retention.html")).unwrap();
    let fill = |selector: &str, value: &str| BrowserCommand::Fill {
        selector: selector.into(),
        value: PacketValue::Text(value.into()),
    };
    driver
        .execute(&fill("#normal", "Fictional candidate"))
        .unwrap();
    assert!(driver.execute(&fill("#limited", "Too long")).is_err());
    assert!(driver.execute(&fill("#cleared", "Not retained")).is_err());
    driver.execute(&fill("#years", "3")).unwrap();
    assert!(driver.execute(&fill("#years", "8")).is_err());
    driver.execute(&fill("#code", "ABC")).unwrap();
    assert!(driver.execute(&fill("#code", "123")).is_err());
    let checkbox = BrowserCommand::Fill {
        selector: "#agree".into(),
        value: PacketValue::Bool(true),
    };
    let select = BrowserCommand::SelectOption {
        selector: "#place".into(),
        label: "Beta".into(),
    };
    let radio = BrowserCommand::SelectRadio {
        selector: "#on".into(),
        label: "Remote".into(),
    };
    for choice in [&checkbox, &select, &radio] {
        driver.execute(choice).unwrap();
        driver.verify_choice(choice).unwrap();
    }
    driver
        .verify_text_value("#normal", "Fictional candidate")
        .unwrap();
    driver.execute(&fill("#later", "Changed")).unwrap();
    assert!(driver
        .verify_text_value("#normal", "Fictional candidate")
        .is_err());
    for choice in [&checkbox, &select, &radio] {
        assert!(driver.verify_choice(choice).is_err());
    }
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
}

#[test]
#[ignore = "launches one browser against a local fixture without submitting"]
fn resolves_accessible_label_references_and_flags_ambiguous_ids() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver
        .open(&file_url("form_accessible_labels.html"))
        .unwrap();
    let name = live
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#candidate")
        .unwrap();
    assert_eq!(name.label, "Given name");
    let permit = live
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#permit")
        .unwrap();
    assert_eq!(permit.label, "Work authorization");
    assert!(permit.sensitive);
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.contains("ambiguous accessible label")));
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
}

#[test]
#[ignore = "launches one browser against a local fixture without submitting"]
fn selects_accessibly_labelled_native_radio_groups() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver
        .open(&file_url("form_radio_accessible.html"))
        .unwrap();
    assert!(live.walls.is_empty(), "{:?}", live.walls);
    let group = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Work location")
        .unwrap();
    assert_eq!(
        group.field_type,
        FormFieldType::Radio {
            options: vec!["On site".into(), "Remote".into()]
        }
    );
    driver
        .execute(&BrowserCommand::SelectRadio {
            selector: group.selector.clone(),
            label: "Remote".into(),
        })
        .unwrap();
    assert_eq!(
        driver
            .read_attribute("#remote", "data-seen")
            .unwrap()
            .as_deref(),
        Some("true")
    );
    let schedule = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Schedule")
        .unwrap();
    driver
        .execute(&BrowserCommand::SelectRadio {
            selector: schedule.selector.clone(),
            label: "Evening".into(),
        })
        .unwrap();
    assert_eq!(
        driver
            .read_attribute("#evening", "data-seen")
            .unwrap()
            .as_deref(),
        Some("true")
    );
    let duplicate = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Ambiguous option")
        .unwrap();
    assert!(
        live.schema
            .fields
            .iter()
            .find(|field| field.label == "Work authorization")
            .unwrap()
            .sensitive
    );
    assert!(driver
        .execute(&BrowserCommand::SelectRadio {
            selector: duplicate.selector.clone(),
            label: "Same".into()
        })
        .is_err());
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
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
#[ignore = "launches a real browser with a local fixture"]
fn a_turnstile_widget_is_reported_as_a_wall() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let live = driver
        .open(&file_url("form_turnstile.html"))
        .expect("open fixture");
    assert!(
        live.walls.iter().any(|w| w.contains("captcha")),
        "the challenge must be reported: {:?}",
        live.walls
    );
}

#[test]
#[ignore = "launches a real browser with a local fixture"]
fn a_required_custom_control_is_reported_alongside_native_fields() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let live = driver
        .open(&file_url("form_custom_control.html"))
        .expect("open fixture");
    assert!(
        !live.schema.fields.is_empty(),
        "native fields must still be detected"
    );
    assert!(
        live.walls
            .iter()
            .any(|w| w.contains("required custom control") && w.contains("Preferred location")),
        "the unsupported required question must be reported: {:?}",
        live.walls
    );
    assert!(live
        .walls
        .iter()
        .any(|w| w.contains("required custom control") && w.contains("Preferred workplace")));
    assert!(live
        .walls
        .iter()
        .any(|w| w == "a required custom control needs manual review"));
    assert!(live.walls.iter().all(|w| !w.contains("Unreliable label")));
    for label in ["Motivation", "Availability"] {
        assert!(
            live.walls
                .iter()
                .any(|w| w.contains("required custom control") && w.contains(label)),
            "missing {label}: {:?}",
            live.walls
        );
    }
    assert!(live.walls.iter().all(|w| !w.contains("Hidden question")));
    let editor = live
        .schema
        .fields
        .iter()
        .find(|f| f.label == "Relevant experience")
        .expect("plain editor is a reviewable field");
    assert_eq!(
        editor.field_type,
        waypoint_applications::form_mapping::FormFieldType::TextArea
    );
    assert!(editor.required);
    let command = waypoint_applications::form_mapping::BrowserCommand::Fill {
        selector: editor.selector.clone(),
        value: waypoint_applications::form_mapping::PacketValue::Text(
            "Fictional experience\nSecond line".into(),
        ),
    };
    driver.execute(&command).unwrap();
    driver
        .verify_text_value(&editor.selector, "Fictional experience\nSecond line")
        .unwrap();
    assert!(!live.walls.iter().any(|w| w.contains("Relevant experience")));
}

#[test]
#[ignore = "launches a real browser with a local fixture"]
fn grouped_select_options_can_be_inspected_and_filled() {
    use waypoint_applications::form_mapping::{BrowserCommand, FormFieldType};
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let live = driver
        .open(&file_url("form_optgroups.html"))
        .expect("open fixture");
    let field = live
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#location")
        .unwrap();
    assert_eq!(
        field.field_type,
        FormFieldType::Select {
            options: vec!["Germany".into(), "French Republic".into()]
        }
    );
    driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#location".into(),
            label: "French Republic".into(),
        })
        .expect("choose enabled grouped option");
    assert_eq!(
        driver
            .read_attribute("#location", "data-seen")
            .unwrap()
            .as_deref(),
        Some("fr")
    );
    assert!(driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#location".into(),
            label: "Unavailable".into()
        })
        .is_err());
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser with a local fixture"]
fn multiple_choice_fill_detects_later_changes_and_rejects_unavailable_options() {
    use waypoint_applications::form_mapping::{BrowserCommand, FormFieldType};
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let live = driver
        .open(&file_url("form_optgroups.html"))
        .expect("open fixture");
    let skills = live
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#skills")
        .unwrap();
    assert_eq!(
        skills.field_type,
        FormFieldType::MultiSelect {
            options: vec!["Rust".into(), "Python".into()]
        }
    );
    let first = BrowserCommand::SelectOptions {
        selector: "#skills".into(),
        labels: vec!["Rust".into(), "Python".into()],
    };
    driver.execute(&first).expect("select reviewed options");
    driver.verify_choice(&first).expect("verify selection");
    assert_eq!(
        driver
            .read_attribute("#skills", "data-seen")
            .unwrap()
            .as_deref(),
        Some("rust,python")
    );
    let invalid = BrowserCommand::SelectOptions {
        selector: "#skills".into(),
        labels: vec!["Unavailable".into()],
    };
    assert!(driver.execute(&invalid).is_err());
    driver
        .verify_choice(&first)
        .expect("rejected change leaves selection intact");
    let changed = BrowserCommand::SelectOptions {
        selector: "#skills".into(),
        labels: vec!["Python".into()],
    };
    driver.execute(&changed).expect("change selected options");
    assert!(driver.verify_choice(&first).is_err());
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser with a local fixture"]
fn application_links_use_the_accessible_label_without_navigating() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    driver
        .open(&file_url("application_accessible_links.html"))
        .expect("open fixture");
    let links = driver.application_links().expect("read links");
    assert_eq!(
        links,
        vec![
            "application-form.html",
            "application-other.html",
            "fallback.html",
            "referenced.html"
        ]
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser with a local fixture"]
fn generated_ids_and_quoted_names_are_fillable() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let live = driver
        .open(&file_url("form_generated_ids.html"))
        .expect("open fixture");
    for (label, value) in [("First name", "Example"), ("Last name", "Person")] {
        let field = live
            .schema
            .fields
            .iter()
            .find(|field| field.label == label)
            .unwrap();
        driver
            .execute(&BrowserCommand::Fill {
                selector: field.selector.clone(),
                value: PacketValue::Text(value.into()),
            })
            .expect("fill generated selector");
        assert_eq!(
            driver
                .read_attribute(&field.selector, "data-seen")
                .unwrap()
                .as_deref(),
            Some(value)
        );
    }
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser with a local fixture"]
fn hidden_duplicate_does_not_hide_the_active_form_field() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let live = driver
        .open(&file_url("form_hidden_duplicate.html"))
        .expect("open fixture");
    assert!(live.walls.is_empty(), "{:?}", live.walls);
    assert_eq!(live.schema.fields.len(), 1);
    let field = &live.schema.fields[0];
    driver
        .execute(&BrowserCommand::Fill {
            selector: field.selector.clone(),
            value: PacketValue::Text("Example".into()),
        })
        .expect("fill active field");
    assert_eq!(
        driver
            .read_attribute(&field.selector, "data-seen")
            .unwrap()
            .as_deref(),
        Some("Example")
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser and changes sections without submitting"]
fn reinspection_tracks_a_revealed_form_section() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let first = driver
        .open(&file_url("form_hidden_steps.html"))
        .expect("open fixture");
    assert!(first.walls.is_empty(), "{:?}", first.walls);
    assert_eq!(first.schema.fields.len(), 1);
    assert_eq!(first.schema.fields[0].label, "First answer");
    driver
        .execute(&BrowserCommand::Fill {
            selector: first.schema.fields[0].selector.clone(),
            value: PacketValue::Text("First".into()),
        })
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#next".into(),
        })
        .unwrap();
    let second = driver.inspect_current().expect("inspect revealed section");
    assert!(second.walls.is_empty(), "{:?}", second.walls);
    assert_eq!(second.schema.fields.len(), 1);
    assert_eq!(second.schema.fields[0].label, "Second answer");
    assert_ne!(first.schema, second.schema);
    driver
        .execute(&BrowserCommand::Fill {
            selector: second.schema.fields[0].selector.clone(),
            value: PacketValue::Text("Second".into()),
        })
        .unwrap();
    assert_eq!(
        driver
            .read_attribute(&second.schema.fields[0].selector, "data-seen")
            .unwrap()
            .as_deref(),
        Some("Second")
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser and changes CSS-hidden sections without submitting"]
fn css_hidden_sections_are_reinspected_and_visible_field_is_filled() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let first = driver
        .open(&file_url("form_css_steps.html"))
        .expect("open fixture");
    assert!(first.walls.is_empty(), "{:?}", first.walls);
    assert!(first.schema.multi_step);
    assert!(
        first
            .schema
            .fields
            .iter()
            .any(|field| matches!(field.field_type, FormFieldType::File)),
        "visually hidden file input should remain attachable"
    );
    let field = first
        .schema
        .fields
        .iter()
        .find(|field| field.label == "First answer")
        .unwrap();
    driver
        .execute(&BrowserCommand::Fill {
            selector: field.selector.clone(),
            value: PacketValue::Text("First".into()),
        })
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#next".into(),
        })
        .unwrap();
    let second = driver.inspect_current().expect("inspect revealed section");
    assert!(second.walls.is_empty(), "{:?}", second.walls);
    assert!(
        !second.schema.multi_step,
        "the previous step's CSS-hidden button must not count"
    );
    assert!(!second
        .schema
        .fields
        .iter()
        .any(|field| field.label == "First answer"));
    let field = second
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Second answer")
        .unwrap();
    driver
        .execute(&BrowserCommand::Fill {
            selector: field.selector.clone(),
            value: PacketValue::Text("Second".into()),
        })
        .unwrap();
    assert_eq!(
        driver
            .read_attribute(&field.selector, "data-seen")
            .unwrap()
            .as_deref(),
        Some("Second")
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser with a fictional CSS-hidden custom widget"]
fn required_custom_widget_is_reported_only_when_rendered() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let first = driver
        .open(&file_url("form_css_custom.html"))
        .expect("open fixture");
    assert!(first.walls.is_empty(), "{:?}", first.walls);
    driver
        .execute(&BrowserCommand::Click {
            selector: "#reveal".into(),
        })
        .unwrap();
    let second = driver.inspect_current().expect("inspect revealed widget");
    assert!(
        second
            .walls
            .iter()
            .any(|wall| wall.contains("required custom control")),
        "{:?}",
        second.walls
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser with a fictional native-input combobox"]
fn required_input_combobox_requires_manual_review() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let form = driver
        .open(&file_url("form_input_combobox.html"))
        .expect("open fixture");
    assert!(
        form.walls
            .iter()
            .any(|wall| wall.contains("required custom control")),
        "{:?}",
        form.walls
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser with fictional native controls"]
fn native_select_and_number_roles_remain_supported() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let form = driver
        .open(&file_url("form_native_roles.html"))
        .expect("open fixture");
    assert!(form.walls.is_empty(), "{:?}", form.walls);
    assert_eq!(form.schema.fields.len(), 2);
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser with a bounded synthetic form"]
fn oversized_form_reports_manual_review() {
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    let error = driver
        .open(&file_url("form_many_controls.html"))
        .unwrap_err();
    assert!(
        error.0.contains("too many controls") && error.0.contains("manual review"),
        "{error}"
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
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
