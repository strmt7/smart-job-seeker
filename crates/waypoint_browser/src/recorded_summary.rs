//! A read-only summary census in the same owned document as the review image.
use super::*;

const SUMMARY_INSPECTOR: &str = include_str!("recorded_summary.js");
const SUMMARY_VIEWPORT: &str = include_str!("recorded_summary_viewport.js");

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryKey {
    pub sections: Vec<String>,
    pub label: String,
}

impl SummaryKey {
    pub fn from_field(field: &FormField) -> Self {
        // The form inspector flattens unique section names into display labels.
        // Recover the leaf using only its recorded, ordered section prefixes.
        let mut label = field.label.as_str();
        let mut seen = std::collections::HashSet::new();
        for section in &field.context.sections {
            if seen.insert(section.label.to_ascii_lowercase()) {
                let candidate = if section.inferred {
                    label.strip_prefix("Possible section: ")
                } else {
                    Some(label)
                };
                if let Some(leaf) = candidate
                    .and_then(|part| part.strip_prefix(&section.label))
                    .and_then(|part| part.strip_prefix(" — "))
                {
                    label = leaf;
                }
            }
        }
        Self {
            sections: field
                .context
                .sections
                .iter()
                .map(|section| section.label.clone())
                .collect(),
            label: label.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryAnswer {
    pub key: SummaryKey,
    pub value: String,
}

#[derive(PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct NativeRow {
    answer: SummaryAnswer,
    bounds: Option<Vec<NativeTextBounds>>,
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct NativeTextBounds {
    /// Layout rectangles, outer headings first, then the field label and value.
    document: [f64; 4],
    in_image: bool,
    page_clip: bool,
}

#[derive(PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct NativeSummary {
    rows: Vec<NativeRow>,
    incomplete: bool,
    viewport: [f64; 5],
}

pub struct RecordedSummaryCapture {
    document: RecordedDocument,
    inline: Option<InlineRecordedScope>,
    root: (u64, u64),
    native: NativeSummary,
    digest: String,
}

/// A previously verified file input in the owned rehearsal.
pub struct RecordedAttachmentWitness {
    document: RecordedDocument,
    inline_document: Option<u64>,
    control: u64,
    form_owner: Option<u64>,
    key: SummaryKey,
    prepared: FileExpectation,
}

impl RecordedAttachmentWitness {
    pub fn matches_prepared(&self, key: &SummaryKey, filename: &str, sha256: &str) -> bool {
        self.key == *key && self.prepared.name == filename && self.prepared.sha256 == sha256
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecordedAttachmentVerification {
    Retained { sha256: String, size: u64 },
    NotRetained,
    Different,
    Unavailable { reason: String },
}

impl RecordedSummaryCapture {
    pub fn answers(&self) -> impl Iterator<Item = (&SummaryAnswer, bool)> {
        self.native.rows.iter().map(|row| {
            let in_image = row.bounds.as_ref().is_some_and(|bounds| {
                bounds.len() == row.answer.key.sections.len() + 2
                    && bounds.iter().all(|part| part.in_image)
            });
            (&row.answer, in_image)
        })
    }

    pub fn incomplete(&self) -> bool {
        self.native.incomplete
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Clone, Copy, serde::Serialize)]
pub struct SummaryClip {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

pub struct RecordedSummaryRegion {
    pub clip: SummaryClip,
    pub image: String,
    pub mode: &'static str,
    pub viewport: Option<[f64; 5]>,
}

impl SummaryClip {
    fn around(mut rectangles: impl Iterator<Item = [f64; 4]>, page: (f64, f64)) -> Option<Self> {
        let first = rectangles.next()?;
        let (mut left, mut top, mut right, mut bottom) =
            (first[0], first[1], first[0] + first[2], first[1] + first[3]);
        for [x, y, width, height] in std::iter::once(first).chain(rectangles) {
            if ![x, y, width, height].iter().all(|value| value.is_finite())
                || x < 0.
                || y < 0.
                || width <= 0.
                || height <= 0.
                || x + width > page.0
                || y + height > page.1
            {
                return None;
            }
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + width);
            bottom = bottom.max(y + height);
        }
        let x = (left - 12.).floor().max(0.);
        let y = (top - 8.).floor().max(0.);
        let width = (right + 12.).ceil().min(page.0) - x;
        let height = (bottom + 8.).ceil().min(page.1) - y;
        // Fit the local vision input with enough detail to read label characters.
        (width <= 1280. && height <= 540. && x + width <= 4096.).then_some(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn quad(self) -> [f64; 8] {
        let (left, top) = (self.x, self.y);
        let (right, bottom) = (left + self.width, top + self.height);
        [left, top, right, top, right, bottom, left, bottom]
    }
}

fn row_parts(row: &NativeRow) -> Option<impl ExactSizeIterator<Item = &[NativeTextBounds]>> {
    let bounds = row.bounds.as_ref()?;
    let heading_count = row.answer.key.sections.len();
    if heading_count > 8 || bounds.len() != heading_count + 2 {
        return None;
    }
    // Connecting a heading to a later row in one crop can include other rows
    // or make the heading look like the label. Read the native parts independently.
    Some((0..heading_count + 1).map(move |index| {
        if index < heading_count {
            &bounds[index..index + 1]
        } else {
            &bounds[index..]
        }
    }))
}

fn plan_clips(row: &NativeRow, page: (f64, f64)) -> Option<Vec<SummaryClip>> {
    row_parts(row)?
        .map(|parts| {
            if !parts.iter().all(|bounds| bounds.page_clip) {
                return None;
            }
            SummaryClip::around(parts.iter().map(|bounds| bounds.document), page)
        })
        .collect()
}

fn summary_changed() -> DriverError {
    DriverError("The recorded review summary changed during inspection".into())
}

fn require_summary_replay(driver: &mut CdpDriver) -> Result<(), DriverError> {
    let replay = driver.rehearsal_replay_report()?;
    if replay["mode"] != "offline_response_replay"
        || [
            "blocked",
            "cached_non_read_replies",
            "unreplayed_transports",
            "unmanaged_child_sessions",
        ]
        .iter()
        .any(|key| replay[*key].as_u64() != Some(0))
        || replay["unsupported_targets"]
            .as_object()
            .is_none_or(|targets| !targets.is_empty())
        || driver.observed_non_read_requests()? != 0
    {
        return Err(DriverError(
            "The replay contains unresolved activity before summary verification".into(),
        ));
    }
    Ok(())
}

impl CdpDriver {
    /// Preserve the exact file control and owner for a later read-only check.
    pub fn capture_recorded_attachment_witness(
        &mut self,
        capture: &RecordedFieldCapture,
        prepared_sha256: &str,
    ) -> Result<RecordedAttachmentWitness, DriverError> {
        self.require_recorded_frames()?;
        let BrowserCommand::UploadFile {
            file_name,
            selector,
        } = &capture.command
        else {
            return Err(DriverError(
                "The recorded control does not hold an attachment".into(),
            ));
        };
        let binding = capture.binding.as_ref().ok_or_else(summary_changed)?;
        self.verify_recorded_field(capture)?;
        let (path, admitted_sha256) = self.recorded_attachment(file_name)?;
        if admitted_sha256 != prepared_sha256 {
            return Err(DriverError(
                "The prepared attachment differs from the recorded file".into(),
            ));
        }
        let prepared = FileExpectation::read(&path, Some(prepared_sha256))?;
        let inline = self.recorded_inline_control_scope(selector)?;
        let context =
            self.file_inspection_context(inline.as_ref().map(InlineRecordedScope::frame))?;
        let form_owner = self.with_file_review_object(
            json!({"backendNodeId":binding.backend}),
            Some(context),
            |driver, object| driver.recorded_file_form_owner(object),
        )?;
        if self.recorded_document()? != capture.document {
            return Err(summary_changed());
        }
        self.stop_if_non_read_request_observed(capture.network_before)?;
        Ok(RecordedAttachmentWitness {
            document: capture.document.clone(),
            inline_document: inline.map(|scope| scope.document),
            control: binding.backend,
            form_owner,
            key: SummaryKey::from_field(&binding.field),
            prepared,
        })
    }

    fn recorded_file_form_owner(&mut self, input: &str) -> Result<Option<u64>, DriverError> {
        let owner = self.cdp.call("Runtime.callFunctionOn", json!({
            "objectId":input,
            "functionDeclaration":"function() { if (this.isConnected && this.ownerDocument === document && this instanceof HTMLInputElement && this.type === 'file') return this.form; }"
        })).map_err(derr)?;
        let owner = &owner["result"];
        if owner["subtype"] == "null" {
            return Ok(None);
        }
        let object = owner["objectId"].as_str().ok_or_else(summary_changed)?;
        let node = self
            .cdp
            .call("DOM.describeNode", json!({"objectId":object}));
        self.release_file_review_object(object);
        let node = &node.map_err(derr)?["node"];
        if node["nodeName"] != "FORM" {
            return Err(summary_changed());
        }
        node["backendNodeId"]
            .as_u64()
            .map(Some)
            .ok_or_else(summary_changed)
    }

    /// Hash the original file inputs and recheck the set together after hashing.
    pub fn verify_recorded_summary_attachments(
        &mut self,
        summary: &RecordedSummaryCapture,
        witnesses: &[&RecordedAttachmentWitness],
    ) -> Result<Vec<RecordedAttachmentVerification>, DriverError> {
        self.validate_captured_frame(false)?;
        require_summary_replay(self)?;
        if self.recorded_document()? != summary.document {
            return Err(summary_changed());
        }
        if let Some(inline) = &summary.inline {
            self.recorded_inline_root(inline)?;
        }
        let context =
            self.file_inspection_context(summary.inline.as_ref().map(InlineRecordedScope::frame))?;
        let mut resolved = Vec::new();
        let mut outcomes = Vec::new();
        for (index, witness) in witnesses.iter().enumerate() {
            let objects = (|| -> Result<(String, Option<String>), DriverError> {
                if witness.document != summary.document
                    || witness.inline_document
                        != summary.inline.as_ref().map(|scope| scope.document)
                {
                    return Err(summary_changed());
                }
                let (path, admitted_sha256) = self.recorded_attachment(&witness.prepared.name)?;
                if admitted_sha256 != witness.prepared.sha256
                    || FileExpectation::read(&path, Some(&admitted_sha256))? != witness.prepared
                {
                    return Err(DriverError("The prepared file changed after entry".into()));
                }
                let input = self
                    .file_review_object(json!({"backendNodeId":witness.control}), Some(context))?;
                let owner = witness
                    .form_owner
                    .map(|owner| {
                        self.file_review_object(json!({"backendNodeId":owner}), Some(context))
                    })
                    .transpose();
                match owner {
                    Ok(owner) => Ok((input, owner)),
                    Err(error) => {
                        self.release_file_review_object(&input);
                        Err(error)
                    }
                }
            })();
            match objects {
                Ok((input, owner)) => {
                    resolved.push((index, input, owner));
                    outcomes.push(RecordedAttachmentVerification::Unavailable {
                        reason: "The retained selection could not be checked".into(),
                    });
                }
                Err(error) => outcomes.push(RecordedAttachmentVerification::Unavailable {
                    reason: error.to_string(),
                }),
            }
        }
        let selections = resolved
            .iter()
            .map(|(index, input, owner)| FileInspection {
                input,
                expected: std::slice::from_ref(&witnesses[*index].prepared),
                original_form: Some(owner.as_deref()),
            })
            .collect::<Vec<_>>();
        let read = self.inspect_file_selections(&selections);
        for (_, input, owner) in &resolved {
            self.release_file_review_object(input);
            if let Some(owner) = owner {
                self.release_file_review_object(owner);
            }
        }
        for ((index, _, _), checked) in resolved.into_iter().zip(read?) {
            let prepared = &witnesses[index].prepared;
            outcomes[index] = match checked {
                FileSelectionReadback::NotRetained => RecordedAttachmentVerification::NotRetained,
                FileSelectionReadback::Different => RecordedAttachmentVerification::Different,
                FileSelectionReadback::Unavailable => RecordedAttachmentVerification::Unavailable {
                    reason: "The browser could not read the retained bytes".into(),
                },
                FileSelectionReadback::Verified { accept } => {
                    match check_file_accept(&accept, &prepared.name) {
                        Ok(()) => RecordedAttachmentVerification::Retained {
                            sha256: prepared.sha256.clone(),
                            size: prepared.size,
                        },
                        Err(error) => RecordedAttachmentVerification::Unavailable {
                            reason: error.to_string(),
                        },
                    }
                }
            };
        }
        if self.recorded_document()? != summary.document {
            return Err(summary_changed());
        }
        if let Some(inline) = &summary.inline {
            self.recorded_inline_root(inline)?;
        }
        require_summary_replay(self)?;
        Ok(outcomes)
    }

    /// Capture independent heading and answer regions. A nested or embedded
    /// view uses its current pixels and restores any owned viewport movement.
    pub fn capture_recorded_summary_row(
        &mut self,
        capture: &RecordedSummaryCapture,
        row: usize,
    ) -> Result<Option<Vec<RecordedSummaryRegion>>, DriverError> {
        let native_row = capture.native.rows.get(row).ok_or_else(summary_changed)?;
        let Some(part_count) = row_parts(native_row).map(|parts| parts.len()) else {
            return Ok(None);
        };
        self.verify_recorded_summary_native(capture)
            .map_err(|error| DriverError(format!("Before the summary detail capture. {error}")))?;
        let regions = if self.captured_scope.is_none() && capture.inline.is_none() {
            plan_clips(native_row, self.page_dimensions()?)
        } else {
            None
        };
        let Some(regions) = regions else {
            return self.capture_viewport_summary_row(capture, row, part_count);
        };
        let mut images = Vec::with_capacity(regions.len());
        let mut encoded_bytes = 0;
        for clip in regions {
            let image = self.capture_page_tile(clip.x, clip.y, clip.width, clip.height, false)?;
            encoded_bytes += image.len();
            if encoded_bytes > 4 * 1024 * 1024 {
                return Err(DriverError(
                    "Summary detail exceeds the visual input budget".into(),
                ));
            }
            images.push(RecordedSummaryRegion {
                clip,
                image,
                mode: "document",
                viewport: None,
            });
        }
        self.verify_recorded_summary_native(capture)
            .map_err(|error| DriverError(format!("After the summary detail capture. {error}")))?;
        Ok(Some(images))
    }

    fn capture_viewport_summary_row(
        &mut self,
        capture: &RecordedSummaryCapture,
        row: usize,
        part_count: usize,
    ) -> Result<Option<Vec<RecordedSummaryRegion>>, DriverError> {
        let expression = format!("({SUMMARY_VIEWPORT})(({SUMMARY_INSPECTOR}),{row})");
        let (root, binding) = self.evaluate_recorded_summary(
            &capture.document,
            capture.inline.as_ref(),
            &expression,
            false,
        )?;
        let Some(object) = binding["objectId"].as_str() else {
            return Ok(None);
        };
        let captured = catch_unwind(AssertUnwindSafe(|| {
            let initial =
                self.call_summary_binding(object, "function(){return this.read()}", None)?;
            if root != capture.root
                || serde_json::from_value::<NativeSummary>(initial)
                    .ok()
                    .as_ref()
                    != Some(&capture.native)
            {
                return Err(summary_changed());
            }
            let mut images = Vec::with_capacity(part_count);
            let mut bytes = 0;
            for part in 0..part_count {
                let shifted = self.call_summary_binding(
                    object,
                    "function(part){return this.reveal(part)}",
                    Some(part),
                )?;
                if shifted.is_null() {
                    return Ok(None);
                }
                let shifted: NativeSummary =
                    serde_json::from_value(shifted).map_err(|_| summary_changed())?;
                if shifted.viewport[2..] != capture.native.viewport[2..]
                    || shifted.incomplete != capture.native.incomplete
                    || !shifted.rows.iter().map(|row| &row.answer).eq(capture
                        .native
                        .rows
                        .iter()
                        .map(|row| &row.answer))
                    || self.recorded_document()? != capture.document
                {
                    return Err(summary_changed());
                }
                let bounds = shifted
                    .rows
                    .get(row)
                    .and_then(row_parts)
                    .and_then(|mut parts| parts.nth(part))
                    .ok_or_else(summary_changed)?;
                let [scroll_x, scroll_y, width, height, _] = shifted.viewport;
                if !bounds.iter().all(|part| part.in_image) {
                    return Ok(None);
                }
                let Some(clip) = SummaryClip::around(
                    bounds.iter().map(|part| {
                        let [x, y, w, h] = part.document;
                        [x - scroll_x, y - scroll_y, w, h]
                    }),
                    (width, height),
                ) else {
                    return Ok(None);
                };
                require_summary_replay(self)?;
                let image = match &capture.inline {
                    Some(inline) => self.screenshot_inline_viewport_bounds(
                        inline,
                        clip.quad(),
                        [width, height],
                    )?,
                    None => self.screenshot_viewport_bounds(clip.quad(), [width, height], true)?,
                };
                let observed = self.call_summary_binding(
                    object,
                    "function(part){return this.observe(part)}",
                    Some(part),
                )?;
                if serde_json::from_value::<NativeSummary>(observed)
                    .ok()
                    .as_ref()
                    != Some(&shifted)
                {
                    return Err(summary_changed());
                }
                bytes += image.len();
                if bytes > 4 * 1024 * 1024 {
                    return Err(DriverError(
                        "Summary detail exceeds the visual input budget".into(),
                    ));
                }
                let mode = if self.captured_scope.is_some() || capture.inline.is_some() {
                    "embedded_viewport"
                } else {
                    "viewport"
                };
                images.push(RecordedSummaryRegion {
                    clip,
                    image,
                    mode,
                    viewport: Some(shifted.viewport),
                });
            }
            Ok(Some(images))
        }));
        let restored = self.call_summary_binding(object, "function(){return this.restore()}", None);
        let released = self
            .cdp
            .call("Runtime.releaseObject", json!({"objectId":object}))
            .map_err(derr);
        if restored? != true {
            return Err(DriverError(
                "The summary scroll position could not be restored".into(),
            ));
        }
        released?;
        self.verify_recorded_summary_native(capture)
            .map_err(|error| {
                DriverError(format!("After restoring the summary viewport. {error}"))
            })?;
        match captured {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn call_summary_binding(
        &mut self,
        object: &str,
        function: &str,
        part: Option<usize>,
    ) -> Result<Value, DriverError> {
        let arguments = part
            .map(|part| vec![json!({"value":part})])
            .unwrap_or_default();
        let response = self.cdp.call("Runtime.callFunctionOn", json!({"objectId":object,
            "functionDeclaration":function,"arguments":arguments,"returnByValue":true,"awaitPromise":true})).map_err(derr)?;
        if response.get("exceptionDetails").is_some() {
            return Err(summary_changed());
        }
        response
            .pointer("/result/value")
            .cloned()
            .ok_or_else(summary_changed)
    }

    pub(super) fn recorded_summary_before_image(
        &mut self,
        document: &RecordedDocument,
        inline: Option<InlineRecordedScope>,
    ) -> Result<RecordedSummaryCapture, DriverError> {
        require_summary_replay(self)?;
        let (root, native) = self.recorded_summary_native(document, inline.as_ref())?;
        let digest = waypoint_domain::sha256_hex(
            &serde_json::to_vec(&json!({
                "url":document.url,"frame":document.frame,"loader":document.loader,
                "lease":document.lease,"inline":inline,"root":root,"native":native
            }))
            .map_err(|_| DriverError("The native review summary could not be recorded".into()))?,
        );
        Ok(RecordedSummaryCapture {
            document: document.clone(),
            inline,
            root,
            native,
            digest,
        })
    }

    pub(super) fn recorded_summary_inline_overview(
        &mut self,
        capture: &RecordedSummaryCapture,
    ) -> Result<Option<String>, DriverError> {
        let Some(inline) = &capture.inline else {
            return Ok(None);
        };
        let [_, _, width, height, _] = capture.native.viewport;
        let clip = SummaryClip {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        self.screenshot_inline_viewport_bounds(inline, clip.quad(), [width, height])
            .map(Some)
    }

    pub(super) fn verify_recorded_summary_native(
        &mut self,
        capture: &RecordedSummaryCapture,
    ) -> Result<(), DriverError> {
        let (root, native) =
            self.recorded_summary_native(&capture.document, capture.inline.as_ref())?;
        if root != capture.root {
            return Err(summary_changed());
        }
        if native.viewport != capture.native.viewport {
            return Err(DriverError(format!(
                "The summary viewport changed from {:?} to {:?}",
                capture.native.viewport, native.viewport
            )));
        }
        if native.rows.len() != capture.native.rows.len()
            || native.incomplete != capture.native.incomplete
        {
            return Err(DriverError(
                "The structured summary inventory changed".into(),
            ));
        }
        for (index, (current, before)) in native.rows.iter().zip(&capture.native.rows).enumerate() {
            if current.answer != before.answer {
                return Err(DriverError(format!("Summary answer {} changed", index + 1)));
            }
            if current.bounds != before.bounds {
                return Err(DriverError(format!(
                    "Summary row {} geometry changed from {:?} to {:?}",
                    index + 1,
                    before.bounds,
                    current.bounds
                )));
            }
        }
        require_summary_replay(self)
    }

    /// Recheck the original native evidence after the independent image read.
    /// This method cannot navigate, focus, click or change a control.
    pub fn verify_recorded_summary(
        &mut self,
        form: &LiveForm,
        capture: &RecordedSummaryCapture,
    ) -> Result<(), DriverError> {
        if !form.walls.is_empty()
            || !form.schema.fields.is_empty()
            || self.inspect_current()? != *form
        {
            return Err(summary_changed());
        }
        self.verify_recorded_summary_native(capture)
    }

    fn recorded_summary_native(
        &mut self,
        document: &RecordedDocument,
        inline: Option<&InlineRecordedScope>,
    ) -> Result<((u64, u64), NativeSummary), DriverError> {
        let (root, data) = self.evaluate_recorded_summary(
            document,
            inline,
            &format!("({SUMMARY_INSPECTOR})(null)"),
            true,
        )?;
        let native = serde_json::from_value(data["value"].clone())
            .map_err(|_| DriverError("The native review summary could not be read".into()))?;
        Ok((root, native))
    }

    fn evaluate_recorded_summary(
        &mut self,
        document: &RecordedDocument,
        inline: Option<&InlineRecordedScope>,
        expression: &str,
        return_by_value: bool,
    ) -> Result<((u64, u64), Value), DriverError> {
        self.validate_captured_frame(false)?;
        if self.recorded_document()? != *document {
            return Err(summary_changed());
        }
        let (frame, ids) = match inline {
            Some(inline) => (
                inline.frame(),
                summary_root_ids(&self.recorded_inline_root(inline)?)?,
            ),
            None => {
                let root = self
                    .cdp
                    .call("DOM.getDocument", json!({"depth":1,"pierce":false}))
                    .map_err(derr)?;
                (document.frame.as_str(), summary_root_ids(&root["root"])?)
            }
        };
        let world = self
            .cdp
            .call(
                "Page.createIsolatedWorld",
                json!({"frameId":frame,"worldName":"Smart Jobseeker summary review"}),
            )
            .map_err(derr)?;
        let context = world["executionContextId"]
            .as_u64()
            .filter(|id| *id != 0)
            .ok_or_else(summary_changed)?;
        let result = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"contextId":context, "expression":expression,
                    "returnByValue":return_by_value,"silent":true,"timeout":1000}),
            )
            .map_err(derr)?;
        if result.get("exceptionDetails").is_some() || self.recorded_document()? != *document {
            return Err(summary_changed());
        }
        if let Some(inline) = inline {
            if summary_root_ids(&self.recorded_inline_root(inline)?)? != ids {
                return Err(summary_changed());
            }
        }
        Ok((ids, result["result"].clone()))
    }
}

fn summary_root_ids(root: &Value) -> Result<(u64, u64), DriverError> {
    let element = root["children"]
        .as_array()
        .and_then(|children| children.iter().find(|node| node["nodeType"] == 1));
    root["backendNodeId"]
        .as_u64()
        .zip(element.and_then(|node| node["backendNodeId"].as_u64()))
        .filter(|(document, html)| *document > 0 && *html > 0)
        .ok_or_else(summary_changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_details_separate_the_heading_from_one_answer_row() {
        let mut row = NativeRow {
            answer: SummaryAnswer {
                key: SummaryKey {
                    sections: vec!["Documents".into()],
                    label: "Writing sample file".into(),
                },
                value: "sample.pdf".into(),
            },
            bounds: Some(
                [
                    [50., 700., 72., 20.],
                    [50., 744., 94., 18.],
                    [180., 744., 95., 18.],
                ]
                .into_iter()
                .map(|document| NativeTextBounds {
                    document,
                    in_image: false,
                    page_clip: true,
                })
                .collect(),
            ),
        };
        let page = (1000., 1200.);
        let clips = plan_clips(&row, page).unwrap();
        assert_eq!(clips.len(), 2);
        assert!(clips[0].y + clips[0].height < clips[1].y);

        row.answer.key.sections.clear();
        row.bounds.as_mut().unwrap().remove(0);
        let ungrouped = plan_clips(&row, page).unwrap();
        assert_eq!(ungrouped.len(), 1);
        assert_eq!(ungrouped[0].y, clips[1].y);
    }

    #[test]
    fn key_recovers_known_prefixes_and_preserves_punctuation_in_leaf() {
        let field = serde_json::from_value(json!({
            "selector":"#job","label":"Employer — Possible section: Details — Employer — Role",
            "field_type":"text","required":true,"sensitive":false,
            "context":{"sections":[
                {"id":"1","label":"Employer","inferred":false},
                {"id":"2","label":"Details","inferred":true},
                {"id":"3","label":"Employer","inferred":false}
            ]}
        }))
        .unwrap();
        assert_eq!(
            SummaryKey::from_field(&field),
            SummaryKey {
                sections: vec!["Employer".into(), "Details".into(), "Employer".into()],
                label: "Employer — Role".into(),
            }
        );
    }
}
