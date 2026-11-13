use super::*;
use waypoint_applications::driver::BrowserDriver;

#[test]
#[ignore = "explicit contained engine; read-only structural identity across native scope replacement"]
fn logical_controls_survive_native_binding_changes_without_merging_siblings() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let entry = "https://fixture.invalid/logical-controls";
    let child = "https://fixture.invalid/shared-form";
    let external = "https://embedded.invalid/shared-form";
    let input = "<label>Given name<input id=given required></label>";
    let page = |body: &str| {
        format!("<!doctype html><html><head><meta charset=utf-8><link rel=icon href=data:,><style>body{{font:18px Arial;margin:12px}}iframe{{display:block;width:900px;height:180px;margin:8px 0}}</style></head><body>{body}</body></html>")
    };
    let response = |body: &str| {
        let wire=format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",body.len());
        waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap()
    };
    let mut observations = Vec::new();
    for name in [
        "root_id_change",
        "inline",
        "same_url_siblings",
        "srcdoc",
        "shadow",
        "independent_same_url_siblings",
    ] {
        let embedded = matches!(
            name,
            "inline" | "same_url_siblings" | "independent_same_url_siblings"
        );
        let cross = name == "independent_same_url_siblings";
        let url = if cross { external } else { child };
        let body=match name {
            "inline"=>format!("<iframe title='Given name form' src='{url}'></iframe>"),
            "same_url_siblings"|"independent_same_url_siblings"=>format!("<iframe title='First form' src='{url}'></iframe><iframe title='Second form' src='{url}'></iframe>"),
            "srcdoc"=>format!("<iframe title='Inherited form' srcdoc=\"{input}\"></iframe>"),
            "shadow"=>format!("<main id=host></main><script>document.getElementById('host').attachShadow({{mode:'open'}}).innerHTML='{input}'</script>"),
            _=>input.to_owned(),
        };
        let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(
            &folder.join(format!("logical-{name}")),
            entry,
        )
        .unwrap();
        builder.add(entry, response(&page(&body))).unwrap();
        if embedded {
            builder.add(url, response(&page(input))).unwrap();
        }
        let snapshot = builder.finish().unwrap();
        let mut config = template.clone();
        config.schema = 2;
        config.content_root = snapshot.root;
        config.content_files = snapshot.files;
        config.entry = snapshot.entry;
        config.replay_manifest = Some(snapshot.manifest);
        let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
        assert!(ready.appcontainer && ready.capabilities == 0);
        let mut rounds = Vec::<Value>::new();
        let result = (|| -> Result<(), DriverError> {
            for round in 0..2 {
                let form = if name == "root_id_change" && round == 1 {
                    driver.cdp.call("Runtime.evaluate",json!({"expression":"document.querySelector('input').id='runtime-after'"})).map_err(derr)?;
                    driver.inspect_current()?
                } else {
                    driver.open(entry)?
                };
                let mut controls = Vec::new();
                let mut native_selectors = Vec::new();
                if cross {
                    let frames = driver.captured_frames()?;
                    if frames.frames.len() != 2 || frames.uninspected != 0 || frames.truncated {
                        return Err(DriverError(
                            "Expected two independent native frame choices".into(),
                        ));
                    }
                    for frame in &frames.frames {
                        let form = driver.enter_captured_frame(frame)?;
                        if form.schema.fields.len() != 1 {
                            return Err(DriverError(
                                "Independent logical fixture has wrong field denominator".into(),
                            ));
                        }
                        let selector = &form.schema.fields[0].selector;
                        controls.push(
                            serde_json::to_value(
                                driver.recorded_control_identity(&form, selector)?,
                            )
                            .map_err(|_| DriverError("Identity encoding failed".into()))?,
                        );
                        native_selectors.push(selector.clone());
                        driver.leave_captured_frame()?;
                    }
                } else {
                    let count = if name == "same_url_siblings" { 2 } else { 1 };
                    if form.schema.fields.len() != count {
                        return Err(DriverError(
                            "Logical fixture native field denominator differs".into(),
                        ));
                    }
                    for field in &form.schema.fields {
                        controls.push(
                            serde_json::to_value(
                                driver.recorded_control_identity(&form, &field.selector)?,
                            )
                            .map_err(|_| DriverError("Identity encoding failed".into()))?,
                        );
                        native_selectors.push(field.selector.clone());
                    }
                }
                if controls.iter().any(|control| {
                    !control["logical"].is_object() || control.get("logical_error").is_some()
                }) {
                    rounds.push(json!({"round":round,"controls":controls,"native_selectors":native_selectors}));
                    return Err(DriverError(
                        "Read-only structural identity unavailable; observations retained".into(),
                    ));
                }
                if controls.len() == 2
                    && (controls[0]["logical"] == controls[1]["logical"]
                        || controls[0]["logical"]["selector"] != controls[1]["logical"]["selector"]
                        || controls[0]["logical"]["scopes"][0]["document_url"]
                            != controls[1]["logical"]["scopes"][0]["document_url"])
                {
                    return Err(DriverError(
                        "Same-document sibling ownership collapsed or its fixture premise differs"
                            .into(),
                    ));
                }
                if round == 1 {
                    let first = &rounds[0];
                    for (i, control) in controls.iter().enumerate() {
                        if control["logical"] != first["controls"][i]["logical"] {
                            return Err(DriverError(
                                "Structural identity changed with only native bindings".into(),
                            ));
                        }
                    }
                    if name == "root_id_change"
                        && json!(native_selectors) == first["native_selectors"]
                    {
                        return Err(DriverError(
                            "Volatile-ID fixture did not change its native selector".into(),
                        ));
                    }
                }
                rounds.push(
                    json!({"round":round,"controls":controls,"native_selectors":native_selectors}),
                );
            }
            if driver
                .trace
                .iter()
                .any(|event| event.starts_with("clicked") || event.starts_with("typed"))
            {
                return Err(DriverError(
                    "Read-only identity fixture interacted with controls".into(),
                ));
            }
            let replay = driver.rehearsal_replay_report()?;
            if replay["blocked"] != 0 || replay["cached_non_read_replies"] != 0 {
                return Err(DriverError(
                    "Read-only identity fixture has unqualified replay activity".into(),
                ));
            }
            Ok(())
        })();
        let closed = driver.execute(&waypoint_applications::form_mapping::BrowserCommand::Close);
        let observation = json!({"case":name,"result":result.as_ref().map_err(|error|error.to_string()),"rounds":rounds,"closed":closed.is_ok(),"close_error":closed.as_ref().err().map(|error|error.to_string()),"fields_entered":0,"scope":"Read-only fictional identity qualification; not model accuracy, native entry, workflow or authorization"});
        crate::rehearsal_capture::write_new(
            &folder,
            &format!("logical-{name}.private.json"),
            &serde_json::to_vec_pretty(&observation).unwrap(),
        )
        .unwrap();
        observations.push(observation);
        if result.is_err() || closed.is_err() {
            break;
        }
    }
    crate::rehearsal_capture::write_new(
        &folder,
        "logical-controls.private.json",
        &serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    assert_eq!(observations.len(), 6);
    assert!(observations
        .iter()
        .all(|case| case["result"].get("Ok").is_some() && case["closed"] == true));
}
