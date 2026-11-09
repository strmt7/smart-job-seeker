//! Single local PDF worker. Never loads PDF bytes into the web server's Python process.
#[cfg(not(windows))]
use std::{
    io::{Read, Seek, SeekFrom, Write},
    process::Stdio,
    time::{Duration, Instant},
};
use std::{path::PathBuf, process::Command};

pub(crate) const PDF_MAX_PAGES: usize = 50;

fn is_store_alias_directory(path: &std::path::Path, local_app_data: &std::path::Path) -> bool {
    let normalize = |value: &std::path::Path| {
        value
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .replace('/', "\\")
    };
    normalize(path).eq_ignore_ascii_case(&normalize(
        &local_app_data.join("Microsoft").join("WindowsApps"),
    ))
}

fn configured_python() -> Option<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("WAYPOINT_PDF_PYTHON") {
        paths.push(PathBuf::from(p));
    }
    if let Ok(control) = crate::storage_settings::control_dir() {
        if let Some(python) = crate::pdf_setup::managed_python(&control) {
            paths.push(python);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("dependencies/python/python.exe"));
        }
    }
    paths.into_iter().find(|path| path.is_file())
}

fn path_pythons() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&env) {
            if std::env::var_os("LOCALAPPDATA")
                .is_some_and(|base| is_store_alias_directory(&dir, std::path::Path::new(&base)))
            {
                continue;
            }
            let path = dir.join("python.exe");
            if path.is_file() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

pub fn python_path() -> Option<PathBuf> {
    configured_python().or_else(|| path_pythons().into_iter().next())
}

// Setup may need venv and ensurepip even when the first PATH interpreter does
// not provide them. Explicit, managed and bundled runtimes keep precedence.
// Call only from the setup worker: probes can take up to two seconds each.
#[cfg(all(test, windows))]
fn supports_pdf_setup(path: &std::path::Path) -> bool {
    let mut command = Command::new(path);
    command.args([
        "-I",
        "-c",
        "import sys,venv,ensurepip; sys.exit(0 if sys.version_info >= (3,10) else 1)",
    ]);
    crate::pdf_job::run(&command, None, 128 * 1024 * 1024, 2, 0).is_ok_and(|result| result.success)
}

#[cfg(test)]
fn choose_setup_python(
    paths: Vec<PathBuf>,
    mut supports: impl FnMut(&std::path::Path) -> bool,
) -> Option<PathBuf> {
    paths.into_iter().take(4).find(|path| supports(path))
}

// Probe only automatic PATH candidates. Explicit, managed and bundled runtimes
// retain their established precedence and failures remain visible. The first
// candidate is retained as a fallback so missing-package diagnostics survive.
#[cfg(any(windows, test))]
fn choose_path_python(
    paths: Vec<PathBuf>,
    mut supports: impl FnMut(&std::path::Path) -> bool,
) -> Option<PathBuf> {
    let fallback = paths.first().cloned();
    paths
        .into_iter()
        .take(4)
        .find(|path| supports(path))
        .or(fallback)
}

#[cfg(windows)]
fn worker_python(modules: &str) -> Option<PathBuf> {
    configured_python().or_else(|| choose_path_python(path_pythons(), |path| {
        let mut command = Command::new(path);
        command.args(["-I", "-c", "import importlib.util,sys; sys.exit(0 if all(importlib.util.find_spec(n) is not None for n in sys.argv[1:]) else 1)"]);
        command.args(modules.split(','));
        crate::pdf_job::run(&command, None, 128 * 1024 * 1024, 2, 0)
            .is_ok_and(|result| result.success)
    }))
}

#[cfg(not(windows))]
fn worker_python(_modules: &str) -> Option<PathBuf> {
    python_path()
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    #[test]
    fn excludes_the_store_alias_directory_from_automatic_path_discovery() {
        let base = std::path::Path::new("C:/Users/fixture/AppData/Local");
        assert!(is_store_alias_directory(
            &base.join("Microsoft/WindowsApps"),
            base
        ));
        assert!(!is_store_alias_directory(
            &base.join("Programs/Python/Python312"),
            base
        ));
    }
    #[test]
    fn path_selection_prefers_supported_runtime_with_a_bounded_fallback() {
        let paths: Vec<_> = (0..6)
            .map(|n| PathBuf::from(format!("python-{n}")))
            .collect();
        let mut calls = Vec::new();
        let selected = choose_path_python(paths.clone(), |path| {
            calls.push(path.to_owned());
            path == std::path::Path::new("python-2")
        });
        assert_eq!(selected, Some(paths[2].clone()));
        assert_eq!(calls.len(), 3);
        let mut count = 0;
        assert_eq!(
            choose_path_python(paths.clone(), |_| {
                count += 1;
                false
            }),
            Some(paths[0].clone())
        );
        assert_eq!(count, 4);
        assert!(choose_path_python(Vec::new(), |_| true).is_none());
        let mut count = 0;
        assert!(choose_setup_python(paths, |_| {
            count += 1;
            false
        })
        .is_none());
        assert_eq!(count, 4);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "probes a user-specified local Python without modifying it"]
    fn setup_probe_skips_an_incompatible_executable() {
        let compatible = PathBuf::from(std::env::var_os("WAYPOINT_TEST_SETUP_PYTHON").unwrap());
        let incompatible =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/where.exe");
        assert!(!supports_pdf_setup(&incompatible));
        assert!(supports_pdf_setup(&compatible));
        assert_eq!(
            choose_setup_python(vec![incompatible, compatible.clone()], supports_pdf_setup),
            Some(compatible)
        );
    }
}

static PDF_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) struct PdfLease;
impl Drop for PdfLease {
    fn drop(&mut self) {
        PDF_BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}
pub(crate) fn acquire() -> Result<PdfLease, String> {
    PDF_BUSY
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| "A PDF operation is already running")?;
    Ok(PdfLease)
}
fn run(
    bytes: &[u8],
    input: &[u8],
    script: &str,
    argument: Option<&str>,
    limit: u64,
    pdf_input: bool,
    modules: &str,
) -> Result<Vec<u8>, String> {
    let _lease = acquire()?;
    if bytes.is_empty()
        || bytes.len() > 10 * 1024 * 1024
        || (pdf_input && !bytes.starts_with(b"%PDF-"))
    {
        return Err("Invalid or oversized document".into());
    }
    let python = worker_python(modules)
        .ok_or("PDF operations need a local Python runtime; configure WAYPOINT_PDF_PYTHON")?;
    let mut command = Command::new(python);
    command.args(["-I", "-c", script]);
    if let Some(argument) = argument {
        command.arg(argument);
    }
    #[cfg(windows)]
    {
        let result =
            crate::pdf_job::run(&command, Some(input), 512 * 1024 * 1024, 30, limit as usize)?;
        if !result.success {
            let message = String::from_utf8_lossy(&result.stdout);
            return Err(if message.trim().is_empty() {
                format!("PDF operation failed ({:?})", result.code)
            } else {
                message.trim().chars().take(600).collect()
            });
        }
        Ok(result.stdout)
    }
    #[cfg(not(windows))]
    {
        let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::from(output.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start PDF worker: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("PDF worker input unavailable")?;
        let input = input.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = writer.join();
                    return Err("PDF operation timed out".into());
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = writer.join();
                    return Err(error.to_string());
                }
            }
        };
        let _ = writer.join();
        output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        output
            .take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 > limit {
            return Err("PDF worker output exceeds its limit".into());
        }
        if !status.success() {
            let message = String::from_utf8_lossy(&data);
            return Err(if message.trim().is_empty() {
                "PDF operation failed".into()
            } else {
                message.trim().chars().take(600).collect()
            });
        }
        Ok(data)
    }
}
/// Extract selectable text and identify pages where none was found.
pub fn extract_with_coverage(bytes: &[u8]) -> Result<(String, serde_json::Value), String> {
    extract_coverage_inner(bytes, false).map(|(text, coverage, _)| (text, coverage))
}

/// Alternative layout extraction for explicit text review, never an automatic
/// replacement for the normal extractor used by other consumers.
pub fn extract_layout_with_coverage(bytes: &[u8]) -> Result<(String, serde_json::Value), String> {
    let (text, mut coverage, _) = extract_coverage_mode(bytes, false, true)?;
    coverage["text_mode"] = serde_json::json!("layout");
    Ok((text, coverage))
}

/// Inventory scanned pages even when the whole document has no selectable text.
#[cfg(test)]
pub(crate) fn scan_coverage(bytes: &[u8]) -> Result<(String, serde_json::Value), String> {
    scan_coverage_with_pages(bytes).map(|(text, coverage, _)| (text, coverage))
}

pub(crate) fn scan_coverage_with_pages(
    bytes: &[u8],
) -> Result<(String, serde_json::Value, Vec<String>), String> {
    extract_coverage_inner(bytes, true)
}

fn extract_coverage_inner(
    bytes: &[u8],
    allow_empty: bool,
) -> Result<(String, serde_json::Value, Vec<String>), String> {
    extract_coverage_mode(bytes, allow_empty, false)
}

fn extract_coverage_mode(
    bytes: &[u8],
    allow_empty: bool,
    layout: bool,
) -> Result<(String, serde_json::Value, Vec<String>), String> {
    let data = run(
        bytes,
        bytes,
        include_str!("pdf_extract.py"),
        if layout {
            Some("layout")
        } else {
            allow_empty.then_some("allow-empty")
        },
        2_600_000,
        true,
        "pypdf",
    )?;
    let result: serde_json::Value =
        serde_json::from_slice(&data).map_err(|_| "PDF extractor returned invalid data")?;
    let text = result["text"]
        .as_str()
        .filter(|text| (allow_empty || !text.trim().is_empty()) && text.len() <= 200_000)
        .ok_or("PDF extractor returned invalid text")?;
    let pages = result["page_count"]
        .as_u64()
        .filter(|pages| (1..=PDF_MAX_PAGES as u64).contains(pages))
        .ok_or("PDF extractor returned invalid page count")?;
    let page_texts: Vec<String> = result["page_texts"]
        .as_array()
        .filter(|items| items.len() == pages as usize)
        .and_then(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or("PDF extractor returned invalid page text")?;
    if page_texts.join("\n").trim() != text {
        return Err("PDF extractor returned inconsistent page text".into());
    }
    let empty = result["empty_text_pages"]
        .as_array()
        .filter(|empty| empty.len() <= pages as usize)
        .ok_or("PDF extractor returned invalid page coverage")?;
    let mut previous = 0;
    for page in empty {
        let page = page
            .as_u64()
            .filter(|page| *page > previous && *page <= pages)
            .ok_or("PDF extractor returned invalid page coverage")?;
        previous = page;
    }
    if page_texts.iter().enumerate().any(|(index, text)| {
        text.trim().is_empty() != empty.contains(&serde_json::json!(index + 1))
    }) {
        return Err("PDF extractor returned inconsistent page coverage".into());
    }
    let blank = result["structurally_blank_pages"]
        .as_array()
        .filter(|blank| blank.len() <= empty.len())
        .ok_or("PDF extractor returned invalid blank-page coverage")?;
    let mut last = 0;
    for page in blank {
        let number = page
            .as_u64()
            .filter(|number| *number > last && empty.contains(page))
            .ok_or("PDF extractor returned invalid blank-page coverage")?;
        last = number;
    }
    let mut image_lists = Vec::new();
    for key in ["mixed_image_pages", "image_inventory_unknown_pages"] {
        let list = result[key]
            .as_array()
            .filter(|list| list.len() <= pages as usize)
            .ok_or("PDF extractor returned invalid image coverage")?;
        let mut previous = 0;
        for value in list {
            let number = value
                .as_u64()
                .filter(|number| *number > previous && *number <= pages && !empty.contains(value))
                .ok_or("PDF extractor returned invalid image coverage")?;
            previous = number;
        }
        image_lists.push(list);
    }
    Ok((
        text.to_owned(),
        serde_json::json!({"pages":pages,"pages_without_selectable_text":empty,"structurally_blank_pages":blank,"pages_with_images_and_text":image_lists[0],"pages_with_unknown_image_inventory":image_lists[1]}),
        page_texts,
    ))
}

/// Consumers without coverage handling must not silently use an incomplete PDF.
pub fn extract_checked_with_coverage(bytes: &[u8]) -> Result<(String, serde_json::Value), String> {
    let (text, coverage) = extract_with_coverage(bytes)?;
    if coverage["pages_without_selectable_text"]
        .as_array()
        .map(Vec::len)
        != coverage["structurally_blank_pages"]
            .as_array()
            .map(Vec::len)
    {
        return Err("Some PDF pages have no selectable text; review the extracted pages and use OCR or a text-based document".into());
    }
    Ok((text, coverage))
}
pub fn extract(bytes: &[u8]) -> Result<String, String> {
    extract_checked_with_coverage(bytes).map(|(text, _)| text)
}
pub fn render_page(bytes: &[u8], page: usize) -> Result<serde_json::Value, String> {
    render_page_inner(bytes, page, "full")
}
pub fn render_page_for_review(bytes: &[u8], page: usize) -> Result<serde_json::Value, String> {
    render_page_inner(bytes, page, "geometry")
}
pub fn render_page_region(
    bytes: &[u8],
    page: usize,
    region: &str,
) -> Result<serde_json::Value, String> {
    if !matches!(region, "full" | "top" | "bottom" | "left" | "right") {
        return Err("Invalid page region".into());
    }
    render_page_inner(bytes, page, region)
}
fn render_page_inner(bytes: &[u8], page: usize, region: &str) -> Result<serde_json::Value, String> {
    if page >= PDF_MAX_PAGES {
        return Err("Page number exceeds 50-page limit".into());
    }
    let argument = if region == "full" {
        page.to_string()
    } else {
        format!("{page}:{region}")
    };
    let data = run(
        bytes,
        bytes,
        include_str!("pdf_render.py"),
        Some(&argument),
        4 * 1024 * 1024,
        true,
        "pypdf,pypdfium2,PIL",
    )?;
    serde_json::from_slice(&data).map_err(|_| "PDF renderer returned invalid data".into())
}
pub fn edit(bytes: &[u8], before: &str, after: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    if before.is_empty()
        || before.len() > 1000
        || after.len() > 4000
        || before.chars().chain(after.chars()).any(char::is_control)
    {
        return Err("Choose a nonempty single-line passage and replacement".into());
    }
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("Invalid or oversized PDF".into());
    }
    let input = serde_json::to_vec(&serde_json::json!({"pdf":base64::engine::general_purpose::STANDARD.encode(bytes),"before":before,"after":after})).map_err(|e| e.to_string())?;
    run(
        bytes,
        &input,
        concat!(
            include_str!("pdf_visual_guard.py"),
            "\n",
            include_str!("pdf_stream_edit.py"),
            "\n",
            include_str!("pdf_edit.py")
        ),
        None,
        10 * 1024 * 1024,
        true,
        "pypdf,pypdfium2,PIL",
    )
}

pub fn image_to_pdf(bytes: &[u8]) -> Result<Vec<u8>, String> {
    run(
        bytes,
        bytes,
        include_str!("image_to_pdf.py"),
        None,
        10 * 1024 * 1024,
        false,
        "PIL",
    )
}

pub fn probe_packages() -> Result<serde_json::Value, String> {
    let script = r#"import json, importlib.metadata, sys
try:
 import pypdf, pypdfium2, PIL
 print(json.dumps({"python":sys.version.split()[0],"pypdf":importlib.metadata.version("pypdf"),"pypdfium2":importlib.metadata.version("pypdfium2"),"pillow":importlib.metadata.version("Pillow")}))
except Exception as error:
 print("PDF dependency check failed: " + str(error)[:400])
 sys.exit(1)
"#;
    let result = run(
        b"probe",
        b"",
        script,
        None,
        4096,
        false,
        "pypdf,pypdfium2,PIL",
    )?;
    serde_json::from_slice(&result).map_err(|_| "PDF dependency check returned invalid data".into())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "checks two public CVs from a user-supplied local directory"]
    fn edits_public_cv_heading_streams() {
        let root = std::path::PathBuf::from(std::env::var_os("WAYPOINT_PUBLIC_CV_DIR").unwrap());
        for (index, before, after) in [
            (0, "PUBLICATIONS", "POSITIONS"),
            (1, "Employment", "Employee"),
        ] {
            let source = std::fs::read(root.join(format!("cv-{index}.pdf"))).unwrap();
            let edited = super::edit(&source, before, after)
                .unwrap_or_else(|error| panic!("CV fixture {index} needs review: {error}"));
            std::fs::write(root.join(format!("cv-{index}-test-edit.pdf")), edited).unwrap();
        }
    }
    #[test]
    fn only_one_pdf_operation_can_hold_the_worker_lease() {
        let lease = super::acquire().unwrap();
        assert!(super::acquire().is_err());
        drop(lease);
        assert!(super::acquire().is_ok());
    }
    #[test]
    #[ignore = "inspects an explicitly supplied public CV; retains no text in the repository"]
    fn public_cv_extracts_and_renders_without_recording_content() {
        let path = std::env::var_os("WAYPOINT_PUBLIC_CV_FIXTURE").expect("public CV fixture path");
        let bytes = std::fs::read(path).unwrap();
        let (text, coverage) = super::extract_with_coverage(&bytes).unwrap();
        let page = super::render_page_for_review(&bytes, 0).unwrap();
        assert!(!text.trim().is_empty());
        assert_eq!(page["page"], 0);
        assert_eq!(page["pages"], coverage["pages"]);
        let count = coverage["pages"].as_u64().unwrap() as usize;
        if count > 30 {
            let last = super::render_page_for_review(&bytes, count - 1).unwrap();
            assert_eq!(last["page"], count - 1);
            assert_eq!(last["pages"], count);
        }
        assert!(page["image"]
            .as_str()
            .is_some_and(|image| image.starts_with("iVBOR")));
        println!("Public CV: {} bytes, {} pages, {} extracted UTF-8 bytes, {} pages without selectable text, {} image/text pages", bytes.len(), coverage["pages"], text.len(), coverage["pages_without_selectable_text"].as_array().unwrap().len(), coverage["pages_with_images_and_text"].as_array().unwrap().len());
    }

    #[test]
    #[ignore = "edits an explicitly supplied public CV into a separate local file"]
    fn public_cv_edit_either_succeeds_or_reports_manual_review() {
        let path = std::env::var_os("WAYPOINT_PUBLIC_CV_FIXTURE").expect("public CV fixture path");
        let before = std::fs::read(&path).unwrap();
        let passage = std::env::var("WAYPOINT_PUBLIC_CV_PASSAGE")
            .unwrap_or_else(|_| "Curriculum Vitae".into());
        let replacement = std::env::var("WAYPOINT_PUBLIC_CV_REPLACEMENT")
            .unwrap_or_else(|_| "Career Profile".into());
        let outcome = super::edit(&before, &passage, &replacement);
        match outcome {
            Ok(pdf) => {
                let text = super::extract(&pdf).unwrap();
                assert!(text.contains(&replacement));
                if let Some(output) = std::env::var_os("WAYPOINT_PUBLIC_CV_OUTPUT") {
                    use std::io::Write;
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(output)
                        .unwrap()
                        .write_all(&pdf)
                        .unwrap();
                }
                println!(
                    "Public CV edit: successful; {} output bytes; review layout manually",
                    pdf.len()
                );
            }
            Err(error) => {
                assert!(
                    error.contains("review manually"),
                    "unexpected PDF editing failure: {error}"
                );
                println!("Public CV edit: manual review required: {error}");
            }
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    #[ignore = "requires pypdfium2, Pillow, pypdf and a fictional PDF"]
    fn renders_a_fixture_and_rejects_invalid_pages() {
        use base64::Engine;
        let path = std::env::var("WAYPOINT_PDF_FIXTURE").unwrap();
        let bytes = std::fs::read(path).unwrap();
        let page = super::render_page(&bytes, 0).unwrap();
        assert_eq!(page["page"], 0);
        assert!(page["pages"].as_u64().unwrap() > 0);
        let png = base64::engine::general_purpose::STANDARD
            .decode(page["image"].as_str().unwrap())
            .unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(super::render_page(&bytes, super::PDF_MAX_PAGES).is_err());
        assert!(super::render_page(b"%PDF-1.4 broken", 0).is_err());
    }
    #[test]
    #[ignore = "requires the local PDF renderer"]
    fn renders_overlapping_regions_of_the_requested_page() {
        let bytes = include_bytes!("../tests/fictional-document-overlap.pdf");
        let full = super::render_page_region(bytes, 0, "full").unwrap();
        let top = super::render_page_region(bytes, 0, "top").unwrap();
        let bottom = super::render_page_region(bytes, 0, "bottom").unwrap();
        assert_eq!(full["region"], "full");
        assert_eq!(top["region"], "top");
        assert_eq!(bottom["region"], "bottom");
        assert_ne!(full["image"], top["image"]);
        assert_ne!(top["image"], bottom["image"]);
        fn size(value: &serde_json::Value) -> (u32, u32) {
            use base64::Engine;
            let png = base64::engine::general_purpose::STANDARD
                .decode(value["image"].as_str().unwrap())
                .unwrap();
            (
                u32::from_be_bytes(png[16..20].try_into().unwrap()),
                u32::from_be_bytes(png[20..24].try_into().unwrap()),
            )
        }
        let (width, height) = size(&full);
        let (top_width, top_height) = size(&top);
        let (bottom_width, bottom_height) = size(&bottom);
        let full_scale = full["scale"].as_f64().unwrap();
        let top_scale = top["scale"].as_f64().unwrap();
        let bottom_scale = bottom["scale"].as_f64().unwrap();
        assert!(top_scale > full_scale && bottom_scale > full_scale);
        assert!(top_width > width && bottom_width > width);
        assert!(
            (top_height as f64 / top_scale) + (bottom_height as f64 / bottom_scale)
                > height as f64 / full_scale
        );
        assert!((top_width as u64 * top_height as u64) <= 3_200_000);
        assert!((bottom_width as u64 * bottom_height as u64) <= 3_200_000);
        let left = super::render_page_region(bytes, 0, "left").unwrap();
        let right = super::render_page_region(bytes, 0, "right").unwrap();
        let (lw, lh) = size(&left);
        let (rw, rh) = size(&right);
        assert_eq!(left["region"], "left");
        assert_eq!(right["region"], "right");
        assert_ne!(left["image"], right["image"]);
        assert!(
            (lw as f64 / left["scale"].as_f64().unwrap())
                + (rw as f64 / right["scale"].as_f64().unwrap())
                > width as f64 / full_scale
        );
        assert!((lw as u64 * lh as u64) <= 3_200_000 && (rw as u64 * rh as u64) <= 3_200_000);
        assert!(super::render_page_region(bytes, 0, "unknown").is_err());
    }

    #[test]
    #[ignore = "requires an explicitly configured local pypdf runtime"]
    fn reports_selectable_pages_with_images() {
        let bytes = include_bytes!("../tests/fictional-mixed-image-text.pdf");
        let (text, coverage) = super::extract_with_coverage(bytes).unwrap();
        assert!(!text.trim().is_empty());
        let (_, model_coverage) =
            crate::document_extract::extract_for_model("cv.pdf", bytes).unwrap();
        assert_eq!(
            model_coverage.unwrap()["pages_with_images_and_text"],
            serde_json::json!([1])
        );
        let (_, draft_coverage) = crate::draft_task::extract_template("cv.PDF", bytes).unwrap();
        assert_eq!(
            draft_coverage.unwrap()["pages_with_images_and_text"],
            serde_json::json!([1])
        );
        assert_eq!(
            coverage["pages_with_images_and_text"],
            serde_json::json!([1])
        );
        assert_eq!(
            coverage["pages_with_unknown_image_inventory"],
            serde_json::json!([])
        );
    }

    #[test]
    #[ignore = "requires an explicitly configured local pypdf runtime"]
    fn reports_inline_images_on_selectable_pages() {
        let bytes = include_bytes!("../tests/fictional-inline-image.pdf");
        let (text, coverage) = super::extract_with_coverage(bytes).unwrap();
        assert!(!text.trim().is_empty());
        assert_eq!(
            coverage["pages_with_images_and_text"],
            serde_json::json!([1])
        );
    }

    #[test]
    #[ignore = "requires an explicitly configured local pypdf runtime"]
    fn reports_pages_without_selectable_text() {
        let blank = include_bytes!("../tests/fictional-partial-text.pdf");
        let (_, blank_coverage) = super::extract_with_coverage(blank).unwrap();
        assert_eq!(
            blank_coverage["structurally_blank_pages"],
            serde_json::json!([2])
        );
        assert!(super::extract(blank).is_ok());
        let annotation = include_bytes!("../tests/fictional-annotation-page.pdf");
        let (_, annotation_coverage) = super::extract_with_coverage(annotation).unwrap();
        assert_eq!(
            annotation_coverage["pages_without_selectable_text"],
            serde_json::json!([2])
        );
        assert_eq!(
            annotation_coverage["structurally_blank_pages"],
            serde_json::json!([])
        );
        assert!(super::extract(annotation).is_err());
        let partial = include_bytes!("../tests/fictional-image-page.pdf");
        let (text, coverage) = super::extract_with_coverage(partial).unwrap();
        assert!(!text.is_empty());
        assert_eq!(coverage["pages"], 2);
        assert_eq!(coverage["structurally_blank_pages"], serde_json::json!([]));
        assert_eq!(
            coverage["pages_without_selectable_text"],
            serde_json::json!([2])
        );
        assert!(super::extract(partial)
            .unwrap_err()
            .contains("no selectable text"));
        let (reviewed, report) =
            crate::document_extract::extract_for_review("fictional.pdf", partial).unwrap();
        assert_eq!(reviewed, text);
        assert_eq!(report.unwrap(), coverage);
        let (model_text, model_coverage) =
            crate::document_extract::extract_for_model("fictional.pdf", partial).unwrap();
        assert_eq!(model_text, text);
        assert_eq!(model_coverage.unwrap(), coverage);
        assert!(crate::document_extract::extract("fictional.pdf", partial).is_err());
        assert!(crate::draft_task::extract_template("fictional.pdf", partial).is_err());
        let (template, draft_coverage, vision) = crate::draft_task::scanned_template(partial, None, |_, page| {
            assert_eq!(page, 1);
            Ok(serde_json::json!({"page":1,"pages":2,"text":"Fictional scanned page", "uncertain":true, "image":"ZmljdGlvbmFs"}))
        }).unwrap();
        assert!(template.contains("Fictional scanned page"));
        assert!(template.contains(&text));
        assert!(template.find(&text).unwrap() < template.find("Fictional scanned page").unwrap());
        assert_eq!(draft_coverage.unwrap(), coverage);
        assert_eq!(vision["page"], 2);
        assert_eq!(vision["other_pages_not_transcribed"], 1);
        let scanned = include_bytes!("../tests/fictional-scanned-cv.pdf");
        let mut observed_image = String::new();
        let (template, coverage, vision) =
            crate::draft_task::scanned_template(scanned, None, |bytes, page| {
                assert_eq!(page, 0);
                crate::pdf_transcribe::transcribe(
                    bytes,
                    page,
                    "full",
                    super::render_page,
                    |image| {
                        observed_image = image.to_owned();
                        Ok(serde_json::json!({"text":"Fictional template", "uncertain":false}))
                    },
                )
            })
            .unwrap();
        assert_eq!(template, "Fictional template");
        assert_eq!(
            coverage.unwrap()["pages_without_selectable_text"],
            serde_json::json!([1])
        );
        assert_eq!(vision["page"], 1);
        assert!(observed_image.len() > 100);
        assert_eq!(vision["image"].as_str().unwrap(), observed_image);

        let blank_then_scan = include_bytes!("../tests/fictional-blank-then-scanned.pdf");
        assert!(super::extract(blank_then_scan).is_err());
        let (_, scan_coverage) = super::scan_coverage(blank_then_scan).unwrap();
        assert_eq!(
            scan_coverage["pages_without_selectable_text"],
            serde_json::json!([1, 2])
        );
        assert_eq!(
            scan_coverage["structurally_blank_pages"],
            serde_json::json!([1])
        );
        let (_, coverage, vision) =
            crate::draft_task::scanned_template(blank_then_scan, None, |bytes, page| {
                assert_eq!(page, 1);
                crate::pdf_transcribe::transcribe(bytes, page, "full", super::render_page, |_| {
                    Ok(serde_json::json!({"text":"Fictional template", "uncertain":false}))
                })
            })
            .unwrap();
        assert_eq!(coverage.unwrap(), scan_coverage);
        assert_eq!(vision["page"], 2);
        for page in [0, 2, usize::MAX] {
            assert!(
                crate::draft_task::scanned_template(blank_then_scan, Some(page), |_, _| {
                    panic!("invalid page must not invoke vision")
                })
                .is_err()
            );
        }
        let (_, _, chosen) = crate::draft_task::scanned_template(blank_then_scan, Some(1), |_, page| {
            Ok(serde_json::json!({"page":page,"pages":2,"text":"Fictional template","uncertain":false,"image":"ZmljdGlvbmFs"}))
        }).unwrap();
        assert_eq!(chosen["page"], 2);

        let complete = include_bytes!("../tests/fictional-document-overlap.pdf");
        let (_, complete_coverage) = super::extract_with_coverage(complete).unwrap();
        assert_eq!(
            complete_coverage["pages_without_selectable_text"],
            serde_json::json!([])
        );
        assert!(super::extract(complete).is_ok());
    }

    #[test]
    #[ignore = "requires an explicitly configured local pypdf runtime and fictional PDF"]
    fn extracts_fixture() {
        let p = std::env::var("WAYPOINT_PDF_FIXTURE").unwrap();
        let bytes = std::fs::read(p).unwrap();
        let text = super::extract(&bytes).unwrap();
        assert!(text.contains("Fictional CV"));
        assert!(text.contains("Synthetic experience"));
        assert!(super::extract(b"%PDF-1.4 broken").is_err());
    }
}
