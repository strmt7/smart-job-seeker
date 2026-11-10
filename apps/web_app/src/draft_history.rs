//! Checked history is candidate-entered drafting material, not an attested claim.
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Source {
    record: crate::candidate_history::SelectedRecord,
    statement: String,
    digest: String,
}

/// Keep complete records. An oversized statement is omitted by the existing
/// prompt budget with explicit accounting, never cut across a qualification.
pub fn augment(facts: &mut Vec<String>, profile: &Value) -> Result<Vec<Source>, String> {
    let raw = match &profile["career_history"] {
        Value::Null => return Ok(Vec::new()),
        Value::String(raw) if raw.is_empty() => return Ok(Vec::new()),
        Value::String(raw) => raw,
        _ => return Err("Stored career history is invalid".into()),
    };
    let mut sources = Vec::new();
    for record in crate::candidate_history::reviewed(raw)? {
        let values: serde_json::Map<_, _> = [
            "employer",
            "title",
            "institution",
            "qualification",
            "field_of_study",
            "location",
            "start_date",
            "end_date",
            "description",
        ]
        .into_iter()
        .map(|key| (key.to_owned(), json!(record.values.get(key))))
        .collect();
        let ongoing = if record.ongoing_confirmed {
            Some(true)
        } else if record.values.contains_key("end_date") {
            Some(false)
        } else {
            None
        };
        let statement = json!({
            "source_type":"candidate_entered_history",
            "record_type":record.kind,
            "values":values,
            "ongoing":ongoing,
            "notice":"Checked for reuse, not independently verified. Null values are unknown. Dates retain their supplied precision. Education completion is not established by a record alone."
        }).to_string();
        let digest = waypoint_domain::sha256_hex(
            &serde_json::to_vec(&record).map_err(|_| "Cannot bind career history source")?,
        );
        if !facts.contains(&statement) {
            facts.push(statement.clone());
        }
        sources.push(Source {
            record,
            statement,
            digest,
        });
    }
    Ok(sources)
}

/// Bind only material actually supplied to generation, with 1-based citations.
pub fn used(sources: &[Source], facts: &[String]) -> Vec<Value> {
    sources
        .iter()
        .filter_map(|source| {
            facts
                .iter()
                .position(|fact| fact == &source.statement)
                .map(|index| {
                    json!({
                        "fact":index+1,"kind":source.record.kind,"record_id":source.record.id,
                        "label":source.record.label,"source_sha256":source.digest,
                        "statement_sha256":waypoint_domain::sha256_hex(source.statement.as_bytes()),
                        "verification":"candidate_entered_checked_for_reuse"
                    })
                })
        })
        .collect()
}

/// Shared profile contact values for drafting and final-document comparison.
pub fn contact_facts(profile: &Value) -> Vec<String> {
    [
        ("full_name", "Full name"),
        ("first_name", "First name"),
        ("last_name", "Last name"),
        ("email", "Email"),
        ("phone", "Phone"),
        ("city", "City"),
        ("linkedin", "LinkedIn"),
        ("website", "Website"),
    ]
    .into_iter()
    .filter_map(|(key, label)| {
        profile[key]
            .as_str()
            .filter(|v| !v.trim().is_empty() && v.len() <= 500)
            .map(|v| format!("{label}: {v}"))
    })
    .collect()
}

/// Whole-source budget, including JSON escaping. Missing sources remain unknown.
pub fn review_sources(
    reviewed: &[String],
    profile: &Value,
    listing: &str,
) -> Result<Value, String> {
    let mut candidates = reviewed.to_vec();
    let history = augment(&mut candidates, profile)?;
    let contacts = contact_facts(profile);
    let available = candidates.len() + contacts.len();
    let mut ordered = contacts;
    ordered.extend(crate::fit_semantic_pool(&candidates, listing));
    let mut sources = Vec::new();
    let mut bytes = 2usize;
    for source in ordered {
        let size = serde_json::to_string(&source)
            .map_err(|_| "Cannot encode document source")?
            .len()
            + 1;
        if bytes + size <= 3500 && !sources.contains(&source) {
            bytes += size;
            sources.push(source);
        }
    }
    Ok(
        json!({"sources":sources,"available":available,"supplied":sources.len(),
        "omitted":available.saturating_sub(sources.len()),"checked_history_available":history.len(),
        "scope":"Up to 500 reviewed statements, checked career history and profile contact values. Candidate-entered, not independently verified. Missing sources leave factual support unknown."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Value {
        json!({"career_history":json!({"version":1,"employment":[
            {"id":"a".repeat(32),"employer":"Fictional North","title":"Engineer","start_date":"2020","description":"Built measurement tools.","reviewed":true},
            {"id":"b".repeat(32),"employer":"Unreviewed East","title":"Director","reviewed":false}],
            "education":[{"id":"c".repeat(32),"institution":"Fictional University","qualification":"MSc","start_date":"2024-09","current":true,"reviewed":true}]}).to_string()})
    }
    #[test]
    fn only_checked_records_are_added_without_inventing_precision_or_completion() {
        let mut facts = vec!["Existing reviewed statement".into()];
        let sources = augment(&mut facts, &profile()).unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(facts.len(), 3);
        let employment: Value = serde_json::from_str(&facts[1]).unwrap();
        assert_eq!(employment["values"]["start_date"], "2020");
        assert!(employment["values"]["end_date"].is_null());
        assert!(employment["ongoing"].is_null());
        let education: Value = serde_json::from_str(&facts[2]).unwrap();
        assert_eq!(education["values"]["start_date"], "2024-09");
        assert_eq!(education["ongoing"], true);
        assert!(!facts.iter().any(|text| text.contains("Unreviewed East")));
    }
    #[test]
    fn source_bindings_follow_selected_fact_numbers_and_change_with_history() {
        let mut facts = Vec::new();
        let sources = augment(&mut facts, &profile()).unwrap();
        let selected = vec!["Other candidate fact".into(), facts[1].clone()];
        let used = used(&sources, &selected);
        assert_eq!(used.len(), 1);
        assert_eq!(used[0]["fact"], 2);
        assert_eq!(used[0]["record_id"], "c".repeat(32));
        assert_eq!(
            used[0]["verification"],
            "candidate_entered_checked_for_reuse"
        );
        let before = sources[0].digest.clone();
        let mut profile = profile();
        let mut raw: Value =
            serde_json::from_str(profile["career_history"].as_str().unwrap()).unwrap();
        raw["employment"][0]["description"] = json!("Corrected measurement work.");
        profile["career_history"] = json!(raw.to_string());
        let changed = augment(&mut Vec::new(), &profile).unwrap();
        assert_ne!(before, changed[0].digest);
    }
    #[test]
    fn long_descriptions_are_complete_and_invalid_history_is_not_silently_ignored() {
        let mut profile = profile();
        let mut raw: Value =
            serde_json::from_str(profile["career_history"].as_str().unwrap()).unwrap();
        let description = "No clinical authority. ".repeat(100);
        raw["employment"][0]["description"] = json!(description);
        profile["career_history"] = json!(raw.to_string());
        let mut facts = Vec::new();
        augment(&mut facts, &profile).unwrap();
        let statement: Value = serde_json::from_str(&facts[0]).unwrap();
        assert_eq!(statement["values"]["description"], description);
        assert!(augment(&mut Vec::new(), &json!({"career_history":42})).is_err());
        assert!(augment(&mut Vec::new(), &json!({"career_history":"damaged"})).is_err());
        assert!(augment(&mut Vec::new(), &json!({})).unwrap().is_empty());
    }

    #[test]
    fn visual_review_uses_the_same_checked_history_and_contact_values_with_atomic_coverage() {
        let mut profile = profile();
        profile["first_name"] = json!("Avery");
        profile["email"] = json!("avery@fixture.invalid");
        let review = review_sources(&[], &profile, "Engineer").unwrap();
        assert_eq!(review["checked_history_available"], 2);
        assert_eq!(review["available"], 4);
        assert_eq!(review["supplied"], 4);
        assert_eq!(review["omitted"], 0);
        let sources = review["sources"].as_array().unwrap();
        assert!(sources.iter().any(|v| v == "Email: avery@fixture.invalid"));
        assert!(sources
            .iter()
            .any(|v| v.as_str().unwrap().contains("Fictional North")));
        assert!(!sources
            .iter()
            .any(|v| v.as_str().unwrap().contains("Unreviewed East")));
        let oversized = "Do not claim clinical expertise. ".repeat(180);
        let partial =
            review_sources(std::slice::from_ref(&oversized), &profile, "Clinical").unwrap();
        assert!(partial["omitted"].as_u64().unwrap() >= 1);
        assert!(partial["sources"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| !v.as_str().unwrap().starts_with("Do not claim clinical")));
        assert!(serde_json::to_string(&partial["sources"]).unwrap().len() <= 3500);
    }
}
