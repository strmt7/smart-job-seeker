//! Read-only native proof for an additive, same-document field transition.
use super::*;

impl CdpDriver {
    pub(super) fn recorded_field_backend(&mut self, selector: &str) -> Result<u64, DriverError> {
        let node = self
            .node_id(selector)?
            .ok_or_else(|| DriverError("The inspected field disappeared".into()))?;
        let described = self
            .cdp
            .call("DOM.describeNode", json!({"nodeId":node}))
            .map_err(derr)?;
        described
            .pointer("/node/backendNodeId")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .ok_or_else(|| DriverError("The field's native identity is unavailable".into()))
    }
}

pub(super) fn additive_recorded_form(before: &LiveForm, after: &LiveForm) -> bool {
    use std::collections::BTreeMap;
    if !before.shares_inspected_native_scope(after)
        || before.walls != after.walls
        || before.schema.origin != after.schema.origin
        || before.schema.steps != after.schema.steps
        || before.schema.multi_step != after.schema.multi_step
        || after.schema.fields.len() <= before.schema.fields.len()
    {
        return false;
    }
    let first: BTreeMap<_, _> = before
        .schema
        .fields
        .iter()
        .map(|field| (field.selector.as_str(), field))
        .collect();
    let next: BTreeMap<_, _> = after
        .schema
        .fields
        .iter()
        .map(|field| (field.selector.as_str(), field))
        .collect();
    first.len() == before.schema.fields.len()
        && next.len() == after.schema.fields.len()
        && first
            .iter()
            .all(|(selector, field)| next.get(selector) == Some(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_applications::form_mapping::{FormField, FormSchema};

    fn field(selector: &str) -> FormField {
        serde_json::from_value(json!({"selector":selector,"label":"Profile name",
            "field_type":"text","required":true,"sensitive":false}))
        .unwrap()
    }
    fn inspected(fields: Vec<FormField>, scope: Option<&str>) -> LiveForm {
        LiveForm::from_scoped_inspection(
            FormSchema {
                origin: "https://fixture.invalid".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            vec![],
            vec![],
            scope.map(str::to_owned),
        )
    }
    #[test]
    fn additions_preserve_all_existing_descriptors_and_current_scope() {
        let before = inspected(vec![field("#first")], Some("native-a"));
        let after = inspected(vec![field("#first"), field("#second")], Some("native-a"));
        assert!(additive_recorded_form(&before, &after));
        assert!(!additive_recorded_form(&before, &before));
        assert!(!additive_recorded_form(&after, &before));
        for changed in ["label", "selector", "required", "sensitive", "context"] {
            let mut value = serde_json::to_value(&after.schema.fields[0]).unwrap();
            value[changed] = match changed {
                "required" => json!(false),
                "sensitive" => json!(true),
                "context" => {
                    json!({"sections":[{"id":"other","label":"Another person","inferred":false}]})
                }
                _ => json!("different"),
            };
            let mut candidate = after.clone();
            candidate.schema.fields[0] = serde_json::from_value(value).unwrap();
            // Reconstruct inspection rather than credit stale manufactured evidence.
            let candidate = inspected(candidate.schema.fields, Some("native-a"));
            assert!(!additive_recorded_form(&before, &candidate), "{changed}");
        }
    }
    #[test]
    fn missing_stale_changed_or_duplicate_evidence_cannot_continue() {
        let before = inspected(vec![field("#first")], Some("native-a"));
        let after = inspected(vec![field("#first"), field("#second")], Some("native-a"));
        let changed = inspected(after.schema.fields.clone(), Some("native-b"));
        assert!(!additive_recorded_form(&before, &changed));
        assert!(!additive_recorded_form(
            &before,
            &inspected(after.schema.fields.clone(), None)
        ));
        let mut stale = after.clone();
        stale.schema.fields.push(field("#third"));
        assert!(!additive_recorded_form(&before, &stale));
        let mut duplicate = after.schema.fields.clone();
        duplicate.push(field("#first"));
        assert!(!additive_recorded_form(
            &before,
            &inspected(duplicate, Some("native-a"))
        ));
        let mut wall = after;
        wall.walls.push("Uninspected scope".into());
        assert!(!additive_recorded_form(&before, &wall));
    }
}
