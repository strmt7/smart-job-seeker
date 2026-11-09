//! Exact checked-history links for reviewed answers, independent of model claims.
//! Validation is local and completes before any browser write or archive update.
use crate::{candidate_history::SelectedRecord, form_preview::FormPreview};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use waypoint_domain::Sha256Digest;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryLink {
    pub entry_id: String,
    pub component: String,
    pub source_sha256: String,
}

/// Preserve the original history-proposal digest encoding.
pub fn source_digest(record: &SelectedRecord, component: &str) -> Option<Sha256Digest> {
    let value = record.values.get(component)?;
    Some(Sha256Digest::of(
        &json!({"entry":record,"component":component,"value":value}).to_string(),
    ))
}

pub fn attach(
    profile: &Value,
    preview: &FormPreview,
    answers: &[Value],
    fields: &mut [Value],
) -> Result<(), String> {
    let linked: Vec<_> = answers
        .iter()
        .filter(|answer| answer.get("history_source").is_some())
        .collect();
    if linked.is_empty() {
        return Ok(());
    }
    if answers.len() > crate::suggestions::MAX_FIELDS {
        return Err("Too many history-linked answers".into());
    }
    let records = crate::candidate_history::reviewed(
        profile["career_history"]
            .as_str()
            .ok_or("Saved career history is no longer available")?,
    )?;
    let profile_digest = Sha256Digest::of(&profile.to_string());
    let schema_digest = Sha256Digest::of(&preview.schema.schema_hash_input());
    let mut pending = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for answer in linked {
        if answer.get("draft_id").is_some() || answer.get("letter_draft_id").is_some() {
            return Err("A history answer cannot also link another draft".into());
        }
        let link: HistoryLink = serde_json::from_value(answer["history_source"].clone())
            .map_err(|_| "Invalid history answer source")?;
        if link.entry_id.len() != 32
            || !link
                .entry_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || link.component.len() > 40
            || link.source_sha256.len() != 64
            || !link
                .source_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid history answer source".into());
        }
        let selector = answer["selector"]
            .as_str()
            .ok_or("History answer field is missing")?;
        if !seen.insert(selector) {
            return Err("History source repeats an answer field".into());
        }
        let field = preview
            .schema
            .fields
            .iter()
            .find(|f| f.selector == selector)
            .ok_or("History answer field is no longer present")?;
        if !crate::history_advice::eligible(field)
            || preview.model_review_selectors.contains(selector)
        {
            return Err(
                "This field requires manual handling; a history source cannot authorize it".into(),
            );
        }
        let record = records
            .iter()
            .find(|r| r.id == link.entry_id)
            .ok_or("The linked history entry is missing or no longer checked")?;
        let digest = source_digest(record, &link.component)
            .ok_or("The linked history component is unavailable")?;
        if digest.as_str() != link.source_sha256 {
            return Err("The linked history source changed; review the answer again".into());
        }
        let saved = record
            .values
            .get(&link.component)
            .ok_or("History component is missing")?;
        let typed = crate::history_advice::value_for(field, &link.component, saved)
            .ok_or("The history component is incompatible with this field or its subject")?;
        if answer["value"].as_str() != Some(saved.as_str()) {
            // A select can expose a casing/whitespace-equivalent label. Only
            // the exact resolved option, not arbitrary case changes, is valid.
            let resolved = match &typed {
                waypoint_applications::form_mapping::PacketValue::Text(text)
                | waypoint_applications::form_mapping::PacketValue::Date(text) => text.as_str(),
                _ => return Err("Unsupported history value".into()),
            };
            if answer["value"].as_str() != Some(resolved) {
                return Err("This answer was edited; remove the saved-history link or select the history suggestion again".into());
            }
        }
        let relationship = match &typed {
            waypoint_applications::form_mapping::PacketValue::Text(text)
            | waypoint_applications::form_mapping::PacketValue::Date(text)
                if text != saved =>
            {
                "unique_matching_option"
            }
            _ => "exact_saved_value",
        };
        let index = fields
            .iter()
            .position(|f| f["selector"].as_str() == Some(selector))
            .ok_or("History-linked answer was not selected for filling")?;
        if fields
            .iter()
            .filter(|f| f["selector"].as_str() == Some(selector))
            .count()
            != 1
            || fields[index]["value"]
                != serde_json::to_value(&typed).map_err(|_| "Cannot encode history value")?
        {
            return Err("The requested field value does not match its saved history source".into());
        }
        // A shared DOM container does not prove a single record: a section
        // may ask about current and previous roles in independently named fields.
        // Validate each explicit source; automatic advice already selects one
        // complete record for its own scoped request.
        pending.push((index,json!({"version":1,"entry_id":record.id,"entry_kind":record.kind,"entry_label":record.label,"component":link.component,"source_sha256":digest.as_str(),"profile_sha256":profile_digest.as_str(),"form_schema_sha256":schema_digest.as_str(),"relationship":relationship,"checked":true,"note":"The requested value was resolved from a user-checked history component. This does not independently verify the experience or confirm filling or submission."})));
    }
    for (index, source) in pending {
        fields[index]["source_career_history"] = source;
    }
    Ok(())
}

/// Viewing history never rewrites its snapshot. These annotations compare only
/// the source entry, not the whole old application or the authenticity of a file.
pub fn current_records(profile: &Value) -> Result<Vec<SelectedRecord>, String> {
    if !profile.is_object() {
        return Err("Saved profile is unreadable".into());
    }
    let raw = match profile.get("career_history") {
        None => "",
        Some(Value::String(raw)) => raw,
        _ => return Err("Saved career history is unreadable".into()),
    };
    if raw.is_empty() {
        Ok(vec![])
    } else {
        crate::candidate_history::reviewed(raw)
    }
}
pub fn current_status(
    records: &Result<Vec<SelectedRecord>, String>,
    source: &Value,
    requested: &Value,
) -> &'static str {
    if source["version"] != 1 {
        return "unsupported_snapshot";
    }
    if source["checked"] != true
        || !matches!(
            source["relationship"].as_str(),
            Some("exact_saved_value" | "unique_matching_option")
        )
    {
        return "invalid_snapshot";
    }
    let (Some(id), Some(component), Some(digest)) = (
        source["entry_id"].as_str(),
        source["component"].as_str(),
        source["source_sha256"].as_str(),
    ) else {
        return "invalid_snapshot";
    };
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || component.len() > 40
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return "invalid_snapshot";
    }
    let Ok(records) = records else {
        return "unavailable";
    };
    let Some(record) = records.iter().find(|record| record.id == id) else {
        return "entry_changed_or_unchecked";
    };
    if source_digest(record, component)
        .as_ref()
        .map(Sha256Digest::as_str)
        != Some(digest)
    {
        return "entry_changed_or_unchecked";
    }
    if source["entry_kind"] != record.kind || source["entry_label"] != record.label {
        return "invalid_snapshot";
    }
    let Some(saved) = record.values.get(component) else {
        return "entry_changed_or_unchecked";
    };
    let Ok(value) = serde_json::from_value::<waypoint_applications::form_mapping::PacketValue>(
        requested.clone(),
    ) else {
        return "invalid_snapshot";
    };
    let matches = match (source["relationship"].as_str(), value) {
        (
            Some("exact_saved_value"),
            waypoint_applications::form_mapping::PacketValue::Text(text)
            | waypoint_applications::form_mapping::PacketValue::Date(text),
        ) => text == *saved,
        (
            Some("unique_matching_option"),
            waypoint_applications::form_mapping::PacketValue::Text(text),
        ) => text.trim().eq_ignore_ascii_case(saved.trim()),
        _ => false,
    };
    if !matches {
        return "invalid_snapshot";
    }
    "entry_unchanged"
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_applications::form_mapping::FormField;
    fn profile() -> Value {
        json!({"career_history":json!({"version":1,"employment":[{"id":"a".repeat(32),"employer":"Fictional & Co","title":"Engineer","start_date":"2020-06","reviewed":true},{"id":"b".repeat(32),"employer":"Other Fictional","title":"Lead","reviewed":true}],"education":[]}).to_string()})
    }
    fn field(selector: &str, label: &str, kind: Value) -> FormField {
        serde_json::from_value(json!({"selector":selector,"label":label,"field_type":kind,"sensitive":false,"required":true,"context":{"sections":[{"id":"group:1","label":"Employment","inferred":false}]}})).unwrap()
    }
    fn preview() -> FormPreview {
        FormPreview {
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "file://".into(),
                fields: vec![
                    field("#company", "Employment — Employer", json!("text")),
                    field("#month", "Employment — Starting month", json!("month")),
                ],
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        }
    }
    fn answer(profile: &Value, selector: &str, component: &str) -> Value {
        let record = crate::candidate_history::selected(
            profile["career_history"].as_str().unwrap(),
            &"a".repeat(32),
        )
        .unwrap();
        json!({"selector":selector,"value":record.values[component],"history_source":{"entry_id":record.id,"component":component,"source_sha256":source_digest(&record,component).unwrap().as_str()}})
    }
    fn requested() -> Vec<Value> {
        vec![
            json!({"selector":"#company","value":{"text":"Fictional & Co"}}),
            json!({"selector":"#month","value":{"date":"2020-06"}}),
        ]
    }
    #[test]
    fn exact_typed_links_are_atomic_and_record_no_unrelated_history() {
        let profile = profile();
        let preview = preview();
        let mut fields = requested();
        attach(
            &profile,
            &preview,
            &[
                answer(&profile, "#company", "employer"),
                answer(&profile, "#month", "start_year_month"),
            ],
            &mut fields,
        )
        .unwrap();
        assert_eq!(
            fields[0]["source_career_history"]["entry_label"],
            "Engineer at Fictional & Co"
        );
        assert_eq!(
            fields[1]["source_career_history"]["component"],
            "start_year_month"
        );
        assert!(!fields[0]["source_career_history"]
            .to_string()
            .contains("Other Fictional"));
        assert_eq!(fields[0]["source_career_history"]["checked"], true);
        let mut fields = requested();
        let mut wrong = answer(&profile, "#month", "start_year_month");
        wrong["value"] = json!("2021-06");
        assert!(attach(
            &profile,
            &preview,
            &[answer(&profile, "#company", "employer"), wrong],
            &mut fields
        )
        .is_err());
        assert_eq!(fields, requested());
    }
    #[test]
    fn edits_stale_unchecked_missing_and_conflicting_sources_are_refused() {
        let profile = profile();
        let preview = preview();
        let good = answer(&profile, "#company", "employer");
        for (key, value) in [
            ("value", json!("Edited")),
            ("draft_id", json!("draft")),
            ("letter_draft_id", json!("letter")),
            ("history_source", Value::Null),
        ] {
            let mut changed = good.clone();
            changed[key] = value;
            assert!(attach(&profile, &preview, &[changed], &mut requested()).is_err());
        }
        for (key, value) in [
            ("entry_id", json!("f".repeat(32))),
            ("component", json!("made_up")),
            ("source_sha256", json!("0".repeat(64))),
            ("extra", json!(true)),
        ] {
            let mut changed = good.clone();
            changed["history_source"][key] = value;
            assert!(attach(&profile, &preview, &[changed], &mut requested()).is_err());
        }
        for (key, value) in [
            ("employer", json!("Corrected Co")),
            ("reviewed", json!(false)),
        ] {
            let mut history: Value =
                serde_json::from_str(profile["career_history"].as_str().unwrap()).unwrap();
            history["employment"][0][key] = value;
            let changed = json!({"career_history":history.to_string()});
            assert!(attach(
                &changed,
                &preview,
                std::slice::from_ref(&good),
                &mut requested()
            )
            .is_err());
        }
        assert!(attach(&profile, &preview, &[good.clone(), good], &mut requested()).is_err());
    }
    #[test]
    fn sources_cannot_bypass_subject_type_review_or_requested_value_checks() {
        let profile = profile();
        let good = answer(&profile, "#company", "employer");
        for case in 0..5 {
            let mut preview = preview();
            match case {
                0 => preview.schema.fields[0].sensitive = true,
                1 => {
                    preview.model_review_selectors.insert("#company".into());
                }
                2 => preview.schema.fields[0].label = "Referee's employer".into(),
                3 => {
                    preview.schema.fields[0].field_type =
                        waypoint_applications::form_mapping::FormFieldType::Email
                }
                _ => preview.schema.fields[0].max_length_utf16 = Some(1),
            };
            assert!(attach(
                &profile,
                &preview,
                std::slice::from_ref(&good),
                &mut requested()
            )
            .is_err());
        }
        let mut fields = requested();
        fields[0]["value"] = json!({"text":"Other"});
        assert!(attach(
            &profile,
            &preview(),
            std::slice::from_ref(&good),
            &mut fields
        )
        .is_err());
        assert!(attach(&profile, &preview(), &[good], &mut []).is_err());
    }
    #[test]
    fn archived_source_status_distinguishes_correction_absence_and_read_failure() {
        let profile = profile();
        let mut fields = requested();
        attach(
            &profile,
            &preview(),
            &[answer(&profile, "#company", "employer")],
            &mut fields,
        )
        .unwrap();
        let source = fields[0]["source_career_history"].clone();
        let requested = fields[0]["value"].clone();
        let records = current_records(&profile);
        assert_eq!(
            current_status(&records, &source, &requested),
            "entry_unchanged"
        );
        assert_eq!(
            current_status(&Ok(vec![]), &source, &requested),
            "entry_changed_or_unchecked"
        );
        assert_eq!(
            current_status(&Err("Unavailable".into()), &source, &requested),
            "unavailable"
        );
        let mut invalid = source.clone();
        invalid["entry_label"] = json!("Invented label");
        assert_eq!(
            current_status(&records, &invalid, &requested),
            "invalid_snapshot"
        );
        invalid["version"] = json!(999);
        assert_eq!(
            current_status(&records, &invalid, &requested),
            "unsupported_snapshot"
        );
        let mut revised: Value =
            serde_json::from_str(profile["career_history"].as_str().unwrap()).unwrap();
        revised["employment"][0]["title"] = json!("Corrected role");
        assert_eq!(
            current_status(
                &current_records(&json!({"career_history":revised.to_string()})),
                &source,
                &requested
            ),
            "entry_changed_or_unchecked"
        );
        assert!(current_records(&json!({"career_history":""}))
            .unwrap()
            .is_empty());
        assert!(current_records(&json!([])).is_err());
        assert_eq!(
            current_status(
                &records,
                &source,
                &json!({"text":"Changed after recording"})
            ),
            "invalid_snapshot"
        );
    }

    #[test]
    fn a_unique_option_can_keep_its_label_without_claiming_byte_exact_copy() {
        let profile = profile();
        let mut preview = preview();
        preview.schema.fields[0].field_type =
            waypoint_applications::form_mapping::FormFieldType::Select {
                options: vec!["fictional & co".into()],
            };
        let mut fields = requested();
        fields[0]["value"] = json!({"text":"fictional & co"});
        let mut linked = answer(&profile, "#company", "employer");
        linked["value"] = json!("fictional & co");
        attach(
            &profile,
            &preview,
            std::slice::from_ref(&linked),
            &mut fields,
        )
        .unwrap();
        assert_eq!(
            fields[0]["source_career_history"]["relationship"],
            "unique_matching_option"
        );
        preview.schema.fields[0].field_type =
            waypoint_applications::form_mapping::FormFieldType::Select {
                options: vec!["fictional & co".into(), "Fictional & Co".into()],
            };
        assert!(attach(&profile, &preview, &[linked], &mut requested()).is_err());
    }
    #[test]
    fn independently_reviewed_fields_can_name_different_records_in_one_container() {
        let profile = profile();
        let mut preview = preview();
        preview.schema.fields[1] = field("#month", "Employment — Title", json!("text"));
        let record = crate::candidate_history::selected(
            profile["career_history"].as_str().unwrap(),
            &"b".repeat(32),
        )
        .unwrap();
        let other = json!({"selector":"#month","value":"Lead","history_source":{"entry_id":record.id,"component":"title","source_sha256":source_digest(&record,"title").unwrap().as_str()}});
        let answers = [answer(&profile, "#company", "employer"), other];
        let mut fields = requested();
        fields[1]["value"] = json!({"text":"Lead"});
        attach(&profile, &preview, &answers, &mut fields).unwrap();
        assert_eq!(
            fields[0]["source_career_history"]["entry_id"],
            "a".repeat(32)
        );
        preview.schema.fields[1].context.sections[0].id = "group:2".into();
        attach(&profile, &preview, &answers, &mut fields).unwrap();
        assert_eq!(
            fields[1]["source_career_history"]["entry_id"],
            "b".repeat(32)
        );
    }
}
