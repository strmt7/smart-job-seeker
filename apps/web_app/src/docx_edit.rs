//! Apply one exact, reviewed text replacement within a DOCX body, header or footer paragraph.
//! Text spanning Word runs is supported; the replacement uses the first run's style.
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use std::io::{Cursor, Write};

struct Node {
    start: usize,
    end: usize,
    tag_start: usize,
    tag_end: usize,
    text: String,
}
fn parse_blocks(xml: &[u8]) -> Result<Vec<Vec<Node>>, String> {
    let mut reader = NsReader::from_reader(xml);
    let mut stack: Vec<Vec<Node>> = Vec::new();
    let mut blocks = Vec::new();
    let mut active: Option<(usize, usize, usize)> = None;
    loop {
        let before = reader.buffer_position() as usize;
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX body")?;
        let word = matches!(namespace, ResolveResult::Bound(ns) if
            ns.as_ref() == b"http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            || ns.as_ref() == b"http://purl.oclc.org/ooxml/wordprocessingml/main");
        let after = reader.buffer_position() as usize;
        match event {
            Event::DocType(_) => return Err("DOCX document types are unsupported".into()),
            Event::Start(ref e) if word && e.local_name().as_ref() == b"p" => {
                if let Some(parent) = stack.last_mut() {
                    if !parent.is_empty() {
                        blocks.push(std::mem::take(parent));
                    }
                }
                stack.push(Vec::new());
            }
            Event::End(ref e) if word && e.local_name().as_ref() == b"p" => {
                if active.is_some() {
                    return Err("Invalid DOCX text structure".into());
                }
                let group = stack.pop().ok_or("Malformed DOCX paragraph")?;
                if !group.is_empty() {
                    blocks.push(group);
                }
            }
            Event::Start(ref e) if word && e.local_name().as_ref() == b"t" && !stack.is_empty() => {
                if active.is_some() {
                    return Err("Nested DOCX text is unsupported".into());
                }
                active = Some((before, after, after));
            }
            Event::End(ref e) if word && e.local_name().as_ref() == b"t" && active.is_some() => {
                let (tag_start, tag_end, start) = active.take().ok_or("Invalid DOCX text")?;
                let raw =
                    std::str::from_utf8(&xml[start..before]).map_err(|_| "Invalid DOCX UTF-8")?;
                if raw.contains('<') {
                    return Err("Complex DOCX text is unsupported".into());
                }
                let text = quick_xml::escape::unescape(raw)
                    .map_err(|_| "Unsupported DOCX text entity")?
                    .into_owned();
                stack
                    .last_mut()
                    .ok_or("Text outside paragraph")?
                    .push(Node {
                        start,
                        end: before,
                        tag_start,
                        tag_end,
                        text,
                    });
            }
            Event::Empty(ref e)
                if word
                    && matches!(e.local_name().as_ref(), b"tab" | b"br" | b"cr")
                    && !stack.is_empty() =>
            {
                let current = stack.last_mut().unwrap();
                if !current.is_empty() {
                    blocks.push(std::mem::take(current));
                }
            }
            Event::Eof => break,
            _ => (),
        }
    }
    if active.is_some() || !stack.is_empty() {
        return Err("Unclosed DOCX text or paragraph".into());
    }
    Ok(blocks)
}

pub fn replace(docx: &[u8], before: &str, after: &str) -> Result<Vec<u8>, String> {
    if before.is_empty()
        || before.len() > 1000
        || after.len() > 4000
        || before.chars().any(char::is_control)
        || after.chars().any(char::is_control)
    {
        return Err(
            "Use a nonempty single-line original and replacement within the size limits".into(),
        );
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(docx)).map_err(|_| "Invalid DOCX archive")?;
    let parts = crate::document_extract::docx_text_parts(&mut archive)?;
    let mut total = 0;
    let mut found = None;
    for name in parts {
        let xml = crate::document_extract::read_docx_text_part(&mut archive, &name, &mut total)?;
        let blocks = parse_blocks(&xml)?;
        for (block_index, block) in blocks.iter().enumerate() {
            let text: String = block.iter().map(|node| node.text.as_str()).collect();
            let mut cursor = 0;
            while cursor < text.len() {
                let Some(relative) = text[cursor..].find(before) else {
                    break;
                };
                let at = cursor + relative;
                if found.is_some() {
                    return Err(
                        "Original text appears more than once; select a unique passage".into(),
                    );
                }
                found = Some((name.clone(), xml.clone(), block_index, at));
                cursor = at + text[at..].chars().next().unwrap().len_utf8();
            }
        }
    }
    let (part_name, xml, block_index, start) =
        found.ok_or("Original text was not found within one paragraph")?;
    let blocks = parse_blocks(&xml)?;
    let end = start + before.len();
    let mut patches: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut position = 0;
    let mut inserted = false;
    for node in &blocks[block_index] {
        let next = position + node.text.len();
        if start < next && end > position {
            let local_start = start.saturating_sub(position);
            let local_end = (end - position).min(node.text.len());
            let mut replacement = String::new();
            replacement.push_str(&node.text[..local_start]);
            if !inserted {
                replacement.push_str(after);
                inserted = true;
            }
            replacement.push_str(&node.text[local_end..]);
            let tag = std::str::from_utf8(&xml[node.tag_start..node.tag_end])
                .map_err(|_| "Invalid DOCX tag")?;
            if (replacement.starts_with(' ') || replacement.ends_with(' '))
                && !tag.contains("xml:space")
            {
                let mut updated = tag[..tag.len() - 1].to_owned();
                updated.push_str(" xml:space=\"preserve\">");
                patches.push((node.tag_start, node.tag_end, updated.into_bytes()));
            } else if (replacement.starts_with(' ') || replacement.ends_with(' '))
                && tag.contains("xml:space")
                && !(tag.contains("xml:space=\"preserve\"") || tag.contains("xml:space='preserve'"))
            {
                return Err("DOCX whitespace policy needs manual editing".into());
            }
            patches.push((
                node.start,
                node.end,
                quick_xml::escape::escape(&replacement).as_bytes().to_vec(),
            ));
        }
        position = next;
    }
    patches.sort_by_key(|p| p.0);
    let mut out = Vec::with_capacity(xml.len() + after.len());
    let mut cursor = 0;
    for (a, b, data) in patches {
        if a < cursor {
            return Err("Overlapping DOCX edits".into());
        }
        out.extend_from_slice(&xml[cursor..a]);
        out.extend_from_slice(&data);
        cursor = b;
    }
    out.extend_from_slice(&xml[cursor..]);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|_| "Invalid DOCX entry")?;
        if file.name() == part_name {
            drop(file);
            writer
                .start_file(
                    &part_name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .map_err(|_| "Cannot write DOCX body")?;
            writer
                .write_all(&out)
                .map_err(|_| "Cannot write DOCX body")?;
        } else {
            writer
                .raw_copy_file(file)
                .map_err(|_| "Cannot preserve DOCX entry")?;
        }
    }
    let result = writer
        .finish()
        .map_err(|_| "Cannot finish DOCX")?
        .into_inner();
    if result.len() > 10 * 1024 * 1024 {
        return Err("Edited DOCX exceeds 10 MiB".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    #[test]
    fn replacement_preserves_unedited_parts_and_handles_split_runs() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Research </w:t></w:r><w:r><w:t>engineer &amp; scientist</w:t></w:r></w:p></w:body></w:document>"#;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
        writer
            .start_file("word/styles.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"styles unchanged").unwrap();
        let doc = writer.finish().unwrap().into_inner();
        let edited = replace(&doc, "Research engineer", "Applied researcher").unwrap();
        let text = crate::document_extract::extract("x.docx", &edited).unwrap();
        assert_eq!(text, "Applied researcher & scientist");
        let mut zip = zip::ZipArchive::new(Cursor::new(edited)).unwrap();
        let mut style = String::new();
        zip.by_name("word/styles.xml")
            .unwrap()
            .read_to_string(&mut style)
            .unwrap();
        assert_eq!(style, "styles unchanged");
        assert!(replace(&doc, "missing", "new").is_err());
        assert!(replace(&doc, "engineer", "a\nb").is_err());
    }

    #[test]
    fn edits_header_or_footer_and_rejects_cross_part_ambiguity() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in [
            ("word/document.xml", "Body sentence"),
            ("word/header1.xml", "Header name"),
            ("word/footer1.xml", "Footer contact"),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            write!(writer, "<w:p xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:r><w:t>{text}</w:t></w:r></w:p>").unwrap();
        }
        let doc = writer.finish().unwrap().into_inner();
        let extracted = crate::document_extract::extract("sample.docx", &doc).unwrap();
        assert_eq!(extracted, "Header name\nBody sentence\nFooter contact");
        let edited = replace(&doc, "Header name", "New header").unwrap();
        let edited = replace(&edited, "Footer contact", "New contact").unwrap();
        assert_eq!(
            crate::document_extract::extract("sample.docx", &edited).unwrap(),
            "New header\nBody sentence\nNew contact"
        );
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["word/document.xml", "word/header1.xml"] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(b"<w:p xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:r><w:t>shared phrase</w:t></w:r></w:p>")
                .unwrap();
        }
        let ambiguous = writer.finish().unwrap().into_inner();
        assert!(replace(&ambiguous, "shared phrase", "replacement")
            .unwrap_err()
            .contains("more than once"));
    }
    #[test]
    fn ambiguous_or_cross_paragraph_matches_are_rejected() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Repeated</w:t></w:r></w:p><w:p><w:r><w:t>Repeated</w:t></w:r></w:p></w:body></w:document>"#;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
        let doc = writer.finish().unwrap().into_inner();
        assert!(replace(&doc, "Repeated", "Changed").is_err());
        assert!(replace(&doc, "RepeatedRepeated", "Changed").is_err());
    }

    #[test]
    fn edits_alternate_namespace_without_changing_other_parts() {
        let xml = r#"<d:document xmlns:d="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><d:body><d:p><d:r><d:t>Data </d:t></d:r><d:r><d:t>analyst</d:t></d:r></d:p></d:body></d:document>"#;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
        let doc = writer.finish().unwrap().into_inner();
        let edited = replace(&doc, "Data analyst", "Research analyst").unwrap();
        assert_eq!(
            crate::document_extract::extract("cv.docx", &edited).unwrap(),
            "Research analyst"
        );
    }

    #[test]
    #[ignore = "converts a fictional DOCX header and footer with local LibreOffice"]
    fn edited_header_and_footer_convert_to_pdf() {
        let source = std::env::var("WAYPOINT_HEADER_FIXTURE").expect("fixture path");
        let output = std::env::var("WAYPOINT_HEADER_PDF").expect("new PDF output path");
        let original = std::fs::read(source).unwrap();
        let edited = replace(&original, "Synthetic Header Name", "Fictional Header Name").unwrap();
        let edited = replace(
            &edited,
            "Synthetic Footer Contact",
            "Fictional Footer Contact",
        )
        .unwrap();
        let pdf = crate::document_convert::docx_to_pdf(&edited).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .unwrap();
        file.write_all(&pdf).unwrap();
    }
    #[test]
    #[ignore = "converts a fictional DOCX with local LibreOffice"]
    fn edited_fictional_template_converts_to_pdf() {
        let source = std::env::var("WAYPOINT_EDIT_FIXTURE").expect("fixture path");
        let output = std::env::var("WAYPOINT_EDIT_PDF").expect("new PDF output path");
        let docx = std::fs::read(source).unwrap();
        let edited = replace(
            &docx,
            "Synthetic experience and qualifications.",
            "Fictional research and qualifications.",
        )
        .unwrap();
        let text = crate::document_extract::extract("edited.docx", &edited).unwrap();
        assert!(text.contains("Fictional research and qualifications."));
        let pdf = crate::document_convert::docx_to_pdf(&edited).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .unwrap();
        file.write_all(&pdf).unwrap();
    }
}
