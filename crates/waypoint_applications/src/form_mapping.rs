//! T027/T028 — dedicated browser session + typed form mapping.
//!
//! The model (or user) proposes a *typed* mapping of packet fields to form
//! fields. The policy layer decides allowability; the browser engine receives
//! only narrow commands — never scripts. Prefill is an information-disclosure
//! boundary requiring its own grant, separate from the submit grant.

use serde::{Deserialize, Serialize};

/// A field type the engine can fill. No arbitrary JS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormFieldType {
    Text,
    Email,
    TextArea,
    Select { options: Vec<String> },
    Checkbox,
    File,
    Date,
    Radio { options: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub selector: String, // stable CSS selector from the form schema
    pub label: String,
    pub field_type: FormFieldType,
    pub required: bool,
    pub sensitive: bool, // salary/authorization/demographic/attestation
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormSchema {
    pub origin: String,
    pub fields: Vec<FormField>,
    pub multi_step: bool,
    pub steps: u8,
}

impl FormSchema {
    /// Deterministic schema identity used in approval grants.
    pub fn schema_hash_input(&self) -> String {
        let mut parts = vec![
            self.origin.clone(),
            self.multi_step.to_string(),
            self.steps.to_string(),
        ];
        for f in &self.fields {
            parts.push(format!(
                "{}|{}|{:?}|{}|{}",
                f.selector, f.label, f.field_type, f.required, f.sensitive
            ));
        }
        parts.join("\n")
    }
}

/// A typed value a packet can supply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketValue {
    Text(String),
    Bool(bool),
    Date(String),
    FileRef { name: String },
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedMapping {
    pub field_selector: String,
    pub value: PacketValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingError {
    UnknownField {
        selector: String,
    },
    TypeMismatch {
        selector: String,
        why: String,
    },
    /// Sensitive fields require explicit user answers; model cannot guess.
    SensitiveFieldNeedsUserAnswer {
        selector: String,
    },
    MissingRequired {
        selector: String,
    },
}

/// Validate a model-proposed mapping against the actual form schema and the
/// set of explicit user answers. The model can never satisfy a sensitive
/// field on its own.
pub fn validate_mapping(
    schema: &FormSchema,
    proposals: &[ProposedMapping],
    explicit_user_answers: &[(String, PacketValue)],
) -> Result<(), Vec<MappingError>> {
    let mut issues = vec![];
    let user: std::collections::HashMap<&str, &PacketValue> = explicit_user_answers
        .iter()
        .map(|(s, v)| (s.as_str(), v))
        .collect();

    for f in &schema.fields {
        let prop = proposals.iter().find(|p| p.field_selector == f.selector);
        let user_answer = user.get(f.selector.as_str());

        if f.sensitive && user_answer.is_none() {
            issues.push(MappingError::SensitiveFieldNeedsUserAnswer {
                selector: f.selector.clone(),
            });
            continue;
        }
        let value = if f.sensitive {
            user_answer.copied().unwrap()
        } else {
            prop.map(|p| &p.value).unwrap_or(&PacketValue::None)
        };

        if f.required && matches!(value, PacketValue::None) {
            issues.push(MappingError::MissingRequired {
                selector: f.selector.clone(),
            });
            continue;
        }
        if matches!(value, PacketValue::None) {
            continue;
        }

        let type_ok = match (&f.field_type, value) {
            (FormFieldType::Text, PacketValue::Text(_))
            | (FormFieldType::Email, PacketValue::Text(_))
            | (FormFieldType::TextArea, PacketValue::Text(_))
            | (FormFieldType::Date, PacketValue::Date(_))
            | (FormFieldType::Checkbox, PacketValue::Bool(_)) => true,
            (FormFieldType::Select { options }, PacketValue::Text(t)) => options.contains(t),
            (FormFieldType::Radio { options }, PacketValue::Text(t)) => options.contains(t),
            (FormFieldType::File, PacketValue::FileRef { .. }) => true,
            _ => false,
        };
        if !type_ok {
            issues.push(MappingError::TypeMismatch {
                selector: f.selector.clone(),
                why: format!("{:?} cannot accept {:?}", f.field_type, value),
            });
        }
    }

    for p in proposals {
        if !schema.fields.iter().any(|f| f.selector == p.field_selector) {
            issues.push(MappingError::UnknownField {
                selector: p.field_selector.clone(),
            });
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

/// A narrow command the browser engine may execute. There is deliberately no
/// RunScript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserCommand {
    Navigate {
        url: String,
    },
    Fill {
        selector: String,
        value: PacketValue,
    },
    Click {
        selector: String,
    },
    ReadText {
        selector: String,
    },
    UploadFile {
        selector: String,
        file_name: String,
    },
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> FormSchema {
        FormSchema {
            origin: "https://jobs.acme.com".into(),
            fields: vec![
                FormField {
                    selector: "#name".into(),
                    label: "Full name".into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: false,
                },
                FormField {
                    selector: "#salary".into(),
                    label: "Expected salary".into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: true,
                },
                FormField {
                    selector: "#country".into(),
                    label: "Country".into(),
                    field_type: FormFieldType::Select {
                        options: vec!["CH".into(), "DE".into()],
                    },
                    required: true,
                    sensitive: false,
                },
            ],
            multi_step: false,
            steps: 1,
        }
    }

    #[test]
    fn sensitive_fields_reject_model_only_answers() {
        let proposals = vec![
            ProposedMapping {
                field_selector: "#name".into(),
                value: PacketValue::Text("Ada".into()),
            },
            ProposedMapping {
                field_selector: "#salary".into(),
                value: PacketValue::Text("120k".into()),
            },
            ProposedMapping {
                field_selector: "#country".into(),
                value: PacketValue::Text("CH".into()),
            },
        ];
        // Model filled the sensitive salary field itself -> error.
        let err = validate_mapping(&schema(), &proposals, &[]).unwrap_err();
        assert!(matches!(
            err[0],
            MappingError::SensitiveFieldNeedsUserAnswer { .. }
        ));
    }

    #[test]
    fn explicit_user_answer_satisfies_sensitive_field() {
        let proposals = vec![
            ProposedMapping {
                field_selector: "#name".into(),
                value: PacketValue::Text("Ada".into()),
            },
            ProposedMapping {
                field_selector: "#country".into(),
                value: PacketValue::Text("CH".into()),
            },
        ];
        let user = vec![(
            "#salary".to_string(),
            PacketValue::Text("130000 CHF".into()),
        )];
        assert!(validate_mapping(&schema(), &proposals, &user).is_ok());
    }

    #[test]
    fn unknown_select_option_is_type_mismatch() {
        let proposals = vec![
            ProposedMapping {
                field_selector: "#name".into(),
                value: PacketValue::Text("Ada".into()),
            },
            ProposedMapping {
                field_selector: "#country".into(),
                value: PacketValue::Text("US".into()),
            },
        ];
        let user = vec![("#salary".to_string(), PacketValue::Text("130000".into()))];
        let err = validate_mapping(&schema(), &proposals, &user).unwrap_err();
        assert!(matches!(err[0], MappingError::TypeMismatch { .. }));
    }

    #[test]
    fn missing_required_field_is_reported() {
        let user = vec![("#salary".to_string(), PacketValue::Text("130000".into()))];
        let proposals = vec![ProposedMapping {
            field_selector: "#country".into(),
            value: PacketValue::Text("CH".into()),
        }];
        let err = validate_mapping(&schema(), &proposals, &user).unwrap_err();
        assert!(matches!(err[0], MappingError::MissingRequired { .. }));
    }

    #[test]
    fn schema_hash_is_deterministic_and_order_sensitive() {
        let a = schema();
        let b = schema();
        assert_eq!(a.schema_hash_input(), b.schema_hash_input());
    }
}
