//! Single local PDF worker. Never loads PDF bytes into the web server's Python process.
#[cfg(not(windows))]
use std::{
    io::{Read, Seek, SeekFrom, Write},
    process::Stdio,
    time::{Duration, Instant},
};
use std::{path::PathBuf, process::Command};

pub fn python_path() -> Option<PathBuf> {
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
    if let Some(env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&env) {
            paths.push(dir.join("python.exe"));
        }
    }
    paths.into_iter().find(|path| path.is_file())
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
) -> Result<Vec<u8>, String> {
    let _lease = acquire()?;
    if bytes.is_empty()
        || bytes.len() > 10 * 1024 * 1024
        || (pdf_input && !bytes.starts_with(b"%PDF-"))
    {
        return Err("Invalid or oversized document".into());
    }
    let python = python_path()
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
pub fn extract(bytes: &[u8]) -> Result<String, String> {
    let data = run(
        bytes,
        bytes,
        include_str!("pdf_extract.py"),
        None,
        200000,
        true,
    )?;
    String::from_utf8(data).map_err(|_| "PDF extractor returned invalid UTF-8".into())
}
pub fn render_page(bytes: &[u8], page: usize) -> Result<serde_json::Value, String> {
    if page > 29 {
        return Err("Page number exceeds 30-page limit".into());
    }
    let data = run(
        bytes,
        bytes,
        include_str!("pdf_render.py"),
        Some(&page.to_string()),
        4 * 1024 * 1024,
        true,
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
    )
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
        assert!(super::render_page(&bytes, 30).is_err());
        assert!(super::render_page(b"%PDF-1.4 broken", 0).is_err());
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
