//! Private, native control-plane qualification. No product debug API.
use super::CdpDriver;
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use waypoint_applications::{
    driver::BrowserDriver,
    form_mapping::{BrowserCommand, PacketValue},
};

#[test]
#[ignore = "explicit frozen fictional replay, selected engine, owned Windows qualification"]
fn closed_interceptor_releases_aliases_before_reattachment() {
    let (path, mut driver, ready) = frozen_fixture();
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#name".into(),
            value: PacketValue::Text("Fictional retained name".into()),
        })
        .unwrap();
    let targets = driver
        .cdp
        .call_browser("Target.getTargets", json!({}))
        .unwrap();
    let matching: Vec<_> = targets["targetInfos"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|info| info["type"] == "page" && info["url"] == ready.entry_url)
        .collect();
    assert_eq!(matching.len(), 1);
    let target_id = matching[0]["targetId"].as_str().unwrap();
    let extra = driver
        .cdp
        .call_browser(
            "Target.attachToTarget",
            json!({"targetId":target_id,"flatten":true}),
        )
        .unwrap();
    let extra = extra["sessionId"].as_str().unwrap().to_string();
    let primary = driver.cdp.session.clone();
    driver
        .cdp
        .call_browser("Target.detachFromTarget", json!({"sessionId":primary}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let retired = loop {
        let report = driver.rehearsal_replay_report().unwrap();
        if report["closed_aliases"].as_u64().unwrap() >= 1 && report["pending_detachments"] == 0 {
            break report;
        }
        assert!(
            Instant::now() < deadline,
            "Related sessions were not closed: {report}"
        );
        driver.cdp.pump(Duration::from_millis(50));
    };
    let absent = driver
        .cdp
        .call_browser("Target.detachFromTarget", json!({"sessionId":extra}))
        .unwrap_err();
    assert!(
        absent.to_string().contains("No session with given id"),
        "{absent}"
    );
    let fresh = driver
        .cdp
        .call_browser(
            "Target.attachToTarget",
            json!({"targetId":target_id,"flatten":true}),
        )
        .unwrap();
    driver.cdp.session = fresh["sessionId"].as_str().unwrap().to_string();
    for method in ["Page.enable", "Network.enable", "DOM.enable"] {
        driver.cdp.call(method, json!({})).unwrap();
    }
    driver
        .verify_text_value("#name", "Fictional retained name")
        .unwrap();
    let after = driver.rehearsal_replay_report().unwrap();
    driver.execute(&BrowserCommand::Close).unwrap();
    let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
        .join("Packages")
        .join(ready.profile_name);
    assert!(!profile.join("AC").exists());
    std::fs::write(path.parent().unwrap().join("alias-lifecycle.private.json"),serde_json::to_vec_pretty(&json!({"complete":true,"scope":"fictional native CDP aliases; not employer application entry","retired":retired,"after":after,"alias_physically_absent":true,"value_retained":true,"profile_removed":true})).unwrap()).unwrap();
}

fn frozen_fixture() -> (PathBuf, CdpDriver, super::rehearsal::RehearsalReady) {
    let path = PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_CONFIG").unwrap())
        .canonicalize()
        .unwrap();
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap();
    assert!(path.starts_with(target));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    assert_eq!(
        waypoint_domain::sha256_hex(&bytes),
        std::env::var("WAYPOINT_REHEARSAL_CONFIG_SHA256").unwrap()
    );
    let config: super::rehearsal::RehearsalConfig = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(config.schema, 2);
    let python = PathBuf::from(std::env::var_os("WAYPOINT_REHEARSAL_PYTHON").unwrap());
    let (mut driver, ready) = CdpDriver::launch_rehearsal(&python, config).unwrap();
    ready.validate().unwrap();
    driver.open(&ready.entry_url).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while driver
        .verify_text_value("#bootstrap-status", "ready")
        .is_err()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    (path, driver, ready)
}

#[test]
#[ignore = "explicit fictional recorded child target; owned frame-control qualification"]
fn recorded_child_scope_preserves_origin_controls_and_parent() {
    let (path, mut driver, ready) = frozen_fixture();
    driver
        .execute(&BrowserCommand::Fill {
            selector: "#name".into(),
            value: PacketValue::Text("Parent preserved".into()),
        })
        .unwrap();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#open-inspected".into(),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while driver
        .verify_text_value("#interactive-status", "ready")
        .is_err()
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    let available = driver.captured_frames().unwrap();
    let matches: Vec<_> = available
        .frames
        .iter()
        .filter(|frame| frame.origin() == "https://steps.example.invalid")
        .collect();
    assert_eq!(matches.len(), 1);
    let choice = matches[0].clone();
    let selected = driver.enter_captured_frame(&choice).unwrap();
    let frame_key = choice.id().to_string();
    let frame_origin = selected.origin.clone();
    let mut previous_selector = String::new();
    let mut cases = Vec::new();
    let mut covered_refused = false;
    let mut transformed_refused = false;
    let outcome = (|| -> Result<(), super::DriverError> {
        let form = driver.inspect_current()?;
        if form.origin != "https://steps.example.invalid" {
            return Err(super::DriverError(format!(
                "Child origin did not match: {}",
                form.origin
            )));
        }
        for (label, value) in [
            ("Cycle answer", "Fictional child value"),
            ("Motivation", "Fictional motivation"),
        ] {
            let fields: Vec<_> = form
                .schema
                .fields
                .iter()
                .filter(|field| field.label.ends_with(label))
                .collect();
            if fields.len() != 1 {
                return Err(super::DriverError(format!(
                    "Child field not unique: {label}"
                )));
            }
            if previous_selector.is_empty() {
                previous_selector = fields[0].selector.clone();
                let raw = previous_selector
                    .strip_prefix("waypoint-captured:")
                    .unwrap()
                    .split_once(':')
                    .unwrap()
                    .1;
                assert!(driver
                    .execute(&BrowserCommand::Fill {
                        selector: raw.into(),
                        value: PacketValue::Text("Must not enter".into())
                    })
                    .is_err());
            }
            let command = BrowserCommand::Fill {
                selector: fields[0].selector.clone(),
                value: PacketValue::Text(value.into()),
            };
            let result = driver
                .execute(&command)
                .and_then(|_| driver.verify_text_value(&fields[0].selector, value))
                .map_err(|e| e.to_string());
            cases.push(json!({"label":label,"result":result}));
        }
        for label in ["Remote work", "Team"] {
            let fields: Vec<_> = form
                .schema
                .fields
                .iter()
                .filter(|field| field.label.ends_with(label))
                .collect();
            if fields.len() != 1 {
                return Err(super::DriverError(format!(
                    "Child choice not unique: {label}"
                )));
            }
            let selector = fields[0].selector.clone();
            let command = if label == "Remote work" {
                BrowserCommand::Fill {
                    selector,
                    value: PacketValue::Bool(true),
                }
            } else {
                BrowserCommand::SelectOption {
                    selector,
                    label: "Platform".into(),
                }
            };
            let result = driver
                .execute(&command)
                .and_then(|_| driver.verify_choice(&command))
                .map_err(|e| e.to_string());
            cases.push(json!({"label":label,"result":result}));
        }
        let files: Vec<_> = form
            .schema
            .fields
            .iter()
            .filter(|field| field.label.ends_with("Resume"))
            .collect();
        if files.len() != 1 {
            return Err(super::DriverError("Child file field not unique".into()));
        }
        let upload = BrowserCommand::UploadFile {
            selector: files[0].selector.clone(),
            file_name: "fictional.pdf".into(),
        };
        let result = driver
            .execute(&upload)
            .and_then(|_| {
                driver.verify_local_file(
                    &files[0].selector,
                    &ready.content_root.join("fictional.pdf"),
                )
            })
            .map_err(|e| e.to_string());
        cases.push(json!({"label":"Resume","result":result}));
        let buttons = driver.page_controls()?;
        let next: Vec<_> = buttons
            .controls
            .iter()
            .filter(|control| control.label == "Continue section" && !control.disabled)
            .collect();
        if next.len() != 1 {
            return Err(super::DriverError(
                "Child section control not unique".into(),
            ));
        }
        driver.scroll_field_into_view(&next[0].selector)?;
        let node=driver.node_id(&next[0].selector)?.unwrap();
        let model=driver.cdp.call("DOM.getBoxModel",json!({"nodeId":node})).map_err(super::derr)?;
        let quad=model["model"]["border"].as_array().unwrap();
        let x=[0,2,4,6].iter().map(|i|quad[*i].as_f64().unwrap()).sum::<f64>()/4.0;
        let y=[1,3,5,7].iter().map(|i|quad[*i].as_f64().unwrap()).sum::<f64>()/4.0;
        let point=driver.captured_pointer_point(node,x,y)?.unwrap();
        let overlay=driver.cdp.call_session(&point.session,"Runtime.evaluate",json!({"expression":format!(r#"(()=>{{const frame=document.querySelector('iframe[src="https://steps.example.invalid/interactive"]'),r=frame.getBoundingClientRect(),overlay=document.createElement('div');overlay.id='qualification-overlay';Object.assign(overlay.style,{{position:'fixed',left:'{}px',top:'{}px',width:'6px',height:'6px',zIndex:'2147483647',background:'red'}});document.body.append(overlay);return Math.abs((r.left+r.right)/2-{})>4 || Math.abs((r.top+r.bottom)/2-{})>4;}})()"#,point.x-3.0,point.y-3.0,point.x,point.y),"returnByValue":true})).map_err(super::derr)?;
        assert_eq!(overlay.pointer("/result/value").and_then(serde_json::Value::as_bool),Some(true),"Overlay must cover the actual click point while leaving the iframe center clear");
        covered_refused=driver.execute(&BrowserCommand::Click{selector:next[0].selector.clone()}).is_err() && !driver.inspect_current()?.schema.fields.iter().any(|field|field.label.ends_with("Followup"));
        driver.cdp.call_session(&point.session,"Runtime.evaluate",json!({"expression":"document.getElementById('qualification-overlay').remove();document.querySelector('iframe[src=\"https://steps.example.invalid/interactive\"]').style.transform='translateX(1px)'"})).map_err(super::derr)?;
        transformed_refused=driver.execute(&BrowserCommand::Click{selector:next[0].selector.clone()}).is_err();
        driver.cdp.call_session(&point.session,"Runtime.evaluate",json!({"expression":"document.querySelector('iframe[src=\"https://steps.example.invalid/interactive\"]').style.transform=''"})).map_err(super::derr)?;
        if !covered_refused || !transformed_refused {return Err(super::DriverError("Covered or transformed frame click was not refused".into()));}
        driver.execute(&BrowserCommand::Click {
            selector: next[0].selector.clone(),
        })?;
        let followup = driver.inspect_current()?;
        let fields: Vec<_> = followup
            .schema
            .fields
            .iter()
            .filter(|field| field.label.ends_with("Followup"))
            .collect();
        if fields.len() != 1 {
            return Err(super::DriverError("Child followup field not unique".into()));
        }
        let selector = fields[0].selector.clone();
        let result = driver
            .execute(&BrowserCommand::Fill {
                selector: selector.clone(),
                value: PacketValue::Text("Fictional followup".into()),
            })
            .and_then(|_| driver.verify_text_value(&selector, "Fictional followup"))
            .map_err(|e| e.to_string());
        cases.push(json!({"label":"Followup after section change","result":result}));
        let focus = driver.screenshot_field_context(&selector)?;
        std::fs::write(
            path.parent().unwrap().join("followup-screenshot.base64"),
            focus,
        )
        .unwrap();
        Ok(())
    })()
    .map_err(|e| e.to_string());
    let image = driver.screenshot_base64().map_err(|e| e.to_string());
    if let Ok(image) = &image {
        std::fs::write(
            path.parent().unwrap().join("child-screenshot.base64"),
            image,
        )
        .unwrap();
    }
    driver.leave_captured_frame().unwrap();
    let stale_rejected = driver
        .verify_text_value(&previous_selector, "Fictional child value")
        .is_err();
    let second = driver.enter_captured_frame(&choice).unwrap();
    assert!(second
        .schema
        .fields
        .iter()
        .all(|field| field.selector != previous_selector));
    assert!(driver
        .execute(&BrowserCommand::Fill {
            selector: previous_selector.clone(),
            value: PacketValue::Text("Must not enter".into())
        })
        .is_err());
    let reload_selector = second
        .schema
        .fields
        .iter()
        .find(|field| field.label.ends_with("Cycle answer"))
        .unwrap()
        .selector
        .clone();
    let before = driver.cdp.call("Page.getFrameTree", json!({})).unwrap();
    driver
        .cdp
        .call(
            "Runtime.evaluate",
            json!({"expression":"location.reload()"}),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let tree = driver.cdp.call("Page.getFrameTree", json!({})).unwrap();
        if tree.pointer("/frameTree/frame/loaderId") != before.pointer("/frameTree/frame/loaderId")
        {
            break;
        }
        assert!(Instant::now() < deadline, "Recorded child did not reload");
        std::thread::sleep(Duration::from_millis(50));
    }
    let reload_stale_rejected = driver
        .execute(&BrowserCommand::Fill {
            selector: reload_selector.clone(),
            value: PacketValue::Text("Must not enter".into()),
        })
        .is_err();
    let refreshed = driver.inspect_current().unwrap();
    let fresh = refreshed
        .schema
        .fields
        .iter()
        .find(|field| field.label.ends_with("Cycle answer"))
        .unwrap();
    assert_ne!(fresh.selector, reload_selector);
    driver.verify_text_value(&fresh.selector, "").unwrap();
    driver.leave_captured_frame().unwrap();
    let parent_ok = driver
        .verify_text_value("#name", "Parent preserved")
        .is_ok();
    let fresh_frames = driver.captured_frames().unwrap();
    let fresh_choice = fresh_frames
        .frames
        .iter()
        .find(|frame| frame.origin() == "https://steps.example.invalid")
        .unwrap()
        .clone();
    driver
        .execute(&BrowserCommand::Click {
            selector: "#close-inspected".into(),
        })
        .unwrap();
    let removed_refused = driver.enter_captured_frame(&fresh_choice).is_err();
    let parent_image = driver.screenshot_base64().unwrap();
    std::fs::write(
        path.parent().unwrap().join("replay-screenshot.base64"),
        parent_image,
    )
    .unwrap();
    let passed = outcome.is_ok()
        && stale_rejected
        && reload_stale_rejected
        && covered_refused
        && transformed_refused
        && removed_refused
        && image.is_ok()
        && parent_ok
        && cases.len() == 6
        && cases.iter().all(|case| case["result"].get("Ok").is_some());
    driver.execute(&BrowserCommand::Close).unwrap();
    let profile = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
        .join("Packages")
        .join(ready.profile_name);
    assert!(!profile.join("AC").exists());
    std::fs::write(path.parent().unwrap().join("native.private.json"),serde_json::to_vec_pretty(&json!({"complete":passed,"scope":"fictional isolated recorded child controls, upload, section change and reload; no real form or web entry route","frame_key":frame_key,"frame_origin":frame_origin,"stale_selector_rejected":stale_rejected,"reload_stale_selector_rejected":reload_stale_rejected,"covered_click_refused":covered_refused,"transformed_click_refused":transformed_refused,"removed_frame_refused":removed_refused,"outcome":outcome,"cases":cases,"child_png_returned":image.is_ok(),"child_image_error":image.as_ref().err(),"parent_retained":parent_ok,"profile_removed":true})).unwrap()).unwrap();
    assert!(
        passed,
        "Child control criteria failed; inspect private native report"
    );
}
