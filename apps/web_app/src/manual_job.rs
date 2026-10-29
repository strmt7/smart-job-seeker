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
        return Ok(json!({"id":id,"existing":true,"requisition_added":identifier_added}));
    }
    if let Some(id) = ws.resolve_alias(&url).map_err(|e| e.to_string())? {
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
        last_seen: Some(now),
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
    fn reinspection_adds_missing_identifier_without_replacing_existing_data() {
        let mut ws = Workspace::in_memory().unwrap();
        let first = json!({"url":"https://example.com/one","title":"Engineer","employer":"Example Labs","description":"Original description"});
        let added = add(&mut ws, &first, 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
        add(&mut ws, &json!({"url":"https://other.example/two","title":"Research Analyst","employer":"Example Labs","requisition_id":"REQ-123"}), 1).unwrap();
        let result = add(&mut ws, &json!({"url":"https://example.com/one","title":"Changed title","employer":"Example Labs","requisition_id":"REQ-123"}), 2).unwrap();
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
