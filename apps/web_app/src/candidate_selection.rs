//! Bounded, sealed selection of unverified search links; no navigation occurs.
use serde_json::{json, Value};
use std::collections::HashSet;
use waypoint_workspace::Workspace;

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
        result.push(json!({"url":url.as_str(),"title":title}));
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
        let items = json!([{"url":"https://example.org/job","title":"Fictional role"}]);
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
