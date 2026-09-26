//! T024 — exporters. DOCX via minimal OOXML (std-only zip writer below);
//! PDF via deterministic Typst source emission. Typst rendering itself is
//! an external dependency to be qualified in G6 packaging; we emit and
//! round-trip-test the source now.

use crate::tree::{DocNode, DocNodeKind, SemanticDocument};

/// Flatten the tree to ordered block/line items for renderers.
pub fn render_lines(doc: &SemanticDocument) -> Vec<(String, u8)> {
    let mut out = vec![];
    fn walk(n: &DocNode, out: &mut Vec<(String, u8)>) {
        match n.kind {
            DocNodeKind::Heading => out.push((n.text.clone(), 1)),
            DocNodeKind::Bullet
            | DocNodeKind::TextSpan
            | DocNodeKind::Paragraph
            | DocNodeKind::Role => {
                if !n.text.trim().is_empty() {
                    out.push((n.text.clone(), 0))
                }
            }
            DocNodeKind::Link => out.push((n.text.clone(), 0)),
            _ => {}
        }
        for c in &n.children {
            walk(c, out);
        }
    }
    walk(&doc.root, &mut out);
    out
}

/// Emit Typst source. Deterministic: same tree -> identical bytes.
pub fn to_typst(doc: &SemanticDocument) -> String {
    let mut s = String::from("// waypoint-generated\n#set page(margin: 2cm)\n");
    for (text, level) in render_lines(doc) {
        if level == 1 {
            s.push_str(&format!("= {text}\n"));
        } else if text.starts_with("- ") {
            s.push_str(&format!("- {text}\n"));
        } else {
            s.push_str(&format!("{text}\n\n"));
        }
    }
    s
}

/// Minimal but valid OOXML (docx) writer. A .docx is a zip containing
/// [Content_Types].xml, _rels/.rels, word/document.xml. We store entries
/// uncompressed (stored method) with correct CRC32 so Word can open them.
pub fn to_docx_bytes(doc: &SemanticDocument) -> Vec<u8> {
    let mut body = String::new();
    for (text, level) in render_lines(doc) {
        if level == 1 {
            body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>",
                xml_escape(&text)
            ));
        } else {
            body.push_str(&format!(
                "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
                xml_escape(&text)
            ));
        }
    }

    let document_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:body>{}</w:body></w:document>",
        body
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
</Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

    let entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document_xml.as_bytes()),
    ];
    build_zip_stored(&entries)
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/* ---------------------------- minimal zip writer ---------------------------- */

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

fn push_u16(v: &mut Vec<u8>, n: u16) {
    v.extend_from_slice(&n.to_le_bytes());
}
fn push_u32(v: &mut Vec<u8>, n: u32) {
    v.extend_from_slice(&n.to_le_bytes());
}

/// Build a STORED-method zip archive with local headers + central directory.
/// Every field is little-endian per the ZIP spec; offsets and sizes are real.
fn build_zip_stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut count = 0u16;

    for (name, data) in entries {
        let offset = out.len() as u32;
        let name_b = name.as_bytes();
        let crc = crc32(data);

        // local file header
        out.extend_from_slice(b"PK");
        push_u16(&mut out, 20); // version needed
        push_u16(&mut out, 0); // flags
        push_u16(&mut out, 0); // method: stored
        push_u16(&mut out, 0); // mod time
        push_u16(&mut out, 0); // mod date
        push_u32(&mut out, crc);
        push_u32(&mut out, data.len() as u32); // compressed
        push_u32(&mut out, data.len() as u32); // uncompressed
        push_u16(&mut out, name_b.len() as u16);
        push_u16(&mut out, 0); // extra len
        out.extend_from_slice(name_b);
        out.extend_from_slice(data);

        // central directory record
        central.extend_from_slice(b"PK");
        push_u16(&mut central, 20); // version made by
        push_u16(&mut central, 20); // version needed
        push_u16(&mut central, 0); // flags
        push_u16(&mut central, 0); // method
        push_u16(&mut central, 0); // time
        push_u16(&mut central, 0); // date
        push_u32(&mut central, crc);
        push_u32(&mut central, data.len() as u32);
        push_u32(&mut central, data.len() as u32);
        push_u16(&mut central, name_b.len() as u16);
        push_u16(&mut central, 0); // extra
        push_u16(&mut central, 0); // comment
        push_u16(&mut central, 0); // disk number
        push_u16(&mut central, 0); // internal attrs
        push_u32(&mut central, 0); // external attrs
        push_u32(&mut central, offset);
        central.extend_from_slice(name_b);
        count += 1;
    }

    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    // end of central directory
    out.extend_from_slice(b"PK");
    push_u16(&mut out, 0);
    push_u16(&mut out, 0);
    push_u16(&mut out, count);
    push_u16(&mut out, count);
    push_u32(&mut out, central.len() as u32);
    push_u32(&mut out, cd_offset);
    push_u16(&mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{DocNode, DocNodeKind};

    fn sample() -> SemanticDocument {
        let claim_free = DocNode {
            id: "r".into(),
            kind: DocNodeKind::Document,
            text: String::new(),
            claim_ids: vec![],
            locked: false,
            children: vec![
                DocNode::heading("Experience"),
                DocNode::text_span("Engineer at Acme (2020-2022)", None),
            ],
        };
        SemanticDocument {
            id: "cv".into(),
            kind: "cv".into(),
            profile_revision: 1,
            root: claim_free,
        }
    }

    #[test]
    fn typst_output_is_deterministic_and_ordered() {
        let a = to_typst(&sample());
        let b = to_typst(&sample());
        assert_eq!(a, b);
        assert!(a.contains("= Experience"));
        assert!(a.contains("Engineer at Acme (2020-2022)"));
    }

    #[test]
    fn docx_contains_escaped_xml_and_required_parts() {
        let bytes = to_docx_bytes(&sample());
        assert!(bytes.starts_with(b"PK"));
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("[Content_Types].xml"));
        assert!(s.contains("word/document.xml"));
        assert!(s.contains("Engineer at Acme"));
    }
}
