//! Anonymous research transport, separate from browser execution and replay.
//! Reuses capture permissions, robots, URL checks and aggregate transport budgets.
use super::*;

#[derive(Clone, Serialize)]
pub struct ReadReceipt {
    pub url: String,
    pub status: u16,
    pub mime: Option<String>,
    pub body_file: String,
    pub body_sha256: String,
    pub body_bytes: usize,
}

pub struct PublicDocument {
    pub receipt: ReadReceipt,
    pub body: Vec<u8>,
}

/// No cookie jar, candidate material, browser handle, POST or upload capability.
pub struct PublicReader {
    session: CaptureSession,
    receipts: Vec<ReadReceipt>,
}

impl PublicReader {
    pub fn start(
        directory: &Path,
        entry: &str,
        registry: PermissionRegistry,
        budget: CaptureBudget,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let url =
            resource_url(entry).map_err(|_| "Research URL is not a public anonymous reference")?;
        let source =
            waypoint_policy::source_for_host(url.host_str().ok_or("Research host is missing")?);
        registry
            .check_public_route(source, Operation::Read)
            .map_err(|e| e.to_string())?;
        let session = CaptureSession::start(
            directory,
            entry,
            vec![CaptureOrigin {
                origin: url.origin().ascii_serialization(),
                source: source.into(),
            }],
            registry,
            budget,
            cancelled,
        )?;
        Ok(Self {
            session,
            receipts: Vec::new(),
        })
    }

    /// Every hop, including redirects, passes the caller's additional PII guard.
    /// Admitted responses, including redirects/errors, retain native bytes.
    /// Incomplete/over-budget transport failures remain failures, never replies.
    pub fn read(
        &mut self,
        raw: &str,
        private_guard: &dyn Fn(&str) -> bool,
    ) -> Result<PublicDocument, CaptureGap> {
        let mut next = resource_url(raw)?;
        let mut seen = BTreeSet::new();
        for depth in 0..=5 {
            self.session.check_time()?;
            if !private_guard(next.as_str()) {
                return Err(CaptureGap::UrlPrivacyUncertain);
            }
            let source =
                waypoint_policy::source_for_host(next.host_str().ok_or(CaptureGap::InvalidUrl)?);
            self.session
                .registry
                .check_public_route(source, Operation::Read)
                .map_err(|_| CaptureGap::SourceNotAllowed)?;
            self.session
                .sources
                .insert(next.origin().ascii_serialization(), source.into());
            if !seen.insert(next.to_string()) {
                return Err(CaptureGap::RedirectLoop);
            }
            if self.receipts.len() >= self.session.budget.resources {
                return Err(CaptureGap::BudgetExceeded);
            }
            self.session.robots_allows(&next, false)?;
            let response = self
                .session
                .get(&next, self.session.budget.body_bytes, None, true)?;
            let name = format!("research-response-{:02}.bin", self.receipts.len() + 1);
            let hash = write_new(&self.session.root, &name, &response.body)
                .map_err(|_| CaptureGap::ArchiveRejected)?;
            let receipt = ReadReceipt {
                url: next.to_string(),
                status: response.status,
                mime: response_mime(&response),
                body_file: name,
                body_sha256: hash,
                body_bytes: response.body.len(),
            };
            // Header values remain original bytes, not lossy decoded text.
            let metadata = serde_json::json!({"receipt":receipt,"request_method":response.request_method,
                "headers":response.headers.iter().map(|h|serde_json::json!({"name":h.name,"value_bytes":h.value})).collect::<Vec<_>>(),
                "framing":format!("{:?}",response.framing),"trailers":response.trailers.iter().map(|h|serde_json::json!({"name":h.name,"value_bytes":h.value})).collect::<Vec<_>>()});
            write_new(
                &self.session.root,
                &format!("research-response-{:02}.json", self.receipts.len() + 1),
                &serde_json::to_vec_pretty(&metadata).map_err(|_| CaptureGap::ArchiveRejected)?,
            )
            .map_err(|_| CaptureGap::ArchiveRejected)?;
            self.receipts.push(receipt.clone());
            if response.framing == waypoint_net::BodyFraming::ConnectionClose
                || !response.trailers.is_empty()
            {
                return Err(CaptureGap::RepresentationUnsupported);
            }
            if is_redirect(response.status) {
                if depth == 5 {
                    return Err(CaptureGap::RedirectLimit);
                }
                next = redirect(&next, &response)?;
                continue;
            }
            if !(200..300).contains(&response.status) || response.body.is_empty() {
                return Err(CaptureGap::RepresentationUnsupported);
            }
            let supported = receipt.mime.as_deref().is_some_and(|m| {
                matches!(
                    m,
                    "text/html"
                        | "application/xhtml+xml"
                        | "text/plain"
                        | "text/markdown"
                        | "application/json"
                )
            });
            if !supported {
                return Err(CaptureGap::RepresentationUnsupported);
            }
            self.session.check_time()?;
            return Ok(PublicDocument {
                receipt,
                body: response.body,
            });
        }
        Err(CaptureGap::RedirectLimit)
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"scope":"anonymous_public_research_gets","responses":self.receipts,
            "requests":self.session.requests,"received_body_bytes":self.session.received,
            "robots":self.session.robots_records,"elapsed_ms":self.session.started.elapsed().as_millis(),
            "browser_or_candidate_operations":false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Mutex};
    struct Fake {
        replies: BTreeMap<String, CapturedResponse>,
        calls: Arc<Mutex<Vec<String>>>,
    }
    impl Transport for Fake {
        fn get(
            &mut self,
            url: &str,
            _: u64,
            _: usize,
            origin: Option<&waypoint_net::PublicRequestOrigin>,
            _: Option<&waypoint_net::PublicDocumentAccept>,
        ) -> Result<CapturedResponse, ()> {
            assert!(origin.is_none());
            self.calls.lock().unwrap().push(url.into());
            self.replies.remove(url).ok_or(())
        }
    }
    fn response(status: u16, body: &str, mime: &str) -> CapturedResponse {
        CapturedResponse {
            request_method: "GET".into(),
            request_header_names: None,
            request_accept: None,
            request_origin: None,
            status,
            body: body.as_bytes().to_vec(),
            headers: vec![waypoint_net::Header {
                name: "Content-Type".into(),
                value: mime.as_bytes().to_vec(),
            }],
            trailers: vec![],
            framing: waypoint_net::BodyFraming::ContentLength,
        }
    }
    fn setup(
        robots: &str,
        page: CapturedResponse,
    ) -> (PublicReader, tempfile::TempDir, Arc<Mutex<Vec<String>>>) {
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let replies = [
            (
                "https://fixture.invalid/robots.txt".into(),
                response(200, robots, "text/plain"),
            ),
            ("https://fixture.invalid/docs".into(), page),
        ]
        .into();
        let session = CaptureSession::with_transport(
            &root.path().join("read"),
            "https://fixture.invalid/docs",
            vec![CaptureOrigin {
                origin: "https://fixture.invalid".into(),
                source: "user_supplied_url".into(),
            }],
            PermissionRegistry::from_json(waypoint_policy::INITIAL_REGISTRY_JSON).unwrap(),
            CaptureBudget::default(),
            Arc::new(AtomicBool::new(false)),
            Box::new(Fake {
                replies,
                calls: calls.clone(),
            }),
        )
        .unwrap();
        (
            PublicReader {
                session,
                receipts: vec![],
            },
            root,
            calls,
        )
    }
    #[test]
    fn public_read_preserves_json_and_native_headers_without_browser_or_replay() {
        let (mut reader, root, calls) = setup(
            "User-agent: *\nAllow: /",
            response(200, "{\"public\":true}", "application/json"),
        );
        let result = reader
            .read("https://fixture.invalid/docs", &|_| true)
            .unwrap();
        assert_eq!(result.body, b"{\"public\":true}");
        assert_eq!(calls.lock().unwrap().len(), 2);
        assert_eq!(
            std::fs::read(root.path().join("read").join(&result.receipt.body_file)).unwrap(),
            result.body
        );
        assert_eq!(
            result.receipt.body_sha256,
            waypoint_domain::sha256_hex(&result.body)
        );
        assert_eq!(reader.report()["responses"].as_array().unwrap().len(), 1);
        assert!(!root.path().join("read/index.html").exists());
    }
    #[test]
    fn robots_denial_and_early_cancellation_never_fetch_the_target() {
        let (mut reader, _root, calls) = setup(
            "User-agent: *\nDisallow: /",
            response(200, "private", "text/plain"),
        );
        assert!(matches!(
            reader.read("https://fixture.invalid/docs", &|_| true),
            Err(CaptureGap::RobotsDenied)
        ));
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["https://fixture.invalid/robots.txt"]
        );
        let (mut reader, _root, calls) = setup(
            "User-agent: *\nAllow: /",
            response(200, "public", "text/plain"),
        );
        reader.session.cancelled.store(true, Ordering::Release);
        assert!(matches!(
            reader.read("https://fixture.invalid/docs", &|_| true),
            Err(CaptureGap::Cancelled)
        ));
        assert!(calls.lock().unwrap().is_empty());
    }
    #[test]
    fn redirect_pii_and_restricted_sources_are_refused_before_the_next_transport() {
        for (target, private) in [
            ("/private-marker", true),
            ("https://www.linkedin.com/technical", false),
        ] {
            let mut page = response(302, "redirect", "text/plain");
            page.headers.push(waypoint_net::Header {
                name: "Location".into(),
                value: target.as_bytes().to_vec(),
            });
            let (mut reader, _root, calls) = setup("User-agent: *\nAllow: /", page);
            let result = reader.read("https://fixture.invalid/docs", &|url| {
                !url.contains("private-marker")
            });
            if private {
                assert!(matches!(result, Err(CaptureGap::UrlPrivacyUncertain)));
            } else {
                assert!(matches!(result, Err(CaptureGap::SourceNotAllowed)));
            }
            assert_eq!(calls.lock().unwrap().len(), 2);
            assert_eq!(reader.receipts.len(), 1);
        }
    }
    #[test]
    fn failed_http_and_incomplete_native_bodies_do_not_become_successful_documents() {
        for (status, framing) in [
            (403, waypoint_net::BodyFraming::ContentLength),
            (200, waypoint_net::BodyFraming::ConnectionClose),
        ] {
            let mut page = response(status, "retained failure", "text/plain");
            page.framing = framing;
            let (mut reader, _root, _calls) = setup("User-agent: *\nAllow: /", page);
            assert!(matches!(
                reader.read("https://fixture.invalid/docs", &|_| true),
                Err(CaptureGap::RepresentationUnsupported)
            ));
            assert_eq!(reader.receipts.len(), 1);
        }
    }
}
