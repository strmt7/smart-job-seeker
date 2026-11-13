use super::*;
use waypoint_applications::driver::BrowserDriver;

#[test]
#[ignore = "explicit contained engine; fictional inline frame coverage, stale snapshots and opaque siblings"]
fn inline_frame_completeness_and_stale_scope_qualification() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let entry = "https://fixture.invalid/inline-coverage";
    let a = "https://fixture.invalid/inline-a";
    let b = "https://fixture.invalid/inline-b";
    let opaque = "https://child.fixture.invalid/opaque";
    let targeted = "https://embedded.invalid/opaque";
    let input = "<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{font:18px Arial;margin:12px}input{font:inherit}</style><label>Applicant given name <input id=given required></label>";
    let iframe = |url: &str, extra: &str| {
        format!("<iframe title='Fictional applicant form' width=900 height=240 src='{url}' {extra}></iframe>")
    };
    let response = |text: &str| {
        let wire = format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{text}", text.len());
        waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap()
    };
    let mut results = Vec::new();
    let cases = [
        "nested",
        "siblings",
        "hidden_sibling",
        "mixed_inline_opaque",
        "mixed_inline_target",
        "quota",
        "deep",
        "covered",
        "failed_response",
        "unknown_response",
        "srcdoc",
        "about_blank",
        "navigation",
        "reparented_owner",
        "replaced_owner",
        "tall",
        "wide",
        "partly_above",
        "nested_tall",
        "tall_scroll_end",
        "tall_covered",
        "tall_clipped",
        "tall_offscreen",
        "tall_transparent",
    ];
    let selected = std::env::var("WAYPOINT_INLINE_CASE").ok();
    if let Some(selected) = &selected {
        assert!(cases.contains(&selected.as_str()));
    }
    for name in cases {
        if selected.as_ref().is_some_and(|selected| selected != name) {
            continue;
        }
        let mut sources: Vec<(String, String)> = vec![];
        let body = match name {
            "tall" | "wide" | "partly_above" | "tall_scroll_end" | "tall_covered"
            | "tall_clipped" | "tall_offscreen" | "tall_transparent" => {
                let field = if name == "partly_above" {
                    format!("<div style='padding-top:1720px'>{input}</div>")
                } else if name == "tall_scroll_end" {
                    format!("<div style='padding-top:2500px'>{input}</div>")
                } else {
                    input.to_owned()
                };
                sources.push((a.into(), field));
                let style = match name {
                    "wide" => "style='width:3200px;height:240px'",
                    "partly_above" => "style='height:1800px;position:relative;top:-1700px'",
                    "tall_transparent" => "style='height:3200px;opacity:0'",
                    _ => "style='height:3200px'",
                };
                let owner = iframe(a, style);
                match name {
                    "tall_covered" => format!("{owner}<div style='position:fixed;inset:0;background:white;z-index:100'>Fictional overlay</div>"),
                    "tall_clipped" => format!("<div style='overflow:hidden;height:240px'>{owner}</div>"),
                    "tall_offscreen" => format!("<div style='height:3200px'></div>{owner}"),
                    _ => owner,
                }
            }
            "nested_tall" => {
                sources.push((a.into(), format!("{input}{}", iframe(b, "style='width:700px;height:2800px'"))));
                sources.push((b.into(), input.into()));
                iframe(a, "style='height:3200px'")
            }
            "nested" => {
                sources.push((a.into(), format!("{input}{}", iframe(b, "height=160"))));
                sources.push((b.into(), input.into()));
                iframe(a, "style='height:520px'")
            }
            "siblings" => {
                sources.extend([(a.into(), input.into()), (b.into(), input.into())]);
                format!("{}{}", iframe(a, ""), iframe(b, ""))
            }
            "hidden_sibling" | "mixed_inline_opaque" | "mixed_inline_target" => {
                let other = if name == "mixed_inline_target" {
                    targeted
                } else {
                    opaque
                };
                sources.extend([(a.into(), input.into()), (other.into(), input.into())]);
                format!(
                    "{}{}",
                    iframe(a, ""),
                    iframe(
                        other,
                        if name == "hidden_sibling" {
                            "style='display:none'"
                        } else {
                            ""
                        }
                    )
                )
            }
            "quota" | "deep" => {
                for i in 0..5 {
                    let url = format!("https://fixture.invalid/depth-{i}");
                    let child = if name == "deep" && i < 4 {
                        iframe(
                            &format!("https://fixture.invalid/depth-{}", i + 1),
                            "style='width:700px;height:450px'",
                        )
                    } else {
                        String::new()
                    };
                    sources.push((url, format!("{input}{child}")));
                }
                if name == "deep" {
                    iframe(&sources[0].0, "style='height:600px'")
                } else {
                    sources
                        .iter()
                        .map(|(url, _)| iframe(url, "style='height:110px'"))
                        .collect::<Vec<_>>()
                        .join("")
                }
            }
            "about_blank" => r#"<iframe title='Fictional inherited blank form' width=900 height=240 src='about:blank'></iframe><script>document.querySelector('iframe').contentDocument.body.innerHTML='<label>Applicant given name <input id=given required></label>';</script>"#.into(),
            "srcdoc" => r#"<iframe title='Fictional unknown response' width=900 height=240 srcdoc="<label>Applicant given name <input id=given required></label>"></iframe>"#.into(),
            _ => {
                sources.push((a.into(), input.into()));
                let cover = if name == "covered" {
                    "<div style='position:fixed;inset:0;background:white;z-index:100'>Fictional overlay</div>"
                } else {
                    ""
                };
                format!(
                    "<main>{}</main><div id=newowner></div>{cover}",
                    iframe(a, "")
                )
            }
        };
        let root = format!("<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{{margin:12px;font:18px Arial}}iframe{{display:block;border:2px solid #777;margin:8px 0}}</style>{body}");
        let mut builder = crate::rehearsal_capture::SnapshotBuilder::start(
            &folder.join(format!("inline-{name}")),
            entry,
        )
        .unwrap();
        builder.add(entry, response(&root)).unwrap();
        for (url, html) in &sources {
            let mut observed_response = response(html);
            if name == "failed_response" {
                observed_response.status = 404;
            }
            builder.add(url, observed_response).unwrap();
        }
        let snapshot = builder.finish().unwrap();
        let mut config = template.clone();
        config.schema = 2;
        config.content_root = snapshot.root;
        config.content_files = snapshot.files;
        config.entry = snapshot.entry;
        config.replay_manifest = Some(snapshot.manifest);
        let started = Instant::now();
        let mut phases = Vec::new();
        let mut mark = |phase: &str| {
            phases.push(json!({"phase":phase,"milliseconds":started.elapsed().as_millis()}))
        };
        let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
        assert!(ready.appcontainer && ready.capabilities == 0);
        let mut native_form = Value::Null;
        let result = (|| -> Result<Value, DriverError> {
            let mut form = driver.open(entry)?;
            mark("opened");
            if name == "unknown_response" {
                // Real observer omission: do not edit retained events or invent
                // an HTTP response. The root is already observed; the reloaded
                // child's new loader receives no Network-domain observation.
                let initial = form.clone();
                driver
                    .cdp
                    .call("Network.disable", json!({}))
                    .map_err(derr)?;
                driver.cdp.call("Runtime.evaluate", json!({"expression":"document.querySelector('iframe').contentWindow.location.reload()"})).map_err(derr)?;
                let mut current = None;
                for _ in 0..16 {
                    driver
                        .cdp
                        .pump_checked(Duration::from_millis(100))
                        .map_err(derr)?;
                    if let Ok(observed) = driver.inspect_current() {
                        if observed != initial
                            && observed.schema.fields.len() == 1
                            && observed.independent_field_entry_blocked()
                        {
                            current = Some(observed);
                            break;
                        }
                    }
                }
                form = current.ok_or_else(|| {
                    DriverError("Unobserved HTTP response did not retain review".into())
                })?;
            }
            native_form = json!({"schema":form.schema,"walls":form.walls,"independent_entry_blocked":form.independent_field_entry_blocked()});
            let eligible = matches!(
                name,
                "nested"
                    | "siblings"
                    | "hidden_sibling"
                    | "srcdoc"
                    | "about_blank"
                    | "navigation"
                    | "reparented_owner"
                    | "replaced_owner"
                    | "tall"
                    | "wide"
                    | "partly_above"
                    | "nested_tall"
                    | "tall_scroll_end"
            );
            if eligible == form.independent_field_entry_blocked() {
                let root = driver.document_with_frames(true)?;
                let identity = driver.frame_inspection_identity(&root);
                native_form["frame_identity"] =
                    json!(identity.as_ref().map_err(|error| error.to_string()));
                native_form["cached_main_status"] = json!(driver.document_status);
                native_form["native_frame_tree"] = driver
                    .cdp
                    .call("Page.getFrameTree", json!({}))
                    .map_err(derr)?;
                native_form["native_dom"] = root;
                return Err(DriverError(format!(
                    "Unexpected inline eligibility for {name}"
                )));
            }
            if form.schema.fields.is_empty() && !matches!(name, "tall_clipped" | "tall_transparent")
            {
                return Err(DriverError(
                    "Fixture's known inline field was not inspected".into(),
                ));
            }
            if !eligible {
                let mut called = false;
                // A clipped/transparent scope can expose no inspected control.
                // Do not invent one to claim that its action guard was tested.
                let attempted = form.schema.fields.first().map(|field| {
                    driver.rehearse_recorded_field_reviewed(
                        &form,
                        &field.selector,
                        "Avery",
                        &[],
                        |_| {
                            called = true;
                            Ok(())
                        },
                    )
                });
                if attempted.as_ref().is_some_and(Result::is_ok)
                    || called
                    || driver
                        .trace
                        .iter()
                        .any(|event| event.starts_with("clicked") || event.starts_with("typed"))
                {
                    return Err(DriverError(
                        "An unresolved frame scope reached an interaction".into(),
                    ));
                }
                if !form
                    .walls
                    .iter()
                    .any(|wall| wall == UNINSPECTED_FRAME_REVIEW)
                {
                    return Err(DriverError(
                        "An incomplete frame census lost its barrier".into(),
                    ));
                }
                return Ok(
                    json!({"status":"needs_review","fields_entered":0,"command_attempted":attempted.is_some(),"callback_reached":called,"walls":form.walls}),
                );
            }
            let expected = if matches!(name, "nested" | "siblings" | "nested_tall") {
                2
            } else {
                1
            };
            if form.schema.fields.len() != expected
                || form
                    .schema
                    .fields
                    .iter()
                    .any(|field| !field.required || !field.selector.starts_with("waypoint-frame:"))
            {
                return Err(DriverError(
                    "Authored inline control identity/count/requiredness differs".into(),
                ));
            }
            let mut stale_scope = Value::Null;
            if matches!(name, "navigation" | "reparented_owner" | "replaced_owner") {
                let old = form.clone();
                let mutation=match name {
                    "navigation"=>"document.querySelector('iframe').contentWindow.location.reload()",
                    "reparented_owner"=>"(() => { const parent = document.getElementById('newowner'); if (typeof parent.moveBefore !== 'function') throw new Error('State-preserving frame move is unavailable'); parent.moveBefore(document.querySelector('iframe'), null); })()",
                    _=>"document.querySelector('iframe').replaceWith(document.querySelector('iframe').cloneNode(true))",
                };
                driver
                    .cdp
                    .call("Runtime.evaluate", json!({"expression":mutation}))
                    .map_err(derr)?;
                let mut fresh = None;
                for _ in 0..16 {
                    driver
                        .cdp
                        .pump_checked(Duration::from_millis(100))
                        .map_err(derr)?;
                    if let Ok(current) = driver.inspect_current() {
                        if current != old
                            && !current.independent_field_entry_blocked()
                            && current.schema.fields.len() == 1
                        {
                            fresh = Some(current);
                            break;
                        }
                    }
                }
                form = fresh.ok_or_else(|| {
                    DriverError(
                        "Changed inline scope did not obtain a fresh complete inspection".into(),
                    )
                })?;
                if name == "reparented_owner"
                    && (form.schema != old.schema || form.walls != old.walls)
                {
                    return Err(DriverError(
                        "Reparented fixture must isolate native identity from equal schema/walls"
                            .into(),
                    ));
                }
                let mut called = false;
                let attempted = driver.rehearse_recorded_field_reviewed(
                    &old,
                    &old.schema.fields[0].selector,
                    "Stale value must not enter",
                    &[],
                    |_| {
                        called = true;
                        Ok(())
                    },
                );
                if attempted.is_ok()
                    || called
                    || !driver
                        .text_value_snapshot(&form.schema.fields[0].selector)?
                        .value
                        .is_empty()
                {
                    return Err(DriverError(
                        "A stale inline scope reached entry or review".into(),
                    ));
                }
                stale_scope = json!({"old_entry_refused":true,"old_callback_reached":called,"schema_equal":form.schema==old.schema,"walls_equal":form.walls==old.walls});
            }
            let mut entered = Vec::new();
            for (i, field) in form.schema.fields.iter().enumerate() {
                let value = if i == 0 { "Avery" } else { "Jordan" };
                let captured = driver.rehearse_recorded_field_reviewed(
                    &form,
                    &field.selector,
                    value,
                    &[],
                    |capture| {
                        crate::rehearsal_capture::write_new(
                            &folder,
                            &format!("{name}-{i}-before.base64"),
                            capture.image().as_bytes(),
                        )
                        .map(|_| ())
                        .map_err(DriverError)
                    },
                )?;
                driver.verify_recorded_field(&captured)?;
                mark("entry_verified");
                if captured.value() != value {
                    return Err(DriverError("Inline native readback differs".into()));
                }
                crate::rehearsal_capture::write_new(
                    &folder,
                    &format!("{name}-{i}-after.base64"),
                    captured.image().as_bytes(),
                )
                .map_err(DriverError)?;
                for other in form.schema.fields.iter().skip(i + 1) {
                    if !driver
                        .text_value_snapshot(&other.selector)?
                        .value
                        .is_empty()
                    {
                        return Err(DriverError(
                            "Another frame's same-ID control changed unexpectedly".into(),
                        ));
                    }
                }
                entered.push(json!({"selector":field.selector,"value":captured.value()}));
            }
            let replay = driver.rehearsal_replay_report()?;
            if replay["blocked"] != 0 || replay["cached_non_read_replies"] != 0 {
                return Err(DriverError(
                    "Inline fixture produced a replay refusal or non-read response".into(),
                ));
            }
            Ok(
                json!({"status":"native_entries_verified","entered":entered,"stale_scope":stale_scope,"replay":replay}),
            )
        })();
        mark("before_close");
        let closed = driver.execute(&waypoint_applications::form_mapping::BrowserCommand::Close);
        mark("after_close");
        let observation = json!({"case":name,"result":result.as_ref().map_err(|error|error.to_string()),"browser_closed":closed.is_ok(),"close_error":closed.as_ref().err().map(|error|error.to_string()),"phases":phases,"native_form":native_form,"scope":"Fictional native qualification only; no model, workflow or external-delivery credit"});
        crate::rehearsal_capture::write_new(
            &folder,
            &format!("inline-{name}.private.json"),
            &serde_json::to_vec_pretty(&observation).unwrap(),
        )
        .unwrap();
        results.push(observation);
        if result.is_err() || closed.is_err() {
            break;
        }
    }
    crate::rehearsal_capture::write_new(
        &folder,
        "inline-coverage.private.json",
        &serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    assert_eq!(
        results.len(),
        if selected.is_some() { 1 } else { cases.len() },
        "Partial native results retained"
    );
    assert!(results
        .iter()
        .all(|case| case["result"].get("Ok").is_some() && case["browser_closed"] == true));
}
