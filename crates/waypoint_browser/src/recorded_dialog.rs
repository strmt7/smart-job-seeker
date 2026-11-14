//! Native witnesses and transitions for dialogs in zero-capability saved copies.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
struct Control {
    selector: String,
    label: String,
    available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
struct Scene {
    count: usize,
    selector: String,
    title: String,
    text: String,
    controls: Vec<Control>,
    values_sha256: String,
}

/// Descriptive data only. Selectors and native witnesses never enter prompts.
#[derive(serde::Serialize)]
pub struct DialogDescription {
    pub title: String,
    pub text: String,
    pub controls: Vec<DialogChoice>,
}
#[derive(serde::Serialize)]
pub struct DialogChoice {
    pub id: String,
    pub label: String,
    pub available: bool,
}

pub struct RecordedDialog {
    document: RecordedDocument,
    form: LiveForm,
    scene: Scene,
}
impl RecordedDialog {
    pub fn description(&self) -> DialogDescription {
        DialogDescription {
            title: self.scene.title.clone(),
            text: self.scene.text.clone(),
            controls: self
                .scene
                .controls
                .iter()
                .enumerate()
                .map(|(i, c)| DialogChoice {
                    id: format!("control-{i}"),
                    label: c.label.clone(),
                    available: c.available,
                })
                .collect(),
        }
    }
}

pub struct RecordedDialogCapture {
    document: RecordedDocument,
    values_sha256: String,
    form: LiveForm,
    image: String,
    replay: Value,
    non_read: u64,
}
impl RecordedDialogCapture {
    pub fn form(&self) -> &LiveForm {
        &self.form
    }
    pub fn image(&self) -> &str {
        &self.image
    }
}

pub enum RecordedDialogOutcome {
    NativeClosed {
        capture: Box<RecordedDialogCapture>,
    },
    Stopped {
        reason: &'static str,
        image: Option<String>,
    },
}

impl CdpDriver {
    fn dialog_data(&mut self) -> Result<Value, DriverError> {
        let root = self.document()?;
        let node = root["nodeId"].as_u64().ok_or_else(review)?;
        let object = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = object["object"]["objectId"].as_str().ok_or_else(review)?;
        let result = self.cdp.call("Runtime.callFunctionOn",json!({"objectId":object,"functionDeclaration":include_str!("recorded_dialog.js"),"returnByValue":true,"awaitPromise":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let result = result.map_err(derr)?;
        released.map_err(derr)?;
        if result.get("exceptionDetails").is_some() {
            return Err(review());
        }
        Ok(result["result"]["value"].clone())
    }
    fn dialog_values_hash(&mut self) -> Result<String, DriverError> {
        let value = self.dialog_data()?;
        value["values_sha256"]
            .as_str()
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
            .map(str::to_owned)
            .ok_or_else(review)
    }
    fn dialog_scene(&mut self) -> Result<Option<Scene>, DriverError> {
        let value = self.dialog_data()?;
        if value["count"] == 0
            && value["values_sha256"]
                .as_str()
                .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Ok(None);
        }
        let scene: Scene = serde_json::from_value(value).map_err(|_| review())?;
        if scene.count != 1
            || scene.controls.is_empty()
            || scene.controls.len() > 12
            || scene.text.len() > 16000
            || scene.selector.len() > 4096
            || scene.values_sha256.len() != 64
            || !scene.values_sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(review());
        }
        Ok(Some(scene))
    }

    pub fn verify_recorded_dialog_closed(
        &mut self,
        capture: &RecordedDialogCapture,
    ) -> Result<(), DriverError> {
        self.require_recorded_frames()?;
        let report = self.rehearsal_replay_report()?;
        if self.recorded_document()? != capture.document
            || self.inspect_current()? != capture.form
            || self.dialog_scene()?.is_some()
            || self.dialog_values_hash()? != capture.values_sha256
            || self.observed_non_read_requests()? != capture.non_read
            || [
                "blocked",
                "cached_non_read_replies",
                "unreplayed_transports",
                "unmanaged_child_sessions",
                "unsupported_targets",
            ]
            .iter()
            .any(|key| report[*key] != capture.replay[*key])
            || self.recorded_document()? != capture.document
        {
            return Err(review());
        }
        Ok(())
    }

    pub fn recorded_dialog(
        &mut self,
        form: &LiveForm,
    ) -> Result<Option<RecordedDialog>, DriverError> {
        self.require_recorded_frames()?;
        let document = self.recorded_document()?;
        if self.inspect_current()? != *form {
            return Err(review());
        }
        let scene = self.dialog_scene()?;
        if self.recorded_document()? != document || self.inspect_current()? != *form {
            return Err(review());
        }
        Ok(scene.map(|scene| RecordedDialog {
            document,
            form: form.clone(),
            scene,
        }))
    }

    /// A local proposal can select a witnessed control only. This never accepts
    /// a live driver, supplies a grant or establishes server-side consent.
    pub fn dismiss_recorded_dialog_reviewed(
        &mut self,
        dialog: &RecordedDialog,
        choice: &str,
        mut review_target: impl FnMut(&RecordedTargetCapture) -> Result<(), DriverError>,
    ) -> Result<RecordedDialogOutcome, DriverError> {
        use RecordedDialogOutcome::*;
        self.require_recorded_frames()?;
        let index = choice
            .strip_prefix("control-")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|i| format!("control-{i}") == choice)
            .ok_or_else(review)?;
        let control = dialog
            .scene
            .controls
            .get(index)
            .filter(|c| c.available)
            .ok_or_else(review)?;
        if dialog
            .scene
            .controls
            .iter()
            .filter(|other| {
                other
                    .label
                    .trim()
                    .eq_ignore_ascii_case(control.label.trim())
            })
            .count()
            != 1
        {
            return Err(review());
        }
        let current = |driver: &mut Self| -> Result<Option<&'static str>, DriverError> {
            Ok(if driver.recorded_document()? != dialog.document {
                Some("document_changed")
            } else if driver.inspect_current()? != dialog.form {
                Some("form_changed")
            } else if driver.dialog_scene()?.as_ref() != Some(&dialog.scene) {
                Some("dialog_changed")
            } else {
                None
            })
        };
        if let Some(reason) = current(self)? {
            return Ok(Stopped {
                reason,
                image: None,
            });
        }
        let selector = self.captured_selector(&control.selector);
        let network_before = self.observed_non_read_requests()?;
        let replay_before = self.rehearsal_replay_report()?;
        let intact = |driver: &mut Self| -> Result<bool, DriverError> {
            let report = driver.rehearsal_replay_report()?;
            Ok(driver.observed_non_read_requests()? == network_before
                && [
                    "blocked",
                    "cached_non_read_replies",
                    "unreplayed_transports",
                    "unmanaged_child_sessions",
                    "unsupported_targets",
                ]
                .iter()
                .all(|key| report[*key] == replay_before[*key]))
        };
        let image = self.screenshot_field_context(&selector)?;
        let detail = self.screenshot_control(&selector)?;
        let changed = current(self)?;
        if changed.is_some() || !intact(self)? {
            return Ok(Stopped {
                reason: changed.unwrap_or("replay_gap"),
                image: Some(image),
            });
        }
        review_target(&RecordedTargetCapture {
            associated_label_detail: false,
            witness: None,
            detail_image: Some(detail),
            detail_error: None,
            image,
            label: control.label.clone(),
            required_marker: false,
            file_group_label: None,
            kind: "button",
        })?;
        let changed = current(self)?;
        if changed.is_some() || !intact(self)? {
            return Ok(Stopped {
                reason: changed.unwrap_or("replay_gap"),
                image: self.screenshot_base64().ok(),
            });
        }
        let node = self.node_id(&selector)?.ok_or_else(review)?;
        if self
            .click_node_guarded(
                node,
                &selector,
                ClickGuard::RecordedDialog {
                    label: &control.label,
                    dialog_selector: &dialog.scene.selector,
                    dialog_text: &dialog.scene.text,
                    non_read_before: network_before,
                },
            )
            .is_err()
        {
            return Ok(Stopped {
                reason: "interaction_unverified",
                image: self.screenshot_base64().ok(),
            });
        }
        for _ in 0..3 {
            self.cdp
                .pump_checked(Duration::from_millis(250))
                .map_err(derr)?;
        }
        let image = self.screenshot_base64().ok();
        if !intact(self)? {
            return Ok(Stopped {
                reason: "replay_gap",
                image,
            });
        }
        if self.recorded_document()? != dialog.document {
            return Ok(Stopped {
                reason: "document_changed",
                image,
            });
        }
        if self.dialog_scene()?.is_some() {
            return Ok(Stopped {
                reason: "dialog_remains",
                image,
            });
        }
        // Values are checked again even after the dialog disappeared.
        let value = self.dialog_values_hash()?;
        if value != dialog.scene.values_sha256 {
            return Ok(Stopped {
                reason: "field_changed",
                image,
            });
        }
        let form = self.inspect_current()?;
        let image = image.ok_or_else(review)?;
        Ok(NativeClosed {
            capture: Box::new(RecordedDialogCapture {
                document: dialog.document.clone(),
                values_sha256: dialog.scene.values_sha256.clone(),
                form,
                image,
                replay: self.rehearsal_replay_report()?,
                non_read: network_before,
            }),
        })
    }
}

#[cfg(all(test, windows))]
#[path = "recorded_dialog_qualification.rs"]
mod passive_qualification;
