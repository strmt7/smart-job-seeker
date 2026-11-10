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
                if let (Some(node), Some(backend), Some(target)) = (
                    node["nodeId"].as_u64(),
                    node["backendNodeId"].as_u64(),
                    node["frameId"].as_str(),
                ) {
                    owners.push((node, backend, target.to_owned()));
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
        for (node, owner, target) in owners {
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
                label: matches[0]["title"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("Embedded form")
                    .chars()
                    .take(200)
                    .collect(),
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
        let local=self.cdp.call("Runtime.callFunctionOn",json!({"objectId":object,"functionDeclaration":"function(){const r=this.getBoundingClientRect();return this.isConnected?{x:(r.left+r.right)/2,y:(r.top+r.bottom)/2,width:innerWidth,height:innerHeight}:null;}","returnByValue":true}));
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
            Ok(CapturedPointerPoint { session: frame.parent_session.clone(), x:values[0],y:values[1],width:values[2],height:values[3] })
        })?;
        Ok(Some(point))
    }
    /// A parent-rendered viewport image, never an asserted full child-document capture.
    pub fn screenshot_captured_frame(&mut self) -> Result<String, DriverError> {
        self.validate_captured_frame(false)?;

        let scope = self.captured_scope.as_ref().ok_or_else(review)?;
        let frame = scope.frame.clone();
        let owner = self.parent_frame_owner(&frame)?;
        let image = self.with_capture_styles(|driver| {
            let parent_sheet = driver
                .captured_scope
                .as_mut()
                .ok_or_else(review)?
                .parent_sheet
                .take();
            let child_sheet = std::mem::replace(&mut driver.capture_sheet, parent_sheet);
            let result = catch_unwind(AssertUnwindSafe(|| {
                driver.with_protocol_session(&frame.parent_session, |driver| {
                    driver
                        .cdp
                        .call("DOM.scrollIntoViewIfNeeded", json!({"nodeId":owner}))
                        .map_err(derr)?;
                    driver.screenshot_node(owner, false, true)
                })
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
