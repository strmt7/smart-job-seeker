//! Explicit, source-bound entry into one recorded cross-origin frame.
use super::*;
use std::{
    borrow::Cow,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::atomic::{AtomicU64, Ordering},
};
const PREFIX: &str = "waypoint-captured:";
static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct CapturedFrame {
    id: String,
    origin: String,
    label: String,
    parent_session: String,
    parent_frame: String,
    parent_loader: String,
    parent_url: String,
    owner: u64,
    target: String,
    loader: String,
    url: String,
}
impl CapturedFrame {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn label(&self) -> &str {
        &self.label
    }
}
pub struct CapturedFrames {
    pub frames: Vec<CapturedFrame>,
    pub uninspected: usize,
    pub truncated: bool,
}
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedFieldKind {
    Text,
    Select,
    Radio,
}

/// Pixels with independently verified field state in a recorded form.
/// Only the recorded driver can construct this evidence.
pub struct RecordedFieldCapture {
    image: String,
    label: String,
    kind: RecordedFieldKind,
    command: BrowserCommand,
    network_before: u64,
    document: RecordedDocument,
}

#[derive(PartialEq, Eq)]
struct RecordedDocument {
    url: String,
    frame: String,
    loader: String,
    lease: Option<String>,
}

impl RecordedFieldCapture {
    pub fn image(&self) -> &str {
        &self.image
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn value(&self) -> &str {
        match &self.command {
            BrowserCommand::Fill {
                value: PacketValue::Text(value),
                ..
            }
            | BrowserCommand::SelectOption { label: value, .. }
            | BrowserCommand::SelectRadio { label: value, .. } => value,
            _ => unreachable!("recorded field capture always has a scalar answer"),
        }
    }
    pub fn kind(&self) -> RecordedFieldKind {
        self.kind
    }
}
pub(super) struct CapturedScope {
    frame: CapturedFrame,
    session: String,
    key: String,
    parent_sheet: Option<(String, String, String)>,
}
#[derive(PartialEq)]
pub(super) struct CapturedPointerPoint {
    pub(super) session: String,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    child_width: f64,
    child_height: f64,
}
fn frame_caption(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || url::Url::parse(raw).is_ok() {
        return None;
    }
    let label: String = raw
        .chars()
        .filter(|ch| !ch.is_control())
        .take(160)
        .collect();
    (!label.trim().is_empty()).then_some(label)
}

fn review() -> DriverError {
    DriverError("The embedded form changed or could not be verified; inspect it again".into())
}
fn scope_key(frame: &CapturedFrame) -> Result<String, DriverError> {
    let epoch = NEXT_SCOPE
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .map_err(|_| review())?;
    Ok(waypoint_domain::sha256_hex(
        serde_json::to_string(&(
            epoch,
            &frame.parent_frame,
            &frame.parent_loader,
            frame.owner,
            &frame.target,
            &frame.loader,
            &frame.url,
        ))
        .map_err(|_| review())?
        .as_bytes(),
    ))
}
impl CdpDriver {
    /// Exercise an unambiguous text or native choice control. Bind pixels to
    /// readback on both sides of capture and reject a changed scope.
    pub fn rehearse_recorded_field(
        &mut self,
        form: &LiveForm,
        selector: &str,
        value: &str,
    ) -> Result<RecordedFieldCapture, DriverError> {
        use waypoint_applications::form_mapping::FormFieldType;
        self.require_recorded_frames()?;
        let fields: Vec<_> = form
            .schema
            .fields
            .iter()
            .filter(|f| f.selector == selector)
            .collect();
        let [field] = fields.as_slice() else {
            return Err(review());
        };
        if !form.walls.is_empty()
            || field.sensitive
            || value.trim().is_empty()
            || value.len() > 1000
        {
            return Err(review());
        }
        let (kind, command) = match &field.field_type {
            FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea => (
                RecordedFieldKind::Text,
                BrowserCommand::Fill {
                    selector: selector.into(),
                    value: PacketValue::Text(value.into()),
                },
            ),
            FormFieldType::Select { options }
                if options.iter().filter(|label| *label == value).count() == 1 =>
            {
                (
                    RecordedFieldKind::Select,
                    BrowserCommand::SelectOption {
                        selector: selector.into(),
                        label: value.into(),
                    },
                )
            }
            FormFieldType::Radio { options }
                if options.iter().filter(|label| *label == value).count() == 1 =>
            {
                (
                    RecordedFieldKind::Radio,
                    BrowserCommand::SelectRadio {
                        selector: selector.into(),
                        label: value.into(),
                    },
                )
            }
            _ => return Err(review()),
        };
        let document = self.recorded_document()?;
        let check_scope = |driver: &mut Self| {
            if driver.inspect_current()? != *form || driver.recorded_document()? != document {
                return Err(review());
            }
            Ok(())
        };
        check_scope(self)?;
        let network_before = self.observed_non_read_requests()?;
        match kind {
            RecordedFieldKind::Text => {
                self.ensure_text_replaceable(selector, value)?;
                BrowserDriver::check_text_candidate(self, selector, value)?;
                self.execute(&command)?;
            }
            RecordedFieldKind::Select | RecordedFieldKind::Radio => {
                if matches!(kind, RecordedFieldKind::Select) {
                    let node = self.node_id(selector)?.ok_or_else(review)?;
                    let described = self
                        .cdp
                        .call("DOM.describeNode", json!({"nodeId":node}))
                        .map_err(derr)?;
                    if described["node"]["nodeName"] != "SELECT" {
                        return Err(review());
                    }
                }
                if self.verify_choice(&command).is_err() {
                    self.ensure_choice_replaceable(&command)?;
                    self.execute(&command)?;
                }
            }
        }
        self.stop_if_non_read_request_observed(network_before)?;
        self.verify_recorded_command(&command)?;
        let image = if matches!(kind, RecordedFieldKind::Radio) {
            self.screenshot_recorded_radio(selector, value)?
        } else {
            self.screenshot_field_context(selector)?
        };
        check_scope(self)?;
        self.verify_recorded_command(&command)?;
        self.observed_non_read_requests()?;
        self.stop_if_non_read_request_observed(network_before)?;
        Ok(RecordedFieldCapture {
            image,
            label: field.label.clone(),
            kind,
            command,
            network_before,
            document,
        })
    }

    fn recorded_document(&mut self) -> Result<RecordedDocument, DriverError> {
        let url = self.current_url()?;
        let tree = self
            .cdp
            .call("Page.getFrameTree", json!({}))
            .map_err(derr)?;
        let frame = &tree["frameTree"]["frame"];
        let identity = |key: &str| {
            frame[key]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .ok_or_else(review)
        };
        Ok(RecordedDocument {
            url,
            frame: identity("id")?,
            loader: identity("loaderId")?,
            lease: self.captured_scope.as_ref().map(|scope| scope.key.clone()),
        })
    }

    pub fn verify_recorded_field(
        &mut self,
        capture: &RecordedFieldCapture,
    ) -> Result<(), DriverError> {
        self.verify_recorded_command(&capture.command)?;
        if self.recorded_document()? != capture.document {
            return Err(review());
        }
        self.stop_if_non_read_request_observed(capture.network_before)
    }

    fn verify_recorded_command(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        match command {
            BrowserCommand::Fill {
                selector,
                value: PacketValue::Text(value),
            } => self.verify_text_value(selector, value),
            _ => self.verify_choice(command),
        }
    }

    fn screenshot_recorded_radio(
        &mut self,
        selector: &str,
        label: &str,
    ) -> Result<String, DriverError> {
        let node = self
            .radio_selection(selector, label, false, true)?
            .ok_or_else(review)?;
        self.cdp
            .call("DOM.scrollIntoViewIfNeeded", json!({"nodeId":node}))
            .map_err(derr)?;
        if self.captured_scope.is_some() {
            self.screenshot_captured_node(node, true, true)
        } else {
            self.screenshot_node(node, true, false)
        }
    }
    fn require_recorded_frames(&mut self) -> Result<(), DriverError> {
        #[cfg(windows)]
        {
            if !matches!(self.browser, BrowserOwner::Rehearsal(_)) {
                return Err(DriverError(
                    "Embedded form entry requires an isolated recorded page".into(),
                ));
            }
            let report = self.rehearsal_replay_report()?;
            if report["mode"] != "offline_response_replay" {
                return Err(review());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err(DriverError(
                "Isolated recorded frame entry requires Windows".into(),
            ))
        }
    }
    fn with_protocol_session<T>(
        &mut self,
        session: &str,
        read: impl FnOnce(&mut Self) -> Result<T, DriverError>,
    ) -> Result<T, DriverError> {
        let previous = std::mem::replace(&mut self.cdp.session, session.to_owned());
        let outcome = catch_unwind(AssertUnwindSafe(|| read(self)));
        self.cdp.session = previous;
        match outcome {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    fn detach_captured_alias(&mut self, session: &str) -> Result<(), DriverError> {
        match self
            .cdp
            .call_browser("Target.detachFromTarget", json!({"sessionId":session}))
        {
            Ok(_) => Ok(()),
            Err(error) => {
                let missing = error
                    .0
                    .strip_prefix("Target.detachFromTarget: protocol error ")
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .is_some_and(|v| {
                        matches!(v["code"].as_i64(), Some(-32000 | -32602))
                            && v["message"] == "No session with given id"
                    });
                if missing {
                    Ok(())
                } else {
                    let stopped = self.browser.shutdown().is_ok();
                    Err(DriverError(if stopped {"The embedded form session could not close; its browser was stopped"}else{"The embedded form session could not close; browser cleanup is unconfirmed"}.into()))
                }
            }
        }
    }
    fn parent_frame_owner(&mut self, frame: &CapturedFrame) -> Result<u64, DriverError> {
        self.with_protocol_session(&frame.parent_session, |driver| {
            if driver.current_url()? != frame.parent_url {
                return Err(review());
            }
            let tree = driver
                .cdp
                .call("Page.getFrameTree", json!({}))
                .map_err(derr)?;
            if tree.pointer("/frameTree/frame/id").and_then(Value::as_str)
                != Some(&frame.parent_frame)
                || tree
                    .pointer("/frameTree/frame/loaderId")
                    .and_then(Value::as_str)
                    != Some(&frame.parent_loader)
            {
                return Err(review());
            }
            driver
                .cdp
                .call("DOM.getDocument", json!({"depth":0}))
                .map_err(derr)?;
            let owner = driver
                .cdp
                .call("DOM.getFrameOwner", json!({"frameId":frame.target}))
                .map_err(derr)?;
            if owner["backendNodeId"].as_u64() != Some(frame.owner) {
                return Err(review());
            }
            let node = if let Some(node) = owner["nodeId"].as_u64().filter(|n| *n > 0) {
                node
            } else {
                let nodes = driver
                    .cdp
                    .call(
                        "DOM.pushNodesByBackendIdsToFrontend",
                        json!({"backendNodeIds":[frame.owner]}),
                    )
                    .map_err(derr)?;
                nodes["nodeIds"][0]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or_else(review)?
            };
            let (visible, _, _, needs_review, obscured) =
                driver.control_visibility_and_path(node)?;
            if !visible || needs_review || obscured {
                return Err(review());
            }
            Ok(node)
        })
    }
    fn frame_tree_binding(
        &mut self,
        session: &str,
        expected: &CapturedFrame,
    ) -> Result<(String, String), DriverError> {
        let tree = self
            .cdp
            .call_session(session, "Page.getFrameTree", json!({}))
            .map_err(derr)?;
        let frame = tree.pointer("/frameTree/frame").ok_or_else(review)?;
        if frame["id"] != expected.target || frame["parentId"] != expected.parent_frame {
            return Err(review());
        }
        let url = embedded_frame_url(&tree)?;
        let origin = url::Url::parse(&url)
            .map_err(|_| review())?
            .origin()
            .ascii_serialization();
        if origin != expected.origin {
            return Err(review());
        }
        let loader = frame["loaderId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(review)?
            .to_owned();
        Ok((loader, url))
    }
    /// Only direct recorded targets are offered. Uninspectable frames stay explicit.
    pub fn captured_frames(&mut self) -> Result<CapturedFrames, DriverError> {
        self.require_recorded_frames()?;
        if self.captured_scope.is_some() {
            return Err(DriverError(
                "Return to the parent page before choosing another embedded form".into(),
            ));
        }
        let tree = self
            .cdp
            .call("Page.getFrameTree", json!({}))
            .map_err(derr)?;
        let parent = tree.pointer("/frameTree/frame").ok_or_else(review)?;
        if parent.get("parentId").is_some() {
            return Err(review());
        }
        let parent_frame = parent["id"].as_str().ok_or_else(review)?.to_owned();
        let parent_loader = parent["loaderId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(review)?
            .to_owned();
        let parent_url = self.current_url()?;
        let root = self.document_with_frames(true)?;
        let mut owners = Vec::new();
        walk_with_visibility(&root, false, false, &mut |node, _, hidden| {
            if owners.len() >= 32 {
                return false;
            }
            let name = node["nodeName"].as_str().unwrap_or("");
            if !hidden && matches!(name.to_ascii_lowercase().as_str(), "iframe" | "frame") {
                if let (Some(node_id), Some(backend), Some(target)) = (
                    node["nodeId"].as_u64(),
                    node["backendNodeId"].as_u64(),
                    node["frameId"].as_str(),
                ) {
                    let attrs = attributes(node);
                    let label = [attrs.get("aria-label"), attrs.get("title")]
                        .into_iter()
                        .flatten()
                        .find_map(|label| frame_caption(label));
                    owners.push((node_id, backend, target.to_owned(), label));
                }
            }
            owners.len() < 32
        });
        let targets = self
            .cdp
            .call_browser("Target.getTargets", json!({}))
            .map_err(derr)?;
        let targets = targets["targetInfos"].as_array().ok_or_else(review)?;
        let mut result = CapturedFrames {
            frames: vec![],
            uninspected: 0,
            truncated: owners.len() >= 32,
        };
        let started = Instant::now();
        for (node, owner, target, label) in owners {
            if result.frames.len() >= 4 || started.elapsed() > Duration::from_secs(8) {
                result.truncated = true;
                break;
            }
            if self.frame_access(node)? != Some(false) {
                continue;
            }
            let matches: Vec<_> = targets
                .iter()
                .filter(|info| info["type"] == "iframe" && info["targetId"] == target)
                .collect();
            if matches.len() != 1 {
                result.uninspected += 1;
                continue;
            }
            let url = matches[0]["url"].as_str().unwrap_or("").to_owned();
            let Ok(parsed) = url::Url::parse(&url) else {
                result.uninspected += 1;
                continue;
            };
            let origin = parsed.origin().ascii_serialization();
            let mut choice = CapturedFrame {
                id: String::new(),
                origin,
                label: label
                    .or_else(|| matches[0]["title"].as_str().and_then(frame_caption))
                    .unwrap_or_else(|| "Embedded frame".into()),
                parent_session: self.cdp.session.clone(),
                parent_frame: parent_frame.clone(),
                parent_loader: parent_loader.clone(),
                parent_url: parent_url.clone(),
                owner,
                target: target.clone(),
                loader: String::new(),
                url,
            };
            let attached = self
                .cdp
                .call_browser(
                    "Target.attachToTarget",
                    json!({"targetId":target,"flatten":true}),
                )
                .map_err(derr)?;
            let session = attached["sessionId"]
                .as_str()
                .ok_or_else(review)?
                .to_owned();
            let binding = self.frame_tree_binding(&session, &choice);
            self.detach_captured_alias(&session)?;
            match binding {
                Ok((loader, url)) => {
                    choice.loader = loader;
                    choice.url = url;
                    choice.id = scope_key(&choice)?;
                    result.frames.push(choice);
                }
                Err(_) => result.uninspected += 1,
            }
        }
        Ok(result)
    }
    pub fn enter_captured_frame(&mut self, frame: &CapturedFrame) -> Result<LiveForm, DriverError> {
        self.require_recorded_frames()?;
        if self.captured_scope.is_some() || self.cdp.session != frame.parent_session {
            return Err(review());
        }
        let parent = self.inspect_current()?;
        if parent.walls.iter().any(|wall| {
            ![
                INACCESSIBLE_FRAME_FORM_REVIEW,
                UNINSPECTED_FRAME_REVIEW,
                ISOLATED_FRAME_INSPECTION_UNAVAILABLE,
            ]
            .contains(&wall.as_str())
        }) {
            return Err(DriverError(
                "The parent page needs review before entering an embedded form".into(),
            ));
        }
        self.parent_frame_owner(frame)?;
        let attached = self
            .cdp
            .call_browser(
                "Target.attachToTarget",
                json!({"targetId":frame.target,"flatten":true}),
            )
            .map_err(derr)?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or_else(review)?
            .to_owned();
        let setup = (|| {
            let (loader, url) = self.frame_tree_binding(&session, frame)?;
            if loader != frame.loader || url != frame.url {
                return Err(review());
            }
            for method in ["Page.enable", "DOM.enable"] {
                self.cdp
                    .call_session(&session, method, json!({}))
                    .map_err(derr)?;
            }
            self.cdp
                .call_session(
                    &session,
                    "Network.enable",
                    json!({"maxTotalBufferSize":0,"maxResourceBufferSize":0,"maxPostDataSize":0}),
                )
                .map_err(derr)?;
            scope_key(frame)
        })();
        let key = match setup {
            Ok(key) => key,
            Err(error) => {
                self.detach_captured_alias(&session)?;
                return Err(error);
            }
        };
        self.captured_scope = Some(CapturedScope {
            frame: frame.clone(),
            session: session.clone(),
            key,
            parent_sheet: self.capture_sheet.take(),
        });
        self.cdp.session = session;
        self.root_node = None;
        self.document_status = None;
        self.pointer_position = None;
        match self.inspect_current() {
            Ok(form) => Ok(form),
            Err(error) => {
                self.leave_captured_frame()?;
                Err(error)
            }
        }
    }
    pub fn leave_captured_frame(&mut self) -> Result<LiveForm, DriverError> {
        let scope = self.captured_scope.take().ok_or_else(review)?;
        self.cdp.session = scope.frame.parent_session.clone();
        self.root_node = None;
        self.document_status = None;
        self.pointer_position = None;
        self.capture_sheet = scope.parent_sheet;
        self.detach_captured_alias(&scope.session)?;
        self.inspect_current()
    }
    fn validate_captured_frame(&mut self, refresh: bool) -> Result<(), DriverError> {
        let Some(scope) = self.captured_scope.as_ref() else {
            return Ok(());
        };
        let frame = scope.frame.clone();
        let session = scope.session.clone();
        if self.cdp.session != session {
            return Err(review());
        }
        self.parent_frame_owner(&frame)?;
        let (loader, url) = self.frame_tree_binding(&session, &frame)?;
        if loader != frame.loader || url != frame.url {
            if !refresh {
                return Err(review());
            }
            let scope = self.captured_scope.as_mut().ok_or_else(review)?;
            scope.frame.loader = loader;
            scope.frame.url = url;
            scope.key = scope_key(&scope.frame)?;
            self.root_node = None;
            self.document_status = None;
            self.capture_sheet = None;
        }
        Ok(())
    }
    pub(super) fn bind_captured_form(&mut self, mut form: LiveForm) -> LiveForm {
        if let Some(scope) = &self.captured_scope {
            for field in &mut form.schema.fields {
                field.selector = format!("{PREFIX}{}:{}", scope.key, field.selector);
                for section in &mut field.context.sections {
                    section.id = format!("{PREFIX}{}:{}", scope.key, section.id);
                }
            }
        }
        form
    }
    pub(super) fn captured_selector(&self, raw: &str) -> String {
        match &self.captured_scope {
            Some(scope) => format!("{PREFIX}{}:{raw}", scope.key),
            None => raw.to_owned(),
        }
    }
    pub(super) fn decode_captured_selector<'a>(
        &mut self,
        selector: &'a str,
    ) -> Result<Cow<'a, str>, DriverError> {
        match &self.captured_scope {
            Some(scope) => {
                let rest = selector
                    .strip_prefix(PREFIX)
                    .and_then(|s| s.split_once(':'))
                    .filter(|(key, _)| *key == scope.key)
                    .map(|(_, inner)| inner)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(review)?;
                self.validate_captured_frame(false)?;
                Ok(Cow::Borrowed(rest))
            }
            None if selector.starts_with(PREFIX) => Err(review()),
            None => Ok(Cow::Borrowed(selector)),
        }
    }
    pub(super) fn refresh_captured_form(&mut self) -> Result<(), DriverError> {
        self.validate_captured_frame(true)
    }
    /// Translate a verified child point into its direct parent's viewport. This
    /// is restricted to the recorded scope; transformed/covered frames refuse.
    pub(super) fn captured_pointer_point(
        &mut self,
        node: u64,
        x: f64,
        y: f64,
    ) -> Result<Option<CapturedPointerPoint>, DriverError> {
        let Some(scope) = self.captured_scope.as_ref() else {
            return Ok(None);
        };
        let frame = scope.frame.clone();
        self.validate_captured_frame(false)?;
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(review)?;
        let local=self.cdp.call("Runtime.callFunctionOn",json!({"objectId":object,"functionDeclaration":r#"function(){if(!this.isConnected)return null;const r=this.getBoundingClientRect(),x=(r.left+r.right)/2,y=(r.top+r.bottom)/2,root=this.getRootNode();const hit=typeof root.elementFromPoint==="function"?root.elementFromPoint(x,y):null;if(!hit||(hit!==this&&!this.contains(hit)))return null;for(let n=this,depth=0;n;n=n.parentElement){if(++depth>64)return null;const s=getComputedStyle(n);if(s.visibility!=="visible"||Number(s.opacity)===0)return null;}return {x,y,width:innerWidth,height:innerHeight};}"#,"returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let local = local.map_err(derr)?;
        released.map_err(derr)?;
        if local.get("exceptionDetails").is_some() {
            return Err(review());
        }
        let local = local.pointer("/result/value").ok_or_else(review)?;
        let numbers: Vec<_> = ["x", "y", "width", "height"]
            .iter()
            .filter_map(|key| local[key].as_f64().filter(|v| v.is_finite()))
            .collect();
        if numbers.len() != 4
            || (numbers[0] - x).abs() > 0.5
            || (numbers[1] - y).abs() > 0.5
            || x < 0.0
            || y < 0.0
            || x >= numbers[2]
            || y >= numbers[3]
        {
            return Err(review());
        }
        let owner = self.parent_frame_owner(&frame)?;
        let point=self.with_protocol_session(&frame.parent_session,|driver|{
            let resolved=driver.cdp.call("DOM.resolveNode",json!({"nodeId":owner})).map_err(derr)?;
            let object=resolved.pointer("/object/objectId").and_then(Value::as_str).ok_or_else(review)?;
            let checked=driver.cdp.call("Runtime.callFunctionOn",json!({"objectId":object,"functionDeclaration":r#"function(x,y,width,height){
                if(!this.isConnected || !(this instanceof HTMLIFrameElement) || this.closest('a[href],button:not([type="button"]),input[type="submit" i],input[type="image" i]')) return null;
                if(Math.abs(this.clientWidth-width)>.5 || Math.abs(this.clientHeight-height)>.5) return null;
                const rect=this.getBoundingClientRect(),style=getComputedStyle(this);
                if([style.paddingLeft,style.paddingTop,style.paddingRight,style.paddingBottom].some(value=>parseFloat(value)!==0) || Math.abs(rect.width-this.offsetWidth)>.5 || Math.abs(rect.height-this.offsetHeight)>.5) return null;
                for(let current=this,depth=0;current;current=current.parentElement){if(++depth>64)return null;const s=getComputedStyle(current);if(s.transform!=='none' || s.perspective!=='none' || s.translate!=='none' || s.rotate!=='none' || s.scale!=='none' || Number(s.zoom)!==1 || s.visibility!=='visible' || Number(s.opacity)===0)return null;}
                const px=rect.left+this.clientLeft+x,py=rect.top+this.clientTop+y;
                if(px<0 || py<0 || px>=innerWidth || py>=innerHeight || document.elementFromPoint(px,py)!==this)return null;
                return {x:px,y:py,width:innerWidth,height:innerHeight};
            }"#,"arguments":[{"value":x},{"value":y},{"value":numbers[2]},{"value":numbers[3]}],"returnByValue":true}));
            let released=driver.cdp.call("Runtime.releaseObject",json!({"objectId":object}));
            let checked=checked.map_err(derr)?;released.map_err(derr)?;
            if checked.get("exceptionDetails").is_some(){return Err(review());}
            let checked=checked.pointer("/result/value").ok_or_else(review)?;
            let values:Vec<_>=["x","y","width","height"].iter().filter_map(|key|checked[key].as_f64().filter(|v|v.is_finite())).collect();
            if values.len()!=4 || values[0]<0.0 || values[1]<0.0 || values[0]>=values[2] || values[1]>=values[3] {return Err(review());}
            Ok(CapturedPointerPoint { session: frame.parent_session.clone(), x:values[0],y:values[1],width:values[2],height:values[3],child_width:numbers[2],child_height:numbers[3] })
        })?;
        Ok(Some(point))
    }
    /// A parent-rendered viewport image, never an asserted full child-document capture.
    pub fn screenshot_captured_frame(&mut self) -> Result<String, DriverError> {
        self.validate_captured_frame(false)?;
        let frame = self
            .captured_scope
            .as_ref()
            .ok_or_else(review)?
            .frame
            .clone();
        let owner = self.parent_frame_owner(&frame)?;
        self.with_captured_parent_capture(|driver| {
            driver
                .cdp
                .call("DOM.scrollIntoViewIfNeeded", json!({"nodeId":owner}))
                .map_err(derr)?;
            driver.screenshot_node(owner, false, true)
        })
    }

    pub(super) fn screenshot_captured_field(
        &mut self,
        selector: &str,
        context: bool,
    ) -> Result<String, DriverError> {
        self.scroll_field_into_view(selector)?;
        let node = self.node_id(selector)?.ok_or_else(review)?;
        self.screenshot_captured_node(node, context, false)
    }

    fn screenshot_captured_node(
        &mut self,
        node: u64,
        context: bool,
        radio: bool,
    ) -> Result<String, DriverError> {
        let box_model = |driver: &mut Self| -> Result<[f64; 8], DriverError> {
            let response = driver
                .cdp
                .call("DOM.getBoxModel", json!({"nodeId":node}))
                .map_err(derr)?;
            let quad = response
                .pointer("/model/border")
                .and_then(Value::as_array)
                .ok_or_else(review)?;
            if quad.len() != 8 {
                return Err(review());
            }
            let mut values = [0.0; 8];
            for (dst, src) in values.iter_mut().zip(quad) {
                *dst = src.as_f64().filter(|v| v.is_finite()).ok_or_else(review)?;
            }
            Ok(values)
        };
        let quad = box_model(self)?;
        let x = (quad[0] + quad[2] + quad[4] + quad[6]) / 4.0;
        let y = (quad[1] + quad[3] + quad[5] + quad[7]) / 4.0;
        let point = self
            .captured_pointer_point(node, x, y)?
            .ok_or_else(review)?;
        let bounds = recorded_field_crop(quad, &point, x, y, context, radio)?;
        let image = self.with_captured_parent_capture(|driver| {
            driver.screenshot_bounds(&bounds, false, false)
        })?;
        if box_model(self)? != quad
            || self.captured_pointer_point(node, x, y)?.as_ref() != Some(&point)
        {
            return Err(DriverError(
                "The embedded field moved during capture; inspect it again".into(),
            ));
        }
        Ok(image)
    }

    fn with_captured_parent_capture(
        &mut self,
        capture: impl FnOnce(&mut Self) -> Result<String, DriverError>,
    ) -> Result<String, DriverError> {
        let parent_session = self
            .captured_scope
            .as_ref()
            .ok_or_else(review)?
            .frame
            .parent_session
            .clone();
        let image = self.with_capture_styles(|driver| {
            let parent_sheet = driver
                .captured_scope
                .as_mut()
                .ok_or_else(review)?
                .parent_sheet
                .take();
            let child_sheet = std::mem::replace(&mut driver.capture_sheet, parent_sheet);
            let result = catch_unwind(AssertUnwindSafe(|| {
                driver.with_protocol_session(&parent_session, capture)
            }));
            let parent_sheet = std::mem::replace(&mut driver.capture_sheet, child_sheet);
            driver
                .captured_scope
                .as_mut()
                .ok_or_else(review)?
                .parent_sheet = parent_sheet;
            match result {
                Ok(result) => result,
                Err(panic) => std::panic::resume_unwind(panic),
            }
        })?;
        self.validate_captured_frame(false)?;
        Ok(image)
    }
}

fn recorded_field_crop(
    quad: [f64; 8],
    point: &CapturedPointerPoint,
    x: f64,
    y: f64,
    context: bool,
    radio: bool,
) -> Result<[f64; 8], DriverError> {
    let left = quad
        .iter()
        .step_by(2)
        .copied()
        .fold(f64::INFINITY, f64::min);
    let right = quad
        .iter()
        .step_by(2)
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let top = quad
        .iter()
        .skip(1)
        .step_by(2)
        .copied()
        .fold(f64::INFINITY, f64::min);
    let bottom = quad
        .iter()
        .skip(1)
        .step_by(2)
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if left < 0.0
        || top < 0.0
        || right > point.child_width
        || bottom > point.child_height
        || right <= left
        || bottom <= top
    {
        return Err(DriverError(
            "The embedded field is not fully visible; review it manually".into(),
        ));
    }
    let (left, top, right, bottom) = if radio {
        // Include the group heading and adjacent option labels when the
        // selected radio is positioned far across the row.
        let width = point.child_width.min(720.0);
        let start = (right + 240.0 - width).clamp(0.0, point.child_width - width);
        (
            start,
            (top - 100.0).max(0.0),
            start + width,
            (bottom + 26.0).min(point.child_height),
        )
    } else if context {
        // Keep nearby labels, but never include pixels outside this frame.
        (
            (left - 180.0).max(0.0),
            (top - 24.0).max(0.0),
            (right + 18.0).min(point.child_width),
            (bottom + 18.0).min(point.child_height),
        )
    } else {
        (left, top, right, bottom)
    };
    let (dx, dy) = (point.x - x, point.y - y);
    let (left, top, right, bottom) = (left + dx, top + dy, right + dx, bottom + dy);
    Ok([left, top, right, top, right, bottom, left, bottom])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point() -> CapturedPointerPoint {
        CapturedPointerPoint {
            session: "parent".into(),
            x: 330.0,
            y: 420.0,
            width: 1200.0,
            height: 800.0,
            child_width: 300.0,
            child_height: 100.0,
        }
    }

    #[test]
    fn field_context_is_clipped_to_the_bound_child_viewport() {
        let quad = [20.0, 10.0, 140.0, 10.0, 140.0, 30.0, 20.0, 30.0];
        assert_eq!(
            recorded_field_crop(quad, &point(), 80.0, 20.0, true, false).unwrap(),
            [250.0, 400.0, 408.0, 400.0, 408.0, 448.0, 250.0, 448.0]
        );
        assert_eq!(
            recorded_field_crop(quad, &point(), 80.0, 20.0, false, false).unwrap(),
            [270.0, 410.0, 390.0, 410.0, 390.0, 430.0, 270.0, 430.0]
        );
    }

    #[test]
    fn partially_visible_or_empty_child_control_never_becomes_a_complete_capture() {
        for quad in [
            [-1.0, 10.0, 10.0, 10.0, 10.0, 20.0, -1.0, 20.0],
            [20.0, 90.0, 30.0, 90.0, 30.0, 101.0, 20.0, 101.0],
            [20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0],
        ] {
            assert!(recorded_field_crop(quad, &point(), 80.0, 20.0, true, false).is_err());
        }
    }
}
