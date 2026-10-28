//! Start PDF workers inside a bounded Windows job before they execute.
#[cfg(windows)]
#[derive(Debug)]
pub struct WorkerOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
}

#[cfg(windows)]
pub fn run(
    command: &std::process::Command,
    input: Option<&[u8]>,
    memory: usize,
    seconds: u64,
    output_limit: usize,
) -> Result<WorkerOutput, String> {
    use processkit::{
        Command, OutputBufferPolicy, ProcessGroup, ProcessGroupOptions, Stdin, StdioMode,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Cannot start document worker runtime: {e}"))?;
    runtime.block_on(async {
        let group =
            ProcessGroup::with_options(ProcessGroupOptions::default().max_memory(memory as u64))
                .map_err(|e| format!("Cannot create document resource limit: {e}"))?;
        let mut cmd = Command::new(command.get_program())
            .args(command.get_args())
            .create_no_window()
            .stderr(StdioMode::Null)
            .timeout(std::time::Duration::from_secs(seconds))
            .output_buffer(
                OutputBufferPolicy::fail_loud(usize::MAX).with_max_bytes(output_limit.max(1)),
            );
        if output_limit == 0 {
            cmd = cmd.stdout(StdioMode::Null);
        }
        if let Some(bytes) = input {
            cmd = cmd.stdin(Stdin::from_bytes(bytes.to_vec()));
        }
        if let Some(dir) = command.get_current_dir() {
            cmd = cmd.current_dir(dir);
        }
        for (key, value) in command.get_envs() {
            cmd = match value {
                Some(value) => cmd.env(key, value),
                None => cmd.env_remove(key),
            };
        }
        let process = group
            .start(&cmd)
            .await
            .map_err(|e| format!("Could not start bounded document worker: {e}"))?;
        if output_limit == 0 {
            let outcome = process
                .wait()
                .await
                .map_err(|e| format!("Document worker failed: {e}"))?;
            if outcome.timed_out() {
                return Err("Document operation timed out".into());
            }
            return Ok(WorkerOutput {
                success: outcome.code() == Some(0),
                code: outcome.code(),
                stdout: Vec::new(),
            });
        }
        let result = process
            .output_bytes()
            .await
            .map_err(|e| format!("Document worker failed: {e}"))?;
        if result.timed_out() {
            return Err("Document operation timed out".into());
        }
        if result.truncated() {
            return Err("Document worker output exceeded its limit".into());
        }
        Ok(WorkerOutput {
            success: result.is_success(),
            code: result.code(),
            stdout: result.stdout().clone(),
        })
    })
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore = "requires local Python; verifies a small Windows job limit"]
    fn bounds_worker_committed_memory() {
        let python = std::env::var_os("WAYPOINT_PDF_PYTHON").unwrap();
        let script = "import sys\ntry:\n x=bytearray(96*1024*1024)\nexcept MemoryError:\n sys.exit(0)\nsys.exit(9)";
        let mut cmd = std::process::Command::new(python);
        cmd.args(["-I", "-c", script]);
        let output = super::run(&cmd, None, 64 * 1024 * 1024, 10, 1024).unwrap();
        assert!(
            output.success,
            "worker allocation was not rejected: {:?}",
            output.code
        );
    }

    #[test]
    #[ignore = "requires local Python; verifies output and time limits"]
    fn enforces_output_and_time_limits() {
        let python = std::env::var_os("WAYPOINT_PDF_PYTHON").unwrap();
        let mut noisy = std::process::Command::new(&python);
        noisy.args(["-I", "-c", "print('x'*4096)"]);
        assert!(super::run(&noisy, None, 64 * 1024 * 1024, 10, 128).is_err());
        let mut slow = std::process::Command::new(python);
        slow.args(["-I", "-c", "import time; time.sleep(10)"]);
        assert!(super::run(&slow, None, 64 * 1024 * 1024, 1, 128)
            .unwrap_err()
            .contains("timed out"));
    }

    #[test]
    #[ignore = "requires local Pillow; checks rendered pixel comparison"]
    fn visual_guard_distinguishes_inside_and_outside_changes() {
        let python = std::env::var_os("WAYPOINT_PDF_PYTHON").unwrap();
        let script = concat!(
            include_str!("pdf_visual_guard.py"),
            r#"
from PIL import Image
a = Image.new('RGBA', (30, 30), 'white')
b = a.copy()
b.putpixel((8, 8), (0, 0, 0, 255))
assert not changed_outside(a, b, (5, 5, 12, 12))[0]
assert changed_outside(a, b, None)[0]
b.putpixel((20, 20), (255, 0, 0, 255))
assert changed_outside(a, b, (5, 5, 12, 12))[0]
assert changed_outside(a, Image.new('RGB', (31, 30)), None)[0]
"#
        );
        let mut cmd = std::process::Command::new(python);
        cmd.args(["-I", "-c", script]);
        let output = super::run(&cmd, None, 128 * 1024 * 1024, 10, 4096).unwrap();
        assert!(
            output.success,
            "visual guard fixture failed: {:?}",
            output.code
        );
    }
    #[test]
    #[ignore = "requires local pypdf and PDFium; checks split text operators"]
    fn stream_edit_handles_split_text_and_keeps_other_content() {
        let python = std::env::var_os("WAYPOINT_PDF_PYTHON").unwrap();
        let script = concat!(
            include_str!("pdf_stream_edit.py"),
            r#"
from pypdf import PdfReader, PdfWriter
from pypdf.generic import DictionaryObject, NameObject, DecodedStreamObject
from io import BytesIO
import pypdfium2 as pdfium
w = PdfWriter()
page = w.add_blank_page(612, 792)
font = DictionaryObject({NameObject('/Type'):NameObject('/Font'), NameObject('/Subtype'):NameObject('/Type1'), NameObject('/BaseFont'):NameObject('/Helvetica'), NameObject('/Encoding'):NameObject('/WinAnsiEncoding')})
page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'):DictionaryObject({NameObject('/F1'):w._add_object(font)})})
content = DecodedStreamObject()
content.set_data(b'BT /F1 12 Tf 72 700 Td [(Res) 15 (earch)] TJ ET BT /F1 12 Tf 72 650 Td (Unchanged) Tj ET')
page[NameObject('/Contents')] = w._add_object(content)
source = BytesIO(); w.write(source)
r = PdfReader(BytesIO(source.getvalue()))
edited = edit_pdf_stream(r, 0, 'Research', 'Search')
assert edited is not None
d = pdfium.PdfDocument(edited); page = d[0]; text = page.get_textpage().get_text_range()
assert 'Search' in text and 'Unchanged' in text and 'Research' not in text
assert edit_pdf_stream(r, 0, 'Research', '\u2603') is None
"#
        );
        let mut cmd = std::process::Command::new(python);
        cmd.args(["-I", "-c", script]);
        let output = super::run(&cmd, None, 128 * 1024 * 1024, 15, 4096).unwrap();
        assert!(
            output.success,
            "stream editing fixture failed: {:?}",
            output.code
        );
    }
}
