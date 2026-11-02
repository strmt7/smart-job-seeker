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
    Number {
        min: Option<String>,
        max: Option<String>,
        step: Option<String>,
    },
    Email,
    TextArea,
    Select {
        options: Vec<String>,
    },
    MultiSelect {
        options: Vec<String>,
    },
    Checkbox,
    File,
    Date,
    Month,
    Time,
    DateTimeLocal,
    Radio {
        options: Vec<String>,
    },
}

/// Distinguish editing hosts from native text controls for review and binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditingMode {
    PlainText,
    RichText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub selector: String, // stable CSS selector from the form schema
    pub label: String,
    pub field_type: FormFieldType,
    pub required: bool,
    pub sensitive: bool, // salary/authorization/demographic/attestation
    #[serde(default)]
    pub editing_mode: Option<EditingMode>,
    /// HTML maxlength for native text controls, measured in UTF-16 code units.
    #[serde(default)]
    pub max_length_utf16: Option<u32>,
    /// HTML minlength for nonempty native text values, in UTF-16 code units.
    #[serde(default)]
    pub min_length_utf16: Option<u32>,
    /// Bounded HTML pattern supplied by the page, not executable model instructions.
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub autocomplete: Option<String>,
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
        // JSON escapes separators and preserves field boundaries. The version
        // prefix also invalidates approvals created with the old encoding.
        format!(
            "form-schema-v2:{}",
            serde_json::to_string(self).expect("form schemas contain only serializable values")
        )
    }
}

/// HTML text field limits count UTF-16 code units, not bytes or Unicode scalars.
pub fn text_fits_field(field: &FormField, text: &str) -> bool {
    let length = text.encode_utf16().count();
    field
        .max_length_utf16
        .is_none_or(|limit| length <= limit as usize)
        && (text.is_empty()
            || field
                .min_length_utf16
                .is_none_or(|limit| length >= limit as usize))
}

/// A typed value a packet can supply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketValue {
    Text(String),
    TextList(Vec<String>),
    Bool(bool),
    Date(String),
    FileRef { name: String },
    None,
}

pub fn valid_multi_select(options: &[String], values: &[String]) -> bool {
    !values.is_empty()
        && values.len() <= 100
        && values.iter().all(|value| {
            !value.trim().is_empty()
                && options
                    .iter()
                    .filter(|option| option.trim().eq_ignore_ascii_case(value.trim()))
                    .count()
                    == 1
        })
        && values.iter().enumerate().all(|(i, value)| {
            !values[..i]
                .iter()
                .any(|previous| previous.trim().eq_ignore_ascii_case(value.trim()))
        })
}

/// Validate a finite, unpadded HTML-style numeric answer. Browser constraints
/// such as min, max and step are checked on the live control before entry.
pub fn valid_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    let start = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    let integer_digits = index - start;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fractional_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == fractional_start {
            return false;
        }
    } else if integer_digits == 0 {
        return false;
    }
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        if bytes
            .get(index)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            index += 1;
        }
        let exponent_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_start {
            return false;
        }
    }
    index == bytes.len() && value.parse::<f64>().is_ok_and(f64::is_finite)
}

/// Accept the year and month syntax used by native month inputs.
pub fn valid_iso_month(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || c.is_ascii_digit())
        && value[0..4].parse::<u32>().is_ok_and(|year| year > 0)
        && value[5..7]
            .parse::<u32>()
            .is_ok_and(|month| (1..=12).contains(&month))
}

/// A minute-precision time accepted by native time inputs.
pub fn valid_iso_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 5
        && bytes[2] == b':'
        && [0, 1, 3, 4].iter().all(|&i| bytes[i].is_ascii_digit())
        && value[0..2].parse::<u32>().is_ok_and(|hour| hour < 24)
        && value[3..5].parse::<u32>().is_ok_and(|minute| minute < 60)
}

/// A local date and time at minute precision, without a timezone.
pub fn valid_iso_datetime_local(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 16
        && b[10] == b'T'
        && b.iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13) || c.is_ascii_digit())
        && b[4] == b'-'
        && b[7] == b'-'
        && b[13] == b':'
        && valid_iso_date(&value[..10])
        && valid_iso_time(&value[11..])
}

/// Accept a complete Gregorian calendar date in the format used by native date inputs.
pub fn valid_iso_date(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return false;
    }
    let year = value[0..4].parse::<u32>().unwrap_or(0);
    let month = value[5..7].parse::<u32>().unwrap_or(0);
    let day = value[8..10].parse::<u32>().unwrap_or(0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days
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

        let missing_required = match (&f.field_type, value) {
            (_, PacketValue::None) => true,
            (_, PacketValue::Text(text) | PacketValue::Date(text)) => text.trim().is_empty(),
            (FormFieldType::Checkbox, PacketValue::Bool(checked)) => !checked,
            (_, PacketValue::TextList(values)) => values.is_empty(),
            (_, PacketValue::FileRef { name }) => name.trim().is_empty(),
            _ => false,
        };
        if f.required && missing_required {
            issues.push(MappingError::MissingRequired {
                selector: f.selector.clone(),
            });
            continue;
        }
        if matches!(value, PacketValue::None) {
            continue;
        }

        let type_ok = match (&f.field_type, value) {
            (
                FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea,
                PacketValue::Text(text),
            ) => text_fits_field(f, text),
            (FormFieldType::Number { .. }, PacketValue::Text(value)) => valid_number(value),
            (FormFieldType::Date, PacketValue::Date(date)) => valid_iso_date(date),
            (FormFieldType::Month, PacketValue::Date(date)) => valid_iso_month(date),
            (FormFieldType::Time, PacketValue::Date(time)) => valid_iso_time(time),
            (FormFieldType::DateTimeLocal, PacketValue::Date(value)) => {
                valid_iso_datetime_local(value)
            }
            (FormFieldType::Checkbox, PacketValue::Bool(_)) => true,
            (FormFieldType::Select { options }, PacketValue::Text(t)) => options.contains(t),
            (FormFieldType::MultiSelect { options }, PacketValue::TextList(values)) => {
                valid_multi_select(options, values)
            }
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
    /// Select a single native option by its reviewed visible label.
    SelectOption {
        selector: String,
        label: String,
    },
    SelectOptions {
        selector: String,
        labels: Vec<String>,
    },
    /// Choose a reviewed option within a native fieldset radio group.
    SelectRadio {
        selector: String,
        label: String,
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
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: None,
                },
                FormField {
                    selector: "#salary".into(),
                    label: "Expected salary".into(),
                    field_type: FormFieldType::Text,
                    required: true,
                    sensitive: true,
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: None,
                },
                FormField {
                    selector: "#country".into(),
                    label: "Country".into(),
                    field_type: FormFieldType::Select {
                        options: vec!["CH".into(), "DE".into()],
                    },
                    required: true,
                    sensitive: false,
                    editing_mode: None,
                    max_length_utf16: None,
                    min_length_utf16: None,
                    pattern: None,
                    autocomplete: None,
                },
            ],
            multi_step: false,
            steps: 1,
        }
    }

    #[test]
    fn native_minimum_counts_utf16_and_allows_empty_optional_values() {
        let mut field = schema().fields.remove(0);
        field.required = false;
        field.min_length_utf16 = Some(3);
        field.max_length_utf16 = Some(4);
        assert!(text_fits_field(&field, ""));
        assert!(!text_fits_field(&field, "😀"));
        assert!(text_fits_field(&field, "A😀"));
        assert!(text_fits_field(&field, "😀😀"));
        assert!(!text_fits_field(&field, "A😀😀"));
    }

    #[test]
    fn native_month_requires_a_valid_four_digit_year_and_month() {
        for valid in ["2020-01", "9999-12"] {
            assert!(valid_iso_month(valid));
        }
        for invalid in [
            "0000-01",
            "2020-00",
            "2020-13",
            "20-01",
            "2020-1",
            "2020-01-01",
            "é020-01",
        ] {
            assert!(!valid_iso_month(invalid));
        }
    }

    #[test]
    fn native_time_requires_a_valid_minute_precision_time() {
        for value in ["00:00", "09:05", "23:59"] {
            assert!(valid_iso_time(value));
        }
        for value in ["24:00", "12:60", "9:05", "09:05:00", "é:05", ""] {
            assert!(!valid_iso_time(value));
        }
    }

    #[test]
    fn local_datetime_requires_a_complete_date_and_minute_without_timezone() {
        for value in ["2024-02-29T00:00", "2026-11-01T23:59"] {
            assert!(valid_iso_datetime_local(value));
        }
        for value in [
            "2026-02-29T12:00",
            "2026-11-01T24:00",
            "2026-11-01T12:00Z",
            "2026-11-01 12:00",
            "é026-11-01T12:00",
            "",
        ] {
            assert!(!valid_iso_datetime_local(value));
        }
        assert_eq!(
            serde_json::to_value(FormFieldType::DateTimeLocal).unwrap(),
            "date_time_local"
        );
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
    fn required_values_and_dates_are_checked_before_form_commands() {
        let mut form = schema();
        form.fields.truncate(1);
        let proposal = |value| {
            vec![ProposedMapping {
                field_selector: "#name".into(),
                value,
            }]
        };
        assert!(matches!(
            validate_mapping(&form, &proposal(PacketValue::Text("  ".into())), &[]).unwrap_err()[0],
            MappingError::MissingRequired { .. }
        ));
        form.fields[0].field_type = FormFieldType::Checkbox;
        assert!(matches!(
            validate_mapping(&form, &proposal(PacketValue::Bool(false)), &[]).unwrap_err()[0],
            MappingError::MissingRequired { .. }
        ));
        form.fields[0].field_type = FormFieldType::File;
        assert!(matches!(
            validate_mapping(
                &form,
                &proposal(PacketValue::FileRef { name: " ".into() }),
                &[]
            )
            .unwrap_err()[0],
            MappingError::MissingRequired { .. }
        ));
        form.fields[0].field_type = FormFieldType::MultiSelect {
            options: vec!["A".into()],
        };
        assert!(matches!(
            validate_mapping(&form, &proposal(PacketValue::TextList(vec![])), &[]).unwrap_err()[0],
            MappingError::MissingRequired { .. }
        ));
        form.fields[0].field_type = FormFieldType::Checkbox;
        form.fields[0].required = false;
        assert!(validate_mapping(&form, &proposal(PacketValue::Bool(false)), &[]).is_ok());
        form.fields[0].field_type = FormFieldType::Date;
        assert!(validate_mapping(
            &form,
            &proposal(PacketValue::Date("2024-02-29".into())),
            &[]
        )
        .is_ok());
        assert!(matches!(
            validate_mapping(
                &form,
                &proposal(PacketValue::Date("2025-02-29".into())),
                &[]
            )
            .unwrap_err()[0],
            MappingError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn numeric_answers_require_finite_unpadded_numbers() {
        for value in ["0", "5", "-2", "1.5", ".5", "1e3", "1e+2"] {
            assert!(valid_number(value), "{value}");
        }
        for value in [
            "", "-", ".", "1.", "1.e2", "1e", "five", "5 years", " 5", "+5", "NaN", "inf", "1e999",
        ] {
            assert!(!valid_number(value), "{value}");
        }
    }

    #[test]
    fn schema_hash_is_deterministic_and_order_sensitive() {
        let a = schema();
        let b = schema();
        assert_eq!(a.schema_hash_input(), b.schema_hash_input());
        let mut changed = b;
        changed.fields[0].autocomplete = Some("family-name".into());
        assert_ne!(a.schema_hash_input(), changed.schema_hash_input());
        let mut changed_mode = a.clone();
        changed_mode.fields[0].editing_mode = Some(EditingMode::RichText);
        assert_ne!(a.schema_hash_input(), changed_mode.schema_hash_input());
        let mut first = a.clone();
        first.fields[0].selector = "#field|extra".into();
        first.fields[0].label = "Name".into();
        let mut second = a.clone();
        second.fields[0].selector = "#field".into();
        second.fields[0].label = "extra|Name".into();
        assert_ne!(first.schema_hash_input(), second.schema_hash_input());
        let old = serde_json::json!({"selector":"#answer","label":"Answer","field_type":"text_area","required":false,"sensitive":false});
        let restored: FormField = serde_json::from_value(old).unwrap();
        assert_eq!(restored.editing_mode, None);
    }
}
