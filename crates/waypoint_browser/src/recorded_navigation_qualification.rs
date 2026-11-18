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
