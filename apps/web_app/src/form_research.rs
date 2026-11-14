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

fn search_providers(
    query: &str,
    mut fetch: impl FnMut(&url::Url) -> Result<String, String>,
) -> (Result<Value, String>, Vec<Value>) {
    let mut events = Vec::new();
    let mut empty = None;
    for provider in ["https://freeserp.ai/", "https://wiby.me/"] {
        let result = crate::web_search::request_url(provider, query).and_then(|url| {
            fetch(&url)
                .and_then(|body| crate::web_search::parse_provider(&body, &url, &HashSet::new()))
        });
        match result {
            Ok(parsed) => {
                let count = parsed["results"].as_array().map_or(0, Vec::len);
                events.push(json!({"provider":provider,"status":if count==0{"empty"}else{"available"},"results":count}));
                if count > 0 {
                    return (Ok(parsed), events);
                }
                empty = Some(parsed);
            }
            Err(error) => {
                events.push(json!({"provider":provider,"status":"unavailable","reason":error}))
            }
        }
    }
    if events.iter().any(|e| e["status"] == "unavailable") {
        (
            Err(
                "Public search is incomplete because one or more providers were unavailable".into(),
            ),
            events,
        )
    } else {
        (
            empty.ok_or("No public provider response was established".into()),
            events,
        )
    }
}

struct CachedDocument {
    request_key: String,
    receipt: waypoint_browser::capture_session::public_read::ReadReceipt,
    body: String,
    source_body: String,
}

struct Broker {
    directory: PathBuf,
    identifiers: Vec<String>,
    guard: Option<crate::visual_review::RequestGuard>,
    started: Instant,
    reads: usize,
    provider_events: Vec<Value>,
    cached: Option<CachedDocument>,
    views: u8,
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
        result.map_err(|reason| {
            let status = report["responses"]
                .as_array()
                .and_then(|rows| rows.last())
                .and_then(|row| row["status"].as_u64())
                .map_or_else(|| "unavailable".into(), |status| status.to_string());
            format!("Public read needs review: {reason:?}; last native HTTP status: {status}")
        })
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
        let (result, events) = search_providers(query, |url| {
            let document = self.fetch(url.as_str())?;
            String::from_utf8(document.body).map_err(|_| "Search response is not UTF-8".into())
        });
        let event = json!({"query":query,"attempts":events});
        record(
            &self.directory,
            &format!("search-{:02}.json", self.provider_events.len() + 1),
            &event,
        )?;
        self.provider_events.push(event);
        self.public_links(&result?)
    }

    fn read(&mut self, url: &str) -> Result<PublicEvidence, String> {
        self.read_focused(url, None)
    }
    fn supports_focused_read(&self) -> bool {
        true
    }
    fn read_focused(&mut self, url: &str, focus: Option<&str>) -> Result<PublicEvidence, String> {
        self.check()?;
        if !self.allowed(url) {
            return Err("Public research refused a private reference".into());
        }
        if self.views >= 8 {
            return Err("Public document view budget reached".into());
        }
        self.views += 1;
        let mut key = url::Url::parse(url).map_err(|_| "Invalid public document URL")?;
        key.set_fragment(None);
        let key = key.to_string();
        let reused = self
            .cached
            .as_ref()
            .is_some_and(|cached| cached.request_key == key || cached.receipt.url == key);
        if !reused {
            let document = self.fetch(url)?;
            let source_body = format!("read-{:02}/{}", self.reads, document.receipt.body_file);
            let body =
                String::from_utf8(document.body).map_err(|_| "Public document is not UTF-8")?;
            self.cached = Some(CachedDocument {
                request_key: key,
                receipt: document.receipt,
                body,
                source_body,
            });
        }
        let cached = self
            .cached
            .as_ref()
            .ok_or("Public source cache is unavailable")?;
        let result = if focus.is_some() {
            crate::public_document::extract_focused(
                &cached.body,
                cached.receipt.mime.as_deref(),
                url,
                &cached.receipt.url,
                focus,
            )
        } else {
            crate::public_document::extract(
                &cached.body,
                cached.receipt.mime.as_deref(),
                url,
                &cached.receipt.url,
            )
        };
        let result = result.map(|mut extracted| {
            let before = extracted.references.len();
            let registry = waypoint_workspace::default_registry();
            extracted.references.retain(|link| {
                self.allowed(&link.url)
                    && waypoint_connectors::is_public_host_url(&link.url)
                    && url::Url::parse(&link.url)
                        .ok()
                        .and_then(|u| u.host_str().map(waypoint_policy::source_for_host))
                        .is_some_and(|source| {
                            registry
                                .check_public_route(source, waypoint_policy::Operation::Read)
                                .is_ok()
                        })
            });
            extracted.references_omitted += before - extracted.references.len();
            extracted
        });
        record(
            &self.directory,
            &format!("source-view-{:02}.json", self.views),
            &json!({"body_sha256":cached.receipt.body_sha256,"source_body":cached.source_body,"native_response_reused":reused,"requested_url":url,"final_url":cached.receipt.url,"focus":focus,"extraction":result}),
        )?;
        self.check()?;
        let extracted = result?;
        let mut citation =
            url::Url::parse(&cached.receipt.url).map_err(|_| "Invalid source citation URL")?;
        if let Some(fragment) = &extracted.requested_fragment {
            citation.set_fragment(Some(fragment));
        }
        Ok(PublicEvidence {
            url: citation.into(),
            body_sha256: cached.receipt.body_sha256.clone(),
            text: extracted.text,
            view: Some(waypoint_research::tools::PublicView {
                requested_url: url.into(),
                focus: extracted.focus,
                scope: extracted.scope,
                text_offset_bytes: extracted.text_offset_bytes,
                scope_text_bytes: extracted.section_text_bytes,
                truncated: extracted.text_truncated,
            }),
            references: extracted.references,
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

pub(crate) fn technical_question(kind: &str, platform: &str, visual_refused: bool) -> String {
    let question = if visual_refused {
        "Which visual and native observations can establish that a control matches its accessible label and current state, and what uncertainty requires manual review?".to_owned()
    } else if kind == "file attachment" {
        "Which read-only browser checks distinguish native file selection from a page widget accepting an attachment, and what remains unknown without server confirmation?".to_owned()
    } else {
        format!("Which standard DOM events and validity properties can help investigate a {kind} whose expected value failed native readback? Separate observations from possible explanations.")
    };
    format!("Technical question: {question} Control category: {kind}. Public platform family: {platform}. Use authoritative public documentation. No candidate labels, values, images or personal URLs are supplied. Do not research applicant answers, authentication or challenge bypass. Findings are untested advice and cannot authorize or retry any application action.")
}

fn research_identifiers(profile: &Value) -> Result<Vec<String>, String> {
    let mut identifiers = crate::search_planner::protected_identifiers(profile);
    if let Some(history) = profile.get("career_history") {
        let raw = history
            .as_str()
            .ok_or("Private history guard is unavailable")?;
        if !raw.is_empty() {
            for record in crate::candidate_history::reviewed(raw)? {
                for value in record.values.values() {
                    let value = crate::search_planner::normalized_identifier(value);
                    if value.chars().count() >= 2 {
                        identifiers.push(value);
                    }
                }
            }
        }
    }
    if let Some(facts) = profile.get("reviewed_qualifications") {
        for fact in facts
            .as_array()
            .ok_or("Private qualification guard is unavailable")?
        {
            let value = crate::search_planner::normalized_identifier(
                fact.as_str()
                    .ok_or("Private qualification guard is unavailable")?,
            );
            if value.chars().count() >= 2 {
                identifiers.push(value);
            }
        }
    }
    identifiers.sort();
    identifiers.dedup();
    Ok(identifiers)
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
        identifiers: research_identifiers(profile)?,
        guard,
        started: Instant::now(),
        reads: 0,
        provider_events: vec![],
        cached: None,
        views: 0,
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
    let attempts: Vec<_> = broker
        .provider_events
        .iter()
        .flat_map(|e| e["attempts"].as_array().into_iter().flatten())
        .collect();
    let source_status = json!({"degraded":attempts.iter().any(|e|e["status"]=="unavailable"),"provider_attempts":attempts.len()});
    Ok(
        json!({"scope":"anonymous_public_technical_advice","result":result,"source_status":source_status,"form_actions_authorized":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_global_provider_is_used_before_fallback_and_all_failures_stay_visible() {
        let mut calls = vec![];
        let (result, events) = search_providers("technical query", |url| {
            calls.push(url.host_str().unwrap().to_owned());
            Ok(json!({"ok":true,"index":"web","results":[{"url":"https://example.org/docs","title":"Docs"}]}).to_string())
        });
        assert!(result.is_ok());
        assert_eq!(calls, ["freeserp.ai"]);
        assert_eq!(events[0]["status"], "available");
        let (result, events) = search_providers("technical query", |url| {
            if url.host_str() == Some("freeserp.ai") {
                Err("native503".into())
            } else {
                Ok(r#"[{"URL":"https://example.org/docs","Title":"Docs"}]"#.into())
            }
        });
        assert!(result.is_ok());
        assert_eq!(events[0]["reason"], "native503");
        assert_eq!(events[1]["status"], "available");
        let (result, events) = search_providers("technical query", |url| {
            if url.host_str() == Some("freeserp.ai") {
                Err("native503".into())
            } else {
                Ok("[]".into())
            }
        });
        assert!(result.is_err());
        assert_eq!(events[1]["status"], "empty");
        let (result, events) = search_providers("technical query", |url| {
            Ok(if url.host_str() == Some("freeserp.ai") {
                r#"{"ok":true,"index":"web","results":[]}"#
            } else {
                "[]"
            }
            .into())
        });
        assert!(result.unwrap()["results"].as_array().unwrap().is_empty());
        assert!(events.iter().all(|e| e["status"] == "empty"));
    }
    #[test]
    fn reviewed_qualification_values_stay_in_the_local_research_guard() {
        let protected = research_identifiers(
            &json!({"reviewed_qualifications":["Fictional Private Qualification"]}),
        )
        .unwrap();
        assert!(
            protected.contains(&crate::search_planner::normalized_identifier(
                "Fictional Private Qualification"
            ))
        );
        assert!(research_identifiers(&json!({"reviewed_qualifications":[42]})).is_err());
    }

    #[test]
    fn checked_history_values_are_private_guards_not_public_research_context() {
        let history=json!({"version":1,"employment":[{"id":"a".repeat(32),"employer":"Fictional Private Works","title":"Engineer","reviewed":true}],"education":[{"id":"b".repeat(32),"institution":"IBM","qualification":"Certificate","reviewed":true}]}).to_string();
        let root = tempfile::tempdir().unwrap();
        let mut broker = Broker {
            directory: root.path().into(),
            identifiers: research_identifiers(&json!({"career_history":history})).unwrap(),
            guard: None,
            started: Instant::now(),
            reads: 0,
            provider_events: vec![],
            cached: None,
            views: 0,
        };
        for url in [
            "https://fixture.invalid/?q=Fictional%20Private%20Works",
            "https://fixture.invalid/?q=IBM",
        ] {
            assert!(!broker.allowed(url));
            assert!(broker.fetch(url).is_err());
        }
        assert_eq!(broker.reads, 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        assert!(research_identifiers(&json!({"career_history":true})).is_err());
    }
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
            provider_events: vec![],
            cached: None,
            views: 0,
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
            provider_events: vec![],
            cached: None,
            views: 0,
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
        let text = crate::public_document::extract(
            html,
            Some("text/html"),
            "https://fixture.invalid/docs",
            "https://fixture.invalid/docs",
        )
        .unwrap()
        .text;
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
            provider_events: vec![],
            cached: None,
            views: 0,
        };
        let result = broker.read(&url);
        if let Ok(focus) = std::env::var("WAYPOINT_RESEARCH_LIVE_FOCUS") {
            let reads = broker.reads;
            let focused = broker.read_focused(&url, Some(&focus));
            record(&root,"focused-result.json",&json!({"result":focused,"native_reads_before":reads,"native_reads_after":broker.reads})).unwrap();
            assert!(
                focused.is_ok(),
                "Focused public read failed; native evidence retained"
            );
            assert_eq!(
                broker.reads, reads,
                "Focused view must reuse the already captured document"
            );
        }
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
