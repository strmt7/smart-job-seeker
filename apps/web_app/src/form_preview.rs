//! Form inspection and a model-generated plan. This path does not execute
//! field commands or submit; the caller controls the browser's lifetime.
use serde_json::Value;
use waypoint_applications::driver::BrowserDriver;
use waypoint_applications::form_mapping::{
    BrowserCommand, FormFieldType, FormSchema, PacketValue, ProposedMapping,
};
use waypoint_domain::{
    ApplicationState, ApprovalGrant, GrantOperation, IntentId, JobIdentityId, Sha256Digest,
};
use waypoint_inference::Inference;
use waypoint_policy::Operation;
use waypoint_workspace::Workspace;

#[derive(Clone)]
pub struct FormPreview {
    pub schema: FormSchema,
    pub suggestions: Vec<ProposedMapping>,
    pub manual_review: Vec<String>,
    pub model_review_selectors: std::collections::HashSet<String>,
}

impl FormPreview {
    pub fn repeated_label_selectors(&self) -> Vec<String> {
        self.schema
            .fields
            .iter()
            .filter(|field| {
                self.schema
                    .fields
                    .iter()
                    .filter(|other| same_review_label(&other.label, &field.label))
                    .count()
                    > 1
            })
            .map(|field| field.selector.clone())
            .collect()
    }

    /// Preserve the original schema for change detection and approval hashes;
    /// only the UI copy carries conservative model review flags.
    pub fn fields_for_review(&self) -> Vec<waypoint_applications::form_mapping::FormField> {
        self.schema
            .fields
            .iter()
            .cloned()
            .map(|mut field| {
                field.sensitive |= self.model_review_selectors.contains(&field.selector);
                field.label = field_review_label(&self.schema, &field);
                field
            })
            .collect()
    }
}

// Unsupported custom questions remain review obligations; other walls block.
pub(crate) fn is_custom_review(reason: &str) -> bool {
    reason == waypoint_browser::UNINSPECTED_FRAME_REVIEW
        || reason == waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW
        || reason == waypoint_browser::ISOLATED_FRAME_INSPECTION_UNAVAILABLE
        || reason == waypoint_browser::SHADOW_FORM_REVIEW
        || reason == waypoint_browser::CLOSED_SHADOW_FORM_REVIEW
        || reason == waypoint_browser::EMBEDDED_SCOPE_LIMIT_REVIEW
        || reason == waypoint_browser::NATIVE_DATALIST_REVIEW
        || reason.starts_with(waypoint_browser::INVALID_FIELD_REVIEW_PREFIX)
        || reason == "an unavailable field needs manual review"
        || reason == "a required hidden control needs manual review"
        || reason == "a required custom control needs manual review"
        || reason.starts_with("a required custom control needs manual review: ")
        || reason == "a custom control needs manual review"
        || reason.starts_with("a custom control needs manual review: ")
}

pub const POST_CHOICE_VISUAL_REQUIRED: &str = "Visual review required after a choice action";
pub const CHANGED_INFORMATIONAL_PREVIEW: &str = "The application changed during inspection; this view is informational only. Inspect again before filling.";

const UNRESOLVED_BUTTON_REVIEW: &str =
    "a button has an unresolved accessible name; manual review is needed";

pub fn same_reviewable_walls(preview: &FormPreview, fresh_walls: &[String]) -> bool {
    same_walls_for_operation(preview, fresh_walls, false)
}

/// Only reviewed custom-choice interaction may proceed past this one stable
/// warning. It remains a blocker for filling, navigation, and submission.
pub fn same_choice_reviewable_walls(preview: &FormPreview, fresh_walls: &[String]) -> bool {
    same_walls_for_operation(preview, fresh_walls, true)
}

fn same_walls_for_operation(
    preview: &FormPreview,
    fresh_walls: &[String],
    allow_unresolved_button: bool,
) -> bool {
    let permitted = |reason: &str| {
        is_custom_review(reason) || (allow_unresolved_button && reason == UNRESOLVED_BUTTON_REVIEW)
    };
    if preview.manual_review.iter().any(|reason| {
        reason == "captcha present"
            || reason == "ambiguous form controls need manual review"
            || reason == POST_CHOICE_VISUAL_REQUIRED
            || reason == CHANGED_INFORMATIONAL_PREVIEW
    }) || fresh_walls.iter().any(|wall| !permitted(wall))
    {
        return false;
    }
    let mut expected: Vec<_> = preview
        .manual_review
        .iter()
        .filter(|reason| permitted(reason))
        .collect();
    let mut actual: Vec<_> = fresh_walls.iter().collect();
    expected.sort();
    actual.sort();
    expected == actual
}

/// Describe commands and required fields that still need review. This is not a
/// browser validity check: the page may change values after the commands run.
pub fn fill_coverage(preview: &FormPreview) -> (Vec<String>, Vec<String>) {
    let selected: std::collections::HashSet<_> = preview
        .suggestions
        .iter()
        .map(|mapping| mapping.field_selector.as_str())
        .collect();
    let filled = preview
        .schema
        .fields
        .iter()
        .filter(|field| selected.contains(field.selector.as_str()))
        .map(|field| field_review_label(&preview.schema, field))
        .collect();
    let remaining = preview
        .schema
        .fields
        .iter()
        .filter(|field| {
            field.required
                && !preview.suggestions.iter().any(|mapping| {
                    mapping.field_selector == field.selector
                        && match &mapping.value {
                            PacketValue::Text(value) | PacketValue::Date(value) => {
                                !value.trim().is_empty()
                            }
                            PacketValue::TextList(values) => !values.is_empty(),
                            PacketValue::Bool(value) => *value,
                            PacketValue::FileRef { name } => !name.trim().is_empty(),
                            PacketValue::None => false,
                        }
                })
        })
        .map(|field| field_review_label(&preview.schema, field))
        .collect();
    (filled, remaining)
}

/// Pair each reviewed value with the original site question before any browser
/// command runs. This records requested answers, not verified page state.
pub fn requested_field_records(preview: &FormPreview) -> Result<Vec<Value>, String> {
    preview.suggestions.iter().map(|mapping| {
        let field = preview.schema.fields.iter().find(|field| field.selector == mapping.field_selector)
            .ok_or("Requested field is missing from the inspected form")?;
        Ok(serde_json::json!({"selector":field.selector,"question":field.label,"review_label":field_review_label(&preview.schema, field),"editing_mode":field.editing_mode,"value":mapping.value}))
    }).collect()
}

/// Compare display labels conservatively: changes in case and whitespace do
/// not establish that otherwise identical labels refer to different fields.
fn same_review_label(left: &str, right: &str) -> bool {
    fn normalized(label: &str) -> String {
        label
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
    normalized(left) == normalized(right)
}

/// Repeated labels are common in address sections. Preserve the site's label
/// while making each unresolved control distinguishable in review messages.
pub(crate) fn field_review_label(
    schema: &FormSchema,
    field: &waypoint_applications::form_mapping::FormField,
) -> String {
    let matching: Vec<_> = schema
        .fields
        .iter()
        .filter(|other| same_review_label(&other.label, &field.label))
        .collect();
    if matching.len() <= 1 {
        return field.label.clone();
    }
    let position = matching
        .iter()
        .position(|other| other.selector == field.selector)
        .map_or(0, |index| index + 1);
    format!("{} (field {position} of {})", field.label, matching.len())
}

pub struct FillContext<'a> {
    pub intent: &'a IntentId,
    pub packet_digest: &'a Sha256Digest,
    pub source_reviewed: bool,
    pub timestamp: i64,
    pub completed: &'a mut Vec<Value>,
}

pub type ReviewedReplacements = std::collections::BTreeMap<String, String>;

pub fn selection_snapshot_digest(
    scope: &str,
    selector: &str,
    snapshot: &waypoint_applications::driver::SelectSnapshot,
) -> String {
    Sha256Digest::of(
        &serde_json::json!({"scope":scope,"selector":selector,"selection":snapshot}).to_string(),
    )
    .as_str()
    .to_owned()
}

pub fn text_snapshot_digest(
    scope: &str,
    selector: &str,
    snapshot: &waypoint_applications::driver::TextSnapshot,
) -> String {
    Sha256Digest::of(
        &serde_json::json!({"scope":scope,"selector":selector,"text":snapshot}).to_string(),
    )
    .as_str()
    .to_owned()
}

pub fn choice_snapshot_digest(
    scope: &str,
    selector: &str,
    snapshot: &waypoint_applications::driver::ChoiceSnapshot,
) -> String {
    Sha256Digest::of(
        &serde_json::json!({"scope":scope,"selector":selector,"choice":snapshot}).to_string(),
    )
    .as_str()
    .to_owned()
}

pub fn fill_payload_digest(
    preview: &FormPreview,
    replacements: &ReviewedReplacements,
) -> Result<Sha256Digest, String> {
    let payload = if replacements.is_empty() {
        serde_json::to_string(&preview.suggestions)
    } else {
        serde_json::to_string(&serde_json::json!({"suggestions":preview.suggestions,"reviewed_replacements":replacements}))
    }.map_err(|e| e.to_string())?;
    Ok(Sha256Digest::of(&payload))
}

fn check_existing_value(
    driver: &mut dyn BrowserDriver,
    field: &waypoint_applications::form_mapping::FormField,
    value: &PacketValue,
    scope: &str,
    replacements: &ReviewedReplacements,
) -> Result<(), String> {
    if let Some(expected) = replacements.get(&field.selector) {
        let actual = match (&field.field_type, value) {
            (FormFieldType::Select { .. }, PacketValue::Text(_)) => {
                let snapshot = driver
                    .selected_option_snapshot(&field.selector)
                    .map_err(|error| error.to_string())?;
                selection_snapshot_digest(scope, &field.selector, &snapshot)
            }
            (
                FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea,
                PacketValue::Text(_),
            )
            | (
                FormFieldType::Date
                | FormFieldType::Month
                | FormFieldType::Time
                | FormFieldType::DateTimeLocal,
                PacketValue::Date(_),
            ) => {
                let snapshot = driver
                    .text_value_snapshot(&field.selector)
                    .map_err(|error| error.to_string())?;
                text_snapshot_digest(scope, &field.selector, &snapshot)
            }
            (FormFieldType::MultiSelect { .. }, PacketValue::TextList(_))
            | (FormFieldType::Radio { .. }, PacketValue::Text(_))
            | (FormFieldType::Checkbox, PacketValue::Bool(_)) => {
                let kind = match field.field_type {
                    FormFieldType::MultiSelect { .. } => "multi",
                    FormFieldType::Radio { .. } => "radio",
                    FormFieldType::Checkbox => "checkbox",
                    _ => unreachable!(),
                };
                let snapshot = driver
                    .choice_snapshot(&field.selector, kind)
                    .map_err(|error| error.to_string())?;
                choice_snapshot_digest(scope, &field.selector, &snapshot)
            }
            _ => return Err("Replacement approval does not apply to this field".into()),
        };
        if actual != *expected {
            return Err(format!(
                "{} changed since the existing value was reviewed",
                field.label
            ));
        }
        return Ok(());
    }
    match (&field.field_type, value) {
        (
            FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea,
            PacketValue::Text(text),
        )
        | (
            FormFieldType::Date
            | FormFieldType::Month
            | FormFieldType::Time
            | FormFieldType::DateTimeLocal,
            PacketValue::Date(text),
        ) => driver
            .ensure_text_replaceable(&field.selector, text)
            .map_err(|error| format!("{} needs manual review: {error}", field.label)),
        (FormFieldType::Select { .. }, PacketValue::Text(label)) => driver
            .ensure_choice_replaceable(&BrowserCommand::SelectOption {
                selector: field.selector.clone(),
                label: label.clone(),
            })
            .map_err(|error| format!("{} needs manual review: {error}", field.label)),
        (FormFieldType::MultiSelect { .. }, PacketValue::TextList(labels)) => driver
            .ensure_choice_replaceable(&BrowserCommand::SelectOptions {
                selector: field.selector.clone(),
                labels: labels.clone(),
            })
            .map_err(|error| format!("{} needs manual review: {error}", field.label)),
        (FormFieldType::Radio { .. }, PacketValue::Text(label)) => driver
            .ensure_choice_replaceable(&BrowserCommand::SelectRadio {
                selector: field.selector.clone(),
                label: label.clone(),
            })
            .map_err(|error| format!("{} needs manual review: {error}", field.label)),
        (FormFieldType::Checkbox, PacketValue::Bool(value)) => driver
            .ensure_choice_replaceable(&BrowserCommand::Fill {
                selector: field.selector.clone(),
                value: PacketValue::Bool(*value),
            })
            .map_err(|error| format!("{} needs manual review: {error}", field.label)),
        _ => Ok(()),
    }
}

/// Check the chosen answers before any fill command or approval consumption.
pub fn preflight_existing_values(
    driver: &mut dyn BrowserDriver,
    preview: &FormPreview,
    scope: &str,
    replacements: &ReviewedReplacements,
) -> Result<(), String> {
    for mapping in &preview.suggestions {
        let field = preview
            .schema
            .fields
            .iter()
            .find(|field| field.selector == mapping.field_selector)
            .ok_or("Reviewed field is unavailable")?;
        check_existing_value(driver, field, &mapping.value, scope, replacements)?;
    }
    Ok(())
}

/// Apply only the exact reviewed mappings, using one prefill grant. This path
/// has no submit or arbitrary click operation. The caller must keep the same
/// browser session and acquire permission for the actual destination first.
#[cfg(test)]
pub fn apply_reviewed_text_fields(
    ws: &mut Workspace,
    job_id: &JobIdentityId,
    driver: &mut dyn BrowserDriver,
    url: &str,
    preview: &FormPreview,
    grant: &mut ApprovalGrant,
    context: FillContext<'_>,
) -> Result<usize, String> {
    apply_reviewed_text_fields_with_replacements(
        ws,
        job_id,
        driver,
        url,
        (preview, &ReviewedReplacements::new(), None),
        grant,
        context,
    )
}

pub fn apply_reviewed_text_fields_with_replacements(
    ws: &mut Workspace,
    job_id: &JobIdentityId,
    driver: &mut dyn BrowserDriver,
    url: &str,
    review: (
        &FormPreview,
        &ReviewedReplacements,
        Option<&(String, String)>,
    ),
    grant: &mut ApprovalGrant,
    context: FillContext<'_>,
) -> Result<usize, String> {
    let (preview, replacements, route_binding) = review;
    let FillContext {
        intent,
        packet_digest,
        source_reviewed,
        timestamp,
        completed,
    } = context;
    if grant.operation != GrantOperation::Prefill {
        return Err("A prefill grant is required".into());
    }
    let job = ws
        .job(job_id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    if !matches!(
        job.state,
        ApplicationState::Shortlisted | ApplicationState::Prepared
    ) || !crate::form_destination_current(ws, &job, url, route_binding)?
    {
        return Err("Job or destination changed; review again".into());
    }
    if !source_reviewed {
        return Err("Destination review is required".into());
    }
    let fresh = driver.inspect_current().map_err(|e| e.to_string())?;
    if !same_reviewable_walls(preview, &fresh.walls)
        || fresh.schema != preview.schema
        || fresh.origin != preview.schema.origin
    {
        return Err("Form changed or requires manual attention; review again".into());
    }
    crate::form_links::check_job_and_destination(&job.source, &fresh.origin, Operation::Prefill)?;
    let mut seen = std::collections::HashSet::new();
    for mapping in &preview.suggestions {
        if !seen.insert(&mapping.field_selector) {
            return Err("Duplicate field mapping".into());
        }
        let field = fresh
            .schema
            .fields
            .iter()
            .find(|f| f.selector == mapping.field_selector)
            .ok_or("Unknown field")?;
        let valid = match (&field.field_type, &mapping.value) {
            (
                FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea,
                PacketValue::Text(text),
            ) => waypoint_applications::form_mapping::text_fits_field(field, text),
            (FormFieldType::Date, PacketValue::Date(value)) => {
                waypoint_applications::form_mapping::valid_iso_date(value)
            }
            (FormFieldType::Month, PacketValue::Date(value)) => {
                waypoint_applications::form_mapping::valid_iso_month(value)
            }
            (FormFieldType::Time, PacketValue::Date(value)) => {
                waypoint_applications::form_mapping::valid_iso_time(value)
            }
            (FormFieldType::DateTimeLocal, PacketValue::Date(value)) => {
                waypoint_applications::form_mapping::valid_iso_datetime_local(value)
            }
            (FormFieldType::Checkbox, PacketValue::Bool(_)) => true,
            (FormFieldType::MultiSelect { options }, PacketValue::TextList(values)) => {
                waypoint_applications::form_mapping::valid_multi_select(options, values)
            }
            (
                FormFieldType::Select { options } | FormFieldType::Radio { options },
                PacketValue::Text(value),
            ) => {
                options
                    .iter()
                    .filter(|option| option.trim().eq_ignore_ascii_case(value.trim()))
                    .count()
                    == 1
            }
            _ => false,
        };
        if field.sensitive || preview.model_review_selectors.contains(&field.selector) || !valid {
            return Err("Field requires manual handling".into());
        }
        check_existing_value(driver, field, &mapping.value, intent.as_str(), replacements)?;
    }
    let payload_digest = fill_payload_digest(preview, replacements)?;
    let schema_digest = Sha256Digest::of(&fresh.schema.schema_hash_input());
    grant
        .authorize(
            intent,
            &payload_digest,
            packet_digest,
            &schema_digest,
            &fresh.origin,
            false,
        )
        .map_err(|e| e.to_string())?;
    ws.record_consumed_prefill_grant(&waypoint_store::prefill_grants::PrefillGrantRecord {
        id: grant.id.as_str().to_owned(),
        job_id: job_id.as_str().to_owned(),
        intent_id: intent.as_str().to_owned(),
        payload_hash: payload_digest.as_str().to_owned(),
        profile_hash: packet_digest.as_str().to_owned(),
        schema_hash: schema_digest.as_str().to_owned(),
        destination: fresh.origin.clone(),
        created_at: timestamp,
    })
    .map_err(|e| e.to_string())?;
    for mapping in &preview.suggestions {
        let field = fresh
            .schema
            .fields
            .iter()
            .find(|f| f.selector == mapping.field_selector)
            .ok_or("Unknown field")?;
        check_existing_value(driver, field, &mapping.value, intent.as_str(), replacements)?;
        let command = match (&field.field_type, &mapping.value) {
            (FormFieldType::MultiSelect { .. }, PacketValue::TextList(labels)) => {
                BrowserCommand::SelectOptions {
                    selector: mapping.field_selector.clone(),
                    labels: labels.clone(),
                }
            }
            (FormFieldType::Select { .. }, PacketValue::Text(label)) => {
                BrowserCommand::SelectOption {
                    selector: mapping.field_selector.clone(),
                    label: label.clone(),
                }
            }
            (FormFieldType::Radio { .. }, PacketValue::Text(label)) => {
                BrowserCommand::SelectRadio {
                    selector: mapping.field_selector.clone(),
                    label: label.clone(),
                }
            }
            _ => BrowserCommand::Fill {
                selector: mapping.field_selector.clone(),
                value: mapping.value.clone(),
            },
        };
        execute_and_verify_field(driver, preview, field, &command, completed)?;
    }
    // Check again after all writes because later fields can change earlier ones.
    for mapping in &preview.suggestions {
        verify_filled_field(driver, preview, &mapping.field_selector)
            .map_err(|error| format!("After filling, {error}"))?;
    }
    Ok(preview.suggestions.len())
}

fn execute_and_verify_field(
    driver: &mut dyn BrowserDriver,
    preview: &FormPreview,
    field: &waypoint_applications::form_mapping::FormField,
    command: &BrowserCommand,
    completed: &mut Vec<Value>,
) -> Result<(), String> {
    driver.execute(command).map_err(|e| {
        format!(
            "Prefill stopped at {}; review the form: {e}",
            field_review_label(&preview.schema, field)
        )
    })?;
    completed.push(serde_json::json!({"selector":field.selector,"label":field.label}));
    verify_filled_field(driver, preview, &field.selector)
        .map_err(|error| format!("After filling, {error}"))
}

/// Bounded live-value check after navigation. A failure or budget cutoff is
/// explicit; remaining fields are not counted as verified.
pub struct CompletedFieldCheck {
    pub selected_retained: bool,
    pub verified: usize,
    pub unverified: usize,
    pub failed_labels: Vec<String>,
    /// Fixed native-constraint categories only; never an entered value.
    pub failed_details: Vec<String>,
}

pub fn verify_completed_fields(
    driver: &mut dyn BrowserDriver,
    preview: &FormPreview,
    completed: &[String],
    selected: &str,
) -> CompletedFieldCheck {
    let start = std::time::Instant::now();
    let mut checked = 0usize;
    let mut verified = 0usize;
    let mut selected_retained = false;
    let mut failed_labels = Vec::new();
    let mut failed_details = Vec::new();
    for selector in std::iter::once(selected).chain(
        completed
            .iter()
            .map(String::as_str)
            .filter(|item| *item != selected),
    ) {
        if checked >= 128 || start.elapsed() >= std::time::Duration::from_secs(8) {
            break;
        }
        checked += 1;
        let result = verify_filled_field(driver, preview, selector);
        let valid = result.is_ok();
        if selector == selected {
            selected_retained = valid;
        }
        if valid {
            verified += 1;
        } else {
            let label = preview
                .schema
                .fields
                .iter()
                .find(|field| field.selector == selector)
                .map(|field| field_review_label(&preview.schema, field))
                .unwrap_or_else(|| "Unknown field".into());
            if let Err(error) = result {
                if let Some(reason) = native_constraint_detail(&error) {
                    failed_details.push(format!("{label}: {reason}"));
                }
            }
            failed_labels.push(label);
            // A changed field or disconnected browser requires manual review;
            // do not spend the remaining budget repeatedly querying a broken page.
            break;
        }
    }
    CompletedFieldCheck {
        selected_retained,
        verified,
        unverified: completed.len().saturating_sub(checked),
        failed_labels,
        failed_details,
    }
}

fn native_constraint_detail(error: &str) -> Option<&'static str> {
    let (_, reason) =
        error.rsplit_once("A previously filled field failed a browser constraint: ")?;
    match reason {
        "a value is required" => Some("a value is required"),
        "the value does not match the expected type" => {
            Some("the value does not match the expected type")
        }
        "the value does not match the required format" => {
            Some("the value does not match the required format")
        }
        "the value is too short" => Some("the value is too short"),
        "the value is too long" => Some("the value is too long"),
        "the value is below the allowed minimum" => Some("the value is below the allowed minimum"),
        "the value is above the allowed maximum" => Some("the value is above the allowed maximum"),
        "the value does not match the allowed increment" => {
            Some("the value does not match the allowed increment")
        }
        "the browser cannot interpret the value" => Some("the browser cannot interpret the value"),
        "the site supplied a custom validation error" => {
            Some("the site supplied a custom validation error")
        }
        "the browser reports invalid input" => Some("the browser reports invalid input"),
        _ => None,
    }
}

/// Confirm a completed answer remains in the current field without changing it.
pub fn verify_filled_field(
    driver: &mut dyn BrowserDriver,
    preview: &FormPreview,
    selector: &str,
) -> Result<(), String> {
    let field = preview
        .schema
        .fields
        .iter()
        .find(|field| field.selector == selector)
        .ok_or("Completed field is no longer in the reviewed form")?;
    let mapping = preview
        .suggestions
        .iter()
        .find(|mapping| mapping.field_selector == selector)
        .ok_or("Reviewed answer is unavailable")?;
    let checked = match (&field.field_type, &mapping.value) {
        (
            FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea,
            PacketValue::Text(value),
        )
        | (
            FormFieldType::Date
            | FormFieldType::Month
            | FormFieldType::Time
            | FormFieldType::DateTimeLocal,
            PacketValue::Date(value),
        ) => driver.verify_text_value(selector, value),
        (FormFieldType::MultiSelect { .. }, PacketValue::TextList(labels)) => {
            driver.verify_choice(&BrowserCommand::SelectOptions {
                selector: selector.into(),
                labels: labels.clone(),
            })
        }
        (FormFieldType::Select { .. }, PacketValue::Text(label)) => {
            driver.verify_choice(&BrowserCommand::SelectOption {
                selector: selector.into(),
                label: label.clone(),
            })
        }
        (FormFieldType::Radio { .. }, PacketValue::Text(label)) => {
            driver.verify_choice(&BrowserCommand::SelectRadio {
                selector: selector.into(),
                label: label.clone(),
            })
        }
        (FormFieldType::Checkbox, PacketValue::Bool(value)) => {
            driver.verify_choice(&BrowserCommand::Fill {
                selector: selector.into(),
                value: PacketValue::Bool(*value),
            })
        }
        _ => {
            return Err(format!(
                "{} needs manual verification",
                field_review_label(&preview.schema, field)
            ))
        }
    };
    checked.map_err(|error| {
        format!(
            "{} needs review: {error}",
            field_review_label(&preview.schema, field)
        )
    })
}

#[cfg(test)]
pub fn inspect_and_suggest(
    driver: &mut dyn BrowserDriver,
    inference: &mut dyn Inference,
    url: &str,
    profile: &Value,
) -> Result<FormPreview, String> {
    let live = driver.open(url).map_err(|e| e.to_string())?;
    suggest_for_live_form(live, inference, profile)
}

pub fn suggest_for_live_form(
    live: waypoint_applications::driver::LiveForm,
    inference: &mut dyn Inference,
    profile: &Value,
) -> Result<FormPreview, String> {
    build_preview(live, Some(inference), profile, "")
}

pub fn suggest_without_model(
    live: waypoint_applications::driver::LiveForm,
    profile: &Value,
    reason: &str,
) -> Result<FormPreview, String> {
    build_preview(live, None, profile, reason)
}

fn build_preview(
    live: waypoint_applications::driver::LiveForm,
    inference: Option<&mut dyn Inference>,
    profile: &Value,
    no_model_reason: &str,
) -> Result<FormPreview, String> {
    let has_captcha = live.walls.iter().any(|wall| wall == "captcha present");
    let has_ambiguity = live
        .walls
        .iter()
        .any(|wall| wall == "ambiguous form controls need manual review");
    let blocked = live.schema.fields.is_empty()
        || live.walls.iter().any(|wall| {
            wall != "captcha present"
                && wall != "ambiguous form controls need manual review"
                && !is_custom_review(wall)
        });
    let mut manual_review = live.walls;
    if has_captcha && !blocked {
        manual_review.push("CAPTCHA integration detected. Suggestions are for review only; inspect the page again after resolving the obstacle before filling.".into());
    }
    if has_ambiguity && !blocked {
        manual_review.push("Some form controls are ambiguous. Suggestions are for review only; inspect the page again before filling.".into());
    }
    if live.schema.fields.is_empty() {
        manual_review.push("No form fields were detected; navigation, login, an embedded frame, or manual review may be required".into());
        if manual_review
            .iter()
            .any(|wall| wall.contains("A field may be obscured, clipped or transparent"))
        {
            manual_review.push("Some pages reveal fields as you scroll. Open the visible browser, scroll to the form and inspect again; an overlay or another issue may also be responsible.".into());
        }
    }
    if live.schema.multi_step {
        manual_review.push("A possible multi-step application was detected. Inspect each section manually; later steps and submission status are unknown".into());
    }
    let mut model_review_selectors = std::collections::HashSet::new();
    let mut suggestions = if !blocked {
        let mapped = match inference {
            Some(inference) => {
                crate::suggestions::suggest_with_review(inference, &live.schema, profile)
            }
            None => Err(no_model_reason.to_owned()),
        };
        match mapped {
            Ok((items, warnings, model_review)) => {
                model_review_selectors = model_review;
                manual_review.extend(warnings);
                items
            }
            Err(error) => {
                manual_review.push(format!(
                    "{error}; only exact labels or standard autocomplete hints are suggested"
                ));
                if crate::suggestions::missing_full_name_for_form(&live.schema, profile) {
                    manual_review.push("This form asks for a full name, but your profile has no full name for applications. Enter it in Personal details or handle the field manually.".into());
                }
                match crate::suggestions::suggest_exact(&live.schema, profile) {
                    Ok(items) => items,
                    Err(fallback_error) => {
                        manual_review.push(format!(
                            "Automatic field suggestions are unavailable: {fallback_error}. Continue manually."
                        ));
                        Vec::new()
                    }
                }
            }
        }
    } else {
        vec![]
    };
    suggestions.retain(|mapping| {
        let Some(field) = live
            .schema
            .fields
            .iter()
            .find(|field| field.selector == mapping.field_selector)
        else {
            return true;
        };
        if let PacketValue::Text(text) = &mapping.value {
            if matches!(
                field.field_type,
                FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea
            ) && !waypoint_applications::form_mapping::text_fits_field(field, text)
            {
                let too_short = !text.is_empty()
                    && field
                        .min_length_utf16
                        .is_some_and(|minimum| text.encode_utf16().count() < minimum as usize);
                let reason = if too_short {
                    "Suggested value is shorter than field minimum: "
                } else {
                    "Suggested value exceeds field limit: "
                };
                manual_review.push(format!("{reason}{}", field.label));
                return false;
            }
        }
        true
    });
    let mut preview = FormPreview {
        schema: live.schema,
        suggestions,
        manual_review,
        model_review_selectors,
    };
    refresh_required_review(&mut preview);
    Ok(preview)
}

pub fn refresh_required_review(preview: &mut FormPreview) {
    let field_warnings: std::collections::HashSet<_> = preview
        .schema
        .fields
        .iter()
        .filter(|field| field.required)
        .flat_map(|field| {
            [
                format!("Required field needs review: {}", field.label),
                format!(
                    "Required field needs review: {}",
                    field_review_label(&preview.schema, field)
                ),
            ]
        })
        .collect();
    preview.manual_review.retain(|reason| {
        // The aggregate suggestion diagnostic is replaced by the field-specific
        // coverage below, which can be refreshed after a user edits a mapping.
        if reason.ends_with(" required field(s) have no suggestion and need manual review") {
            return false;
        }
        if field_warnings.contains(reason) {
            return false;
        }
        // Match the entire generated warning to a field, then require every
        // same-label field to be answered before clearing it.
        if let Some(field) = preview.schema.fields.iter().find(|field| {
            *reason == crate::suggestions::invalid_suggestion_warning(field)
                || *reason
                    == format!(
                        "Invalid or duplicate suggestion needs review: {}",
                        field.label
                    )
        }) {
            let all_answered = preview
                .schema
                .fields
                .iter()
                .filter(|other| other.label == field.label)
                .all(|other| {
                    preview
                        .suggestions
                        .iter()
                        .any(|mapping| mapping.field_selector == other.selector)
                });
            if all_answered {
                return false;
            }
        }
        for prefix in [
            "Conflicting suggestions need review: ",
            "Suggested value exceeds field limit: ",
            "Suggested value is shorter than field minimum: ",
        ] {
            if let Some(label) = reason.strip_prefix(prefix) {
                let fields: Vec<_> = preview
                    .schema
                    .fields
                    .iter()
                    .filter(|field| field.label == label)
                    .collect();
                if !fields.is_empty()
                    && fields.iter().all(|field| {
                        preview
                            .suggestions
                            .iter()
                            .any(|mapping| mapping.field_selector == field.selector)
                    })
                {
                    return false;
                }
            }
        }
        true
    });
    let (_, required) = fill_coverage(preview);
    preview.manual_review.extend(
        required
            .into_iter()
            .map(|label| format!("Required field needs review: {label}")),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use waypoint_applications::driver::{DriverError, LiveForm, SiteAck};
    use waypoint_applications::form_mapping::BrowserCommand;
    use waypoint_inference::{CapabilityReport, InferenceError, UsageReport};

    #[test]
    fn source_route_prefill_rechecks_binding_before_writing() {
        struct FixtureDriver {
            live: LiveForm,
            written: String,
        }
        impl BrowserDriver for FixtureDriver {
            fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
                Ok(self.live.clone())
            }
            fn inspect_current(&mut self) -> Result<LiveForm, DriverError> {
                Ok(self.live.clone())
            }
            fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
                if let BrowserCommand::Fill {
                    value: PacketValue::Text(value),
                    ..
                } = command
                {
                    self.written = value.clone();
                    Ok(())
                } else {
                    Err(DriverError("unexpected command".into()))
                }
            }
            fn ensure_text_replaceable(&mut self, _: &str, _: &str) -> Result<(), DriverError> {
                Ok(())
            }
            fn verify_text_value(&mut self, _: &str, expected: &str) -> Result<(), DriverError> {
                if self.written == expected {
                    Ok(())
                } else {
                    Err(DriverError("mismatch".into()))
                }
            }
            fn commit(&mut self) -> Result<SiteAck, DriverError> {
                panic!("must not submit")
            }
        }
        let listing = "https://jobs.ashbyhq.com/ashby/fixture";
        let route = "https://jobs.ashbyhq.com/ashby/fixture/application";
        let board = "https://api.ashbyhq.com/posting-api/job-board/ashby";
        let body = format!(
            r#"{{"jobs":[{{"id":"fixture","title":"Engineer","jobUrl":"{listing}","applyUrl":"{route}"}}]}}"#
        );
        let mut ws = Workspace::in_memory().unwrap();
        let target = [waypoint_workspace::DiscoveryTarget::new("ashby", "ashby")];
        let fetcher = waypoint_workspace::StaticFetcher::default().route(board, &body);
        ws.discover(&target, &fetcher, 100);
        let job = ws.jobs().unwrap().remove(0);
        ws.set_state(&job.id, ApplicationState::Shortlisted)
            .unwrap();
        let field = waypoint_applications::form_mapping::FormField {
            selector: "#name".into(),
            label: "Name".into(),
            field_type: FormFieldType::Text,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        };
        let schema = FormSchema {
            origin: "https://jobs.ashbyhq.com".into(),
            fields: vec![field],
            multi_step: false,
            steps: 1,
        };
        let preview = FormPreview {
            schema: schema.clone(),
            suggestions: vec![ProposedMapping {
                field_selector: "#name".into(),
                value: PacketValue::Text("Fictional".into()),
            }],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let mut driver = FixtureDriver {
            live: LiveForm {
                origin: schema.origin.clone(),
                schema: schema.clone(),
                walls: vec![],
            },
            written: String::new(),
        };
        let intent = IntentId::new("fixture-route").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: waypoint_domain::GrantId::new("fixture-route-grant").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: fill_payload_digest(&preview, &ReviewedReplacements::new())
                .unwrap(),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&schema.schema_hash_input()),
            destination_origin: schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let binding = ("ashby".to_owned(), listing.to_owned());
        let changed = body.replace(route, "https://jobs.ashbyhq.com/ashby/fixture/new");
        let fetcher = waypoint_workspace::StaticFetcher::default().route(board, &changed);
        ws.discover(&target, &fetcher, 101);
        let mut completed = Vec::new();
        let mut attempt =
            |ws: &mut Workspace, driver: &mut FixtureDriver, grant: &mut ApprovalGrant| {
                apply_reviewed_text_fields_with_replacements(
                    ws,
                    &job.id,
                    driver,
                    route,
                    (&preview, &ReviewedReplacements::new(), Some(&binding)),
                    grant,
                    FillContext {
                        intent: &intent,
                        packet_digest: &packet,
                        source_reviewed: true,
                        timestamp: 102,
                        completed: &mut completed,
                    },
                )
            };
        assert!(attempt(&mut ws, &mut driver, &mut grant).is_err());
        assert!(driver.written.is_empty());
        assert!(!grant.consumed);
        let fetcher = waypoint_workspace::StaticFetcher::default().route(board, &body);
        ws.discover(&target, &fetcher, 103);
        assert_eq!(attempt(&mut ws, &mut driver, &mut grant).unwrap(), 1);
        assert_eq!(driver.written, "Fictional");
    }

    #[test]
    fn replacement_approval_binds_session_selection_and_fill_payload() {
        let snapshot = waypoint_applications::driver::SelectSnapshot {
            index: 1,
            label: "Alpha".into(),
            value: "a".into(),
        };
        let digest = selection_snapshot_digest("prefill-session-a", "#choice", &snapshot);
        assert_ne!(
            digest,
            selection_snapshot_digest("prefill-session-b", "#choice", &snapshot)
        );
        assert_ne!(
            digest,
            selection_snapshot_digest("prefill-session-a", "#other", &snapshot)
        );
        let text = waypoint_applications::driver::TextSnapshot {
            value: "Old answer".into(),
        };
        assert_ne!(
            text_snapshot_digest("prefill-session-a", "#field", &text),
            text_snapshot_digest("prefill-session-b", "#field", &text)
        );
        let checkbox = waypoint_applications::driver::ChoiceSnapshot {
            kind: "checkbox".into(),
            checked: Some(true),
            selections: vec![],
        };
        let changed = waypoint_applications::driver::ChoiceSnapshot {
            checked: Some(false),
            ..checkbox.clone()
        };
        assert_ne!(
            choice_snapshot_digest("prefill-session-a", "#field", &checkbox),
            choice_snapshot_digest("prefill-session-a", "#field", &changed)
        );
        assert_ne!(
            choice_snapshot_digest("prefill-session-a", "#field", &checkbox),
            choice_snapshot_digest("prefill-session-b", "#field", &checkbox)
        );
        let preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![ProposedMapping {
                field_selector: "#choice".into(),
                value: PacketValue::Text("Beta".into()),
            }],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let plain = fill_payload_digest(&preview, &ReviewedReplacements::new()).unwrap();
        let mut approved = ReviewedReplacements::new();
        approved.insert("#choice".into(), digest);
        let bound = fill_payload_digest(&preview, &approved).unwrap();
        assert_ne!(plain, bound);
        approved.insert("#choice".into(), "0".repeat(64));
        assert_ne!(bound, fill_payload_digest(&preview, &approved).unwrap());
    }

    #[test]
    #[ignore = "starts a local browser against fictional preselected choices"]
    fn existing_choices_are_not_replaced_without_review() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-existing-choices.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(path).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        let select = |selector: &str, label: &str| BrowserCommand::SelectOption {
            selector: selector.into(),
            label: label.into(),
        };
        let multi = |selector: &str, label: &str| BrowserCommand::SelectOptions {
            selector: selector.into(),
            labels: vec![label.into()],
        };
        let radio = |selector: &str, label: &str| BrowserCommand::SelectRadio {
            selector: selector.into(),
            label: label.into(),
        };
        let checkbox = |selector: &str, value: bool| BrowserCommand::Fill {
            selector: selector.into(),
            value: PacketValue::Bool(value),
        };
        assert!(driver
            .ensure_choice_replaceable(&select("#empty", "Beta"))
            .is_ok());
        assert!(driver
            .ensure_choice_replaceable(&select("#chosen", "Alpha"))
            .is_ok());
        assert!(driver
            .ensure_choice_replaceable(&select("#chosen", "Beta"))
            .is_err());
        assert!(driver
            .ensure_choice_replaceable(&multi("#multi", "Alpha"))
            .is_ok());
        assert!(driver
            .ensure_choice_replaceable(&multi("#multi", "Beta"))
            .is_err());
        assert!(driver
            .ensure_choice_replaceable(&multi("#empty-multi", "Beta"))
            .is_ok());
        assert!(driver
            .ensure_choice_replaceable(&radio("#r1", "Alpha"))
            .is_ok());
        assert!(driver
            .ensure_choice_replaceable(&radio("#r1", "Beta"))
            .is_err());
        assert!(driver
            .ensure_choice_replaceable(&radio("#r3", "Beta"))
            .is_ok());
        assert!(driver
            .ensure_choice_replaceable(&checkbox("#checked", true))
            .is_ok());
        assert!(driver
            .ensure_choice_replaceable(&checkbox("#checked", false))
            .is_err());
        assert!(driver
            .ensure_choice_replaceable(&checkbox("#unchecked", true))
            .is_ok());
        let old_multi = driver.choice_snapshot("#multi", "multi").unwrap();
        assert_eq!(old_multi.selections[0].label, "Alpha");
        let old_radio = driver.choice_snapshot("#r1", "radio").unwrap();
        assert_eq!(old_radio.selections[0].label, "Alpha");
        assert_eq!(
            driver
                .choice_snapshot("#checked", "checkbox")
                .unwrap()
                .checked,
            Some(true)
        );
        driver.execute(&multi("#multi", "Beta")).unwrap();
        driver.execute(&radio("#r1", "Beta")).unwrap();
        assert_ne!(
            old_multi,
            driver.choice_snapshot("#multi", "multi").unwrap()
        );
        assert_ne!(old_radio, driver.choice_snapshot("#r1", "radio").unwrap());
        assert!(driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a local browser against a fictional prefilled form"]
    fn conflicting_existing_text_stops_before_any_field_is_changed() {
        use waypoint_domain::GrantId;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-existing-value.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(path).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        assert!(driver
            .ensure_text_replaceable("#first", "New response")
            .is_ok());
        assert!(driver
            .ensure_text_replaceable("#second", "Existing answer")
            .is_ok());
        assert!(driver
            .ensure_text_replaceable("#second", "Different answer")
            .is_err());
        assert!(driver
            .ensure_text_replaceable("#additional", "New response")
            .is_ok());
        assert!(driver
            .ensure_text_replaceable("#editable", "Existing note")
            .is_ok());
        assert_eq!(
            driver.text_value_snapshot("#editable").unwrap().value,
            "Existing note"
        );
        assert!(driver
            .ensure_text_replaceable("#editable", "Different note")
            .is_err());
        let preview = FormPreview {
            schema: live.schema,
            suggestions: vec![
                ProposedMapping {
                    field_selector: "#first".into(),
                    value: PacketValue::Text("New response".into()),
                },
                ProposedMapping {
                    field_selector: "#second".into(),
                    value: PacketValue::Text("Different answer".into()),
                },
            ],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let mut ws = Workspace::in_memory().unwrap();
        let job = JobIdentityId::new("existing-value-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job.clone(),
            title: "Fictional role".into(),
            employer: "Example".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.clone()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("existing-value-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("existing-value-grant").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fictional".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        let error = apply_reviewed_text_fields(
            &mut ws,
            &job,
            &mut driver,
            &url,
            &preview,
            &mut grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 1,
                completed: &mut completed,
            },
        )
        .unwrap_err();
        assert!(error.contains("different value"), "{error}");
        assert!(!grant.consumed);
        assert!(completed.is_empty());
        assert!(driver.ensure_text_replaceable("#first", "").is_ok());
        driver
            .verify_text_value("#second", "Existing answer")
            .unwrap();
        assert!(driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }

    #[test]
    fn completed_field_check_reports_budget_and_stops_after_failure() {
        struct CheckingDriver {
            failed: Option<String>,
        }
        impl BrowserDriver for CheckingDriver {
            fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
                unreachable!()
            }
            fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
                unreachable!()
            }
            fn commit(&mut self) -> Result<SiteAck, DriverError> {
                unreachable!()
            }
            fn verify_text_value(&mut self, selector: &str, _: &str) -> Result<(), DriverError> {
                if self.failed.as_deref() == Some(selector) {
                    Err(DriverError("A previously filled field failed a browser constraint: the value does not match the required format".into()))
                } else {
                    Ok(())
                }
            }
        }
        let fields: Vec<_> = (0..130)
            .map(|i| waypoint_applications::form_mapping::FormField {
                selector: format!("#field-{i}"),
                label: format!("Field {i}"),
                field_type: FormFieldType::Text,
                required: false,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                min_length_utf16: None,
                pattern: None,
                autocomplete: None,
            })
            .collect();
        let suggestions = fields
            .iter()
            .map(|field| ProposedMapping {
                field_selector: field.selector.clone(),
                value: PacketValue::Text("fictional".into()),
            })
            .collect();
        let completed: Vec<_> = fields.iter().map(|field| field.selector.clone()).collect();
        let preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.invalid".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions,
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let limited = verify_completed_fields(
            &mut CheckingDriver { failed: None },
            &preview,
            &completed,
            "#field-0",
        );
        assert!(limited.selected_retained);
        assert_eq!(limited.verified, 128);
        assert_eq!(limited.unverified, 2);
        assert!(limited.failed_labels.is_empty());
        let failed = verify_completed_fields(
            &mut CheckingDriver {
                failed: Some("#field-1".into()),
            },
            &preview,
            &completed,
            "#field-0",
        );
        assert!(failed.selected_retained);
        assert_eq!(failed.verified, 1);
        assert_eq!(failed.unverified, 128);
        assert_eq!(failed.failed_labels, vec!["Field 1"]);
        assert_eq!(
            failed.failed_details,
            vec!["Field 1: the value does not match the required format"]
        );
        assert!(native_constraint_detail(
            "A previously filled field failed a browser constraint: private-fixture"
        )
        .is_none());
    }

    #[test]
    #[ignore = "starts a local browser against a fictional form without submitting"]
    fn long_form_completed_field_review_checks_all_fields() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-long-review.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let completed: Vec<_> = live
            .schema
            .fields
            .iter()
            .map(|field| field.selector.clone())
            .collect();
        assert_eq!(completed.len(), 40);
        let suggestions = completed
            .iter()
            .map(|selector| ProposedMapping {
                field_selector: selector.clone(),
                value: PacketValue::Text("fictional".into()),
            })
            .collect();
        let preview = FormPreview {
            schema: live.schema,
            suggestions,
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let check = driver.with_review_timeout(|driver| {
            verify_completed_fields(driver, &preview, &completed, &completed[0])
        });
        assert!(check.selected_retained);
        assert_eq!(check.verified, 40);
        assert_eq!(check.unverified, 0);
        assert!(check.failed_labels.is_empty());
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a local browser against a fictional form without submitting"]
    fn later_field_native_validation_reason_reaches_completed_review() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_native_constraint.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let fill = |selector: &str, value: &str| BrowserCommand::Fill {
            selector: selector.into(),
            value: PacketValue::Text(value.into()),
        };
        driver.execute(&fill("#code", "AB-123")).unwrap();
        driver
            .execute(&fill("#email", "fictional@example.test"))
            .unwrap();
        let preview = FormPreview {
            schema: live.schema,
            suggestions: vec![
                ProposedMapping {
                    field_selector: "#code".into(),
                    value: PacketValue::Text("AB-123".into()),
                },
                ProposedMapping {
                    field_selector: "#email".into(),
                    value: PacketValue::Text("fictional@example.test".into()),
                },
            ],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let result = verify_completed_fields(
            &mut driver,
            &preview,
            &["#code".into(), "#email".into()],
            "#email",
        );
        assert!(result.selected_retained);
        assert_eq!(result.verified, 1);
        assert_eq!(result.failed_labels, vec!["Fictional reference"]);
        assert_eq!(
            result.failed_details,
            vec!["Fictional reference: the site supplied a custom validation error"]
        );
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
    }

    #[test]
    fn failed_immediate_verification_stops_later_field_commands() {
        struct RejectingDriver {
            written: Vec<String>,
        }
        impl BrowserDriver for RejectingDriver {
            fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
                unreachable!()
            }
            fn execute(&mut self, command: &BrowserCommand) -> Result<(), DriverError> {
                if let BrowserCommand::Fill { selector, .. } = command {
                    self.written.push(selector.clone());
                }
                Ok(())
            }
            fn verify_text_value(&mut self, _: &str, _: &str) -> Result<(), DriverError> {
                Err(DriverError("site rejected value".into()))
            }
            fn commit(&mut self) -> Result<SiteAck, DriverError> {
                unreachable!()
            }
        }
        let fields: Vec<_> = ["first", "second"]
            .iter()
            .map(|name| waypoint_applications::form_mapping::FormField {
                selector: format!("#{name}"),
                label: "Name".into(),
                field_type: FormFieldType::Text,
                required: true,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                min_length_utf16: None,
                pattern: None,
                autocomplete: None,
            })
            .collect();
        let suggestions = fields
            .iter()
            .map(|field| ProposedMapping {
                field_selector: field.selector.clone(),
                value: PacketValue::Text("Example".into()),
            })
            .collect();
        let preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.invalid".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions,
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let mut driver = RejectingDriver { written: vec![] };
        let mut completed = vec![];
        let outcome: Result<(), String> = preview.suggestions.iter().try_for_each(|mapping| {
            let field = preview
                .schema
                .fields
                .iter()
                .find(|field| field.selector == mapping.field_selector)
                .unwrap();
            let command = BrowserCommand::Fill {
                selector: mapping.field_selector.clone(),
                value: mapping.value.clone(),
            };
            execute_and_verify_field(&mut driver, &preview, field, &command, &mut completed)
        });
        assert!(outcome.unwrap_err().contains("Name (field 1 of 2)"));
        assert_eq!(driver.written, vec!["#first"]);
        assert_eq!(completed.len(), 1);
    }

    #[test]
    fn failed_fill_reports_field_and_constraint_without_marking_it_complete() {
        struct RejectingDriver;
        impl BrowserDriver for RejectingDriver {
            fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
                unreachable!()
            }
            fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
                Err(DriverError(
                    "the value does not match the required format".into(),
                ))
            }
            fn commit(&mut self) -> Result<SiteAck, DriverError> {
                unreachable!()
            }
        }
        let field = waypoint_applications::form_mapping::FormField {
            selector: "#code".into(),
            label: "Reference code".into(),
            field_type: FormFieldType::Text,
            required: false,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        };
        let preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields: vec![field.clone()],
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let mut completed = vec![];
        let error = execute_and_verify_field(
            &mut RejectingDriver,
            &preview,
            &field,
            &BrowserCommand::Fill {
                selector: "#code".into(),
                value: PacketValue::Text("private-fixture".into()),
            },
            &mut completed,
        )
        .unwrap_err();
        assert!(error.contains("Reference code"));
        assert!(error.contains("required format"));
        assert!(!error.contains("private-fixture"));
        assert!(completed.is_empty());
    }

    #[test]
    fn optional_custom_choice_is_a_reviewable_obligation() {
        assert!(is_custom_review("a custom control needs manual review"));
        assert!(is_custom_review(
            "a required hidden control needs manual review"
        ));
        assert!(is_custom_review(
            "a custom control needs manual review: City"
        ));
        assert!(is_custom_review("an unavailable field needs manual review"));
        assert!(is_custom_review(waypoint_browser::UNINSPECTED_FRAME_REVIEW));
        assert!(is_custom_review(
            waypoint_browser::INACCESSIBLE_FRAME_FORM_REVIEW
        ));
        assert!(is_custom_review(
            waypoint_browser::ISOLATED_FRAME_INSPECTION_UNAVAILABLE
        ));
        assert!(is_custom_review(waypoint_browser::SHADOW_FORM_REVIEW));
        assert!(is_custom_review(
            waypoint_browser::CLOSED_SHADOW_FORM_REVIEW
        ));
        assert!(!is_custom_review(
            "a required field is disabled or read-only"
        ));
        assert!(is_custom_review(
            waypoint_browser::EMBEDDED_SCOPE_LIMIT_REVIEW
        ));
        assert!(is_custom_review(waypoint_browser::NATIVE_DATALIST_REVIEW));
        assert!(!is_custom_review(
            "A field format pattern exceeds the inspection limit; review it manually"
        ));
        assert!(!is_custom_review("captcha present"));
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_local_model_fills_and_visually_checks_rich_textbox() {
        use waypoint_domain::GrantId;
        let addr = "127.0.0.1:11434".parse().unwrap();
        assert_eq!(
            waypoint_inference::ollama::resident_context(addr, "qwen3.5:9b").unwrap(),
            4096
        );
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-rich-model.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_resident_context();
        let mut llm = crate::resident_inference::ResidentInference::new(
            llm,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let preview =
            inspect_and_suggest(&mut driver, &mut llm, &url, &json!({"first_name":"Avery"}))
                .unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert_eq!(
            requested_field_records(&preview).unwrap()[0]["editing_mode"],
            "rich_text"
        );
        let selector = preview.suggestions[0].field_selector.clone();
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("rich-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Fictional role".into(),
            employer: "Fixture".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.clone()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("rich-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("rich-grant").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fictional".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        assert_eq!(
            apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut driver,
                &url,
                &preview,
                &mut grant,
                FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut completed
                }
            )
            .unwrap(),
            1
        );
        driver.verify_text_value(&selector, "Avery").unwrap();
        let visual = crate::visual_review::analyze(
            addr,
            "qwen3.5:9b",
            &driver.screenshot_base64().unwrap(),
            &[("First name".into(), "Avery".into())],
        )
        .unwrap();
        println!("fictional rich textbox visual review: {visual}");
        assert_eq!(visual["comparisons"][0]["status"], "matches_transcription");
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_local_model_prefills_and_visually_checks_fictional_form() {
        exercise_local_model_form_pipeline(false, false, false);
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_local_model_detects_a_changed_fictional_field() {
        exercise_local_model_form_pipeline(true, false, false);
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_local_model_prefills_and_visually_checks_multilingual_form() {
        exercise_local_model_form_pipeline(false, true, false);
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn local_model_fills_and_visually_checks_source_bound_fictional_form() {
        exercise_local_model_form_pipeline(false, false, true);
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_model_review_flags_block_forged_prefill_on_fictional_form() {
        use waypoint_domain::GrantId;
        let addr = "127.0.0.1:11434".parse().unwrap();
        assert_eq!(
            waypoint_inference::ollama::resident_context(addr, "qwen3.5:9b").unwrap(),
            4096
        );
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-model-sensitive-portuguese.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_resident_context();
        let mut llm = crate::resident_inference::ResidentInference::new(
            llm,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
        let preview = inspect_and_suggest(
            &mut driver,
            &mut llm,
            &url,
            &json!({"email":"avery@example.invalid"}),
        )
        .unwrap();
        println!(
            "fictional model flags: {:?}; mappings: {:?}; review: {:?}",
            preview.model_review_selectors, preview.suggestions, preview.manual_review
        );
        assert!(preview.model_review_selectors.contains("#candidate"));
        assert!(preview.model_review_selectors.contains("#q3"));
        assert!(
            !preview
                .schema
                .fields
                .iter()
                .find(|field| field.selector == "#candidate")
                .unwrap()
                .sensitive
        );
        assert!(
            preview
                .fields_for_review()
                .iter()
                .find(|field| field.selector == "#candidate")
                .unwrap()
                .sensitive
        );
        assert_eq!(preview.suggestions.len(), 1);
        assert_eq!(preview.suggestions[0].field_selector, "#contact");
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("sensitive-portuguese-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Fictional role".into(),
            employer: "Fixture".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.clone()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("sensitive-portuguese-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut forged = preview.clone();
        forged.suggestions.push(ProposedMapping {
            field_selector: "#candidate".into(),
            value: PacketValue::Text("avery@example.invalid".into()),
        });
        let grant_for = |preview: &FormPreview, id: &str| ApprovalGrant {
            id: GrantId::new(id).unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fictional".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut forged_grant = grant_for(&forged, "forged-review-grant");
        let mut completed = Vec::new();
        assert!(apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            &url,
            &forged,
            &mut forged_grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 1,
                completed: &mut completed
            }
        )
        .is_err());
        assert!(!forged_grant.consumed);
        assert!(completed.is_empty());
        let mut grant = grant_for(&preview, "valid-review-grant");
        assert_eq!(
            apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut driver,
                &url,
                &preview,
                &mut grant,
                FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut completed
                }
            )
            .unwrap(),
            1
        );
        driver
            .verify_text_value("#contact", "avery@example.invalid")
            .unwrap();
        driver.verify_text_value("#candidate", "").unwrap();
        driver
            .verify_choice(&BrowserCommand::Fill {
                selector: "#q3".into(),
                value: PacketValue::Bool(false),
            })
            .unwrap();
        let visual = crate::visual_review::analyze(
            addr,
            "qwen3.5:9b",
            &driver.screenshot_base64().unwrap(),
            &[("Endereço de e-mail".into(), "avery@example.invalid".into())],
        )
        .unwrap();
        println!("fictional Portuguese visual review: {visual}");
        assert_eq!(visual["comparisons"][0]["status"], "matches_transcription");
        for (label, expected) in [
            ("Tem autorização legal para trabalhar neste país?", ""),
            (
                "Concorda com o tratamento dos seus dados pessoais?",
                "unchecked",
            ),
        ] {
            let matching: Vec<_> = visual["observations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["label"] == label)
                .collect();
            assert_eq!(matching.len(), 1, "visual label {label}");
            assert_eq!(
                matching[0]["visible_value"], expected,
                "visual value for {label}"
            );
        }
        assert_eq!(
            driver.read_attribute("body", "data-submitted").unwrap(),
            None
        );
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "resource-gated local model and fictional multi-step form"]
    fn resource_gated_local_model_wizard() {
        crate::experience_inference::tests::with_resource_gated_model(|| {
            live_local_model_reinspects_and_prefills_a_fictional_wizard();
        });
    }

    #[test]
    #[ignore = "starts a local browser and requires a preloaded local model and resource preflight"]
    fn live_local_model_reinspects_and_prefills_a_fictional_wizard() {
        use waypoint_domain::GrantId;
        let model = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL")
            .unwrap_or_else(|_| "qwen3.5:9b".into());
        let (addr, context) = crate::visual_review::benchmark_target(&model).unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-local-model-wizard.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let llm =
            waypoint_inference::OllamaInference::new(&format!("http://{addr}"), &model).unwrap();
        let llm = if let Some(context) = context {
            llm.with_context_window(context).unwrap()
        } else {
            llm.with_resident_context()
        };
        let checked_model = model.clone();
        let mut llm =
            crate::resident_inference::ResidentInference::new(llm, model.clone(), move || {
                crate::visual_review::benchmark_target(&checked_model)
                    .map(|_| Some(checked_model.clone()))
            });
        let profile = json!({"first_name":"Avery", "email":"avery@example.invalid"});
        let first = inspect_and_suggest(&mut driver, &mut llm, &url, &profile).unwrap();
        assert!(
            !first
                .manual_review
                .iter()
                .any(|reason| reason.contains("only exact labels")),
            "first step fell back without model mapping: {:?}",
            first.manual_review
        );
        assert!(first.schema.multi_step);
        assert_eq!(
            first
                .schema
                .fields
                .iter()
                .find(|field| field.selector == "#given")
                .unwrap()
                .pattern
                .as_deref(),
            Some("[A-Za-z]+")
        );
        assert_eq!(
            first
                .schema
                .fields
                .iter()
                .find(|field| field.selector == "#given")
                .unwrap()
                .min_length_utf16,
            Some(2)
        );
        assert!(first
            .manual_review
            .iter()
            .any(|reason| reason.contains("multi-step")));
        assert_eq!(first.suggestions.len(), 1);
        assert_eq!(first.suggestions[0].field_selector, "#given");
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("wizard-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Fictional role".into(),
            employer: "Fixture".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.clone()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("wizard-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut completed = Vec::new();
        let mut fill =
            |driver: &mut waypoint_browser::CdpDriver, preview: &FormPreview, grant_id: &str| {
                let mut grant = ApprovalGrant {
                    id: GrantId::new(grant_id).unwrap(),
                    intent_id: intent.clone(),
                    bound_payload_sha256: Sha256Digest::of(
                        &serde_json::to_string(&preview.suggestions).unwrap(),
                    ),
                    profile_revision: 1,
                    packet_sha256: packet.clone(),
                    form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
                    destination_origin: preview.schema.origin.clone(),
                    operation: GrantOperation::Prefill,
                    nonce: "fictional".into(),
                    explicit_user_gesture: true,
                    consumed: false,
                    expired: false,
                };
                apply_reviewed_text_fields(
                    &mut ws,
                    &job_id,
                    driver,
                    &url,
                    preview,
                    &mut grant,
                    FillContext {
                        intent: &intent,
                        packet_digest: &packet,
                        source_reviewed: true,
                        timestamp: 1,
                        completed: &mut completed,
                    },
                )
            };
        assert_eq!(fill(&mut driver, &first, "wizard-first").unwrap(), 1);
        driver.verify_text_value("#given", "Avery").unwrap();
        let visual_first = crate::visual_review::benchmark_analyze(
            &model,
            &driver.screenshot_base64().unwrap(),
            &[("Your given name".into(), "Avery".into())],
        )
        .unwrap();
        assert_eq!(
            visual_first["comparisons"][0]["status"],
            "matches_transcription"
        );
        // This click simulates a user's navigation gesture on this local fixture.
        // It does not implement automatic next-button navigation in the app.
        driver
            .execute(&BrowserCommand::Click {
                selector: "#next".into(),
            })
            .unwrap();
        let live = driver.inspect_current().unwrap();
        assert_ne!(first.schema, live.schema);
        let second = suggest_for_live_form(live, &mut llm, &profile).unwrap();
        assert!(
            !second
                .manual_review
                .iter()
                .any(|reason| reason.contains("only exact labels")),
            "second step fell back without model mapping: {:?}",
            second.manual_review
        );
        println!(
            "Fictional second-step fields: {:?}; suggestions: {:?}; review: {:?}",
            second.schema.fields, second.suggestions, second.manual_review
        );
        assert_eq!(second.suggestions.len(), 1);
        assert_eq!(second.suggestions[0].field_selector, "#email");
        assert!(second
            .schema
            .fields
            .iter()
            .any(|field| field.selector == "#permit" && field.sensitive));
        assert_eq!(fill(&mut driver, &second, "wizard-second").unwrap(), 1);
        driver
            .verify_text_value("#email", "avery@example.invalid")
            .unwrap();
        driver.verify_text_value("#permit", "").unwrap();
        let visual_second = crate::visual_review::benchmark_analyze(
            &model,
            &driver.screenshot_base64().unwrap(),
            &[("Email address".into(), "avery@example.invalid".into())],
        )
        .unwrap();
        println!("wizard visual stage one: {visual_first}; stage two: {visual_second}");
        assert_eq!(
            visual_second["comparisons"][0]["status"],
            "matches_transcription"
        );
        assert!(visual_second["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "Work authorization" && item["visible_value"] == ""));
        assert_eq!(
            driver.read_attribute("body", "data-submitted").unwrap(),
            None
        );
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    fn exercise_local_model_form_pipeline(altered: bool, multilingual: bool, source_bound: bool) {
        use base64::Engine;
        use waypoint_domain::GrantId;
        struct FictionalTrace<I>(I);
        impl<I: Inference> Inference for FictionalTrace<I> {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                self.0.probe()
            }
            fn generate_structured(
                &mut self,
                schema: &str,
                prompt: &str,
                max: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                println!("fictional model prompt ({schema}): {prompt}");
                let result = self.0.generate_structured(schema, prompt, max);
                match &result {
                    Ok((value, _)) => println!("fictional model response ({schema}): {value}"),
                    Err(error) => println!("fictional model response ({schema}) failed: {error}"),
                }
                result
            }
        }
        let addr = "127.0.0.1:11434".parse().unwrap();
        let model = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL")
            .unwrap_or_else(|_| "qwen3.5:9b".into());
        assert_eq!(
            waypoint_inference::ollama::resident_context(addr, &model).unwrap(),
            4096
        );
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if multilingual {
            "../../crates/waypoint_browser/tests/fixtures/form_multilingual_review.html"
        } else if altered {
            "tests/form-local-model-partial.html"
        } else {
            "tests/form-local-model-pipeline.html"
        });
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap())
            .unwrap()
            .to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", &model)
            .unwrap()
            .with_resident_context();
        let llm = crate::resident_inference::ResidentInference::new(
            llm,
            model.clone(),
            crate::resident_model,
        );
        let mut llm = FictionalTrace(llm);
        let preview = inspect_and_suggest(&mut driver, &mut llm, &url, &json!({"first_name":"Avery", "email":"avery@example.invalid", "work_authorization":"unknown"})).unwrap();
        let given_selector = if multilingual { "#first" } else { "#given" };
        let given_label = if multilingual {
            "Vorname"
        } else {
            "Your given name"
        };
        let email_label = if multilingual {
            "E-Mail-Adresse"
        } else {
            "Email address"
        };
        println!(
            "fictional preview: mappings={:?}; review={:?}",
            preview.suggestions, preview.manual_review
        );
        assert!(preview
            .suggestions
            .iter()
            .any(|item| item.field_selector == given_selector
                && item.value == PacketValue::Text("Avery".into())));
        assert!(preview
            .suggestions
            .iter()
            .any(|item| item.field_selector == "#email"
                && item.value == PacketValue::Text("avery@example.invalid".into())));
        assert!(!preview
            .suggestions
            .iter()
            .any(|item| item.field_selector == "#permit"));
        let mut ws = Workspace::in_memory().unwrap();
        let (job_id, fill_url, binding) = if source_bound {
            let listing = "https://jobs.ashbyhq.com/ashby/fictional";
            let route = "https://jobs.ashbyhq.com/ashby/fictional/application";
            let body = format!(
                r#"{{"jobs":[{{"id":"fictional","title":"Role","jobUrl":"{listing}","applyUrl":"{route}"}}]}}"#
            );
            let fetcher = waypoint_workspace::StaticFetcher::default()
                .route("https://api.ashbyhq.com/posting-api/job-board/ashby", &body);
            assert_eq!(
                ws.discover(
                    &[waypoint_workspace::DiscoveryTarget::new("ashby", "ashby")],
                    &fetcher,
                    1
                )
                .inserted,
                1
            );
            let id = ws.jobs().unwrap().remove(0).id;
            ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
            (
                id,
                route.to_owned(),
                Some(("ashby".to_owned(), listing.to_owned())),
            )
        } else {
            let job_id = JobIdentityId::new("model-pipeline-fixture").unwrap();
            ws.import_job(waypoint_store::StoredJob {
                id: job_id.clone(),
                title: "Fictional role".into(),
                employer: "Fixture".into(),
                location: String::new(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some(url.clone()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            })
            .unwrap();
            (job_id, url.clone(), None)
        };
        let intent = IntentId::new("model-pipeline-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("model-pipeline-grant").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        let filled = apply_reviewed_text_fields_with_replacements(
            &mut ws,
            &job_id,
            &mut driver,
            &fill_url,
            (&preview, &ReviewedReplacements::new(), binding.as_ref()),
            &mut grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 1,
                completed: &mut completed,
            },
        );
        println!("fictional fill result: {filled:?}; completed: {completed:?}");
        if altered {
            assert!(
                filled.is_err(),
                "a field altered by the page must stop the fill"
            );
            assert_eq!(completed.len(), 2);
            assert!(driver.verify_text_value(given_selector, "Avery").is_err());
        } else {
            assert_eq!(filled.unwrap(), 2);
            driver.verify_text_value(given_selector, "Avery").unwrap();
        }
        driver
            .verify_text_value("#email", "avery@example.invalid")
            .unwrap();
        if multilingual {
            for selector in ["#permit", "#gender", "#salary", "#sponsor", "#birth"] {
                driver.verify_text_value(selector, "").unwrap();
            }
        }
        let image = driver.screenshot_base64().unwrap();
        let output_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/model-eval");
        std::fs::create_dir_all(&output_dir).unwrap();
        std::fs::write(
            output_dir.join(if multilingual {
                "local-model-multilingual-filled-form.png"
            } else if altered {
                "local-model-altered-form.png"
            } else {
                "local-model-filled-form.png"
            }),
            base64::engine::general_purpose::STANDARD
                .decode(&image)
                .unwrap(),
        )
        .unwrap();
        let visual = crate::visual_review::analyze(
            addr,
            &model,
            &image,
            &[
                (given_label.into(), "Avery".into()),
                (email_label.into(), "avery@example.invalid".into()),
            ],
        )
        .unwrap();
        println!("fictional post-fill visual review: {visual}");
        let comparisons = visual["comparisons"].as_array().unwrap();
        let name = comparisons
            .iter()
            .find(|item| item["label"] == given_label)
            .unwrap();
        if altered {
            assert_ne!(
                name["status"], "matches_transcription",
                "visual review falsely matched the altered field"
            );
        } else {
            assert_eq!(name["status"], "matches_transcription");
        }
        assert!(comparisons
            .iter()
            .any(|item| item["label"] == email_label && item["status"] == "matches_transcription"));
        if multilingual {
            let observations = visual["observations"].as_array().unwrap();
            for label in [
                "Besitzen Sie eine Arbeitserlaubnis?",
                "Geschlecht",
                "Gehaltsvorstellung",
                "¿Necesita patrocinio laboral?",
                "Date de naissance",
            ] {
                let matching: Vec<_> = observations
                    .iter()
                    .filter(|item| item["label"] == label)
                    .collect();
                assert_eq!(matching.len(), 1, "visual label {label}");
                assert_eq!(matching[0]["visible_value"], "", "visual value for {label}");
            }
        }
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
        assert_eq!(
            driver.read_attribute("body", "data-submitted").unwrap(),
            None
        );
    }

    #[test]
    fn coverage_flags_empty_required_values_for_review() {
        use waypoint_applications::form_mapping::FormField;
        let fields = [
            ("#name", "Name", FormFieldType::Text),
            ("#consent", "Consent", FormFieldType::Checkbox),
            ("#email", "Email", FormFieldType::Email),
            ("#missing", "Missing", FormFieldType::Text),
        ]
        .into_iter()
        .map(|(selector, label, field_type)| FormField {
            selector: selector.into(),
            label: label.into(),
            field_type,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        })
        .collect();
        let mut preview = FormPreview {
            model_review_selectors: Default::default(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![
                ProposedMapping {
                    field_selector: "#name".into(),
                    value: PacketValue::Text("  ".into()),
                },
                ProposedMapping {
                    field_selector: "#consent".into(),
                    value: PacketValue::Bool(false),
                },
                ProposedMapping {
                    field_selector: "#email".into(),
                    value: PacketValue::Text("test@example.test".into()),
                },
            ],
            manual_review: vec![],
        };
        let (filled, review) = fill_coverage(&preview);
        assert_eq!(filled, vec!["Name", "Consent", "Email"]);
        assert_eq!(review, vec!["Name", "Consent", "Missing"]);
        refresh_required_review(&mut preview);
        assert_eq!(
            preview.manual_review,
            vec![
                "Required field needs review: Name",
                "Required field needs review: Consent",
                "Required field needs review: Missing",
            ]
        );
        preview.manual_review.push("Visual review required".into());
        preview.suggestions[0].value = PacketValue::Text("Ada".into());
        preview.suggestions[2].value = PacketValue::Text(" ".into());
        refresh_required_review(&mut preview);
        assert_eq!(
            preview.manual_review,
            vec![
                "Visual review required",
                "Required field needs review: Consent",
                "Required field needs review: Email",
                "Required field needs review: Missing",
            ]
        );
    }

    #[test]
    fn duplicate_labels_identify_the_unfilled_field_and_refresh_correctly() {
        use waypoint_applications::form_mapping::FormField;
        let fields = ["#personal-email", "#work-email"]
            .into_iter()
            .map(|selector| FormField {
                selector: selector.into(),
                label: "Email".into(),
                field_type: FormFieldType::Email,
                required: true,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                min_length_utf16: None,
                pattern: None,
                autocomplete: None,
            })
            .collect();
        let mut preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![ProposedMapping {
                field_selector: "#personal-email".into(),
                value: PacketValue::Text("avery@example.invalid".into()),
            }],
            manual_review: vec!["Visual review required".into()],
            model_review_selectors: Default::default(),
        };
        let shown = preview.fields_for_review();
        assert_eq!(
            preview.repeated_label_selectors(),
            ["#personal-email", "#work-email"]
        );
        assert_eq!(shown[0].label, "Email (field 1 of 2)");
        assert_eq!(shown[1].label, "Email (field 2 of 2)");
        let (filled, missing) = fill_coverage(&preview);
        assert_eq!(filled, ["Email (field 1 of 2)"]);
        assert_eq!(missing, ["Email (field 2 of 2)"]);
        refresh_required_review(&mut preview);
        assert_eq!(
            preview.manual_review,
            [
                "Visual review required",
                "Required field needs review: Email (field 2 of 2)"
            ]
        );
        preview.suggestions.push(ProposedMapping {
            field_selector: "#work-email".into(),
            value: PacketValue::Text("work@example.invalid".into()),
        });
        refresh_required_review(&mut preview);
        assert_eq!(preview.manual_review, ["Visual review required"]);
        assert_eq!(preview.schema.fields[0].label, "Email");
    }

    #[test]
    fn repeated_labels_ignore_case_and_whitespace_without_merging_distinct_fields() {
        use waypoint_applications::form_mapping::FormField;
        let fields = [
            ("#one", " Email Address "),
            ("#two", "email\u{a0}address"),
            ("#three", "Work email address"),
        ]
        .into_iter()
        .map(|(selector, label)| FormField {
            selector: selector.into(),
            label: label.into(),
            field_type: FormFieldType::Email,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        })
        .collect();
        let mut preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![ProposedMapping {
                field_selector: "#one".into(),
                value: PacketValue::Text("fictional@example.invalid".into()),
            }],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        assert_eq!(preview.repeated_label_selectors(), ["#one", "#two"]);
        let fields = preview.fields_for_review();
        assert_eq!(fields[0].label, " Email Address  (field 1 of 2)");
        assert_eq!(fields[1].label, "email\u{a0}address (field 2 of 2)");
        assert_eq!(fields[2].label, "Work email address");
        let (filled, missing) = fill_coverage(&preview);
        assert_eq!(filled, [" Email Address  (field 1 of 2)"]);
        assert_eq!(
            missing,
            ["email\u{a0}address (field 2 of 2)", "Work email address"]
        );
        refresh_required_review(&mut preview);
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("field 2 of 2")));
        assert_eq!(preview.schema.fields[0].label, " Email Address ");
    }

    #[test]
    fn requested_field_records_preserve_question_value_and_repeated_identity() {
        use waypoint_applications::form_mapping::FormField;
        let fields = [
            ("#personal", "Email"),
            ("#work", "EMAIL"),
            ("#start", "Start date"),
        ]
        .into_iter()
        .map(|(selector, label)| FormField {
            selector: selector.into(),
            label: label.into(),
            field_type: FormFieldType::Text,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: None,
        })
        .collect();
        let mut preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![
                ProposedMapping {
                    field_selector: "#work".into(),
                    value: PacketValue::Text("fictional@example.invalid".into()),
                },
                ProposedMapping {
                    field_selector: "#start".into(),
                    value: PacketValue::Date("2029-01-01".into()),
                },
            ],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let recorded = requested_field_records(&preview).unwrap();
        assert_eq!(recorded[0]["question"], "EMAIL");
        assert_eq!(recorded[0]["review_label"], "EMAIL (field 2 of 2)");
        assert_eq!(recorded[0]["selector"], "#work");
        assert_eq!(
            recorded[0]["value"],
            json!({"text":"fictional@example.invalid"})
        );
        assert_eq!(recorded[1]["value"], json!({"date":"2029-01-01"}));
        preview.suggestions[0].field_selector = "#missing".into();
        assert!(requested_field_records(&preview).is_err());
    }

    #[test]
    #[ignore = "starts one local browser against a fictional form without submitting"]
    fn duplicate_labels_are_distinguishable_on_a_live_form() {
        use waypoint_applications::driver::BrowserDriver;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-duplicate-labels.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(path).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let preview =
            suggest_without_model(live, &serde_json::json!({}), "Model unavailable").unwrap();
        let fields = preview.fields_for_review();
        assert_eq!(
            fields
                .iter()
                .map(|field| field.label.as_str())
                .collect::<Vec<_>>(),
            ["Email (field 1 of 2)", "Email (field 2 of 2)"]
        );
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "Required field needs review: Email (field 1 of 2)"));
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "Required field needs review: Email (field 2 of 2)"));
        let mapped = suggest_without_model(
            driver.inspect_current().unwrap(),
            &serde_json::json!({"email":"avery@example.invalid"}),
            "Model unavailable",
        )
        .unwrap();
        assert_eq!(mapped.suggestions.len(), 2);
        assert_eq!(mapped.repeated_label_selectors().len(), 2);
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser against a fictional form without submitting"]
    fn similar_labels_require_individual_review_on_a_live_form() {
        use waypoint_applications::driver::BrowserDriver;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-similar-labels.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(path).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let preview = suggest_without_model(
            live,
            &json!({"email":"fictional@example.invalid"}),
            "Model unavailable",
        )
        .unwrap();
        assert_eq!(preview.suggestions.len(), 2);
        assert_eq!(preview.repeated_label_selectors().len(), 2);
        let fields = preview.fields_for_review();
        assert_eq!(
            fields
                .iter()
                .map(|field| field.label.as_str())
                .collect::<Vec<_>>(),
            ["EMAIL (field 1 of 2)", "email (field 2 of 2)"]
        );
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }

    struct Model;
    impl Inference for Model {
        fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
            unreachable!()
        }
        fn generate_structured(
            &mut self,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<(Value, UsageReport), InferenceError> {
            Ok((
                json!({"mappings":[{"selector":"#candidate","profile_key":"first_name"}]}),
                UsageReport {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    wall_time_ms: 0,
                },
            ))
        }
    }
    struct WalledDriver;
    impl BrowserDriver for WalledDriver {
        fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
            Ok(LiveForm {
                origin: "https://example.test".into(),
                schema: FormSchema {
                    origin: "https://example.test".into(),
                    fields: vec![],
                    multi_step: false,
                    steps: 1,
                },
                walls: vec!["captcha present".into()],
            })
        }
        fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
            panic!("preview must not execute browser actions")
        }
        fn commit(&mut self) -> Result<SiteAck, DriverError> {
            panic!("preview must not submit")
        }
    }
    #[test]
    fn conflicting_profile_mapping_remains_a_required_review() {
        struct ConflictingModel;
        impl Inference for ConflictingModel {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                unreachable!()
            }
            fn generate_structured(
                &mut self,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                Ok((
                    json!({"mappings":[{"selector":"#first","profile_key":"last_name"}],"manual_review":[]}),
                    UsageReport {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        wall_time_ms: 0,
                    },
                ))
            }
        }
        let live = LiveForm {
            origin: "https://example.test".into(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields: vec![waypoint_applications::form_mapping::FormField {
                    selector: "#first".into(),
                    label: "First name".into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: false,
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: Some("given-name".into()),
                }],
                multi_step: false,
                steps: 1,
            },
            walls: vec![],
        };
        let mut preview = suggest_for_live_form(
            live,
            &mut ConflictingModel,
            &json!({"first_name":"Example", "last_name":"Person"}),
        )
        .unwrap();
        assert!(preview.suggestions.is_empty());
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "Required field needs review: First name"));
        assert!(preview.manual_review.iter().any(|reason| reason
            == &crate::suggestions::invalid_suggestion_warning(&preview.schema.fields[0])));
        preview.suggestions.push(ProposedMapping {
            field_selector: "#first".into(),
            value: PacketValue::Text("Example".into()),
        });
        refresh_required_review(&mut preview);
        assert!(preview.manual_review.is_empty());
    }

    #[test]
    fn model_review_flag_is_preserved_without_mutating_browser_schema() {
        struct ReviewModel;
        impl Inference for ReviewModel {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                unreachable!()
            }
            fn generate_structured(
                &mut self,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                Ok((
                    json!({"mappings":[],"manual_review":["#question"]}),
                    UsageReport {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        wall_time_ms: 0,
                    },
                ))
            }
        }
        let schema = FormSchema {
            origin: "https://example.test".into(),
            fields: vec![waypoint_applications::form_mapping::FormField {
                selector: "#question".into(),
                label: "Tem autorização legal para trabalhar neste país?".into(),
                field_type: FormFieldType::Text,
                required: true,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                min_length_utf16: None,
                pattern: None,
                autocomplete: Some("email".into()),
            }],
            multi_step: false,
            steps: 1,
        };
        let live = LiveForm {
            origin: schema.origin.clone(),
            schema: schema.clone(),
            walls: vec![],
        };
        let preview = suggest_for_live_form(
            live,
            &mut ReviewModel,
            &json!({"email":"avery@example.invalid"}),
        )
        .unwrap();
        assert_eq!(preview.schema, schema);
        assert!(preview.suggestions.is_empty());
        assert!(preview.model_review_selectors.contains("#question"));
        assert!(preview.fields_for_review()[0].sensitive);
        assert!(preview
            .manual_review
            .iter()
            .any(|warning| warning.contains("Model flagged a field")));
    }

    #[test]
    fn unavailable_model_marks_unreviewed_fields_manual_in_preview() {
        struct Unavailable;
        impl Inference for Unavailable {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                unreachable!()
            }
            fn generate_structured(
                &mut self,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                Err(InferenceError::RuntimeUnreachable("fictional".into()))
            }
        }
        let schema = FormSchema {
            origin: "https://example.test".into(),
            fields: vec![waypoint_applications::form_mapping::FormField {
                selector: "#question".into(),
                label: "A question".into(),
                field_type: FormFieldType::Text,
                required: true,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                min_length_utf16: None,
                pattern: None,
                autocomplete: Some("email".into()),
            }],
            multi_step: false,
            steps: 1,
        };
        let live = LiveForm {
            origin: schema.origin.clone(),
            schema: schema.clone(),
            walls: vec![],
        };
        let preview = suggest_for_live_form(
            live,
            &mut Unavailable,
            &json!({"email":"avery@example.invalid"}),
        )
        .unwrap();
        assert_eq!(preview.schema, schema);
        assert!(preview.suggestions.is_empty());
        assert!(preview.model_review_selectors.contains("#question"));
        assert!(preview.fields_for_review()[0].sensitive);
        assert!(preview
            .manual_review
            .iter()
            .any(|warning| warning.contains("Sensitivity review unavailable")));
    }
    #[test]
    fn wall_stops_before_suggestions() {
        let preview = inspect_and_suggest(
            &mut WalledDriver,
            &mut Model,
            "https://example.test",
            &json!({"first_name":"A"}),
        )
        .unwrap();
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("captcha")));
        assert!(preview.suggestions.is_empty());
    }

    #[test]
    fn captcha_fields_are_advisory_and_require_a_fresh_unblocked_inspection() {
        struct CompleteModel;
        impl Inference for CompleteModel {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                unreachable!()
            }
            fn generate_structured(
                &mut self,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                Ok((
                    json!({"mappings":[{"selector":"#candidate","profile_key":"first_name"}],"manual_review":[]}),
                    UsageReport {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        wall_time_ms: 0,
                    },
                ))
            }
        }
        let live = LiveForm {
            origin: "https://example.test".into(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields: vec![waypoint_applications::form_mapping::FormField {
                    selector: "#candidate".into(),
                    label: "First name".into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: false,
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: Some("given-name".into()),
                }],
                multi_step: false,
                steps: 1,
            },
            walls: vec!["captcha present".into()],
        };
        let preview =
            suggest_without_model(live.clone(), &json!({"first_name":"A"}), "offline").unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("review only")));
        assert!(!same_reviewable_walls(&preview, &live.walls));
        assert!(!same_reviewable_walls(&preview, &[]));
        let modeled =
            suggest_for_live_form(live.clone(), &mut CompleteModel, &json!({"first_name":"A"}))
                .unwrap();
        assert_eq!(modeled.suggestions.len(), 1);
        assert!(!same_reviewable_walls(&modeled, &[]));

        let mut ambiguous = live.clone();
        ambiguous
            .walls
            .push("ambiguous form controls need manual review".into());
        let modeled = suggest_for_live_form(
            ambiguous.clone(),
            &mut CompleteModel,
            &json!({"first_name":"A"}),
        )
        .unwrap();
        assert_eq!(modeled.suggestions.len(), 1);
        assert!(!same_reviewable_walls(&modeled, &ambiguous.walls));
        assert!(!same_reviewable_walls(&modeled, &[]));

        let mut mixed = live;
        mixed.walls.push("login required".into());
        let preview = suggest_without_model(mixed, &json!({"first_name":"A"}), "offline").unwrap();
        assert!(preview.suggestions.is_empty());
    }

    #[test]
    fn unresolved_button_permits_only_stable_choice_review() {
        let unresolved = "a button has an unresolved accessible name; manual review is needed";
        let custom = "a custom control needs manual review: Position";
        let live = LiveForm {
            origin: "https://example.invalid".into(),
            schema: FormSchema {
                origin: "https://example.invalid".into(),
                fields: vec![],
                multi_step: false,
                steps: 1,
            },
            walls: vec![unresolved.into(), custom.into()],
        };
        let mut preview = suggest_without_model(live.clone(), &json!({}), "fixture").unwrap();
        assert!(!same_reviewable_walls(&preview, &live.walls));
        assert!(same_choice_reviewable_walls(&preview, &live.walls));
        assert!(!same_choice_reviewable_walls(&preview, &[custom.into()]));
        let mut changed = live.walls.clone();
        changed.push("captcha present".into());
        assert!(!same_choice_reviewable_walls(&preview, &changed));
        changed.pop();
        changed.push("login required".into());
        assert!(!same_choice_reviewable_walls(&preview, &changed));
        preview
            .manual_review
            .push(POST_CHOICE_VISUAL_REQUIRED.into());
        assert!(!same_choice_reviewable_walls(&preview, &live.walls));
    }

    #[test]
    fn hidden_empty_form_suggests_scrolling_without_promising_a_fix() {
        let mut live = LiveForm {
            origin: "https://example.invalid".into(),
            schema: FormSchema { origin: "https://example.invalid".into(), fields: vec![], multi_step: false, steps: 1 },
            walls: vec!["a custom control needs manual review: A field may be obscured, clipped or transparent; check overlays and the visible browser".into()],
        };
        let preview = suggest_without_model(live.clone(), &json!({}), "offline").unwrap();
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("reveal fields as you scroll")));
        live.walls.clear();
        let preview = suggest_without_model(live, &json!({}), "offline").unwrap();
        assert!(!preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("reveal fields as you scroll")));
    }

    #[test]
    #[ignore = "starts one local browser and inspects fictional custom choices"]
    fn custom_radio_visual_result_requires_unchanged_snapshot() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_custom_radio_action.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = suggest_without_model(live, &json!({}), "fixture").unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        let job = crate::manual_job::add(
            &mut ws,
            &json!({"url":"https://example.invalid/fictional","title":"Role","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let mut session = Some(crate::ReviewSession {
            id: "fixture-session".into(),
            job_id: JobIdentityId::new(job["id"].as_str().unwrap()).unwrap(),
            inspection_task_id: "fixture".into(),
            inspection_pending_revision: 0,
            url: url.to_string(),
            route_binding: None,
            profile_digest: Sha256Digest::of("fixture"),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: false,
            fill_attempted: false,
            completed_selectors: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: None,
        });
        let (inventory, digest) =
            crate::custom_radio_snapshot(&mut session.as_mut().unwrap().driver).unwrap();
        let group = inventory
            .groups
            .iter()
            .find(|g| g.label == "Working arrangement")
            .unwrap();
        let selector = group.selector.clone();
        let image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_custom_radio_option(&selector, "Remote")
            .unwrap();
        let job_id = session.as_ref().unwrap().job_id.clone();
        let question_image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_custom_radio_question(&selector)
            .unwrap();
        let revision =
            crate::control_vision::begin_pending(&ws, &job_id, "radio-question", 1).unwrap();
        crate::control_vision::set_radio_group_fixture(
            "radio-question",
            "fixture-session",
            job_id.as_str(),
            revision,
            (&selector, &digest, &question_image, "Working arrangement"),
        );
        let question_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let question_port = question_listener.local_addr().unwrap().port();
        let body = json!({"task_id":"radio-question"}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", question_port)).unwrap();
        write!(client,"POST /api/form/control-vision-status HTTP/1.1\r\nHost: 127.0.0.1:{question_port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = question_listener.accept().unwrap();
        crate::serve(
            &mut server,
            &mut ws,
            &mut session,
            "fixture-token",
            question_port,
        )
        .unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let question: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(question["comparison"], "label_observed");
        assert_eq!(question["rank_task_id"], "custom-radio-group");
        let revision =
            crate::control_vision::begin_pending(&ws, &job_id, "radio-visual", 1).unwrap();
        crate::control_vision::set_radio_option_fixture(
            "radio-visual",
            "fixture-session",
            job_id.as_str(),
            revision,
            (&selector, &digest, &image, "Remote"),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"task_id":"radio-visual"}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/form/control-vision-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["comparison"], "label_observed");
        assert_eq!(result["rank_task_id"], "custom-radio-option");
        assert_eq!(result["pending_finished"], true);
        // A fresh model result attached to the old snapshot must be marked stale
        // if the page changes before its status is collected.
        let revision =
            crate::control_vision::begin_pending(&ws, &job_id, "radio-stale", 1).unwrap();
        crate::control_vision::set_radio_option_fixture(
            "radio-stale",
            "fixture-session",
            job_id.as_str(),
            revision,
            (&selector, &digest, &image, "Remote"),
        );
        session
            .as_mut()
            .unwrap()
            .driver
            .select_custom_radio_option(&selector, "Remote")
            .unwrap();
        let body = json!({"task_id":"radio-stale"}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/form/control-vision-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["comparison"], "stale");
        assert!(session
            .as_ref()
            .unwrap()
            .driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser and inspects fictional custom choices"]
    fn custom_radio_endpoint_reports_options_without_interaction() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_custom_radio_required.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = suggest_without_model(live, &json!({}), "fixture").unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        let job = crate::manual_job::add(
            &mut ws,
            &json!({"url":"https://example.invalid/fictional","title":"Role","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let mut session = Some(crate::ReviewSession {
            id: "fixture-session".into(),
            job_id: JobIdentityId::new(job["id"].as_str().unwrap()).unwrap(),
            inspection_task_id: "fixture".into(),
            inspection_pending_revision: 0,
            url: url.to_string(),
            route_binding: None,
            profile_digest: Sha256Digest::of("fixture"),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: false,
            fill_attempted: false,
            completed_selectors: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: None,
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"session_id":"fixture-session"}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/form/custom-radio-groups HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["groups"][0]["label"], "Work arrangement");
        assert_eq!(result["groups"][0]["options"][1]["label"], "Remote");
        assert_eq!(result["groups"][0]["options"][1]["checked"], true);
        assert_eq!(result["groups"][0]["options"][1]["disabled"], true);
        let selector = result["groups"][0]["selector"].as_str().unwrap();
        for (digest, expected) in [
            ("0".repeat(64), "page changed"),
            (
                result["snapshot_digest"].as_str().unwrap().to_owned(),
                "already selected",
            ),
        ] {
            let body = json!({"session_id":"fixture-session","selector":selector,"option":"On site","snapshot_digest":digest}).to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST /api/form/custom-radio-option-vision-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            let error = crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port)
                .unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
        for (task_id, digest, expected_status) in [
            (
                "radio-advice-fresh",
                result["snapshot_digest"].as_str().unwrap().to_owned(),
                "complete",
            ),
            ("radio-advice-stale", "0".repeat(64), "failed"),
        ] {
            crate::choice_advice::set_radio_fixture(task_id, "fixture-session", &digest);
            let body = json!({"task_id":task_id}).to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST /api/form/custom-choice-advice-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            let advice: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(advice["status"], expected_status);
            if expected_status == "complete" {
                assert_eq!(advice["radio"], true);
            }
        }
        let body = json!({"session_id":"fixture-session","selector":selector,"answer":"On site","snapshot_digest":result["snapshot_digest"]}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/form/custom-radio-advice-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let exact: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(exact["method"], "exact");
        assert_eq!(exact["advice"]["option"], "On site");
        let body = json!({"session_id":"fixture-session","selector":selector,"answer":"Remote","snapshot_digest":"0".repeat(64)}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/form/custom-radio-advice-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        assert!(
            crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port)
                .unwrap_err()
                .contains("page changed")
        );
        assert!(session
            .as_ref()
            .unwrap()
            .driver
            .trace
            .iter()
            .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser and inspects fictional custom choices"]
    fn custom_choice_endpoint_reports_options_without_interaction() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_custom_choices.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = suggest_without_model(live, &json!({}), "fixture").unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        let job = crate::manual_job::add(
            &mut ws,
            &json!({"url":"https://example.invalid/fictional","title":"Role","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let mut session = Some(crate::ReviewSession {
            id: "fixture-session".into(),
            job_id: JobIdentityId::new(job["id"].as_str().unwrap()).unwrap(),
            inspection_task_id: "fixture".into(),
            inspection_pending_revision: 0,
            url: url.to_string(),
            route_binding: None,
            profile_digest: Sha256Digest::of("fixture"),
            preview,
            driver,
            created: std::time::Instant::now(),
            ttl: std::time::Duration::from_secs(120),
            filled: false,
            fill_attempted: false,
            completed_selectors: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: None,
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"session_id":"fixture-session"}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/form/custom-choices HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["choices"][0]["label"], "Job location");
        assert_eq!(result["choices"][0]["options"], json!(["Berlin", "Lisbon"]));
        assert!(session
            .as_ref()
            .unwrap()
            .driver
            .trace
            .iter()
            .all(|entry| !entry.contains("clicked") && !entry.contains("submitted")));
        let selector = result["choices"][0]["selector"]
            .as_str()
            .unwrap()
            .to_owned();
        let digest = result["snapshot_digest"].as_str().unwrap().to_owned();
        let stale_body = json!({"session_id":"fixture-session","selector":selector,"snapshot_digest":"0".repeat(64)}).to_string();
        let mut stale_client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stale_client,"POST /api/form/custom-choice-vision-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{stale_body}",stale_body.len()).unwrap();
        let (mut stale_server, _) = listener.accept().unwrap();
        let error = crate::serve(
            &mut stale_server,
            &mut ws,
            &mut session,
            "fixture-token",
            port,
        )
        .unwrap_err();
        assert!(error.contains("page changed"), "{error}");
        drop(stale_server);
        drop(stale_client);
        if std::env::var_os("WAYPOINT_TEST_CHOICE_VISION").is_some() {
            let body = json!({"session_id":"fixture-session","selector":selector,"snapshot_digest":digest}).to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST /api/form/custom-choice-vision-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            let started: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            let task_id = started["task_id"].as_str().unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            loop {
                let body = json!({"task_id":task_id}).to_string();
                let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(client,"POST /api/form/control-vision-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
                let (mut server, _) = listener.accept().unwrap();
                crate::serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
                drop(server);
                let mut response = String::new();
                client.read_to_string(&mut response).unwrap();
                let status: Value =
                    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
                if status["status"] != "running" {
                    assert_eq!(status["status"], "complete", "{status}");
                    assert_eq!(status["choice"], true);
                    assert_eq!(status["comparison"], "label_observed", "{status}");
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "vision task timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
        let image = session
            .as_mut()
            .unwrap()
            .driver
            .screenshot_field_context(&selector)
            .unwrap();
        let job_id = session.as_ref().unwrap().job_id.clone();
        let revision =
            crate::control_vision::begin_pending(&ws, &job_id, "choice-vision-fixture", 2).unwrap();
        crate::control_vision::set_choice_fixture(
            "choice-vision-fixture",
            "fixture-session",
            job_id.as_str(),
            revision,
            (&selector, &digest, &image, "Job location"),
            false,
        );
        let poll = |ws: &mut Workspace, session: &mut Option<crate::ReviewSession>| -> Value {
            let body = json!({"task_id":"choice-vision-fixture"}).to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST /api/form/control-vision-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            crate::serve(&mut server, ws, session, "fixture-token", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        let fresh = poll(&mut ws, &mut session);
        assert_eq!(fresh["choice"], true);
        assert_eq!(fresh["comparison"], "label_observed");
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&BrowserCommand::Click {
                selector: "#change-label".into(),
            })
            .unwrap();
        let stale = poll(&mut ws, &mut session);
        assert_eq!(stale["comparison"], "stale");
        assert!(session
            .as_ref()
            .unwrap()
            .driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "resource-gated visual review of a fictional choice"]
    fn resource_gated_custom_choice_endpoint_visual_review() {
        assert!(
            std::env::var_os("WAYPOINT_TEST_CHOICE_VISION").is_some(),
            "enable the visual portion of the fixture"
        );
        crate::experience_inference::tests::with_resource_gated_model(
            custom_choice_endpoint_reports_options_without_interaction,
        );
    }

    #[test]
    #[ignore = "starts a browser against a fictional form"]
    fn unannotated_choice_does_not_receive_a_profile_suggestion() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_unannotated_choice.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let location = live
            .schema
            .fields
            .iter()
            .find(|field| field.label == "Location")
            .unwrap();
        assert!(location.sensitive);
        let selector = location.selector.clone();
        let preview =
            suggest_without_model(live, &json!({"city":"Example City"}), "offline").unwrap();
        assert!(preview
            .suggestions
            .iter()
            .all(|mapping| mapping.field_selector != selector));
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a browser against a fictional form"]
    fn native_special_controls_keep_other_fields_reviewable() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_native_special_controls.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let special: Vec<_> = live
            .schema
            .fields
            .iter()
            .filter(|field| field.label != "Name" && field.label != "Interview time")
            .map(|field| field.selector.clone())
            .collect();
        let preview = suggest_without_model(
            live,
            &json!({"first_name":"Avery","last_name":"Example"}),
            "offline",
        )
        .unwrap();
        assert!(!preview.manual_review.is_empty());
        assert!(preview
            .suggestions
            .iter()
            .all(|mapping| !special.contains(&mapping.field_selector)));
        assert!(preview
            .schema
            .fields
            .iter()
            .any(|field| field.label == "Name" && !field.sensitive));
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser and does not submit"]
    fn reviewed_time_reaches_browser_under_a_one_use_grant() {
        use waypoint_domain::GrantId;
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_native_special_controls.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = suggest_without_model(live, &json!({}), "offline").unwrap();
        let (preview, _) = crate::prepare_reviewed_fill(
            &preview,
            &json!({"selectors":[],"answers":[{"selector":"#time","value":"13:30"}]}),
        )
        .unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("time-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Role".into(),
            employer: "Example".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.to_string()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("time-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("time-grant").unwrap(),
            intent_id: IntentId::new(intent.as_str()).unwrap(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        assert_eq!(
            apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut driver,
                url.as_str(),
                &preview,
                &mut grant,
                FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut completed
                }
            )
            .unwrap(),
            1
        );
        assert!(grant.consumed);
        driver.verify_text_value("#time", "13:30").unwrap();
        assert_eq!(completed.len(), 1);
        let mut replay_completed = Vec::new();
        assert!(apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            url.as_str(),
            &preview,
            &mut grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 2,
                completed: &mut replay_completed
            }
        )
        .is_err());
        assert!(replay_completed.is_empty());
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted") && !entry.contains("clicked")));
    }

    #[test]
    #[ignore = "starts one local browser and does not submit"]
    fn reviewed_local_datetime_reaches_browser_under_a_one_use_grant() {
        use waypoint_domain::GrantId;
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_native_special_controls.html");
        let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = suggest_without_model(live, &json!({}), "offline").unwrap();
        let (preview, _) = crate::prepare_reviewed_fill(
            &preview,
            &json!({"selectors":[],"answers":[{"selector":"#datetime","value":"2026-11-01T13:30"}]}),
        )
        .unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("time-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Role".into(),
            employer: "Example".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.to_string()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("time-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("time-grant").unwrap(),
            intent_id: IntentId::new(intent.as_str()).unwrap(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        assert_eq!(
            apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut driver,
                url.as_str(),
                &preview,
                &mut grant,
                FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut completed
                }
            )
            .unwrap(),
            1
        );
        assert!(grant.consumed);
        driver
            .verify_text_value("#datetime", "2026-11-01T13:30")
            .unwrap();
        assert_eq!(completed.len(), 1);
        let mut replay_completed = Vec::new();
        assert!(apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            url.as_str(),
            &preview,
            &mut grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 2,
                completed: &mut replay_completed
            }
        )
        .is_err());
        assert!(replay_completed.is_empty());
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted") && !entry.contains("clicked")));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "resource-gated read-only comparison on a previously reviewed public application page"]
    fn resource_gated_public_conflicting_lesson_comparison() {
        crate::experience_inference::tests::with_resource_gated_model(
            live_public_phone_lesson_comparison_is_read_only,
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "resource-gated read-only model inspection of a public form"]
    fn resource_gated_public_form_context_mapping() {
        crate::experience_inference::tests::with_resource_gated_model(|| {
            let url = std::env::var("WAYPOINT_PUBLIC_APPLICATION_URL").unwrap();
            let parsed = url::Url::parse(&url).unwrap();
            crate::form_links::check_job_and_destination(
                crate::manual_job::source(parsed.host_str().unwrap()),
                &parsed.origin().ascii_serialization(),
                waypoint_policy::Operation::Read,
            )
            .unwrap();
            let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
            let live = driver.open(&url).unwrap();
            let model = crate::resident_model().unwrap().expect("resident model");
            let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", &model)
                .unwrap()
                .with_resident_context();
            let mut llm = crate::resident_inference::ResidentInference::new(
                llm,
                model,
                crate::resident_model,
            );
            let preview = suggest_for_live_form(live, &mut llm, &json!({"full_name":"Avery Example","email":"avery@example.invalid","phone":"+12025550123"})).unwrap();
            let instruction_fields: Vec<_> = preview
                .schema
                .fields
                .iter()
                .filter(|field| field.label.to_lowercase().contains("without ai"))
                .collect();
            assert!(!instruction_fields.is_empty());
            assert!(instruction_fields.iter().all(|field| !preview
                .suggestions
                .iter()
                .any(|mapping| mapping.field_selector == field.selector)));
            let phone = preview
                .schema
                .fields
                .iter()
                .find(|field| field.label.ends_with(" — Phone"));
            println!(
                "instruction_fields={} phone_suggested={} warnings={}",
                instruction_fields.len(),
                phone.is_some_and(|field| preview
                    .suggestions
                    .iter()
                    .any(|mapping| mapping.field_selector == field.selector)),
                preview.manual_review.len()
            );
            assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
                && !entry.contains("clicked")
                && !entry.contains("submitted")));
        });
    }

    #[test]
    #[ignore = "read-only inspection of a public application page"]
    fn live_public_application_structure_is_read_only() {
        let url = std::env::var("WAYPOINT_PUBLIC_APPLICATION_URL")
            .expect("set the public application URL");
        let parsed = url::Url::parse(&url).unwrap();
        let source = crate::manual_job::source(parsed.host_str().unwrap());
        crate::form_links::check_job_and_destination(
            source,
            parsed.origin().ascii_serialization().as_str(),
            waypoint_policy::Operation::Read,
        )
        .unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        assert!(!live.schema.fields.is_empty(), "no fields detected");
        if let Some(path) = std::env::var_os("WAYPOINT_PUBLIC_APPLICATION_SCREENSHOT") {
            use base64::Engine;
            let png = base64::engine::general_purpose::STANDARD
                .decode(driver.screenshot_base64().unwrap())
                .unwrap();
            std::fs::write(path, png).unwrap();
        }
        if std::env::var_os("WAYPOINT_EXPECT_NO_AI_NOTICE").is_some() {
            let (text, truncated) = driver.page_text_bounded(32_000).unwrap();
            println!(
                "no_ai_instruction_detected={} text_truncated={truncated}",
                crate::form_task::possible_no_ai_instruction(&text)
            );
            assert!(crate::form_task::possible_no_ai_instruction(&text));
        }
        let choices = driver.page_custom_choices().unwrap();
        println!(
            "fields={} required={} sensitive={} custom_choices={} expanded={} incomplete_choices={} inventory_incomplete={}",
            live.schema.fields.len(),
            live.schema.fields.iter().filter(|field| field.required).count(),
            live.schema.fields.iter().filter(|field| field.sensitive).count(),
            choices.choices.len(),
            choices.choices.iter().filter(|choice| choice.expanded).count(),
            choices.choices.iter().filter(|choice| choice.options_incomplete).count(),
            choices.incomplete,
        );
        for field in &live.schema.fields {
            if matches!(field.field_type, FormFieldType::TextArea) {
                let normalized = field.label.to_lowercase();
                println!(
                    "text_area label_chars={} mentions_ai={} mentions_without_ai={} sensitive={}",
                    field.label.chars().count(),
                    normalized.contains("ai"),
                    normalized.contains("without ai"),
                    field.sensitive
                );
            }
            if field.label.to_lowercase().contains("phone") {
                println!("phone field label={:?}, type={:?}, required={}, sensitive={}, autocomplete={:?}", field.label, field.field_type, field.required, field.sensitive, field.autocomplete);
            }
            if matches!(field.field_type, FormFieldType::Radio { .. }) {
                println!(
                    "radio={:?} sensitive={} required={}",
                    field.label, field.sensitive, field.required
                );
            }
        }
        let refreshed = driver.inspect_current().unwrap();
        println!(
            "initial_walls={} initial_captcha={} refreshed_walls={} refreshed_captcha={}",
            live.walls.len(),
            live.walls.iter().any(|wall| wall == "captcha present"),
            refreshed.walls.len(),
            refreshed.walls.iter().any(|wall| wall == "captcha present")
        );
        println!(
            "initial_wall_details={:?}; refreshed_wall_details={:?}",
            live.walls, refreshed.walls
        );
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "read-only inspection of a public application page"]
    fn live_public_application_preview_is_read_only() {
        struct Recording<I> {
            inner: I,
            completed: usize,
        }
        impl<I: Inference> Inference for Recording<I> {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                self.inner.probe()
            }
            fn generate_structured(
                &mut self,
                schema: &str,
                prompt: &str,
                max: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                println!("Public form mapping request ({schema}): {prompt}");
                let result = self.inner.generate_structured(schema, prompt, max)?;
                self.completed += 1;
                println!("Public form mapping response ({schema}): {}", result.0);
                Ok(result)
            }
        }
        let url = std::env::var("WAYPOINT_PUBLIC_APPLICATION_URL")
            .expect("set the public application URL");
        let model =
            std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL").expect("set the local model name");
        let (addr, context) = crate::visual_review::benchmark_target(&model).unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let source = crate::manual_job::source(parsed.host_str().unwrap());
        crate::form_links::check_job_and_destination(
            source,
            parsed.origin().ascii_serialization().as_str(),
            waypoint_policy::Operation::Read,
        )
        .unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let mut live = driver.open(&url).unwrap();
        if live
            .walls
            .iter()
            .any(|wall| wall == waypoint_browser::CHANGED_DURING_INSPECTION)
        {
            println!("Initial inspection changed; requesting one fresh read-only inspection");
            live = driver.inspect_current().unwrap();
        }
        assert!(!live.schema.fields.is_empty(), "no fields detected");
        let llm =
            waypoint_inference::ollama::OllamaInference::new(&format!("http://{addr}"), &model)
                .unwrap();
        let llm = if let Some(context) = context {
            llm.with_context_window(context).unwrap()
        } else {
            llm.with_resident_context()
        };
        let checked_model = model.clone();
        let guarded = crate::resident_inference::ResidentInference::new(llm, model, move || {
            crate::visual_review::benchmark_target(&checked_model)
                .map(|_| Some(checked_model.clone()))
        });
        let mut llm = Recording {
            inner: guarded,
            completed: 0,
        };
        let preview = suggest_for_live_form(live.clone(), &mut llm, &json!({"full_name":"Avery Example", "first_name":"Avery", "last_name":"Example", "email":"avery@example.invalid", "phone":"+12025550123", "city":"Example City", "linkedin":"https://example.invalid/avery"})).unwrap();
        assert!(
            llm.completed > 0,
            "No model response was produced; walls: {:?}",
            live.walls
        );
        for field in &preview.schema.fields {
            let value = preview
                .suggestions
                .iter()
                .find(|mapping| mapping.field_selector == field.selector)
                .map(|mapping| &mapping.value);
            let kind = match &field.field_type {
                FormFieldType::Select { options } => format!("select ({} options)", options.len()),
                FormFieldType::Radio { options } => format!("radio ({} options)", options.len()),
                FormFieldType::MultiSelect { options } => {
                    format!("multi-select ({} options)", options.len())
                }
                other => format!("{other:?}"),
            };
            println!(
                "field={:?} required={} sensitive={} type={:?} suggested={:?}",
                field.label,
                field.required,
                field.sensitive || preview.model_review_selectors.contains(&field.selector),
                kind,
                value
            );
            assert!(
                !(field.sensitive || preview.model_review_selectors.contains(&field.selector))
                    || value.is_none(),
                "sensitive field received a suggestion"
            );
        }
        println!("walls={:?} manual={:?}", live.walls, preview.manual_review);
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "read-only public page, local browser and resource-gated resident model"]
    fn live_public_phone_lesson_comparison_is_read_only() {
        use waypoint_workspace::form_experience::{
            Control, Experience, ExperienceMatch, Observation, Outcome, Platform, Response, Stage,
        };
        struct Counting<T> {
            inner: T,
            completed: usize,
        }
        impl<T: Inference> Inference for Counting<T> {
            fn probe(
                &mut self,
            ) -> Result<waypoint_inference::CapabilityReport, waypoint_inference::InferenceError>
            {
                self.inner.probe()
            }
            fn generate_structured(
                &mut self,
                schema: &str,
                prompt: &str,
                max: u32,
            ) -> Result<(Value, waypoint_inference::UsageReport), waypoint_inference::InferenceError>
            {
                let result = self.inner.generate_structured(schema, prompt, max)?;
                if schema == "form_mapping" {
                    self.completed += 1;
                }
                Ok(result)
            }
        }
        let url = std::env::var("WAYPOINT_PUBLIC_APPLICATION_URL").expect("public application URL");
        let parsed = url::Url::parse(&url).unwrap();
        let source = crate::manual_job::source(parsed.host_str().unwrap());
        crate::form_links::check_job_and_destination(
            source,
            &parsed.origin().ascii_serialization(),
            waypoint_policy::Operation::Read,
        )
        .unwrap();
        let model = crate::resident_model()
            .unwrap()
            .expect("eligible resident model");
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", &model)
            .unwrap()
            .with_resident_context();
        let mut llm = Counting {
            inner: crate::resident_inference::ResidentInference::new(
                llm,
                model,
                crate::resident_model,
            ),
            completed: 0,
        };
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let mut live = driver.open(&url).unwrap();
        if live
            .walls
            .iter()
            .any(|wall| wall == waypoint_browser::CHANGED_DURING_INSPECTION)
        {
            live = driver.inspect_current().unwrap();
        }
        let phone: Vec<_> = live
            .schema
            .fields
            .iter()
            .filter(|field| field.label.trim().eq_ignore_ascii_case("phone") && !field.sensitive)
            .map(|field| field.selector.clone())
            .collect();
        assert_eq!(
            phone.len(),
            1,
            "expected one non-sensitive phone label; form may have changed"
        );
        let profile = json!({"first_name":"Avery","last_name":"Example","email":"avery@example.invalid","phone":"+12025550123"});
        let lesson = ExperienceMatch { id:"00000000000000000000000000000001".into(), revision:1, same_platform:true, experience:Experience { version:1,reviewed:true,enabled:true,observed_at:1,platform:Some(Platform::Greenhouse),stage:Stage::EnterInformation,control:Control::Text,observation:Observation::AmbiguousLabel,response:Response::InspectContext,outcome:Outcome::Unknown,note:Some("On a prior form, a field explicitly asking for the applicant's phone was ordinary contact information; confirm whose phone this form asks for.".into()),use_note_in_model:true } };
        let lessons = if std::env::var_os("WAYPOINT_CONFLICTING_LESSONS").is_some() {
            let mut lessons = vec![lesson.clone()];
            for i in 1..=7 {
                let mut item = lesson.clone();
                item.id = format!("{:032x}", i + 1);
                item.experience.observed_at = i + 1;
                item.experience.note = None;
                item.experience.use_note_in_model = false;
                match i {
                    1 => item.experience.outcome = Outcome::DidNotHelp,
                    2 => item.experience.outcome = Outcome::Helped,
                    3 => item.experience.observation = Observation::ValueChanged,
                    4 => item.experience.observation = Observation::ValidationError,
                    5 => item.experience.observation = Observation::HiddenControl,
                    _ => item.experience.observation = Observation::UnexpectedNavigation,
                }
                lessons.push(item);
            }
            let selected = crate::form_task::relevant_experiences(&live.schema, &lessons);
            assert!(selected
                .iter()
                .any(|item| item.experience.outcome == Outcome::Helped));
            assert!(selected
                .iter()
                .any(|item| item.experience.outcome == Outcome::DidNotHelp));
            selected
        } else {
            vec![lesson]
        };
        let lesson_first = std::env::var_os("WAYPOINT_LESSON_FIRST").is_some();
        let run_lesson = |llm: &mut Counting<_>| {
            let before = llm.completed;
            let mut informed = crate::experience_inference::ExperienceInference::new(llm, &lessons);
            let preview = suggest_for_live_form(live.clone(), &mut informed, &profile).unwrap();
            assert!(
                informed.attempted_with_hints(),
                "no lesson-assisted request"
            );
            assert!(
                llm.completed > before,
                "no completed lesson-assisted mapping"
            );
            preview
        };
        let run_baseline = |llm: &mut Counting<_>| {
            let before = llm.completed;
            let preview = suggest_for_live_form(live.clone(), llm, &profile).unwrap();
            assert!(llm.completed > before, "no completed baseline mapping");
            preview
        };
        let (baseline, with_lesson) = if lesson_first {
            let with_lesson = run_lesson(&mut llm);
            (run_baseline(&mut llm), with_lesson)
        } else {
            let baseline = run_baseline(&mut llm);
            (baseline, run_lesson(&mut llm))
        };
        println!("lesson_first={lesson_first}");
        for (name, preview) in [("baseline", baseline), ("with lesson", with_lesson)] {
            for (label, expected) in [
                ("first name", "Avery"),
                ("last name", "Example"),
                ("email", "avery@example.invalid"),
                ("phone", "+12025550123"),
            ] {
                let selectors: Vec<_> = preview
                    .schema
                    .fields
                    .iter()
                    .filter(|field| field.label.trim().eq_ignore_ascii_case(label))
                    .map(|field| &field.selector)
                    .collect();
                if selectors.len() == 1 {
                    for mapping in preview
                        .suggestions
                        .iter()
                        .filter(|mapping| &mapping.field_selector == selectors[0])
                    {
                        assert_eq!(
                            mapping.value,
                            PacketValue::Text(expected.into()),
                            "{name}: incorrect fictional value for {label}"
                        );
                    }
                }
            }
            let phone_mappings: Vec<_> = preview
                .suggestions
                .iter()
                .filter(|mapping| mapping.field_selector == phone[0])
                .collect();
            let mapped = !phone_mappings.is_empty();
            let value_correct = phone_mappings
                .iter()
                .all(|mapping| mapping.value == PacketValue::Text("+12025550123".into()));
            let flagged = preview.model_review_selectors.contains(&phone[0]);
            let sensitive_mapped = preview.suggestions.iter().any(|mapping| {
                preview.schema.fields.iter().any(|field| {
                    field.selector == mapping.field_selector
                        && (field.sensitive
                            || preview.model_review_selectors.contains(&field.selector))
                })
            });
            let value_result = if mapped {
                value_correct.to_string()
            } else {
                "not_applicable".into()
            };
            println!("{name}: phone_mapped={mapped}, phone_value_correct={value_result}, phone_flagged={flagged}, sensitive_mapping={sensitive_mapped}, review_reasons={}", preview.manual_review.len());
            assert!(phone_mappings.len() <= 1, "duplicate phone suggestions");
            assert!(
                value_correct,
                "phone suggestion differs from fictional profile"
            );
            assert!(!sensitive_mapped);
        }
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "read-only inspection of a public application page"]
    fn live_workable_custom_choice_inventory_is_read_only() {
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver
            .open("https://apply.workable.com/goglobal/j/4905C591AD/apply/")
            .unwrap();
        let inventory = driver.page_custom_choices().unwrap();
        let location = inventory
            .choices
            .iter()
            .find(|choice| choice.label == "Job location")
            .expect("visible location choice");
        assert!(!location.expanded && location.options.is_empty() && location.options_incomplete);
        assert!(driver.trace.iter().all(|entry| !entry.contains("clicked")
            && !entry.contains("typed")
            && !entry.contains("submitted")));
    }

    #[test]
    #[ignore = "read-only public page and resource-gated resident local vision-capable model"]
    fn live_workable_captcha_preview_is_advisory_only() {
        let model =
            std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL").expect("set the local model name");
        assert_eq!(
            crate::resident_model().unwrap().as_deref(),
            Some(model.as_str())
        );
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver
            .open("https://apply.workable.com/goglobal/j/4905C591AD/apply/")
            .unwrap();
        assert!(!live.schema.fields.is_empty(), "page has no fields");
        assert!(live.walls.iter().any(|wall| wall == "captcha present"));
        let fields = live.schema.fields.len();
        let mut llm =
            waypoint_inference::ollama::OllamaInference::new("http://127.0.0.1:11434", model)
                .unwrap()
                .with_resident_context();
        let preview = suggest_for_live_form(
            live.clone(),
            &mut llm,
            &json!({
                "first_name":"Alex", "last_name":"Example", "email":"alex@example.invalid",
                "phone":"+12025550123", "address":"10 Example Street", "city":"Example City",
                "linkedin":"https://example.invalid/alex"
            }),
        )
        .unwrap();
        println!(
            "public read-only preview: fields={fields} suggestions={} selectors={:?} walls={:?}",
            preview.suggestions.len(),
            preview
                .suggestions
                .iter()
                .map(|item| item.field_selector.as_str())
                .collect::<Vec<_>>(),
            live.walls
        );
        let selected: std::collections::HashSet<_> = preview
            .suggestions
            .iter()
            .filter_map(|item| {
                if let PacketValue::Text(text) = &item.value {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            selected,
            [
                "+12025550123",
                "10 Example Street",
                "https://example.invalid/alex"
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(preview.suggestions.len(), 3);
        for mapping in &preview.suggestions {
            let label = preview
                .schema
                .fields
                .iter()
                .find(|field| field.selector == mapping.field_selector)
                .expect("suggested field")
                .label
                .to_lowercase();
            let expected_label = match &mapping.value {
                PacketValue::Text(value) if value == "+12025550123" => "phone",
                PacketValue::Text(value) if value == "10 Example Street" => "address",
                PacketValue::Text(value) if value == "https://example.invalid/alex" => "linkedin",
                _ => panic!("unexpected suggestion"),
            };
            assert!(
                label.contains(expected_label),
                "unexpected mapping for {label}"
            );
        }
        assert!(!same_reviewable_walls(&preview, &live.walls));
        assert!(!same_reviewable_walls(&preview, &[]));
        assert!(driver.trace.iter().all(|entry| !entry.contains("typed")
            && !entry.contains("clicked")
            && !entry.contains("submitted")));
    }

    #[test]
    fn oversized_suggestion_does_not_block_other_fields() {
        let field = |selector: &str, label: &str, hint: &str, limit| {
            waypoint_applications::form_mapping::FormField {
                selector: selector.into(),
                label: label.into(),
                field_type: FormFieldType::Text,
                required: true,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: limit,
                min_length_utf16: None,
                pattern: None,
                autocomplete: Some(hint.into()),
            }
        };
        let live = LiveForm {
            origin: "https://example.test".into(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields: vec![
                    field("#first", "First name", "given-name", Some(2)),
                    field("#last", "Last name", "family-name", Some(20)),
                ],
                multi_step: false,
                steps: 1,
            },
            walls: vec![],
        };
        let mut preview = suggest_without_model(
            live,
            &json!({"first_name":"A😀", "last_name":"Example"}),
            "No model",
        )
        .unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert_eq!(preview.suggestions[0].field_selector, "#last");
        assert!(preview
            .manual_review
            .iter()
            .any(|item| item == "Suggested value exceeds field limit: First name"));
        assert!(preview
            .manual_review
            .iter()
            .any(|item| item == "Required field needs review: First name"));
        preview.suggestions.push(ProposedMapping {
            field_selector: "#first".into(),
            value: PacketValue::Text("Al".into()),
        });
        refresh_required_review(&mut preview);
        assert!(!preview
            .manual_review
            .iter()
            .any(|item| item.contains("First name")));
    }

    #[test]
    fn edited_hinted_field_clears_warning_only_after_all_duplicate_labels_are_answered() {
        let field = |selector: &str| waypoint_applications::form_mapping::FormField {
            selector: selector.into(),
            label: "Phone number".into(),
            field_type: FormFieldType::Text,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: Some("tel-national".into()),
        };
        let fields = vec![field("#first"), field("#second")];
        let warning = crate::suggestions::invalid_suggestion_warning(&fields[0]);
        let mut preview = FormPreview {
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![],
            manual_review: vec![warning.clone()],
            model_review_selectors: Default::default(),
        };
        preview.suggestions.push(ProposedMapping {
            field_selector: "#first".into(),
            value: PacketValue::Text("2025550123".into()),
        });
        refresh_required_review(&mut preview);
        assert!(preview.manual_review.contains(&warning));
        preview.suggestions.push(ProposedMapping {
            field_selector: "#second".into(),
            value: PacketValue::Text("2025550123".into()),
        });
        refresh_required_review(&mut preview);
        assert!(!preview.manual_review.contains(&warning));
    }

    #[test]
    fn undersized_profile_suggestion_is_withheld_for_review() {
        let field = waypoint_applications::form_mapping::FormField {
            selector: "#first".into(),
            label: "First name".into(),
            field_type: FormFieldType::Text,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: Some(3),
            pattern: None,
            autocomplete: Some("given-name".into()),
        };
        let live = LiveForm {
            origin: "https://example.test".into(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields: vec![field],
                multi_step: false,
                steps: 1,
            },
            walls: vec![],
        };
        let mut preview =
            suggest_without_model(live, &json!({"first_name":"😀"}), "No model").unwrap();
        assert!(preview.suggestions.is_empty());
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "Suggested value is shorter than field minimum: First name"));
        preview.suggestions.push(ProposedMapping {
            field_selector: "#first".into(),
            value: PacketValue::Text("A😀".into()),
        });
        refresh_required_review(&mut preview);
        assert!(!preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("shorter than field minimum")));
    }

    #[test]
    fn embedded_frame_keeps_reviewable_main_fields_but_requires_stable_warning() {
        let field = waypoint_applications::form_mapping::FormField {
            selector: "#first".into(),
            label: "First name".into(),
            field_type: FormFieldType::Text,
            required: true,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            min_length_utf16: None,
            pattern: None,
            autocomplete: Some("given-name".into()),
        };
        let live = LiveForm {
            origin: "https://example.test".into(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields: vec![field],
                multi_step: false,
                steps: 1,
            },
            walls: vec![waypoint_browser::UNINSPECTED_FRAME_REVIEW.into()],
        };
        let preview =
            suggest_without_model(live, &json!({"first_name":"Fictional"}), "No model").unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == waypoint_browser::UNINSPECTED_FRAME_REVIEW));
        assert!(same_reviewable_walls(
            &preview,
            &[waypoint_browser::UNINSPECTED_FRAME_REVIEW.into()]
        ));
        assert!(!same_reviewable_walls(&preview, &[]));
        assert!(!same_reviewable_walls(
            &preview,
            &["captcha present".into()]
        ));
    }

    #[test]
    fn site_invalid_field_remains_reviewable_but_change_requires_rescan() {
        let warning = format!(
            "{}First name",
            waypoint_browser::INVALID_FIELD_REVIEW_PREFIX
        );
        let live = LiveForm {
            origin: "https://example.test".into(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields: vec![waypoint_applications::form_mapping::FormField {
                    selector: "#first".into(),
                    label: "First name".into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: false,
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: Some("given-name".into()),
                }],
                multi_step: false,
                steps: 1,
            },
            walls: vec![warning.clone()],
        };
        let preview =
            suggest_without_model(live, &json!({"first_name":"Fictional"}), "No model").unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert!(preview.manual_review.contains(&warning));
        assert!(same_reviewable_walls(&preview, &[warning]));
        assert!(!same_reviewable_walls(
            &preview,
            &[format!(
                "{}First name — New site message",
                waypoint_browser::INVALID_FIELD_REVIEW_PREFIX
            )]
        ));
        assert!(!same_reviewable_walls(&preview, &[]));
    }

    #[test]
    fn oversized_form_remains_open_for_manual_review() {
        use waypoint_applications::form_mapping::FormField;
        let fields = (0..201)
            .map(|index| FormField {
                selector: format!("#field-{index}"),
                label: format!("Field {index}"),
                field_type: FormFieldType::Text,
                required: false,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                min_length_utf16: None,
                pattern: None,
                autocomplete: None,
            })
            .collect();
        let live = LiveForm {
            origin: "https://example.test".into(),
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            walls: vec![],
        };
        let preview = suggest_without_model(live, &json!({}), "No model").unwrap();
        assert_eq!(preview.schema.fields.len(), 201);
        assert!(preview.suggestions.is_empty());
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("200-field mapping limit")));
    }

    #[test]
    #[ignore = "starts one local browser and does not submit"]
    fn offline_form_preview_suggests_exact_matches_and_flags_other_required_fields() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = suggest_without_model(
            live,
            &json!({"country":"Germany", "first_name":"Example"}),
            "No eligible local model is loaded",
        )
        .unwrap();
        assert!(preview
            .suggestions
            .iter()
            .any(|item| item.field_selector == "#country"));
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("Given name")));
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("standard autocomplete")));
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser and does not submit"]
    fn month_field_is_detected_filled_and_verified_without_submission() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-month.html");
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut browser = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = browser.open(url.as_str()).unwrap();
        let field = live
            .schema
            .fields
            .iter()
            .find(|field| field.selector == "#start")
            .unwrap();
        assert_eq!(field.field_type, FormFieldType::Month);
        let fill = |selector: &str, value: &str| BrowserCommand::Fill {
            selector: selector.into(),
            value: PacketValue::Date(value.into()),
        };
        assert!(browser.execute(&fill("#start", "2019-12")).is_err());
        assert_eq!(browser.text_value_snapshot("#start").unwrap().value, "");
        assert!(browser.execute(&fill("#start", "2023-02-01")).is_err());
        browser.execute(&fill("#start", "2023-02")).unwrap();
        browser.verify_text_value("#start", "2023-02").unwrap();
        assert!(browser.execute(&fill("#day", "2023-02")).is_err());
        assert_eq!(browser.text_value_snapshot("#day").unwrap().value, "");
        assert!(browser.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser and does not submit"]
    fn obscured_rich_text_is_not_filled() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-obscured-rich-text.html");
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        let fill = |selector: &str| BrowserCommand::Fill {
            selector: selector.into(),
            value: PacketValue::Text("Fictional answer".into()),
        };
        assert!(driver.execute(&fill("#covered")).is_err());
        assert_eq!(driver.text_value_snapshot("#covered").unwrap().value, "");
        assert!(driver.execute(&fill("#focus-covered")).is_err());
        assert_eq!(
            driver.text_value_snapshot("#focus-covered").unwrap().value,
            ""
        );
        driver.execute(&fill("#clear")).unwrap();
        driver
            .verify_text_value("#clear", "Fictional answer")
            .unwrap();
        // The last inserted segment can change the page after pre-write checks.
        // A partial value can remain, but must not be reported as success.
        for selector in ["#input-covered", "#focus-shift"] {
            assert!(driver
                .execute(&BrowserCommand::Fill {
                    selector: selector.into(),
                    value: PacketValue::Text("\u{03a9}".into()),
                })
                .is_err());
        }
        assert_eq!(
            driver.text_value_snapshot("#input-covered").unwrap().value,
            "\u{03a9}"
        );
        assert_eq!(driver.text_value_snapshot("#other").unwrap().value, "");
    }

    #[test]
    #[ignore = "starts one local browser and does not submit"]
    fn field_hidden_before_write_is_not_modified() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-hidden-before-write.html");
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(url.as_str()).unwrap();
        let fill = |selector: &str, value: &str| BrowserCommand::Fill {
            selector: selector.into(),
            value: PacketValue::Text(value.into()),
        };
        driver.execute(&fill("#first", "Avery")).unwrap();
        let error = driver.execute(&fill("#last", "Example")).unwrap_err();
        assert!(error.to_string().contains("no longer visibly rendered"));
        assert_eq!(driver.text_value_snapshot("#last").unwrap().value, "");
        let commands = [
            BrowserCommand::SelectOption {
                selector: "#country".into(),
                label: "Germany".into(),
            },
            BrowserCommand::Fill {
                selector: "#consent".into(),
                value: PacketValue::Bool(true),
            },
            BrowserCommand::Fill {
                selector: "#start".into(),
                value: PacketValue::Date("2026-12-01".into()),
            },
        ];
        for command in &commands {
            assert!(driver
                .execute(command)
                .unwrap_err()
                .to_string()
                .contains("no longer visibly rendered"));
        }
        assert_eq!(
            driver.selected_option_snapshot("#country").unwrap().value,
            ""
        );
        assert_eq!(
            driver
                .choice_snapshot("#consent", "checkbox")
                .unwrap()
                .checked,
            Some(false)
        );
        assert_eq!(driver.text_value_snapshot("#start").unwrap().value, "");
        assert!(driver
            .execute(&fill("#focus-target", "Should not be entered"))
            .is_err());
        assert_eq!(
            driver.text_value_snapshot("#focus-target").unwrap().value,
            ""
        );
        driver
            .execute(&fill("#stable-rich", "First line\nSecond line"))
            .unwrap();
        driver
            .verify_text_value("#stable-rich", "First line\nSecond line")
            .unwrap();
        assert!(driver
            .execute(&fill("#rich", "First line\nSecond line"))
            .is_err());
        assert_eq!(driver.text_value_snapshot("#other").unwrap().value, "");
        let hidden_radio = BrowserCommand::SelectRadio {
            selector: "#remote".into(),
            label: "Remote".into(),
        };
        assert!(driver.execute(&hidden_radio).is_err());
        assert!(driver
            .choice_snapshot("#remote", "radio")
            .unwrap()
            .selections
            .is_empty());
        let visible_radio = BrowserCommand::SelectRadio {
            selector: "#hidden-anchor".into(),
            label: "Visible".into(),
        };
        driver.execute(&visible_radio).unwrap();
        assert_eq!(
            driver
                .choice_snapshot("#visible-option", "radio")
                .unwrap()
                .selections[0]
                .value,
            "visible"
        );
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a local browser against a fictional form without submitting"]
    fn hidden_filled_field_stops_later_writes_in_browser() {
        use waypoint_domain::GrantId;
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-disappearing-field.html");
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        let preview = suggest_without_model(
            live,
            &json!({"first_name":"Avery","last_name":"Example"}),
            "No model",
        )
        .unwrap();
        assert_eq!(preview.suggestions.len(), 2);
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("disappearing-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Role".into(),
            employer: "Example".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.to_string()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("disappearing-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("disappearing-grant").unwrap(),
            intent_id: intent.clone(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        let error = apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            url.as_str(),
            &preview,
            &mut grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 1,
                completed: &mut completed,
            },
        )
        .unwrap_err();
        assert!(error.contains("After filling"), "{error}");
        assert_eq!(completed.len(), 1);
        driver.verify_text_value("#last", "").unwrap();
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a local browser and does not submit"]
    fn offline_autocomplete_can_be_reviewed_and_filled_without_submission() {
        use waypoint_domain::GrantId;
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-autocomplete.html");
        let url = url::Url::from_file_path(fixture).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(url.as_str()).unwrap();
        assert_eq!(
            live.schema.fields[0].autocomplete.as_deref(),
            Some("section-app given-name")
        );
        let preview = suggest_without_model(
            live,
            &json!({"first_name":"Example","email":"example@example.invalid"}),
            "No model",
        )
        .unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert_eq!(preview.suggestions[0].field_selector, "#candidate");
        assert!(!preview
            .suggestions
            .iter()
            .any(|mapping| mapping.field_selector == "#ambiguous"));
        assert!(!preview
            .suggestions
            .iter()
            .any(|mapping| mapping.field_selector == "#conflicting-type"));
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason.contains("Preferred location")));
        assert!(!same_reviewable_walls(
            &preview,
            &["captcha present".into()]
        ));
        assert!(!same_reviewable_walls(&preview, &[]));
        assert!(driver
            .trace
            .iter()
            .all(|line| !line.contains("typed") && !line.contains("submitted")));
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("autocomplete-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Role".into(),
            employer: "Example".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.to_string()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("autocomplete-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("autocomplete-grant").unwrap(),
            intent_id: IntentId::new(intent.as_str()).unwrap(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        assert_eq!(
            apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut driver,
                url.as_str(),
                &preview,
                &mut grant,
                FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut completed
                }
            )
            .unwrap(),
            1
        );
        assert_eq!(
            driver
                .read_attribute("#candidate", "data-seen")
                .unwrap()
                .as_deref(),
            Some("Example")
        );
        assert_eq!(driver.read_attribute("#permit", "value").unwrap(), None);
        driver.verify_text_value("#conflicting-type", "").unwrap();
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local browser and does not submit"]
    fn reviewed_prefill_reaches_a_visible_embedded_form_after_hidden_frames() {
        use waypoint_domain::GrantId;
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        for (filename, text_option) in [
            ("form_iframe_controls.html", false),
            ("form_iframe_nested.html", false),
            ("form_shadow.html", false),
            ("form_mixed_scopes.html", false),
            ("form_four_scopes.html", false),
            ("form_datalist.html", false),
            ("form_datalist.html", true),
        ] {
            let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/waypoint_browser/tests/fixtures")
                .join(filename);
            let url = url::Url::from_file_path(fixture.canonicalize().unwrap()).unwrap();
            let live = driver.open(url.as_str()).unwrap();
            let mut preview = suggest_without_model(
                live,
                &json!({"first_name":"Fictional","country":"Germany"}),
                "No model",
            )
            .unwrap();
            // The fixtures deliberately use labels such as "Name" and "Region"
            // that do not establish the more specific autocomplete value. Model
            // a user-reviewed correction rather than silently trusting the hint.
            assert!(preview.suggestions.is_empty(), "{filename}");
            let hint = if filename == "form_datalist.html" {
                "country-name"
            } else {
                "given-name"
            };
            let field = preview
                .schema
                .fields
                .iter()
                .find(|field| {
                    if text_option {
                        field.selector == "#text-option"
                    } else {
                        field.autocomplete.as_deref() == Some(hint)
                    }
                })
                .unwrap_or_else(|| panic!("Missing {hint} field in {filename}"));
            preview.suggestions.push(ProposedMapping {
                field_selector: field.selector.clone(),
                value: PacketValue::Text(
                    if text_option {
                        "New Zealand"
                    } else if filename == "form_datalist.html" {
                        "Germany"
                    } else {
                        "Fictional"
                    }
                    .into(),
                ),
            });
            assert_eq!(preview.suggestions.len(), 1);
            let selector = preview.suggestions[0].field_selector.clone();
            assert!(
                selector.starts_with("waypoint-frame:")
                    || selector.starts_with("waypoint-shadow:")
                    || (filename == "form_datalist.html"
                        && (selector == "#region" || selector == "#text-option"))
            );
            assert!(preview
                .manual_review
                .iter()
                .any(|wall| wall == waypoint_browser::UNINSPECTED_FRAME_REVIEW
                    || wall == waypoint_browser::SHADOW_FORM_REVIEW
                    || wall == waypoint_browser::NATIVE_DATALIST_REVIEW));
            let mut ws = Workspace::in_memory().unwrap();
            let job_id = JobIdentityId::new("embedded-fixture").unwrap();
            ws.import_job(waypoint_store::StoredJob {
                id: job_id.clone(),
                title: "Role".into(),
                employer: "Example".into(),
                location: String::new(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some(url.to_string()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            })
            .unwrap();
            let intent = IntentId::new("embedded-intent").unwrap();
            let packet = Sha256Digest::of("fictional profile");
            let mut grant = ApprovalGrant {
                id: GrantId::new("embedded-grant").unwrap(),
                intent_id: intent.clone(),
                bound_payload_sha256: Sha256Digest::of(
                    &serde_json::to_string(&preview.suggestions).unwrap(),
                ),
                profile_revision: 1,
                packet_sha256: packet.clone(),
                form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
                destination_origin: preview.schema.origin.clone(),
                operation: GrantOperation::Prefill,
                nonce: "fixture".into(),
                explicit_user_gesture: true,
                consumed: false,
                expired: false,
            };
            let mut completed = Vec::new();
            assert_eq!(
                apply_reviewed_text_fields(
                    &mut ws,
                    &job_id,
                    &mut driver,
                    url.as_str(),
                    &preview,
                    &mut grant,
                    FillContext {
                        intent: &intent,
                        packet_digest: &packet,
                        source_reviewed: true,
                        timestamp: 1,
                        completed: &mut completed
                    }
                )
                .unwrap(),
                1
            );
            if filename == "form_datalist.html" {
                driver
                    .verify_choice(&BrowserCommand::SelectOption {
                        selector: selector.clone(),
                        label: if text_option {
                            "New Zealand"
                        } else {
                            "Germany"
                        }
                        .into(),
                    })
                    .unwrap();
            } else {
                driver.verify_text_value(&selector, "Fictional").unwrap();
            }
            assert!(grant.consumed);
            assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
        }
    }

    #[test]
    #[ignore = "starts a local browser with a fictional profile and does not submit"]
    fn model_radio_suggestion_can_be_reviewed_and_filled() {
        use waypoint_domain::GrantId;
        struct RadioModel;
        impl Inference for RadioModel {
            fn probe(&mut self) -> Result<CapabilityReport, InferenceError> {
                unreachable!()
            }
            fn generate_structured(
                &mut self,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<(Value, UsageReport), InferenceError> {
                Ok((
                    json!({"mappings":[{"selector":"#de","profile_key":"country"}],"manual_review":[]}),
                    UsageReport {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        wall_time_ms: 0,
                    },
                ))
            }
        }
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/radio-profile.html");
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let preview = inspect_and_suggest(
            &mut driver,
            &mut RadioModel,
            &url,
            &json!({"country":"Germany"}),
        )
        .unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert!(preview.manual_review.is_empty());
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("radio-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Role".into(),
            employer: "Fixture".into(),
            location: "".into(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.clone()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let intent = IntentId::new("radio-intent").unwrap();
        let packet = Sha256Digest::of("fictional profile");
        let mut grant = ApprovalGrant {
            id: GrantId::new("radio-grant").unwrap(),
            intent_id: IntentId::new(intent.as_str()).unwrap(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&preview.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&preview.schema.schema_hash_input()),
            destination_origin: preview.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut model_flagged = preview.clone();
        model_flagged
            .model_review_selectors
            .insert(preview.suggestions[0].field_selector.clone());
        assert!(apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            &url,
            &model_flagged,
            &mut grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 1,
                completed: &mut Vec::new()
            }
        )
        .is_err());
        assert!(!grant.consumed);
        assert_eq!(
            apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut driver,
                &url,
                &preview,
                &mut grant,
                FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut Vec::new()
                }
            )
            .unwrap(),
            1
        );
        assert_eq!(
            driver
                .read_attribute("#de", "data-seen")
                .unwrap()
                .as_deref(),
            Some("true")
        );
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts one local headless browser; no employer page or model is used"]
    fn reads_actual_fixture_and_proposes_mapping_without_submitting() {
        use waypoint_domain::GrantId;
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = format!(
            "file:///{}",
            fixture.display().to_string().replace('\\', "/")
        );
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let result = inspect_and_suggest(
            &mut driver,
            &mut Model,
            &url,
            &json!({"first_name":"Synthetic"}),
        )
        .unwrap();
        assert!(result
            .schema
            .fields
            .iter()
            .any(|field| field.selector == "#candidate"));
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].field_selector, "#candidate");
        assert!(result
            .manual_review
            .iter()
            .any(|reason| reason.contains("Work authorization")));
        let intent = IntentId::new("fixture-intent").unwrap();
        let packet = Sha256Digest::of("fictional candidate packet");
        let payload = Sha256Digest::of(&serde_json::to_string(&result.suggestions).unwrap());
        let schema = Sha256Digest::of(&result.schema.schema_hash_input());
        let mut ws = Workspace::in_memory().unwrap();
        let job_id = JobIdentityId::new("prefill-fixture").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job_id.clone(),
            title: "Role".into(),
            employer: "Fixture".into(),
            location: "".into(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some(url.clone()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        let mut grant = ApprovalGrant {
            id: GrantId::new("fixture-grant").unwrap(),
            intent_id: IntentId::new("fixture-intent").unwrap(),
            bound_payload_sha256: payload,
            profile_revision: 1,
            packet_sha256: Sha256Digest::of("fictional candidate packet"),
            form_schema_sha256: schema,
            destination_origin: result.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "fixture".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        assert_eq!(
            apply_reviewed_text_fields(
                &mut ws,
                &job_id,
                &mut driver,
                &url,
                &result,
                &mut grant,
                FillContext {
                    intent: &intent,
                    packet_digest: &packet,
                    source_reviewed: true,
                    timestamp: 1,
                    completed: &mut Vec::new()
                }
            )
            .unwrap(),
            1
        );
        assert_eq!(
            driver
                .read_attribute("#candidate", "data-seen")
                .unwrap()
                .as_deref(),
            Some("Synthetic")
        );
        assert!(grant.consumed);
        assert!(apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            &url,
            &result,
            &mut grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 2,
                completed: &mut Vec::new()
            }
        )
        .is_err());
        let partial = FormPreview {
            model_review_selectors: Default::default(),
            schema: result.schema.clone(),
            suggestions: vec![
                ProposedMapping {
                    field_selector: "#candidate".into(),
                    value: PacketValue::Text("Synthetic".into()),
                },
                ProposedMapping {
                    field_selector: "#email".into(),
                    value: PacketValue::Text("invalid-email".into()),
                },
            ],
            manual_review: vec![],
        };
        let mut partial_grant = ApprovalGrant {
            id: GrantId::new("partial-grant").unwrap(),
            intent_id: IntentId::new(intent.as_str()).unwrap(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&partial.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&partial.schema.schema_hash_input()),
            destination_origin: partial.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "partial".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        assert!(apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            &url,
            &partial,
            &mut partial_grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 3,
                completed: &mut completed
            }
        )
        .is_err());
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0]["selector"], "#candidate");
        let changed = FormPreview {
            model_review_selectors: Default::default(),
            schema: result.schema.clone(),
            suggestions: vec![
                ProposedMapping {
                    field_selector: "#candidate".into(),
                    value: PacketValue::Text("Synthetic".into()),
                },
                ProposedMapping {
                    field_selector: "#later-answer".into(),
                    value: PacketValue::Text("Later".into()),
                },
            ],
            manual_review: vec![],
        };
        let mut changed_grant = ApprovalGrant {
            id: GrantId::new("changed-grant").unwrap(),
            intent_id: IntentId::new(intent.as_str()).unwrap(),
            bound_payload_sha256: Sha256Digest::of(
                &serde_json::to_string(&changed.suggestions).unwrap(),
            ),
            profile_revision: 1,
            packet_sha256: packet.clone(),
            form_schema_sha256: Sha256Digest::of(&changed.schema.schema_hash_input()),
            destination_origin: changed.schema.origin.clone(),
            operation: GrantOperation::Prefill,
            nonce: "changed".into(),
            explicit_user_gesture: true,
            consumed: false,
            expired: false,
        };
        let mut completed = Vec::new();
        let error = apply_reviewed_text_fields(
            &mut ws,
            &job_id,
            &mut driver,
            &url,
            &changed,
            &mut changed_grant,
            FillContext {
                intent: &intent,
                packet_digest: &packet,
                source_reviewed: true,
                timestamp: 4,
                completed: &mut completed,
            },
        )
        .unwrap_err();
        assert!(error.contains("Given name"), "{error}");
        assert_eq!(completed.len(), 2);
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
        let ambiguous =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-ambiguous.html");
        let ambiguous_url = format!(
            "file:///{}",
            ambiguous.display().to_string().replace('\\', "/")
        );
        let blocked = driver.open(&ambiguous_url).unwrap();
        assert!(blocked
            .walls
            .iter()
            .any(|reason| reason.contains("ambiguous")));
    }
}
