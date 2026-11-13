//! Explicit, source-bound entry into one recorded cross-origin frame.
#[cfg(windows)]
#[path = "recorded_dialog.rs"]
mod dialog;
#[cfg(windows)]
pub use dialog::{DialogChoice, DialogDescription, RecordedDialog, RecordedDialogOutcome};

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
    File,
}

#[cfg(windows)]
pub enum RecordedSectionOutcome {
    Advanced { label: String, form: LiveForm },
    Stopped(RecordedSectionStop),
}

#[cfg(windows)]
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedSectionStop {
    NoForwardControl,
    IncompleteControls,
    AmbiguousControl,
    UnsupportedControl,
    RequiredFieldsUnverified,
    LimitReached,
    ControlChanged,
    DocumentChanged,
    PageNeedsReview,
    FieldChanged,
    ReplayGap,
    NoNewSection,
}

#[cfg(any(windows, test))]
fn recorded_forward_label(label: &str) -> bool {
    let label = label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    matches!(
        label.trim_end_matches(['→', '»', '>']).trim_end(),
        "next"
            | "continue"
            | "next step"
            | "next section"
            | "continue section"
            | "continue to next step"
            | "continue to next section"
            | "weiter"
            | "nächster schritt"
            | "suivant"
            | "étape suivante"
            | "siguiente"
            | "paso siguiente"
    )
}

#[cfg(windows)]
fn section_fields(form: &LiveForm) -> std::collections::BTreeSet<Vec<String>> {
    form.schema
        .fields
        .iter()
        .map(|field| {
            field
                .context
                .sections
                .iter()
                .map(|section| section.label.as_str())
                .chain(std::iter::once(field.label.as_str()))
                .map(|text| {
                    text.split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase()
                })
                .collect()
        })
        .collect()
}

/// Pixels with independently verified field state in a recorded form.
/// Only the recorded driver can construct this evidence.
pub struct RecordedFieldCapture {
    image: String,
    detail_image: Option<String>,
    detail_error: Option<String>,
    label: String,
    kind: RecordedFieldKind,
    required_marker: bool,
    command: BrowserCommand,
    network_before: u64,
    document: RecordedDocument,
    control: RecordedControlIdentity,
    observed_value: String,
    value_comparison: &'static str,
    file_group_label: Option<String>,
    file_group_context: Option<waypoint_applications::form_mapping::FormFieldContext>,
}

/// A fresh, focused target image made by the recorded driver before an action.
/// The caller may refuse the action; accepting never bypasses native checks.
pub struct RecordedTargetCapture {
    image: String,
    detail_image: Option<String>,
    detail_error: Option<String>,
    label: String,
    kind: &'static str,
    required_marker: bool,
    file_group_label: Option<String>,
    witness: Option<RecordedFieldWitness>,
}

/// Legacy native identity plus separate read-only structural evaluation identity.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RecordedControlIdentity {
    origin: String,
    selector: String,
    sections: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logical: Option<LogicalControlIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logical_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct LogicalControlIdentity {
    schema: u8,
    document_url: String,
    selector: String,
    scopes: Vec<LogicalControlScope>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum LogicalControlScope {
    Frame {
        owner_selector: String,
        document_url: String,
    },
    Shadow {
        owner_selector: String,
    },
}

/// Only an observed recorded target can create this read-only witness.
#[derive(Clone)]
pub struct RecordedFieldWitness {
    selector: String,
    control: RecordedControlIdentity,
    schema: waypoint_applications::form_mapping::FormSchema,
    document: RecordedDocument,
    network_before: u64,
    kind: RecordedFieldKind,
}

/// Coherent current evidence; it never proves successful or valid entry.
pub struct RecordedFieldObservation {
    image: String,
    value: Option<String>,
    control: RecordedControlIdentity,
}
impl RecordedFieldObservation {
    pub fn image(&self) -> &str {
        &self.image
    }
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
    pub fn control(&self) -> &RecordedControlIdentity {
        &self.control
    }
}

#[cfg(any(windows, test))]
fn required_marker_unambiguous(form: &LiveForm, field: &FormField) -> bool {
    let normalized = |label: &str| {
        label
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let label = normalized(&field.label);
    field.required
        && !label.is_empty()
        && !label.ends_with('*')
        && form
            .schema
            .fields
            .iter()
            .filter(|other| {
                let other = normalized(&other.label);
                other == label
                    || other
                        .strip_suffix('*')
                        .is_some_and(|base| base.trim_end() == label)
            })
            .count()
            == 1
}

/// A file chooser may be visually named by its explicit enclosing group while
/// its input has a hidden button label. Never infer an alternative from words.
#[cfg(any(windows, test))]
fn unique_file_group_label(form: &LiveForm, field: &FormField) -> Option<String> {
    use waypoint_applications::form_mapping::FormFieldType;
    if field.field_type != FormFieldType::File || field.context.sections.is_empty() {
        return None;
    }
    if field
        .context
        .sections
        .iter()
        .any(|section| section.inferred || section.id.is_empty() || section.label.trim().is_empty())
    {
        return None;
    }
    let group = field.context.sections.last()?;
    let label = field
        .context
        .sections
        .iter()
        .map(|s| s.label.as_str())
        .collect::<Vec<_>>()
        .join(" — ");
    if label.len() > 300 || !field.label.starts_with(&format!("{label} — ")) {
        return None;
    }
    let same_group = form
        .schema
        .fields
        .iter()
        .filter(|other| {
            other.field_type == FormFieldType::File
                && other.context.sections.iter().any(|s| s.id == group.id)
        })
        .count();
    let labels = |field: &FormField| {
        field
            .context
            .sections
            .iter()
            .map(|s| {
                s.label
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            })
            .collect::<Vec<_>>()
    };
    let path = labels(field);
    let same_name = form
        .schema
        .fields
        .iter()
        .filter(|other| other.field_type == FormFieldType::File && labels(other) == path)
        .count();
    (same_group == 1 && same_name == 1).then_some(label)
}

impl RecordedTargetCapture {
    pub fn witness(&self) -> Option<RecordedFieldWitness> {
        self.witness.clone()
    }
    pub fn control(&self) -> Option<&RecordedControlIdentity> {
        self.witness.as_ref().map(|witness| &witness.control)
    }
    pub fn image(&self) -> &str {
        &self.image
    }
    pub fn detail_image(&self) -> Option<&str> {
        self.detail_image.as_deref()
    }
    pub fn detail_error(&self) -> Option<&str> {
        self.detail_error.as_deref()
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn kind(&self) -> &str {
        self.kind
    }
    /// A trailing required asterisk may differ from the accessible label only
    /// when the inspected field is required and has no conflicting field label.
    pub fn required_marker(&self) -> bool {
        self.required_marker
    }
    /// Available only for one native file input in uniquely named, explicit groups.
    pub fn file_group_label(&self) -> Option<&str> {
        self.file_group_label.as_deref()
    }
}

#[derive(Clone, PartialEq, Eq)]
struct RecordedDocument {
    url: String,
    frame: String,
    loader: String,
    lease: Option<String>,
}

impl RecordedFieldCapture {
    pub fn detail_image(&self) -> Option<&str> {
        self.detail_image.as_deref()
    }
    pub fn detail_error(&self) -> Option<&str> {
        self.detail_error.as_deref()
    }
    pub fn file_group_label(&self) -> Option<&str> {
        self.file_group_label.as_deref()
    }
    /// Actual captured native text, not the commanded candidate fact.
    pub fn observed_value(&self) -> &str {
        &self.observed_value
    }
    pub fn value_comparison(&self) -> &'static str {
        self.value_comparison
    }
    pub fn control(&self) -> &RecordedControlIdentity {
        &self.control
    }
    pub fn required_marker(&self) -> bool {
        self.required_marker
    }
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
            | BrowserCommand::SelectRadio { label: value, .. }
            | BrowserCommand::UploadFile {
                file_name: value, ..
            } => value,
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
    /// Try one same-document section change in the owned replay browser.
    /// Only newly observed controls count as progress.
    #[cfg(windows)]
    pub fn advance_recorded_section(
        &mut self,
        before: &LiveForm,
        visited: &[LiveForm],
        verified_fields: &[&RecordedFieldCapture],
        budget_available: bool,
    ) -> Result<RecordedSectionOutcome, DriverError> {
        self.advance_recorded_section_reviewed(
            before,
            visited,
            verified_fields,
            budget_available,
            |_| Ok(()),
        )
    }

    #[cfg(windows)]
    pub fn advance_recorded_section_reviewed(
        &mut self,
        before: &LiveForm,
        visited: &[LiveForm],
        verified_fields: &[&RecordedFieldCapture],
        budget_available: bool,
        mut review_target: impl FnMut(&RecordedTargetCapture) -> Result<(), DriverError>,
    ) -> Result<RecordedSectionOutcome, DriverError> {
        use RecordedSectionOutcome::{Advanced, Stopped};
        use RecordedSectionStop::*;
        self.require_recorded_frames()?;
        if !before.walls.is_empty() || before.schema.fields.is_empty() {
            return Ok(Stopped(PageNeedsReview));
        }
        let document = self.recorded_document()?;
        let controls = self.page_controls()?;
        if controls.incomplete {
            return Ok(Stopped(IncompleteControls));
        }
        let candidates: Vec<_> = controls
            .controls
            .iter()
            .filter(|control| {
                possible_next_label(&control.label) || recorded_forward_label(&control.label)
            })
            .collect();
        let target = match candidates.as_slice() {
            [] => return Ok(Stopped(NoForwardControl)),
            [target] => *target,
            _ => return Ok(Stopped(AmbiguousControl)),
        };
        if target.disabled || !recorded_forward_label(&target.label) {
            return Ok(Stopped(UnsupportedControl));
        }
        if before
            .schema
            .fields
            .iter()
            .filter(|field| field.required)
            .any(|field| {
                !verified_fields
                    .iter()
                    .any(|capture| match &capture.command {
                        BrowserCommand::Fill { selector, .. }
                        | BrowserCommand::SelectOption { selector, .. }
                        | BrowserCommand::SelectRadio { selector, .. }
                        | BrowserCommand::UploadFile { selector, .. } => {
                            *selector == field.selector
                        }
                        _ => false,
                    })
            })
        {
            return Ok(Stopped(RequiredFieldsUnverified));
        }
        if !budget_available {
            return Ok(Stopped(LimitReached));
        }
        let network_before = self.observed_non_read_requests()?;
        let replay_before = self.rehearsal_replay_report()?;
        let replay_intact = |driver: &mut Self| -> Result<bool, DriverError> {
            let replay = driver.rehearsal_replay_report()?;
            Ok(driver.observed_non_read_requests()? == network_before
                && [
                    "blocked",
                    "cached_non_read_replies",
                    "unreplayed_transports",
                    "unmanaged_child_sessions",
                    "unsupported_targets",
                ]
                .iter()
                .all(|key| replay[*key] == replay_before[*key]))
        };
        self.scroll_field_into_view(&target.selector)?;
        let image = self.screenshot_field_context(&target.selector)?;
        review_target(&RecordedTargetCapture {
            witness: None,
            detail_image: None,
            detail_error: None,
            image,
            label: target.label.clone(),
            required_marker: false,
            file_group_label: None,
            kind: "button",
        })?;
        if self.recorded_document()? != document || self.inspect_current()? != *before {
            return Ok(Stopped(DocumentChanged));
        }
        if verified_fields
            .iter()
            .any(|capture| self.verify_recorded_field(capture).is_err())
        {
            return Ok(Stopped(FieldChanged));
        }
        if self.page_controls()? != controls {
            return Ok(Stopped(ControlChanged));
        }
        if !replay_intact(self)? {
            return Ok(Stopped(ReplayGap));
        }
        let node = self.node_id(&target.selector)?.ok_or_else(review)?;
        if self
            .click_node_guarded(
                node,
                &target.selector,
                ClickGuard::RecordedSection {
                    label: &target.label,
                    non_read_before: network_before,
                },
            )
            .is_err()
        {
            return Ok(Stopped(
                if self.observed_non_read_requests()? > network_before {
                    ReplayGap
                } else {
                    UnsupportedControl
                },
            ));
        }
        let before_fields = section_fields(before);
        let seen: Vec<_> = visited.iter().map(section_fields).collect();
        for _ in 0..3 {
            self.cdp
                .pump_checked(Duration::from_millis(300))
                .map_err(derr)?;
            if !replay_intact(self)? {
                return Ok(Stopped(ReplayGap));
            }
            if self.recorded_document()? != document {
                return Ok(Stopped(DocumentChanged));
            }
            let after = self.inspect_current()?;
            if after.origin != before.origin || self.recorded_document()? != document {
                return Ok(Stopped(DocumentChanged));
            }
            if !after.walls.is_empty() {
                return Ok(Stopped(PageNeedsReview));
            }
            let fields = section_fields(&after);
            if !fields.is_subset(&before_fields) && !seen.contains(&fields) {
                if !replay_intact(self)? {
                    return Ok(Stopped(ReplayGap));
                }
                return Ok(Advanced {
                    label: target.label.clone(),
                    form: after,
                });
            }
        }
        Ok(Stopped(NoNewSection))
    }

    /// Exercise an unambiguous text or native choice control. Bind pixels to
    /// readback on both sides of capture and reject a changed scope.
    pub fn rehearse_recorded_field(
        &mut self,
        form: &LiveForm,
        selector: &str,
        value: &str,
    ) -> Result<RecordedFieldCapture, DriverError> {
        self.rehearse_recorded_field_reviewed(form, selector, value, &[], |_| Ok(()))
    }

    pub fn rehearse_recorded_field_reviewed(
        &mut self,
        form: &LiveForm,
        selector: &str,
        value: &str,
        verified_fields: &[&RecordedFieldCapture],
        mut review_target: impl FnMut(&RecordedTargetCapture) -> Result<(), DriverError>,
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
        if form.independent_field_entry_blocked()
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
            FormFieldType::File => (
                RecordedFieldKind::File,
                BrowserCommand::UploadFile {
                    selector: selector.into(),
                    file_name: value.into(),
                },
            ),
            _ => return Err(review()),
        };
        let document = self.recorded_document()?;
        let check_scope = |driver: &mut Self, phase: &str| {
            let current = driver.inspect_current()?;
            if current != *form || driver.recorded_document()? != document {
                return Err(DriverError(format!("The saved form changed {phase}; inspect it again before continuing (schema changed: {}, review notes changed: {}).", current.schema != form.schema, current.walls != form.walls)));
            }
            Ok(())
        };
        check_scope(self, "before the field review")?;
        let network_before = self.observed_non_read_requests()?;
        let image = if matches!(kind, RecordedFieldKind::Radio) {
            self.screenshot_recorded_radio(selector, value, false)?
        } else {
            self.scroll_field_into_view(selector)?;
            self.screenshot_field_context(selector)?
        };
        let (detail_image, detail_error) = self.recorded_detail_context(selector, kind);
        let control = self.recorded_control_identity(form, selector)?;
        review_target(&RecordedTargetCapture {
            detail_image,
            detail_error,
            witness: Some(RecordedFieldWitness {
                selector: selector.to_owned(),
                control: control.clone(),
                schema: form.schema.clone(),
                document: document.clone(),
                network_before,
                kind,
            }),
            image,
            label: field.label.clone(),
            required_marker: required_marker_unambiguous(form, field),
            file_group_label: unique_file_group_label(form, field),
            kind: match kind {
                RecordedFieldKind::Text => "text",
                RecordedFieldKind::Select => "select",
                RecordedFieldKind::Radio => "radio",
                RecordedFieldKind::File => "file",
            },
        })?;
        check_scope(self, "after the field image review")?;
        self.stop_if_non_read_request_observed(network_before)?;
        for capture in verified_fields {
            self.verify_recorded_field(capture)?;
        }
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
            RecordedFieldKind::File => {
                let (path, hash) = self.recorded_attachment(value)?;
                self.attach_local_files_inner(selector, &[path], Some(&[]), Some(&hash))?;
            }
        }
        self.stop_if_non_read_request_observed(network_before)?;
        self.verify_recorded_command(&command)?;
        let observed_value = if matches!(kind, RecordedFieldKind::Text) {
            self.text_value_snapshot(selector)?.value
        } else {
            value.to_owned()
        };
        let value_comparison = if observed_value == value {
            "exact"
        } else if matches!(kind, RecordedFieldKind::Text)
            && waypoint_domain::telephone::same_international(value, &observed_value)
        {
            "international_telephone_presentation"
        } else {
            return Err(review());
        };
        let image = if matches!(kind, RecordedFieldKind::Radio) {
            self.screenshot_recorded_radio(selector, value, true)?
        } else {
            self.screenshot_field_feedback(selector)?
        };
        let (detail_image, detail_error) = self.recorded_detail_context(selector, kind);
        if matches!(kind, RecordedFieldKind::Text)
            && self.text_value_snapshot(selector)?.value != observed_value
        {
            return Err(review());
        }
        check_scope(self, "after field entry")?;
        self.verify_recorded_command(&command)?;
        self.observed_non_read_requests()?;
        self.stop_if_non_read_request_observed(network_before)?;
        let file_group_label = unique_file_group_label(form, field);
        Ok(RecordedFieldCapture {
            detail_image,
            detail_error,
            file_group_context: file_group_label.as_ref().map(|_| field.context.clone()),
            file_group_label,
            observed_value,
            value_comparison,
            control,
            image,
            label: field.label.clone(),
            required_marker: required_marker_unambiguous(form, field),
            kind,
            command,
            network_before,
            document,
        })
    }

    // Both phases use the same bounded native rectangle without focus changes.
    // Existing before/after document, schema and value checks enclose capture.
    fn recorded_detail_context(
        &mut self,
        selector: &str,
        kind: RecordedFieldKind,
    ) -> (Option<String>, Option<String>) {
        if matches!(kind, RecordedFieldKind::Text | RecordedFieldKind::Select)
            && self.captured_scope.is_none()
        {
            match self.screenshot_field_detail(selector) {
                Ok(image) => (Some(image), None),
                Err(error) => (None, Some(error.to_string())),
            }
        } else {
            (None, None)
        }
    }

    /// A read-only logical identity. It conveys no field-entry authority.
    pub fn recorded_control_identity(
        &mut self,
        form: &LiveForm,
        selector: &str,
    ) -> Result<RecordedControlIdentity, DriverError> {
        self.require_recorded_frames()?;
        let mut fields = form
            .schema
            .fields
            .iter()
            .filter(|field| field.selector == selector);
        let field = fields.next().ok_or_else(review)?;
        if fields.next().is_some() {
            return Err(review());
        }
        let logical = self.logical_control_identity(selector);
        let logical_error = logical.as_ref().err().map(|error| {
            let detail: String = error.0.chars().take(1900).collect();
            format!(
                "Structural identity unavailable: {detail}{}",
                if error.0.chars().count() > 1900 {
                    " (truncated)"
                } else {
                    ""
                }
            )
        });
        Ok(RecordedControlIdentity {
            logical: logical.ok(),
            logical_error,
            origin: form.origin.clone(),
            selector: self.decode_captured_selector(selector)?.into_owned(),
            sections: field
                .context
                .sections
                .iter()
                .map(|section| section.label.clone())
                .collect(),
        })
    }

    fn logical_control_identity(
        &mut self,
        selector: &str,
    ) -> Result<LogicalControlIdentity, DriverError> {
        let mut scopes = Vec::new();
        let document_url = if let Some(scope) = &self.captured_scope {
            let frame = scope.frame.clone();
            let owner = self.parent_frame_owner(&frame)?;
            let owner_selector = self.with_protocol_session(&frame.parent_session, |driver| {
                driver
                    .control_visibility_and_path(owner)
                    .map(|visibility| visibility.1)
            })?;
            scopes.push(LogicalControlScope::Frame {
                owner_selector,
                document_url: frame.url.clone(),
            });
            frame.parent_url
        } else {
            self.current_url()?
        };
        let resolved = self
            .resolve_control_node(selector, true)?
            .ok_or_else(review)?;
        scopes.extend(resolved.scopes);
        let selector = self.control_visibility_and_path(resolved.node)?.1;
        let identity = LogicalControlIdentity {
            schema: 1,
            document_url,
            selector,
            scopes,
        };
        if identity.scopes.len() > 8
            || identity.document_url.is_empty()
            || identity.document_url.len() > 8192
            || serde_json::to_vec(&identity).map_err(|_| review())?.len() > 32 * 1024
        {
            return Err(review());
        }
        Ok(identity)
    }

    /// Inspect after a failed action without changing values, retrying or
    /// granting entry. Navigation/schema/barrier/network changes refuse evidence.
    pub fn observe_recorded_field(
        &mut self,
        witness: &RecordedFieldWitness,
    ) -> Result<RecordedFieldObservation, DriverError> {
        self.require_recorded_frames()?;
        let check = |driver: &mut Self| {
            let current = driver.inspect_current()?;
            if current.origin != witness.control.origin
                || current.schema != witness.schema
                || current.independent_field_entry_blocked()
                || driver.recorded_document()? != witness.document
            {
                return Err(review());
            }
            driver.stop_if_non_read_request_observed(witness.network_before)
        };
        check(self)?;
        let text = matches!(witness.kind, RecordedFieldKind::Text);
        let read = |driver: &mut Self| -> Result<Option<String>, DriverError> {
            if text {
                Ok(Some(driver.text_value_snapshot(&witness.selector)?.value))
            } else {
                Ok(None)
            }
        };
        let value = read(self)?;
        let image = self.screenshot_field_feedback(&witness.selector)?;
        if read(self)? != value {
            return Err(review());
        }
        check(self)?;
        Ok(RecordedFieldObservation {
            image,
            value,
            control: witness.control.clone(),
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
        if matches!(capture.kind, RecordedFieldKind::Text) {
            let BrowserCommand::Fill { selector, .. } = &capture.command else {
                return Err(review());
            };
            if self.text_value_snapshot(selector)?.value != capture.observed_value {
                return Err(review());
            }
        }
        if self.recorded_document()? != capture.document {
            return Err(review());
        }
        if let Some(context) = &capture.file_group_context {
            let BrowserCommand::UploadFile { selector, .. } = &capture.command else {
                return Err(review());
            };
            let current = self.inspect_current()?;
            let fields: Vec<_> = current
                .schema
                .fields
                .iter()
                .filter(|f| &f.selector == selector)
                .collect();
            let [field] = fields.as_slice() else {
                return Err(review());
            };
            if &field.context != context
                || unique_file_group_label(&current, field) != capture.file_group_label
            {
                return Err(review());
            }
        }
        self.stop_if_non_read_request_observed(capture.network_before)
    }

    fn verify_recorded_command(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
        match command {
            BrowserCommand::Fill {
                selector,
                value: PacketValue::Text(value),
            } => self.verify_text_value(selector, value),
            BrowserCommand::UploadFile {
                selector,
                file_name,
            } => {
                let (path, hash) = self.recorded_attachment(file_name)?;
                self.verify_local_files_inner(selector, &[path], Some(&hash))
            }
            _ => self.verify_choice(command),
        }
    }

    fn recorded_attachment(&self, name: &str) -> Result<(PathBuf, String), DriverError> {
        let inventory = self.recorded_attachments.as_ref().ok_or_else(review)?;
        let hash = inventory.files.get(name).ok_or_else(review)?;
        Ok((inventory.root.join(name), hash.clone()))
    }

    fn screenshot_recorded_radio(
        &mut self,
        selector: &str,
        label: &str,
        require_checked: bool,
    ) -> Result<String, DriverError> {
        let node = self
            .radio_selection_inner(selector, label, false, true, require_checked)?
            .ok_or_else(review)?;
        self.cdp
            .call("DOM.scrollIntoViewIfNeeded", json!({"nodeId":node}))
            .map_err(derr)?;
        if self.captured_scope.is_some() {
            self.screenshot_captured_node(node, true, true, false)
        } else {
            self.screenshot_radio_node(node)
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
    pub(super) fn bind_captured_form(&mut self, form: LiveForm) -> LiveForm {
        if let Some(scope) = &self.captured_scope {
            form.with_control_scope(&format!("{PREFIX}{}:", scope.key))
        } else {
            form
        }
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
        feedback: bool,
    ) -> Result<String, DriverError> {
        self.scroll_field_into_view(selector)?;
        let node = self.node_id(selector)?.ok_or_else(review)?;
        self.screenshot_captured_node(node, context, false, feedback)
    }

    fn screenshot_captured_node(
        &mut self,
        node: u64,
        context: bool,
        radio: bool,
        feedback: bool,
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
        let file_context = context && feedback && !radio && self.is_native_file_node(node)?;
        let bounds = recorded_field_crop(quad, &point, x, y, context, radio, file_context)?;
        let image = self.with_captured_parent_capture(|driver| {
            driver.screenshot_bounds(&bounds, false, false, false)
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
    file_context: bool,
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
        let [left, top, right, bottom] = crate::radio_context_bounds(
            [left, top, right, bottom],
            [0.0, 0.0, point.child_width, point.child_height],
        )?;
        (left, top, right, bottom)
    } else if context && file_context {
        // File progress/errors can be below the chooser. Share the main-page
        // file context budget, clipped to this exact child. Other crops stay small.
        let start = context_left_edge(left, right, 0.0, point.child_width);
        let width = 720.0_f64.min(point.child_width - start);
        let (top, height) = context_vertical_bounds(top, bottom, 0.0, point.child_height, true);
        (start, top, start + width, top + height)
    } else if context {
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
    use waypoint_applications::form_mapping::FormSchema;

    #[test]
    fn file_group_labels_require_explicit_unique_native_file_context() {
        let field: FormField = serde_json::from_value(json!({
            "selector":"#document","label":"Supporting material — Choose file","field_type":"file",
            "required":false,"sensitive":false,"context":{"sections":[{"id":"native-group","label":"Supporting material","inferred":false}]}
        })).unwrap();
        let original = LiveForm::from_inspection(
            FormSchema {
                origin: "https://example.org".into(),
                fields: vec![field],
                multi_step: false,
                steps: 1,
            },
            vec![],
            vec![],
        );
        let label = |form: &LiveForm| unique_file_group_label(form, &form.schema.fields[0]);
        assert_eq!(label(&original).as_deref(), Some("Supporting material"));
        let mut changed = original.clone();
        changed.schema.fields[0].context.sections[0].inferred = true;
        assert_eq!(label(&changed), None);
        changed = original.clone();
        changed.schema.fields[0].context.sections[0].id.clear();
        assert_eq!(label(&changed), None);
        changed = original.clone();
        changed.schema.fields[0].label = "Other — Choose file".into();
        assert_eq!(label(&changed), None);
        changed = original.clone();
        changed.schema.fields[0].field_type =
            waypoint_applications::form_mapping::FormFieldType::Text;
        assert_eq!(label(&changed), None);
        changed = original.clone();
        changed.schema.fields[0].context.sections.clear();
        assert_eq!(label(&changed), None);
        let mut second = original.schema.fields[0].clone();
        second.selector = "#second".into();
        changed = original.clone();
        changed.schema.fields.push(second.clone());
        assert_eq!(label(&changed), None, "two files in the same group");
        second.context.sections[0].id = "another-native-group".into();
        changed = original.clone();
        changed.schema.fields.push(second.clone());
        assert_eq!(
            label(&changed),
            None,
            "same visible group names remain ambiguous"
        );
        second.context.sections[0].label = "Other documents".into();
        second.label = "Other documents — Choose file".into();
        changed = original.clone();
        changed.schema.fields.push(second);
        assert_eq!(label(&changed).as_deref(), Some("Supporting material"));
    }

    #[test]
    fn required_marker_evidence_needs_unique_inspected_required_field() {
        let field: FormField = serde_json::from_value(json!({"selector":"#name","label":"Name","field_type":"text","required":true,"sensitive":false})).unwrap();
        let mut form = LiveForm::from_inspection(
            FormSchema {
                origin: "https://example.org".into(),
                fields: vec![field.clone()],
                multi_step: false,
                steps: 1,
            },
            vec![],
            vec![],
        );
        assert!(required_marker_unambiguous(&form, &field));
        let mut optional = field.clone();
        optional.required = false;
        assert!(!required_marker_unambiguous(&form, &optional));
        for label in ["Name", "NAME *", "Name*"] {
            let mut duplicate = field.clone();
            duplicate.selector = "#other".into();
            duplicate.label = label.into();
            form.schema.fields.push(duplicate);
            assert!(!required_marker_unambiguous(&form, &field));
            form.schema.fields.pop();
        }
        let mut literal = field.clone();
        literal.label = "Name*".into();
        assert!(!required_marker_unambiguous(&form, &literal));
    }

    #[test]
    fn recorded_forward_labels_leave_submit_and_ambiguous_phrases_for_review() {
        for label in [
            "Next",
            " Continue section ",
            "Next step →",
            "Étape suivante",
        ] {
            assert!(recorded_forward_label(label), "{label}");
        }
        for label in [
            "Continue to submit",
            "Continue with Google",
            "Next employer",
            "Save and send",
        ] {
            assert!(!recorded_forward_label(label), "{label}");
        }
    }

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
            recorded_field_crop(quad, &point(), 80.0, 20.0, true, false, false).unwrap(),
            [250.0, 400.0, 408.0, 400.0, 408.0, 448.0, 250.0, 448.0]
        );
        assert_eq!(
            recorded_field_crop(quad, &point(), 80.0, 20.0, false, false, false).unwrap(),
            [270.0, 410.0, 390.0, 410.0, 390.0, 430.0, 270.0, 430.0]
        );
        assert_eq!(
            recorded_field_crop(quad, &point(), 80.0, 20.0, true, false, true).unwrap(),
            [250.0, 400.0, 550.0, 400.0, 550.0, 500.0, 250.0, 500.0]
        );
    }

    #[test]
    fn partially_visible_or_empty_child_control_never_becomes_a_complete_capture() {
        for quad in [
            [-1.0, 10.0, 10.0, 10.0, 10.0, 20.0, -1.0, 20.0],
            [20.0, 90.0, 30.0, 90.0, 30.0, 101.0, 20.0, 101.0],
            [20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0],
        ] {
            assert!(recorded_field_crop(quad, &point(), 80.0, 20.0, true, false, false).is_err());
        }
    }
}
