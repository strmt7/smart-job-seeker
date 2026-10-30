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
    reason == "a required custom control needs manual review"
        || reason.starts_with("a required custom control needs manual review: ")
        || reason == "a custom control needs manual review"
        || reason.starts_with("a custom control needs manual review: ")
}

pub fn same_reviewable_walls(preview: &FormPreview, fresh_walls: &[String]) -> bool {
    if fresh_walls.iter().any(|wall| !is_custom_review(wall)) {
        return false;
    }
    let mut expected: Vec<_> = preview
        .manual_review
        .iter()
        .filter(|reason| is_custom_review(reason))
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
fn field_review_label(
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

/// Apply only the exact reviewed mappings, using one prefill grant. This path
/// has no submit or arbitrary click operation. The caller must keep the same
/// browser session and acquire permission for the actual destination first.
pub fn apply_reviewed_text_fields(
    ws: &mut Workspace,
    job_id: &JobIdentityId,
    driver: &mut dyn BrowserDriver,
    url: &str,
    preview: &FormPreview,
    grant: &mut ApprovalGrant,
    context: FillContext<'_>,
) -> Result<usize, String> {
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
    ) || job.canonical_url.as_deref() != Some(url)
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
    }
    let payload = serde_json::to_string(&preview.suggestions).map_err(|e| e.to_string())?;
    let payload_digest = Sha256Digest::of(&payload);
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
        driver
            .execute(&command)
            .map_err(|e| format!("Prefill stopped; review the form: {e}"))?;
        completed.push(serde_json::json!({"selector":field.selector,"label":field.label}));
    }
    for mapping in &preview.suggestions {
        let field = fresh
            .schema
            .fields
            .iter()
            .find(|field| field.selector == mapping.field_selector)
            .ok_or("Unknown field")?;
        let expected = match (&field.field_type, &mapping.value) {
            (
                FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea,
                PacketValue::Text(value),
            ) => Some(value),
            (FormFieldType::Date, PacketValue::Date(value)) => Some(value),
            _ => None,
        };
        if let Some(expected) = expected {
            driver
                .verify_text_value(&mapping.field_selector, expected)
                .map_err(|error| format!("After filling, {} needs review: {error}", field.label))?;
        } else {
            let command = match (&field.field_type, &mapping.value) {
                (FormFieldType::MultiSelect { .. }, PacketValue::TextList(labels)) => {
                    Some(BrowserCommand::SelectOptions {
                        selector: mapping.field_selector.clone(),
                        labels: labels.clone(),
                    })
                }
                (FormFieldType::Select { .. }, PacketValue::Text(label)) => {
                    Some(BrowserCommand::SelectOption {
                        selector: mapping.field_selector.clone(),
                        label: label.clone(),
                    })
                }
                (FormFieldType::Radio { .. }, PacketValue::Text(label)) => {
                    Some(BrowserCommand::SelectRadio {
                        selector: mapping.field_selector.clone(),
                        label: label.clone(),
                    })
                }
                (FormFieldType::Checkbox, PacketValue::Bool(value)) => Some(BrowserCommand::Fill {
                    selector: mapping.field_selector.clone(),
                    value: PacketValue::Bool(*value),
                }),
                _ => None,
            };
            if let Some(command) = command {
                driver.verify_choice(&command).map_err(|error| {
                    format!("After filling, {} needs review: {error}", field.label)
                })?;
            }
        }
    }
    Ok(preview.suggestions.len())
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
    let blocked =
        live.walls.iter().any(|wall| !is_custom_review(wall)) || live.schema.fields.is_empty();
    let mut manual_review = live.walls;
    if live.schema.fields.is_empty() {
        manual_review.push("No form fields were detected; navigation, login, an embedded frame, or manual review may be required".into());
    }
    if live.schema.multi_step {
        manual_review.push("A possible next-step control was found. Navigate and inspect the next page manually; the total number of steps is unknown".into());
    }
    let mut model_review_selectors = std::collections::HashSet::new();
    let suggestions = if !blocked {
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
        for prefix in [
            "Conflicting suggestions need review: ",
            "Invalid or duplicate suggestion needs review: ",
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
    fn optional_custom_choice_is_a_reviewable_obligation() {
        assert!(is_custom_review("a custom control needs manual review"));
        assert!(is_custom_review(
            "a custom control needs manual review: City"
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
        exercise_local_model_form_pipeline(false, false);
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_local_model_detects_a_changed_fictional_field() {
        exercise_local_model_form_pipeline(true, false);
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_local_model_prefills_and_visually_checks_multilingual_form() {
        exercise_local_model_form_pipeline(false, true);
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
        for label in [
            "Tem autorização legal para trabalhar neste país?",
            "Concorda com o tratamento dos seus dados pessoais?",
        ] {
            let matching: Vec<_> = visual["observations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["label"] == label)
                .collect();
            assert_eq!(matching.len(), 1, "visual label {label}");
            assert_eq!(matching[0]["visible_value"], "", "visual value for {label}");
        }
        assert_eq!(
            driver.read_attribute("body", "data-submitted").unwrap(),
            None
        );
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
    }

    #[test]
    #[ignore = "starts a local browser and requires preloaded local qwen3.5:9b and resource preflight"]
    fn live_local_model_reinspects_and_prefills_a_fictional_wizard() {
        use waypoint_domain::GrantId;
        let addr = "127.0.0.1:11434".parse().unwrap();
        assert_eq!(
            waypoint_inference::ollama::resident_context(addr, "qwen3.5:9b").unwrap(),
            4096
        );
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-local-model-wizard.html")
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
        let profile = json!({"first_name":"Avery", "email":"avery@example.invalid"});
        let first = inspect_and_suggest(&mut driver, &mut llm, &url, &profile).unwrap();
        assert!(first.schema.multi_step);
        assert!(first
            .manual_review
            .iter()
            .any(|reason| reason.contains("next-step")));
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
        let visual_first = crate::visual_review::analyze(
            addr,
            "qwen3.5:9b",
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
        let visual_second = crate::visual_review::analyze(
            addr,
            "qwen3.5:9b",
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

    fn exercise_local_model_form_pipeline(altered: bool, multilingual: bool) {
        use base64::Engine;
        use waypoint_domain::GrantId;
        let addr = "127.0.0.1:11434".parse().unwrap();
        assert_eq!(
            waypoint_inference::ollama::resident_context(addr, "qwen3.5:9b").unwrap(),
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
        let llm = waypoint_inference::OllamaInference::new("http://127.0.0.1:11434", "qwen3.5:9b")
            .unwrap()
            .with_resident_context();
        let mut llm = crate::resident_inference::ResidentInference::new(
            llm,
            "qwen3.5:9b".into(),
            crate::resident_model,
        );
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
        let filled = apply_reviewed_text_fields(
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
            "qwen3.5:9b",
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
        assert!(preview
            .manual_review
            .iter()
            .any(|reason| reason == "Conflicting suggestions need review: First name"));
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
        assert!(driver.trace.iter().all(|line| !line.contains("submitted")));
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
