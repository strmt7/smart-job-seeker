//! Native-number admission on inert, contained pages; no local-model claims.
use super::*;

#[test]
#[ignore = "explicit contained engine; numeric min/max/step and focus-mutation refusal"]
fn recorded_number_entry_checks_native_constraints_before_events() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let mut observations = Vec::new();
    for (name, value, focus, allowed) in [
        ("exact", "4", "", true),
        ("below-min", "-1", "", false),
        ("above-max", "11", "", false),
        ("step", "4.5", "", false),
        ("prose", "4 years", "", false),
        ("changed-on-focus", "4", "onfocus=\"this.min='5'\"", false),
    ] {
        let entry = "https://fixture.invalid/number-admission";
        let html = format!("<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{{font:18px Arial;margin:24px}}label{{display:block}}input{{font:inherit;padding:8px}}</style><label for=years>Years of test tooling experience</label><input id=years type=number min=0 max=10 step=1 required {focus}><script>window.entryEvents=0;document.querySelector('#years').addEventListener('input',()=>window.entryEvents++);</script>");
        let wire = format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}", html.len());
        let response = waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap();
        let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(
            &folder.join(format!("number-{name}")),
            entry,
        )
        .unwrap();
        builder.add(entry, response).unwrap();
        let snapshot = builder.finish().unwrap();
        let mut config = template.clone();
        config.schema = 2;
        config.content_root = snapshot.root;
        config.content_files = snapshot.files;
        config.entry = snapshot.entry;
        config.replay_manifest = Some(snapshot.manifest);
        let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
        assert!(ready.appcontainer && ready.capabilities == 0);
        let outcome = (|| -> Result<Value, DriverError> {
            let form = driver.open(entry)?;
            let mut reviews = 0;
            let result =
                driver.rehearse_recorded_field_reviewed(&form, "#years", value, &[], |target| {
                    reviews += 1;
                    if target.kind() != "text" {
                        return Err(DriverError(
                            "Numeric target lacks an editable visual category".into(),
                        ));
                    }
                    Ok(())
                });
            let actual = driver.text_value_snapshot("#years")?.value;
            let events = driver
                .cdp
                .call(
                    "Runtime.evaluate",
                    json!({"expression":"window.entryEvents","returnByValue":true}),
                )
                .map_err(derr)?;
            let events = events["result"]["value"]
                .as_u64()
                .ok_or_else(|| DriverError("Missing native event count".into()))?;
            if allowed {
                let capture = result?;
                if !matches!(capture.kind(), RecordedFieldKind::Number)
                    || capture.value_comparison() != "exact"
                    || actual != value
                    || events == 0
                    || reviews != 1
                {
                    return Err(DriverError(
                        "Valid number lacks exact native entry evidence".into(),
                    ));
                }
                driver.verify_recorded_field(&capture)?;
                driver.cdp.call("Runtime.evaluate",json!({"expression":"document.querySelector('#years').value='5'","returnByValue":true})).map_err(derr)?;
                if driver.verify_recorded_field(&capture).is_ok() {
                    return Err(DriverError(
                        "Changed numeric value kept stale verification".into(),
                    ));
                }
                Ok(
                    json!({"entered":true,"events":events,"reviews":reviews,"value":actual,"stale_refused":true}),
                )
            } else if result.is_err()
                && actual.is_empty()
                && events == 0
                && (focus.is_empty() && reviews == 0 || !focus.is_empty() && reviews == 1)
            {
                Ok(
                    json!({"refused":true,"events":events,"reviews":reviews,"value":actual,"reason":result.err().map(|e|e.to_string())}),
                )
            } else {
                Err(DriverError(format!("Invalid number changed the live input: value={actual:?}, events={events}, reviews={reviews}")))
            }
        })();
        let closed = driver.execute(&BrowserCommand::Close).is_ok();
        observations.push(json!({"case":name,"allowed":allowed,"result":outcome.as_ref().map_err(|e|e.to_string()),"closed":closed}));
        if outcome.is_err() || !closed {
            break;
        }
    }
    crate::rehearsal_capture::write_new(
        &folder,
        "number-admission.private.json",
        &serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    assert_eq!(observations.len(), 6);
    assert!(observations
        .iter()
        .all(|o| o["closed"] == true && o["result"].get("Ok").is_some()));
}
