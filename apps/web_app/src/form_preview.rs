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

pub struct FormPreview {
    pub schema: FormSchema,
    pub suggestions: Vec<ProposedMapping>,
    pub manual_review: Vec<String>,
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
    intent: &IntentId,
    packet_digest: &Sha256Digest,
    source_reviewed: bool,
    timestamp: i64,
) -> Result<usize, String> {
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
    if !fresh.walls.is_empty()
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
                PacketValue::Text(_),
            ) => true,
            (FormFieldType::Date, PacketValue::Date(value)) => {
                waypoint_applications::form_mapping::valid_iso_date(value)
            }
            (FormFieldType::Checkbox, PacketValue::Bool(_)) => true,
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
        if field.sensitive || !valid {
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
    let blocked = !live.walls.is_empty() || live.schema.fields.is_empty();
    let mut manual_review = live.walls;
    if live.schema.fields.is_empty() {
        manual_review.push("No form fields were detected; navigation, login, an embedded frame, or manual review may be required".into());
    }
    if live.schema.multi_step {
        manual_review.push("Multi-step application requires manual review".into());
    }
    let suggestions = if !blocked {
        match crate::suggestions::suggest(inference, &live.schema, profile) {
            Ok(items) => items,
            Err(error) => {
                manual_review.push(format!("Could not map form: {error}"));
                vec![]
            }
        }
    } else {
        vec![]
    };
    for field in &live.schema.fields {
        if field.required
            && !suggestions
                .iter()
                .any(|p| p.field_selector == field.selector)
        {
            manual_review.push(format!("Required field needs review: {}", field.label));
        }
    }
    Ok(FormPreview {
        schema: live.schema,
        suggestions,
        manual_review,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use waypoint_applications::driver::{DriverError, LiveForm, SiteAck};
    use waypoint_applications::form_mapping::BrowserCommand;
    use waypoint_inference::{CapabilityReport, InferenceError, UsageReport};

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
        assert_eq!(result.schema.fields.len(), 2);
        assert_eq!(result.suggestions.len(), 1);
        assert_eq!(result.suggestions[0].field_selector, "#candidate");
        assert!(result.manual_review.is_empty());
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
                &intent,
                &packet,
                true,
                1
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
            &intent,
            &packet,
            true,
            2
        )
        .is_err());
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
