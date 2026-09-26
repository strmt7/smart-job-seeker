use waypoint_materials::to_docx_bytes;
use waypoint_materials::tree::{DocNode, DocNodeKind, SemanticDocument};

fn sample_doc() -> SemanticDocument {
    SemanticDocument {
        id: "cv-x".into(),
        kind: "cv".into(),
        profile_revision: 1,
        root: DocNode {
            id: "r".into(),
            kind: DocNodeKind::Document,
            text: String::new(),
            claim_ids: vec![],
            locked: false,
            children: vec![
                DocNode::heading("Experience"),
                DocNode::text_span("Engineer at Acme (2020-2022)", None),
            ],
        },
    }
}

/// The generated .docx must open as a real zip and carry the required parts.
#[test]
fn docx_roundtrip_validated_by_external_parser() {
    let bytes = to_docx_bytes(&sample_doc());
    let dir = std::env::temp_dir();
    let path = dir.join(format!("waypoint-test-{}.docx", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();

    // Python's zipfile is an independent implementation: if it reads the
    // archive and finds the parts, our writer is spec-correct enough.
    let out = std::process::Command::new("python")
        .args([
            "-c",
            &format!(
                "import zipfile,sys;\
                 z=zipfile.ZipFile(r'{}');\
                 names=z.namelist();\
                 assert '[Content_Types].xml' in names and 'word/document.xml' in names, names;\
                 bad=z.testzip();\
                 assert bad is None, bad;\
                 xml=z.read('word/document.xml').decode('utf-8');\
                 assert 'Engineer at Acme' in xml;\
                 print('ZIP-OK', len(names))",
                path.to_string_lossy()
            ),
        ])
        .output()
        .expect("python is available in the test environment");
    assert!(
        out.status.success(),
        "external zip validation failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("ZIP-OK"));
    let _ = std::fs::remove_file(&path);
}
