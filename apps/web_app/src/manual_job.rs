//! Explicit user-provided job intake; no network request is made.
use serde_json::{json, Value};
use waypoint_domain::{sha256_hex, ApplicationState, JobIdentityId};
use waypoint_store::StoredJob;
use waypoint_workspace::Workspace;

fn input<'a>(body: &'a Value, key: &str, max: usize, required: bool) -> Result<&'a str, String> {
    let text = body.get(key).and_then(Value::as_str).unwrap_or("").trim();
    if (required && text.is_empty())
        || text.len() > max
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(format!("Invalid {key}"));
    }
    Ok(text)
}
fn host_matches(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|p| p.ends_with('.'))
}

fn same_identity_text(a: &str, b: &str) -> bool {
    let normalize = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    normalize(a) == normalize(b)
}

fn check_shared_url_identity(
    existing: &StoredJob,
    title: &str,
    employer: &str,
) -> Result<(), String> {
    if !same_identity_text(&existing.title, title)
        || !same_identity_text(&existing.employer, employer)
    {
        return Err("This URL is already associated with a different title or employer. Review the saved listing: one careers page may contain multiple roles, or the descriptions may refer to the same role.".into());
    }
    Ok(())
}
pub fn source(host: &str) -> &'static str {
    for (domain, source) in [
        ("linkedin.com", "linkedin"),
        ("jobicy.com", "jobicy"),
        ("myworkdayjobs.com", "workday"),
        ("icims.com", "icims"),
        ("smartrecruiters.com", "smartrecruiters"),
        ("successfactors.com", "successfactors"),
        ("successfactors.eu", "successfactors"),
        ("greenhouse.io", "greenhouse"),
        ("lever.co", "lever"),
        ("ashbyhq.com", "ashby"),
    ] {
        if host_matches(host, domain) {
            return source;
        }
    }
    "user_supplied_url"
}
pub fn add(ws: &mut Workspace, body: &Value, timestamp: i64) -> Result<Value, String> {
    add_with_observation(ws, body, timestamp, None)
}

/// Called only after the application's listing inspection has completed.
pub fn add_inspected(
    ws: &mut Workspace,
    body: &Value,
    timestamp: i64,
    observed_at: Option<i64>,
) -> Result<Value, String> {
    add_with_observation(ws, body, timestamp, observed_at)
}

fn add_with_observation(
    ws: &mut Workspace,
    body: &Value,
    timestamp: i64,
    observed_at: Option<i64>,
) -> Result<Value, String> {
    let raw = input(body, "url", 2048, true)?;
    let mut parsed = url::Url::parse(raw).map_err(|_| "Invalid job URL")?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Job URL must be public HTTPS without credentials".into());
    }
    let host = parsed.host_str().ok_or("Job URL needs a host")?.to_owned();
    if !waypoint_connectors::is_public_host_url(parsed.as_str()) {
        return Err("Job URL must use a public host".into());
    }
    // Preserve fragments: some career sites use them for client-side routing.
    parsed.set_username("").map_err(|_| "Invalid URL")?;
    let url = parsed.to_string();
    let title = input(body, "title", 200, true)?;
    let employer = input(body, "employer", 200, true)?;
    let location = input(body, "location", 200, false)?;
    let description = input(body, "description", 30000, false)?;
    let requisition_id = input(body, "requisition_id", 200, false)?;
    if requisition_id.chars().any(char::is_control) {
        return Err("Invalid requisition_id".into());
    }
    if let Some(id) = ws
        .job_id_by_canonical_url(&url)
        .map_err(|e| e.to_string())?
    {
        let key = JobIdentityId::new(&id).map_err(|e| e.to_string())?;
        let mut existing = ws
            .job(&key)
            .map_err(|e| e.to_string())?
            .ok_or("Saved job not found")?;
        check_shared_url_identity(&existing, title, employer)?;
        let mut identifier_added = false;
        if !requisition_id.is_empty()
            && existing
                .requisition_id
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            && existing
                .employer
                .trim()
                .eq_ignore_ascii_case(employer.trim())
        {
            existing.requisition_id = Some(requisition_id.to_owned());
            identifier_added = ws
                .add_requisition_and_refresh(&existing)
                .map_err(|e| e.to_string())?;
        }
        let mut observation_refreshed = false;
        if let Some(observed) =
            observed_at.filter(|observed| *observed > 0 && *observed <= timestamp)
        {
            let current = ws
                .job(&key)
                .map_err(|e| e.to_string())?
                .ok_or("Saved job not found")?;
            let saved_description = ws.job_description(&key).map_err(|e| e.to_string())?;
            let clean_structured = body["extraction"] == "structured"
                && match body.get("extraction_warnings") {
                    None | Some(Value::Null) => true,
                    Some(Value::Array(warnings)) => warnings.is_empty(),
                    _ => false,
                };
            // Truncated descriptions cannot establish the identity of the full listing.
            if clean_structured
                && !description.is_empty()
                && description.len() < 29_996
                && saved_description.as_deref() == Some(description)
                && current.canonical_url.as_deref() == Some(url.as_str())
                && current.title == title
                && current.employer == employer
                && current.location == location
                && body["remote"].as_bool() == Some(current.remote)
                && current.requisition_id.as_deref().unwrap_or("") == requisition_id
            {
                observation_refreshed = ws
                    .refresh_job_observation(&current, description, observed)
                    .map_err(|e| e.to_string())?;
            }
        }
        return Ok(
            json!({"id":id,"existing":true,"requisition_added":identifier_added,"observation_refreshed":observation_refreshed}),
        );
    }
    if let Some(id) = ws.resolve_alias(&url).map_err(|e| e.to_string())? {
        let key = JobIdentityId::new(&id).map_err(|e| e.to_string())?;
        let existing = ws
            .job(&key)
            .map_err(|e| e.to_string())?
            .ok_or("Saved job not found")?;
        check_shared_url_identity(&existing, title, employer)?;
        return Ok(json!({"id":id,"existing":true}));
    }
    let id = JobIdentityId::new(format!("manual-{}", sha256_hex(url.as_bytes())))
        .map_err(|e| e.to_string())?;
    let now = timestamp.to_string();
    let job = StoredJob {
        id: id.clone(),
        title: title.into(),
        employer: employer.into(),
        location: location.into(),
        state: ApplicationState::Discovered,
        canonical_url: Some(url),
        content_hash: None,
        first_seen: Some(now.clone()),
        last_seen: observed_at
            .filter(|observed| *observed > 0 && *observed <= timestamp)
            .map(|observed| observed.to_string()),
        remote: body["remote"].as_bool().unwrap_or(false),
        source: source(&host).into(),
        requisition_id: (!requisition_id.is_empty()).then(|| requisition_id.to_owned()),
        posted_at: None,
    };
    ws.import_manual_job(job, (!description.is_empty()).then_some(description))
        .map_err(|e| e.to_string())?;
    Ok(json!({"id":id.as_str(),"existing":false}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manually_entered_listings_do_not_claim_a_page_observation() {
        let mut ws = Workspace::in_memory().unwrap();
        let raw = json!({"url":"https://example.org/manual","title":"Engineer","employer":"Example","extraction":"structured"});
        let saved = add(&mut ws, &raw, 100).unwrap();
        let id = JobIdentityId::new(saved["id"].as_str().unwrap()).unwrap();
        let manual = ws.job(&id).unwrap().unwrap();
        assert_eq!(manual.first_seen.as_deref(), Some("100"));
        assert_eq!(manual.last_seen, None);
        let raw =
            json!({"url":"https://example.org/observed","title":"Engineer","employer":"Example"});
        let saved = add_inspected(&mut ws, &raw, 300, Some(200)).unwrap();
        let id = JobIdentityId::new(saved["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            ws.job(&id).unwrap().unwrap().last_seen.as_deref(),
            Some("200")
        );
        for (index, observed_at) in [None, Some(0), Some(301)].into_iter().enumerate() {
            let raw = json!({"url":format!("https://example.org/invalid-{index}"),"title":"Engineer","employer":"Example"});
            let saved = add_inspected(&mut ws, &raw, 300, observed_at).unwrap();
            let id = JobIdentityId::new(saved["id"].as_str().unwrap()).unwrap();
            assert_eq!(ws.job(&id).unwrap().unwrap().last_seen, None);
        }
    }

    #[test]
    fn reinspection_refreshes_only_matching_complete_saved_listings() {
        let mut ws = Workspace::in_memory().unwrap();
        let mut data = json!({"url":"https://example.org/job","title":"Engineer","employer":"Example","location":"Berlin","description":"Fictional duties.","remote":false});
        let result = add(&mut ws, &data, 100).unwrap();
        let id = JobIdentityId::new(result["id"].as_str().unwrap()).unwrap();
        data["extraction"] = json!("structured");
        assert_eq!(
            add_inspected(&mut ws, &data, 300, Some(200)).unwrap()["observation_refreshed"],
            true
        );
        assert_eq!(
            ws.job(&id).unwrap().unwrap().last_seen.as_deref(),
            Some("200")
        );
        assert_eq!(
            add_inspected(&mut ws, &data, 300, Some(150)).unwrap()["observation_refreshed"],
            false
        );
        data["description"] = json!("Changed duties.");
        assert_eq!(
            add_inspected(&mut ws, &data, 300, Some(250)).unwrap()["observation_refreshed"],
            false
        );
        data["description"] = json!("Fictional duties.");
        data["extraction_warnings"] = json!(["Incomplete listing"]);
        assert_eq!(
            add_inspected(&mut ws, &data, 300, Some(250)).unwrap()["observation_refreshed"],
            false
        );
        data["extraction_warnings"] = json!([]);
        data["extraction"] = json!("local_model");
        assert_eq!(
            add_inspected(&mut ws, &data, 300, Some(250)).unwrap()["observation_refreshed"],
            false
        );
        assert_eq!(
            ws.job(&id).unwrap().unwrap().last_seen.as_deref(),
            Some("200")
        );
        let snapshot = ws.job(&id).unwrap().unwrap();
        ws.replace_reviewed_description(
            &id,
            Some("Fictional duties."),
            "changed concurrently",
            "300",
        )
        .unwrap();
        assert!(!ws
            .refresh_job_observation(&snapshot, "Fictional duties.", 250)
            .unwrap());
    }

    #[test]
    fn refreshed_observation_survives_restart_and_rejects_stale_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let mut data = json!({"url":"https://example.org/job","title":"Engineer","employer":"Example","location":"Berlin","description":"Fictional duties.","remote":false});
        let saved = add(&mut ws, &data, 100).unwrap();
        let id = JobIdentityId::new(saved["id"].as_str().unwrap()).unwrap();
        let stale = ws.job(&id).unwrap().unwrap();
        data["extraction"] = json!("structured");
        assert_eq!(
            add_inspected(&mut ws, &data, 300, Some(200)).unwrap()["observation_refreshed"],
            true
        );
        assert!(!ws
            .refresh_job_observation(&stale, "Fictional duties.", 250)
            .unwrap());
        drop(ws);
        let mut reopened = Workspace::open(directory.path()).unwrap();
        assert_eq!(
            reopened.job(&id).unwrap().unwrap().last_seen.as_deref(),
            Some("200")
        );
        let old = reopened.job(&id).unwrap().unwrap();
        let mut changed = old.clone();
        changed.location = "Hamburg".into();
        reopened.import_job(changed).unwrap();
        assert!(!reopened
            .refresh_job_observation(&old, "Fictional duties.", 250)
            .unwrap());
        let current = reopened.job(&id).unwrap().unwrap();
        assert_eq!(current.location, "Hamburg");
        assert_eq!(current.last_seen.as_deref(), Some("200"));
    }

    #[test]
    fn similar_cross_site_listings_are_held_until_the_user_decides() {
        let mut ws = Workspace::in_memory().unwrap();
        let first = add(&mut ws, &json!({"url":"https://example.com/jobs/1","title":"Senior Software Engineer","employer":"Acme Inc","location":"Berlin","description":"A fictional software position"}), 1).unwrap();
        let second = add(&mut ws, &json!({"url":"https://other.example/jobs/2","title":"Sr Software Developer","employer":"Acme Ltd","location":"Berlin","description":"A fictional software position"}), 2).unwrap();
        let a = JobIdentityId::new(first["id"].as_str().unwrap()).unwrap();
        let b = JobIdentityId::new(second["id"].as_str().unwrap()).unwrap();
        assert_eq!(ws.possible_duplicates().unwrap().len(), 1);
        assert!(ws.has_duplicate_hold(&a).unwrap() && ws.has_duplicate_hold(&b).unwrap());
        assert!(ws.decide_duplicate(&a, &b, true).unwrap());
        assert!(!ws.has_duplicate_hold(&a).unwrap());
        assert!(ws.has_duplicate_hold(&b).unwrap());
        assert!(ws.decide_duplicate(&a, &b, false).unwrap());
        assert!(!ws.has_duplicate_hold(&b).unwrap());
    }
    #[test]
    fn similar_descriptions_can_flag_different_titles() {
        let mut ws = Workspace::in_memory().unwrap();
        let description = "Analyze manufacturing quality data statistical models production systems equipment safety reporting team research measurements validation standards optimization process engineering technical documentation operations performance continuous improvement";
        let first = add(&mut ws, &json!({"url":"https://example.com/jobs/3","title":"Quality Data Analyst","employer":"Acme Inc","description":description}), 1).unwrap();
        let second = add(&mut ws, &json!({"url":"https://other.example/jobs/4","title":"Quality Process Specialist","employer":"Acme Ltd","description":description}), 2).unwrap();
        let a = JobIdentityId::new(first["id"].as_str().unwrap()).unwrap();
        let b = JobIdentityId::new(second["id"].as_str().unwrap()).unwrap();
        assert!(ws.has_duplicate_hold(&a).unwrap());
        assert!(ws.has_duplicate_hold(&b).unwrap());
        assert!(ws.possible_duplicates().unwrap()[0]
            .reason
            .contains("description overlap 100%"));
    }
    #[test]
    fn manual_intake_flags_matching_requisition_and_description_across_employers() {
        let mut ws = Workspace::in_memory().unwrap();
        let description = "Analyze genomic sequencing data using Python and statistical methods collaborate with research teams prepare reproducible results manage experiments communicate findings across interdisciplinary scientific projects and maintain accurate documentation daily";
        let first = add(&mut ws, &json!({"url":"https://example.org/jobs/one","title":"Computational biologist","employer":"Alpha Institute","requisition_id":"REQ-481","description":description}), 1).unwrap();
        let second = add(&mut ws, &json!({"url":"https://example.net/jobs/two","title":"Genomics specialist","employer":"Beta Staffing","requisition_id":"req-481","description":description}), 2).unwrap();
        let a = JobIdentityId::new(first["id"].as_str().unwrap()).unwrap();
        let b = JobIdentityId::new(second["id"].as_str().unwrap()).unwrap();
        assert!(ws.has_duplicate_hold(&a).unwrap());
        assert!(ws.has_duplicate_hold(&b).unwrap());
        assert_eq!(ws.possible_duplicates().unwrap().len(), 1);
    }
    #[test]
    fn import_deduplicates_without_resetting_state_and_preserves_description() {
        let mut ws = Workspace::in_memory().unwrap();
        let body = json!({"url":"https://example.com/jobs/1","title":"Engineer","employer":"Example","description":"Rust and SQL"});
        let a = add(&mut ws, &body, 1).unwrap();
        let id = JobIdentityId::new(a["id"].as_str().unwrap()).unwrap();
        ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
        assert_eq!(ws.job_description(&id).unwrap().unwrap(), "Rust and SQL");
        assert_eq!(add(&mut ws, &body, 2).unwrap()["existing"], true);
        assert_eq!(
            ws.job(&id).unwrap().unwrap().state,
            ApplicationState::Shortlisted
        );
    }

    #[test]
    fn shared_page_does_not_silently_merge_distinct_roles() {
        let mut ws = Workspace::in_memory().unwrap();
        let first = json!({"url":"https://example.org/careers","title":"Research Engineer","employer":"Example Lab"});
        let added = add(&mut ws, &first, 1).unwrap();
        let other = json!({"url":"https://example.org/careers","title":"Data Scientist","employer":"Example Lab","requisition_id":"DATA-2"});
        assert!(add(&mut ws, &other, 2)
            .unwrap_err()
            .contains("multiple roles"));
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let saved = ws.job(&id).unwrap().unwrap();
        assert_eq!(saved.title, "Research Engineer");
        assert!(saved.requisition_id.is_none());
        let distinct = json!({"url":"https://example.org/careers#data-scientist","title":"Data Scientist","employer":"Example Lab"});
        let second = add(&mut ws, &distinct, 3).unwrap();
        assert_eq!(second["existing"], false);
        assert_ne!(second["id"], added["id"]);
    }

    #[test]
    fn reinspection_adds_missing_identifier_without_replacing_existing_data() {
        let mut ws = Workspace::in_memory().unwrap();
        let first = json!({"url":"https://example.com/one","title":"Engineer","employer":"Example Labs","description":"Original description"});
        let added = add(&mut ws, &first, 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
        add(&mut ws, &json!({"url":"https://other.example/two","title":"Research Analyst","employer":"Example Labs","requisition_id":"REQ-123"}), 1).unwrap();
        let error = add(&mut ws, &json!({"url":"https://example.com/one","title":"Changed title","employer":"Example Labs","requisition_id":"REQ-123"}), 2).unwrap_err();
        assert!(error.contains("multiple roles"));
        assert!(ws.job(&id).unwrap().unwrap().requisition_id.is_none());
        let result = add(&mut ws, &json!({"url":"https://example.com/one","title":" engineer ","employer":"Example Labs","requisition_id":"REQ-123"}), 2).unwrap();
        assert_eq!(result["requisition_added"], true);
        let job = ws.job(&id).unwrap().unwrap();
        assert_eq!(job.requisition_id.as_deref(), Some("REQ-123"));
        assert_eq!(job.title, "Engineer");
        assert_eq!(job.state, ApplicationState::Shortlisted);
        assert_eq!(
            ws.job_description(&id).unwrap().unwrap(),
            "Original description"
        );
        assert!(ws.has_duplicate_hold(&id).unwrap());
        let conflict = add(&mut ws, &json!({"url":"https://example.com/one","title":"Engineer","employer":"Example Labs","requisition_id":"REQ-999"}), 3).unwrap();
        assert_eq!(conflict["requisition_added"], false);
        assert_eq!(
            ws.job(&id).unwrap().unwrap().requisition_id.as_deref(),
            Some("REQ-123")
        );
    }
    #[test]
    fn recognizes_source_restrictions_and_rejects_private_urls() {
        assert_eq!(source("www.linkedin.com"), "linkedin");
        assert_eq!(source("badlinkedin.com"), "user_supplied_url");
        let mut ws = Workspace::in_memory().unwrap();
        for url in [
            "http://example.com/job",
            "https://127.0.0.1/job",
            "https://user:password@example.com/job",
        ] {
            assert!(add(
                &mut ws,
                &json!({"url":url,"title":"Role","employer":"Org"}),
                1
            )
            .is_err());
        }
    }
}
