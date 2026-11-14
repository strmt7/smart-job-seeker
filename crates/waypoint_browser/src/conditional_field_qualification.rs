use super::*;
use waypoint_applications::driver::BrowserDriver;

#[test]
#[ignore = "explicit contained engine; native associated-label image geometry"]
fn native_label_detail_captures_complete_context_or_refuses() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let mut cases = Vec::new();
    for (name, css, allowed) in [
        ("complete", "", true),
        ("hidden", "label{display:none}", false),
        ("distant", "label{margin-left:800px}", false),
        (
            "clipped",
            "fieldset{min-width:0;width:180px;overflow:hidden}label{width:500px}",
            false,
        ),
    ] {
        let entry = "https://fixture.invalid/label-context";
        let html = format!("<!doctype html><meta charset=utf-8><link rel=icon href=data:,><style>body{{font:18px Arial;margin:20px}}fieldset{{width:650px}}label{{display:block;margin:12px 0 4px}}select{{font:inherit;padding:6px}}{css}</style><fieldset><legend>Profile location</legend><label for=country>Current country of residence</label><select id=country><option>Choose</option><option>Canada</option></select></fieldset>");
        let wire = format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}",html.len());
        let response = waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap();
        let mut builder =
            crate::rehearsal_capture::SnapshotBuilder::start(&folder.join(name), entry).unwrap();
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
        let result = (|| -> Result<Value, DriverError> {
            driver.open(entry)?;
            let clipping = if name == "clipped" {
                let native = driver.cdp.call("Runtime.evaluate",json!({"expression":"(()=>{const l=document.querySelector('label').getBoundingClientRect(),g=document.querySelector('fieldset').getBoundingClientRect();return {label_right:l.right,group_right:g.right,group_width:g.width};})()","returnByValue":true})).map_err(derr)?;
                let value = native["result"]["value"].clone();
                if !matches!((value["label_right"].as_f64(),value["group_right"].as_f64()),(Some(label),Some(group)) if label > group)
                {
                    return Err(DriverError(
                        "Clipped fixture did not actually clip its label".into(),
                    ));
                }
                value
            } else {
                Value::Null
            };
            let image = driver.screenshot_field_detail_bound("#country");
            if allowed {
                let (image, associated) = image?;
                if !associated {
                    return Err(DriverError(
                        "Native associated context was not bound".into(),
                    ));
                }
                let node = driver
                    .node_id("#country")?
                    .ok_or_else(|| DriverError("Field disappeared".into()))?;
                let bounds = driver
                    .field_label_context_bounds(node)?
                    .ok_or_else(|| DriverError("Native context disappeared".into()))?;
                let (width, height) = (bounds[2] - bounds[0], bounds[5] - bounds[1]);
                if !(600.0..=720.0).contains(&width) || !(100.0..=400.0).contains(&height) {
                    return Err(DriverError(
                        "Complete label/group context was not captured within budget".into(),
                    ));
                }
                crate::rehearsal_capture::write_new(
                    &folder,
                    &format!("{name}.base64.private.txt"),
                    image.as_bytes(),
                )
                .map_err(DriverError)?;
                Ok(
                    json!({"width":width,"height":height,"base64_sha256":waypoint_domain::sha256_hex(image.as_bytes())}),
                )
            } else if image.is_err() {
                Ok(json!({"refused":true,"clipping":clipping}))
            } else {
                Err(DriverError(
                    "Hidden/clipped/distant label context was accepted".into(),
                ))
            }
        })();
        let closed = driver
            .execute(&waypoint_applications::form_mapping::BrowserCommand::Close)
            .is_ok();
        cases.push(json!({"case":name,"allowed":allowed,"result":result.as_ref().map_err(|e|e.to_string()),"closed":closed}));
        if result.is_err() || !closed {
            break;
        }
    }
    crate::rehearsal_capture::write_new(
        &folder,
        "label-context-cases.private.json",
        &serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    assert_eq!(cases.len(), 4);
    assert!(cases
        .iter()
        .all(|case| case["closed"] == true && case["result"].get("Ok").is_some()));
}

#[test]
#[ignore = "explicit contained engine; fictional conditional entry and native continuation"]
fn conditional_native_append_and_scope_guards() {
    let (template, folder, python) = crate::capture_session::qualification::qualification_config();
    let mut observations = Vec::new();
    for name in [
        "append",
        "inline_append",
        "legacy",
        "changed_target",
        "removed_target",
        "replaced_target",
        "new_wall",
        "prior_changed",
        "reloaded",
        "non_read",
    ] {
        let entry = "https://fixture.invalid/conditional-profile";
        let initial = if name == "prior_changed" {
            "<label for=prior>Earlier full name</label><input id=prior required>"
        } else {
            ""
        };
        let appended = "<fieldset id=person><legend>Applicant details</legend><label for=full>Applicant full name</label><input id=full required></fieldset>";
        let extra = match name {
            "changed_target" => "document.querySelector('label[for=country]').textContent='Country of citizenship';",
            "removed_target" => "document.getElementById('location').remove();",
            "replaced_target" => "const old=document.getElementById('country'),copy=old.cloneNode(true);copy.value=old.value;old.replaceWith(copy);",
            "new_wall" => "document.getElementById('extra').insertAdjacentHTML('beforeend','<div role=dialog aria-modal=true style=\"position:fixed;inset:0;background:white;z-index:999\">Review this form<button type=button>Close</button></div>');",
            "prior_changed" => "document.getElementById('prior').value='';",
            "reloaded" => "location.reload();",
            "non_read" => "fetch('https://fixture.invalid/blocked-write',{method:'POST',body:'fictional'}).catch(()=>{});",
            _ => "",
        };
        let script = format!("document.getElementById('country').addEventListener('change',()=>{{if(!document.getElementById('full')){{document.getElementById('extra').innerHTML={};document.getElementById('full').addEventListener('input',()=>{{if(!document.getElementById('citizenship'))document.getElementById('extra').insertAdjacentHTML('beforeend',{});}},{{once:true}});}}{extra}}});",
            serde_json::to_string(appended).unwrap(),serde_json::to_string("<fieldset id=nationality><legend>Citizenship</legend><label for=citizenship>Country of citizenship</label><select id=citizenship required><option value=''>Choose</option><option value=ca>Canada</option><option value=fr>France</option></select></fieldset>").unwrap());
        let html=format!("<!doctype html><html lang=en><head><meta charset=utf-8><link rel=icon href=data:,><style>body{{font:18px Arial;margin:20px;max-width:700px}}label{{display:block;margin:12px 0 4px}}input,select{{font:inherit;padding:6px}}fieldset{{margin-bottom:20px}}</style></head><body>{initial}<fieldset id=location><legend>Profile location</legend><label for=country>Current country of residence</label><select id=country required><option value=''>Choose</option><option value=ca>Canada</option><option value=fr>France</option></select></fieldset><div id=extra></div><script>{script}</script></body></html>");
        crate::rehearsal_capture::write_new(
            &folder,
            &format!("{name}.fixture.private.html"),
            html.as_bytes(),
        )
        .unwrap();
        let wire=format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}",html.len());
        let response = waypoint_net::read_asset_response(
            &mut std::io::Cursor::new(wire.into_bytes()),
            "GET",
            1024 * 1024,
        )
        .unwrap();
        let mut builder =
            crate::rehearsal_capture::SnapshotBuilder::start(&folder.join(name), entry).unwrap();
        if name == "inline_append" {
            let parent="<!doctype html><html><head><meta charset=utf-8><link rel=icon href=data:,></head><body><iframe width=900 height=700 title='Fictional conditional form' src='https://fixture.invalid/conditional-child'></iframe></body></html>";
            let parent_wire=format!("HTTP/1.1 200 Fictional\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{parent}",parent.len());
            builder
                .add(
                    entry,
                    waypoint_net::read_asset_response(
                        &mut std::io::Cursor::new(parent_wire.into_bytes()),
                        "GET",
                        1024 * 1024,
                    )
                    .unwrap(),
                )
                .unwrap();
            builder
                .add("https://fixture.invalid/conditional-child", response)
                .unwrap();
        } else {
            builder.add(entry, response).unwrap();
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
        let mut observation =
            json!({"case":name,"source_sha256":waypoint_domain::sha256_hex(html.as_bytes())});
        let result = (|| -> Result<(), DriverError> {
            let before = driver.open(entry)?;
            observation["before"] = json!({"schema":before.schema,"walls":before.walls});
            observation["before_native"] = driver
                .cdp
                .call("Page.getFrameTree", json!({}))
                .map_err(derr)?;
            let expected = if name == "prior_changed" { 2 } else { 1 };
            if before.schema.fields.len() != expected || before.independent_field_entry_blocked() {
                return Err(DriverError(
                    "Conditional fixture initial inspection differs".into(),
                ));
            }
            let prior = if name == "prior_changed" {
                Some(driver.rehearse_recorded_field_progressive(
                    &before,
                    "#prior",
                    "Avery Example",
                    &[],
                    |_| Ok(()),
                )?)
            } else {
                None
            };
            let country = before
                .schema
                .fields
                .iter()
                .find(|field| field.selector == "#country" || field.selector.ends_with(":#country"))
                .ok_or_else(|| DriverError("Known fixture country was not inspected".into()))?
                .selector
                .clone();
            let verified: Vec<_> = prior.as_ref().into_iter().collect();
            let mut called = false;
            let captured = if name == "legacy" {
                driver.rehearse_recorded_field_reviewed(
                    &before,
                    &country,
                    "Canada",
                    &verified,
                    |_| {
                        called = true;
                        Ok(())
                    },
                )
            } else {
                driver.rehearse_recorded_field_progressive(
                    &before,
                    &country,
                    "Canada",
                    &verified,
                    |_| {
                        called = true;
                        Ok(())
                    },
                )
            };
            observation["first_callback_reached"] = json!(called);
            observation["first_result"] = json!(captured
                .as_ref()
                .map(|capture| capture.value())
                .map_err(|error| error.to_string()));
            let after = driver.inspect_current();
            observation["after"] = json!(after
                .as_ref()
                .map(|form| json!({"schema":form.schema,"walls":form.walls}))
                .map_err(|error| error.to_string()));
            observation["after_native"] = json!(driver
                .cdp
                .call("Page.getFrameTree", json!({}))
                .map_err(|error| error.to_string()));
            if let Some(prior) = &prior {
                observation["prior_still_verified"] =
                    json!(driver.verify_recorded_field(prior).is_ok());
            }
            if !matches!(name, "append" | "inline_append") {
                if captured.is_ok() {
                    return Err(DriverError(
                        "An unqualified conditional change returned entry proof".into(),
                    ));
                }
                observation["outcome"] = json!("review_required_no_retry");
                return Ok(());
            }
            let after = after?;
            let first = captured?;
            if first.continuation() != Some(&after)
                || after.schema.fields.len() != 2
                || after.independent_field_entry_blocked()
            {
                return Err(DriverError(
                    "Conditional native state was not explicitly returned".into(),
                ));
            }
            driver.verify_recorded_field(&first)?;
            let mut stale_callback = false;
            if driver
                .rehearse_recorded_field_progressive(&before, &country, "France", &[], |_| {
                    stale_callback = true;
                    Ok(())
                })
                .is_ok()
                || stale_callback
            {
                return Err(DriverError(
                    "An old conditional plan reached another action".into(),
                ));
            }
            observation["old_plan_refused_before_callback"] = json!(true);
            let full = after
                .schema
                .fields
                .iter()
                .find(|field| field.selector == "#full" || field.selector.ends_with(":#full"))
                .ok_or_else(|| DriverError("Known fixture name was not inspected".into()))?
                .selector
                .clone();
            let second = driver.rehearse_recorded_field_progressive(
                &after,
                &full,
                "Avery Example",
                &[&first],
                |_| Ok(()),
            )?;
            let final_form = driver.inspect_current()?;
            if second.continuation() != Some(&final_form)
                || final_form.schema.fields.len() != 3
                || final_form.independent_field_entry_blocked()
            {
                return Err(DriverError(
                    "Second conditional state was not independently inspected".into(),
                ));
            }
            driver.verify_recorded_field(&first)?;
            driver.verify_recorded_field(&second)?;
            let mut controls = Vec::new();
            for field in &final_form.schema.fields {
                controls.push(json!({"field":field,"control":driver.recorded_control_identity(&final_form,&field.selector)?}));
            }
            observation["final_controls"] = json!(controls);
            observation["values"] = json!([first.value(), second.value()]);
            observation["unknown_untouched"]=json!(driver.cdp.call("Runtime.evaluate",json!({"expression":if name=="inline_append"{"document.querySelector('iframe').contentDocument.getElementById('citizenship').value"}else{"document.getElementById('citizenship').value"},"returnByValue":true})).map_err(derr)?.pointer("/result/value")==Some(&json!("")));
            if observation["unknown_untouched"] != true {
                return Err(DriverError("Unknown conditional answer changed".into()));
            }
            let replay = driver.rehearsal_replay_report()?;
            if replay["blocked"] != 0 || replay["cached_non_read_replies"] != 0 {
                return Err(DriverError("Conditional fixture replay changed".into()));
            }
            observation["outcome"] = json!("two_native_entries_two_fresh_states");
            observation["replay"] = replay;
            Ok(())
        })();
        let closed = driver.execute(&waypoint_applications::form_mapping::BrowserCommand::Close);
        observation["result"] = json!(result.as_ref().map_err(|error| error.to_string()));
        observation["closed"] = json!(closed.is_ok());
        observation["close_error"] = json!(closed.as_ref().err().map(|error| error.to_string()));
        crate::rehearsal_capture::write_new(
            &folder,
            &format!("conditional-{name}.private.json"),
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
        "conditional-cases.private.json",
        &serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    assert_eq!(observations.len(), 10);
    assert!(observations
        .iter()
        .all(|case| case["result"].get("Ok").is_some() && case["closed"] == true));
}
