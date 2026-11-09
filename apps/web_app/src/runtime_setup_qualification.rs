//! Opt-in real private-runtime checks, always on fictional local inputs.
use super::*;

fn python(python: &Path, control: &Path, mode: &str, arguments: &[&str]) -> Result<Value, String> {
    let mut command = std::process::Command::new(python);
    command
        .args(["-I", "-c", include_str!("runtime_setup_smoke.py"), mode])
        .arg(control)
        .args(arguments);
    let result = crate::pdf_job::run(&command, None, 768 * 1024 * 1024, 90, 16384)?;
    if !result.success {
        return Err(format!("Private runtime smoke failed ({:?})", result.code));
    }
    serde_json::from_slice(&result.stdout).map_err(|e| e.to_string())
}
fn qualify(tool: Tool) {
    assert_eq!(
        std::env::var("WAYPOINT_RUNTIME_SETUP_CONTAINED").as_deref(),
        Ok("1"),
        "Use tools/qualify_runtime_setup.py for process-tree containment"
    );
    let control = tempfile::Builder::new()
        .prefix("runtime-component-")
        .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .unwrap()
        .keep();
    println!("Runtime component evidence: {}", control.display());
    let result = (|| -> Result<Value, String> {
        let interpreter = private_python(&control)?;
        let installed = install(&control, tool, &interpreter)?;
        let binary = managed_at(&control, tool).ok_or("Private runtime was not activated")?;
        let smoke = match tool {
            Tool::Office => {
                python(&interpreter, &control, "prepare", &[])?;
                let docx =
                    std::fs::read(control.join("fictional.docx")).map_err(|e| e.to_string())?;
                let pdf = crate::document_convert::docx_to_pdf_using_runtime(&docx, &binary)?;
                std::fs::write(control.join("fictional.pdf"), &pdf).map_err(|e| e.to_string())?;
                let mut value = python(&interpreter, &control, "pdf", &[])?;
                value["pdf_sha256"] = json!(waypoint_domain::sha256_hex(&pdf));
                value
            }
            Tool::Ollama => {
                let version = include_str!("../scripts/package-windows.py")
                    .split("/ollama/releases/download/v")
                    .nth(1)
                    .and_then(|s| s.split('/').next())
                    .ok_or("Pinned Ollama version unavailable")?;
                python(
                    &interpreter,
                    &control,
                    "ollama",
                    &[binary.to_str().ok_or("Non-Unicode runtime path")?, version],
                )?
            }
            Tool::Browser => return Err("Use the existing browser qualification".into()),
        };
        Ok(json!({"installed":installed,"smoke":smoke}))
    })();
    let report = json!({"complete":result.is_ok(),"tool":tool.name(),"scope":"private runtime download, activation and fictional local smoke; no applications or model inference","result":result.as_ref().ok(),"error":result.as_ref().err(),"sources":{"setup":waypoint_domain::sha256_hex(include_str!("runtime_setup.rs").as_bytes()),"worker":waypoint_domain::sha256_hex(include_str!("../scripts/runtime-setup.py").as_bytes()),"smoke":waypoint_domain::sha256_hex(include_str!("runtime_setup_smoke.py").as_bytes()),"qualification":waypoint_domain::sha256_hex(include_str!("runtime_setup_qualification.rs").as_bytes()),"converter":waypoint_domain::sha256_hex(include_str!("document_convert.rs").as_bytes()),"packager":waypoint_domain::sha256_hex(include_str!("../scripts/package-windows.py").as_bytes())}});
    std::fs::write(
        control.join("qualification.private.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(result.is_ok(), "{report}");
}
#[test]
#[ignore = "pinned LibreOffice/PDF runtime downloads and fictional DOCX/PDF conversion"]
fn private_office_repair_downloads_activates_and_converts_docx() {
    qualify(Tool::Office);
}
#[test]
#[ignore = "pinned Ollama/PDF runtime downloads and a private model-free local server"]
fn private_ollama_repair_downloads_activates_and_serves_without_models() {
    qualify(Tool::Ollama);
}
