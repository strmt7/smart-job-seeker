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
                    let _ = stream.read(&mut request);
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
    assert!(result.is_err(), "HTTP error page was accepted: {result:?}");
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
    assert_eq!(field("Answer").max_length_utf16, Some(5));
    assert_eq!(field("Invalid limit").max_length_utf16, None);
    let mut changed = live.schema.clone();
    changed
        .fields
        .iter_mut()
        .find(|f| f.label == "Name")
        .unwrap()
        .max_length_utf16 = Some(4);
    assert_ne!(changed.schema_hash_input(), live.schema.schema_hash_input());
}

#[test]
#[ignore = "launches a real browser with a fictional rich text fixture"]
fn labelled_rich_textbox_accepts_plain_text_and_checks_retention() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver.open(&file_url("form_rich_textbox.html")).unwrap();
    assert_eq!(
        page.schema.fields.len(),
        3,
        "unlabelled editor must not be offered"
    );
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
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: selector.clone(),
            value: PacketValue::Text("line\n".repeat(41))
        })
        .is_err());
    driver
        .verify_text_value(&selector, "First\n\nThird")
        .unwrap();
    for label in ["Unsafe editor", "Disabled editor"] {
        let selector = &page
            .schema
            .fields
            .iter()
            .find(|field| field.label == label)
            .unwrap()
            .selector;
        assert!(driver
            .execute(&BrowserCommand::Fill {
                selector: selector.clone(),
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
#[ignore = "launches a real browser with a fictional autocomplete fixture"]
fn autocomplete_choices_are_not_mistaken_for_selected_text_fields() {
    let mut driver = CdpDriver::launch(vec![]).unwrap();
    let page = driver
        .open(&file_url("form_autocomplete_choice.html"))
        .unwrap();
    assert!(page.schema.fields.iter().any(|field| field.label == "Name"));
    for label in [
        "City",
        "Country",
        "Category",
        "Region",
        "Nested choice",
        "Search terms",
    ] {
        assert!(
            !page.schema.fields.iter().any(|field| field.label == label),
            "{label} should remain manual"
        );
    }
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
