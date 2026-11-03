//! Bounded extraction of referenced standard DOCX footnotes and endnotes.
use quick_xml::{events::Event, name::ResolveResult, NsReader};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

const WORD_NAMESPACES: [&[u8]; 2] = [
    b"http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    b"http://purl.oclc.org/ooxml/wordprocessingml/main",
];
const MAX_REFERENCES: usize = 128;
const REL_NS: &[u8] = b"http://schemas.openxmlformats.org/package/2006/relationships";

pub(crate) fn note_relationships(xml: &[u8]) -> Result<[Option<String>; 2], String> {
    let mut reader = NsReader::from_reader(xml);
    let mut result: [Option<String>; 2] = [None, None];
    let mut counts = [0usize; 2];
    let mut depth = 0usize;
    let mut root = false;
    loop {
        let (ns, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX relationships")?;
        let in_ns = matches!(ns, ResolveResult::Bound(value) if value.as_ref() == REL_NS);
        if let Event::Start(e) | Event::Empty(e) = &event {
            if depth == 0 {
                if root || !in_ns || e.local_name().as_ref() != b"Relationships" {
                    return Err("Invalid DOCX relationships root".into());
                }
                root = true;
            } else if depth == 1 && in_ns && e.local_name().as_ref() == b"Relationship" {
                let mut ty = None;
                let mut target = None;
                let mut mode = None;
                for a in e.attributes() {
                    let a = a.map_err(|_| "Invalid DOCX relationship attribute")?;
                    let value = a
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| "Invalid DOCX relationship value")?
                        .into_owned();
                    match a.key.as_ref() {
                        b"Type" => ty = Some(value),
                        b"Target" => target = Some(value),
                        b"TargetMode" => mode = Some(value),
                        _ => {}
                    }
                }
                let index = match ty.as_deref() {
                    Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" | "http://purl.oclc.org/ooxml/officeDocument/relationships/footnotes") => Some(0),
                    Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" | "http://purl.oclc.org/ooxml/officeDocument/relationships/endnotes") => Some(1),
                    _ => None,
                };
                if let Some(i) = index {
                    counts[i] += 1;
                    let base = url::Url::parse("https://waypoint.invalid/word/document.xml")
                        .expect("constant URL");
                    result[i] = target
                        .filter(|s| !s.contains(['\\', '%', ':']) && !s.starts_with("//"))
                        .and_then(|s| base.join(&s).ok())
                        .filter(|u| {
                            u.scheme() == "https"
                                && u.host_str() == Some("waypoint.invalid")
                                && u.query().is_none()
                                && u.fragment().is_none()
                                && u.username().is_empty()
                                && u.password().is_none()
                                && u.port().is_none()
                        })
                        .filter(|_| mode.as_deref().is_none_or(|m| m == "Internal"))
                        .and_then(|u| u.path().strip_prefix('/').map(str::to_owned));
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
    for i in 0..2 {
        if counts[i] != 1 {
            result[i] = None;
        }
    }
    Ok(result)
}

fn word(ns: ResolveResult<'_>) -> bool {
    matches!(ns, ResolveResult::Bound(value) if WORD_NAMESPACES.contains(&value.as_ref()))
}

fn reference_id(
    reader: &mut NsReader<&[u8]>,
    event: &quick_xml::events::BytesStart<'_>,
) -> Option<i64> {
    let mut id = None;
    for attribute in event.attributes() {
        let attribute = attribute.ok()?;
        let (ns, local) = reader.resolver_mut().resolve_attribute(attribute.key);
        if word(ns) && local.as_ref() == b"id" {
            if id.is_some() {
                return None;
            }
            let parsed = std::str::from_utf8(attribute.value.as_ref())
                .ok()?
                .parse::<i64>()
                .ok()?;
            if parsed < 0 {
                return None;
            }
            id = Some(parsed);
        }
    }
    id
}

#[derive(Default)]
struct References {
    footnotes: Vec<Option<i64>>,
    endnotes: Vec<Option<i64>>,
}

fn collect_references(xml: &[u8], references: &mut References) -> Result<(), String> {
    let mut reader = NsReader::from_reader(xml);
    let mut removed = 0usize;
    loop {
        let (ns, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX XML")?;
        let is_word = word(ns);
        if removed > 0 {
            match event {
                Event::Start(_) => removed += 1,
                Event::End(_) => removed -= 1,
                Event::Eof => return Err("Unclosed removed DOCX revision".into()),
                _ => {}
            }
            continue;
        }
        if is_word
            && matches!(&event, Event::Start(e) if matches!(e.local_name().as_ref(), b"del" | b"moveFrom"))
        {
            removed = 1;
            continue;
        }
        match event {
            Event::Start(e) | Event::Empty(e) if is_word => {
                let target = match e.local_name().as_ref() {
                    b"footnoteReference" => Some(&mut references.footnotes),
                    b"endnoteReference" => Some(&mut references.endnotes),
                    _ => None,
                };
                if let Some(target) = target {
                    if target.len() >= MAX_REFERENCES {
                        return Err("DOCX has too many note references; review manually".into());
                    }
                    target.push(reference_id(&mut reader, &e));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// Read only notes whose IDs are referenced, keeping the source XML namespace
/// context. Duplicate IDs are rejected rather than selecting an arbitrary note.
fn extract_note_part(
    xml: &[u8],
    kind: &[u8],
    wanted: &HashSet<i64>,
) -> Result<HashMap<i64, String>, String> {
    let mut reader = NsReader::from_reader(xml);
    let mut found = HashMap::new();
    let mut duplicate = HashSet::new();
    let mut depth = 0usize;
    let mut active: Option<(i64, usize, String)> = None;
    let (mut in_text, mut removed, mut run_depth, mut tabs_depth) = (false, 0usize, 0usize, 0usize);
    let mut invalid = false;
    let mut root_seen = false;
    loop {
        let (ns, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX note XML")?;
        let is_word = word(ns);
        if let Event::Start(e) | Event::Empty(e) = &event {
            if depth == 0 {
                let expected = if kind == b"footnote" {
                    b"footnotes".as_slice()
                } else {
                    b"endnotes".as_slice()
                };
                if root_seen || !is_word || e.local_name().as_ref() != expected {
                    return Err("Invalid DOCX note root".into());
                }
                root_seen = true;
            }
        }
        match &event {
            Event::Start(e) if is_word && e.local_name().as_ref() == kind && depth == 1 => {
                if let Some(id) = reference_id(&mut reader, e).filter(|id| wanted.contains(id)) {
                    active = Some((id, depth, String::new()));
                    in_text = false;
                    removed = 0;
                    run_depth = 0;
                    tabs_depth = 0;
                    invalid = e.attributes().any(|a| match a {
                        Ok(a) => {
                            let (ns, local) = reader.resolver_mut().resolve_attribute(a.key);
                            word(ns) && local.as_ref() == b"type" && a.value.as_ref() != b"normal"
                        }
                        Err(_) => true,
                    });
                }
            }
            Event::Empty(e) if is_word && e.local_name().as_ref() == kind && depth == 1 => {
                if let Some(id) = reference_id(&mut reader, e).filter(|id| wanted.contains(id)) {
                    if found.insert(id, String::new()).is_some() {
                        duplicate.insert(id);
                    }
                }
            }
            _ => {}
        }
        if let Some((_, note_depth, text)) = active.as_mut() {
            if removed > 0 {
                match &event {
                    Event::Start(_) => removed += 1,
                    Event::End(_) => removed -= 1,
                    _ => {}
                }
            } else if is_word
                && matches!(&event, Event::Start(e) if matches!(e.local_name().as_ref(), b"del" | b"moveFrom"))
            {
                removed = 1;
            } else {
                match &event {
                    Event::Start(e) if is_word && e.local_name().as_ref() == b"r" => run_depth += 1,
                    Event::End(e) if is_word && e.local_name().as_ref() == b"r" => {
                        run_depth = run_depth.saturating_sub(1)
                    }
                    Event::Start(e) if is_word && e.local_name().as_ref() == b"tabs" => {
                        tabs_depth += 1
                    }
                    Event::End(e) if is_word && e.local_name().as_ref() == b"tabs" => {
                        tabs_depth = tabs_depth.saturating_sub(1)
                    }
                    Event::Start(e) | Event::Empty(e)
                        if is_word
                            && matches!(
                                e.local_name().as_ref(),
                                b"footnoteReference" | b"endnoteReference"
                            ) =>
                    {
                        invalid = true
                    }
                    Event::Start(e) if is_word && e.local_name().as_ref() == b"t" => in_text = true,
                    Event::End(e) if is_word && e.local_name().as_ref() == b"t" => in_text = false,
                    Event::Text(e) if in_text => {
                        let value = e.decode().map_err(|_| "Invalid DOCX note encoding")?;
                        text.push_str(
                            &quick_xml::escape::unescape(&value)
                                .map_err(|_| "Invalid DOCX note entity")?,
                        );
                    }
                    Event::GeneralRef(e) if in_text => {
                        let name = e.decode().map_err(|_| "Invalid DOCX note entity")?;
                        text.push_str(
                            &quick_xml::escape::unescape(&format!("&{name};"))
                                .map_err(|_| "Unsupported DOCX note entity")?,
                        );
                    }
                    Event::Empty(e) | Event::Start(e)
                        if is_word && matches!(e.local_name().as_ref(), b"br" | b"cr") =>
                    {
                        text.push('\n')
                    }
                    Event::Empty(e) | Event::Start(e)
                        if is_word
                            && e.local_name().as_ref() == b"tab"
                            && run_depth > 0
                            && tabs_depth == 0 =>
                    {
                        text.push('\t')
                    }
                    Event::Empty(e) | Event::Start(e)
                        if is_word
                            && run_depth > 0
                            && e.local_name().as_ref() == b"noBreakHyphen" =>
                    {
                        text.push('\u{2011}')
                    }
                    Event::Empty(e) | Event::Start(e)
                        if is_word && run_depth > 0 && e.local_name().as_ref() == b"softHyphen" =>
                    {
                        text.push('\u{00ad}')
                    }
                    Event::End(e) if is_word && e.local_name().as_ref() == b"p" => text.push('\n'),
                    _ => {}
                }
            }
            if text.len() > 200_000 {
                return Err("Extracted DOCX note text exceeds limit".into());
            }
            if matches!(&event, Event::End(e) if is_word && e.local_name().as_ref() == kind && depth == *note_depth + 1)
            {
                let (id, _, text) = active.take().expect("active note");
                if found
                    .insert(
                        id,
                        if invalid {
                            String::new()
                        } else {
                            text.trim().to_owned()
                        },
                    )
                    .is_some()
                {
                    duplicate.insert(id);
                }
            }
        }
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.checked_sub(1).ok_or("Malformed DOCX note XML")?,
            Event::DocType(_) => return Err("DOCX note declarations are not supported".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if active.is_some() || depth != 0 || !root_seen {
        return Err("Unclosed DOCX note XML".into());
    }
    for id in duplicate {
        found.remove(&id);
    }
    Ok(found)
}

pub(crate) fn extract(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    parts: &[String],
) -> Result<(String, Value), String> {
    let mut refs = References::default();
    let mut total = 0u64;
    for name in parts {
        let xml = crate::document_extract::read_docx_text_part(archive, name, &mut total)?;
        collect_references(&xml, &mut refs)?;
    }
    let mut output = String::new();
    let mut unresolved = 0usize;
    let rel_name = "word/_rels/document.xml.rels";
    let rel_count = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index_raw(i)
                .ok()
                .is_some_and(|entry| entry.name().is_ok_and(|name| name == rel_name))
        })
        .count();
    let targets = if rel_count == 0 || (refs.footnotes.is_empty() && refs.endnotes.is_empty()) {
        [None, None]
    } else if rel_count == 1 {
        note_relationships(&crate::document_extract::read_docx_text_part(
            archive, rel_name, &mut total,
        )?)?
    } else {
        return Err("Duplicate DOCX relationship part".into());
    };
    for (target, kind, ids) in [
        (&targets[0], b"footnote".as_slice(), &refs.footnotes),
        (&targets[1], b"endnote".as_slice(), &refs.endnotes),
    ] {
        if ids.is_empty() {
            continue;
        }
        let wanted: HashSet<_> = ids.iter().flatten().copied().collect();
        let entries = (0..archive.len())
            .filter(|&i| {
                archive.by_index_raw(i).ok().is_some_and(|entry| {
                    entry
                        .name()
                        .is_ok_and(|name| Some(name.as_ref()) == target.as_deref())
                })
            })
            .count();
        let notes = if entries == 0 {
            HashMap::new()
        } else if entries == 1 {
            let xml = crate::document_extract::read_docx_text_part(
                archive,
                target.as_deref().expect("matched target"),
                &mut total,
            )?;
            extract_note_part(&xml, kind, &wanted)?
        } else {
            return Err("Duplicate DOCX note part".into());
        };
        let mut written = HashSet::new();
        for id in ids {
            match id.and_then(|id| {
                notes
                    .get(&id)
                    .filter(|text| !text.is_empty())
                    .map(|text| (id, text))
            }) {
                Some((id, text)) => {
                    if written.insert(id) {
                        if !output.is_empty() {
                            output.push('\n');
                        }
                        output.push_str(text);
                    }
                }
                None => unresolved += 1,
            }
        }
    }
    Ok((
        output,
        json!({"kind":"docx","footnote_references":refs.footnotes.len(),"endnote_references":refs.endnotes.len(),"unresolved_note_references":unresolved}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationship_targets_are_resolved_only_when_unambiguous_and_internal() {
        let xml = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="other/../notes/f.xml"/><Relationship Id="r2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="/custom/end.xml"/></Relationships>"#;
        assert_eq!(
            note_relationships(xml).unwrap(),
            [
                Some("word/notes/f.xml".into()),
                Some("custom/end.xml".into())
            ]
        );
        let duplicate = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="f.xml"/><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="g.xml"/><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="https://example.com/e.xml" TargetMode="External"/></Relationships>"#;
        assert_eq!(note_relationships(duplicate).unwrap(), [None, None]);
        assert!(note_relationships(b"<Relationships/>").is_err());
    }

    #[test]
    fn ambiguous_special_and_nested_notes_are_unresolved() {
        let wanted = [1, 2, 3].into_iter().collect();
        let xml = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"/><w:footnote w:id="1"><w:p><w:r><w:t>Ambiguous</w:t></w:r></w:p></w:footnote><w:footnote w:id="2" w:type="separator"><w:p><w:r><w:t>Special</w:t></w:r></w:p></w:footnote><w:footnote w:id="3"><w:p><w:r><w:t>Nested</w:t><w:endnoteReference w:id="4"/></w:r></w:p></w:footnote></w:footnotes>"#;
        let notes = extract_note_part(xml, b"footnote", &wanted).unwrap();
        assert!(!notes.contains_key(&1));
        assert!(notes[&2].is_empty());
        assert!(notes[&3].is_empty());
        assert!(extract_note_part(b"<wrong/>", b"footnote", &wanted).is_err());
    }

    #[test]
    fn note_text_respects_run_tabs_and_hyphens() {
        let wanted = [1].into_iter().collect();
        let xml = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:pPr><w:tabs><w:tab/></w:tabs></w:pPr><w:r><w:t>Full</w:t><w:noBreakHyphen/><w:t>stack</w:t><w:tab/><w:t>Rust</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
        assert_eq!(
            extract_note_part(xml, b"footnote", &wanted).unwrap()[&1],
            "Full\u{2011}stack\tRust"
        );
    }
}
