//! ATS discovery connectors (T014, gate G2) + safe URL policy (T015 support).
//!
//! Each adapter parses the provider's *documented public* JSON/HTML postings
//! format into domain `RawPosting`s with provenance. Network I/O itself goes
//! through a broker that enforces the permission registry (T015); these
//! adapters are pure parsers plus the URL/plan each source needs.

use waypoint_policy::{Operation, PermissionRegistry};
use waypoint_search::identity_key;

/// A parsed posting, before dedup/constraint evaluation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RawPosting {
    pub identity: String,
    pub source: String,
    pub source_url: String,
    pub requisition_id: Option<String>,
    pub employer_namespace: String,
    pub title: String,
    pub employer: String,
    pub location: String,
    pub remote: bool,
    pub posted_at: Option<String>,
    pub content_hash_source: String, // canonical string used for hashing upstream
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectorError {
    /// The source is not permitted for this operation under the registry.
    NotPermitted(String),
    Parse(String),
}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectorError::NotPermitted(m) => write!(f, "connector not permitted: {m}"),
            ConnectorError::Parse(m) => write!(f, "parse error: {m}"),
        }
    }
}

impl std::error::Error for ConnectorError {}

/// Greenhouse public job-board listing shape (boards.greenhouse.io/<board> and
/// <board>.greenhouse.io/api) — a subset we actually consume.
#[derive(serde::Deserialize)]
struct GreenhouseJob {
    #[serde(default)]
    id: Option<serde_json::Value>,
    title: String,
    #[serde(default)]
    location: GreenhouseLocation,
    #[serde(default)]
    absolute_url: String,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct GreenhouseLocation {
    #[serde(default)]
    name: String,
}

/// Lever public postings shape (api.lever.co/v0/postings/<company>).
#[derive(serde::Deserialize)]
struct LeverPosting {
    id: String,
    text: String,
    #[serde(default)]
    categories: LeverCategories,
    #[serde(default, rename = "hostedUrl")]
    hosted_url: String,
    #[serde(default, rename = "createdAt")]
    created_at: Option<i64>,
}

#[derive(serde::Deserialize, Default)]
struct LeverCategories {
    #[serde(default)]
    location: String,
    #[serde(default, rename = "workplaceType")]
    workplace_type: String,
}

/// Ashby public postings shape (posting API returns items with id/title...).
#[derive(serde::Deserialize)]
struct AshbyPosting {
    id: String,
    title: String,
    #[serde(default)]
    location: Option<String>,
    #[serde(default, rename = "jobUrl")]
    job_url: Option<String>,
    #[serde(default, rename = "publishedAt")]
    published_at: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    employment_type_sniffed: Option<String>,
    #[serde(default = "default_false", rename = "isRemote")]
    is_remote: bool,
}

fn default_false() -> bool {
    false
}

/// Accept both a bare JSON array and {"<wrapper>": [...]} shapes, since live
/// endpoints differ and drift is a recorded risk (T034).
fn extract_job_list<'a>(
    payload: &'a str,
    wrappers: &[&str],
    source: &str,
) -> Result<Vec<GreenhouseJob>, ConnectorError> {
    let v: serde_json::Value = serde_json::from_str(payload)
        .map_err(|e| ConnectorError::Parse(format!("{source} list: {e}")))?;
    let arr = v
        .as_array()
        .cloned()
        .or_else(|| {
            wrappers
                .iter()
                .find_map(|w| v.get(*w).and_then(|x| x.as_array().cloned()))
        })
        .ok_or_else(|| ConnectorError::Parse(format!("{source}: no job array found")))?;
    serde_json::from_value(serde_json::Value::Array(arr))
        .map_err(|e| ConnectorError::Parse(format!("{source} items: {e}")))
}

/// Parse a Greenhouse board JSON payload into raw postings.
pub fn parse_greenhouse(board: &str, payload: &str) -> Result<Vec<RawPosting>, ConnectorError> {
    let jobs = extract_job_list(payload, &["jobs"], "greenhouse")?;
    Ok(jobs
        .into_iter()
        .map(|j| {
            let req = match &j.id {
                Some(serde_json::Value::Number(n)) => Some(n.to_string()),
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                _ => None,
            };
            RawPosting {
                identity: identity_key("greenhouse", req.as_deref(), &j.absolute_url),
                source: "greenhouse".into(),
                source_url: j.absolute_url.clone(),
                requisition_id: req.clone(),
                employer_namespace: board.to_string(),
                title: j.title,
                employer: board.to_string(),
                location: j.location.name,
                remote: false,
                posted_at: j.updated_at,
                content_hash_source: format!("gh://{board}/{}", req.unwrap_or_default()),
            }
        })
        .collect())
}

/// Parse a Lever postings payload.
pub fn parse_lever(company: &str, payload: &str) -> Result<Vec<RawPosting>, ConnectorError> {
    let jobs: Vec<LeverPosting> = serde_json::from_str(payload)
        .map_err(|e| ConnectorError::Parse(format!("lever list: {e}")))?;
    Ok(jobs
        .into_iter()
        .map(|j| RawPosting {
            identity: identity_key("lever", Some(&j.id), &j.hosted_url),
            source: "lever".into(),
            source_url: j.hosted_url.clone(),
            requisition_id: Some(j.id.clone()),
            employer_namespace: company.to_string(),
            title: j.text,
            employer: company.to_string(),
            location: j.categories.location,
            remote: j.categories.workplace_type.eq_ignore_ascii_case("remote"),
            posted_at: j.created_at.map(|ms| format!("unix:{ms}")),
            content_hash_source: format!("lv://{company}/{}", j.id),
        })
        .collect())
}

/// Parse an Ashby postings payload.
pub fn parse_ashby(org: &str, payload: &str) -> Result<Vec<RawPosting>, ConnectorError> {
    // Ashby returns {"results": [...]} on some endpoints and a bare list on
    // others; accept both, remember which for the record.
    let list: serde_json::Value = serde_json::from_str(payload)
        .map_err(|e| ConnectorError::Parse(format!("ashby list: {e}")))?;
    let items = list
        .get("jobs")
        .or_else(|| list.get("results"))
        .cloned()
        .unwrap_or(list);
    let jobs: Vec<AshbyPosting> = serde_json::from_value(items)
        .map_err(|e| ConnectorError::Parse(format!("ashby items: {e}")))?;
    Ok(jobs
        .into_iter()
        .map(|j| RawPosting {
            identity: identity_key("ashby", Some(&j.id), j.job_url.as_deref().unwrap_or("")),
            source: "ashby".into(),
            source_url: j.job_url.clone().unwrap_or_default(),
            requisition_id: Some(j.id.clone()),
            employer_namespace: org.to_string(),
            title: j.title,
            employer: org.to_string(),
            location: j.location.clone().unwrap_or_default(),
            remote: j.is_remote,
            posted_at: j.published_at,
            content_hash_source: format!("ab://{org}/{}", j.id),
        })
        .collect())
}

/// The discovery plan for a permitted source. Fetching is done by the network
/// broker; this only records what to request, after permission checks.
pub struct DiscoveryRequest {
    pub source: &'static str,
    pub url: String,
}

/// Build a discovery request for `board`, or an error if the registry forbids
/// discovery for that source. T015's broker re-checks at fetch time.
pub fn discovery_plan(
    registry: &PermissionRegistry,
    source: &str,
    board: &str,
) -> Result<DiscoveryRequest, ConnectorError> {
    if let Err(e) = registry.check_public_route(source, Operation::Discover) {
        return Err(ConnectorError::NotPermitted(e.to_string()));
    }
    let url = match source {
        "greenhouse" => format!("https://boards-api.greenhouse.io/v1/boards/{board}/jobs"),
        "lever" => format!("https://api.lever.co/v0/postings/{board}?mode=json"),
        "ashby" => format!("https://api.ashbyhq.com/posting-api/job-board/{board}"),
        _ => {
            return Err(ConnectorError::NotPermitted(format!(
                "no discovery URL template for source '{source}'"
            )))
        }
    };
    Ok(DiscoveryRequest {
        source: match source {
            "greenhouse" => "greenhouse",
            "lever" => "lever",
            _ => "ashby",
        },
        url,
    })
}

/// Private-address guard for the network broker (T015). Denies private,
/// loopback and link-local targets for untrusted fetches.
pub fn is_public_host_url(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else {
        return false;
    };
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    // IPv6 literals carry their own colons: only strip a trailing :port when
    // the remainder is not an IPv6 literal itself.
    let host = if host_port.starts_with('[') {
        host_port
            .split_once(']')
            .map(|(h, _)| &h[1..])
            .unwrap_or(host_port)
    } else {
        host_port
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(host_port)
    };
    let host = host.trim_matches(['[', ']']);
    let lower = host.to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return false;
    }
    // IPv4 literals
    if let Ok(v4) = lower.parse::<std::net::IpAddr>() {
        return !v4.is_loopback()
            && !matches!(
                v4,
                std::net::IpAddr::V4(v)
                    if v.is_private() || v.is_link_local() || v.is_unspecified()
            );
    }
    // common private v4 text forms
    if lower.starts_with("10.")
        || lower.starts_with("192.168.")
        || lower.starts_with("169.254.")
        || lower.starts_with("0.")
        || (lower.starts_with("172.")
            && lower
                .split('.')
                .nth(1)
                .and_then(|o| o.parse::<u8>().ok())
                .is_some_and(|o| (16..=31).contains(&o)))
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use waypoint_policy::INITIAL_REGISTRY_JSON;

    const GH_FIXTURE: &str = r#"[
      {"id": 482731, "title": "Senior Rust Engineer",
       "location": {"name": "Zurich"},
       "absolute_url": "https://boards.greenhouse.io/acme/jobs/482731",
       "updated_at": "2026-09-20T08:00:00Z"}
    ]"#;

    const LEVER_FIXTURE: &str = r#"[
      {"id": "abc-123", "text": "Platform Engineer",
       "categories": {"location": "Berlin", "workplaceType": "remote"},
       "hostedUrl": "https://jobs.lever.co/acme/abc-123",
       "createdAt": 1758800000000}
    ]"#;

    const ASHBY_FIXTURE: &str = r#"{"results": [
      {"id": "ash-9", "title": "Firmware Developer", "location": "Remote (EU)",
       "jobUrl": "https://jobs.ashbyhq.com/acme/ash-9", "publishedAt": "2026-09-18",
       "isRemote": true}
    ]}"#;

    fn registry() -> PermissionRegistry {
        PermissionRegistry::from_json(INITIAL_REGISTRY_JSON).unwrap()
    }

    #[test]
    fn greenhouse_fixture_parses_with_provenance() {
        let out = parse_greenhouse("acme", GH_FIXTURE).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].identity, "greenhouse::req::482731");
        assert_eq!(out[0].requisition_id.as_deref(), Some("482731"));
        assert_eq!(
            out[0].source_url,
            "https://boards.greenhouse.io/acme/jobs/482731"
        );
    }

    #[test]
    fn lever_fixture_parses_remote_flag() {
        let out = parse_lever("acme", LEVER_FIXTURE).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].remote);
        assert_eq!(out[0].identity, "lever::req::abc-123");
    }

    #[test]
    fn ashby_fixture_parses_wrapped_and_bare_shapes() {
        let wrapped = parse_ashby("acme", ASHBY_FIXTURE).unwrap();
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].identity, "ashby::req::ash-9");
        let bare = parse_ashby(
            "acme",
            r#"[{"id":"ash-1","title":"T","jobUrl":"https://jobs.ashbyhq.com/acme/ash-1"}]"#,
        )
        .unwrap();
        assert_eq!(bare.len(), 1);
    }

    #[test]
    fn bad_payload_is_parse_error_not_panic() {
        assert!(matches!(
            parse_greenhouse("acme", "{not json"),
            Err(ConnectorError::Parse(_))
        ));
    }

    #[test]
    fn discovery_plan_respects_permission_registry() {
        let r = registry();
        let plan = discovery_plan(&r, "greenhouse", "acme").unwrap();
        assert!(plan.url.starts_with("https://boards-api.greenhouse.io/"));
        // LinkedIn is forbidden in the registry -> refused before any fetch.
        assert!(matches!(
            discovery_plan(&r, "linkedin", "anything"),
            Err(ConnectorError::NotPermitted(_))
        ));
        // Unknown source -> refused.
        assert!(matches!(
            discovery_plan(&r, "mystery", "x"),
            Err(ConnectorError::NotPermitted(_))
        ));
    }

    #[test]
    fn public_host_guard_blocks_private_targets() {
        for blocked in [
            "http://localhost/x",
            "http://127.0.0.1/x",
            "http://10.0.0.5/x",
            "http://192.168.1.4/x",
            "http://172.16.3.3/x",
            "http://169.254.9.9/x",
            "http://[::1]/x",
            "https://foo.local/x",
        ] {
            assert!(!is_public_host_url(blocked), "should block {blocked}");
        }
        for allowed in [
            "https://boards-api.greenhouse.io/v1/boards/acme/jobs",
            "https://api.lever.co/v0/postings/acme",
            "https://api.ashbyhq.com/posting-api/job-board/acme",
            "http://8.8.8.8/x",
        ] {
            assert!(is_public_host_url(allowed), "should allow {allowed}");
        }
        assert!(!is_public_host_url("notaurl"));
    }
}
