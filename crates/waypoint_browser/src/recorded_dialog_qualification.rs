use super::*;
use waypoint_applications::driver::BrowserDriver;

#[test]
#[ignore = "explicit contained engine; fictional centered radio questions in root and child documents"]
fn centered_radio_context_preserves_question_and_native_choice() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let entry = "https://fixture.invalid/radio-context";
    let html="<!doctype html><html lang=en><meta charset=utf-8><link rel=icon href=data:,><style>body{font:18px Arial;max-width:650px;margin:24px auto;padding:0 20px}label{display:inline-block;margin-right:20px}input{width:20px;height:20px;vertical-align:middle}</style><div role=radiogroup aria-label='Current country of residence'><p>Current country of residence *</p><label><input id=ca name=residence type=radio value=ca required>Canada</label><label><input id=fr name=residence type=radio value=fr required>France</label></div></html>";
    let response = |text: &str| {
        let wire = format!(
            "HTTP/1.1 200 Fictional\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{text}",
            text.len()
        );
        waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap()
    };
    let mut observations = Vec::new();
    for name in ["root", "same-origin", "same-site", "cross-site"] {
        let embedded = name != "root";
        let child = if name == "cross-site" {
            "https://embedded.invalid/radio-context-child"
        } else if name == "same-site" {
            "https://child.fixture.invalid/radio-context-child"
        } else {
            "https://fixture.invalid/radio-context-child"
        };
        let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(
            &folder.join(format!("radio-{name}")),
            entry,
        )
        .unwrap();
        if embedded {
            builder.add(entry,response(&format!("<!doctype html><link rel=icon href=data:,><iframe title='Fictional contact form' width=1000 height=650 src={child}></iframe>"))).unwrap();
            builder.add(child, response(html)).unwrap();
        } else {
            builder.add(entry, response(html)).unwrap();
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
        let mut phase = "open";
        let mut native_form = Value::Null;
        let result = (|| -> Result<Value, DriverError> {
            let mut form = driver.open(entry)?;
            native_form = json!({"schema":form.schema,"walls":form.walls});
            if matches!(name, "same-site" | "cross-site") {
                phase = "enter_cross_origin";
                let frames = driver.captured_frames()?;
                if name == "same-site" && frames.frames.is_empty() && frames.uninspected > 0 {
                    if !form.independent_field_entry_blocked()
                        || driver
                            .trace
                            .iter()
                            .any(|event| event.starts_with("clicked"))
                    {
                        return Err(DriverError(
                            "An unavailable child target lost its review barrier".into(),
                        ));
                    }
                    return Ok(
                        json!({"document":name,"status":"needs_review","reason":"no_independent_child_target","fields_entered":0,"external_delivery":"not_performed","scope":"Existing target refusal only; no crop or model success credit"}),
                    );
                }
                if frames.frames.len() != 1 {
                    return Err(DriverError(
                        "Cross-origin radio fixture expected one isolated child".into(),
                    ));
                }
                form = driver.enter_captured_frame(&frames.frames[0])?;
            }
            phase = "check_native_inventory";
            native_form = json!({"schema":form.schema,"walls":form.walls});
            if form.schema.fields.len() != 1 || !form.schema.fields[0].required {
                return Err(DriverError("Radio fixture native field count/requiredness differs from the authored source".into()));
            }
            let selector = form.schema.fields[0].selector.clone();
            if name == "same-origin" {
                // This existing scope barrier is not repaired by a pixel crop.
                // Preserve the refusal rather than treating visible fields as
                // proof that every embedded scope was completely inspected.
                if !form.independent_field_entry_blocked()
                    || !form
                        .walls
                        .iter()
                        .any(|wall| wall == crate::UNINSPECTED_FRAME_REVIEW)
                {
                    return Err(DriverError(
                        "Same-origin scope qualification needs reassessment".into(),
                    ));
                }
                let before = driver.observed_non_read_requests()?;
                let mut reviewed = false;
                let attempted = driver.rehearse_recorded_field_reviewed(
                    &form,
                    &selector,
                    "Canada",
                    &[],
                    |_| {
                        reviewed = true;
                        Ok(())
                    },
                );
                if attempted.is_ok()
                    || reviewed
                    || driver.inspect_current()?.schema != form.schema
                    || driver.observed_non_read_requests()? != before
                    || driver
                        .trace
                        .iter()
                        .any(|event| event.starts_with("clicked"))
                {
                    return Err(DriverError(
                        "Uninspected inline scope reached a radio interaction".into(),
                    ));
                }
                return Ok(
                    json!({"document":name,"status":"needs_review","reason":"uninspected_inline_scope","fields_entered":0,"external_delivery":"not_performed","scope":"Existing refusal only; no crop or model success credit"}),
                );
            }
            let mut before = None;
            phase = "rehearse_radio";
            let capture = driver.rehearse_recorded_field_reviewed(
                &form,
                &selector,
                "Canada",
                &[],
                |target| {
                    let file = format!("radio-{name}-before.base64");
                    crate::rehearsal_capture::write_new(&folder, &file, target.image().as_bytes())
                        .map_err(DriverError)?;
                    before = Some(file);
                    Ok(())
                },
            )?;
            phase = "verify_radio";
            driver.verify_recorded_field(&capture)?;
            if capture.value() != "Canada" {
                return Err(review());
            }
            let after = format!("radio-{name}-after.base64");
            crate::rehearsal_capture::write_new(&folder, &after, capture.image().as_bytes())
                .map_err(DriverError)?;
            let replay = driver.rehearsal_replay_report()?;
            if replay["blocked"] != 0 || replay["cached_non_read_replies"] != 0 {
                return Err(review());
            }
            Ok(
                json!({"document":name,"status":"native_choice_verified","before":before,"after":after,"native_value":capture.value(),"replay":replay,"external_delivery":"not_performed","scope":"Native/image capture only; no model-vision credit"}),
            )
        })();
        let closed = driver.execute(&waypoint_applications::form_mapping::BrowserCommand::Close);
        observations.push(json!({"result":result.as_ref().map_err(|e|e.to_string()),"browser_closed":closed.is_ok(),"phase":phase,"native_form":native_form,"document":name}));
        if result.is_err() || closed.is_err() {
            break;
        }
    }
    crate::rehearsal_capture::write_new(
        &folder,
        "radio-context.private.json",
        &serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    assert_eq!(observations.len(), 4);
    assert!(observations
        .iter()
        .all(|o| o["result"].get("Ok").is_some() && o["browser_closed"] == true));
}

#[test]
#[ignore = "explicit isolated engine; fictional responsive resource requests during clipping"]
fn responsive_capture_resource_probe() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let entry = "https://fixture.invalid/responsive-resources";
    let narrow = "https://fixture.invalid/narrow.svg";
    let wide = "https://fixture.invalid/wide.svg";
    let html = format!("<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{{margin:0}}.cover{{height:600px;background-image:url('{narrow}')}}@media(min-width:480px){{.cover{{background-image:url('{wide}')}}}}</style><div class=cover><button id=target type=button>Fictional control</button></div>");
    let response = |mime: &str, text: &str| {
        let wire = format!(
            "HTTP/1.1 200 Fictional\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n{text}",
            text.len()
        );
        waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap()
    };
    let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(
        &folder.join("responsive-resource-archive"),
        entry,
    )
    .unwrap();
    builder.add(entry, response("text/html", &html)).unwrap();
    builder.add(wide,response("image/svg+xml","<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><path fill='blue' d='M0 0h8v8H0z'/></svg>")).unwrap();
    let snapshot = builder.finish().unwrap();
    let mut samples = Vec::new();
    for (beyond, scale) in [(false, 1), (true, 1), (false, 2), (true, 2)] {
        let mut config = template.clone();
        config.schema = 2;
        config.content_root = snapshot.root.clone();
        config.content_files = snapshot.files.clone();
        config.entry = snapshot.entry.clone();
        config.replay_manifest = Some(snapshot.manifest.clone());
        let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
        assert!(ready.appcontainer && ready.capabilities == 0);
        let result = (|| -> Result<Value, DriverError> {
            let form = driver.open(entry)?;
            driver.prepare_capture_styles()?;
            let before = driver.rehearsal_replay_report()?;
            let context = driver.screenshot_field_context("#target")?;
            let detail = driver.screenshot_control("#target")?;
            let production = driver.rehearsal_replay_report()?;
            if before["blocked"] != 0 || production["blocked"] != 0 {
                return Err(DriverError(
                    "Visible field capture requested an inactive responsive resource".into(),
                ));
            }
            for (kind, image) in [("context", context), ("detail", detail)] {
                crate::rehearsal_capture::write_new(
                    &folder,
                    &format!("resource-{beyond}-{scale}-{kind}.base64"),
                    image.as_bytes(),
                )
                .map_err(DriverError)?;
            }
            let screenshot=driver.capture_png(json!({"format":"png","captureBeyondViewport":beyond,"clip":{"x":0,"y":0,"width":280,"height":60,"scale":scale}}))?;
            let after = driver.rehearsal_replay_report()?;
            let name = format!("resource-{beyond}-{scale}.base64");
            crate::rehearsal_capture::write_new(
                &folder,
                &name,
                screenshot["data"].as_str().ok_or_else(review)?.as_bytes(),
            )
            .map_err(DriverError)?;
            if driver.inspect_current()? != form {
                return Err(review());
            }
            Ok(
                json!({"beyond":beyond,"scale":scale,"before":before,"production":production,"after":after,"image":name}),
            )
        })();
        let closed = driver.execute(&waypoint_applications::form_mapping::BrowserCommand::Close);
        samples.push(json!({"result":result.as_ref().map_err(|e|e.to_string()),"browser_closed":closed.is_ok()}));
        // Preserve the complete case evidence before reporting an error.
        if result.is_err() || closed.is_err() {
            break;
        }
    }
    crate::rehearsal_capture::write_new(
        &folder,
        "responsive-resources.private.json",
        &serde_json::to_vec_pretty(&samples).unwrap(),
    )
    .unwrap();
    assert_eq!(samples.len(), 4);
    assert!(samples
        .iter()
        .all(|s| s["result"].get("Ok").is_some() && s["browser_closed"] == true));
    for sample in samples {
        let result = &sample["result"]["Ok"];
        let expected = if result["beyond"] == true { 1 } else { 0 };
        assert_eq!(result["after"]["blocked"], expected);
        assert_eq!(result["after"]["cached_non_read_replies"], 0);
        if expected == 1 {
            assert_eq!(
                result["after"]["refused_request_samples"][0]["url_sha256"],
                waypoint_domain::sha256_hex(narrow.as_bytes())
            );
        }
    }
}

#[test]
#[ignore = "explicit isolated engine; fictional responsive clipping and raster observations"]
fn responsive_capture_viewport_probe() {
    let (mut config, folder, python) =
        crate::capture_session::qualification::qualification_config();
    let entry = "https://fixture.invalid/responsive-capture";
    let html="<!doctype html><html lang=en><meta charset=utf-8><link rel=icon href=data:,><style>body{margin:0}#target{display:block;box-sizing:border-box;border:0;padding:0;margin:0;width:280px;height:60px;background:rgb(0,102,204);color:white}#target::before{content:'Wide viewport'}@media(max-width:479px){#target{background:rgb(204,0,102)}#target::before{content:'Narrow viewport'}}</style><button id=target type=button></button><label>Email<input id=email></label></html>";
    let wire = format!(
        "HTTP/1.1 200 Fictional\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{html}",
        html.len()
    );
    let response = waypoint_net::read_asset_response(
        &mut std::io::Cursor::new(wire.into_bytes()),
        "GET",
        1024 * 1024,
    )
    .unwrap();
    let mut builder =
        crate::rehearsal_capture::SnapshotBuilder::start(&folder.join("responsive-capture"), entry)
            .unwrap();
    builder.add(entry, response).unwrap();
    let snapshot = builder.finish().unwrap();
    config.schema = 2;
    config.content_root = snapshot.root;
    config.content_files = snapshot.files;
    config.entry = snapshot.entry;
    config.replay_manifest = Some(snapshot.manifest);
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    assert!(ready.appcontainer && ready.capabilities == 0);
    let result = (|| -> Result<Vec<Value>, DriverError> {
        let form = driver.open(entry)?;
        let mut samples = Vec::new();
        for (beyond, scale) in [(true, 1), (true, 2), (false, 1), (false, 2)] {
            let before=driver.cdp.call("Runtime.evaluate",json!({"expression":"JSON.stringify({width:innerWidth,dpr:devicePixelRatio,color:getComputedStyle(document.getElementById('target')).backgroundColor})","returnByValue":true})).map_err(derr)?;
            let parameters = json!({"format":"png","captureBeyondViewport":beyond,"clip":{"x":0,"y":0,"width":280,"height":60,"scale":scale}});
            let screenshot = driver.capture_png(parameters.clone())?;
            let name = format!("responsive-{beyond}-{scale}.base64");
            crate::rehearsal_capture::write_new(
                &folder,
                &name,
                screenshot["data"].as_str().ok_or_else(review)?.as_bytes(),
            )
            .map_err(DriverError)?;
            let after=driver.cdp.call("Runtime.evaluate",json!({"expression":"JSON.stringify({width:innerWidth,dpr:devicePixelRatio,color:getComputedStyle(document.getElementById('target')).backgroundColor})","returnByValue":true})).map_err(derr)?;
            samples.push(json!({"parameters":parameters,"before":before["result"]["value"],"after":after["result"]["value"],"image":name}));
        }
        if driver.inspect_current()? != form {
            return Err(review());
        }
        Ok(samples)
    })();
    let closed = driver.execute(&waypoint_applications::form_mapping::BrowserCommand::Close);
    crate::rehearsal_capture::write_new(&folder,"responsive-capture.private.json",&serde_json::to_vec_pretty(&json!({"result":result.as_ref().map_err(|e|e.to_string()),"browser_closed":closed.is_ok(),"fields_entered":0,"external_delivery":"not_performed"})).unwrap()).unwrap();
    closed.unwrap();
    assert!(result.is_ok());
}

#[test]
#[ignore = "explicit isolated saved source and prior local-model proposal; all actions refused"]
fn saved_dialog_passive_replay_phases() {
    let (config, folder, python) = crate::capture_session::qualification::qualification_config();
    let target = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
    )
    .unwrap();
    let input = std::fs::canonicalize(
        std::env::var_os("WAYPOINT_DIALOG_PASSIVE_CASE").expect("explicit private case"),
    )
    .unwrap();
    assert!(input.starts_with(&target));
    let bytes = std::fs::read(input).unwrap();
    assert!(bytes.len() <= 128 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_DIALOG_PASSIVE_CASE_SHA256").unwrap()
    );
    let case: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(case["scope"], "saved_public_development_only");
    assert!(std::fs::canonicalize(&config.content_root)
        .unwrap()
        .starts_with(&target));
    let manifest = std::fs::read(
        config
            .content_root
            .join(config.replay_manifest.as_ref().unwrap()),
    )
    .unwrap();
    assert_eq!(
        waypoint_domain::sha256_hex(&manifest),
        case["manifest_sha256"].as_str().unwrap()
    );
    let entry: Value = serde_json::from_slice(&manifest).unwrap();
    let entry = entry["entry_url"].as_str().unwrap();
    let mut results = Vec::new();
    for (name, warm, idle, review_delay) in [
        ("cold", false, 0, 0),
        ("warm", true, 0, 0),
        ("warm_review_delay", true, 0, 3000),
        ("idle_without_warm", false, 3000, 0),
        ("warm_long_idle", true, 15000, 0),
        ("warm_sweep", true, 0, 0),
    ] {
        if let Some(selected) = case.get("probe_cases") {
            let selected = selected.as_array().expect("explicit passive case list");
            assert!(!selected.is_empty() && selected.len() <= 6);
            if !selected.iter().any(|value| value.as_str() == Some(name)) {
                continue;
            }
        }
        let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config.clone()).unwrap();
        assert!(ready.appcontainer && ready.capabilities == 0);
        let started = Instant::now();
        let mut phases = Vec::new();
        let mut mark = |phase: &str| {
            phases.push(json!({"phase":phase,"elapsed_ms":started.elapsed().as_millis()}))
        };
        let result = (|| -> Result<Value, DriverError> {
            mark("open");
            let form = driver.open(entry)?;
            mark("opened");
            let opened = driver.rehearsal_replay_report()?;
            if warm {
                mark("prepare_inspection");
                driver.prepare_capture_styles()?;
                driver
                    .cdp
                    .pump_checked(Duration::from_millis(250))
                    .map_err(derr)?;
            }
            if idle > 0 {
                driver
                    .cdp
                    .pump_checked(Duration::from_millis(idle))
                    .map_err(derr)?;
            }
            mark("observe_dialog");
            let dialog = driver.recorded_dialog(&form)?.ok_or_else(review)?;
            mark("dialog_observed");
            if serde_json::to_value(dialog.description()).map_err(|_| review())?
                != case["description"]
            {
                return Err(DriverError(
                    "Frozen model proposal no longer describes this dialog".into(),
                ));
            }
            let mut sweep = Vec::new();
            if name == "warm_sweep" {
                mark("sweep");
                sweep.push(json!({"phase":"start","replay":driver.rehearsal_replay_report()?}));
                let index = case["control"]
                    .as_str()
                    .and_then(|s| s.strip_prefix("control-"))
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or_else(review)?;
                let selector = driver.captured_selector(
                    &dialog
                        .scene
                        .controls
                        .get(index)
                        .ok_or_else(review)?
                        .selector,
                );
                let node = driver.node_id(&selector)?.ok_or_else(review)?;
                sweep.push(
                    json!({"phase":"node_resolved","replay":driver.rehearsal_replay_report()?}),
                );
                driver
                    .cdp
                    .call("DOM.getBoxModel", json!({"nodeId":node}))
                    .map_err(derr)?;
                sweep.push(json!({"phase":"box_model","replay":driver.rehearsal_replay_report()?}));
                let applied =
                    driver.with_capture_styles(|driver| driver.rehearsal_replay_report())?;
                sweep.push(json!({"phase":"caret_applied","replay":applied}));
                sweep.push(
                    json!({"phase":"caret_restored","replay":driver.rehearsal_replay_report()?}),
                );
                driver.screenshot_base64()?;
                sweep.push(
                    json!({"phase":"page_screenshot","replay":driver.rehearsal_replay_report()?}),
                );
                driver.screenshot_field_context(&selector)?;
                sweep.push(json!({"phase":"context_screenshot","replay":driver.rehearsal_replay_report()?}));
                driver.screenshot_control(&selector)?;
                sweep.push(
                    json!({"phase":"detail_screenshot","replay":driver.rehearsal_replay_report()?}),
                );
            }
            let before = driver.rehearsal_replay_report()?;
            let writes_before = driver.observed_non_read_requests()?;
            let mut callback = false;
            mark("capture_target");
            let outcome = driver.dismiss_recorded_dialog_reviewed(
                &dialog,
                case["control"].as_str().ok_or_else(review)?,
                |_| {
                    callback = true;
                    mark("review_callback");
                    std::thread::sleep(Duration::from_millis(review_delay));
                    Err(DriverError(
                        "Passive probe refuses every interaction".into(),
                    ))
                },
            );
            mark("capture_returned");
            let reason = match outcome {
                Ok(RecordedDialogOutcome::Stopped { reason, .. }) => reason.to_owned(),
                Err(error) if callback && error.0 == "Passive probe refuses every interaction" => {
                    "review_refused_by_probe".into()
                }
                Err(error) => return Err(error),
                Ok(RecordedDialogOutcome::NativeClosed { .. }) => {
                    return Err(DriverError("Passive probe unexpectedly acted".into()))
                }
            };
            let after = driver.rehearsal_replay_report()?;
            let writes_after = driver.observed_non_read_requests()?;
            let current = driver.recorded_dialog(&form)?;
            mark("context_rechecked");
            let unchanged = current.is_some_and(|current| {
                current.document == dialog.document
                    && current.form == dialog.form
                    && current.scene == dialog.scene
            });
            Ok(
                json!({"case":name,"opened":opened,"before":before,"after":after,"sweep":sweep,"non_read_before":writes_before,"non_read_after":writes_after,"review_callback_reached":callback,"reason":reason,"native_context_unchanged":unchanged,"fields_entered":0,"external_delivery":"not_performed"}),
            )
        })();
        let no_clicks = driver
            .trace
            .iter()
            .all(|event| !event.starts_with("clicked"));
        let closed = driver.execute(&waypoint_applications::form_mapping::BrowserCommand::Close);
        let evidence = json!({"result":result.as_ref().map_err(|error|error.to_string()),"browser_closed":closed.is_ok(),"no_click_trace":no_clicks,"phases":phases});
        crate::rehearsal_capture::write_new(
            &folder,
            &format!("passive-dialog-{name}.private.json"),
            &serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
        closed.unwrap();
        assert!(no_clicks);
        results.push(evidence);
    }
    let expected = case
        .get("probe_cases")
        .map_or(6, |selected| selected.as_array().unwrap().len());
    let complete = !results.is_empty()
        && results.len() == expected
        && results
            .iter()
            .all(|case| case["result"].get("Ok").is_some());
    crate::rehearsal_capture::write_new(&folder,"passive-dialog.private.json",&serde_json::to_vec_pretty(&json!({"complete":complete,"cases":results,"scope":"No-action saved-copy diagnostics, not model accuracy or workflow qualification"})).unwrap()).unwrap();
    assert!(complete, "Private passive case errors retained");
}
