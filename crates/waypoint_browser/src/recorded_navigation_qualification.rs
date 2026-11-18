//! Native transport and control-identity evidence using fictional saved pages.
use super::*;

struct Reviewer {
    label: &'static str,
    intent: RecordedSectionIntent,
    inventory: Vec<(String, bool)>,
    visual_checks: usize,
}

impl RecordedSectionReviewer for Reviewer {
    fn choose(
        &mut self,
        _: &LiveForm,
        options: &[RecordedSectionOption],
    ) -> Result<Option<RecordedSectionChoice>, DriverError> {
        self.inventory = options
            .iter()
            .map(|option| (option.label.clone(), option.eligible))
            .collect();
        let control = options
            .iter()
            .find(|option| option.label == self.label)
            .ok_or_else(|| DriverError("Fixture control missing".into()))?
            .id;
        // Intentionally return the requested ID even when native admission is false.
        Ok(Some(RecordedSectionChoice {
            control,
            intent: self.intent,
        }))
    }

    fn verify(&mut self, target: &RecordedTargetCapture) -> Result<(), DriverError> {
        assert_eq!(target.label(), self.label);
        assert_eq!(target.kind(), "button");
        self.visual_checks += 1;
        Ok(())
    }
}

#[test]
#[ignore = "explicit pipe-only contained engine; document-replacing saved steps"]
fn planned_navigation_crosses_verified_archived_documents() {
    let (mut config, folder, python) =
        crate::capture_session::qualification::qualification_config();
    let first_url = "https://documents.invalid/apply";
    let second_url = "https://documents.invalid/questions";
    let final_url = "https://documents.invalid/preview";
    let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(
        &folder.join("document-journey"),
        first_url,
    )
    .unwrap();
    for (url, html) in [
        (
            first_url,
            r#"<h1>Applicant details</h1><label for=answer>Given name</label><input id=answer required><button id=next type=button>See the remaining questions</button><script>document.querySelector('#next').onclick=()=>{sessionStorage.setItem('given',document.querySelector('#answer').value);location.assign('/questions');};</script>"#,
        ),
        (
            second_url,
            r#"<h1>Additional details</h1><label for=answer>Family name</label><input id=answer required><button id=preview type=button>Read the application preview</button><button id=send type=submit onclick="sessionStorage.setItem('send-touched','yes')">Submit application</button><script>document.querySelector('#preview').onclick=()=>{sessionStorage.setItem('family',document.querySelector('#answer').value);location.assign('/preview');};</script>"#,
        ),
        (
            final_url,
            r#"<h1>Review your application</h1><dl><dt>Given name</dt><dd id=given></dd><dt>Family name</dt><dd id=family></dd></dl><script>for(const key of ['given','family'])document.getElementById(key).textContent=sessionStorage.getItem(key);document.body.dataset.sendTouched=sessionStorage.getItem('send-touched')||'no';</script>"#,
        ),
    ] {
        let html = format!("<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{{font:18px Arial;margin:24px}}label,button{{display:block;margin-top:12px}}input,button{{font:inherit;padding:5px}}</style>{html}");
        let wire = format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}",html.len());
        let response =
            waypoint_net::read_asset_response(&mut std::io::Cursor::new(wire), "GET", 1024 * 1024)
                .unwrap();
        builder.add(url, response).unwrap();
    }
    let snapshot = builder.finish().unwrap();
    config.schema = 2;
    config.content_root = snapshot.root;
    config.content_files = snapshot.files;
    config.entry = snapshot.entry;
    config.replay_manifest = Some(snapshot.manifest);
    config.seconds = config.seconds.max(120);
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    assert!(ready.appcontainer && ready.capabilities == 0);
    let mut observed = json!({});
    let outcome = (|| -> Result<(), DriverError> {
        let first = driver.open(first_url)?;
        let given = driver.rehearse_recorded_field(&first, "#answer", "Avery")?;
        let mut reviewer = Reviewer {
            label: "See the remaining questions",
            intent: RecordedSectionIntent::Advance,
            inventory: Vec::new(),
            visual_checks: 0,
        };
        let action =
            driver.advance_recorded_section_planned(&first, &[], &[&given], true, &mut reviewer)?;
        observed["reached_second_url"] = json!(driver.current_url()? == second_url);
        let second = match action {
            RecordedSectionOutcome::Advanced {
                form,
                document_transition: Some(proof),
                ..
            } => {
                observed["advance_document"] = json!(proof);
                form
            }
            RecordedSectionOutcome::Advanced {
                document_transition: None,
                ..
            } => {
                return Err(DriverError(
                    "The advance did not bind the new document".into(),
                ));
            }
            RecordedSectionOutcome::Stopped(reason)
            | RecordedSectionOutcome::ReviewRequired { reason, .. } => {
                observed["advance_stop"] = json!(reason);
                observed["native_document_responses"] = json!(driver
                    .cdp
                    .events
                    .iter()
                    .filter(|(kind, _)| kind == "Network.responseReceived")
                    .map(|(_, event)| &event["params"])
                    .collect::<Vec<_>>());
                return Err(DriverError(
                    "The saved next document did not advance the workflow".into(),
                ));
            }
        };
        assert_eq!(second.schema.fields[0].label, "Family name");
        assert!(driver.verify_recorded_field(&given).is_err());
        let family = driver.rehearse_recorded_field(&second, "#answer", "Example")?;
        reviewer.label = "Read the application preview";
        reviewer.intent = RecordedSectionIntent::Review;
        let result = driver.advance_recorded_section_planned(
            &second,
            &[first],
            &[&family],
            true,
            &mut reviewer,
        )?;
        let RecordedSectionOutcome::ReviewRequired {
            reason: RecordedSectionStop::NoEditableFields,
            summary: Some(summary),
            image,
            form,
            document_transition: Some(proof),
        } = result
        else {
            return Err(DriverError(
                "The saved review document was not observed".into(),
            ));
        };
        observed["review_document"] = json!(proof);
        let summary = summary?;
        driver.verify_recorded_summary(&form, &summary)?;
        let answers: Vec<_> = summary
            .answers()
            .map(|(answer, _)| (answer.key.label.as_str(), answer.value.as_str()))
            .collect();
        assert_eq!(
            answers,
            [("Given name", "Avery"), ("Family name", "Example")]
        );
        assert_eq!(
            driver
                .read_attribute("body", "data-send-touched")?
                .as_deref(),
            Some("no")
        );
        assert!(reviewer
            .inventory
            .iter()
            .any(|(label, eligible)| label == "Submit application" && !eligible));
        observed["summary_rows"] = json!(answers.len());
        crate::rehearsal_capture::write_new(
            &folder,
            "document-review.base64.private.txt",
            image.as_bytes(),
        )
        .map_err(DriverError)?;
        Ok(())
    })();
    let replay = driver.rehearsal_replay_report();
    let closed = driver.execute(&BrowserCommand::Close).is_ok();
    let report = json!({"observed":observed,"result":outcome.as_ref().map_err(|error|error.to_string()),"replay":replay.as_ref().ok(),"closed":closed});
    crate::rehearsal_capture::write_new(
        &folder,
        "document-journey.private.json",
        &serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(closed);
    assert_eq!(
        replay.unwrap()["external_delivery"],
        "not_performed_by_replay"
    );
    outcome.unwrap();
}

#[test]
#[ignore = "explicit pipe-only contained engine; nested frame progression and review"]
fn recorded_navigation_reaches_review_inside_a_nested_leased_document() {
    let (mut config, folder, python) =
        crate::capture_session::qualification::qualification_config();
    let portal = "https://portal.invalid/position";
    let shell = "https://provider.invalid/embedded";
    let form_url = "https://provider.invalid/form";
    let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(
        &folder.join("nested-leased-navigation"),
        portal,
    )
    .unwrap();
    for (url, html) in [
        (
            portal,
            r#"<!doctype html><link rel=icon href=data:,><button id=decoy type=button onclick="this.dataset.used='yes'" data-used=no>Read your answers</button><div style="margin:24px"><iframe title="Application provider" width=920 height=585 src="https://provider.invalid/embedded"></iframe></div>"#,
        ),
        (
            shell,
            r#"<!doctype html><link rel=icon href=data:,><button id=decoy type=button onclick="this.dataset.used='yes'" data-used=no>Read your answers</button><div style="margin:28px 37px"><iframe title="Application questions" width=620 height=350 src="/form"></iframe></div>"#,
        ),
        (
            form_url,
            r#"<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{font:18px Arial;margin:18px}label,button{display:block;margin-top:10px}input,button{font:inherit}</style><main id=flow><label for=first>Given name</label><input id=first required><button id=step type=button>Show the next questions</button></main><script>
const flow=document.querySelector('#flow');
document.querySelector('#step').onclick=()=>{
  const first=document.querySelector('#first').value;
  flow.innerHTML='<label for=last>Family name</label><input id=last required><button id=preview type=button>Read your answers</button><button id=send type=submit>Submit application</button>';
  document.querySelector('#send').onclick=()=>{window.submitTouched=true;};
  document.querySelector('#preview').onclick=()=>{
    const last=document.querySelector('#last').value;
    flow.innerHTML='<h1>Review your application</h1><dl><dt>Given name</dt><dd></dd><dt>Family name</dt><dd></dd></dl>';
    const answers=flow.querySelectorAll('dd'); answers[0].textContent=first; answers[1].textContent=last;
  };
};</script>"#,
        ),
    ] {
        let wire = format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}", html.len());
        let response =
            waypoint_net::read_asset_response(&mut std::io::Cursor::new(wire), "GET", 1024 * 1024)
                .unwrap();
        builder.add(url, response).unwrap();
    }
    let snapshot = builder.finish().unwrap();
    config.schema = 2;
    config.content_root = snapshot.root;
    config.content_files = snapshot.files;
    config.entry = snapshot.entry;
    config.replay_manifest = Some(snapshot.manifest);
    config.seconds = config.seconds.max(150);
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    let mut observed = json!({});
    let outcome = (|| -> Result<(), DriverError> {
        assert!(ready.appcontainer && ready.capabilities == 0);
        driver.open(portal)?;
        let frames = driver.captured_frames()?;
        let frame = frames
            .frames
            .iter()
            .find(|frame| frame.origin() == "https://provider.invalid")
            .ok_or_else(|| DriverError("Provider lease was unavailable".into()))?;
        let first = driver.enter_captured_frame(frame)?;
        observed["initial_field_count"] = json!(first.schema.fields.len());
        observed["initial_walls"] = json!(first.walls);
        let tree = driver
            .cdp
            .call("Page.getFrameTree", json!({}))
            .map_err(derr)?;
        let root = &tree["frameTree"];
        observed["separate_root_reports_parent"] = json!(root["frame"]["parentId"].is_string());
        let mut timings = Vec::new();
        let mut events = Vec::new();
        if let Some(inner) = root["childFrames"]
            .as_array()
            .and_then(|frames| frames.first())
        {
            for frame in [&root["frame"], &inner["frame"]] {
                events.push(driver.cdp.events.iter().rev().find_map(|(kind, event)| {
                    (kind == "Network.responseReceived"
                        && event["params"]["frameId"] == frame["id"]
                        && event["params"]["loaderId"] == frame["loaderId"])
                        .then(|| event["params"]["status"].as_u64())
                        .flatten()
                }));
                let world = driver
                    .cdp
                    .call(
                        "Page.createIsolatedWorld",
                        json!({"frameId":frame["id"],"worldName":"Smart Jobseeker fixture timing"}),
                    )
                    .map_err(derr)?;
                let native = driver.cdp.call("Runtime.evaluate", json!({"contextId":world["executionContextId"],"expression":"performance.getEntriesByType('navigation').map(entry=>entry.responseStatus)","returnByValue":true})).map_err(derr)?;
                timings.push(native["result"]["value"].clone());
            }
        }
        observed["navigation_response_statuses"] = json!(timings);
        observed["network_response_events"] = json!(events);
        if !first.walls.is_empty() {
            let root = driver.document_with_frames(true)?;
            observed["census_error"] = json!(driver
                .frame_inspection_identity(&root)
                .err()
                .map(|error| error.to_string()));
        }
        if first.schema.fields.len() != 1 || first.schema.fields[0].label != "Given name" {
            return Err(DriverError(
                "The nested form was not independently inspected".into(),
            ));
        }
        let given = driver.rehearse_recorded_field_reviewed(
            &first,
            &first.schema.fields[0].selector,
            "Avery",
            &[],
            |target| {
                observed["given_native_detail"] = json!(target.detail_focus().is_some());
                target
                    .detail_focus()
                    .is_some()
                    .then_some(())
                    .ok_or_else(|| {
                        DriverError("Given name detail lost its nested frame binding".into())
                    })
            },
        )?;
        observed["first_entered"] = json!(true);
        let mut next = Reviewer {
            label: "Show the next questions",
            intent: RecordedSectionIntent::Advance,
            inventory: Vec::new(),
            visual_checks: 0,
        };
        let second = match driver.advance_recorded_section_planned(
            &first,
            &[],
            &[&given],
            true,
            &mut next,
        )? {
            RecordedSectionOutcome::Advanced { form, .. } => form,
            RecordedSectionOutcome::Stopped(reason)
            | RecordedSectionOutcome::ReviewRequired { reason, .. } => {
                observed["advance_stop"] = json!(reason);
                return Err(DriverError("Nested application did not advance".into()));
            }
        };
        observed["first_inventory"] = json!(next.inventory);
        let name = second
            .schema
            .fields
            .first()
            .filter(|field| field.label == "Family name")
            .ok_or_else(|| DriverError("Wrong second application section".into()))?;
        let family = driver.rehearse_recorded_field_reviewed(
            &second,
            &name.selector,
            "Rivera",
            &[],
            |target| {
                observed["family_native_detail"] = json!(target.detail_focus().is_some());
                target
                    .detail_focus()
                    .is_some()
                    .then_some(())
                    .ok_or_else(|| {
                        DriverError("Family name detail lost its nested frame binding".into())
                    })
            },
        )?;
        let mut preview = Reviewer {
            label: "Read your answers",
            intent: RecordedSectionIntent::Review,
            inventory: Vec::new(),
            visual_checks: 0,
        };
        let review = driver.advance_recorded_section_planned(
            &second,
            &[first],
            &[&family],
            true,
            &mut preview,
        )?;
        observed["preview_inventory"] = json!(preview.inventory);
        assert!(preview
            .inventory
            .iter()
            .any(|(label, admitted)| label == "Submit application" && !admitted));
        let RecordedSectionOutcome::ReviewRequired {
            reason: RecordedSectionStop::NoEditableFields,
            summary: Some(summary),
            image,
            form,
            ..
        } = review
        else {
            return Err(DriverError(
                "Nested read-only review was not observed".into(),
            ));
        };
        let summary = summary?;
        driver.verify_recorded_summary(&form, &summary)?;
        let answers: Vec<_> = summary
            .answers()
            .map(|(answer, _)| (answer.key.label.as_str(), answer.value.as_str()))
            .collect();
        if answers != [("Given name", "Avery"), ("Family name", "Rivera")] {
            return Err(DriverError(
                "Nested summary did not retain both inputs".into(),
            ));
        }
        observed["summary_rows"] = json!(answers.len());
        let regions = driver
            .capture_recorded_summary_row(&summary, 0)?
            .filter(|regions| {
                !regions.is_empty()
                    && regions
                        .iter()
                        .all(|region| region.mode == "embedded_viewport")
            })
            .ok_or_else(|| {
                DriverError("Nested summary needed an independently bound review crop".into())
            })?;
        observed["summary_crop_count"] = json!(regions.len());
        driver.verify_recorded_summary(&form, &summary)?;
        crate::rehearsal_capture::write_new(
            &folder,
            "nested-review.base64.private.txt",
            image.as_bytes(),
        )
        .map_err(DriverError)?;
        if driver
            .read_attribute(&driver.captured_selector("#decoy"), "data-used")?
            .as_deref()
            != Some("no")
        {
            return Err(DriverError(
                "A control in the provider shell received the action".into(),
            ));
        }
        driver.leave_captured_frame()?;
        if driver.read_attribute("#decoy", "data-used")?.as_deref() != Some("no") {
            return Err(DriverError(
                "A control on the host page received the action".into(),
            ));
        }
        let replay = driver.rehearsal_replay_report()?;
        assert_eq!(replay["blocked"], 0);
        assert_eq!(replay["external_delivery"], "not_performed_by_replay");
        observed["replay"] = replay;
        Ok(())
    })();
    let closed = driver.execute(&BrowserCommand::Close).is_ok();
    let receipt = json!({"observed":observed,"result":outcome.as_ref().map_err(|e|e.to_string()),"closed":closed});
    crate::rehearsal_capture::write_new(
        &folder,
        "nested-navigation.private.json",
        &serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    assert!(closed);
    outcome.unwrap();
}

enum Case {
    Review,
    UnexpectedEditableStep,
    ReplaceOnHover,
    BlockScriptWrite,
}

#[test]
#[ignore = "explicit pipe-only contained engine; admission, review, hover replacement and replay write refusal"]
fn planned_navigation_binds_intent_and_native_control_before_acting() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let mut receipts = Vec::new();
    for (name, case) in [
        ("review", Case::Review),
        ("unexpected", Case::UnexpectedEditableStep),
        ("replacement", Case::ReplaceOnHover),
        ("write", Case::BlockScriptWrite),
    ] {
        let url = format!("https://fixture.invalid/navigation-{name}");
        let mut html = String::from(
            r#"<!doctype html><meta charset=utf-8><link rel=icon href=data:,>
<style>body{font:18px Arial;margin:26px}input,button{font:inherit;padding:6px}button{margin:4px;display:block}</style>
<form id=application action=/only-fictional method=post>
<label for=first>First name</label><input id=first name=first required>
<button id=preview type=button>Inspect your responses</button>
<button>Review all entries</button>
<button type=button commandfor=pop command=toggle-popover>Inspect the panel</button>
<button type=button>Send application</button>
<button type=button>Duplicate</button><button type=button>Duplicate</button>
<div popover id=pop>Panel</div></form>
<script>window.reviewEvents=0;window.replacements=0;const preview=document.querySelector('#preview');"#,
        );
        html.push_str(match case {
            Case::Review => "preview.onclick=()=>{window.reviewEvents++; const value=document.querySelector('#first').value; document.body.innerHTML='<h1>Review your application</h1><dl><dt>First name</dt><dd id=summary></dd></dl>'; document.querySelector('#summary').textContent=value;};",
            Case::UnexpectedEditableStep => "preview.onclick=()=>{window.reviewEvents++; document.querySelector('form').innerHTML='<label for=extra>Portfolio URL</label><input id=extra required>';};",
            Case::ReplaceOnHover => "preview.addEventListener('pointerenter',()=>{window.replacements++; const replacement=preview.cloneNode(true); replacement.onclick=()=>window.reviewEvents++; preview.replaceWith(replacement);},{once:true});",
            Case::BlockScriptWrite => "preview.onclick=()=>{window.reviewEvents++; fetch('/only-fictional',{method:'POST',body:'fictional-preview-marker'}).catch(()=>{});};",
        });
        html.push_str("</script>");
        let wire = format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}", html.len());
        let response = waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap();
        let mut snapshot = crate::rehearsal_capture::SnapshotBuilder::start(
            &folder.join(format!("navigation-{name}")),
            &url,
        )
        .unwrap();
        snapshot.add(&url, response).unwrap();
        let snapshot = snapshot.finish().unwrap();
        let mut config = template.clone();
        config.schema = 2;
        config.content_root = snapshot.root;
        config.content_files = snapshot.files;
        config.entry = snapshot.entry;
        config.replay_manifest = Some(snapshot.manifest);
        let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
        let result = (|| -> Result<Value, DriverError> {
            assert!(ready.appcontainer && ready.capabilities == 0);
            let before = driver.open(&url)?;
            let entry = driver.rehearse_recorded_field(&before, "#first", "Avery")?;
            let mut reviewer = Reviewer {
                label: "Inspect your responses",
                intent: RecordedSectionIntent::Review,
                inventory: Vec::new(),
                visual_checks: 0,
            };
            if matches!(case, Case::Review) {
                assert!(matches!(
                    driver.advance_recorded_section(&before, &[], &[&entry], true)?,
                    RecordedSectionOutcome::Stopped(RecordedSectionStop::NoForwardControl)
                ));
                for forbidden in [
                    "Review all entries",
                    "Inspect the panel",
                    "Send application",
                    "Duplicate",
                ] {
                    reviewer.label = forbidden;
                    assert!(matches!(
                        driver.advance_recorded_section_planned(
                            &before,
                            &[],
                            &[&entry],
                            true,
                            &mut reviewer
                        )?,
                        RecordedSectionOutcome::Stopped(RecordedSectionStop::UnsupportedControl)
                    ));
                    assert!(reviewer
                        .inventory
                        .iter()
                        .filter(|(label, _)| label == forbidden)
                        .all(|(_, eligible)| !eligible));
                    assert_eq!(reviewer.visual_checks, 0);
                }
                reviewer.label = "Inspect your responses";
            }
            let before_requests = driver.observed_non_read_requests()?;
            let outcome = driver.advance_recorded_section_planned(
                &before,
                &[],
                &[&entry],
                true,
                &mut reviewer,
            )?;
            let observed = driver.cdp.call("Runtime.evaluate", json!({"expression":"[window.reviewEvents, window.replacements]", "returnByValue":true})).map_err(derr)?;
            let events = observed
                .pointer("/result/value")
                .cloned()
                .unwrap_or_default();
            assert_eq!(reviewer.visual_checks, 1);
            let replay = driver.rehearsal_replay_report()?;
            assert_eq!(replay["external_delivery"], "not_performed_by_replay");
            let status = match (case, outcome) {
                (
                    Case::Review,
                    RecordedSectionOutcome::ReviewRequired {
                        reason: RecordedSectionStop::NoEditableFields,
                        summary: Some(Ok(_)),
                        ..
                    },
                ) => {
                    assert_eq!(events, json!([1, 0]));
                    "review_observed"
                }
                (
                    Case::UnexpectedEditableStep,
                    RecordedSectionOutcome::ReviewRequired {
                        reason: RecordedSectionStop::UnexpectedTransition,
                        ..
                    },
                ) => {
                    assert_eq!(events, json!([1, 0]));
                    "unexpected_step_stopped"
                }
                (
                    Case::ReplaceOnHover,
                    RecordedSectionOutcome::Stopped(RecordedSectionStop::UnsupportedControl),
                ) => {
                    assert_eq!(events, json!([0, 1]));
                    "stale_button_refused"
                }
                (
                    Case::BlockScriptWrite,
                    RecordedSectionOutcome::Stopped(RecordedSectionStop::ReplayGap),
                ) => {
                    assert_eq!(events, json!([1, 0]));
                    assert!(driver.observed_non_read_requests()? > before_requests);
                    assert!(replay["blocked"]
                        .as_u64()
                        .is_some_and(|blocked| blocked > 0));
                    "write_refused_by_replay"
                }
                _ => {
                    return Err(DriverError(format!(
                        "Unexpected navigation outcome in {name}"
                    )))
                }
            };
            Ok(
                json!({"native_status":status,"events":events,"inventory":reviewer.inventory,"replay":replay}),
            )
        })();
        let closed = driver.execute(&BrowserCommand::Close).is_ok();
        receipts.push(
            json!({"case":name,"result":result.as_ref().map_err(|e|e.to_string()),"closed":closed}),
        );
        if result.is_err() || !closed {
            break;
        }
    }
    crate::rehearsal_capture::write_new(
        &folder,
        "navigation.private.json",
        &serde_json::to_vec_pretty(&receipts).unwrap(),
    )
    .unwrap();
    assert_eq!(receipts.len(), 4, "{receipts:?}");
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["closed"] == true && receipt["result"].get("Ok").is_some()),
        "{receipts:?}"
    );
}
