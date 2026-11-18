//! Bounded readback of an existing native file selection.
use super::*;

#[derive(serde::Serialize, PartialEq, Eq)]
pub(super) struct FileExpectation {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

impl FileExpectation {
    pub(super) fn read(
        path: &std::path::Path,
        pinned_hash: Option<&str>,
    ) -> Result<Self, DriverError> {
        use std::io::Read;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| DriverError("File name cannot be represented in the browser".into()))?;
        let metadata = std::fs::metadata(path)
            .map_err(|e| DriverError(format!("Cannot read attachment: {e}")))?;
        let cap = 10 * 1024 * 1024;
        if !metadata.is_file() || metadata.len() > cap {
            return Err(DriverError(
                "Attachment is not a regular file of verifiable size".into(),
            ));
        }
        let file = std::fs::File::open(path)
            .map_err(|e| DriverError(format!("Cannot read attachment: {e}")))?;
        let opened = file
            .metadata()
            .map_err(|e| DriverError(format!("Cannot read attachment: {e}")))?;
        if !opened.is_file() || opened.len() != metadata.len() {
            return Err(DriverError("Attachment changed during verification".into()));
        }
        let mut bytes = Vec::new();
        file.take(cap + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| DriverError(format!("Cannot read attachment: {e}")))?;
        if bytes.len() as u64 != metadata.len() {
            return Err(DriverError("Attachment changed during verification".into()));
        }
        let sha256 = waypoint_domain::sha256_hex(&bytes);
        if pinned_hash.is_some_and(|hash| hash != sha256) {
            return Err(DriverError(
                "Staged attachment differs from its verified inventory".into(),
            ));
        }
        Ok(Self {
            name: name.to_owned(),
            size: metadata.len(),
            sha256,
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum FileSelectionReadback {
    NotRetained,
    Different,
    Unavailable,
    Verified { accept: String },
}

pub(super) struct FileInspection<'a> {
    pub input: &'a str,
    pub expected: &'a [FileExpectation],
    /// A supplied restriction can also require that the input has no form owner.
    pub original_form: Option<Option<&'a str>>,
}

impl CdpDriver {
    pub(super) fn file_inspection_context(&mut self) -> Result<u64, DriverError> {
        let tree = self
            .cdp
            .call("Page.getFrameTree", json!({}))
            .map_err(derr)?;
        let frame = tree
            .pointer("/frameTree/frame/id")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError("File inspection frame is unavailable".into()))?;
        let world = self
            .cdp
            .call(
                "Page.createIsolatedWorld",
                json!({"frameId":frame, "worldName":"Smart Jobseeker file verification"}),
            )
            .map_err(derr)?;
        world["executionContextId"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| DriverError("File verification world is unavailable".into()))
    }

    pub(super) fn file_review_object(
        &mut self,
        mut params: Value,
        context: Option<u64>,
    ) -> Result<String, DriverError> {
        if let Some(context) = context {
            params["executionContextId"] = json!(context);
        }
        let resolved = self.cdp.call("DOM.resolveNode", params).map_err(derr)?;
        resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| DriverError("Cannot inspect file field".into()))
    }

    pub(super) fn with_file_review_object<T>(
        &mut self,
        node: Value,
        context: Option<u64>,
        inspect: impl FnOnce(&mut Self, &str) -> Result<T, DriverError>,
    ) -> Result<T, DriverError> {
        let object = self.file_review_object(node, context)?;
        let result = inspect(self, &object);
        self.release_file_review_object(&object);
        result
    }

    pub(super) fn release_file_review_object(&mut self, object: &str) {
        let _ = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}));
    }

    pub(super) fn inspect_file_selections(
        &mut self,
        selections: &[FileInspection<'_>],
    ) -> Result<Vec<FileSelectionReadback>, DriverError> {
        let Some(first) = selections.first() else {
            return Ok(vec![]);
        };
        let specs = selections
            .iter()
            .map(
                |item| json!({"expected":item.expected,"requireForm":item.original_form.is_some()}),
            )
            .collect::<Vec<_>>();
        let mut arguments = vec![json!({"value":specs})];
        for item in selections {
            arguments.push(json!({"objectId":item.input}));
            arguments.push(
                item.original_form
                    .flatten()
                    .map_or_else(|| json!({"value":null}), |id| json!({"objectId":id})),
            );
        }
        let read = self.cdp.call("Runtime.callFunctionOn", json!({
            "objectId":first.input,"functionDeclaration":include_str!("file_selection_readback.js"),
            "arguments":arguments,
            "awaitPromise":true,"returnByValue":true
        })).map_err(derr)?;
        let inspected = if read.get("exceptionDetails").is_none() {
            read.pointer("/result/value")
                .cloned()
                .and_then(|value| serde_json::from_value::<Vec<FileSelectionReadback>>(value).ok())
        } else {
            None
        };
        inspected
            .filter(|items| items.len() == selections.len())
            .ok_or_else(|| DriverError("Cannot inspect the current file selections".into()))
    }
}
