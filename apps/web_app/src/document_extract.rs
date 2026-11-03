//! Local bounded extraction for UTF-8 and DOCX body, header, and footer text.
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use std::io::{Cursor, Read};

const MAX_XML: u64 = 4 * 1024 * 1024;
const MAX_TEXT: usize = 200_000;

/// Return partial selectable text for explicit user review, with page coverage.
pub fn extract_for_review(
    name: &str,
    bytes: &[u8],
) -> Result<(String, Option<serde_json::Value>), String> {
    if name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("pdf"))
    {
        let (text, coverage) = crate::pdf_extract::extract_with_coverage(bytes)?;
        Ok((text, Some(coverage)))
    } else if name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("docx"))
    {
        let (text, coverage) = extract_docx(bytes)?;
        Ok((validate_text(text)?, Some(coverage)))
    } else {
        Ok((extract(name, bytes)?, None))
    }
}

/// Model suggestions are advisory and carry coverage to the user.
pub fn extract_for_model(
    name: &str,
    bytes: &[u8],
) -> Result<(String, Option<serde_json::Value>), String> {
    extract_for_review(name, bytes)
}

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
        "docx" => extract_docx(bytes)?.0,
        "pdf" => crate::pdf_extract::extract(bytes)?,
        _ => return Err("Text extraction supports UTF-8 text, Markdown, DOCX and selectable-text PDF; this file remains stored".into()),
    };
    validate_text(text)
}

fn validate_text(text: String) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("No text was found".into());
    }
    if text.len() > MAX_TEXT {
        return Err("Extracted text exceeds 200 KB".into());
    }
    Ok(text.to_owned())
}

// Header and footer part names are determined by package relationships; they
// need not follow Word's usual header1.xml/footer1.xml naming convention.
pub(crate) fn related_header_footer_parts(xml: &[u8]) -> Result<Vec<(bool, String)>, String> {
    const REL_NS: &[u8] = b"http://schemas.openxmlformats.org/package/2006/relationships";
    let mut reader = NsReader::from_reader(xml);
    let mut depth = 0usize;
    let mut root = false;
    let mut parts = Vec::new();
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX relationships")?;
        let in_ns = matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == REL_NS);
        if let Event::Start(e) | Event::Empty(e) = &event {
            if depth == 0 {
                if root || !in_ns || e.local_name().as_ref() != b"Relationships" {
                    return Err("Invalid DOCX relationships root".into());
                }
                root = true;
            } else if depth == 1 && in_ns && e.local_name().as_ref() == b"Relationship" {
                let (mut ty, mut target, mut mode) = (None, None, None);
                for attr in e.attributes() {
                    let attr = attr.map_err(|_| "Invalid DOCX relationship attribute")?;
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| "Invalid DOCX relationship value")?
                        .into_owned();
                    match attr.key.as_ref() {
                        b"Type" => ty = Some(value),
                        b"Target" => target = Some(value),
                        b"TargetMode" => mode = Some(value),
                        _ => {}
                    }
                }
                let header = match ty.as_deref() {
                    Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" | "http://purl.oclc.org/ooxml/officeDocument/relationships/header") => Some(true),
                    Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" | "http://purl.oclc.org/ooxml/officeDocument/relationships/footer") => Some(false),
                    _ => None,
                };
                if let Some(header) = header {
                    let base = url::Url::parse("https://waypoint.invalid/word/document.xml")
                        .expect("constant URL");
                    let part = target
                        .filter(|value| {
                            !value.contains(['\\', '%', ':']) && !value.starts_with("//")
                        })
                        .and_then(|value| base.join(&value).ok())
                        .filter(|value| {
                            value.scheme() == "https"
                                && value.host_str() == Some("waypoint.invalid")
                                && value.query().is_none()
                                && value.fragment().is_none()
                                && value.username().is_empty()
                                && value.password().is_none()
                                && value.port().is_none()
                        })
                        .filter(|_| mode.as_deref().is_none_or(|value| value == "Internal"))
                        .and_then(|value| value.path().strip_prefix('/').map(str::to_owned))
                        .filter(|value| value.ends_with(".xml"))
                        .ok_or("Unsupported DOCX header or footer relationship")?;
                    if !parts.iter().any(|(_, existing)| existing == &part) {
                        parts.push((header, part));
                    }
                    if parts.len() > 31 {
                        return Err("DOCX has too many text parts".into());
                    }
                }
            }
        }
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.checked_sub(1).ok_or("Malformed DOCX relationships")?,
            Event::DocType(_) => {
                return Err("DOCX relationship declarations are not supported".into())
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root || depth != 0 {
        return Err("Unclosed DOCX relationships".into());
    }
    Ok(parts)
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
        let name = file
            .name()
            .map_err(|_| "Invalid DOCX entry name")?
            .into_owned();
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

fn extract_docx(bytes: &[u8]) -> Result<(String, serde_json::Value), String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid DOCX archive")?;
    let mut names = docx_text_parts(&mut archive)?;
    let mut output = String::new();
    let mut total = 0;
    let rel_name = "word/_rels/document.xml.rels";
    let rel_count = (0..archive.len())
        .filter(|&index| {
            archive
                .by_index_raw(index)
                .ok()
                .and_then(|entry| entry.name().ok().map(|name| name == rel_name))
                .unwrap_or(false)
        })
        .count();
    if rel_count > 1 {
        return Err("Duplicate DOCX relationships part".into());
    }
    if rel_count == 1 {
        let relationships = read_docx_text_part(&mut archive, rel_name, &mut total)?;
        for (header, part) in related_header_footer_parts(&relationships)? {
            let count = (0..archive.len())
                .filter(|&index| {
                    archive
                        .by_index_raw(index)
                        .ok()
                        .and_then(|entry| entry.name().ok().map(|name| name == part))
                        .unwrap_or(false)
                })
                .count();
            if count != 1 {
                return Err("DOCX header or footer part is missing or duplicated".into());
            }
            if !names.contains(&part) {
                if names.len() >= 32 {
                    return Err("DOCX has too many text parts".into());
                }
                let index = if header { 0 } else { names.len() };
                names.insert(index, part);
            }
        }
    }
    for name in &names {
        let xml = read_docx_text_part(&mut archive, name, &mut total)?;
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        extract_docx_xml(&xml, &mut output)?;
    }
    let (notes, coverage) = crate::docx_notes::extract(&mut archive, &names)?;
    if !notes.is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&notes);
    }
    Ok((output, coverage))
}

fn extract_docx_xml(xml: &[u8], output: &mut String) -> Result<(), String> {
    let mut reader = NsReader::from_reader(xml);
    let mut in_text = false;
    let mut run_depth = 0usize;
    let mut tab_stops_depth = 0usize;
    let mut removed_revision_depth = 0usize;
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX XML")?;
        let word = matches!(namespace, ResolveResult::Bound(ns) if
            ns.as_ref() == b"http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            || ns.as_ref() == b"http://purl.oclc.org/ooxml/wordprocessingml/main");
        // Use the accepted-change view for text: deleted and moved-from
        // content is not a current qualification; insertions and moved-to
        // content remain. Revisions still need review in the readability report.
        if matches!(event, Event::DocType(_)) {
            return Err("DOCX document type declarations are not supported".into());
        }
        if removed_revision_depth > 0 {
            match event {
                Event::Start(_) => removed_revision_depth += 1,
                Event::End(_) => removed_revision_depth -= 1,
                Event::Eof => return Err("Unclosed removed DOCX revision".into()),
                _ => {}
            }
            continue;
        }
        if word
            && matches!(&event, Event::Start(e) if matches!(e.local_name().as_ref(), b"del" | b"moveFrom"))
        {
            removed_revision_depth = 1;
            continue;
        }
        match event {
            Event::Start(e) if word && e.local_name().as_ref() == b"r" => run_depth += 1,
            Event::End(e) if word && e.local_name().as_ref() == b"r" => {
                run_depth = run_depth.saturating_sub(1)
            }
            Event::Start(e) if word && e.local_name().as_ref() == b"tabs" => tab_stops_depth += 1,
            Event::End(e) if word && e.local_name().as_ref() == b"tabs" => {
                tab_stops_depth = tab_stops_depth.saturating_sub(1)
            }
            Event::Start(e) if word && e.local_name().as_ref() == b"t" => in_text = true,
            Event::End(e) if word && e.local_name().as_ref() == b"t" => in_text = false,
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
            Event::Empty(e) | Event::Start(e)
                if word
                    && run_depth > 0
                    && tab_stops_depth == 0
                    && e.local_name().as_ref() == b"tab" =>
            {
                output.push('\t')
            }
            // These are characters in a Word text run, not formatting-only
            // tags; dropping them joins words and changes qualifications.
            Event::Empty(e) | Event::Start(e)
                if word && e.local_name().as_ref() == b"noBreakHyphen" =>
            {
                output.push('\u{2011}')
            }
            Event::Empty(e) | Event::Start(e)
                if word && e.local_name().as_ref() == b"softHyphen" =>
            {
                output.push('\u{00ad}')
            }
            Event::Empty(e) | Event::Start(e)
                if word
                    && (e.local_name().as_ref() == b"br" || e.local_name().as_ref() == b"cr") =>
            {
                output.push('\n')
            }
            Event::End(e) if word && e.local_name().as_ref() == b"p" => output.push('\n'),
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
    fn extracts_relationship_named_header_and_footer_parts() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let part = |text: &str| {
            format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:document>"#
            )
        };
        for (name, text) in [
            ("word/document.xml", "Body"),
            ("word/custom/top.xml", "Header"),
            ("word/custom/bottom.xml", "Footer"),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(part(text).as_bytes()).unwrap();
        }
        writer
            .start_file("word/_rels/document.xml.rels", options)
            .unwrap();
        writer.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="h" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="custom/top.xml"/><Relationship Id="f" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="custom/bottom.xml"/></Relationships>"#).unwrap();
        let text = extract("cv.docx", &writer.finish().unwrap().into_inner()).unwrap();
        assert_eq!(text, "Header\nBody\nFooter");
    }

    #[test]
    fn rejects_external_header_relationships() {
        let relationships = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="https://example.invalid/header.xml" TargetMode="External"/></Relationships>"#;
        assert!(related_header_footer_parts(relationships).is_err());
    }

    #[test]
    fn reports_missing_relationship_target() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("word/document.xml", options).unwrap();
        writer.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Body</w:t></w:r></w:p></w:document>"#).unwrap();
        writer
            .start_file("word/_rels/document.xml.rels", options)
            .unwrap();
        writer.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="custom/missing.xml"/></Relationships>"#).unwrap();
        let error = extract("cv.docx", &writer.finish().unwrap().into_inner()).unwrap_err();
        assert!(error.contains("missing or duplicated"));
    }

    #[test]
    fn docx_review_reports_note_references_not_in_extracted_text() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:x="urn:other"><w:body><w:p><w:r><w:t>Fictional qualification</w:t><w:footnoteReference w:id="1"/><w:endnoteReference w:id="2"/><x:footnoteReference/></w:r></w:p></w:body></w:document>"#;
        let (text, coverage) = extract_for_review("cv.docx", &archive(xml)).unwrap();
        assert_eq!(text, "Fictional qualification");
        let coverage = coverage.unwrap();
        assert_eq!(coverage["kind"], "docx");
        assert_eq!(coverage["footnote_references"], 1);
        assert_eq!(coverage["endnote_references"], 1);
        assert_eq!(coverage["unresolved_note_references"], 2);
    }

    #[test]
    fn extracts_only_referenced_current_notes_and_reports_ambiguous_ids() {
        fn package(footnotes: &str) -> Vec<u8> {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("word/document.xml", options).unwrap();
            writer.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Main text</w:t><w:footnoteReference w:id="1"/><w:footnoteReference w:id="1"/><w:endnoteReference w:id="2"/></w:r><w:del><w:r><w:footnoteReference w:id="3"/></w:r></w:del></w:p></w:body></w:document>"#).unwrap();
            writer
                .start_file("word/_rels/document.xml.rels", options)
                .unwrap();
            writer.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="notes/custom.xml"/><Relationship Id="r2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/></Relationships>"#).unwrap();
            writer.start_file("word/notes/custom.xml", options).unwrap();
            writer.write_all(footnotes.as_bytes()).unwrap();
            writer.start_file("word/footnotes.xml", options).unwrap();
            writer.write_all(br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:t>Unlinked decoy</w:t></w:r></w:p></w:footnote></w:footnotes>"#).unwrap();
            writer.start_file("word/endnotes.xml", options).unwrap();
            writer.write_all(br#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="2"><w:p><w:r><w:t>Endnote qualification</w:t></w:r></w:p></w:endnote></w:endnotes>"#).unwrap();
            writer.finish().unwrap().into_inner()
        }
        let notes = r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:t>Certified</w:t></w:r><w:del><w:r><w:delText>Deleted</w:delText></w:r></w:del></w:p></w:footnote><w:footnote w:id="3"><w:p><w:r><w:t>Deleted reference</w:t></w:r></w:p></w:footnote><w:footnote w:id="4"><w:p><w:r><w:t>Unreferenced</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
        let (text, coverage) = extract_for_review("cv.docx", &package(notes)).unwrap();
        let coverage = coverage.unwrap();
        assert_eq!(text, "Main text\nCertified\nEndnote qualification");
        assert_eq!(coverage["footnote_references"], 2);
        assert_eq!(coverage["endnote_references"], 1);
        assert_eq!(coverage["unresolved_note_references"], 0);
        let duplicate = r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:r><w:t>One</w:t></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>Two</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
        let (text, coverage) = extract_for_review("cv.docx", &package(duplicate)).unwrap();
        assert!(!text.contains("One") && !text.contains("Two"));
        assert_eq!(coverage.unwrap()["unresolved_note_references"], 2);
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
    fn extracts_word_text_with_alternate_and_default_namespaces() {
        let xml = r#"<d:document xmlns:d="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><d:body><d:p><d:r><d:t>Skills</d:t><d:tab/><d:t>Rust</d:t></d:r></d:p><p xmlns="http://purl.oclc.org/ooxml/wordprocessingml/main"><r><t>Engineer</t></r></p></d:body></d:document>"#;
        assert_eq!(
            extract("cv.docx", &archive(xml)).unwrap(),
            "Skills\tRust\nEngineer"
        );
    }

    #[test]
    fn uses_accepted_view_for_deleted_and_moved_text() {
        let xml = r#"<d:document xmlns:d="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><d:body><d:p><d:del><d:r><d:t>Removed skill</d:t></d:r></d:del><d:moveFrom><d:r><d:t>Old location</d:t></d:r></d:moveFrom><d:ins><d:r><d:t>Current skill</d:t></d:r></d:ins></d:p><d:p><d:moveTo><d:r><d:t>Current location</d:t></d:r></d:moveTo></d:p></d:body></d:document>"#;
        assert_eq!(
            extract("cv.docx", &archive(xml)).unwrap(),
            "Current skill\nCurrent location"
        );
        let unclosed = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:del><w:r>"#;
        assert!(extract("cv.docx", &archive(unclosed)).is_err());
    }

    #[test]
    fn preserves_word_run_hyphens_in_extracted_qualifications() {
        let xml = r#"<d:document xmlns:d="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><d:body><d:p><d:r><d:t>state</d:t><d:noBreakHyphen/><d:t>of</d:t><d:softHyphen/><d:t>the art</d:t></d:r></d:p></d:body></d:document>"#;
        assert_eq!(
            extract("cv.docx", &archive(xml)).unwrap(),
            "state\u{2011}of\u{00ad}the art"
        );
    }

    #[test]
    fn extracts_paired_and_empty_word_break_elements_equivalently() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:other="urn:other"><w:body><w:p><w:r><w:t>Rust</w:t><w:tab></w:tab><w:t>SQL</w:t><w:br></w:br><w:t>full</w:t><w:noBreakHyphen></w:noBreakHyphen><w:t>stack</w:t><w:softHyphen></w:softHyphen><w:t>work</w:t><w:cr></w:cr><w:t>Done</w:t><other:tab/></w:r></w:p></w:body></w:document>"#;
        assert_eq!(
            extract("cv.docx", &archive(xml)).unwrap(),
            "Rust\tSQL\nfull\u{2011}stack\u{00ad}work\nDone"
        );
    }

    #[test]
    fn paragraph_tab_stop_settings_do_not_become_extracted_tabs() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:r><w:t>Python</w:t><w:tab/><w:t>SQL</w:t></w:r></w:p><w:p><w:pPr><w:tabs><w:tab w:val="right" w:pos="1440"/></w:tabs></w:pPr><w:r><w:t>Data analyst</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(
            extract("cv.docx", &archive(xml)).unwrap(),
            "Python\tSQL\nData analyst"
        );
    }

    #[test]
    fn nested_word_runs_keep_real_tabs_but_ignore_tab_stops() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Lead</w:t><w:txbxContent><w:p><w:pPr><w:tabs><w:tab w:val="left"/></w:tabs></w:pPr><w:r><w:t>Inner</w:t></w:r></w:p></w:txbxContent><w:tab/><w:t>After</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(
            extract("cv.docx", &archive(xml)).unwrap(),
            "LeadInner\n\tAfter"
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
