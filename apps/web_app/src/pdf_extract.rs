//! Single local PDF worker. Never loads PDF bytes into the web server's Python process.
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn python_path() -> Option<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("WAYPOINT_PDF_PYTHON") {
        paths.push(PathBuf::from(p));
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
struct PdfLease;
impl Drop for PdfLease {
    fn drop(&mut self) {
        PDF_BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}
pub fn extract(bytes: &[u8]) -> Result<String, String> {
    PDF_BUSY
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| "A PDF extraction is already running")?;
    let _lease = PdfLease;
    if bytes.len() > 10 * 1024 * 1024 || !bytes.starts_with(b"%PDF-") {
        return Err("Invalid or oversized PDF".into());
    }
    let python=python_path().ok_or("PDF text extraction needs a local Python runtime with pypdf; configure WAYPOINT_PDF_PYTHON")?;
    let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", include_str!("pdf_extract.py")])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(output.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start PDF extractor: {e}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or("PDF extractor input unavailable")?;
    let input = bytes.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                return Err("PDF extraction timed out".into());
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
        .take(200001)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 200000 {
        return Err("PDF text exceeds 200 KB".into());
    }
    if !status.success() {
        let message = String::from_utf8_lossy(&data);
        return Err(if message.trim().is_empty() {
            "PDF extraction failed".into()
        } else {
            message.trim().chars().take(600).collect()
        });
    }
    String::from_utf8(data).map_err(|_| "PDF extractor returned invalid UTF-8".into())
}

#[cfg(test)]
mod tests {
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
