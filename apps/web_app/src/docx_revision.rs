//! Prepare a temporary accepted-text view for narrowly supported revisions.
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use std::io::{Cursor, Write};

const UNSUPPORTED: &str = "DOCX revisions need manual resolution before PDF conversion";

fn remove_revisions(xml: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut reader = NsReader::from_reader(xml);
    let mut stack: Vec<(bool, Vec<u8>)> = Vec::new();
    let mut removed_depth = 0usize;
    let mut removed_start = 0usize;
    let mut spans = Vec::new();
    loop {
        let before = reader.buffer_position() as usize;
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX XML")?;
        let word = matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref().as_bytes() == b"http://schemas.openxmlformats.org/wordprocessingml/2006/main" || ns.as_ref().as_bytes() == b"http://purl.oclc.org/ooxml/wordprocessingml/main");
        let after = reader.buffer_position() as usize;
        if removed_depth > 0 {
            match event {
                Event::Start(_) => removed_depth += 1,
                Event::End(_) => {
                    removed_depth -= 1;
                    if removed_depth == 0 {
                        spans.push((removed_start, after));
                    }
                }
                Event::Eof => return Err("Unclosed DOCX revision".into()),
                _ => {}
            }
            continue;
        }
        match event {
            Event::DocType(_) => {
                return Err("DOCX document type declarations are not supported".into())
            }
            Event::Start(ref e) | Event::Empty(ref e)
                if word
                    && matches!(
                        e.local_name().as_ref().as_bytes(),
                        b"moveFromRangeStart"
                            | b"moveFromRangeEnd"
                            | b"delRangeStart"
                            | b"delRangeEnd"
                    ) =>
            {
                return Err(UNSUPPORTED.into())
            }
            Event::Start(ref e)
                if word && matches!(e.local_name().as_ref().as_bytes(), b"del" | b"moveFrom") =>
            {
                if !stack.last().is_some_and(|(word, name)| {
                    *word && matches!(name.as_slice(), b"p" | b"hyperlink")
                }) {
                    return Err(UNSUPPORTED.into());
                }
                removed_start = before;
                removed_depth = 1;
            }
            Event::Empty(ref e)
                if word && matches!(e.local_name().as_ref().as_bytes(), b"del" | b"moveFrom") =>
            {
                return Err(UNSUPPORTED.into())
            }
            Event::Start(ref e) => {
                if stack.len() >= 256 {
                    return Err("DOCX structure is too deeply nested".into());
                }
                stack.push((word, e.local_name().as_ref().as_bytes().to_vec()));
            }
            Event::End(_) => {
                stack.pop().ok_or("Malformed DOCX structure")?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("Unclosed DOCX structure".into());
    }
    if spans.is_empty() {
        return Ok(None);
    }
    let mut output = Vec::with_capacity(xml.len());
    let mut cursor = 0;
    for (start, end) in spans {
        output.extend_from_slice(&xml[cursor..start]);
        cursor = end;
    }
    output.extend_from_slice(&xml[cursor..]);
    Ok(Some(output))
}

/// Never modifies the source. None means no rewrite was needed.
pub fn accepted_view(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("DOCX exceeds 10 MiB".into());
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid DOCX archive")?;
    let mut parts = crate::document_extract::docx_text_parts(&mut archive)?;
    for name in ["word/footnotes.xml", "word/endnotes.xml"] {
        let count = (0..archive.len())
            .filter(|&i| {
                archive
                    .by_index_raw(i)
                    .is_ok_and(|file| file.name().is_ok_and(|entry_name| entry_name == name))
            })
            .count();
        if count > 1 {
            return Err("Duplicate DOCX note part".into());
        }
        if count == 1 {
            parts.push(name.into());
        }
    }
    let mut total = 0;
    let mut changes = std::collections::HashMap::new();
    for part in parts {
        let xml = crate::document_extract::read_docx_text_part(&mut archive, &part, &mut total)?;
        if let Some(revised) = remove_revisions(&xml)? {
            changes.insert(part, revised);
        }
    }
    if changes.is_empty() {
        return Ok(None);
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|_| "Invalid DOCX entry")?;
        let name = file
            .name()
            .map_err(|_| "Invalid DOCX entry name")?
            .into_owned();
        if let Some(data) = changes.get(&name) {
            drop(file);
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .map_err(|_| "Could not write DOCX part")?;
            writer
                .write_all(data)
                .map_err(|_| "Could not write DOCX part")?;
        } else {
            writer
                .raw_copy_file(file)
                .map_err(|_| "Could not preserve DOCX part")?;
        }
    }
    let output = writer
        .finish()
        .map_err(|_| "Could not finish DOCX")?
        .into_inner();
    if output.len() > 10 * 1024 * 1024 {
        return Err("Prepared DOCX exceeds 10 MiB".into());
    }
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_view_cleans_linked_custom_header() {
        use std::io::{Read, Write};
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(br#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:r><w:t>Body</w:t></w:r></w:p>"#).unwrap();
        zip.start_file("word/custom/header.xml", options).unwrap();
        zip.write_all(br#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:r><w:t>Current</w:t></w:r><w:del><w:r><w:delText>Removed</w:delText></w:r></w:del></w:p>"#).unwrap();
        zip.start_file("word/_rels/document.xml.rels", options)
            .unwrap();
        zip.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="custom/header.xml"/></Relationships>"#).unwrap();
        let source = zip.finish().unwrap().into_inner();
        let revised = accepted_view(&source).unwrap().unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(revised)).unwrap();
        let mut header = String::new();
        archive
            .by_name("word/custom/header.xml")
            .unwrap()
            .read_to_string(&mut header)
            .unwrap();
        assert!(header.contains("Current"));
        assert!(!header.contains("Removed"));
    }

    #[test]
    fn removes_only_supported_inline_revisions() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Current</w:t></w:r><w:del><w:r><w:delText>Removed</w:delText></w:r></w:del><w:moveFrom><w:r><w:t>Moved</w:t></w:r></w:moveFrom></w:p></w:document>"#;
        let result = String::from_utf8(remove_revisions(xml).unwrap().unwrap()).unwrap();
        assert!(result.contains("Current"));
        assert!(!result.contains("Removed") && !result.contains("Moved"));
        let property = br#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:pPr><w:rPr><w:del/></w:rPr></w:pPr></w:p>"#;
        assert!(remove_revisions(property).is_err());
        let range = br#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:moveFromRangeStart w:id="1"/></w:p>"#;
        assert!(remove_revisions(range).is_err());
        assert!(remove_revisions(br#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:ins/></w:p>"#).unwrap().is_none());
    }
}
