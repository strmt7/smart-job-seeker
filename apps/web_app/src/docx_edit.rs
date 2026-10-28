//! Apply one exact, reviewed text replacement within a DOCX paragraph.
//! Text spanning Word runs is supported; the replacement uses the first run's style.
use quick_xml::{events::Event, Reader};
use std::io::{Cursor, Read, Write};

struct Node {
    start: usize,
    end: usize,
    tag_start: usize,
    tag_end: usize,
    text: String,
}
fn parse_blocks(xml: &[u8]) -> Result<Vec<Vec<Node>>, String> {
    let mut reader = Reader::from_reader(xml);
    let mut stack: Vec<Vec<Node>> = Vec::new();
    let mut blocks = Vec::new();
    let mut active: Option<(usize, usize, usize)> = None;
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| "Malformed DOCX body")?;
        let after = reader.buffer_position() as usize;
        match event {
            Event::DocType(_) => return Err("DOCX document types are unsupported".into()),
            Event::Start(ref e) if e.name().as_ref() == b"w:p" => {
                if let Some(parent) = stack.last_mut() {
                    if !parent.is_empty() {
                        blocks.push(std::mem::take(parent));
                    }
                }
                stack.push(Vec::new());
            }
            Event::End(ref e) if e.name().as_ref() == b"w:p" => {
                if active.is_some() {
                    return Err("Invalid DOCX text structure".into());
                }
                let group = stack.pop().ok_or("Malformed DOCX paragraph")?;
                if !group.is_empty() {
                    blocks.push(group);
                }
            }
            Event::Start(ref e) if e.name().as_ref() == b"w:t" && !stack.is_empty() => {
                if active.is_some() {
                    return Err("Nested DOCX text is unsupported".into());
                }
                active = Some((before, after, after));
            }
            Event::End(ref e) if e.name().as_ref() == b"w:t" && active.is_some() => {
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
                if matches!(e.name().as_ref(), b"w:tab" | b"w:br" | b"w:cr")
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
    let matching = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index_raw(i)
                .ok()
                .is_some_and(|f| f.name() == "word/document.xml")
        })
        .count();
    if matching != 1 {
        return Err("DOCX must contain exactly one document body".into());
    }
    let xml = {
        let mut file = archive
            .by_name("word/document.xml")
            .map_err(|_| "DOCX body missing")?;
        if file.size() > 4 * 1024 * 1024 {
            return Err("DOCX body exceeds 4 MiB".into());
        }
        let mut xml = Vec::new();
        file.by_ref()
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut xml)
            .map_err(|_| "Cannot read DOCX body")?;
        if xml.len() > 4 * 1024 * 1024 {
            return Err("DOCX body exceeds 4 MiB".into());
        }
        xml
    };
    let blocks = parse_blocks(&xml)?;
    let mut found = None;
    for (block_index, block) in blocks.iter().enumerate() {
        let text: String = block.iter().map(|n| n.text.as_str()).collect();
        let mut cursor = 0;
        while cursor < text.len() {
            let Some(relative) = text[cursor..].find(before) else {
                break;
            };
            let at = cursor + relative;
            if found.is_some() {
                return Err("Original text appears more than once; select a unique passage".into());
            }
            found = Some((block_index, at));
            cursor = at + text[at..].chars().next().unwrap().len_utf8();
        }
    }
    let (block_index, start) = found.ok_or("Original text was not found within one paragraph")?;
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
        if file.name() == "word/document.xml" {
            drop(file);
            writer
                .start_file(
                    "word/document.xml",
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
    #[test]
    fn replacement_preserves_unedited_parts_and_handles_split_runs() {
        let xml = r#"<w:document xmlns:w="urn:test"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Research </w:t></w:r><w:r><w:t>engineer &amp; scientist</w:t></w:r></w:p></w:body></w:document>"#;
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
    fn ambiguous_or_cross_paragraph_matches_are_rejected() {
        let xml = r#"<w:document xmlns:w="urn:test"><w:body><w:p><w:r><w:t>Repeated</w:t></w:r></w:p><w:p><w:r><w:t>Repeated</w:t></w:r></w:p></w:body></w:document>"#;
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
