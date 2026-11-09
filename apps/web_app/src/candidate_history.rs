//! Candidate-entered career records, sealed and revisioned with the profile.
//! Checking a record makes its values available for explicit answer selection;
//! it does not attest a claim, authorize a fill or establish model correctness.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 20;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Employment {
    id: String,
    employer: String,
    title: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    start_date: String,
    #[serde(default)]
    end_date: String,
    #[serde(default)]
    current: bool,
    #[serde(default)]
    description: String,
    #[serde(default)]
    reviewed: bool,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Education {
    id: String,
    institution: String,
    qualification: String,
    #[serde(default)]
    field_of_study: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    start_date: String,
    #[serde(default)]
    end_date: String,
    #[serde(default)]
    current: bool,
    #[serde(default)]
    description: String,
    #[serde(default)]
    reviewed: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct History {
    version: u32,
    employment: Vec<Employment>,
    education: Vec<Education>,
}

fn text(value: &str, limit: usize, required: bool, multiline: bool) -> Result<(), String> {
    if value.len() > limit
        || (required && value.trim().is_empty())
        || value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
    {
        return Err("History contains missing, oversized or invalid text".into());
    }
    Ok(())
}

// Partial dates retain their precision. Unknown months are never invented.
fn date_range(value: &str) -> Result<Option<(u32, u32)>, String> {
    if value.is_empty() {
        return Ok(None);
    }
    let bytes = value.as_bytes();
    if !matches!(bytes.len(), 4 | 7) || !bytes[..4].iter().all(u8::is_ascii_digit) {
        return Err("History dates must be YYYY or YYYY-MM, or left blank".into());
    }
    let year: u32 = value[..4].parse().map_err(|_| "Invalid history year")?;
    if !(1000..=9999).contains(&year) {
        return Err("History year must contain four digits from 1000 to 9999".into());
    }
    if bytes.len() == 4 {
        return Ok(Some((year * 12, year * 12 + 11)));
    }
    if bytes[4] != b'-' || !bytes[5..].iter().all(u8::is_ascii_digit) {
        return Err("History dates must be YYYY or YYYY-MM, or left blank".into());
    }
    let month: u32 = value[5..].parse().map_err(|_| "Invalid history month")?;
    if !(1..=12).contains(&month) {
        return Err("History month must be from 01 to 12".into());
    }
    Ok(Some((year * 12 + month - 1, year * 12 + month - 1)))
}

fn dates(start: &str, end: &str, current: bool) -> Result<(), String> {
    let start = date_range(start)?;
    let end = date_range(end)?;
    if current && end.is_some() {
        return Err("An ongoing history entry cannot also have an end date".into());
    }
    if matches!((start, end), (Some((first, _)), Some((_, last))) if first > last) {
        return Err("History end date precedes its start date".into());
    }
    Ok(())
}

fn identity(id: &str, seen: &mut HashSet<String>) -> Result<(), String> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || !seen.insert(id.into())
    {
        return Err("History entry identity is missing, invalid or duplicated".into());
    }
    Ok(())
}

pub fn canonicalize(raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return Ok(String::new());
    }
    if raw.len() > MAX_BYTES {
        return Err("Career history exceeds 64 KiB".into());
    }
    let history: History =
        serde_json::from_str(raw).map_err(|_| "Career history has an invalid structure")?;
    if history.version != 1
        || history.employment.len() > MAX_ENTRIES
        || history.education.len() > MAX_ENTRIES
    {
        return Err("Career history supports version 1 and at most 20 entries per section".into());
    }
    let mut seen = HashSet::new();
    for item in &history.employment {
        identity(&item.id, &mut seen)?;
        text(&item.employer, 500, true, false)?;
        text(&item.title, 500, true, false)?;
        text(&item.location, 500, false, false)?;
        text(&item.description, 4000, false, true)?;
        dates(&item.start_date, &item.end_date, item.current)?;
    }
    for item in &history.education {
        identity(&item.id, &mut seen)?;
        text(&item.institution, 500, true, false)?;
        text(&item.qualification, 500, true, false)?;
        text(&item.field_of_study, 500, false, false)?;
        text(&item.location, 500, false, false)?;
        text(&item.description, 4000, false, true)?;
        dates(&item.start_date, &item.end_date, item.current)?;
    }
    let result =
        serde_json::to_string(&history).map_err(|_| "Career history could not be encoded")?;
    if result.len() > MAX_BYTES {
        return Err("Career history exceeds 64 KiB after encoding".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> serde_json::Value {
        json!({"version":1,"employment":[{"id":"a".repeat(32),"employer":"Fictional & Co","title":"Engineer","start_date":"2020","end_date":"2020-01","reviewed":true}],"education":[]})
    }

    #[test]
    fn partial_dates_and_review_status_survive_canonical_roundtrip() {
        let saved = canonicalize(&fixture().to_string()).unwrap();
        assert_eq!(canonicalize(&saved).unwrap(), saved);
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(value["employment"][0]["start_date"], "2020");
        assert_eq!(value["employment"][0]["reviewed"], true);
        assert_eq!(value["employment"][0]["employer"], "Fictional & Co");
        assert_eq!(canonicalize("").unwrap(), "");
    }

    #[test]
    fn invalid_dates_identities_unknown_fields_and_oversized_records_are_refused() {
        for (key, value) in [
            ("end_date", json!("2019-12")),
            ("end_date", json!("2020-13")),
            ("start_date", json!("２０２０")),
            ("start_date", json!("2020-1")),
            ("employer", json!(" ")),
            ("description", json!("x".repeat(4001))),
            ("id", json!("A".repeat(32))),
            ("unexpected", json!(true)),
            ("current", json!(true)),
            ("reviewed", json!("true")),
        ] {
            let mut bad = fixture();
            bad["employment"][0][key] = value;
            assert!(canonicalize(&bad.to_string()).is_err(), "accepted {key}");
        }
        let mut duplicate = fixture();
        let first = duplicate["employment"][0].clone();
        duplicate["employment"].as_array_mut().unwrap().push(first);
        assert!(canonicalize(&duplicate.to_string()).is_err());
        let mut too_many = fixture();
        too_many["employment"] = json!((0..21)
            .map(|i| json!({"id":format!("{i:032x}"),"employer":"Fictional","title":"Role"}))
            .collect::<Vec<_>>());
        assert!(canonicalize(&too_many.to_string()).is_err());
        assert!(canonicalize(&" ".repeat(MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn education_ongoing_dates_and_cross_section_identities_are_checked() {
        let mut value = fixture();
        value["education"] = json!([{"id":"b".repeat(32),"institution":"Fictional University","qualification":"MSc","start_date":"2018-09","end_date":"","current":true}]);
        assert!(canonicalize(&value.to_string()).is_ok());
        value["education"][0]["end_date"] = json!("2020");
        assert!(canonicalize(&value.to_string()).is_err());
        value["education"][0]["current"] = json!(false);
        value["education"][0]["id"] = json!("a".repeat(32));
        assert!(canonicalize(&value.to_string()).is_err());
        value["education"][0]["id"] = json!("b".repeat(32));
        value["education"][0]["institution"] = json!("Fictional\0University");
        assert!(canonicalize(&value.to_string()).is_err());
    }
}
