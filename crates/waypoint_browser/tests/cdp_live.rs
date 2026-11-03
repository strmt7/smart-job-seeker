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
#[ignore = "launches one local browser against a fictional fixture"]
fn aria_invalid_fields_require_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_aria_invalid_retention.html"))
        .unwrap();
    driver.verify_text_value("#valid", "Fictional").unwrap();
    for selector in ["#invalid", "#grammar", "#unknown"] {
        let error = driver.verify_text_value(selector, "Fictional").unwrap_err();
        assert!(error.to_string().contains("invalid"), "{error}");
    }
    for command in [
        BrowserCommand::Fill {
            selector: "#check".into(),
            value: PacketValue::Bool(true),
        },
        BrowserCommand::SelectOption {
            selector: "#select".into(),
            label: "Choice".into(),
        },
        BrowserCommand::SelectRadio {
            selector: "#radio".into(),
            label: "Choice".into(),
        },
    ] {
        let error = driver.verify_choice(&command).unwrap_err();
        assert!(error.to_string().contains("invalid"), "{error}");
    }
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn typing_stops_when_site_reports_aria_invalid() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_input_aria_invalid.html"))
        .unwrap();
    let fill = |selector: &str| BrowserCommand::Fill {
        selector: selector.into(),
        value: PacketValue::Text("Fictional".into()),
    };
    driver.execute(&fill("#good")).unwrap();
    let error = driver.execute(&fill("#bad")).unwrap_err();
    assert!(error.to_string().contains("invalid"), "{error}");
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn control_changes_stop_when_site_reports_aria_invalid() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_control_aria_invalid.html"))
        .unwrap();
    for command in [
        BrowserCommand::Fill {
            selector: "#check".into(),
            value: PacketValue::Bool(true),
        },
        BrowserCommand::Fill {
            selector: "#date".into(),
            value: PacketValue::Date("2026-11-02".into()),
        },
        BrowserCommand::SelectOption {
            selector: "#select".into(),
            label: "Choice".into(),
        },
        BrowserCommand::SelectOptions {
            selector: "#multi".into(),
            labels: vec!["Choice".into()],
        },
        BrowserCommand::SelectRadio {
            selector: "#radio".into(),
            label: "Choice".into(),
        },
    ] {
        let error = driver.execute(&command).unwrap_err();
        assert!(
            error.to_string().contains("invalid"),
            "{command:?}: {error}"
        );
    }
    let required = |value| BrowserCommand::Fill {
        selector: "#required".into(),
        value: PacketValue::Bool(value),
    };
    let error = driver.execute(&required(false)).unwrap_err();
    assert!(error.to_string().contains("invalid"), "{error}");
    driver.execute(&required(true)).unwrap();
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn section_reveal_is_bounded_and_does_not_activate_controls() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_reveal_bounded.html")).unwrap();
    assert_eq!(
        driver.reveal_section_controls("#application").unwrap(),
        (12, true)
    );
    assert!(driver.reveal_section_controls("#missing").is_err());
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn section_reveal_refuses_a_captcha_wall() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_reveal_captcha.html")).unwrap();
    assert!(page.walls.iter().any(|wall| wall.contains("captcha")));
    assert!(driver.reveal_section_controls("#application").is_err());
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn section_reveal_stops_when_scrolling_changes_the_page() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_reveal_navigation.html"))
        .unwrap();
    let error = driver
        .reveal_section_controls("#application")
        .expect_err("changed page must stop discovery");
    assert!(error.to_string().contains("page changed"), "{error}");
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn section_reveal_stops_when_the_control_inventory_changes() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_reveal_mutation.html")).unwrap();
    let error = driver
        .reveal_section_controls("#application")
        .expect_err("changed control inventory must stop discovery");
    assert!(
        error.to_string().contains("changed") || error.to_string().contains("manual review"),
        "{error}"
    );
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn section_reveal_stops_when_control_is_replaced_without_count_change() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_reveal_replacement.html"))
        .unwrap();
    let error = driver
        .reveal_section_controls("#application")
        .expect_err("replaced control must stop discovery");
    assert!(
        error.to_string().contains("changed") || error.to_string().contains("manual review"),
        "{error}"
    );
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn aria_required_tokens_are_case_insensitive() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_aria_required_case.html"))
        .unwrap();
    for label in ["Name", "Email"] {
        assert!(
            live.schema
                .fields
                .iter()
                .find(|field| field.label == label)
                .unwrap()
                .required
        );
    }
    assert!(
        !live
            .schema
            .fields
            .iter()
            .find(|field| field.label == "Optional")
            .unwrap()
            .required
    );
    assert!(live
        .walls
        .iter()
        .any(|wall| wall == "a required field is disabled or read-only"));
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn aria_invalid_tracks_visible_fields_and_correction() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&file_url("form_aria_invalid.html")).unwrap();
    let invalid: Vec<_> = live
        .walls
        .iter()
        .filter(|wall| wall.starts_with(waypoint_browser::INVALID_FIELD_REVIEW_PREFIX))
        .collect();
    assert_eq!(invalid.len(), 3, "walls: {:?}", live.walls);
    assert!(invalid
        .iter()
        .any(|wall| wall.contains("Email address")
            && wall.contains("Enter a complete email address.")));
    assert!(invalid
        .iter()
        .any(|wall| wall.contains("Full name") && wall.contains("site error detail unavailable")));
    assert!(invalid
        .iter()
        .any(|wall| wall.contains("Address") && wall.contains("site error detail unavailable")));
    assert!(!invalid.iter().any(|wall| wall.contains("Hidden detail")
        || wall.contains("First detail")
        || wall.contains("Second detail")));
    assert!(!invalid
        .iter()
        .any(|wall| wall.contains("Phone number") || wall.contains("Hidden field")));
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#email".into(),
            value: PacketValue::Text("fictional@example.test".into()),
        })
        .unwrap();
    let updated = driver.inspect_current().unwrap();
    assert!(!updated.walls.iter().any(|wall| wall
        .starts_with(waypoint_browser::INVALID_FIELD_REVIEW_PREFIX)
        && wall.contains("Email address")));
    assert!(updated.walls.iter().any(|wall| wall
        .starts_with(waypoint_browser::INVALID_FIELD_REVIEW_PREFIX)
        && wall.contains("Full name")));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn native_validation_reports_failure_without_exposing_input() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_native_constraint.html"))
        .unwrap();
    let fill = |selector: &str, text: &str| BrowserCommand::Fill {
        selector: selector.into(),
        value: PacketValue::Text(text.into()),
    };
    let error = driver.execute(&fill("#code", "AB-12")).unwrap_err().0;
    assert!(error.contains("required format"), "{error}");
    assert!(!error.contains("AB-12"));
    assert_eq!(driver.text_value_snapshot("#code").unwrap().value, "");
    driver.execute(&fill("#code", "AB-123")).unwrap();
    let error = driver
        .execute(&fill("#email", "fictional-at-example"))
        .unwrap_err()
        .0;
    assert!(error.contains("expected type"), "{error}");
    assert!(!error.contains("fictional-at-example"));
    assert_eq!(driver.text_value_snapshot("#email").unwrap().value, "");
    for value in ["x", "abcde", "😀😀x"] {
        assert!(driver.execute(&fill("#length", value)).is_err());
        assert_eq!(driver.text_value_snapshot("#length").unwrap().value, "ok");
    }
    assert!(driver.execute(&fill("#dynamic", "letters")).is_err());
    assert_eq!(driver.text_value_snapshot("#dynamic").unwrap().value, "old");
    driver
        .execute(&fill("#email", "fictional@example.test"))
        .unwrap();
    let changed = driver.verify_text_value("#code", "AB-123").unwrap_err().0;
    assert!(changed.contains("custom validation error"), "{changed}");
    assert!(!changed.contains("Fictional private detail"));
    assert!(!changed.contains("AB-123"));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn changing_native_pattern_changes_reviewed_form_identity() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let initial = driver.open(&file_url("form_pattern_change.html")).unwrap();
    assert_eq!(
        initial
            .schema
            .fields
            .iter()
            .find(|field| field.selector == "#code")
            .unwrap()
            .pattern
            .as_deref(),
        Some("[A-Z]{2}-[0-9]{3}")
    );
    let original_hash = initial.schema.schema_hash_input();
    assert_eq!(
        initial
            .schema
            .fields
            .iter()
            .find(|field| field.selector == "#long")
            .unwrap()
            .pattern,
        None
    );
    assert!(initial
        .walls
        .iter()
        .any(|warning| warning.contains("format pattern exceeds the inspection limit")));
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#other".into(),
            value: PacketValue::Text("fictional".into()),
        })
        .unwrap();
    let updated = driver.inspect_current().unwrap();
    assert_eq!(
        updated
            .schema
            .fields
            .iter()
            .find(|field| field.selector == "#code")
            .unwrap()
            .pattern
            .as_deref(),
        Some("[A-Z]{2}-[0-9]{4}")
    );
    assert_ne!(updated.schema.schema_hash_input(), original_hash);
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn radio_group_required_is_inherited_by_its_options() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_radio_group_required.html"))
        .unwrap();
    let workplace = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Workplace")
        .unwrap();
    assert!(workplace.required);
    let unlabelled = live
        .schema
        .fields
        .iter()
        .find(|field| field.label.contains("unlabelled"))
        .unwrap();
    assert!(unlabelled.required);
    assert!(unlabelled.sensitive);
    let optional = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Optional")
        .unwrap();
    assert!(!optional.required);
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn native_special_controls_are_reported_for_manual_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_native_special_controls.html"))
        .unwrap();
    let name = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Name")
        .unwrap();
    assert!(!name.sensitive);
    for label in ["Start week", "Preference", "Colour"] {
        let field = live
            .schema
            .fields
            .iter()
            .find(|field| field.label == label)
            .unwrap();
        assert!(
            field.sensitive,
            "{label} must not receive automatic text mapping"
        );
        assert!(live.walls.iter().any(
            |wall| wall.contains("custom control needs manual review") && wall.contains(label)
        ));
    }
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.contains("required custom control") && wall.contains("Start week")));
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn custom_choice_inventory_reaches_later_controls_and_reports_its_cap() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_paging.html"))
        .unwrap();
    let inventory = driver.page_custom_choices().unwrap();
    assert_eq!(inventory.choices.len(), 45, "{inventory:?}");
    assert!(inventory
        .choices
        .iter()
        .any(|choice| choice.label == "Choice 16"));
    assert!(inventory
        .choices
        .iter()
        .any(|choice| choice.label == "Choice 45"));
    assert!(inventory.incomplete, "the 46th control exceeds the cap");
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn custom_choice_inventory_keeps_long_question_labels() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_long_label.html"))
        .unwrap();
    let choices = driver.page_custom_choices().unwrap();
    assert_eq!(choices.choices.len(), 1);
    let label = &choices.choices[0].label;
    assert!(label.chars().count() > 200);
    assert!(label.ends_with("when the requirements or circumstances change."));
    assert_ne!(label, "Unlabelled choice");
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn native_time_is_filled_and_verified_within_site_limits() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_native_special_controls.html"))
        .unwrap();
    let field = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Interview time")
        .unwrap();
    assert_eq!(field.field_type, FormFieldType::Time);
    let fill = |value: &str| BrowserCommand::Fill {
        selector: "#time".into(),
        value: PacketValue::Date(value.into()),
    };
    assert!(driver.execute(&fill("25:00")).is_err());
    assert!(driver.execute(&fill("08:30")).is_err());
    assert!(driver.execute(&fill("2026-11-01")).is_err());
    assert_eq!(driver.text_value_snapshot("#time").unwrap().value, "");
    driver.ensure_text_replaceable("#time", "13:30").unwrap();
    driver.execute(&fill("13:30")).unwrap();
    driver.verify_text_value("#time", "13:30").unwrap();
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn native_local_datetime_is_checked_and_retained() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_native_special_controls.html"))
        .unwrap();
    let field = live
        .schema
        .fields
        .iter()
        .find(|f| f.label == "Appointment")
        .unwrap();
    assert_eq!(field.field_type, FormFieldType::DateTimeLocal);
    let fill = |value: &str| BrowserCommand::Fill {
        selector: "#datetime".into(),
        value: PacketValue::Date(value.into()),
    };
    assert!(driver.execute(&fill("2026-02-30T12:00")).is_err());
    assert!(driver.execute(&fill("2026-10-30T12:00")).is_err());
    assert!(driver.execute(&fill("2026-11-01T12:00Z")).is_err());
    assert_eq!(driver.text_value_snapshot("#datetime").unwrap().value, "");
    driver
        .ensure_text_replaceable("#datetime", "2026-11-01T13:30")
        .unwrap();
    driver.execute(&fill("2026-11-01T13:30")).unwrap();
    driver
        .verify_text_value("#datetime", "2026-11-01T13:30")
        .unwrap();
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser and opens a nonexistent fictional file"]
fn failed_navigation_does_not_return_an_inspected_form() {
    let missing = format!("waypoint-nonexistent-{}.html", std::process::id());
    assert!(!fixtures_dir().join(&missing).exists());
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_ok.html")).unwrap();
    let result = driver.open(&file_url(&missing));
    assert!(
        result.is_err(),
        "failed navigation returned a form: {result:?}"
    );
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn verification_rejects_hidden_and_transparent_text_fields() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_hidden_verification.html"))
        .unwrap();
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Visible"));
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Transparent"));
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Clipped"));
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Scrollable"));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall == "a custom control needs manual review"));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.contains("obscured, clipped or transparent")));
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Partial"));
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: "#partial".into(),
            value: PacketValue::Text("Changed".into()),
        })
        .is_err());
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#reveal-partial".into(),
            value: PacketValue::Text("reveal".into()),
        })
        .unwrap();
    driver.verify_text_value("#partial", "Example").unwrap();
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: "#focus-clipped".into(),
            value: PacketValue::Text("Changed".into()),
        })
        .is_err());
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#reveal-focus".into(),
            value: PacketValue::Text("reveal".into()),
        })
        .unwrap();
    driver
        .verify_text_value("#focus-clipped", "Example")
        .unwrap();
    for selector in ["#covered", "#focus-covered"] {
        assert!(driver
            .execute(&BrowserCommand::Fill {
                selector: selector.into(),
                value: PacketValue::Text("Changed".into())
            })
            .is_err());
        let reveal = if selector == "#covered" {
            "#reveal-overlay"
        } else {
            "#reveal-focus-overlay"
        };
        driver
            .execute(&BrowserCommand::Fill {
                selector: reveal.into(),
                value: PacketValue::Text("reveal".into()),
            })
            .unwrap();
        driver.verify_text_value(selector, "Example").unwrap();
    }
    driver.verify_text_value("#visible", "Example").unwrap();
    driver.verify_text_value("#offscreen", "Example").unwrap();
    driver
        .verify_text_value("#dynamic-field", "Example")
        .unwrap();
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#hide-trigger".into(),
            value: PacketValue::Text("change".into()),
        })
        .unwrap();
    assert!(driver
        .verify_text_value("#dynamic-field", "Example")
        .is_err());
    for selector in ["#hidden", "#transparent", "#invisible", "#clipped"] {
        assert!(
            driver.verify_text_value(selector, "Example").is_err(),
            "accepted {selector}"
        );
    }
}

#[test]
#[ignore = "starts one local browser against a fictional overlay"]
fn obscured_form_reports_how_to_resume_without_interaction() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_obscured_overlay.html"))
        .unwrap();
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#name"));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.contains("obscured, clipped or transparent")));
    assert!(!live
        .walls
        .iter()
        .any(|wall| wall.contains("A dialog covers a form field")));
    assert!(driver.trace.iter().all(|entry| !entry.contains("clicked")
        && !entry.contains("typed")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "starts one local browser against a fictional dialog"]
fn dialog_obscuring_field_reports_specific_review_without_interaction() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&file_url("form_dialog_obscures.html")).unwrap();
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#name"));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.contains("A dialog covers a form field")));
    assert!(driver.trace.iter().all(|entry| !entry.contains("clicked")
        && !entry.contains("typed")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "starts one local browser against a fictional dismissible dialog"]
fn dismissing_a_dialog_and_reinspecting_recovers_the_obscured_field() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let first = driver.open(&file_url("form_dialog_obscures.html")).unwrap();
    assert!(first
        .walls
        .iter()
        .any(|wall| wall.contains("A dialog covers a form field")));
    driver
        .execute(&BrowserCommand::Click {
            selector: "#dismiss".into(),
        })
        .unwrap();
    let second = driver.inspect_current().unwrap();
    assert!(!second
        .walls
        .iter()
        .any(|wall| wall.contains("A dialog covers a form field")));
    assert!(second
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#name"));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("typed") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn fieldset_context_distinguishes_applicant_and_reference_email() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_contact_sections.html"))
        .unwrap();
    let field = |selector: &str| {
        live.schema
            .fields
            .iter()
            .find(|field| field.selector == selector)
            .unwrap()
    };
    assert_eq!(field("#self").label, "Applicant contact — Email");
    assert_eq!(field("#reference").label, "Professional reference — Email");
    assert_eq!(
        field("#manager").label,
        "Professional reference — Manager email"
    );
    assert_ne!(field("#self").label, field("#reference").label);
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn accessible_group_context_distinguishes_applicant_and_reference() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_contact_named_groups.html"))
        .unwrap();
    let field = |selector: &str| {
        live.schema
            .fields
            .iter()
            .find(|field| field.selector == selector)
            .unwrap()
    };
    assert_eq!(field("#self").label, "Applicant contact — Email");
    assert_eq!(
        field("#reference").label,
        "Professional reference — Contact details — Email"
    );
    assert_eq!(
        field("#manager").label,
        "Professional reference — Contact details — Manager email"
    );
    let editor = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Professional reference — Contact details — Notes")
        .unwrap();
    assert_eq!(editor.field_type, FormFieldType::TextArea);
    let original_schema = live.schema.schema_hash_input();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#change-context".into(),
        })
        .unwrap();
    let updated = driver.inspect_current().unwrap();
    assert!(updated
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Emergency contact — Contact details — Notes"));
    assert_ne!(original_schema, updated.schema.schema_hash_input());
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn named_sections_and_regions_supply_context_for_fields() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_contact_named_sections.html"))
        .unwrap();
    let field = |selector: &str| {
        live.schema
            .fields
            .iter()
            .find(|field| field.selector == selector)
            .unwrap()
    };
    assert_eq!(field("#self").label, "Applicant contact — Email");
    assert_eq!(
        field("#reference").label,
        "Professional reference — Contact details — Email"
    );
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Professional reference — Contact details — Notes"));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against fictional section headings"]
fn visible_unambiguous_headings_give_context_and_others_require_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let first = driver
        .open(&file_url("form_contact_headed_sections.html"))
        .unwrap();
    assert!(first
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#self" && field.label == "Applicant contact — Email"));
    assert!(first
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#reference"
            && field.label == "Professional reference — Contact details — Email"));
    let before = first.schema.schema_hash_input();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#change-context".into(),
        })
        .unwrap();
    let after = driver.inspect_current().unwrap();
    assert_ne!(before, after.schema.schema_hash_input());
    assert!(after
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#reference"
            && field.label == "Emergency contact — Contact details — Email"));
    let blocks = driver.open(&file_url("form_block_heading.html")).unwrap();
    assert!(blocks
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#emergency" && field.label == "Emergency contact — Email"));
    let unclear = driver
        .open(&file_url("form_ambiguous_headings.html"))
        .unwrap();
    assert!(unclear
        .walls
        .iter()
        .any(|reason| reason.contains("section heading is ambiguous")));
    assert!(unclear
        .walls
        .iter()
        .any(|reason| reason.contains("section heading could not be verified")));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against fictional headed containers"]
fn plain_headed_containers_provide_reviewable_context() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let form = driver.open(&file_url("form_div_headings.html")).unwrap();
    for (selector, question) in [("#one", "First question"), ("#two", "Second question")] {
        assert!(form
            .schema
            .fields
            .iter()
            .any(|field| field.selector == selector
                && field.label
                    == format!("Possible section: Project questions — answer in your own words — {question}")));
    }
    assert!(form
        .walls
        .iter()
        .any(|wall| wall.contains("section heading could not be verified")));
    assert!(form
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#unrelated" && !field.label.contains("Navigation only")));
    let original = form.schema.schema_hash_input();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#change".into(),
        })
        .unwrap();
    let updated = driver.inspect_current().unwrap();
    assert_ne!(original, updated.schema.schema_hash_input());
    assert!(
        updated
            .schema
            .fields
            .iter()
            .any(|field| field.selector == "#one"
                && field.label.contains("Changed project questions"))
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a bounded nested form fixture"]
fn deeply_nested_headings_report_context_budget() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let form = driver
        .open(&file_url("form_nested_group_budget.html"))
        .unwrap();
    assert!(
        form.walls
            .iter()
            .any(|wall| wall.contains("section context exceeded the inspection budget")),
        "{:?}",
        form.walls
    );
    assert!(form
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#early"));
    assert!(!form
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#email"));
    assert!(form
        .walls
        .iter()
        .any(|wall| wall == "an unavailable field needs manual review"));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn verification_rejects_hidden_choices() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_hidden_verification.html"))
        .unwrap();
    let checkbox = |selector: &str| BrowserCommand::Fill {
        selector: selector.into(),
        value: PacketValue::Bool(true),
    };
    let select = |selector: &str| BrowserCommand::SelectOption {
        selector: selector.into(),
        label: "Alpha".into(),
    };
    let radio = |selector: &str| BrowserCommand::SelectRadio {
        selector: selector.into(),
        label: "Remote".into(),
    };
    for command in [
        checkbox("#check-visible"),
        select("#select-visible"),
        radio("#radio-visible"),
    ] {
        driver.verify_choice(&command).unwrap();
    }
    for command in [
        checkbox("#check-hidden"),
        checkbox("#check-clipped"),
        select("#select-hidden"),
        select("#select-clipped"),
        radio("#radio-hidden"),
        radio("#radio-first"),
        BrowserCommand::SelectOptions {
            selector: "#multi-hidden".into(),
            labels: vec!["Alpha".into()],
        },
    ] {
        assert!(
            driver.verify_choice(&command).is_err(),
            "accepted {command:?}"
        );
    }
}

#[test]
#[ignore = "launches one local browser and a fictional local HTTP server"]
fn http_error_page_is_not_treated_as_an_application_form() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
                    let mut request = [0u8; 2048];
                    let mut length = 0;
                    while length < request.len() {
                        match stream.read(&mut request[length..]) {
                            Ok(0) | Err(_) => break,
                            Ok(read) => length += read,
                        }
                        if request[..length].contains(&b'\n') {
                            break;
                        }
                    }
                    // Chromium may open a speculative connection before the
                    // navigation request. Do not consume the fixture on it.
                    if !request[..length].starts_with(b"GET /fictional ") {
                        continue;
                    }
                    let body = "<html><body><h1>Not found</h1><input name='email'></body></html>";
                    let response = format!("HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = stream.write_all(response.as_bytes());
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(25))
                }
                Err(_) => return,
            }
        }
    });
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let result = driver.open(&format!("http://127.0.0.1:{port}/fictional"));
    server.join().unwrap();
    assert!(
        result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("HTTP 404")),
        "Expected the fictional HTTP 404: {result:?}"
    );
    let reinspected = driver.inspect_current().unwrap();
    assert!(
        reinspected
            .walls
            .iter()
            .any(|wall| wall.contains("HTTP 404")),
        "reinspection lost the main document error: {:?}",
        reinspected.walls
    );
}

#[test]
#[ignore = "launches one local browser and a fictional local HTTP server"]
fn redirect_and_failed_subresource_do_not_mask_a_successful_main_page() {
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !flag.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
                    let mut request = [0u8; 2048];
                    let size = stream.read(&mut request).unwrap_or(0);
                    let first = String::from_utf8_lossy(&request[..size]);
                    let response = if first.starts_with("GET /redirect ") {
                        "HTTP/1.1 302 Found\r\nLocation: /form\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
                    } else if first.starts_with("GET /form ") {
                        let body =
                            "<html><body><input name='email'><img src='/missing'></body></html>";
                        format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    } else {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_owned()
                    };
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(25))
                }
                Err(_) => break,
            }
        }
    });
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let result = driver.open(&format!("http://127.0.0.1:{port}/redirect"));
    stopped.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let form = result.unwrap();
    assert!(
        form.walls.is_empty(),
        "unexpected warning: {:?}",
        form.walls
    );
    assert_eq!(form.schema.fields.len(), 1);
    assert!(driver.current_url().unwrap().ends_with("/form"));
    let same_document = driver
        .open(&format!("http://127.0.0.1:{port}/form#profile"))
        .unwrap();
    assert!(
        same_document.walls.is_empty(),
        "same-document navigation lost verified status: {:?}",
        same_document.walls
    );
}

#[test]
#[ignore = "launches one local browser against a fictional form without submitting"]
fn file_accept_is_checked_before_the_attachment_changes() {
    use std::io::Write;
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("file_accept.html")).unwrap();
    let path = std::env::temp_dir().join(format!("waypoint-accept-{}.pdf", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(b"fictional PDF").unwrap();
    drop(file);
    let result = driver.attach_local_file("#docx", &path);
    assert!(result.unwrap_err().0.contains("does not list"));
    assert_eq!(
        driver
            .read_attribute("body", "data-changes")
            .unwrap()
            .as_deref(),
        Some("0")
    );
    driver.attach_local_file("#pdf", &path).unwrap();
    driver.verify_local_file("#pdf", &path).unwrap();
    assert_eq!(
        driver
            .read_attribute("body", "data-changes")
            .unwrap()
            .as_deref(),
        Some("1")
    );
    assert!(driver
        .attach_local_file("#changing", &path)
        .unwrap_err()
        .0
        .contains("does not list"));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "launches one browser with fictional local files and never submits"]
fn multiple_pdf_files_are_attached_and_verified_as_a_set() {
    let root = std::env::temp_dir().join(format!("waypoint-multiple-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let first = root.join("resume.pdf");
    let second = root.join("certificate.pdf");
    let bad = root.join("notes.txt");
    let alternate_dir = root.join("alternate");
    std::fs::create_dir(&alternate_dir).unwrap();
    let alternate = alternate_dir.join("resume.pdf");
    std::fs::write(&first, b"fictional resume").unwrap();
    std::fs::write(&alternate, b"different resume").unwrap();
    std::fs::write(&second, b"fictional certificate").unwrap();
    std::fs::write(&bad, b"fictional text").unwrap();
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_multiple_files.html")).unwrap();
    assert!(driver.file_input_allows_multiple("#multiple").unwrap());
    assert!(!driver.file_input_allows_multiple("#single").unwrap());
    let files = vec![first.clone(), second.clone()];
    driver
        .attach_local_files_if_unchanged("#multiple", std::slice::from_ref(&first), &[])
        .unwrap();
    assert!(driver
        .attach_local_files_if_unchanged("#multiple", &files, &[])
        .is_err());
    driver.verify_local_file("#multiple", &first).unwrap();
    driver
        .attach_local_files_if_unchanged("#multiple", &files, std::slice::from_ref(&first))
        .unwrap();
    assert!(driver
        .attach_local_files_if_unchanged(
            "#multiple",
            std::slice::from_ref(&second),
            std::slice::from_ref(&first)
        )
        .is_err());
    driver.verify_local_files("#multiple", &files).unwrap();
    driver
        .attach_local_files("#single", std::slice::from_ref(&first))
        .unwrap();
    assert!(driver
        .attach_local_files_if_unchanged("#single", std::slice::from_ref(&second), &[])
        .is_err());
    driver.verify_local_file("#single", &first).unwrap();
    driver
        .attach_local_files_if_unchanged(
            "#single",
            std::slice::from_ref(&second),
            std::slice::from_ref(&first),
        )
        .unwrap();
    driver.verify_local_file("#single", &second).unwrap();
    driver
        .attach_local_files("#single", std::slice::from_ref(&alternate))
        .unwrap();
    assert!(driver.verify_local_file("#single", &first).is_err());
    assert!(driver
        .attach_local_files_if_unchanged(
            "#single",
            std::slice::from_ref(&second),
            std::slice::from_ref(&first)
        )
        .is_err());
    driver.verify_local_file("#single", &alternate).unwrap();
    assert!(driver.attach_local_files("#single", &files).is_err());
    assert!(driver
        .attach_local_files("#multiple", &[first.clone(), bad.clone()])
        .is_err());
    assert!(driver
        .attach_local_files("#multiple", &[first.clone(), first.clone()])
        .is_err());
    assert!(driver
        .attach_local_files("#multiple", &[first.clone(), root.join("missing.pdf")])
        .is_err());
    driver.verify_local_files("#multiple", &files).unwrap();
    assert!(driver.verify_local_file("#multiple", &first).is_err());
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    drop(driver);
    for path in [first, second, bad, alternate] {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(alternate_dir).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
#[ignore = "launches one local browser against a fictional form without submitting"]
fn multilingual_sensitive_labels_are_marked_for_review() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver
        .open(&file_url("form_multilingual_review.html"))
        .unwrap();
    for (label, sensitive) in [
        ("Vorname", false),
        ("E-Mail-Adresse", false),
        ("Besitzen Sie eine Arbeitserlaubnis?", true),
        ("Geschlecht", true),
        ("Gehaltsvorstellung", true),
        ("¿Necesita patrocinio laboral?", true),
        ("Date de naissance", true),
    ] {
        let field = live
            .schema
            .fields
            .iter()
            .find(|field| field.label == label)
            .expect(label);
        assert_eq!(field.sensitive, sensitive, "{label}");
    }
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
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
#[ignore = "launches one browser against local fixtures without clicking or submitting"]
fn accessible_current_step_indicates_a_multistep_form() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver.open(&file_url("form_aria_step.html")).unwrap();
    assert!(live.schema.multi_step);
    assert_eq!(
        driver.read_attribute("#forward", "data-clicked").unwrap(),
        None
    );
    let hidden = driver
        .open(&file_url("form_aria_step_hidden.html"))
        .unwrap();
    assert!(!hidden.schema.multi_step);
    let uncertain = driver
        .open(&file_url("form_aria_step_uncertain.html"))
        .unwrap();
    assert!(uncertain.schema.multi_step);
    assert_eq!(
        driver.read_attribute("#forward", "data-clicked").unwrap(),
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
#[ignore = "launches one browser against fictional multiple-reference choices"]
fn custom_choice_resolves_a_unique_listbox_in_id_references() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_idrefs.html"))
        .unwrap();
    let inventory = driver.page_custom_choices().unwrap();
    let location = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Location")
        .unwrap()
        .selector
        .clone();
    let ambiguous = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Ambiguous")
        .unwrap()
        .selector
        .clone();
    let work_mode = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Work mode")
        .unwrap()
        .selector
        .clone();
    let uncertain: Vec<_> = ["Missing reference", "Duplicate reference"]
        .iter()
        .map(|label| {
            inventory
                .choices
                .iter()
                .find(|choice| choice.label == *label)
                .unwrap()
                .selector
                .clone()
        })
        .collect();
    let opened = driver.open_custom_choice(&location).unwrap();
    assert_eq!(opened.options, ["Berlin", "Lisbon", "Changing reference"]);
    assert!(opened.options_incomplete);
    driver
        .select_custom_choice_option(&location, "Berlin")
        .unwrap();
    driver
        .verify_custom_choice_value(&location, "Berlin")
        .unwrap();
    assert!(driver
        .select_custom_choice_option(&location, "Changing reference")
        .is_err());
    driver
        .verify_custom_choice_value(&location, "Berlin")
        .unwrap();
    assert_eq!(
        driver.open_custom_choice(&work_mode).unwrap().options,
        ["Remote"]
    );
    driver
        .select_custom_choice_option(&work_mode, "Remote")
        .unwrap();
    driver
        .verify_custom_choice_value(&work_mode, "Remote")
        .unwrap();
    let opened = driver.open_custom_choice(&ambiguous).unwrap();
    assert!(opened.options.is_empty() && opened.options_incomplete);
    assert!(driver
        .select_custom_choice_option(&ambiguous, "One")
        .is_err());
    for selector in uncertain {
        let opened = driver.open_custom_choice(&selector).unwrap();
        assert!(opened.options.is_empty() && opened.options_incomplete);
        assert!(driver
            .select_custom_choice_option(&selector, "Berlin")
            .is_err());
    }
    assert!(driver
        .page_text_bounded(2000)
        .unwrap()
        .0
        .contains("NOT SUBMITTED"));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn unannotated_choice_with_hidden_companion_requires_manual_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_unannotated_choice.html"))
        .unwrap();
    let location = live
        .schema
        .fields
        .iter()
        .find(|field| field.selector.contains("location"))
        .unwrap();
    let reference = live
        .schema
        .fields
        .iter()
        .find(|field| field.selector.contains("reference"))
        .unwrap();
    assert_eq!(location.label, "Location");
    assert!(location.sensitive);
    assert!(!reference.sensitive);
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.starts_with("a custom control needs manual review: Location")));
    assert!(driver.trace.iter().all(|event| !event.contains("clicked")
        && !event.contains("typed")
        && !event.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn rendered_labels_exclude_hidden_status_and_control_contents() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&file_url("form_rendered_labels.html")).unwrap();
    let labels: Vec<_> = live
        .schema
        .fields
        .iter()
        .map(|field| field.label.as_str())
        .collect();
    assert_eq!(
        &labels[..4],
        ["Current city", "Email", "Preferred region", "Postal code"]
    );
    assert!(labels[4].starts_with("Unverified label (review):"));
    assert!(labels[5].starts_with("Unverified label (review):"));
    assert!(live.schema.fields[..4].iter().all(|field| !field.sensitive));
    assert!(live.schema.fields[4].sensitive);
    assert!(live.schema.fields[5].sensitive);
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("clicked") && !event.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn unassociated_questions_are_advisory_and_never_auto_filled() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_unassociated_questions.html"))
        .unwrap();
    let find = |name: &str| {
        live.schema
            .fields
            .iter()
            .find(|field| field.selector.contains(name))
            .unwrap()
    };
    assert!(find("salary")
        .label
        .contains("What is your expected compensation?"));
    assert!(find("salary").sensitive);
    assert!(find("project")
        .label
        .contains("Describe a relevant project."));
    assert!(find("project").sensitive);
    assert_eq!(find("ambiguous").label, "ambiguous");
    assert!(find("ambiguous").sensitive);
    assert_eq!(find("hidden-context").label, "hidden-context");
    assert!(find("hidden-context").sensitive);
    assert!(find("empty-label")
        .label
        .contains("What is your availability?"));
    assert!(find("empty-label").sensitive);
    assert_eq!(find("labelled").label, "Explicit question");
    assert!(!find("labelled").sensitive);
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("clicked") && !event.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn repeated_text_selectors_target_the_individual_field() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_repeated_text_selectors.html"))
        .unwrap();
    let field = |label: &str| {
        live.schema
            .fields
            .iter()
            .find(|field| field.label == label)
            .unwrap()
    };
    for (ours, other, value) in [
        (
            "Applicant email",
            "Reference email",
            "fictional@example.invalid",
        ),
        ("Applicant phone", "Reference phone", "2025550101"),
    ] {
        let applicant = field(ours);
        let reference = field(other);
        assert_ne!(applicant.selector, reference.selector);
        driver
            .execute(&BrowserCommand::Fill {
                selector: applicant.selector.clone(),
                value: PacketValue::Text(value.into()),
            })
            .unwrap();
        assert_eq!(
            driver
                .text_value_snapshot(&applicant.selector)
                .unwrap()
                .value,
            value
        );
        assert_eq!(
            driver
                .text_value_snapshot(&reference.selector)
                .unwrap()
                .value,
            ""
        );
    }
    assert!(!live
        .walls
        .iter()
        .any(|wall| wall == "ambiguous form controls need manual review"));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn checkbox_context_budget_is_reported_and_requires_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_checkbox_context_budget.html"))
        .unwrap();
    let last = live
        .schema
        .fields
        .iter()
        .find(|field| field.label.ends_with("Option 16"))
        .unwrap();
    assert!(last.sensitive);
    assert!(last
        .label
        .starts_with("Question context not inspected (review):"));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.contains("Checkbox question context exceeded the inspection limit")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn unlabelled_checkbox_group_context_is_advisory() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_unlabelled_checkboxes.html"))
        .unwrap();
    let group: Vec<_> = live
        .schema
        .fields
        .iter()
        .filter(|field| field.label.contains("Have you worked here before?"))
        .collect();
    assert_eq!(group.len(), 2);
    assert_ne!(group[0].selector, group[1].selector);
    assert!(!live
        .walls
        .iter()
        .any(|wall| wall == "ambiguous form controls need manual review"));
    assert!(group
        .iter()
        .all(|field| field.sensitive && field.label.starts_with("Possible question (review):")));
    for option in ["A", "B", "C", "D"] {
        assert!(live.schema.fields.iter().any(|field| field.sensitive
            && field.label == format!("Question context unavailable (review): {option}")));
    }
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.sensitive
            && field.label == "Question context unavailable (review): Custom"));
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Separate"));
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.label.contains("Misleading hidden question")));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("clicked") && !event.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn unlabelled_radio_groups_are_deduplicated_but_require_manual_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_unlabelled_radios.html"))
        .unwrap();
    assert!(
        !live
            .walls
            .iter()
            .any(|wall| wall == "ambiguous form controls need manual review"),
        "{:?}",
        live.walls
    );
    let groups: Vec<_> = live
        .schema
        .fields
        .iter()
        .filter(|field| matches!(field.field_type, FormFieldType::Radio { .. }))
        .collect();
    assert_eq!(groups.len(), 4);
    for group in &groups {
        assert!(group.sensitive);
        assert_eq!(
            group.field_type,
            FormFieldType::Radio {
                options: vec!["Yes".into(), "No".into()]
            }
        );
    }
    assert!(groups
        .iter()
        .find(|group| group.selector.contains("question-a"))
        .unwrap()
        .label
        .contains("Possible question (review): Question one?"));
    assert!(groups
        .iter()
        .find(|group| group.selector.contains("question-b"))
        .unwrap()
        .label
        .contains("Possible question (review): Question two?"));
    assert!(groups
        .iter()
        .filter(
            |group| group.selector.contains("question-a") || group.selector.contains("question-b")
        )
        .all(|group| group.required));
    assert!(groups
        .iter()
        .find(|group| group.selector.contains("question-c"))
        .unwrap()
        .label
        .starts_with("Unlabelled radio group:"));
    assert!(groups
        .iter()
        .find(|group| group.selector.contains("question-d"))
        .unwrap()
        .label
        .starts_with("Unlabelled radio group:"));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("clicked") && !event.contains("submitted")));
    let other = driver
        .open(&file_url("form_unlabelled_radios_multiple.html"))
        .unwrap();
    assert_eq!(
        other
            .schema
            .fields
            .iter()
            .filter(|field| matches!(field.field_type, FormFieldType::Radio { .. }))
            .count(),
        2
    );
    assert!(other
        .walls
        .iter()
        .any(|wall| wall == "ambiguous form controls need manual review"));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn scroll_choice_with_multiple_id_references() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_scroll_idrefs.html"))
        .unwrap();
    let choice = driver
        .page_custom_choices()
        .unwrap()
        .choices
        .into_iter()
        .find(|choice| choice.label == "City")
        .unwrap();
    let mut later = choice.clone();
    for _ in 0..12 {
        later = driver
            .scroll_custom_choice_options(&choice.selector, true)
            .unwrap();
        if later.options.iter().any(|option| option == "City 35") {
            break;
        }
    }
    assert!(later.options.iter().any(|option| option == "City 35"));
    assert!(driver
        .screenshot_custom_choice_option(&choice.selector, "City 35")
        .is_ok());
    driver
        .select_custom_choice_option(&choice.selector, "City 35")
        .unwrap();
    driver
        .verify_custom_choice_value(&choice.selector, "City 35")
        .unwrap();
    assert!(driver
        .page_text_bounded(5000)
        .unwrap()
        .0
        .contains("NOT SUBMITTED"));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn scroll_choice_reveals_and_selects_later_option() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_scroll.html"))
        .unwrap();
    let inventory = driver.page_custom_choices().unwrap();
    let choice = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "City")
        .unwrap();
    let selector = choice.selector.clone();
    assert!(driver
        .screenshot_custom_choice_option(&selector, "City 01")
        .is_err());
    assert!(driver
        .scroll_custom_choice_options(&selector, false)
        .is_err());
    let mut later = choice.clone();
    for _ in 0..12 {
        later = driver
            .scroll_custom_choice_options(&selector, true)
            .unwrap();
        if later.options.iter().any(|option| option == "City 35") {
            break;
        }
    }
    assert!(
        later.options.iter().any(|option| option == "City 35"),
        "{:?}",
        later.options
    );
    assert!(driver
        .screenshot_custom_choice_option(&selector, "City 35")
        .is_ok());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("clicked") && !event.contains("submitted")));
    driver
        .select_custom_choice_option(&selector, "City 35")
        .unwrap();
    assert!(driver
        .verify_custom_choice_value(&selector, "City 35")
        .is_ok());
    assert!(driver
        .scroll_custom_choice_options(&selector, false)
        .is_ok());
    let (text, _) = driver.page_text_bounded(5000).unwrap();
    assert!(text.contains("NOT SUBMITTED"));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn option_capture_targets_later_unique_choice_without_clicking() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_many.html"))
        .unwrap();
    let inventory = driver.page_custom_choices().unwrap();
    let choice = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Preferred city")
        .unwrap();
    assert_eq!(choice.options[9], "Sydney");
    let selector = choice.selector.clone();
    let capture = driver
        .screenshot_custom_choice_option(&selector, "Sydney")
        .unwrap();
    assert!(!capture.is_empty());
    assert!(driver
        .screenshot_custom_choice_option(&selector, "Duplicate")
        .is_err());
    assert!(driver
        .screenshot_custom_choice_option(&selector, "Missing")
        .is_err());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("clicked") && !event.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against a fictional changing choice"]
fn disappearing_custom_choice_leaves_inventory_incomplete() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_disappears.html"))
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#arm".into(),
        })
        .unwrap();
    let inventory = driver.page_custom_choices().unwrap();
    assert!(inventory.incomplete);
    assert!(inventory.choices.is_empty());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "starts a local browser against a fictional dynamic choice"]
fn dynamic_choice_created_on_open_can_be_inspected_and_selected() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_dynamic_choice.html")).unwrap();
    let choices = driver.page_custom_choices().unwrap();
    let choice = choices
        .choices
        .iter()
        .find(|choice| choice.label == "Position")
        .unwrap();
    assert!(!choice.expanded);
    assert!(choice.options_incomplete);
    let selector = choice.selector.clone();
    assert!(driver.screenshot_field_context(&selector).is_err());
    driver.center_field_for_review(&selector).unwrap();
    assert!(driver.screenshot_field_context(&selector).is_ok());
    let current = driver.page_custom_choices().unwrap();
    assert!(current
        .choices
        .iter()
        .any(|choice| choice.selector == selector && !choice.expanded));
    let opened = driver.open_custom_choice(&selector).unwrap();
    assert!(opened.expanded);
    assert_eq!(opened.options, ["Designer", "Engineer", "Analyst"]);
    assert!(opened.options_incomplete);
    driver
        .select_custom_choice_option(&selector, "Engineer")
        .unwrap();
    driver
        .verify_custom_choice_value(&selector, "Engineer")
        .unwrap();
    assert!(driver
        .page_text_bounded(2000)
        .unwrap()
        .0
        .contains("NOT SUBMITTED"));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "starts a local browser against a fictional clipped popup"]
fn dynamic_choice_at_page_end_keeps_incomplete_option_inventory() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let url = format!("{}#limited", file_url("form_dynamic_choice.html"));
    driver.open(&url).unwrap();
    let choices = driver.page_custom_choices().unwrap();
    let selector = choices
        .choices
        .iter()
        .find(|choice| choice.label == "Position")
        .unwrap()
        .selector
        .clone();
    driver.center_field_for_review(&selector).unwrap();
    let opened = driver.open_custom_choice(&selector).unwrap();
    assert!(opened.options_incomplete);
    assert!(
        opened.options.len() < 3,
        "fixture must leave some options out of view"
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn opening_custom_choice_rejects_submit_controls_and_reports_options() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_choice_open.html"))
        .unwrap();
    let inventory = driver.page_custom_choices().unwrap();
    let location = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Location")
        .unwrap()
        .selector
        .clone();
    let danger = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Danger")
        .unwrap()
        .selector
        .clone();
    let work_mode = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Work mode")
        .unwrap()
        .selector
        .clone();
    let opened = driver.open_custom_choice(&location).unwrap();
    assert!(opened.expanded);
    assert_eq!(
        opened.options,
        [
            "Berlin",
            "Lisbon",
            "Rome",
            "Duplicate",
            "Duplicate",
            "Send",
            "Hover duplicate",
            "Changing action"
        ]
    );
    assert!(driver.open_custom_choice(&location).is_err());
    driver
        .select_custom_choice_option(&location, "Berlin")
        .unwrap();
    assert!(driver
        .verify_custom_choice_value(&location, "Berlin")
        .is_ok());
    assert!(driver
        .verify_custom_choice_value(&location, "Lisbon")
        .is_err());
    assert!(driver
        .verify_custom_choice_value("#missing-choice", "Berlin")
        .is_err());
    assert!(driver
        .select_custom_choice_option(&location, "Duplicate")
        .is_err());
    assert!(driver
        .select_custom_choice_option(&location, "Send")
        .is_err());
    assert!(driver
        .select_custom_choice_option(&location, "Rome")
        .is_err());
    assert!(driver
        .select_custom_choice_option(&location, "Hover duplicate")
        .is_err());
    assert!(driver
        .select_custom_choice_option(&location, "Changing action")
        .is_err());
    assert!(driver
        .select_custom_choice_option(&location, "Unavailable")
        .is_err());
    assert!(driver.open_custom_choice(&danger).is_err());
    driver
        .select_custom_choice_option(&work_mode, "Remote")
        .unwrap();
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("NOT SUBMITTED"), "{text}");
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against fictional custom choice controls"]
fn custom_choice_inventory_reads_rendered_options_without_interaction() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_custom_choices.html")).unwrap();
    let inventory = driver.page_custom_choices().unwrap();
    let location = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Job location")
        .unwrap();
    assert!(location.expanded);
    assert_eq!(location.options, ["Berlin", "Lisbon"]);
    assert!(location.options_incomplete);
    let department = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Department")
        .unwrap();
    assert!(!department.expanded && department.options.is_empty() && department.options_incomplete);
    let ambiguous = inventory
        .choices
        .iter()
        .find(|choice| choice.label == "Ambiguous")
        .unwrap();
    assert!(ambiguous.expanded && ambiguous.options.is_empty() && ambiguous.options_incomplete);
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("NOT SUBMITTED"), "{text}");
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches a browser against a fictional hidden-control fixture"]
fn required_hidden_proxy_is_not_exposed_as_a_text_answer() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&file_url("form_hidden_proxy.html")).unwrap();
    assert!(live.schema.fields.iter().any(|field| field.label == "Name"));
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.selector.contains("location-proxy")));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall == "a required hidden control needs manual review"));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall.contains("custom control needs manual review: Job location")));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("typed") && !event.contains("submitted")));
}

#[test]
#[ignore = "starts one local browser against fictional submission controls"]
fn generic_click_refuses_native_submit_controls_regardless_of_label() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_generic_click_submit_guard.html"))
        .unwrap();
    for selector in [
        "#implicit",
        "#explicit",
        "#inside",
        "#image",
        "#submit-wrapper",
        "#submit-label",
    ] {
        let error = driver
            .execute(&BrowserCommand::Click {
                selector: selector.into(),
            })
            .expect_err("generic click must reject submit controls");
        assert!(
            error.0.contains("could submit or navigate"),
            "{selector}: {error:?}"
        );
    }
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("INITIAL"));
    assert!(!text.contains("SUBMITTED"));
    driver
        .execute(&BrowserCommand::Click {
            selector: "#safe".into(),
        })
        .unwrap();
    assert!(driver
        .page_text_bounded(2000)
        .unwrap()
        .0
        .contains("DISMISSED"));
}

#[test]
#[ignore = "uses one browser and a loopback WebSocket to evaluate offline emulation"]
fn offline_emulation_existing_websocket_qualification() {
    use serde_json::{json, Value};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use waypoint_browser::cdp::{BrowserProcess, Cdp};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(8)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let baseline = ws
            .read()
            .ok()
            .is_some_and(|value| value.to_text().ok() == Some("baseline"));
        tx.send(baseline).unwrap();
        let message = ws.read().ok();
        tx.send(message.is_some_and(|value| value.to_text().ok() == Some("fictional-test")))
            .unwrap();
    });
    let browser = BrowserProcess::launch(Duration::from_secs(20)).unwrap();
    let mut cdp = Cdp::new(browser.connect().unwrap()).unwrap();
    cdp.call("Network.enable", json!({})).unwrap();
    cdp.call("Runtime.enable", json!({})).unwrap();
    cdp.call("Runtime.evaluate", json!({"expression":format!("window.fixtureSocket = new WebSocket('ws://127.0.0.1:{port}');"),"returnByValue":true})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let state = cdp
            .call(
                "Runtime.evaluate",
                json!({"expression":"window.fixtureSocket.readyState","returnByValue":true}),
            )
            .unwrap();
        if state.pointer("/result/value").and_then(Value::as_u64) == Some(1) {
            break;
        }
        assert!(Instant::now() < deadline, "fixture socket did not connect");
        std::thread::sleep(Duration::from_millis(30));
    }
    cdp.call(
        "Runtime.evaluate",
        json!({"expression":"window.fixtureSocket.send('baseline')","returnByValue":true}),
    )
    .unwrap();
    assert!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "loopback baseline not received"
    );
    cdp.call(
        "Network.emulateNetworkConditions",
        json!({"offline":true,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1}),
    )
    .unwrap();
    let result=cdp.call("Runtime.evaluate", json!({"expression":"window.fixtureSocket.send('fictional-test'); 'sent'","returnByValue":true})).unwrap();
    let before_restore = rx.recv_timeout(Duration::from_secs(1)).ok();
    cdp.call(
        "Network.emulateNetworkConditions",
        json!({"offline":false,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1}),
    )
    .unwrap();
    let received =
        before_restore.unwrap_or_else(|| rx.recv_timeout(Duration::from_secs(9)).unwrap());
    println!(
        "send_result={} received_before_restore={before_restore:?} received_by_end={received}",
        result["result"]["value"]
    );
    server.join().unwrap();
    // Observation only: this test does not establish cross-target isolation.
}

#[test]
#[ignore = "uses one browser and a loopback WebSocket to evaluate request interception"]
fn fetch_interception_does_not_block_existing_websocket_messages() {
    use serde_json::{json, Value};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use waypoint_browser::cdp::{BrowserProcess, Cdp};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(8)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        tx.send(
            ws.read()
                .ok()
                .is_some_and(|value| value.to_text().ok() == Some("fictional")),
        )
        .unwrap();
    });
    let browser = BrowserProcess::launch(Duration::from_secs(20)).unwrap();
    let mut cdp = Cdp::new(browser.connect().unwrap()).unwrap();
    cdp.call(
        "Network.enable",
        json!({"maxTotalBufferSize":0,"maxResourceBufferSize":0,"maxPostDataSize":0}),
    )
    .unwrap();
    cdp.call("Runtime.enable", json!({})).unwrap();
    cdp.call("Runtime.evaluate", json!({"expression":format!("window.fixtureSocket = new WebSocket('ws://127.0.0.1:{port}');"),"returnByValue":true})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let state = cdp
            .call(
                "Runtime.evaluate",
                json!({"expression":"window.fixtureSocket.readyState","returnByValue":true}),
            )
            .unwrap();
        if state.pointer("/result/value").and_then(Value::as_u64) == Some(1) {
            break;
        }
        assert!(Instant::now() < deadline, "fixture socket did not connect");
        std::thread::sleep(Duration::from_millis(30));
    }
    cdp.call(
        "Fetch.enable",
        json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
    )
    .unwrap();
    let before = cdp.observed_non_read_requests();
    cdp.call(
        "Runtime.evaluate",
        json!({"expression":"window.fixtureSocket.send('fictional')","returnByValue":true}),
    )
    .unwrap();
    assert!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "fixture message not received"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while cdp.observed_non_read_requests() == before && Instant::now() < deadline {
        cdp.pump(Duration::from_millis(100));
    }
    assert!(
        cdp.observed_non_read_requests() > before,
        "outgoing frame was not observed"
    );
    assert!(
        cdp.events
            .iter()
            .all(|(name, _)| name != "Network.webSocketFrameSent"),
        "frame payload was retained"
    );
    server.join().unwrap();
}

#[test]
#[ignore = "uses a local browser and loopback server to observe a fictional input-triggered request"]
fn observes_non_read_request_triggered_by_field_input() {
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::time::{Duration, Instant};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = stop.clone();
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        while !stop_worker.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buffer = [0u8; 8192];
                    let count = stream.read(&mut buffer).unwrap_or(0);
                    if buffer[..count].starts_with(b"POST ") {
                        let _ = tx.send(());
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    } else {
                        let body = "<!doctype html><label for='name'>Name</label><input id='name' oninput=\"fetch('/fictional', {method:'POST',body:'fixture'})\">";
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(_) => break,
            }
        }
    });
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&format!("http://127.0.0.1:{port}/")).unwrap();
    let before = driver.observed_non_read_requests().unwrap();
    if let Err(error) = driver.execute(&BrowserCommand::Fill {
        selector: "#name".into(),
        value: PacketValue::Text("A".into()),
    }) {
        assert!(
            error.to_string().contains("non-GET/HEAD request"),
            "{error}"
        );
    }
    rx.recv_timeout(Duration::from_secs(5))
        .expect("fictional POST did not reach loopback server");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if driver.observed_non_read_requests().unwrap() > before {
            break;
        }
        assert!(Instant::now() < deadline, "request event was not observed");
        std::thread::sleep(Duration::from_millis(30));
    }
    drop(driver);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
}

#[test]
#[ignore = "uses a local browser and loopback WebSocket on a fictional form"]
fn observes_websocket_frame_triggered_by_field_input() {
    use std::io::{Read, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let socket_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    socket_listener.set_nonblocking(true).unwrap();
    let socket_port = socket_listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let socket_server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match socket_listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(error) => panic!("fixture WebSocket accept failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        tx.send(socket.read().unwrap().to_text().unwrap() == "fictional")
            .unwrap();
    });
    let page_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let page_port = page_listener.local_addr().unwrap().port();
    let page_server = std::thread::spawn(move || {
        let (mut stream, _) = page_listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 2048];
        let _ = stream.read(&mut request);
        let body = format!("<!doctype html><label for='name'>Name</label><input id='name' oninput=\"socket.send('fictional')\"><script>const socket=new WebSocket('ws://127.0.0.1:{socket_port}');</script>");
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&format!("http://127.0.0.1:{page_port}/"))
        .unwrap();
    let before = driver.observed_non_read_requests().unwrap();
    if let Err(error) = driver.execute(&BrowserCommand::Fill {
        selector: "#name".into(),
        value: PacketValue::Text("A".into()),
    }) {
        assert!(
            error.to_string().contains("outgoing WebSocket frame"),
            "{error}"
        );
    }
    assert!(rx.recv_timeout(Duration::from_secs(8)).unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while driver.observed_non_read_requests().unwrap() == before {
        assert!(Instant::now() < deadline, "outgoing frame was not observed");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
    drop(driver);
    page_server.join().unwrap();
    socket_server.join().unwrap();
}

#[test]
#[ignore = "read-only navigation to an explicitly supplied public listing after robots and policy checks"]
fn public_listing_navigation_probe() {
    let url = std::env::var("WAYPOINT_NAVIGATION_URL").expect("supply a public listing URL");
    let parsed = waypoint_net::parse_url(&url).expect("valid HTTPS URL");
    waypoint_net::resolve_public(&parsed).expect("public DNS addresses");
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    println!("browser version: {}", driver.browser_version().unwrap());
    match driver.open(&url) {
        Ok(form) => println!(
            "navigation succeeded; {} fields; review walls: {:?}",
            form.schema.fields.len(),
            form.walls
        ),
        Err(error) => panic!("navigation failed: {error}"),
    }
    assert!(driver.trace.iter().all(|entry| !entry.contains("clicked")
        && !entry.contains("submitted")
        && !entry.contains("typed")));
}

#[test]
#[ignore = "read-only navigation to a policy-checked public listing using an identified research client"]
fn identified_public_navigation_probe() {
    use serde_json::json;
    use std::time::Duration;
    use waypoint_browser::cdp::{BrowserProcess, Cdp};

    let url = std::env::var("WAYPOINT_NAVIGATION_URL").expect("supply a public listing URL");
    let parsed = waypoint_net::parse_url(&url).expect("valid HTTPS URL");
    waypoint_net::resolve_public(&parsed).expect("public DNS addresses");
    let browser = BrowserProcess::launch(Duration::from_secs(20)).unwrap();
    let mut cdp = Cdp::new(browser.connect().unwrap()).unwrap();
    cdp.call("Page.enable", json!({})).unwrap();
    cdp.call("Network.enable", json!({})).unwrap();
    cdp.call(
        "Network.setUserAgentOverride",
        json!({"userAgent":"WaypointResearch/1.0"}),
    )
    .unwrap();
    let navigation = cdp.call("Page.navigate", json!({"url":url})).unwrap();
    println!("navigation error: {}", navigation["errorText"]);
    assert!(navigation["errorText"].as_str().is_none_or(str::is_empty));
    assert_ne!(navigation["isDownload"], true);
    cdp.pump(Duration::from_secs(3));
    let status = cdp.events.iter().find_map(|(name, event)| {
        (name == "Network.responseReceived"
            && event.pointer("/params/frameId") == navigation.get("frameId")
            && event.pointer("/params/loaderId") == navigation.get("loaderId"))
        .then(|| {
            event
                .pointer("/params/status")
                .and_then(serde_json::Value::as_u64)
        })
        .flatten()
    });
    println!("document status: {status:?}");
    assert!(status.is_some_and(|value| (200..300).contains(&value)));
}

#[test]
#[ignore = "uses two local browser targets and a loopback WebSocket"]
fn offline_emulation_target_scope_qualification() {
    use serde_json::{json, Value};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use waypoint_browser::cdp::{BrowserProcess, Cdp};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(6)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        for expected in ["baseline", "second-target"] {
            tx.send(
                ws.read()
                    .ok()
                    .is_some_and(|value| value.to_text().ok() == Some(expected)),
            )
            .unwrap();
        }
    });
    let browser = BrowserProcess::launch(Duration::from_secs(20)).unwrap();
    let mut cdp = Cdp::new(browser.connect().unwrap()).unwrap();
    cdp.call("Network.enable", json!({})).unwrap();
    let target = cdp
        .call_browser("Target.createTarget", json!({"url":"about:blank"}))
        .unwrap();
    let target_id = target["targetId"].as_str().unwrap();
    let attach = cdp
        .call_browser(
            "Target.attachToTarget",
            json!({"targetId":target_id,"flatten":true}),
        )
        .unwrap();
    let second = attach["sessionId"].as_str().unwrap().to_owned();
    cdp.call_session(&second, "Runtime.enable", json!({}))
        .unwrap();
    cdp.call_session(&second, "Runtime.evaluate", json!({"expression":format!("window.fixtureSocket = new WebSocket('ws://127.0.0.1:{port}');"),"returnByValue":true})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let state = cdp
            .call_session(
                &second,
                "Runtime.evaluate",
                json!({"expression":"window.fixtureSocket.readyState","returnByValue":true}),
            )
            .unwrap();
        if state.pointer("/result/value").and_then(Value::as_u64) == Some(1) {
            break;
        }
        assert!(Instant::now() < deadline, "fixture socket did not connect");
        std::thread::sleep(Duration::from_millis(30));
    }
    cdp.call_session(
        &second,
        "Runtime.evaluate",
        json!({"expression":"window.fixtureSocket.send('baseline')","returnByValue":true}),
    )
    .unwrap();
    assert!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "loopback baseline not received"
    );
    cdp.call(
        "Network.emulateNetworkConditions",
        json!({"offline":true,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1}),
    )
    .unwrap();
    cdp.call_session(
        &second,
        "Runtime.evaluate",
        json!({"expression":"window.fixtureSocket.send('second-target')","returnByValue":true}),
    )
    .unwrap();
    let received = rx.recv_timeout(Duration::from_secs(7)).unwrap();
    println!("other_target_delivered_while_first_offline={received}");
    server.join().unwrap();
}

#[test]
#[ignore = "launches a browser against a fictional pointer fixture"]
fn pointer_moves_in_bounded_steps_between_clicks() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_pointer_path.html")).unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#first".into(),
        })
        .unwrap();
    let start = std::time::Instant::now();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#second".into(),
        })
        .unwrap();
    println!(
        "fictional pointer movement and click: {:?}",
        start.elapsed()
    );
    let moves = driver
        .read_attribute("#events", "data-moves")
        .unwrap()
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(
        (2..=5).contains(&moves),
        "unexpected pointer event count: {moves}"
    );
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("SECOND CLICKED"), "{text}");
}

#[test]
#[ignore = "launches a browser against a fictional covered frame"]
fn click_stops_if_an_ancestor_frame_is_covered() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_frame_overlay.html")).unwrap();
    let selector = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Name")
        .expect("embedded field")
        .selector
        .clone();
    driver
        .execute(&BrowserCommand::Click {
            selector: selector.clone(),
        })
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#cover".into(),
        })
        .unwrap();
    assert!(driver.execute(&BrowserCommand::Click { selector }).is_err());
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("NOT SUBMITTED"), "{text}");
}

#[test]
#[ignore = "launches one browser against a fictional click-overlay fixture"]
fn click_stops_if_hover_covers_target_with_submit_control() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_click_overlay.html")).unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#safe".into(),
        })
        .unwrap();
    let error = driver
        .execute(&BrowserCommand::Click {
            selector: "#target".into(),
        })
        .unwrap_err();
    assert!(error.0.contains("covered"), "{error}");
    let error = driver
        .execute(&BrowserCommand::Click {
            selector: "#asymmetric".into(),
        })
        .unwrap_err();
    assert!(error.0.contains("covered"), "{error}");
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("SAFE CLICK"), "{text}");
    assert!(text.contains("NOT SUBMITTED"), "{text}");
}

#[test]
#[ignore = "launches one browser against a fictional keyboard fixture"]
fn text_entry_emits_keys_and_stops_when_focus_moves() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_keyboard_events.html")).unwrap();
    let fill = |selector: &str, value: &str| BrowserCommand::Fill {
        selector: selector.into(),
        value: PacketValue::Text(value.into()),
    };
    let sample = "Abcdefghijklmnopqrstuvwxyz 0123456789";
    let started = std::time::Instant::now();
    driver.execute(&fill("#name", sample)).unwrap();
    let elapsed = started.elapsed();
    driver.verify_text_value("#name", sample).unwrap();
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains(sample), "missing keyboard events: {text}");
    println!(
        "fictional keyboard entry: {} characters in {:?}",
        sample.len(),
        elapsed
    );
    let email = "A.test+1@example.invalid";
    driver.execute(&fill("#name", email)).unwrap();
    driver.verify_text_value("#name", email).unwrap();
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(
        text.contains(email),
        "missing punctuation key events: {text}"
    );
    let unicode = "Renée 🧭";
    driver.execute(&fill("#name", unicode)).unwrap();
    driver.verify_text_value("#name", unicode).unwrap();
    // More than 4,000 UTF-8 bytes but within the 4,000 UTF-16-unit form
    // answer limit. Verify the browser can also read back the complete value.
    let multilingual = "é".repeat(2001);
    driver.execute(&fill("#name", &multilingual)).unwrap();
    driver.verify_text_value("#name", &multilingual).unwrap();
    assert_eq!(
        driver.text_value_snapshot("#name").unwrap().value,
        multilingual
    );
    let long = "Relevant experience with international teams and résumé review. 🧭 ".repeat(3);
    driver.execute(&fill("#name", &long)).unwrap();
    driver.verify_text_value("#name", &long).unwrap();
    assert!(driver.execute(&fill("#long-redirect", &long)).is_err());
    driver.verify_text_value("#other", "").unwrap();
    assert!(driver
        .execute(&fill("#rich-redirect", &format!("{long}\nSecond line")))
        .is_err());
    driver.verify_text_value("#other", "").unwrap();
    assert!(driver.execute(&fill("#role-change", &long)).is_err());
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("FIRST SEGMENT ONLY"), "{text}");
    assert!(!text.contains("CONTINUED UNSAFELY"), "{text}");
    assert!(driver.execute(&fill("#redirect", "xyz")).is_err());
    driver.verify_text_value("#other", "").unwrap();
    let (text, _) = driver.page_text_bounded(2000).unwrap();
    assert!(text.contains("NOT SUBMITTED"), "{text}");
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
    for selector in ["#disabled-check", "#changing-check"] {
        assert!(driver
            .execute(&BrowserCommand::Fill {
                selector: selector.into(),
                value: PacketValue::Bool(true),
            })
            .is_err());
    }
    for selector in ["#disabled-date", "#changing-date"] {
        assert!(driver
            .execute(&BrowserCommand::Fill {
                selector: selector.into(),
                value: PacketValue::Date("2026-11-01".into()),
            })
            .is_err());
    }
    for selector in ["#disabled-select", "#changing-select"] {
        assert!(driver
            .execute(&BrowserCommand::SelectOption {
                selector: selector.into(),
                label: "Beta".into(),
            })
            .is_err());
    }
    assert!(driver
        .execute(&BrowserCommand::SelectOptions {
            selector: "#changing-options".into(),
            labels: vec!["Beta".into()],
        })
        .is_err());
    let file = upload_dir().join("fictional-attachment.pdf");
    std::fs::write(&file, b"fictional local fixture").unwrap();
    driver.attach_local_file("#kept-file", &file).unwrap();
    assert!(driver.attach_local_file("#cleared-file", &file).is_err());
    driver.verify_local_file("#kept-file", &file).unwrap();

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
    assert!(driver.verify_local_file("#kept-file", &file).is_err());
    std::fs::remove_file(&file).unwrap();
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
    assert_eq!(live.walls, vec!["an unavailable field needs manual review"]);
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
    for (group_label, option, selector) in [
        ("Travel preference", "Yes", "#travel-yes"),
        ("Shift preference", "Night", "#shift-night"),
    ] {
        let field = live
            .schema
            .fields
            .iter()
            .find(|field| field.label == group_label)
            .unwrap();
        let choice = BrowserCommand::SelectRadio {
            selector: field.selector.clone(),
            label: option.into(),
        };
        driver.execute(&choice).unwrap();
        driver.verify_choice(&choice).unwrap();
        assert_eq!(
            driver
                .read_attribute(selector, "data-seen")
                .unwrap()
                .as_deref(),
            Some("true")
        );
    }
    assert!(!live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Outdated legend"));
    let availability = live
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Interview availability")
        .unwrap();
    assert_eq!(
        availability.field_type,
        FormFieldType::Radio {
            options: vec!["Yes".into()]
        }
    );
    assert!(driver
        .execute(&BrowserCommand::SelectRadio {
            selector: availability.selector.clone(),
            label: "No".into()
        })
        .is_err());
    let yes = BrowserCommand::SelectRadio {
        selector: availability.selector.clone(),
        label: "Yes".into(),
    };
    driver.execute(&yes).unwrap();
    driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#lock-trigger".into(),
            label: "Lock".into(),
        })
        .unwrap();
    assert!(driver.verify_choice(&yes).is_err());
    assert!(driver.execute(&yes).is_err());
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
#[ignore = "launches one browser against a local fixture without submitting"]
fn controls_reject_an_aria_disabled_ancestor_after_inspection() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver.open(&file_url("form_aria_locked.html")).unwrap();
    let options = &live
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#single")
        .unwrap()
        .field_type;
    assert_eq!(
        options,
        &FormFieldType::Select {
            options: vec!["A".into(), "B".into(), "D".into()]
        }
    );
    let file = upload_dir().join("aria-state-fixture.txt");
    std::fs::write(&file, b"fictional attachment").unwrap();
    let text = BrowserCommand::Fill {
        selector: "#name".into(),
        value: PacketValue::Text("Avery".into()),
    };
    let date = BrowserCommand::Fill {
        selector: "#date".into(),
        value: PacketValue::Date("2026-11-01".into()),
    };
    let check = BrowserCommand::Fill {
        selector: "#check".into(),
        value: PacketValue::Bool(true),
    };
    let single = BrowserCommand::SelectOption {
        selector: "#single".into(),
        label: "B".into(),
    };
    let multiple = BrowserCommand::SelectOptions {
        selector: "#multiple".into(),
        labels: vec!["A".into(), "B".into()],
    };
    let editor = BrowserCommand::Fill {
        selector: "#editor".into(),
        value: PacketValue::Text("Notes".into()),
    };
    for action in [&text, &date, &check, &single, &multiple, &editor] {
        driver.execute(action).unwrap();
    }
    assert!(driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#single".into(),
            label: "C".into()
        })
        .is_err());
    assert!(driver
        .execute(&BrowserCommand::SelectOptions {
            selector: "#multiple".into(),
            labels: vec!["C".into()]
        })
        .is_err());
    driver.attach_local_file("#file", &file).unwrap();
    driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#option-lock".into(),
            label: "Lock option".into(),
        })
        .unwrap();
    let option_changed = driver.inspect_current().unwrap();
    assert_eq!(
        option_changed
            .schema
            .fields
            .iter()
            .find(|field| field.selector == "#single")
            .unwrap()
            .field_type,
        FormFieldType::Select {
            options: vec!["A".into(), "B".into()]
        }
    );
    assert_ne!(
        live.schema.schema_hash_input(),
        option_changed.schema.schema_hash_input()
    );
    driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#lock".into(),
            label: "Lock".into(),
        })
        .unwrap();
    let updated = driver.inspect_current().unwrap();
    assert!(updated
        .schema
        .fields
        .iter()
        .all(|field| field.selector != "#single"));
    assert!(updated
        .walls
        .iter()
        .any(|wall| wall == "an unavailable field needs manual review"));
    assert!(updated
        .walls
        .iter()
        .any(|wall| wall == "a required field is disabled or read-only"));
    assert_ne!(
        live.schema.schema_hash_input(),
        updated.schema.schema_hash_input()
    );
    for action in [&text, &date, &check, &single, &multiple, &editor] {
        assert!(
            driver.execute(action).is_err(),
            "locked control accepted {action:?}"
        );
    }
    assert!(driver.verify_text_value("#name", "Avery").is_err());
    assert!(driver.verify_text_value("#date", "2026-11-01").is_err());
    assert!(driver.verify_text_value("#editor", "Notes").is_err());
    for action in [&check, &single, &multiple] {
        assert!(driver.verify_choice(action).is_err());
    }
    assert!(driver.verify_local_file("#file", &file).is_err());
    assert!(driver.attach_local_file("#file", &file).is_err());
    assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    std::fs::remove_file(&file).unwrap();
}

#[test]
#[ignore = "launches one browser against a local fixture without submitting"]
fn ambiguous_fieldset_accessible_name_requires_review() {
    let mut driver = CdpDriver::launch(vec![]).expect("launch browser");
    let live = driver
        .open(&file_url("form_radio_unresolved.html"))
        .unwrap();
    assert!(
        live.walls.iter().any(
            |wall| wall.contains("radio group has an unresolved or ambiguous accessible label")
        ),
        "{:?}",
        live.walls
    );
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
fn native_text_limits_are_observed_in_the_live_schema() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&file_url("form_maxlength.html")).unwrap();
    let field = |label: &str| {
        live.schema
            .fields
            .iter()
            .find(|f| f.label == label)
            .unwrap()
    };
    assert_eq!(field("Name").max_length_utf16, Some(3));
    assert_eq!(field("Name").min_length_utf16, Some(2));
    assert_eq!(field("Answer").max_length_utf16, Some(5));
    assert_eq!(field("Answer").min_length_utf16, Some(4));
    assert_eq!(field("Invalid limit").max_length_utf16, None);
    assert_eq!(field("Invalid limit").min_length_utf16, None);
    let mut changed = live.schema.clone();
    changed
        .fields
        .iter_mut()
        .find(|f| f.label == "Name")
        .unwrap()
        .max_length_utf16 = Some(4);
    assert_ne!(changed.schema_hash_input(), live.schema.schema_hash_input());
    let mut changed_minimum = live.schema.clone();
    changed_minimum
        .fields
        .iter_mut()
        .find(|f| f.label == "Name")
        .unwrap()
        .min_length_utf16 = Some(1);
    assert_ne!(
        changed_minimum.schema_hash_input(),
        live.schema.schema_hash_input()
    );
}

#[test]
#[ignore = "launches a real browser with a fictional rich text fixture"]
fn labelled_rich_textbox_accepts_plain_text_and_checks_retention() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_rich_textbox.html")).unwrap();
    assert_eq!(
        page.schema.fields.len(),
        2,
        "unlabelled and disabled editors must not be offered"
    );
    assert!(!page
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Disabled editor"));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == "an unavailable field needs manual review"));
    let field = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Motivation")
        .unwrap();
    assert_eq!(field.field_type, FormFieldType::TextArea);
    assert_eq!(
        field.editing_mode,
        Some(waypoint_applications::form_mapping::EditingMode::RichText)
    );
    assert!(field.required);
    let selector = field.selector.clone();
    driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text("Fictional motivation".into()),
        })
        .unwrap();
    driver
        .verify_text_value(&selector, "Fictional motivation")
        .unwrap();
    driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text("First line\nSecond line".into()),
        })
        .unwrap();
    driver
        .verify_text_value(&selector, "First line\nSecond line")
        .unwrap();
    driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text("First\n\nThird".into()),
        })
        .unwrap();
    driver
        .verify_text_value(&selector, "First\n\nThird")
        .unwrap();
    let long = format!(
        "{}\nSecond line",
        "Relevant résumé experience with international teams. "
            .repeat(4)
            .trim_end()
    );
    driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text(long.clone()),
        })
        .unwrap();
    driver.verify_text_value(&selector, &long).unwrap();
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text("line\n".repeat(41))
        })
        .is_err());
    driver.verify_text_value(&selector, &long).unwrap();
    for selector in ["#unsafe", "#disabled"] {
        assert!(driver
            .execute(&BrowserCommand::Fill {
                selector: selector.into(),
                value: PacketValue::Text("must not enter".into())
            })
            .is_err());
    }
    driver
        .execute(&BrowserCommand::Click {
            selector: "#mode".into(),
        })
        .unwrap();
    let changed = driver.inspect_current().unwrap();
    let changed_field = changed
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Motivation")
        .unwrap();
    assert_eq!(changed_field.selector, selector);
    assert_eq!(
        changed_field.editing_mode,
        Some(waypoint_applications::form_mapping::EditingMode::PlainText)
    );
    assert_ne!(
        page.schema.schema_hash_input(),
        changed.schema.schema_hash_input()
    );
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser against a fictional embedded form"]
fn visible_embedded_form_can_be_filled_with_review_and_rejects_stale_scope() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_iframe.html")).unwrap();
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::UNINSPECTED_FRAME_REVIEW));
    assert!(page
        .schema
        .fields
        .iter()
        .any(|field| field.selector == "#candidate"));
    let embedded = page
        .schema
        .fields
        .iter()
        .find(|field| field.label.contains("Question"))
        .expect("same-origin embedded question")
        .selector
        .clone();
    driver
        .execute(&BrowserCommand::Fill {
            selector: embedded.clone(),
            value: PacketValue::Text("Fictional embedded answer".into()),
        })
        .unwrap();
    driver
        .verify_text_value(&embedded, "Fictional embedded answer")
        .unwrap();
    assert!(!page
        .schema
        .fields
        .iter()
        .any(|field| field.label.contains("Secret question")));
    driver
        .execute(&BrowserCommand::Click {
            selector: "#replace-frame".into(),
        })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: embedded.clone(),
            value: PacketValue::Text("Must not reach replacement".into()),
        })
        .is_err());
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#candidate".into(),
            value: PacketValue::Text("Fictional candidate".into()),
        })
        .unwrap();
    driver
        .verify_text_value("#candidate", "Fictional candidate")
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#hide-frame".into(),
        })
        .unwrap();
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: embedded,
            value: PacketValue::Text("Must not be typed".into()),
        })
        .is_err());
    let after = driver.inspect_current().unwrap();
    assert!(
        !after
            .walls
            .iter()
            .any(|wall| wall == waypoint_browser::UNINSPECTED_FRAME_REVIEW),
        "hidden and zero-sized frames must not add a review warning: {:?}",
        after.walls
    );
}

#[test]
#[ignore = "launches a real browser against fictional inaccessible frames"]
fn sandboxed_out_of_process_form_is_detected_without_exposing_fields() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver
        .open(&file_url("form_inaccessible_frame.html"))
        .unwrap();
    assert!(page.schema.fields.is_empty());
    // A separate owned target reveals the presence of controls without returning fields.
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::UNINSPECTED_FRAME_REVIEW));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW));
    driver
        .execute(&BrowserCommand::Click {
            selector: "#hide-unavailable".into(),
        })
        .unwrap();
    let hidden = driver.inspect_current().unwrap();
    assert!(!hidden
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches one browser against fictional isolated-frame budget fixture"]
fn isolated_frame_budget_reports_uninspected_content() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_isolated_budget.html")).unwrap();
    assert!(page.schema.fields.is_empty());
    assert!(
        page.walls
            .iter()
            .any(|wall| wall == waypoint_browser::ISOLATED_FRAME_INSPECTION_UNAVAILABLE),
        "{:?}",
        page.walls
    );
    assert!(!page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW));
}

#[test]
#[ignore = "launches one browser and two fictional local origins"]
fn inaccessible_same_site_cross_origin_form_is_detected_without_fields() {
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let child = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let parent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let child_port = child.local_addr().unwrap().port();
    let parent_port = parent.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let mut threads = vec![];
    for (listener, body) in [(child, "<form><label>Name <input name='name'></label></form>".to_owned()), (parent, format!("<iframe id='frame' src='http://127.0.0.1:{child_port}/'></iframe><button id='hide' type='button' onclick=\"document.getElementById('frame').style.display='none'\">Hide</button>"))] {
        listener.set_nonblocking(true).unwrap();
        let stop = stop.clone();
        threads.push(std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
                        let mut request = [0u8; 2048]; let _ = stream.read(&mut request);
                        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(10)),
                    Err(_) => break,
                }
            }
        }));
    }
    let outcome = std::panic::catch_unwind(|| {
        let mut driver = CdpDriver::launch(vec![]).unwrap();
        let page = driver
            .open(&format!("http://127.0.0.1:{parent_port}/"))
            .unwrap();
        assert!(page.schema.fields.is_empty());
        assert!(
            page.walls
                .iter()
                .any(|wall| wall == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW),
            "{:?}",
            page.walls
        );
        driver
            .execute(&BrowserCommand::Click {
                selector: "#hide".into(),
            })
            .unwrap();
        assert!(!driver
            .inspect_current()
            .unwrap()
            .walls
            .iter()
            .any(|wall| wall == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW));
    });
    stop.store(true, Ordering::Relaxed);
    for thread in threads {
        thread.join().unwrap();
    }
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}

#[test]
#[ignore = "launches one browser against five fictional mixed scopes"]
fn scope_limit_reports_a_fifth_uninspected_scope() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_five_scopes.html")).unwrap();
    assert!(page.schema.fields.is_empty(), "{:?}", page.schema.fields);
    assert!(
        page.walls
            .iter()
            .any(|wall| wall == waypoint_browser::EMBEDDED_SCOPE_LIMIT_REVIEW),
        "{:?}",
        page.walls
    );
}

#[test]
#[ignore = "launches one browser against four fictional mixed scopes"]
fn four_mixed_scopes_can_be_filled_and_reject_hidden_ancestors() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_four_scopes.html")).unwrap();
    assert!(!page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::EMBEDDED_SCOPE_LIMIT_REVIEW));
    let selector = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Deep name")
        .expect("four-scope field")
        .selector
        .clone();
    assert_eq!(selector.matches("waypoint-shadow:").count(), 2);
    assert_eq!(selector.matches("waypoint-frame:").count(), 2);
    driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text("Fictional".into()),
        })
        .unwrap();
    driver.verify_text_value(&selector, "Fictional").unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#hide".into(),
        })
        .unwrap();
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector,
            value: PacketValue::Text("Must not be entered".into())
        })
        .is_err());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches one browser against a fictional mixed-scope form"]
fn mixed_shadow_frame_shadow_field_binds_every_scope() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_mixed_scopes.html")).unwrap();
    let field = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Mixed name")
        .expect("mixed-scope field");
    let selector = field.selector.clone();
    assert_eq!(selector.matches("waypoint-shadow:").count(), 2);
    assert_eq!(selector.matches("waypoint-frame:").count(), 1);
    let fill = |value: &str| BrowserCommand::Fill {
        selector: selector.clone(),
        value: PacketValue::Text(value.into()),
    };
    driver.execute(&fill("Fictional")).unwrap();
    driver.verify_text_value(&selector, "Fictional").unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#hide".into(),
        })
        .unwrap();
    assert!(driver.execute(&fill("Must not be entered")).is_err());
    driver
        .execute(&BrowserCommand::Click {
            selector: "#show".into(),
        })
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#replace-frame".into(),
        })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(driver.execute(&fill("Must not reach replacement")).is_err());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches one browser against nested fictional shadow roots"]
fn nested_shadow_field_binds_every_visible_host() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_shadow_nested.html")).unwrap();
    let selector = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Nested name")
        .unwrap()
        .selector
        .clone();
    assert_eq!(selector.matches("waypoint-shadow:").count(), 2);
    driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text("Fictional".into()),
        })
        .unwrap();
    driver.verify_text_value(&selector, "Fictional").unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#hide".into(),
        })
        .unwrap();
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector,
            value: PacketValue::Text("Must not be entered".into())
        })
        .is_err());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches one browser against fictional shadow-root controls"]
fn open_shadow_fields_can_be_filled_and_closed_roots_need_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_shadow.html")).unwrap();
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::SHADOW_FORM_REVIEW));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::CLOSED_SHADOW_FORM_REVIEW));
    assert!(!page
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Private question"));
    let selector = |label: &str| {
        page.schema
            .fields
            .iter()
            .find(|field| field.label == label)
            .unwrap_or_else(|| panic!("missing {label}: {:?}", page.schema.fields))
            .selector
            .clone()
    };
    let name = selector("Name");
    assert!(name.starts_with("waypoint-shadow:"));
    driver
        .execute(&BrowserCommand::Fill {
            selector: name.clone(),
            value: PacketValue::Text("Fictional".into()),
        })
        .unwrap();
    driver.verify_text_value(&name, "Fictional").unwrap();
    for choice in [
        BrowserCommand::SelectOption {
            selector: selector("Place"),
            label: "Remote".into(),
        },
        BrowserCommand::Fill {
            selector: selector("Updates"),
            value: PacketValue::Bool(true),
        },
        BrowserCommand::SelectRadio {
            selector: selector("Schedule"),
            label: "Part time".into(),
        },
        BrowserCommand::SelectOption {
            selector: selector("Timezone"),
            label: "CET".into(),
        },
    ] {
        driver.execute(&choice).unwrap();
        driver.verify_choice(&choice).unwrap();
    }
    let attachment = selector("Attachment");
    let file = upload_dir().join("fictional-shadow.pdf");
    std::fs::write(&file, b"fictional fixture").unwrap();
    driver.attach_local_file(&attachment, &file).unwrap();
    driver.verify_local_file(&attachment, &file).unwrap();
    std::fs::remove_file(&file).unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#replace-host".into(),
        })
        .unwrap();
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: name,
            value: PacketValue::Text("Must not be entered".into())
        })
        .is_err());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser against a fictional nested embedded form"]
fn nested_frame_fields_are_bound_to_every_visible_ancestor() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_iframe_nested.html")).unwrap();
    assert!(!page
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Isolated secret"));
    let field = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Nested name")
        .expect("nested field");
    let selector = field.selector.clone();
    assert_eq!(selector.matches("waypoint-frame:").count(), 2);
    let fill = |value: &str| BrowserCommand::Fill {
        selector: selector.clone(),
        value: PacketValue::Text(value.into()),
    };
    driver.execute(&fill("Fictional")).unwrap();
    driver.verify_text_value(&selector, "Fictional").unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#hide".into(),
        })
        .unwrap();
    assert!(driver.execute(&fill("Must not be entered")).is_err());
    driver
        .execute(&BrowserCommand::Click {
            selector: "#show".into(),
        })
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#replace-inner".into(),
        })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(driver
        .execute(&fill("Must not reach inner replacement"))
        .is_err());
    let page = driver.open(&file_url("form_iframe_nested.html")).unwrap();
    let fresh = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Nested name")
        .unwrap()
        .selector
        .clone();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#replace".into(),
        })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: fresh,
            value: PacketValue::Text("Must not reach outer replacement".into())
        })
        .is_err());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser against a fictional staged form"]
fn staged_form_is_inspected_until_the_schema_settles() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&file_url("form_staged_loading.html")).unwrap();
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "First name"));
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Last name"));
    assert!(live.walls.iter().any(|wall| wall == "captcha present"));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser against a fictional partially rendered form"]
fn initially_nonempty_form_is_rechecked_for_later_fields() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_initial_partial_loading.html"))
        .unwrap();
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "First name"));
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Last name"));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a local browser against a fictional form that changes after an initially stable snapshot"]
fn initially_stable_form_is_checked_for_late_fields() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_late_after_stable.html"))
        .unwrap();
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "First name"));
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Last name"));
    assert!(live
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::DYNAMIC_UNSETTLED_REVIEW));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a local browser against a fictional changing form"]
fn disappearing_loading_field_requires_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_disappearing_loading.html"))
        .unwrap();
    assert!(live.schema.fields.is_empty());
    assert!(live
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::DYNAMIC_FIELD_LOSS_REVIEW));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser against a fictional delayed form"]
fn delayed_form_is_inspected_without_clearing_review_walls() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver.open(&file_url("form_delayed_loading.html")).unwrap();
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "First name"));
    assert!(live.walls.iter().any(|wall| wall == "captcha present"));
    assert!(driver.trace.iter().all(|event| !event.contains("typed")
        && !event.contains("submitted")
        && !event.contains("click")));
}

#[test]
#[ignore = "launches a real browser against a fictional scrollable form"]
fn visual_capture_requires_the_target_in_the_viewport() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver.open(&file_url("form_visual_viewport.html")).unwrap();
    assert!(driver
        .screenshot_field_context("#clipped")
        .unwrap_err()
        .to_string()
        .contains("outside the visible viewport"));
    let selector = "#candidate-name";
    let error = driver.screenshot_field_context(selector).unwrap_err();
    assert!(error.to_string().contains("outside the visible viewport"));
    assert!(driver.screenshot_control(selector).is_err());
    driver.scroll_field_into_view(selector).unwrap();
    let context = driver.screenshot_field_context(selector).unwrap();
    let target = driver.screenshot_control(selector).unwrap();
    assert!(!target.is_empty());
    assert_ne!(context, target);
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted") && !event.contains("click")));
}

#[test]
#[ignore = "launches a real browser against a fictional scrollable form"]
fn scroll_to_embedded_field_reveals_it_without_changing_the_answer() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_iframe_scroll.html")).unwrap();
    let selector = page
        .schema
        .fields
        .iter()
        .find(|field| field.label == "Far name")
        .unwrap()
        .selector
        .clone();
    let frame_body = format!(
        "{}body",
        selector.rsplit_once(':').unwrap().0.to_owned() + ":"
    );
    let before = driver.screenshot_base64().unwrap();
    driver.scroll_field_into_view(&selector).unwrap();
    let after = driver.screenshot_base64().unwrap();
    assert_ne!(before, after);
    let scrolled = driver
        .read_attribute(&frame_body, "data-scrolled")
        .unwrap()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(scrolled > 0.0);
    assert!(driver.read_attribute(&selector, "value").unwrap().is_none());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser against fictional embedded controls"]
fn embedded_controls_after_hidden_frames_can_be_filled_and_verified() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_iframe_controls.html")).unwrap();
    let selector = |label: &str| {
        page.schema
            .fields
            .iter()
            .find(|field| field.label == label)
            .unwrap_or_else(|| {
                panic!(
                    "missing embedded field {label}; fields: {:?}",
                    page.schema
                        .fields
                        .iter()
                        .map(|field| &field.label)
                        .collect::<Vec<_>>()
                )
            })
            .selector
            .clone()
    };
    let name = selector("Name");
    let date = selector("Start date");
    let place = selector("Location");
    let updates = selector("Updates");
    let schedule = selector("Schedule");
    let attachment = selector("Attachment");
    driver
        .execute(&BrowserCommand::Fill {
            selector: name.clone(),
            value: PacketValue::Text("Fictional".into()),
        })
        .unwrap();
    driver.verify_text_value(&name, "Fictional").unwrap();
    driver
        .execute(&BrowserCommand::Fill {
            selector: date.clone(),
            value: PacketValue::Date("2026-11-01".into()),
        })
        .unwrap();
    driver.verify_text_value(&date, "2026-11-01").unwrap();
    for choice in [
        BrowserCommand::SelectOption {
            selector: place,
            label: "Remote".into(),
        },
        BrowserCommand::Fill {
            selector: updates,
            value: PacketValue::Bool(true),
        },
        BrowserCommand::SelectRadio {
            selector: schedule,
            label: "Part time".into(),
        },
    ] {
        driver.execute(&choice).unwrap();
        driver.verify_choice(&choice).unwrap();
    }
    let file = upload_dir().join("fictional-embedded.pdf");
    std::fs::write(&file, b"fictional local fixture").unwrap();
    driver.attach_local_file(&attachment, &file).unwrap();
    driver.verify_local_file(&attachment, &file).unwrap();
    std::fs::remove_file(&file).unwrap();
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::UNINSPECTED_FRAME_REVIEW));
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches one browser against fictional native datalists"]
fn native_datalist_choices_are_reviewable_and_checked_after_events() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_datalist.html")).unwrap();
    let field = page
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#region")
        .unwrap();
    assert_eq!(
        field.field_type,
        FormFieldType::Select {
            options: vec!["Germany".into(), "Canada".into()]
        }
    );
    let text_option = page
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#text-option")
        .unwrap();
    assert_eq!(
        text_option.field_type,
        FormFieldType::Select {
            options: vec!["France".into(), "New Zealand".into()]
        }
    );
    let text_command = BrowserCommand::SelectOption {
        selector: "#text-option".into(),
        label: "New Zealand".into(),
    };
    driver.execute(&text_command).unwrap();
    driver.verify_choice(&text_command).unwrap();
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::NATIVE_DATALIST_REVIEW));
    for selector in [
        "#duplicate",
        "#custom",
        "#duplicate-id",
        "#disabled",
        "#aria-disabled",
        "#text-ambiguous",
    ] {
        assert!(
            !page
                .schema
                .fields
                .iter()
                .any(|field| field.selector == selector),
            "{selector}"
        );
    }
    for (selector, value) in [
        ("#duplicate", "One"),
        ("#custom", "Germany"),
        ("#duplicate-id", "One"),
        ("#disabled", "One"),
        ("#aria-disabled", "One"),
        ("#text-ambiguous", "New Zealand"),
    ] {
        assert!(
            driver
                .execute(&BrowserCommand::SelectOption {
                    selector: selector.into(),
                    label: value.into()
                })
                .is_err(),
            "{selector} must not be selected"
        );
    }
    assert!(driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#text-changing".into(),
            label: "France".into(),
        })
        .is_err());
    let command = BrowserCommand::SelectOption {
        selector: "#region".into(),
        label: "Germany".into(),
    };
    driver.execute(&command).unwrap();
    driver.verify_choice(&command).unwrap();
    assert_eq!(
        driver
            .read_attribute("#region", "data-seen")
            .unwrap()
            .as_deref(),
        Some("Germany")
    );
    for selector in ["#clear", "#changing-role"] {
        assert!(
            driver
                .execute(&BrowserCommand::SelectOption {
                    selector: selector.into(),
                    label: "Germany".into()
                })
                .is_err(),
            "{selector}"
        );
    }
    assert!(driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#changing-disabled".into(),
            label: "Germany".into(),
        })
        .is_err());
    assert!(driver.verify_choice(&command).is_err());
    driver
        .execute(&BrowserCommand::Click {
            selector: "#change-list".into(),
        })
        .unwrap();
    assert!(driver.verify_choice(&command).is_err());
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
}

#[test]
#[ignore = "launches a real browser with a fictional autocomplete fixture"]
fn autocomplete_choices_are_not_mistaken_for_selected_text_fields() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver
        .open(&file_url("form_autocomplete_choice.html"))
        .unwrap();
    assert!(page.schema.fields.iter().any(|field| field.label == "Name"));
    for label in ["City", "Country", "Region", "Nested choice", "Search terms"] {
        assert!(
            !page.schema.fields.iter().any(|field| field.label == label),
            "{label} should remain manual"
        );
    }
    assert!(page
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Category"
            && matches!(field.field_type, FormFieldType::Select { .. })));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall == waypoint_browser::NATIVE_DATALIST_REVIEW));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall.contains("City") && wall.contains("manual review")));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall.contains("Country") && wall.contains("manual review")));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall.contains("Region") && wall.contains("manual review")));
    assert!(page
        .walls
        .iter()
        .any(|wall| wall.contains("Search terms") && wall.contains("manual review")));
    assert!(page.walls.iter().any(|wall| wall.contains("manual review")));
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: "#city".into(),
            value: PacketValue::Text("Berlin".into())
        })
        .is_err());
    assert_eq!(driver.read_attribute("body", "data-changed").unwrap(), None);
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: "#changing".into(),
            value: PacketValue::Text("Berlin".into())
        })
        .is_err());
    assert_eq!(driver.read_attribute("body", "data-changed").unwrap(), None);
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#name".into(),
            value: PacketValue::Text("Avery".into()),
        })
        .unwrap();
    driver.verify_text_value("#name", "Avery").unwrap();
    assert!(driver
        .trace
        .iter()
        .all(|event| !event.contains("submitted")));
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
        .execute(&BrowserCommand::SelectOption {
            selector: "#ambiguous-value".into(),
            label: "On site".into()
        })
        .is_err());
    assert_eq!(
        driver
            .read_attribute("#ambiguous-value", "data-seen")
            .unwrap(),
        None,
        "an ambiguous submitted value must not dispatch a change"
    );
    driver
        .verify_choice(&BrowserCommand::SelectOption {
            selector: "#ambiguous-value".into(),
            label: "Choose an arrangement".into(),
        })
        .expect("the original choice remains selected");
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
fn native_choices_reject_ambiguous_values_and_detect_event_mutations() {
    use waypoint_applications::form_mapping::BrowserCommand;
    let mut driver = CdpDriver::launch(vec![upload_dir()]).expect("launch browser");
    driver.open(&file_url("form_optgroups.html")).unwrap();
    assert!(driver
        .execute(&BrowserCommand::SelectOptions {
            selector: "#multi-ambiguous".into(),
            labels: vec!["Weekends".into()],
        })
        .is_err());
    assert_eq!(
        driver
            .read_attribute("#multi-ambiguous", "data-changed")
            .unwrap(),
        None
    );
    assert!(driver
        .execute(&BrowserCommand::SelectOption {
            selector: "#mutating-single".into(),
            label: "Desired".into(),
        })
        .is_err());
    assert!(driver
        .execute(&BrowserCommand::SelectOptions {
            selector: "#mutating-multiple".into(),
            labels: vec!["Desired".into()],
        })
        .is_err());
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
    let page_links = driver.page_links().expect("read page links");
    assert!(page_links
        .iter()
        .any(|link| link.url == "filters.html" && link.label == "Apply filters"));
    assert!(page_links
        .iter()
        .any(|link| link.url == "referenced.html" && link.label == "Apply now"));
    assert!(page_links
        .iter()
        .any(|link| link.url == "ambiguous.html" && link.label == "Label unavailable"));
    assert!(page_links
        .iter()
        .any(|link| link.url == "missing.html" && link.label == "Label unavailable"));
    let links = driver.application_links().expect("read links");
    assert_eq!(
        links,
        vec![
            "application-form.html",
            "application-other.html",
            "fallback.html",
            "referenced.html",
            "punctuation.html",
            "german.html",
            "french.html",
            "spanish.html",
            "portuguese.html"
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
#[ignore = "launches one local browser with fictional numeric fields"]
fn numeric_constraints_are_checked_before_changing_the_field() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_number_constraints.html"))
        .unwrap();
    assert!(live.schema.fields.iter().all(|field| matches!(
        field.field_type,
        waypoint_applications::form_mapping::FormFieldType::Number { .. }
    )));
    let years = live
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#years")
        .unwrap();
    assert!(
        matches!(&years.field_type, waypoint_applications::form_mapping::FormFieldType::Number { min: Some(min), max: Some(max), step: Some(step) } if min == "1" && max == "9" && step == "2")
    );
    let original_schema = live.schema.schema_hash_input();
    let fill = |selector: &str, value: &str| BrowserCommand::Fill {
        selector: selector.into(),
        value: PacketValue::Text(value.into()),
    };
    for value in ["4", "11", "abc", "1."] {
        assert!(driver.execute(&fill("#years", value)).is_err());
        assert_eq!(driver.text_value_snapshot("#years").unwrap().value, "3");
    }
    assert!(driver.execute(&fill("#changing", "5")).is_err());
    assert_eq!(driver.text_value_snapshot("#changing").unwrap().value, "3");
    driver.execute(&fill("#years", "5")).unwrap();
    driver.verify_text_value("#years", "5").unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#change-max".into(),
        })
        .unwrap();
    let changed = driver.inspect_current().unwrap();
    assert_ne!(changed.schema.schema_hash_input(), original_schema);
    let changed_years = changed
        .schema
        .fields
        .iter()
        .find(|field| field.selector == "#years")
        .unwrap();
    assert!(
        matches!(&changed_years.field_type, waypoint_applications::form_mapping::FormFieldType::Number { max: Some(max), .. } if max == "10")
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

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn required_custom_radio_group_is_reported_for_manual_review() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let live = driver
        .open(&file_url("form_custom_radio_required.html"))
        .unwrap();
    assert!(live
        .walls
        .iter()
        .any(|wall| wall == "a required custom control needs manual review: Work arrangement"));
    assert!(!live
        .walls
        .iter()
        .any(|wall| wall.contains("Hidden group") || wall.contains("Native schedule")));
    assert!(live
        .schema
        .fields
        .iter()
        .any(|field| field.label == "Native schedule" && field.required));
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn custom_radio_inventory_reports_page_state_without_changing_it() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_radio_required.html"))
        .unwrap();
    let inventory = driver.page_custom_radio_groups().unwrap();
    assert_eq!(inventory.groups.len(), 1);
    let group = &inventory.groups[0];
    assert_eq!(group.label, "Work arrangement");
    assert_eq!(group.options.len(), 2);
    assert_eq!(group.options[0].label, "On site");
    assert_eq!(group.options[0].checked, Some(false));
    assert_eq!(group.options[1].label, "Remote");
    assert_eq!(group.options[1].checked, Some(true));
    assert!(group.options[1].disabled);
    assert!(!group.incomplete);
    assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
        && !entry.contains("clicked")
        && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn custom_radio_shared_question_labels_keep_only_option_text() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_radio_shared_label.html"))
        .unwrap();
    let inventory = driver.page_custom_radio_groups().unwrap();
    let group = &inventory.groups[0];
    assert_eq!(group.label, "Are you available for travel?");
    assert_eq!(
        group
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>(),
        ["Yes", "No"]
    );
    assert!(!group.incomplete);
    assert!(!driver
        .screenshot_custom_radio_question(&group.selector)
        .unwrap()
        .is_empty());
    let duplicate = &inventory.groups[1];
    assert!(duplicate.incomplete);
    assert!(driver
        .select_custom_radio_option(&duplicate.selector, "Yes")
        .is_err());
    driver
        .select_custom_radio_option(&group.selector, "Yes")
        .unwrap();
    driver
        .verify_custom_radio_value(&group.selector, "Yes")
        .unwrap();
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn custom_radio_question_capture_rejects_hidden_label_parts() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_radio_hidden_label.html"))
        .unwrap();
    let inventory = driver.page_custom_radio_groups().unwrap();
    assert!(driver
        .screenshot_custom_radio_question(&inventory.groups[0].selector)
        .is_err());
}

#[test]
#[ignore = "launches one local browser against a fictional fixture"]
fn custom_radio_inventory_marks_ambiguous_states_incomplete() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_radio_ambiguous.html"))
        .unwrap();
    let inventory = driver.page_custom_radio_groups().unwrap();
    let group = &inventory.groups[0];
    assert_eq!(group.label, "Unlabelled radio group");
    assert!(group.incomplete);
    assert_eq!(group.options.len(), 3);
    assert_eq!(group.options[2].checked, None);
    assert!(driver
        .select_custom_radio_option(&group.selector, "Same")
        .is_err());
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
}

#[test]
#[ignore = "launches one local browser against a fictional fixture without submitting"]
fn custom_radio_action_checks_state_and_rejects_submit_targets() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    driver
        .open(&file_url("form_custom_radio_action.html"))
        .unwrap();
    let inventory = driver.page_custom_radio_groups().unwrap();
    let safe = inventory
        .groups
        .iter()
        .find(|group| group.label == "Working arrangement")
        .unwrap();
    let danger = inventory
        .groups
        .iter()
        .find(|group| group.label == "Danger")
        .unwrap();
    let changing = inventory
        .groups
        .iter()
        .find(|group| group.label == "Changes on hover")
        .unwrap();
    assert!(driver
        .select_custom_radio_option(&changing.selector, "Changing")
        .is_err());
    assert_eq!(
        driver.read_attribute("#changing", "data-clicked").unwrap(),
        None
    );
    assert!(driver
        .select_custom_radio_option(&danger.selector, "Send application")
        .is_err());
    assert!(driver
        .screenshot_custom_radio_option(&safe.selector, "Remote")
        .is_ok());
    driver
        .select_custom_radio_option(&safe.selector, "Remote")
        .unwrap();
    driver
        .verify_custom_radio_value(&safe.selector, "Remote")
        .unwrap();
    assert!(driver
        .select_custom_radio_option(&safe.selector, "On site")
        .is_err());
    assert_eq!(
        driver
            .read_attribute("#onsite", "aria-checked")
            .unwrap()
            .as_deref(),
        Some("false")
    );
    assert!(driver
        .trace
        .iter()
        .all(|entry| !entry.contains("submitted")));
}
