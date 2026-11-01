//! Bounded, sealed selection of unverified search links; no navigation occurs.
use serde_json::{json, Value};
use std::collections::HashSet;
use waypoint_workspace::Workspace;

/// Provider-reported search metadata is advisory and never listing evidence.
pub(crate) fn search_context(value: &Value) -> Result<Option<Value>, String> {
    if value.is_null() {
        return Ok(None);
    }
    if !value.is_object() {
        return Err("Invalid search context".into());
    }
    let text = |value: &Value| -> Result<String, String> {
        let value = value.as_str().ok_or("Invalid search context text")?;
        if value.len() > 320 || value.chars().count() > 80 || value.chars().any(char::is_control) {
            return Err("Invalid search context text".into());
        }
        Ok(value.to_owned())
    };
    let array = |value: &Value, limit: usize| -> Result<Vec<String>, String> {
        let items = value
            .as_array()
            .filter(|items| items.len() <= limit)
            .ok_or("Invalid search context list")?;
        items.iter().map(&text).collect()
    };
    let engines = array(&value["engines"], 8)?;
    let date = text(&value["published_date"])?;
    let dates = array(&value["alternative_published_dates"], 4)?;
    let engines_limited = value["engines_limited"]
        .as_bool()
        .ok_or("Invalid search context flag")?;
    let dates_limited = value["dates_limited"]
        .as_bool()
        .ok_or("Invalid search context flag")?;
    if engines.is_empty()
        && date.is_empty()
        && dates.is_empty()
        && !engines_limited
        && !dates_limited
    {
        return Ok(None);
    }
    Ok(Some(
        json!({"engines":engines,"published_date":date,"alternative_published_dates":dates,"engines_limited":engines_limited,"dates_limited":dates_limited}),
    ))
}

fn validate(value: &Value) -> Result<Vec<Value>, String> {
    let items = value
        .as_array()
        .filter(|items| items.len() <= 100)
        .ok_or("Select at most 100 listings")?;
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let raw = item["url"]
            .as_str()
            .filter(|url| url.len() <= 2048)
            .ok_or("Invalid selected listing URL")?;
        let url = url::Url::parse(raw).map_err(|_| "Invalid selected listing URL")?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || !waypoint_connectors::is_public_host_url(url.as_str())
            || !seen.insert(url.to_string())
        {
            return Err("Selected listings must have distinct public HTTPS URLs".into());
        }
        let title = item["title"]
            .as_str()
            .filter(|title| {
                title.len() <= 800
                    && title.chars().count() <= 200
                    && !title.chars().any(char::is_control)
            })
            .ok_or("Invalid selected listing title")?;
        let mut entry = json!({"url":url.as_str(),"title":title});
        if let Some(context) = search_context(&item["search_context"])? {
            entry["search_context"] = context;
        }
        result.push(entry);
    }
    Ok(result)
}

pub fn save(ws: &Workspace, expected: u64, value: &Value) -> Result<u64, String> {
    let items = validate(value)?;
    let bytes = serde_json::to_vec(&items).map_err(|_| "Could not serialize selected listings")?;
    ws.save_selected_web_candidates(expected, &bytes)
        .map_err(|error| error.to_string())
}

pub fn load(ws: &Workspace) -> Result<(u64, Vec<Value>), String> {
    match ws
        .selected_web_candidates()
        .map_err(|error| error.to_string())?
    {
        Some((revision, bytes)) => {
            let value =
                serde_json::from_slice(&bytes).map_err(|_| "Saved listing selection is invalid")?;
            Ok((revision, validate(&value)?))
        }
        None => Ok((0, Vec::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_are_bounded_and_exclude_private_or_duplicate_destinations() {
        assert_eq!(
            validate(&json!([{"url":"https://example.org/job","title":"Fictional role"}]))
                .unwrap()
                .len(),
            1
        );
        assert!(validate(&json!([{"url":"https://127.0.0.1/job","title":"X"}])).is_err());
        assert!(validate(&json!([{"url":"http://example.org/job","title":"X"}])).is_err());
        assert!(validate(&json!([{"url":"https://example.org/job","title":"X"},{"url":"https://example.org/job","title":"Y"}])).is_err());
        assert!(
            validate(&json!([{"url":"https://example.org/job","title":"bad\nlabel"}])).is_err()
        );
        assert!(validate(&json!(vec![
            json!({"url":"https://example.org/job","title":"X"});
            101
        ]))
        .is_err());
    }
    #[test]
    fn search_context_is_bounded_and_optional_for_existing_selections() {
        let context = json!({"engines":["one","two"],"published_date":"2026-10-01","alternative_published_dates":["2026-10-02"],"engines_limited":false,"dates_limited":false,"untrusted_extra":"discarded"});
        let item = json!({"url":"https://example.org/job","title":"Role","search_context":context});
        let saved = validate(&json!([item])).unwrap();
        assert_eq!(saved[0]["search_context"]["engines"], json!(["one", "two"]));
        assert!(saved[0]["search_context"].get("untrusted_extra").is_none());
        let mut bad = saved[0].clone();
        bad["search_context"]["engines"] = json!(vec!["one"; 9]);
        assert!(validate(&json!([bad])).is_err());
        let mut bad = saved[0].clone();
        bad["search_context"]["published_date"] = json!("bad\nvalue");
        assert!(validate(&json!([bad])).is_err());
        assert!(
            validate(&json!([{"url":"https://example.org/old","title":"Old"}])).unwrap()[0]
                .get("search_context")
                .is_none()
        );
    }
    #[cfg(windows)]
    #[test]
    fn selection_survives_restart_and_stale_windows_cannot_overwrite_it() {
        fn open(path: &std::path::Path) -> Workspace {
            #[cfg(feature = "sqlcipher")]
            {
                Workspace::open_encrypted(path).unwrap()
            }
            #[cfg(not(feature = "sqlcipher"))]
            {
                Workspace::open(path).unwrap()
            }
        }
        let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::Builder::new()
            .prefix("waypoint-selection-")
            .tempdir_in(parent)
            .unwrap();
        let items = json!([{"url":"https://example.org/job","title":"Fictional role","search_context":{"engines":["fixture"],"published_date":"2026-10-01","alternative_published_dates":[],"engines_limited":false,"dates_limited":false}}]);
        {
            let ws = open(dir.path());
            assert_eq!(load(&ws).unwrap(), (0, vec![]));
            assert_eq!(save(&ws, 0, &items).unwrap(), 1);
            assert!(save(&ws, 0, &json!([])).is_err());
            assert_eq!(load(&ws).unwrap().1.len(), 1);
        }
        let ws = open(dir.path());
        assert_eq!(load(&ws).unwrap(), (1, validate(&items).unwrap()));
        assert_eq!(save(&ws, 1, &json!([])).unwrap(), 2);
        assert!(load(&ws).unwrap().1.is_empty());
    }
}
