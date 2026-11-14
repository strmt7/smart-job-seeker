//! Public technical research has no candidate-browser or approval capability.
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};
use waypoint_research::{
    tools::{PublicEvidence, PublicLink, PublicTools},
    ResearchBudget, ResearchPlan, Researcher,
};

fn record(directory: &Path, name: &str, value: &Value) -> Result<(), String> {
    use std::io::Write;
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| "Cannot encode public research evidence")?;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(name))
        .map_err(|_| "Cannot create public research evidence")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot save public research evidence".into())
}

struct Broker {
    directory: PathBuf,
    identifiers: Vec<String>,
    guard: Option<crate::visual_review::RequestGuard>,
    started: Instant,
    reads: usize,
}
impl Broker {
    fn allowed(&self, raw: &str) -> bool {
        waypoint_browser::capture_privacy::checked_reference(raw).is_ok_and(|decoded| {
            !crate::search_planner::contains_protected(&decoded, &self.identifiers)
        })
    }
    fn public_links(&self, parsed: &Value) -> Result<Vec<PublicLink>, String> {
        let links = parsed["results"]
            .as_array()
            .ok_or("Search result inventory is unavailable")?;
        Ok(links
            .iter()
            .filter_map(|link| {
                let url = link["url"].as_str()?;
                let title = link["title"].as_str()?;
                self.allowed(url).then(|| PublicLink {
                    url: url.into(),
                    title: title.into(),
                })
            })
            .collect())
    }
    fn fetch(
        &mut self,
        raw: &str,
    ) -> Result<waypoint_browser::capture_session::public_read::PublicDocument, String> {
        self.check()?;
        if !self.allowed(raw) {
            return Err("Public research refused a private reference".into());
        }
        self.reads += 1;
        let directory = self.directory.join(format!("read-{:02}", self.reads));
        let budget = waypoint_browser::capture_session::CaptureBudget {
            resources: 6,
            requests: 12,
            body_bytes: 1024 * 1024,
            received_bytes: 3 * 1024 * 1024,
            seconds: 20,
        };
        let mut reader = waypoint_browser::capture_session::public_read::PublicReader::start(
            &directory,
            raw,
            waypoint_workspace::default_registry(),
            budget,
            Arc::new(AtomicBool::new(false)),
        )?;
        let result = reader.read(raw, &|url| self.allowed(url));
        let report = reader.report();
        record(&directory, "research-transport.json", &report)?;
        self.check()?;
        result.map_err(|reason| format!("Public read needs review: {reason:?}"))
    }
}
impl PublicTools for Broker {
    fn search(&mut self, query: &str) -> Result<Vec<PublicLink>, String> {
        // Validate generated text locally; self-declared model privacy is not authority.
        let mut guard_url =
            url::Url::parse("https://research.invalid/").map_err(|_| "Invalid research guard")?;
        guard_url.query_pairs_mut().append_pair("q", query);
        if !self.allowed(guard_url.as_str()) {
            return Err("Research query contains private information".into());
        }
        let url = crate::web_search::request_url("https://wiby.me/", query)?;
        let document = self.fetch(url.as_str())?;
        let body = String::from_utf8(document.body).map_err(|_| "Search response is not UTF-8")?;
        let parsed = crate::web_search::parse_provider(&body, &url, &HashSet::new())?;
        self.public_links(&parsed)
    }

    fn read(&mut self, url: &str) -> Result<PublicEvidence, String> {
        let document = self.fetch(url)?;
        let body = String::from_utf8(document.body).map_err(|_| "Public document is not UTF-8")?;
        let text = source_text(&body, document.receipt.mime.as_deref());
        if text.trim().is_empty() {
            return Err("Public source has no usable extracted text".into());
        }
        let mut end = text.len().min(6000);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Ok(PublicEvidence {
            url: document.receipt.url,
            body_sha256: document.receipt.body_sha256,
            text: text[..end].into(),
        })
    }
    fn check(&self) -> Result<(), String> {
        if self.started.elapsed() > Duration::from_secs(60) {
            return Err("research_deadline".into());
        }
        if let Some(guard) = &self.guard {
            guard()?;
        }
        Ok(())
    }
}

fn source_text(body: &str, mime: Option<&str>) -> String {
    if !matches!(mime, Some("text/html" | "application/xhtml+xml")) {
        return body.to_owned();
    }
    let lower = body.to_ascii_lowercase();
    for tag in ["main", "article"] {
        if let Some(start) = lower.find(&format!("<{tag}")) {
            if let Some(end) = lower[start..].find(&format!("</{tag}>")) {
                return waypoint_connectors::description_text(&body[start..start + end]);
            }
        }
    }
    waypoint_connectors::description_text(body)
}

/// The caller supplies only a fixed issue class, control kind and public
/// platform family. Profile identifiers stay exclusively in the local guard.
pub(crate) fn run(
    inference: &mut dyn waypoint_inference::Inference,
    directory: &Path,
    profile: &Value,
    context: &str,
    guard: Option<crate::visual_review::RequestGuard>,
) -> Result<Value, String> {
    let directory = directory.join("public-research");
    std::fs::create_dir(&directory).map_err(|_| "Cannot create a new public research record")?;
    let mut broker = Broker {
        directory: directory.clone(),
        identifiers: crate::search_planner::protected_identifiers(profile),
        guard,
        started: Instant::now(),
        reads: 0,
    };
    broker.check()?;
    let mut researcher = Researcher::new(inference);
    researcher.begin(ResearchPlan {
        employer: String::new(),
        unresolved_questions: vec![context.into()],
        source_hierarchy: vec!["official_technical_documentation".into()],
        budget: ResearchBudget {
            max_queries: 2,
            max_fetches: 3,
            max_verification_passes: 2,
        },
    });
    let result = serde_json::to_value(researcher.investigate(context, &mut broker))
        .map_err(|_| "Cannot encode research advice")?;
    record(&directory, "research-result.json", &result)?;
    Ok(
        json!({"scope":"anonymous_public_technical_advice","result":result,"form_actions_authorized":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_query_references_are_refused_before_record_or_transport() {
        let root = tempfile::tempdir().unwrap();
        let mut broker = Broker {
            directory: root.path().into(),
            identifiers: crate::search_planner::protected_identifiers(
                &json!({"full_name":"Avery Example","email":"private@example.invalid"}),
            ),
            guard: None,
            started: Instant::now(),
            reads: 0,
        };
        for url in [
            "https://fixture.invalid/?q=Avery%20Example",
            "https://fixture.invalid/?q=private%40example.invalid",
            "https://fixture.invalid/?access_token=fictional-token",
        ] {
            assert!(!broker.allowed(url));
            assert!(broker.fetch(url).is_err());
        }
        assert_eq!(broker.reads, 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    fn research_consumes_the_real_provider_results_contract_and_keeps_privacy_filtering() {
        let root = tempfile::tempdir().unwrap();
        let broker = Broker {
            directory: root.path().into(),
            identifiers: crate::search_planner::protected_identifiers(
                &json!({"full_name":"Private Marker"}),
            ),
            guard: None,
            started: Instant::now(),
            reads: 0,
        };
        let url = crate::web_search::request_url("https://wiby.me/", "technical query").unwrap();
        let parsed=crate::web_search::parse_provider(r#"[{"URL":"https://example.org/docs","Title":"Public docs","Snippet":"Technical text"},{"URL":"https://example.org/private-marker","Title":"Private reference","Snippet":"Do not send"}]"#,&url,&HashSet::new()).unwrap();
        let links = broker.public_links(&parsed).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.org/docs");
        let empty = crate::web_search::parse_provider("[]", &url, &HashSet::new()).unwrap();
        assert!(broker.public_links(&empty).unwrap().is_empty());
        assert!(broker.public_links(&json!({"links":[]})).is_err());
    }
    #[test]
    fn research_extracts_actual_main_text_without_navigation_or_script_noise() {
        let html="<nav>Not the article</nav><main><h1>Public docs</h1><script>not text</script><p>Images use the images parameter.</p></main>";
        let text = source_text(html, Some("text/html"));
        assert!(text.contains("Images use the images parameter."));
        assert!(!text.contains("Not the article"));
        assert!(!text.contains("not text"));
    }

    #[test]
    #[ignore = "explicit public read-only URL and private evidence directory; no model or application interaction"]
    fn live_anonymous_technical_document_read() {
        let url = std::env::var("WAYPOINT_RESEARCH_LIVE_URL").expect("explicit public URL");
        let root = PathBuf::from(
            std::env::var_os("WAYPOINT_RESEARCH_EVIDENCE_DIR").expect("private evidence folder"),
        );
        let target = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert!(root.is_absolute());
        let parent = root.parent().unwrap().canonicalize().unwrap();
        assert!(parent.starts_with(&target));
        let root = parent.join(root.file_name().unwrap());
        std::fs::create_dir(&root).unwrap();
        let mut broker = Broker {
            directory: root.clone(),
            identifiers: vec![],
            guard: None,
            started: Instant::now(),
            reads: 0,
        };
        let result = broker.read(&url);
        let report = match &result {
            Ok(evidence) => json!({"status":"read","evidence":evidence}),
            Err(error) => json!({"status":"needs_review","reason":error}),
        };
        record(&root, "live-result.json", &report).unwrap();
        assert!(
            result.is_ok(),
            "Public read failed; native evidence retained"
        );
    }
}
