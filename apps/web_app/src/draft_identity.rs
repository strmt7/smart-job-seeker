//! Profile-sourced signatures and one canonical generated-document composition.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Signature {
    Available {
        text: String,
        source_fields: Vec<String>,
        source_sha256: String,
    },
    #[default]
    Missing,
    Invalid {
        source_fields: Vec<String>,
    },
}

fn usable_name(raw: &str) -> bool {
    !raw.trim().is_empty() && raw.trim().len() <= 200 && !raw.chars().any(char::is_control)
}
fn supplied(value: &Value) -> bool {
    !value.is_null()
        && value
            .as_str()
            .is_none_or(|text| !text.trim().is_empty() || text.chars().any(char::is_control))
}

impl Signature {
    pub fn from_profile(profile: &Value) -> Self {
        let full = &profile["full_name"];
        let fields = if supplied(full) {
            vec!["full_name"]
        } else {
            ["first_name", "last_name"]
                .into_iter()
                .filter(|key| supplied(&profile[key]))
                .collect()
        };
        if fields.is_empty() {
            return Self::Missing;
        }
        if fields
            .iter()
            .any(|key| !profile[key].as_str().is_some_and(usable_name))
        {
            return Self::Invalid {
                source_fields: fields.into_iter().map(str::to_owned).collect(),
            };
        }
        let text = fields
            .iter()
            .map(|key| profile[key].as_str().unwrap_or_default().trim())
            .collect::<Vec<_>>()
            .join(" ");
        if !usable_name(&text) {
            return Self::Invalid {
                source_fields: fields.into_iter().map(str::to_owned).collect(),
            };
        }
        let sources: serde_json::Map<String, Value> = fields
            .iter()
            .map(|key| ((*key).to_owned(), profile[key].clone()))
            .collect();
        Self::Available {
            text,
            source_fields: fields.into_iter().map(str::to_owned).collect(),
            source_sha256: waypoint_domain::sha256_hex(
                Value::Object(sources).to_string().as_bytes(),
            ),
        }
    }
}

/// Shared by generation and export provenance. Old drafts without signature
/// metadata remain readable; a malformed new signature never silently passes.
pub fn compose(draft: &Value, kind: &str) -> Result<String, String> {
    let paragraphs = draft["paragraphs"]
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= 8)
        .ok_or("Stored draft is invalid")?;
    let mut pieces = Vec::new();
    if let Some(opening) = draft["framing"]["opening"]
        .as_str()
        .filter(|text| !text.is_empty())
    {
        pieces.push(opening.to_owned());
    }
    for paragraph in paragraphs {
        pieces.push(
            paragraph["text"]
                .as_str()
                .filter(|text| !text.is_empty())
                .ok_or("Stored draft is invalid")?
                .to_owned(),
        );
    }
    let mut ending = Vec::new();
    if let Some(closing) = draft["framing"]["closing"]
        .as_str()
        .filter(|text| !text.is_empty())
    {
        ending.push(closing.to_owned());
    }
    if kind == "cover_letter" && !draft["signature"].is_null() {
        let signature: Signature = serde_json::from_value(draft["signature"].clone())
            .map_err(|_| "Stored signature is invalid")?;
        if let Signature::Available {
            text,
            source_fields,
            source_sha256,
        } = signature
        {
            if !usable_name(&text)
                || source_fields.is_empty()
                || source_fields.len() > 2
                || source_fields.iter().any(|field| {
                    !matches!(field.as_str(), "full_name" | "first_name" | "last_name")
                })
                || source_sha256.len() != 64
                || !source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Stored signature is invalid".into());
            }
            ending.push(text);
        }
    }
    if !ending.is_empty() {
        pieces.push(ending.join("\n"));
    }
    let text = pieces
        .join("\n\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if text.len() > 16000 {
        return Err("Document exceeds the review text limit".into());
    }
    Ok(text)
}

pub fn attach(draft: &mut Value, kind: &str, signature: &Signature) -> Result<(), String> {
    if kind == "cover_letter" {
        draft["signature"] =
            serde_json::to_value(signature).map_err(|_| "Cannot encode signature source")?;
    }
    draft["text"] = json!(compose(draft, kind)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn letter() -> Value {
        json!({"paragraphs":[{"text":"I built measurement tools.","facts":[1]}],"framing":{"opening":"Dear hiring team,","closing":"Kind regards,"}})
    }

    #[test]
    fn profile_spelling_and_mononyms_are_preserved_without_inferred_identity() {
        for (profile, expected, fields) in [
            (
                json!({"full_name":" 李  明 ","first_name":"Other","last_name":"Name"}),
                "李  明",
                vec!["full_name"],
            ),
            (
                json!({"first_name":" Avery ","last_name":"O’Connor"}),
                "Avery O’Connor",
                vec!["first_name", "last_name"],
            ),
            (json!({"full_name":"Sukarno"}), "Sukarno", vec!["full_name"]),
            (json!({"first_name":"Avery"}), "Avery", vec!["first_name"]),
        ] {
            let value = serde_json::to_value(Signature::from_profile(&profile)).unwrap();
            assert_eq!(value["text"], expected);
            assert_eq!(value["source_fields"], json!(fields));
        }
        assert!(matches!(
            Signature::from_profile(
                &json!({"preferred_name":"Nickname","email":"fictional@example.invalid"})
            ),
            Signature::Missing
        ));
    }

    #[test]
    fn invalid_explicit_names_do_not_fall_back_or_inject_document_lines() {
        for raw in [
            json!("Avery\nInvented"),
            json!("Avery\tInvented"),
            json!("a".repeat(201)),
            json!(["Avery"]),
        ] {
            let signature =
                Signature::from_profile(&json!({"full_name":raw,"first_name":"Fallback"}));
            assert!(matches!(signature, Signature::Invalid { .. }));
            let mut draft = letter();
            attach(&mut draft, "cover_letter", &signature).unwrap();
            assert_eq!(
                draft["text"],
                "Dear hiring team,\n\nI built measurement tools.\n\nKind regards,"
            );
        }
    }

    #[test]
    fn canonical_letter_ends_with_source_bound_name_without_signing_the_cv() {
        let signature = Signature::from_profile(&json!({"full_name":"Avery Invented"}));
        let mut draft = letter();
        attach(&mut draft, "cover_letter", &signature).unwrap();
        assert_eq!(
            draft["text"],
            "Dear hiring team,\n\nI built measurement tools.\n\nKind regards,\nAvery Invented"
        );
        assert_eq!(compose(&draft, "cover_letter").unwrap(), draft["text"]);
        assert!(!compose(&draft, "cv").unwrap().contains("Avery"));
        let before = serde_json::to_value(signature).unwrap();
        let after = serde_json::to_value(Signature::from_profile(
            &json!({"full_name":"Avery Corrected"}),
        ))
        .unwrap();
        assert_ne!(before["source_sha256"], after["source_sha256"]);
        assert_eq!(draft["signature"], before);
    }

    #[test]
    fn legacy_drafts_remain_readable_but_corrupt_signatures_fail() {
        assert_eq!(
            compose(&letter(), "cover_letter").unwrap(),
            "Dear hiring team,\n\nI built measurement tools.\n\nKind regards,"
        );
        let mut draft = letter();
        draft["signature"] = json!({"status":"available","text":"Invented"});
        assert!(compose(&draft, "cover_letter").is_err());
    }
}
