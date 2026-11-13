//! Native frame identity only. This does not certify inspection completeness or
//! discharge any embedded-document review barrier.

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct OwnerIdentity {
    backend: u64,
    document: Option<u64>,
    attributes: Vec<String>,
    ancestry: Vec<u64>,
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
        let identity = FrameIdentity {
            parent,
            loader: identity_text(frame, "loaderId", 256)?,
            url: identity_text(frame, "url", 8192)?,
            origin: identity_text(frame, "securityOrigin", 8192)?,
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
        let identity = FrameInspectionIdentity::parse(root, &tree)?;
        let started = Instant::now();
        for (frame, owner) in &identity.owners {
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
        }
        Ok(identity)
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
