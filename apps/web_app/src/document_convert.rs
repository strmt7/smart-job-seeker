//! Bounded local conversion using an installed LibreOffice; no user profile is reused.
#[cfg(not(windows))]
use std::time::{Duration, Instant};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn checked_office_path(value: &str) -> Result<PathBuf, String> {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return Err("Choose an existing absolute path to soffice.com".into());
    }
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("soffice.com"))
        || !path.is_file()
    {
        return Err("Choose an existing absolute path to soffice.com".into());
    }
    Ok(path)
}

fn stored_office_path(control: &Path) -> Result<Option<String>, String> {
    let bytes = match std::fs::read(control.join("office-path.json")) {
        Ok(bytes) if bytes.len() <= 8192 => bytes,
        Ok(_) => return Err("LibreOffice setting is too large".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read LibreOffice setting".into()),
    };
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid LibreOffice setting")?;
    let path = value["path"].as_str().ok_or("Missing LibreOffice path")?;
    if path.is_empty() {
        Ok(None)
    } else if path.len() > 2048 || path.chars().any(char::is_control) {
        Err("Invalid LibreOffice setting".into())
    } else {
        Ok(Some(path.to_owned()))
    }
}

fn configured_office_path(control: &Path) -> Result<Option<PathBuf>, String> {
    stored_office_path(control)?
        .map(|path| checked_office_path(&path))
        .transpose()
}

pub fn office_setting_status(control: &Path) -> serde_json::Value {
    match stored_office_path(control) {
        Ok(Some(path)) => {
            let available = checked_office_path(&path).is_ok();
            serde_json::json!({"path":path,"available":available})
        }
        Ok(None) => serde_json::json!({"path":null,"available":null}),
        Err(error) => serde_json::json!({"path":null,"available":false,"error":error}),
    }
}

pub fn set_office_path(control: &Path, value: &str) -> Result<(), String> {
    let value = value.trim();
    if !value.is_empty() {
        checked_office_path(value)?;
    }
    std::fs::create_dir_all(control).map_err(|_| "Cannot create settings folder")?;
    let mut file =
        tempfile::NamedTempFile::new_in(control).map_err(|_| "Cannot write LibreOffice setting")?;
    serde_json::to_writer(&mut file, &serde_json::json!({"path":value}))
        .map_err(|_| "Cannot write LibreOffice setting")?;
    file.flush()
        .map_err(|_| "Cannot write LibreOffice setting")?;
    file.persist(control.join("office-path.json"))
        .map_err(|_| "Cannot save LibreOffice setting")?;
    Ok(())
}

pub fn office_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("WAYPOINT_SOFFICE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(control) = crate::storage_settings::control_dir() {
        match configured_office_path(&control) {
            Ok(Some(path)) => candidates.push(path),
            Ok(None) => {}
            Err(_) => return None,
        }
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
    if let Some(path) = std::env::var_os("PROGRAMW6432") {
        candidates.push(PathBuf::from(path).join("LibreOffice/program/soffice.com"));
    }
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(path).join("Programs/LibreOffice/program/soffice.com"));
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
    docx_to_pdf_with_revision_info(bytes).map(|(pdf, _)| pdf)
}

pub fn docx_to_pdf_with_revision_info(bytes: &[u8]) -> Result<(Vec<u8>, bool), String> {
    office_to_pdf(bytes, false)
}

fn reject_removed_revisions(bytes: &[u8]) -> Result<(), String> {
    use quick_xml::{events::Event, name::ResolveResult, NsReader};
    use std::io::Cursor;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid DOCX archive")?;
    let text_parts = crate::document_extract::docx_text_parts(&mut archive)?;
    let (_, coverage) = crate::docx_notes::extract(&mut archive, &text_parts)?;
    if coverage["unresolved_note_references"].as_u64() != Some(0) {
        return Err("DOCX note references could not be inspected; review the document manually before conversion".into());
    }
    // Wordprocessing content can live in custom-named parts, including notes
    // outside word/. Inspect bounded XML parts rather than guessing locations.
    let mut parts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|_| "Invalid DOCX entry")?;
        let name = entry.name();
        if name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("xml"))
        {
            if !seen.insert(name.to_owned()) {
                return Err("Duplicate DOCX XML part".into());
            }
            parts.push(name.to_owned());
            if parts.len() > 128 {
                return Err(
                    "DOCX has too many XML parts for revision inspection; review manually".into(),
                );
            }
        }
    }
    let rel_name = "word/_rels/document.xml.rels";
    let rel_count = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index_raw(i)
                .is_ok_and(|entry| entry.name() == rel_name)
        })
        .count();
    if rel_count > 1 {
        return Err("Duplicate DOCX relationship part".into());
    }
    if rel_count == 1 {
        let rel_xml = crate::document_extract::read_docx_text_part(&mut archive, rel_name, &mut 0)?;
        for name in crate::docx_notes::note_relationships(&rel_xml)?
            .into_iter()
            .flatten()
        {
            let count = (0..archive.len())
                .filter(|&i| {
                    archive
                        .by_index_raw(i)
                        .is_ok_and(|entry| entry.name() == name)
                })
                .count();
            if count > 1 {
                return Err("Duplicate DOCX note part".into());
            }
            if count == 1 && seen.insert(name.clone()) {
                parts.push(name);
            }
        }
    }
    if parts.len() > 128 {
        return Err("DOCX has too many XML parts for revision inspection; review manually".into());
    }
    let mut total = 0;
    for name in parts {
        let xml = crate::document_extract::read_docx_text_part(&mut archive, &name, &mut total)?;
        let mut reader = NsReader::from_reader(xml.as_slice());
        loop {
            let (namespace, event) = reader
                .read_resolved_event()
                .map_err(|_| "Malformed DOCX XML")?;
            let word = matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == b"http://schemas.openxmlformats.org/wordprocessingml/2006/main" || ns.as_ref() == b"http://purl.oclc.org/ooxml/wordprocessingml/main");
            match event {
                Event::Start(ref e) | Event::Empty(ref e) if word && matches!(e.local_name().as_ref(), b"del" | b"moveFrom") => return Err("DOCX contains removed tracked revisions that may appear in the PDF. Resolve revisions in a document editor before conversion.".into()),
                Event::DocType(_) => return Err("DOCX document type declarations are not supported".into()),
                Event::Eof => break,
                _ => {}
            }
        }
    }
    Ok(())
}

/// Convert UTF-8 text as plain text, including Markdown source rather than
/// interpreting markup. A BOM makes the import encoding explicit to the office
/// converter; the saved original document is not modified.
pub fn text_to_pdf(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.is_empty() || bytes.len() > 10 * 1024 * 1024 {
        return Err("Document size is outside the permitted range".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Text must be UTF-8")?;
    if text.contains('\0') || text.trim_start_matches('\u{feff}').trim().is_empty() {
        return Err("Text is empty or contains binary data".into());
    }
    office_to_pdf(bytes, true).map(|(pdf, _)| pdf)
}

fn office_to_pdf(bytes: &[u8], text: bool) -> Result<(Vec<u8>, bool), String> {
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
    let prepared = if text {
        None
    } else {
        crate::docx_revision::accepted_view(bytes)?
    };
    let conversion_bytes = prepared.as_deref().unwrap_or(bytes);
    if !text {
        reject_removed_revisions(conversion_bytes)?;
    }
    let office = office_path().ok_or(
        "LibreOffice was not found. Install it or configure soffice.com in Settings > Local tools and setup.",
    )?;
    let temp = tempfile::Builder::new()
        .prefix("waypoint-convert-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let source = temp
        .path()
        .join(if text { "source.txt" } else { "source.docx" });
    if text && !conversion_bytes.starts_with(b"\xef\xbb\xbf") {
        use std::io::Write;
        let mut file = std::fs::File::create(&source).map_err(|e| e.to_string())?;
        file.write_all(b"\xef\xbb\xbf").map_err(|e| e.to_string())?;
        file.write_all(conversion_bytes)
            .map_err(|e| e.to_string())?;
    } else {
        std::fs::write(&source, conversion_bytes).map_err(|e| e.to_string())?;
    }
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
    let pdf = read_pdf(&output.join("source.pdf"))?;
    drop(_document_lease);
    if prepared.is_some() {
        let check = crate::convert_task::compare_docx_to_pdf(bytes, &pdf);
        if check["status"] != "matches" {
            return Err("The converted PDF could not be verified against the current DOCX text. Resolve revisions and review the document manually.".into());
        }
    }
    Ok((pdf, prepared.is_some()))
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
    fn custom_office_path_is_persistent_validated_and_clearable() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join("settings");
        let executable = dir.path().join("soffice.com");
        std::fs::write(&executable, b"fictional fixture, never executed").unwrap();
        let value = executable.to_str().unwrap();
        super::set_office_path(&control, value).unwrap();
        assert_eq!(
            super::configured_office_path(&control).unwrap().as_deref(),
            Some(executable.as_path())
        );
        assert!(super::set_office_path(&control, "relative/soffice.com").is_err());
        assert!(super::set_office_path(&control, dir.path().to_str().unwrap()).is_err());
        assert_eq!(
            super::configured_office_path(&control).unwrap().as_deref(),
            Some(executable.as_path())
        );
        std::fs::remove_file(&executable).unwrap();
        assert!(super::configured_office_path(&control).is_err());
        let status = super::office_setting_status(&control);
        assert_eq!(status["path"], value);
        assert_eq!(status["available"], false);
        super::set_office_path(&control, "").unwrap();
        assert_eq!(super::configured_office_path(&control).unwrap(), None);
    }
    #[test]
    fn conversion_detects_removed_revisions_in_document_parts() {
        use std::io::{Cursor, Write};
        let make = |part: &str, marker: &str| {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let names = if part == "word/document.xml" {
                vec![part]
            } else {
                vec!["word/document.xml", part]
            };
            for name in names {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                let content = if name == part { marker } else { "<w:p/>" };
                write!(writer, "<w:root xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{content}</w:root>").unwrap();
            }
            writer.finish().unwrap().into_inner()
        };
        for part in [
            "word/document.xml",
            "word/header1.xml",
            "word/footer1.xml",
            "word/footnotes.xml",
            "word/endnotes.xml",
            "word/custom/notes.xml",
            "word/comments.xml",
            "custom/notes.xml",
        ] {
            for marker in ["<w:del/>", "<w:moveFrom><w:r/></w:moveFrom>"] {
                assert!(
                    super::reject_removed_revisions(&make(part, marker))
                        .unwrap_err()
                        .contains("Resolve revisions"),
                    "{part} {marker}"
                );
            }
        }
        assert!(super::reject_removed_revisions(&make(
            "word/document.xml",
            "<w:ins><w:r/></w:ins>"
        ))
        .is_ok());
    }

    #[test]
    fn revision_inspection_rejects_excessive_xml_parts() {
        use std::io::{Cursor, Write};
        let make = || {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("word/document.xml", options).unwrap();
            writer.write_all(b"<document/>").unwrap();
            for i in 0..128 {
                writer
                    .start_file(format!("custom/{i}.xml"), options)
                    .unwrap();
                writer.write_all(b"<root/>").unwrap();
            }
            writer.finish().unwrap().into_inner()
        };
        assert!(super::reject_removed_revisions(&make())
            .unwrap_err()
            .contains("too many"));
    }

    #[test]
    fn revision_inspection_follows_custom_named_note_parts() {
        use std::io::{Cursor, Write};
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, xml) in [
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Experience</w:t><w:footnoteReference w:id="1"/></w:r></w:p></w:body></w:document>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="../custom/notes.bin"/></Relationships>"#,
            ),
            (
                "custom/notes.bin",
                r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:t>Current</w:t></w:r><w:del><w:r><w:delText>Removed</w:delText></w:r></w:del></w:p></w:footnote></w:footnotes>"#,
            ),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        assert!(super::reject_removed_revisions(&bytes)
            .unwrap_err()
            .contains("Resolve revisions"));
    }

    #[test]
    fn text_conversion_rejects_invalid_or_empty_input_before_starting_office() {
        assert!(super::text_to_pdf(b"\xff").unwrap_err().contains("UTF-8"));
        assert!(super::text_to_pdf(b"text\0binary")
            .unwrap_err()
            .contains("binary"));
        assert!(super::text_to_pdf(b" \n\t").unwrap_err().contains("empty"));
    }

    #[test]
    #[ignore = "uses installed LibreOffice to convert fictional UTF-8 text"]
    fn converts_fictional_unicode_text() {
        let bytes =
            "Fictional CV\nAvery Example\nMünchen · São Paulo\nExperience: accessible interfaces\n"
                .as_bytes();
        let pdf = super::text_to_pdf(bytes).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let text = crate::pdf_extract::extract(&pdf).unwrap();
        assert!(text.contains("Avery Example"));
        assert!(text.contains("München"));
        assert!(text.contains("São Paulo"));
        let page = crate::pdf_extract::render_page_for_review(&pdf, 0).unwrap();
        assert_eq!(page["pages"], 1);
        assert!(page["image"].as_str().unwrap().starts_with("iVBOR"));
    }

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
