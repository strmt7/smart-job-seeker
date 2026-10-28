//! Bounded local conversion using an installed LibreOffice; no user profile is reused.
#[cfg(not(windows))]
use std::time::{Duration, Instant};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn office_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("WAYPOINT_SOFFICE") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(root) = executable.parent() {
            candidates.push(root.join("dependencies/libreoffice/program/soffice.com"));
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            candidates.push(dir.join("soffice.com"));
        }
    }
    if let Some(path) = std::env::var_os("PROGRAMFILES") {
        candidates.push(PathBuf::from(path).join("LibreOffice/program/soffice.com"));
    }
    if let Some(path) = std::env::var_os("PROGRAMFILES(X86)") {
        candidates.push(PathBuf::from(path).join("LibreOffice/program/soffice.com"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

static CONVERT_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct ConvertLease;
impl Drop for ConvertLease {
    fn drop(&mut self) {
        CONVERT_BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}
pub fn docx_to_pdf(bytes: &[u8]) -> Result<Vec<u8>, String> {
    CONVERT_BUSY
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| "A document conversion is already running")?;
    let _lease = ConvertLease;
    let _document_lease = crate::pdf_extract::acquire()?;
    if bytes.is_empty() || bytes.len() > 10 * 1024 * 1024 {
        return Err("Document size is outside the permitted range".into());
    }
    let office = office_path().ok_or(
        "LibreOffice was not found. Install it or set WAYPOINT_SOFFICE to its executable.",
    )?;
    let temp = tempfile::Builder::new()
        .prefix("waypoint-convert-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let source = temp.path().join("source.docx");
    std::fs::write(&source, bytes).map_err(|e| e.to_string())?;
    let output = temp.path().join("output");
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    let profile = temp.path().join("profile");
    std::fs::create_dir(&profile).map_err(|e| e.to_string())?;
    let profile_url = url::Url::from_directory_path(&profile)
        .map_err(|_| "Could not create office profile URL")?;
    let mut command = Command::new(&office);
    command
        .arg(format!("-env:UserInstallation={profile_url}"))
        .args([
            "--headless",
            "--convert-to",
            "pdf:writer_pdf_Export",
            "--outdir",
        ])
        .arg(&output)
        .arg(&source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        let result = crate::pdf_job::run(&command, None, 768 * 1024 * 1024, 30, 0)
            .map_err(|e| format!("LibreOffice conversion: {e}"))?;
        if !result.success {
            return Err(format!("LibreOffice conversion failed ({:?})", result.code));
        }
    }
    #[cfg(not(windows))]
    {
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start LibreOffice: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err("LibreOffice conversion failed".into());
                }
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("LibreOffice conversion timed out".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    read_pdf(&output.join("source.pdf"))
}
fn read_pdf(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|_| "LibreOffice did not produce a PDF")?;
    if metadata.len() == 0 || metadata.len() > 20 * 1024 * 1024 {
        return Err("Converted PDF is empty or exceeds 20 MiB".into());
    }
    let pdf = std::fs::read(path).map_err(|e| e.to_string())?;
    if !pdf.starts_with(b"%PDF-") {
        return Err("Conversion did not produce a PDF".into());
    }
    Ok(pdf)
}

#[cfg(test)]
mod tests {
    #[test]
    fn conversion_does_not_overlap_another_pdf_operation() {
        let lease = crate::pdf_extract::acquire().unwrap();
        let error = super::docx_to_pdf(b"fictional").unwrap_err();
        assert!(error.contains("PDF operation is already running"));
        drop(lease);
    }
    #[test]
    #[ignore = "runs an installed LibreOffice against an explicit local fixture"]
    fn converts_local_fixture() {
        let input = std::env::var("WAYPOINT_CONVERSION_FIXTURE").expect("fixture path");
        let output = std::env::var("WAYPOINT_CONVERSION_OUTPUT").expect("output path");
        let pdf = super::docx_to_pdf(&std::fs::read(input).unwrap()).unwrap();
        std::fs::write(output, pdf).unwrap();
    }
}
