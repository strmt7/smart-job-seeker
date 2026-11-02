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
    #[serde(default)]
    pub remote_evidence: Option<bool>,
    pub posted_at: Option<String>,
    /// Text supplied by the board, if the listing includes it.
    pub description: Option<String>,
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
    first_published: Option<String>,
    #[serde(default)]
    content: Option<String>,
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
    #[serde(default, rename = "workplaceType")]
    workplace_type: Option<String>,
    #[serde(default, rename = "createdAt")]
    created_at: Option<i64>,
    #[serde(default, rename = "descriptionPlain")]
    description_plain: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    lists: Option<Vec<LeverList>>,
    #[serde(default, rename = "additionalPlain")]
    additional_plain: Option<String>,
    #[serde(default)]
    additional: Option<String>,
}

#[derive(serde::Deserialize)]
struct LeverList {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct LeverCategories {
    #[serde(default)]
    location: String,
    #[serde(default, rename = "allLocations")]
    all_locations: Option<Vec<String>>,
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
    #[serde(default, rename = "isRemote")]
    is_remote: Option<bool>,
    #[serde(default, rename = "descriptionPlain")]
    description_plain: Option<String>,
}

/// Bounded, best-effort text for matching, not a general HTML renderer.
pub fn description_text(input: &str) -> String {
    fn decode(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '&' {
                out.push(c);
                continue;
            }
            let mut entity = String::new();
            while entity.len() < 12
                && chars
                    .peek()
                    .is_some_and(|ch| *ch != ';' && !ch.is_whitespace())
            {
                entity.push(chars.next().unwrap());
            }
            if chars.peek() != Some(&';') {
                out.push('&');
                out.push_str(&entity);
                continue;
            }
            chars.next();
            let decoded = match entity.as_str() {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|v| u32::from_str_radix(v, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|v| v.parse::<u32>().ok()))
                    .and_then(char::from_u32),
            };
            if let Some(ch) = decoded {
                out.push(ch);
            } else {
                out.push('&');
                out.push_str(&entity);
                out.push(';');
            }
        }
        out
    }
    let source: String = input.chars().take(200_000).collect();
    let decoded = decode(&decode(&source));
    let mut result = String::new();
    let mut chars = decoded.chars();
    let mut hidden = false;
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            for next in chars.by_ref() {
                if next == '>' {
                    break;
                }
                tag.push(next);
            }
            let tag = tag.trim().to_ascii_lowercase();
            if tag.starts_with("script") || tag.starts_with("style") {
                hidden = true;
            }
            if tag.starts_with("/script") || tag.starts_with("/style") {
                hidden = false;
            }
            result.push(' ');
        } else if !hidden {
            result.push(ch);
        }
    }
    result
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(100_000)
        .collect()
}

/// Accept both a bare JSON array and {"<wrapper>": [...]} shapes, since live
/// endpoints differ and drift is a recorded risk (T034).
fn extract_job_list(
    payload: &str,
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
                remote_evidence: None,
                // An update is not evidence of the original publication date.
                posted_at: j.first_published,
                description: j.content.as_deref().map(description_text),
                content_hash_source: format!("gh://{board}/{}", req.unwrap_or_default()),
            }
        })
        .collect())
}

/// Preserve literal characters in a provider's explicitly plain-text field.
fn plain_description_text(input: &str) -> String {
    let bounded: String = input.chars().take(200_000).collect();
    bounded
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(100_000)
        .collect()
}

/// Preserve list-item boundaries for downstream requirement extraction.
fn lever_list_text(input: &str) -> String {
    let bounded: String = input.chars().take(200_000).collect();
    let lower = bounded.to_ascii_lowercase();
    let mut result = Vec::new();
    let mut start = 0;
    for _ in 0..2000 {
        let Some(relative) = lower[start..].find("</li>") else {
            break;
        };
        let end = start + relative + 5;
        let item = description_text(&bounded[start..end]);
        if !item.is_empty() {
            result.push(item);
        }
        start = end;
    }
    let remainder = description_text(&bounded[start..]);
    if !remainder.is_empty() {
        result.push(remainder);
    }
    result.join("\n").chars().take(100_000).collect()
}

/// Combine the board's introduction, titled requirement lists and additional
/// text. Prefer plaintext representations where provided; cap stored text.
fn lever_description(job: &LeverPosting) -> Option<String> {
    const LIMIT: usize = 100_000;
    let mut text = String::new();
    let mut used = 0;
    let mut append = |part: Option<&str>, list: bool, plain: bool| {
        if used >= LIMIT {
            return;
        }
        let Some(part) = part.filter(|part| !part.trim().is_empty()) else {
            return;
        };
        let clean = if list {
            lever_list_text(part)
        } else if plain {
            plain_description_text(part)
        } else {
            description_text(part)
        };
        if clean.is_empty() {
            return;
        }
        if !text.is_empty() && used < LIMIT {
            text.push('\n');
            used += 1;
        }
        let excerpt: String = clean.chars().take(LIMIT - used).collect();
        used += excerpt.chars().count();
        text.push_str(&excerpt);
    };
    let introduction = job
        .description_plain
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    append(
        introduction.or(job.description.as_deref()),
        false,
        introduction.is_some(),
    );
    for list in job.lists.as_deref().unwrap_or(&[]).iter().take(100) {
        append(list.text.as_deref(), false, true);
        append(list.content.as_deref(), true, false);
    }
    let additional = job
        .additional_plain
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    append(
        additional.or(job.additional.as_deref()),
        false,
        additional.is_some(),
    );
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn lever_locations(categories: &LeverCategories) -> String {
    let mut locations = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in
        std::iter::once(&categories.location).chain(categories.all_locations.iter().flatten())
    {
        // Preserve each source location as one line; whitespace in a source
        // value cannot manufacture an additional alternative.
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if !normalized.is_empty() && seen.insert(normalized.to_lowercase()) {
            locations.push(normalized);
        }
    }
    locations.join("\n")
}

/// Parse a Lever postings payload.
pub fn parse_lever(company: &str, payload: &str) -> Result<Vec<RawPosting>, ConnectorError> {
    let jobs: Vec<LeverPosting> = serde_json::from_str(payload)
        .map_err(|e| ConnectorError::Parse(format!("lever list: {e}")))?;
    Ok(jobs
        .into_iter()
        .map(|j| {
            let top = j
                .workplace_type
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let nested = Some(j.categories.workplace_type.trim()).filter(|value| !value.is_empty());
            let workplace = match (top, nested) {
                (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b) => None,
                (Some(value), _) | (None, Some(value)) => Some(value),
                _ => None,
            };
            let evidence = workplace.and_then(|value| match value.to_ascii_lowercase().as_str() {
                "remote" => Some(true),
                "on-site" | "onsite" => Some(false),
                _ => None,
            });
            let description = lever_description(&j);
            let location = lever_locations(&j.categories);
            RawPosting {
                identity: identity_key("lever", Some(&j.id), &j.hosted_url),
                source: "lever".into(),
                source_url: j.hosted_url.clone(),
                requisition_id: Some(j.id.clone()),
                employer_namespace: company.to_string(),
                title: j.text,
                employer: company.to_string(),
                location,
                remote: evidence == Some(true),
                remote_evidence: evidence,
                posted_at: j.created_at.map(|ms| format!("unix:{ms}")),
                description,
                content_hash_source: format!("lv://{company}/{}", j.id),
            }
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
            remote: j.is_remote.unwrap_or(false),
            remote_evidence: j.is_remote,
            posted_at: j.published_at,
            description: j.description_plain.as_deref().map(plain_description_text),
            content_hash_source: format!("ab://{org}/{}", j.id),
        })
        .collect())
}

/// Parse Jobicy's public feed. URLs are Jobicy listings, not direct employer forms.
pub fn parse_jobicy(payload: &str) -> Result<Vec<RawPosting>, ConnectorError> {
    let data: serde_json::Value = serde_json::from_str(payload)
        .map_err(|e| ConnectorError::Parse(format!("jobicy list: {e}")))?;
    if data.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return Err(ConnectorError::Parse(
            "jobicy returned an unsuccessful response".into(),
        ));
    }
    let jobs = data
        .get("jobs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConnectorError::Parse("jobicy: no job array found".into()))?;
    let mut out = Vec::new();
    for job in jobs {
        let id = job
            .get("id")
            .and_then(|v| v.as_u64())
            .filter(|id| *id > 0)
            .ok_or_else(|| ConnectorError::Parse("jobicy: invalid id".into()))?
            .to_string();
        let title = job
            .get("jobTitle")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ConnectorError::Parse("jobicy: missing title".into()))?;
        let employer = job
            .get("companyName")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ConnectorError::Parse("jobicy: missing company".into()))?;
        let url = job
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::Parse("jobicy: missing URL".into()))?;
        let parsed = url::Url::parse(url)
            .map_err(|_| ConnectorError::Parse("jobicy: invalid URL".into()))?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("jobicy.com")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(ConnectorError::Parse(
                "jobicy: URL is not a canonical Jobicy listing".into(),
            ));
        }
        let description = job
            .get("jobDescription")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| job.get("jobExcerpt").and_then(|v| v.as_str()))
            .map(description_text);
        out.push(RawPosting {
            identity: identity_key("jobicy", Some(&id), url),
            source: "jobicy".into(),
            source_url: url.into(),
            requisition_id: Some(id.clone()),
            employer_namespace: employer.into(),
            title: title.into(),
            employer: employer.into(),
            location: job
                .get("jobGeo")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            remote: true,
            remote_evidence: Some(true),
            posted_at: job
                .get("pubDate")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            description,
            content_hash_source: format!("jobicy://{id}"),
        });
    }
    Ok(out)
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
        "greenhouse" => {
            format!("https://boards-api.greenhouse.io/v1/boards/{board}/jobs?content=true")
        }
        "lever" => format!("https://api.lever.co/v0/postings/{board}?mode=json"),
        "ashby" => format!("https://api.ashbyhq.com/posting-api/job-board/{board}"),
        "jobicy" => {
            if board.trim().chars().count() < 3
                || board.chars().count() > 50
                || board.chars().any(char::is_control)
            {
                return Err(ConnectorError::NotPermitted(
                    "invalid Jobicy keyword".into(),
                ));
            }
            let mut url = url::Url::parse("https://jobicy.com/api/jobs").expect("static URL");
            url.query_pairs_mut()
                .append_pair("count", "50")
                .append_pair("tag", board.trim());
            url.to_string()
        }
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
            "jobicy" => "jobicy",
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
        assert_eq!(out[0].posted_at, None);
        assert_eq!(
            out[0].source_url,
            "https://boards.greenhouse.io/acme/jobs/482731"
        );
    }

    #[test]
    fn greenhouse_distinguishes_publication_from_update() {
        let payload = r#"{"jobs":[{"id":1,"title":"Engineer","first_published":"2025-01-01T00:00:00Z","updated_at":"2026-11-01T00:00:00Z"}]}"#;
        let out = parse_greenhouse("acme", payload).unwrap();
        assert_eq!(out[0].posted_at.as_deref(), Some("2025-01-01T00:00:00Z"));
    }

    #[test]
    fn lever_fixture_parses_remote_flag() {
        let out = parse_lever("acme", LEVER_FIXTURE).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].remote);
        assert_eq!(out[0].identity, "lever::req::abc-123");
    }

    #[test]
    fn lever_preserves_all_location_alternatives() {
        let payload = r#"[{"id":"a","text":"Engineer","categories":{"location":"New York, NY","allLocations":["New York, NY","San Francisco, CA","  san francisco, CA "]}}]"#;
        let rows = parse_lever("example", payload).unwrap();
        assert_eq!(rows[0].location, "New York, NY\nSan Francisco, CA");
    }

    #[test]
    fn provider_plain_text_keeps_angle_brackets_and_ampersands() {
        let lever = r#"[{"id":"a","text":"Engineer","descriptionPlain":"Use Vec<T> and R&D.\nLatency < 5 ms","additionalPlain":"Review A & B"}]"#;
        let rows = parse_lever("example", lever).unwrap();
        assert_eq!(
            rows[0].description.as_deref(),
            Some("Use Vec<T> and R&D.\nLatency < 5 ms\nReview A & B")
        );
        let ashby =
            r#"{"jobs":[{"id":"a","title":"Engineer","descriptionPlain":"Use Vec<T> and R&D."}]}"#;
        assert_eq!(
            parse_ashby("example", ashby).unwrap()[0]
                .description
                .as_deref(),
            Some("Use Vec<T> and R&D.")
        );
    }

    #[test]
    fn lever_description_includes_requirements_and_additional_sections() {
        let data = r#"[{"id":"a","text":"Engineer","hostedUrl":"https://jobs.lever.co/example/a","descriptionPlain":"Build tools.","description":"<p>duplicate intro</p>","lists":[{"text":"Requirements","content":"<ul><li>Rust experience</li><li>SQL knowledge</li></ul>"}],"additionalPlain":"Apply in English.","additional":"<p>duplicate footer</p>"},{"id":"b","text":"Engineer","description":"<p>HTML fallback</p>","lists":null}]"#;
        let rows = parse_lever("example", data).unwrap();
        assert_eq!(
            rows[0].description.as_deref(),
            Some("Build tools.\nRequirements\nRust experience\nSQL knowledge\nApply in English.")
        );
        assert_eq!(rows[1].description.as_deref(), Some("HTML fallback"));
    }

    #[test]
    fn lever_top_level_workplace_type_is_preserved_without_guessing() {
        let data = r#"[
          {"id":"a","text":"Engineer","hostedUrl":"https://jobs.lever.co/example/a","workplaceType":"remote"},
          {"id":"b","text":"Engineer","hostedUrl":"https://jobs.lever.co/example/b","workplaceType":"on-site"},
          {"id":"c","text":"Engineer","hostedUrl":"https://jobs.lever.co/example/c","workplaceType":"hybrid"},
          {"id":"d","text":"Engineer","hostedUrl":"https://jobs.lever.co/example/d"},
          {"id":"e","text":"Engineer","hostedUrl":"https://jobs.lever.co/example/e","workplaceType":"remote","categories":{"workplaceType":"on-site"}}
        ]"#;
        let rows = parse_lever("example", data).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.remote_evidence)
                .collect::<Vec<_>>(),
            vec![Some(true), Some(false), None, None, None]
        );
        assert_eq!(
            rows.iter().map(|row| row.remote).collect::<Vec<_>>(),
            vec![true, false, false, false, false]
        );
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
    fn jobicy_feed_preserves_source_and_rejects_untrusted_urls() {
        let payload = r#"{"success":true,"hasMore":false,"jobs":[{"id":123,"url":"https://jobicy.com/jobs/123-role","jobTitle":"Materials Engineer","companyName":"Example","jobGeo":"Europe","jobDescription":"<p>Rust &amp; materials</p>","pubDate":"2026-10-28T00:00:00Z"}]}"#;
        let jobs = parse_jobicy(payload).unwrap();
        assert_eq!(jobs[0].source, "jobicy");
        assert_eq!(jobs[0].source_url, "https://jobicy.com/jobs/123-role");
        assert_eq!(jobs[0].description.as_deref(), Some("Rust & materials"));
        assert!(parse_jobicy(&payload.replace(
            "https://jobicy.com/jobs/123-role",
            "https://example.com/job"
        ))
        .is_err());
        assert!(parse_jobicy(r#"{"success":false,"jobs":[]}"#).is_err());
        let plan = discovery_plan(&registry(), "jobicy", "materials engineer").unwrap();
        assert!(plan.url.contains("tag=materials+engineer"));
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
            "https://boards-api.greenhouse.io/v1/boards/acme/jobs?content=true",
            "https://api.lever.co/v0/postings/acme",
            "https://api.ashbyhq.com/posting-api/job-board/acme",
            "http://8.8.8.8/x",
        ] {
            assert!(is_public_host_url(allowed), "should allow {allowed}");
        }
        assert!(!is_public_host_url("notaurl"));
    }
}
