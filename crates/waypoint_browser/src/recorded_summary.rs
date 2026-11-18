//! A read-only summary census in the same owned document as the review image.
use super::*;

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

#[derive(serde::Deserialize, serde::Serialize)]
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

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct NativeSummary {
    rows: Vec<NativeRow>,
    incomplete: bool,
    viewport: [f64; 5],
}

pub struct RecordedSummaryCapture {
    document: RecordedDocument,
    root: (u64, u64),
    native: NativeSummary,
    digest: String,
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
}

impl SummaryClip {
    fn around(text: &[NativeTextBounds], page: (f64, f64)) -> Option<Self> {
        let [x, y, w, h] = text.first()?.document;
        let (mut left, mut top, mut right, mut bottom) = (x, y, x + w, y + h);
        for part in text {
            let [x, y, width, height] = part.document;
            if !part.page_clip
                || !part.document.iter().all(|value| value.is_finite())
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
}

fn plan_clips(row: &NativeRow, page: (f64, f64)) -> Option<Vec<SummaryClip>> {
    let bounds = row.bounds.as_ref()?;
    let heading_count = row.answer.key.sections.len();
    if heading_count > 8 || bounds.len() != heading_count + 2 {
        return None;
    }
    if let Some(full) = SummaryClip::around(bounds, page) {
        return Some(vec![full]);
    }
    let mut regions = bounds[..heading_count]
        .iter()
        .map(|heading| SummaryClip::around(std::slice::from_ref(heading), page))
        .collect::<Option<Vec<_>>>()?;
    regions.push(SummaryClip::around(&bounds[heading_count..], page)?);
    Some(regions)
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
    /// Capture the native geometry for one row without scrolling it into view.
    /// A separated heading is captured before the label/value region.
    pub fn capture_recorded_summary_row(
        &mut self,
        capture: &RecordedSummaryCapture,
        row: usize,
    ) -> Result<Option<Vec<RecordedSummaryRegion>>, DriverError> {
        if self.captured_scope.is_some() {
            return Ok(None);
        }
        let row = capture.native.rows.get(row).ok_or_else(summary_changed)?;
        let Some(regions) = plan_clips(row, self.page_dimensions()?) else {
            return Ok(None);
        };
        self.verify_recorded_summary_native(capture)
            .map_err(|error| DriverError(format!("Before the summary detail capture. {error}")))?;
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
            images.push(RecordedSummaryRegion { clip, image });
        }
        self.verify_recorded_summary_native(capture)
            .map_err(|error| DriverError(format!("After the summary detail capture. {error}")))?;
        Ok(Some(images))
    }

    pub(super) fn recorded_summary_before_image(
        &mut self,
        document: &RecordedDocument,
    ) -> Result<RecordedSummaryCapture, DriverError> {
        require_summary_replay(self)?;
        let (root, native) = self.recorded_summary_native(document)?;
        let digest = waypoint_domain::sha256_hex(
            &serde_json::to_vec(&json!({
                "url":document.url,"frame":document.frame,"loader":document.loader,
                "lease":document.lease,"root":root,"native":native
            }))
            .map_err(|_| DriverError("The native review summary could not be recorded".into()))?,
        );
        Ok(RecordedSummaryCapture {
            document: document.clone(),
            root,
            native,
            digest,
        })
    }

    pub(super) fn verify_recorded_summary_native(
        &mut self,
        capture: &RecordedSummaryCapture,
    ) -> Result<(), DriverError> {
        let (root, native) = self.recorded_summary_native(&capture.document)?;
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
    ) -> Result<((u64, u64), NativeSummary), DriverError> {
        self.validate_captured_frame(false)?;
        if self.recorded_document()? != *document {
            return Err(summary_changed());
        }
        let root = self
            .cdp
            .call("DOM.getDocument", json!({"depth":1,"pierce":false}))
            .map_err(derr)?;
        let document_id = root.pointer("/root/backendNodeId").and_then(Value::as_u64);
        let element_id = root["root"]["children"].as_array().and_then(|children| {
            children
                .iter()
                .find(|node| node["nodeType"] == 1)
                .and_then(|node| node["backendNodeId"].as_u64())
        });
        let ids = document_id.zip(element_id).ok_or_else(summary_changed)?;
        let world = self
            .cdp
            .call(
                "Page.createIsolatedWorld",
                json!({"frameId":document.frame,"worldName":"Smart Jobseeker summary review"}),
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
                json!({"contextId":context, "expression":include_str!("recorded_summary.js"),
                    "returnByValue":true,"silent":true,"timeout":1000}),
            )
            .map_err(derr)?;
        if result.get("exceptionDetails").is_some() || self.recorded_document()? != *document {
            return Err(summary_changed());
        }
        let native = serde_json::from_value(result["result"]["value"].clone())
            .map_err(|_| DriverError("The native review summary could not be read".into()))?;
        Ok((ids, native))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
