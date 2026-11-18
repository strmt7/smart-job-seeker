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

#[derive(PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct NativeRow {
    answer: SummaryAnswer,
    // Each rectangle belongs to visible heading, label or answer text.
    // A null rectangle means those pixels cannot be bound to this viewport.
    bounds: Option<Vec<[f64; 4]>>,
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
    root: (u64, u64),
    native: NativeSummary,
    digest: String,
}

impl RecordedSummaryCapture {
    pub fn answers(&self) -> impl Iterator<Item = (&SummaryAnswer, bool)> {
        self.native
            .rows
            .iter()
            .map(|row| (&row.answer, row.bounds.is_some()))
    }

    pub fn incomplete(&self) -> bool {
        self.native.incomplete
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
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
        if root != capture.root || native != capture.native {
            return Err(summary_changed());
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
