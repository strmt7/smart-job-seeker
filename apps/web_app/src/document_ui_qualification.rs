//! Fictional document journey through production handlers and the actual client.
use super::*;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn allowed_document_mutation(path: &str) -> bool {
    matches!(
        path,
        "/api/profile"
            | "/api/jobs/add"
            | "/api/shortlist"
            | "/api/documents/upload"
            | "/api/documents/extract-start"
            | "/api/documents/extract-status"
            | "/api/documents/template-plan-start"
            | "/api/documents/template-plan-status"
            | "/api/documents/template-suggest-start"
            | "/api/documents/template-suggest-status"
            | "/api/documents/edit-template-start"
            | "/api/documents/edit-template-status"
            | "/api/documents/download"
            | "/api/draft/start"
            | "/api/draft/status"
            | "/api/draft/list"
            | "/api/draft/read"
            | "/api/draft/pending"
            | "/api/letter/export-start"
            | "/api/letter/export-status"
            | "/api/letter/export-pending"
            | "/api/documents/preview-start"
            | "/api/documents/preview-status"
            | "/api/documents/visual-review-start"
            | "/api/documents/visual-review-status"
            | "/api/documents/visual-review-read"
            | "/api/documents/visual-review-active"
            | "/api/documents/ats-check-start"
            | "/api/documents/ats-check-status"
            | "/api/documents/ats-check-active"
            | "/api/documents/job-check-review"
            | "/api/attempts/list"
            | "/api/form/session-status"
            | "/api/form/recapture-pending"
            | "/api/search-plan/pending"
            | "/api/discover/pending"
            | "/api/application-check/pending"
            | "/api/gmail/status"
            | "/api/storage/status"
            | "/api/tools-setup/pending"
            | "/api/pdf-setup/pending"
            | "/api/documents/convert-pdf-pending"
            | "/api/web-search/selection"
            | "/api/web-search/pending"
            | "/api/web-search/batch-pending"
            | "/api/listing-inspect/pending"
            | "/api/documents/edit-template-pending"
            | "/api/fit/batch"
            | "/api/job-source/pending"
    )
}

#[test]
fn document_qualification_refuses_all_application_and_email_actions() {
    for path in [
        "/api/form/fill",
        "/api/form/approve",
        "/api/form/commit",
        "/api/email-draft",
        "/api/gmail/prepare-application",
        "/api/gmail/send",
        "/api/model-start/start",
    ] {
        assert!(!allowed_document_mutation(path), "{path}");
    }
    assert!(allowed_document_mutation("/api/letter/export-start"));
    assert!(allowed_document_mutation(
        "/api/documents/visual-review-start"
    ));
}

#[test]
#[ignore = "contained fictional HTTP/UI drafting, PDF conversion and local vision; explicit private runtimes"]
fn resource_gated_rendered_document_pdf_journey() {
    assert_eq!(
        std::env::var("WAYPOINT_DOCUMENT_UI_CONTAINED").as_deref(),
        Ok("1"),
        "Use tools/qualify_document_journey.py"
    );
    let selected = |key| {
        let path = PathBuf::from(std::env::var_os(key).expect("explicit qualification path"));
        assert!(path.is_absolute() && path.is_file());
        path
    };
    let node = selected("WAYPOINT_DOCUMENT_UI_NODE");
    let browser = selected("WAYPOINT_DOCUMENT_UI_BROWSER");
    let template = selected("WAYPOINT_DOCUMENT_UI_TEMPLATE");
    let model = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL").expect("explicit model");
    let root = tempfile::Builder::new()
        .prefix("document-journey-native-")
        .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .unwrap()
        .keep();
    let flow = std::env::var("WAYPOINT_DOCUMENT_UI_FLOW").unwrap_or_else(|_| "documents".into());
    let template_flow = matches!(flow.as_str(), "template" | "template-letter");
    let template = if template_flow {
        let path = root.join("fictional-styled-template.docx");
        std::fs::write(
            &path,
            if flow == "template-letter" {
                crate::template_plan::fictional_letter_docx()
            } else {
                crate::template_plan::fictional_docx()
            },
        )
        .unwrap();
        path
    } else {
        template
    };
    println!("Document journey evidence: {}", root.display());
    crate::experience_inference::tests::with_resource_gated_named_model(&model, || {
        let identity = resident_model_identity().unwrap().unwrap();
        assert_eq!(identity.name, model);
        use waypoint_inference::OllamaInference;
        let inference = OllamaInference::new("http://127.0.0.1:11434", &model)
            .unwrap()
            .with_resident_context();
        let mut repair_model = resident_inference::ResidentInference::new(
            inference,
            model.clone(),
            resident_model_identity,
        );
        if !template_flow {
            let facts = vec!["Built measurement tools in Rust.".into()];
            let invalid = json!({"paragraphs":[{"text":"I managed 200 people while building measurement tools in Rust.","facts":[99]}]});
            let (repaired, audit) =
                draft_task::validated_document_output(&mut repair_model, invalid.clone(), &facts)
                    .unwrap();
            let check = draft_task::check_document_evidence(&mut repair_model, &repaired, &facts);
            std::fs::write(root.join("repair.private.json"),json!({"original":invalid,"facts":facts,"repaired":repaired,"audit":audit,"evidence_check":check}).to_string()).unwrap();
            assert_eq!(audit["performed"], true);
            assert_eq!(check["status"], "completed");
            assert!(
                check["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|v| v["status"] == "supported"),
                "Actual model did not ground repaired claims"
            );
            assert!(
                !repaired.to_string().contains("200"),
                "Unsupported staffing claim survived repair"
            );
        }
        if template_flow {
            let review = crate::document_visual::qualify_content_review(&mut repair_model);
            std::fs::write(root.join("content-probe.private.json"), review.to_string()).unwrap();
            assert_eq!(
                review["issues"].as_array().unwrap().len(),
                1,
                "A real contradiction must remain; differing employer names alone are not a defect"
            );
            assert_eq!(
                review["issues"][0]["description"],
                review["original"]["issues"][1]["description"]
            );
            assert_eq!(
                review["suggestions"],
                json!(["Add a portfolio link if the candidate has one."])
            );
            assert_eq!(review["audit"]["unsupported_changes"], 1);
        }
        let mut ws = Workspace::open(&root.join("workspace")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = "5".repeat(64);
        let config = root.join("client.private.json");
        std::fs::write(&config, json!({"url":format!("http://127.0.0.1:{port}/#{token}"),
            "browser":browser,"template":template,"output":root,
            "model":model,"model_identity":{"name":identity.name,"digest":identity.digest},"template_kind":if flow=="template-letter"{"cover_letter"}else{"cv"},"app_sha256":waypoint_domain::sha256_hex(include_bytes!("../web/app.js"))}).to_string()).unwrap();
        use std::os::windows::process::CommandExt;
        let log = std::fs::File::create(root.join("client.private.log")).unwrap();
        let mut child = Command::new(node)
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR")).join(if template_flow {
                    "scripts/qualify-template-journey.cjs"
                } else {
                    "scripts/qualify-document-journey.cjs"
                }),
            )
            .arg(&config)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        struct OwnedChild<'a>(&'a mut std::process::Child);
        impl Drop for OwnedChild<'_> {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let child = OwnedChild(&mut child);
        let deadline = Instant::now() + Duration::from_secs(360);
        let mut session = None;
        let mut denied = Vec::new();
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "Document journey exceeded its deadline"
            );
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut peek = [0; 2048];
                    let line_deadline = Instant::now() + Duration::from_secs(5);
                    let bytes = loop {
                        let n = stream.peek(&mut peek).unwrap_or(0);
                        if n == 0
                            || peek[..n].windows(2).any(|v| v == b"\r\n")
                            || Instant::now() >= line_deadline
                        {
                            break n;
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    };
                    if bytes == 0 {
                        continue;
                    }
                    let first = String::from_utf8_lossy(&peek[..bytes]);
                    let mut request = first.split_whitespace();
                    let method = request.next().unwrap_or("");
                    let path = request.next().unwrap_or("");
                    if method != "GET" && !allowed_document_mutation(path) {
                        denied.push(path.to_owned());
                        json_reply(
                            &mut stream,
                            "403 Forbidden",
                            json!({"error":"Outside document qualification scope"}),
                        );
                        continue;
                    }
                    if let Err(error) = serve(&mut stream, &mut ws, &mut session, &token, port) {
                        json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("Document listener failed: {error}"),
            }
        };
        std::fs::write(root.join("server.private.json"),json!({"client_exit_code":status.code(),"denied":denied,"model":format!("{:?}",resident_model_identity().unwrap())}).to_string()).unwrap();
        assert!(
            status.success(),
            "Client failed; inspect {}",
            root.join("client.private.log").display()
        );
        assert!(denied.is_empty(), "Unexpected document client mutations");
        assert!(
            session.is_none(),
            "Document work must never create a form session"
        );
        let report: Value =
            serde_json::from_slice(&std::fs::read(root.join("result.private.json")).unwrap())
                .unwrap();
        assert_eq!(report["complete"], true);
        assert_eq!(
            resident_model_identity().unwrap().unwrap(),
            identity,
            "The resident model changed during document qualification"
        );
        assert_eq!(report["model_identity"]["digest"], identity.digest);

        if template_flow {
            use std::io::Read;
            let original = std::fs::read(&template).unwrap();
            let edited = ws
                .document_content(report["edited"]["docx_id"].as_str().unwrap())
                .unwrap()
                .unwrap();
            std::fs::write(root.join("edited-template.docx"), &edited).unwrap();
            let mut before = zip::ZipArchive::new(std::io::Cursor::new(original)).unwrap();
            let mut after = zip::ZipArchive::new(std::io::Cursor::new(edited)).unwrap();
            assert_eq!(before.len(), after.len());
            for index in 0..before.len() {
                let mut part = before.by_index(index).unwrap();
                let name = part.name().unwrap().into_owned();
                let mut old = Vec::new();
                part.read_to_end(&mut old).unwrap();
                let mut new = Vec::new();
                after.by_name(&name).unwrap().read_to_end(&mut new).unwrap();
                if name == "word/document.xml" {
                    assert_ne!(old, new, "Expected a meaningful actual-model edit");
                } else {
                    assert_eq!(
                        old, new,
                        "Unchanged package part must retain exact bytes: {name}"
                    );
                }
            }
        }
        for item in report["documents"].as_array().unwrap() {
            let bytes = ws
                .document_content(item["id"].as_str().unwrap())
                .unwrap()
                .unwrap();
            assert!(bytes.starts_with(b"%PDF-"));
            assert_eq!(waypoint_domain::sha256_hex(&bytes), item["sha256"]);
            std::fs::write(
                root.join(format!("{}.pdf", item["kind"].as_str().unwrap())),
                bytes,
            )
            .unwrap();
            let attempt = item["visual"]["attempt_id"].as_str().unwrap();
            let history = report["history"]["attempts"].as_array().unwrap();
            let row = history.iter().find(|r| r["attempt_id"] == attempt).unwrap();
            let material_root = PathBuf::from(row["folder"].as_str().unwrap())
                .join("materials")
                .canonicalize()
                .unwrap();
            assert!(material_root.starts_with(root.canonicalize().unwrap()));
            for finding in item["visual"]["report"]["findings"].as_array().unwrap() {
                let capture = &finding["image_capture"];
                let file = capture["file"].as_str().unwrap();
                assert!(Path::new(file)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))));
                let image = std::fs::read(material_root.join(file)).unwrap();
                assert_eq!(waypoint_domain::sha256_hex(&image), capture["sha256"]);
                assert_eq!(image.len() as u64, capture["bytes"].as_u64().unwrap());
                std::fs::write(
                    root.join(format!(
                        "{}-page-{}.png",
                        item["kind"].as_str().unwrap(),
                        capture["page"].as_u64().unwrap()
                    )),
                    image,
                )
                .unwrap();
            }
        }
    });
}
