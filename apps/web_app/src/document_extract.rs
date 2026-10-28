//! Local bounded extraction for UTF-8 and DOCX body, header, and footer text.
use quick_xml::{events::Event, Reader};
use std::io::{Cursor, Read};

const MAX_XML: u64 = 4 * 1024 * 1024;
const MAX_TEXT: usize = 200_000;

pub fn extract(name: &str, bytes: &[u8]) -> Result<String, String> {
    let suffix = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let text = match suffix.as_str() {
        "txt" | "md" => {
            let text = std::str::from_utf8(bytes).map_err(|_| "Text must be UTF-8")?;
            if text.contains('\0') { return Err("Text contains binary data".into()); }
            text.trim_start_matches('\u{feff}').to_string()
        }
        "docx" => extract_docx(bytes)?,
        "pdf" => crate::pdf_extract::extract(bytes)?,
        _ => return Err("Text extraction supports UTF-8 text, Markdown, DOCX and selectable-text PDF; this file remains stored".into()),
    };
    let text = text.trim();
    if text.is_empty() {
        return Err("No text was found".into());
    }
    if text.len() > MAX_TEXT {
        return Err("Extracted text exceeds 200 KB".into());
    }
    Ok(text.to_owned())
}

pub(crate) fn docx_text_parts(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|_| "Invalid DOCX entry")?;
        let name = file.name();
        let auxiliary = ["word/header", "word/footer"].iter().any(|prefix| {
            name.strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(".xml"))
                .is_some_and(|number| {
                    !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
                })
        });
        if name == "word/document.xml" || auxiliary {
            if !seen.insert(name.to_owned()) {
                return Err("Duplicate DOCX text part".into());
            }
            names.push(name.to_owned());
            if names.len() > 32 {
                return Err("DOCX has too many text parts".into());
            }
        }
    }
    if !seen.contains("word/document.xml") {
        return Err("DOCX body missing".into());
    }
    names.sort_by_key(|name| {
        if name.starts_with("word/header") {
            (0, name.clone())
        } else if name == "word/document.xml" {
            (1, name.clone())
        } else {
            (2, name.clone())
        }
    });
    Ok(names)
}

pub(crate) fn read_docx_text_part(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    total: &mut u64,
) -> Result<Vec<u8>, String> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| "DOCX text part missing")?;
    if file.size() > MAX_XML || total.saturating_add(file.size()) > 8 * 1024 * 1024 {
        return Err("DOCX text parts exceed size limits".into());
    }
    let mut xml = Vec::new();
    file.by_ref()
        .take(MAX_XML + 1)
        .read_to_end(&mut xml)
        .map_err(|_| "Could not read DOCX text part")?;
    if xml.len() as u64 > MAX_XML || total.saturating_add(xml.len() as u64) > 8 * 1024 * 1024 {
        return Err("DOCX text parts exceed size limits".into());
    }
    *total += xml.len() as u64;
    Ok(xml)
}

fn extract_docx(bytes: &[u8]) -> Result<String, String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid DOCX archive")?;
    let names = docx_text_parts(&mut archive)?;
    let mut output = String::new();
    let mut total = 0;
    for name in names {
        let xml = read_docx_text_part(&mut archive, &name, &mut total)?;
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        extract_docx_xml(&xml, &mut output)?;
    }
    Ok(output)
}

fn extract_docx_xml(xml: &[u8], output: &mut String) -> Result<(), String> {
    let mut reader = Reader::from_reader(xml);
    let mut in_text = false;
    loop {
        match reader.read_event().map_err(|_| "Malformed DOCX XML")? {
            Event::DocType(_) => {
                return Err("DOCX document type declarations are not supported".into())
            }
            Event::Start(e) if e.name().as_ref() == b"w:t" => in_text = true,
            Event::End(e) if e.name().as_ref() == b"w:t" => in_text = false,
            Event::Text(e) if in_text => {
                let text = e.decode().map_err(|_| "Invalid DOCX text encoding")?;
                let decoded =
                    quick_xml::escape::unescape(&text).map_err(|_| "Invalid DOCX text entity")?;
                output.push_str(&decoded);
            }
            Event::GeneralRef(e) if in_text => {
                let name = e.decode().map_err(|_| "Invalid DOCX entity")?;
                let entity = format!("&{name};");
                let decoded =
                    quick_xml::escape::unescape(&entity).map_err(|_| "Unsupported DOCX entity")?;
                output.push_str(&decoded);
            }
            Event::Empty(e) if e.name().as_ref() == b"w:tab" => output.push('\t'),
            Event::Empty(e) if e.name().as_ref() == b"w:br" || e.name().as_ref() == b"w:cr" => {
                output.push('\n')
            }
            Event::End(e) if e.name().as_ref() == b"w:p" => output.push('\n'),
            Event::Eof => break,
            _ => (),
        }
        if output.len() > MAX_TEXT {
            return Err("Extracted text exceeds 200 KB".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(xml: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn extracts_text_and_docx_body() {
        assert_eq!(
            extract("cv.txt", "Résumé\nRust".as_bytes()).unwrap(),
            "Résumé\nRust"
        );
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Rust &amp; SQL</w:t><w:tab/><w:t>2024</w:t></w:r></w:p><w:p><w:r><w:t>Engineer</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(
            extract("cv.docx", &archive(xml)).unwrap(),
            "Rust & SQL\t2024\nEngineer"
        );
    }

    #[test]
    fn malformed_or_unsupported_documents_do_not_succeed() {
        assert!(extract("x.pdf", b"%PDF").is_err());
        assert!(extract("x.docx", b"broken").is_err());
        assert!(extract("x.txt", b"\xff").is_err());
        assert!(extract("x.docx", &archive("<!DOCTYPE a><a/>")).is_err());
        assert!(extract("x.txt", &vec![b'a'; MAX_TEXT + 1]).is_err());
    }
}
