//! Native frame identity and a separate bounded inline-inspection certificate.
//! Identity alone never discharges an embedded-document review barrier.

use super::*;
use serde::Serialize;
use std::collections::BTreeMap;

const MAX_FRAMES: usize = 16;
const MAX_DOM_NODES: usize = 20_000;
pub(crate) const FRAME_IDENTITY_REVIEW: &str =
    "Embedded document identity changed or could not be verified; inspect the page again";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FrameIdentity {
    parent: Option<String>,
    loader: String,
    url: String,
    origin: String,
    document_status: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct OwnerIdentity {
    backend: u64,
    document: Option<u64>,
    attributes: Vec<String>,
    ancestry: Vec<u64>,
    rendering: Option<OwnerRendering>,
    accessible_origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
struct OwnerRendering {
    access: Option<bool>,
    needs_review: bool,
}

/// Records successful document inspections, never merely discovered frames.
#[derive(Default)]
pub(crate) struct InlineFrameInspections {
    inspected: BTreeMap<u64, u64>,
    invalid: bool,
}

#[derive(Serialize)]
pub(crate) struct InlineFrameCompleteness {
    native_identity: String,
    inspected: BTreeMap<u64, u64>,
}

impl InlineFrameInspections {
    pub(crate) fn inspected(&mut self, owner: u64, document: u64) {
        if owner == 0 || document == 0 || self.inspected.insert(owner, document).is_some() {
            self.invalid = true;
        }
    }

    pub(crate) fn certify(
        &self,
        before: &FrameInspectionIdentity,
        after: &FrameInspectionIdentity,
    ) -> Option<InlineFrameCompleteness> {
        if self.invalid || before != after {
            return None;
        }
        let mut expected = BTreeMap::new();
        for (frame_id, owner) in &before.owners {
            match owner.rendering.as_ref()? {
                OwnerRendering {
                    access: None,
                    needs_review: false,
                } => (),
                OwnerRendering {
                    access: Some(true),
                    needs_review: false,
                } => {
                    if !before.document_source_verified(frame_id, 4) {
                        return None;
                    }
                    let document = owner.document?;
                    if expected.insert(owner.backend, document).is_some() {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        // A pending visible frame cannot be certified by an empty or partial
        // census. Opaque, uncertain and uninspected siblings remain barriers.
        if expected.is_empty() || expected != self.inspected {
            return None;
        }
        Some(InlineFrameCompleteness {
            native_identity: before.digest().ok()?,
            inspected: self.inspected.clone(),
        })
    }
}

impl InlineFrameCompleteness {
    pub(crate) fn digest(&self) -> Result<String, DriverError> {
        let bytes =
            serde_json::to_vec(&json!({"kind":"inline_frame_inspection_v1","evidence":self}))
                .map_err(|_| DriverError(FRAME_IDENTITY_REVIEW.into()))?;
        Ok(waypoint_domain::sha256_hex(&bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FrameInspectionIdentity {
    root_document: u64,
    frames: BTreeMap<String, FrameIdentity>,
    owners: BTreeMap<String, OwnerIdentity>,
}

fn identity_text(value: &Value, key: &str, limit: usize) -> Result<String, DriverError> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= limit)
        .map(str::to_owned)
        .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))
}

fn positive_id(value: &Value, key: &str) -> Result<u64, DriverError> {
    value[key]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))
}

fn frame_tree(tree: &Value) -> Result<BTreeMap<String, FrameIdentity>, DriverError> {
    let mut frames = BTreeMap::new();
    let mut pending = vec![(&tree["frameTree"], None, 0usize)];
    while let Some((node, parent, depth)) = pending.pop() {
        if frames.len() >= MAX_FRAMES || depth > 4 {
            return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
        }
        let frame = &node["frame"];
        let id = identity_text(frame, "id", 256)?;
        if frame.get("parentId").and_then(Value::as_str) != parent.as_deref() {
            return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
        }
        let url = identity_text(frame, "url", 8192)?;
        let inherited = url::Url::parse(&url).is_ok_and(|url| {
            url.scheme() == "about"
                && matches!(url.path(), "blank" | "srcdoc")
                && url.query().is_none()
        });
        let loader = if frame["loaderId"].as_str() == Some("") && inherited {
            String::new()
        } else {
            identity_text(frame, "loaderId", 256)?
        };
        let identity = FrameIdentity {
            parent,
            loader,
            url,
            origin: identity_text(frame, "securityOrigin", 8192)?,
            document_status: None,
        };
        if frames.insert(id.clone(), identity).is_some() {
            return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
        }
        if let Some(children) = node.get("childFrames") {
            let children = children
                .as_array()
                .filter(|children| children.len() < MAX_FRAMES)
                .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))?;
            pending.extend(
                children
                    .iter()
                    .map(|child| (child, Some(id.clone()), depth + 1)),
            );
        }
    }
    Ok(frames)
}

fn dom_owners(root: &Value) -> Result<BTreeMap<String, OwnerIdentity>, DriverError> {
    let mut owners = BTreeMap::new();
    let mut visited = 0;
    let mut pending = vec![(root, 0usize, Vec::<u64>::new())];
    while let Some((node, depth, ancestry)) = pending.pop() {
        visited += 1;
        if visited > MAX_DOM_NODES || depth > 128 {
            return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
        }
        let tag = node["nodeName"].as_str().unwrap_or("");
        if tag.eq_ignore_ascii_case("iframe") || tag.eq_ignore_ascii_case("frame") {
            if owners.len() >= MAX_FRAMES - 1 {
                return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
            }
            let frame = identity_text(node, "frameId", 256)?;
            let attributes = node["attributes"]
                .as_array()
                .filter(|a| a.len() <= 128 && a.len() % 2 == 0)
                .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))?;
            let attributes: Vec<String> = attributes
                .iter()
                .map(|a| {
                    a.as_str()
                        .filter(|a| a.len() <= 8192)
                        .map(str::to_owned)
                        .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))
                })
                .collect::<Result<_, _>>()?;
            let document = node
                .get("contentDocument")
                .map(|child| positive_id(child, "backendNodeId"))
                .transpose()?;
            let owner = OwnerIdentity {
                backend: positive_id(node, "backendNodeId")?,
                document,
                attributes,
                ancestry: ancestry.clone(),
                rendering: None,
                accessible_origin: None,
            };
            if owners.insert(frame, owner).is_some() {
                return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
            }
        }
        let mut child_ancestry = ancestry;
        if let Some(backend) = node["backendNodeId"].as_u64().filter(|id| *id > 0) {
            child_ancestry.push(backend);
        }
        for key in ["children", "shadowRoots"] {
            if let Some(children) = node.get(key) {
                let children = children
                    .as_array()
                    .filter(|children| children.len() <= MAX_DOM_NODES)
                    .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))?;
                if pending.len().saturating_add(children.len()) > MAX_DOM_NODES {
                    return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
                }
                pending.extend(
                    children
                        .iter()
                        .map(|child| (child, depth + 1, child_ancestry.clone())),
                );
            }
        }
        if let Some(document) = node.get("contentDocument") {
            pending.push((document, depth + 1, child_ancestry));
        }
    }
    Ok(owners)
}

impl FrameInspectionIdentity {
    fn observed_origin(&self, id: &str) -> Option<&str> {
        let frame = self.frames.get(id)?;
        let valid = |origin: &str| {
            url::Url::parse(origin).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.origin().ascii_serialization() == origin
            })
        };
        let observed = self
            .owners
            .get(id)
            .and_then(|owner| owner.accessible_origin.as_deref());
        if valid(&frame.origin) {
            if observed.is_some_and(|observed| observed != frame.origin) {
                return None;
            }
            return Some(&frame.origin);
        }
        let url = url::Url::parse(&frame.url).ok()?;
        if url.scheme() != "about"
            || !matches!(url.path(), "srcdoc" | "blank")
            || url.query().is_some()
            || !matches!(frame.origin.as_str(), "://" | "null")
        {
            return None;
        }
        let owner = self.owners.get(id)?;
        if owner.rendering.as_ref()?.access != Some(true) {
            return None;
        }
        observed.filter(|origin| valid(origin))
    }

    fn document_source_verified(&self, id: &str, remaining: usize) -> bool {
        let Some(frame) = self.frames.get(id) else {
            return false;
        };
        let Some(origin) = self.observed_origin(id) else {
            return false;
        };
        if let Some(parent_id) = &frame.parent {
            if remaining == 0
                || self.observed_origin(parent_id) != Some(origin)
                || !self.document_source_verified(parent_id, remaining - 1)
            {
                return false;
            }
        }
        if frame.url.starts_with("https://") || frame.url.starts_with("http://") {
            return matches!(frame.document_status, Some(200..=299));
        }
        let Some(owner) = self.owners.get(id) else {
            return false;
        };
        if frame.parent.is_none() || owner.document.is_none() {
            return false;
        }
        let Ok(url) = url::Url::parse(&frame.url) else {
            return false;
        };
        if url.scheme() != "about" || url.query().is_some() {
            return false;
        }
        let attribute = |name: &str| {
            owner
                .attributes
                .as_chunks::<2>()
                .0
                .iter()
                .find_map(|pair| (pair[0] == name).then_some(pair[1].as_str()))
        };
        match url.path() {
            "srcdoc" => attribute("srcdoc").is_some(),
            "blank" if attribute("srcdoc").is_none() => attribute("src").is_none_or(|src| {
                src.trim().is_empty()
                    || url::Url::parse(src.trim()).is_ok_and(|src| {
                        src.scheme() == "about" && src.path() == "blank" && src.query().is_none()
                    })
            }),
            _ => false,
        }
    }

    fn parse(root: &Value, tree: &Value) -> Result<Self, DriverError> {
        let frames = frame_tree(tree)?;
        let owners = dom_owners(root)?;
        if owners.len() + 1 != frames.len()
            || frames
                .iter()
                .any(|(id, frame)| frame.parent.is_some() != owners.contains_key(id))
        {
            // Opaque descendants absent from this session are unknown, not a
            // complete owner census. Do not infer their identities.
            return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
        }
        Ok(Self {
            root_document: positive_id(root, "backendNodeId")?,
            frames,
            owners,
        })
    }

    pub(crate) fn digest(&self) -> Result<String, DriverError> {
        let bytes =
            serde_json::to_vec(self).map_err(|_| DriverError(FRAME_IDENTITY_REVIEW.into()))?;
        Ok(waypoint_domain::sha256_hex(&bytes))
    }
}

impl CdpDriver {
    pub(crate) fn frame_inspection_identity(
        &mut self,
        root: &Value,
    ) -> Result<FrameInspectionIdentity, DriverError> {
        let tree = self
            .cdp
            .call("Page.getFrameTree", json!({}))
            .map_err(derr)?;
        let mut identity = FrameInspectionIdentity::parse(root, &tree)?;
        for (id, frame) in &mut identity.frames {
            frame.document_status = self
                .cdp
                .events
                .iter()
                .rev()
                .find_map(|(name, event)| {
                    (name == "Network.responseReceived"
                        && event.pointer("/params/frameId").and_then(Value::as_str) == Some(id)
                        && event.pointer("/params/loaderId").and_then(Value::as_str)
                            == Some(&frame.loader))
                    .then(|| event.pointer("/params/status").and_then(Value::as_u64))
                    .flatten()
                })
                .or_else(|| {
                    self.document_status
                        .as_ref()
                        .filter(|(saved_frame, saved_loader, _)| {
                            saved_frame == id && saved_loader == &frame.loader
                        })
                        .map(|(_, _, status)| *status)
                });
        }
        let started = Instant::now();
        for (frame, owner) in &mut identity.owners {
            if started.elapsed() > Duration::from_secs(8) {
                return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
            }
            // Experimental on some engines: failure preserves review. A frame
            // ID is never substituted for a Target ID or a native owner node.
            let observed = self
                .cdp
                .call("DOM.getFrameOwner", json!({"frameId":frame}))
                .map_err(derr)?;
            if observed["backendNodeId"].as_u64() != Some(owner.backend) {
                return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
            }
            let node = positive_id(&observed, "nodeId")?;
            let (rendering, origin) = self.frame_owner_rendering(node)?;
            owner.rendering = Some(rendering);
            owner.accessible_origin = origin;
        }
        Ok(identity)
    }

    fn frame_owner_rendering(
        &mut self,
        node: u64,
    ) -> Result<(OwnerRendering, Option<String>), DriverError> {
        let resolved = self
            .cdp
            .call("DOM.resolveNode", json!({"nodeId":node}))
            .map_err(derr)?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))?
            .to_owned();
        let checked = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,
            "functionDeclaration":r#"function() {
                if (!this.isConnected) return null;
                const box = this.getBoundingClientRect(), style = getComputedStyle(this);
                let needs_review = false, accessible_origin=null, left=box.left, right=box.right, top=box.top, bottom=box.bottom;
                for (let n=this, depth=0; n; n=n.parentElement) {
                    if (++depth>64) return null;
                    const s=getComputedStyle(n);
                    if (Number(s.opacity)===0) needs_review=true;
                    if (n!==this) {
                        const r=n.getBoundingClientRect();
                        if (['hidden','clip'].includes(s.overflowX)) {left=Math.max(left,r.left+n.clientLeft);right=Math.min(right,r.left+n.clientLeft+n.clientWidth);}
                        if (['hidden','clip'].includes(s.overflowY)) {top=Math.max(top,r.top+n.clientTop);bottom=Math.min(bottom,r.top+n.clientTop+n.clientHeight);}
                    }
                }
                if (!this.getClientRects().length || style.visibility==='hidden' || style.visibility==='collapse' || box.width<=0 || box.height<=0)
                    return {access:null,needs_review,accessible_origin};
                if (right<=left || bottom<=top || left>box.left+0.5 || right<box.right-0.5 || top>box.top+0.5 || bottom<box.bottom-0.5) needs_review=true;
                let access=false;
                try {access=this.contentDocument!==null;if(access){const origin=this.contentWindow.origin;if(typeof origin!=='string' || origin.length>8192)return null;accessible_origin=origin;}} catch (_) {}
                // Owner availability is not full-field visibility. A tall or
                // wide owner can be available while its full center is outside
                // the viewport. Select a point in the common visible rectangle
                // of every accessible ancestor viewport, then retain the exact
                // owner hit tests below. Individual fields still need their
                // own complete capture, hit test and fresh native readback.
                let visibleLeft=Math.max(left,0), visibleRight=Math.min(right,innerWidth);
                let visibleTop=Math.max(top,0), visibleBottom=Math.min(bottom,innerHeight);
                let projection=window, offsetX=0, offsetY=0, reachedTop=false;
                for(let depth=0;depth<12;depth++) {
                    if(visibleRight-visibleLeft<1 || visibleBottom-visibleTop<1) return {access,needs_review:true,accessible_origin};
                    if(projection===projection.top) {reachedTop=true;break;}
                    const owner=projection.frameElement;
                    if(!owner || projection.parent.getComputedStyle(owner).transform!=='none') return {access,needs_review:true,accessible_origin};
                    const bounds=owner.getBoundingClientRect();
                    if(Math.abs(bounds.width-owner.offsetWidth)>.5 || Math.abs(bounds.height-owner.offsetHeight)>.5
                        || Math.abs(owner.clientWidth-projection.innerWidth)>.5 || Math.abs(owner.clientHeight-projection.innerHeight)>.5)
                        return {access,needs_review:true,accessible_origin};
                    const dx=bounds.left+owner.clientLeft, dy=bounds.top+owner.clientTop;
                    if(!Number.isFinite(dx) || !Number.isFinite(dy)) return null;
                    visibleLeft=Math.max(visibleLeft+dx,0);visibleRight=Math.min(visibleRight+dx,projection.parent.innerWidth);
                    visibleTop=Math.max(visibleTop+dy,0);visibleBottom=Math.min(visibleBottom+dy,projection.parent.innerHeight);
                    offsetX+=dx;offsetY+=dy;projection=projection.parent;
                }
                if(!reachedTop) return {access,needs_review:true,accessible_origin};
                const px=Math.min((visibleLeft+visibleRight)/2,projection.innerWidth-1), py=Math.min((visibleTop+visibleBottom)/2,projection.innerHeight-1);
                if(px<visibleLeft || px>=visibleRight || py<visibleTop || py>=visibleBottom) return {access,needs_review:true,accessible_origin};
                let current=this, view=window, x=px-offsetX, y=py-offsetY;
                for (let depth = 0; depth < 12; depth++) {
                    if (x<0 || y<0 || x>=view.innerWidth || y>=view.innerHeight) return {access,needs_review:true,accessible_origin};
                    const root = current.getRootNode();
                    const hit = typeof root.elementFromPoint === 'function' ? root.elementFromPoint(x,y) : null;
                    if (!hit || (hit!==current && !current.contains(hit))) return {access,needs_review:true,accessible_origin};
                    if (root.nodeType===11 && root.host) {current=root.host;continue;}
                    if (view===view.top) return {access,needs_review,accessible_origin};
                    const owner=view.frameElement;
                    if (!owner || view.parent.getComputedStyle(owner).transform!=='none') return {access,needs_review:true,accessible_origin};
                    const bounds=owner.getBoundingClientRect();
                    x+=bounds.left+owner.clientLeft;y+=bounds.top+owner.clientTop;
                    current=owner;view=view.parent;
                }
                return {access,needs_review:true,accessible_origin};
            }"#, "returnByValue":true}));
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
        let checked = checked.map_err(derr)?;
        released.map_err(derr)?;
        let value = checked
            .pointer("/result/value")
            .filter(|value| value.get("access").is_some())
            .ok_or_else(|| DriverError(FRAME_IDENTITY_REVIEW.into()))?;
        if checked.get("exceptionDetails").is_some() {
            return Err(DriverError(FRAME_IDENTITY_REVIEW.into()));
        }
        let origin = value
            .get("accessible_origin")
            .and_then(Value::as_str)
            .filter(|origin| origin.len() <= 8192)
            .map(str::to_owned);
        let rendering = serde_json::from_value(value.clone())
            .map_err(|_| DriverError(FRAME_IDENTITY_REVIEW.into()))?;
        Ok((rendering, origin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Value, Value) {
        (
            json!({"nodeName":"#document","backendNodeId":1,"children":[
                {"nodeName":"IFRAME","nodeId":2,"backendNodeId":3,"frameId":"child","attributes":["src","https://example.invalid/form"],"contentDocument":{"nodeName":"#document","backendNodeId":4}}
            ]}),
            json!({"frameTree":{"frame":{"id":"main","loaderId":"load-a","url":"https://example.invalid/","securityOrigin":"https://example.invalid"},"childFrames":[{"frame":{"id":"child","parentId":"main","loaderId":"load-b","url":"https://example.invalid/form","securityOrigin":"https://example.invalid"}}]}}),
        )
    }

    fn observed_fixture() -> FrameInspectionIdentity {
        let (dom, tree) = fixture();
        let mut observed = FrameInspectionIdentity::parse(&dom, &tree).unwrap();
        observed.frames.get_mut("child").unwrap().document_status = Some(200);
        observed.frames.get_mut("main").unwrap().document_status = Some(200);
        observed.owners.get_mut("child").unwrap().rendering = Some(OwnerRendering {
            access: Some(true),
            needs_review: false,
        });
        observed
    }

    #[test]
    fn inherited_empty_loader_is_explicit_and_still_needs_a_native_document() {
        let (mut dom, mut tree) = fixture();
        dom["children"][0]["attributes"] = json!(["src", "about:blank"]);
        tree["frameTree"]["childFrames"][0]["frame"]["url"] = json!("about:blank");
        tree["frameTree"]["childFrames"][0]["frame"]["loaderId"] = json!("");
        let mut observed = FrameInspectionIdentity::parse(&dom, &tree).unwrap();
        observed.frames.get_mut("main").unwrap().document_status = Some(200);
        let owner = observed.owners.get_mut("child").unwrap();
        owner.rendering = Some(OwnerRendering {
            access: Some(true),
            needs_review: false,
        });
        owner.accessible_origin = Some("https://example.invalid".into());
        let mut ledger = InlineFrameInspections::default();
        ledger.inspected(3, 4);
        assert!(ledger.certify(&observed, &observed).is_some());
        observed.owners.get_mut("child").unwrap().document = None;
        assert!(ledger.certify(&observed, &observed).is_none());
        tree["frameTree"]["childFrames"][0]["frame"]["loaderId"] = Value::Null;
        assert!(FrameInspectionIdentity::parse(&dom, &tree).is_err());
    }
    #[test]
    fn inline_inherited_documents_require_verified_parent_source_and_native_origin() {
        let mut observed = observed_fixture();
        observed.frames.get_mut("main").unwrap().document_status = Some(200);
        let mut ledger = InlineFrameInspections::default();
        ledger.inspected(3, 4);
        for (url, attrs) in [
            ("about:srcdoc", vec!["srcdoc", "<input>"]),
            ("about:blank", vec!["src", "about:blank"]),
        ] {
            let mut inherited = observed.clone();
            let child = inherited.frames.get_mut("child").unwrap();
            child.url = url.into();
            child.origin = "://".into();
            inherited.owners.get_mut("child").unwrap().accessible_origin =
                Some("https://example.invalid".into());
            child.document_status = None;
            inherited.owners.get_mut("child").unwrap().attributes =
                attrs.into_iter().map(str::to_owned).collect();
            assert!(ledger.certify(&inherited, &inherited).is_some());
            inherited.frames.get_mut("main").unwrap().document_status = None;
            assert!(ledger.certify(&inherited, &inherited).is_none());
            inherited.frames.get_mut("main").unwrap().document_status = Some(200);
            inherited.owners.get_mut("child").unwrap().accessible_origin = None;
            assert!(ledger.certify(&inherited, &inherited).is_none());
            inherited.owners.get_mut("child").unwrap().accessible_origin =
                Some("https://other.invalid".into());
            assert!(ledger.certify(&inherited, &inherited).is_none());
        }
        for (url, attrs) in [
            ("about:srcdoc", vec![]),
            ("about:blank", vec!["src", "https://other.invalid"]),
            ("data:text/html,<input>", vec![]),
        ] {
            let mut unknown = observed.clone();
            unknown.frames.get_mut("child").unwrap().url = url.into();
            unknown.owners.get_mut("child").unwrap().attributes =
                attrs.into_iter().map(str::to_owned).collect();
            assert!(ledger.certify(&unknown, &unknown).is_none());
        }
    }
    #[test]
    fn inline_certificate_requires_exact_successful_document_inspections() {
        let before = observed_fixture();
        let mut ledger = InlineFrameInspections::default();
        assert!(ledger.certify(&before, &before).is_none());
        ledger.inspected(3, 4);
        let complete = ledger.certify(&before, &before).unwrap();
        assert_ne!(complete.digest().unwrap(), before.digest().unwrap());
        let mut wrong = InlineFrameInspections::default();
        wrong.inspected(3, 5);
        assert!(wrong.certify(&before, &before).is_none());
        ledger.inspected(3, 4);
        assert!(ledger.certify(&before, &before).is_none());
    }

    #[test]
    fn inline_certificate_never_promotes_opaque_unknown_or_changed_scopes() {
        let before = observed_fixture();
        let mut ledger = InlineFrameInspections::default();
        ledger.inspected(3, 4);
        let mut changed = before.clone();
        changed.frames.get_mut("child").unwrap().loader = "next-loader".into();
        assert!(ledger.certify(&before, &changed).is_none());
        for rendering in [
            None,
            Some(OwnerRendering {
                access: Some(false),
                needs_review: false,
            }),
            Some(OwnerRendering {
                access: Some(true),
                needs_review: true,
            }),
            Some(OwnerRendering {
                access: None,
                needs_review: true,
            }),
        ] {
            let mut unknown = before.clone();
            unknown.owners.get_mut("child").unwrap().rendering = rendering;
            assert!(ledger.certify(&unknown, &unknown).is_none());
        }
        let mut missing_document = before.clone();
        missing_document.owners.get_mut("child").unwrap().document = None;
        assert!(ledger
            .certify(&missing_document, &missing_document)
            .is_none());
        for status in [None, Some(0), Some(404), Some(500)] {
            let mut failed = before.clone();
            failed.frames.get_mut("child").unwrap().document_status = status;
            assert!(ledger.certify(&failed, &failed).is_none());
        }
        let mut changed_origin = before.clone();
        changed_origin.frames.get_mut("child").unwrap().origin = "https://other.invalid".into();
        assert!(ledger.certify(&changed_origin, &changed_origin).is_none());
    }

    #[test]
    fn inline_certificate_accounts_for_every_sibling_and_hidden_owner() {
        let mut before = observed_fixture();
        let mut sibling = before.owners["child"].clone();
        sibling.backend = 5;
        sibling.document = Some(6);
        before.owners.insert("sibling".into(), sibling);
        before
            .frames
            .insert("sibling".into(), before.frames["child"].clone());
        let mut ledger = InlineFrameInspections::default();
        ledger.inspected(3, 4);
        assert!(ledger.certify(&before, &before).is_none());
        before.owners.get_mut("sibling").unwrap().rendering = Some(OwnerRendering {
            access: None,
            needs_review: false,
        });
        assert!(ledger.certify(&before, &before).is_some());
        let mut changed = before.clone();
        changed
            .owners
            .get_mut("sibling")
            .unwrap()
            .rendering
            .as_mut()
            .unwrap()
            .access = Some(true);
        assert!(ledger.certify(&before, &changed).is_none());
        ledger.inspected(99, 100);
        assert!(ledger.certify(&before, &before).is_none());
    }

    #[test]
    fn frame_identity_binds_loader_owner_document_and_hidden_attributes() {
        let (dom, tree) = fixture();
        let original = FrameInspectionIdentity::parse(&dom, &tree).unwrap();
        let mut frontend_handle = dom.clone();
        frontend_handle["children"][0]["nodeId"] = json!(99);
        assert_eq!(
            original,
            FrameInspectionIdentity::parse(&frontend_handle, &tree).unwrap()
        );
        for pointer in [
            "/children/0/backendNodeId",
            "/children/0/contentDocument/backendNodeId",
        ] {
            let mut changed = dom.clone();
            *changed.pointer_mut(pointer).unwrap() = json!(99);
            assert_ne!(
                original,
                FrameInspectionIdentity::parse(&changed, &tree).unwrap()
            );
        }
        let mut hidden = dom.clone();
        hidden["children"][0]["attributes"] = json!(["hidden", ""]);
        assert_ne!(
            original.digest().unwrap(),
            FrameInspectionIdentity::parse(&hidden, &tree)
                .unwrap()
                .digest()
                .unwrap()
        );
        let mut navigated = tree.clone();
        navigated["frameTree"]["childFrames"][0]["frame"]["loaderId"] = json!("next-loader");
        assert_ne!(
            original,
            FrameInspectionIdentity::parse(&dom, &navigated).unwrap()
        );
        let mut moved = dom.clone();
        moved["children"] =
            json!([{"nodeName":"DIV","backendNodeId":90,"children":[dom["children"][0].clone()]}]);
        assert_ne!(
            original,
            FrameInspectionIdentity::parse(&moved, &tree).unwrap()
        );
    }

    #[test]
    fn frame_identity_refuses_missing_duplicate_opaque_and_mismatched_scopes() {
        let (dom, tree) = fixture();
        for pointer in [
            "/frameTree/frame/id",
            "/frameTree/childFrames/0/frame/loaderId",
            "/frameTree/childFrames/0/frame/parentId",
        ] {
            let mut changed = tree.clone();
            *changed.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(FrameInspectionIdentity::parse(&dom, &changed).is_err());
        }
        let mut duplicate = dom.clone();
        duplicate["children"]
            .as_array_mut()
            .unwrap()
            .push(dom["children"][0].clone());
        assert!(FrameInspectionIdentity::parse(&duplicate, &tree).is_err());
        let mut absent = dom.clone();
        absent["children"] = json!([]);
        assert!(FrameInspectionIdentity::parse(&absent, &tree).is_err());
        let mut opaque = tree.clone();
        let nested = json!({"frame":{"id":"opaque-descendant","parentId":"child","loaderId":"load-c","url":"https://other.invalid/","securityOrigin":"https://other.invalid"}});
        opaque["frameTree"]["childFrames"][0]["childFrames"] = json!([nested]);
        assert!(FrameInspectionIdentity::parse(&dom, &opaque).is_err());
    }

    #[test]
    fn frame_identity_bounds_graph_and_dom_work() {
        let (mut dom, tree) = fixture();
        dom["children"] = json!((0..MAX_DOM_NODES)
            .map(|_| json!({"nodeName":"SPAN"}))
            .collect::<Vec<_>>());
        assert!(dom_owners(&dom).is_err());
        let (_, mut too_many) = fixture();
        too_many["frameTree"]["childFrames"] = json!((0..MAX_FRAMES).map(|i| json!({"frame":{"id":format!("child-{i}"),"parentId":"main","loaderId":"load","url":"https://example.invalid/","securityOrigin":"https://example.invalid"}})).collect::<Vec<_>>());
        assert!(frame_tree(&too_many).is_err());
        let mut duplicate = tree.clone();
        duplicate["frameTree"]["childFrames"][0]["frame"]["id"] = json!("main");
        assert!(frame_tree(&duplicate).is_err());
    }
}
