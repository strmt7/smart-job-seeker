//! Local, bounded evidence for reviewing a CV against one saved posting.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use waypoint_domain::Sha256Digest;

struct Task {
    id: String,
    document: String,
    job: String,
    result: Option<Result<Value, String>>,
    completed: Option<std::time::Instant>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value, std::time::Instant)>> = Mutex::new(None);
const RETENTION: std::time::Duration = std::time::Duration::from_secs(600);

fn words(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if !character.is_alphanumeric() && character != '+' && character != '#' {
            continue;
        }
        let mut end = start + character.len_utf8();
        while let Some(&(index, next)) = chars.peek() {
            if !next.is_alphanumeric() && next != '+' && next != '#' {
                break;
            }
            chars.next();
            end = index + next.len_utf8();
        }
        let word = &text[start..end];
        let dotnet = word.eq_ignore_ascii_case("net")
            && text[..start].strip_suffix('.').is_some_and(|prefix| {
                prefix
                    .chars()
                    .next_back()
                    .is_none_or(|previous| !previous.is_alphanumeric())
            });
        if word.len() <= 48
            && (word.chars().count() >= 3
                || (word.len() == 2 && word.bytes().all(|byte| byte.is_ascii_uppercase()))
                || matches!(word, "C" | "R")
                || word.eq_ignore_ascii_case("c#")
                || word.eq_ignore_ascii_case("f#"))
        {
            result.push(if dotnet {
                ".net".into()
            } else {
                word.to_lowercase()
            });
        }
    }
    result
}
fn structure_notes(name: &str, bytes: &[u8]) -> Result<Vec<String>, String> {
    if !name.to_ascii_lowercase().ends_with(".docx") {
        return Ok(vec![
            "PDF visual layout, columns, images, headers and footers are not assessed by this text check."
                .into(),
        ]);
    }
    use std::io::Read;
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "Invalid DOCX archive")?;
    let mut headers = false;
    let mut footers = false;
    let mut has_relationships = false;
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(|_| "Invalid DOCX entry")?;
        let name = entry.name().map_err(|_| "Invalid DOCX entry name")?;
        headers |= name.starts_with("word/header") && name.ends_with(".xml");
        footers |= name.starts_with("word/footer") && name.ends_with(".xml");
        has_relationships |= name == "word/_rels/document.xml.rels";
    }
    if has_relationships {
        let mut total = 0;
        let relationships = crate::document_extract::read_docx_text_part(
            &mut zip,
            "word/_rels/document.xml.rels",
            &mut total,
        )?;
        for (header, _) in crate::document_extract::related_header_footer_parts(&relationships)? {
            headers |= header;
            footers |= !header;
        }
    }
    let mut body = zip
        .by_name("word/document.xml")
        .map_err(|_| "DOCX body missing")?;
    if body.size() > 4 * 1024 * 1024 {
        return Err("DOCX body exceeds the CV check limit".into());
    }
    let mut xml = Vec::new();
    body.by_ref()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut xml)
        .map_err(|_| "Could not read DOCX body")?;
    if xml.len() > 4 * 1024 * 1024 {
        return Err("DOCX body exceeds the CV check limit".into());
    }
    let mut reader = quick_xml::NsReader::from_reader(xml.as_slice());
    let mut tables = 0;
    let mut text_boxes = 0;
    let mut multi_column_sections = 0;
    let mut uncertain_columns = false;
    let mut hidden_markers = 0;
    let mut uncertain_hidden = false;
    let mut revision_markers = 0;
    let (mut footnote_references, mut endnote_references) = (0, 0);
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|_| "Malformed DOCX XML")?;
        let word = matches!(namespace, quick_xml::name::ResolveResult::Bound(ns) if
            ns.as_ref() == b"http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            || ns.as_ref() == b"http://purl.oclc.org/ooxml/wordprocessingml/main");
        match event {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                if word
                    && matches!(
                        e.local_name().as_ref(),
                        b"ins"
                            | b"del"
                            | b"moveFrom"
                            | b"moveTo"
                            | b"pPrChange"
                            | b"rPrChange"
                            | b"sectPrChange"
                            | b"tblPrChange"
                            | b"tcPrChange"
                            | b"trPrChange"
                    )
                {
                    revision_markers += 1;
                }
                if word && e.local_name().as_ref() == b"tbl" {
                    tables += 1;
                }
                if word && e.local_name().as_ref() == b"footnoteReference" {
                    footnote_references += 1;
                }
                if word && e.local_name().as_ref() == b"endnoteReference" {
                    endnote_references += 1;
                }
                if word && e.local_name().as_ref() == b"txbxContent" {
                    text_boxes += 1;
                }
                if word && matches!(e.local_name().as_ref(), b"vanish" | b"webHidden") {
                    let mut state = Some(true);
                    for attribute in e.attributes() {
                        let attribute =
                            attribute.map_err(|_| "Malformed DOCX hidden-text attributes")?;
                        let (namespace, local) =
                            reader.resolver_mut().resolve_attribute(attribute.key);
                        let is_word_attribute = matches!(namespace, quick_xml::name::ResolveResult::Bound(ns) if
                            ns.as_ref() == b"http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                            || ns.as_ref() == b"http://purl.oclc.org/ooxml/wordprocessingml/main");
                        if is_word_attribute && local.as_ref() == b"val" {
                            state = match attribute.value.as_ref() {
                                b"1" | b"true" | b"on" => Some(true),
                                b"0" | b"false" | b"off" => Some(false),
                                _ => None,
                            };
                        }
                    }
                    match state {
                        Some(true) => hidden_markers += 1,
                        Some(false) => {}
                        None => uncertain_hidden = true,
                    }
                }
                if word && e.local_name().as_ref() == b"cols" {
                    for attribute in e.attributes() {
                        let attribute =
                            attribute.map_err(|_| "Malformed DOCX column attributes")?;
                        let (namespace, local) =
                            reader.resolver_mut().resolve_attribute(attribute.key);
                        let is_word_attribute = matches!(namespace, quick_xml::name::ResolveResult::Bound(ns) if
                            ns.as_ref() == b"http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                            || ns.as_ref() == b"http://purl.oclc.org/ooxml/wordprocessingml/main");
                        if is_word_attribute && local.as_ref() == b"num" {
                            match std::str::from_utf8(attribute.value.as_ref())
                                .ok()
                                .and_then(|v| v.parse::<u32>().ok())
                            {
                                Some(2..) => multi_column_sections += 1,
                                Some(1) => {}
                                _ => uncertain_columns = true,
                            }
                        }
                    }
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => (),
        }
    }
    let mut notes = Vec::new();
    if footnote_references > 0 || endnote_references > 0 {
        notes.push(format!("DOCX contains {footnote_references} footnote and {endnote_references} endnote reference(s); referenced text may appear after the main text or be unavailable. Inspect the extracted text and original document."));
    }
    if tables > 0 {
        notes.push(format!(
            "DOCX contains {tables} table(s); extraction order can differ between parsers."
        ));
    }
    if text_boxes > 0 {
        notes.push(format!(
            "DOCX contains {text_boxes} text box(es); some parsers may miss their contents."
        ));
    }
    if multi_column_sections > 0 {
        notes.push(format!("DOCX declares multiple columns in {multi_column_sections} section(s); some parsers may read the content in a different order."));
    }
    if uncertain_columns {
        notes.push(
            "A DOCX column count could not be interpreted; inspect the reading order manually."
                .into(),
        );
    }
    if revision_markers > 0 {
        notes.push(format!("DOCX contains {revision_markers} tracked-change marker(s); extracted text may not match the reviewed or final document. Inspect and resolve revisions before using it."));
    }
    if hidden_markers > 0 {
        notes.push(format!("DOCX contains {hidden_markers} enabled hidden-text formatting marker(s); extracted text may include content not visible in the document. Inspect the PDF and extracted text."));
    }
    if uncertain_hidden {
        notes.push("A hidden-text formatting value could not be interpreted; inspect the document and extracted text.".into());
    }
    if headers {
        notes.push("DOCX has header files; some parsers may miss header content, so check that contact details are also present in the body.".into());
    }
    if footers {
        notes.push("DOCX has footer files; some parsers may miss footer content, so check that important details are also present in the body.".into());
    }
    if headers || footers {
        notes.push("Extracted header or footer text may include unused template parts; compare it with the visible document before using it.".into());
    }
    notes.push("DOCX visual layout, column order and hidden formatting inherited from styles or other parts are not fully assessed.".into());
    Ok(notes)
}

fn report(name: &str, bytes: &[u8], listing: &str, facts: &[String]) -> Result<Value, String> {
    let text = crate::document_extract::extract(name, bytes)?;
    let structure = structure_notes(name, bytes)?;
    let resume_words: HashSet<_> = words(&text).into_iter().collect();
    let ignored: HashSet<&str> = [
        "the",
        "and",
        "for",
        "with",
        "you",
        "your",
        "our",
        "are",
        "will",
        "this",
        "that",
        "from",
        "have",
        "has",
        "all",
        "work",
        "working",
        "role",
        "team",
        "job",
        "experience",
        "skills",
        "ability",
        "including",
        "other",
        "within",
        "using",
        "about",
        "years",
        "strong",
        "must",
        "required",
        "preferred",
        "knowledge",
        "environment",
        "company",
        "across",
        "such",
        "their",
        "they",
        "who",
        "can",
        "not",
        "may",
        "new",
        "any",
        "also",
        "such",
        "per",
        "more",
        "than",
    ]
    .into_iter()
    .collect();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for word in words(listing) {
        if !ignored.contains(word.as_str()) {
            *counts.entry(word).or_default() += 1;
        }
    }
    let mut terms: Vec<_> = counts.into_iter().collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let truncated = terms.len() > 30;
    let fact_words: Vec<_> = facts
        .iter()
        .map(|fact| words(fact).into_iter().collect::<HashSet<_>>())
        .collect();
    let terms: Vec<_> = terms.into_iter().take(30).map(|(term,count)| {
        let sources: Vec<_> = facts.iter().zip(&fact_words).filter(|(_,words)| words.contains(&term)).take(3).map(|(fact,_)|fact).collect();
        json!({"present":resume_words.contains(&term),"term":term,"posting_occurrences":count,"matching_qualifications":sources})
    }).collect();
    let lines: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(12)
        .map(|line| line.chars().take(240).collect::<String>())
        .collect();
    Ok(
        json!({"document_sha256":waypoint_domain::sha256_hex(bytes),"job_text_sha256":Sha256Digest::of(listing).as_str(),"extracted_characters":text.chars().count(),"extracted_words":words(&text).len(),"reading_order_preview":lines,"structure_notes":structure,"terms":terms,"terms_truncated":truncated,"limitations":"One local text extractor and literal single-word matching (two-letter ASCII alphabetic terms require uppercase spelling); not a real ATS result. Inspect the extracted reading order, layout, headers, footers and all claims. Missing terms may be irrelevant or not true for you; do not add unsupported claims."}),
    )
}
pub fn start(
    id: String,
    document: String,
    job: String,
    name: String,
    bytes: Vec<u8>,
    listing: String,
    facts: Vec<String>,
) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "CV check unavailable")?;
    if task
        .as_ref()
        .is_some_and(|t| t.completed.is_some_and(|time| time.elapsed() > RETENTION))
    {
        *task = None;
    }
    if task.is_some() {
        return Err("A CV check is running or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        document,
        job,
        result: None,
        completed: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| report(&name, &bytes, &listing, &facts))
            .unwrap_or_else(|_| Err("CV check stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|t| t.id == id) {
                task.result = Some(result);
                task.completed = Some(std::time::Instant::now());
            }
        }
    });
    Ok(())
}
pub fn active() -> Result<Option<Value>, String> {
    let mut tasks = TASK.lock().map_err(|_| "CV check unavailable")?;
    if tasks
        .as_ref()
        .is_some_and(|t| t.completed.is_some_and(|time| time.elapsed() > RETENTION))
    {
        *tasks = None;
    }
    Ok(tasks
        .as_ref()
        .map(|task| json!({"task_id":task.id,"document_id":task.document,"job_id":task.job})))
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "CV check unavailable")?;
    if tasks
        .as_ref()
        .is_some_and(|t| t.completed.is_some_and(|time| time.elapsed() > RETENTION))
    {
        *tasks = None;
    }
    let Some(task) = tasks.as_mut().filter(|t| t.id == id) else {
        let mut completed = COMPLETED.lock().map_err(|_| "CV check unavailable")?;
        if completed
            .as_ref()
            .is_some_and(|(_, _, time)| time.elapsed() > RETENTION)
        {
            *completed = None;
        }
        return completed
            .as_ref()
            .filter(|(key, _, _)| key == id)
            .map(|(_, result, _)| result.clone())
            .ok_or_else(|| "CV check not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    *tasks = None;
    let response = match result {
        Ok(report) => json!({"status":"complete","report":report}),
        Err(error) => json!({"status":"failed","error":error}),
    };
    *COMPLETED.lock().map_err(|_| "CV check unavailable")? =
        Some((id.into(), response.clone(), std::time::Instant::now()));
    Ok(response)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovers_active_and_completed_checks_without_cross_task_results() {
        start(
            "task-a".into(),
            "doc-a".into(),
            "job-a".into(),
            "cv.txt".into(),
            b"Fictional Rust engineer".to_vec(),
            "Rust role".into(),
            vec![],
        )
        .unwrap();
        let pending = active().unwrap().unwrap();
        assert_eq!(pending["task_id"], "task-a");
        assert_eq!(pending["document_id"], "doc-a");
        assert_eq!(pending["job_id"], "job-a");
        assert!(start(
            "task-b".into(),
            "doc-b".into(),
            "job-b".into(),
            "cv.txt".into(),
            b"Fictional".to_vec(),
            "Role".into(),
            vec![]
        )
        .is_err());
        let result = (0..200)
            .find_map(|_| {
                let result = poll("task-a").unwrap();
                if result["status"] == "running" {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    None
                } else {
                    Some(result)
                }
            })
            .expect("check did not finish");
        assert_eq!(result["status"], "complete");
        assert_eq!(poll("task-a").unwrap(), result);
        assert!(poll("other").is_err());
        assert!(active().unwrap().is_none());
        *TASK.lock().unwrap() = Some(Task {
            id: "expired".into(),
            document: "doc".into(),
            job: "job".into(),
            result: Some(Ok(json!({}))),
            completed: Some(
                std::time::Instant::now() - RETENTION - std::time::Duration::from_secs(1),
            ),
        });
        assert!(poll("expired").is_err());
        assert!(active().unwrap().is_none());
    }
    #[test]
    #[ignore = "requires a local PDF worker and fictional PDF fixture"]
    fn checks_extractable_pdf_against_posting() {
        let path = std::env::var("WAYPOINT_PDF_FIXTURE").unwrap();
        let bytes = std::fs::read(path).unwrap();
        let result = report(
            "fictional.pdf",
            &bytes,
            "Synthetic experience and Rust",
            &[],
        )
        .unwrap();
        assert!(result["reading_order_preview"]
            .to_string()
            .contains("Fictional Candidate"));
        assert!(result["terms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["term"] == "rust" && v["present"] == true));
    }
    #[test]
    fn reports_note_references_in_readability_check() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Fictional experience</w:t><w:footnoteReference w:id="1"/><w:endnoteReference w:id="2"/></w:r></w:p></w:body></w:document>"#).unwrap();
        let notes = structure_notes("fixture.docx", &zip.finish().unwrap().into_inner()).unwrap();
        assert!(notes.iter().any(|note| note.contains("1 footnote")
            && note.contains("1 endnote")
            && note.contains("may appear after")));
    }

    #[test]
    fn detects_docx_table_text_box_header_and_footer() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(br#"<d:document xmlns:d="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><d:body><d:tbl><d:tr><d:tc><d:p><d:r><d:t>Rust experience</d:t></d:r></d:p></d:tc></d:tr></d:tbl><d:txbxContent><d:p><d:r><d:rPr><d:vanish/></d:rPr><d:t>Example</d:t></d:r></d:p></d:txbxContent></d:body></d:document>"#).unwrap();
        zip.start_file("word/header1.xml", options).unwrap();
        zip.write_all(b"<header/>").unwrap();
        zip.start_file("word/footer1.xml", options).unwrap();
        zip.write_all(b"<footer/>").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let result = report("fixture.docx", &bytes, "Rust role", &[]).unwrap();
        let notes = result["structure_notes"].to_string();
        assert!(
            notes.contains("table")
                && notes.contains("text box")
                && notes.contains("header")
                && notes.contains("footer")
                && notes.contains("hidden-text")
        );
        assert!(result["reading_order_preview"]
            .to_string()
            .contains("Rust experience"));
    }
    #[test]
    fn footer_warning_requires_a_footer_part() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#).unwrap();
        let notes = structure_notes("cv.docx", &zip.finish().unwrap().into_inner()).unwrap();
        assert!(!notes.iter().any(|note| note.contains("footer files")));
    }
    #[test]
    fn reports_relationship_named_footer() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#).unwrap();
        zip.start_file("word/custom/contact.xml", options).unwrap();
        zip.write_all(
            br#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#,
        )
        .unwrap();
        zip.start_file("word/_rels/document.xml.rels", options)
            .unwrap();
        zip.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="custom/contact.xml"/></Relationships>"#).unwrap();
        let notes = structure_notes("cv.docx", &zip.finish().unwrap().into_inner()).unwrap();
        assert!(notes.iter().any(|note| note.contains("footer files")));
    }
    #[test]
    fn warns_when_document_body_contains_tracked_revisions() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins><w:r><w:t>Added</w:t></w:r></w:ins><w:del><w:r><w:delText>Removed</w:delText></w:r></w:del></w:p></w:body></w:document>"#).unwrap();
        let result = report(
            "fixture.docx",
            &zip.finish().unwrap().into_inner(),
            "Role",
            &[],
        )
        .unwrap();
        assert!(result["structure_notes"]
            .to_string()
            .contains("2 tracked-change marker"));
        assert!(result["reading_order_preview"]
            .to_string()
            .contains("Added"));
    }

    #[test]
    fn detects_enabled_and_unknown_hidden_text_markers() {
        use std::io::Write;
        fn notes(body: &str) -> Vec<String> {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            zip.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            write!(zip, "<d:document xmlns:d=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><d:body>{body}</d:body></d:document>").unwrap();
            structure_notes("cv.docx", &zip.finish().unwrap().into_inner()).unwrap()
        }
        assert!(
            notes("<d:p><d:r><d:rPr><d:vanish/></d:rPr><d:t>Invisible</d:t></d:r></d:p>")
                .iter()
                .any(|n| n.contains("enabled hidden-text"))
        );
        assert!(
            !notes("<d:p><d:r><d:rPr><d:vanish d:val=\"false\"/></d:rPr></d:r></d:p>")
                .iter()
                .any(|n| n.contains("enabled hidden-text"))
        );
        assert!(
            notes("<d:p><d:r><d:rPr><d:webHidden d:val=\"unexpected\"/></d:rPr></d:r></d:p>")
                .iter()
                .any(|n| n.contains("could not be interpreted"))
        );
    }

    #[test]
    fn detects_column_declarations_without_relying_on_prefix() {
        use std::io::Write;
        fn fixture(xml: &[u8]) -> Vec<u8> {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            zip.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(xml).unwrap();
            zip.finish().unwrap().into_inner()
        }
        let multi = fixture(br#"<d:document xmlns:d="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><d:body><d:sectPr><d:cols d:num="2"/></d:sectPr></d:body></d:document>"#);
        assert!(structure_notes("cv.docx", &multi)
            .unwrap()
            .iter()
            .any(|v| v.contains("declares multiple columns")));
        let invalid = fixture(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:sectPr><w:cols w:num="bad"/></w:sectPr></w:body></w:document>"#);
        assert!(structure_notes("cv.docx", &invalid)
            .unwrap()
            .iter()
            .any(|v| v.contains("could not be interpreted")));
        let single = fixture(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:sectPr><w:cols w:num="1"/></w:sectPr></w:body></w:document>"#);
        assert!(!structure_notes("cv.docx", &single)
            .unwrap()
            .iter()
            .any(|v| v.contains("declares multiple columns")));
    }
    #[test]
    fn reports_extractable_text_and_literal_posting_terms() {
        let result = report(
            "cv.txt",
            b"Fictional Candidate\nRust engineer\nResearch",
            "Rust engineer required; Python and Python",
            &["Used Python for research".into()],
        )
        .unwrap();
        let terms = result["terms"].as_array().unwrap();
        assert!(terms
            .iter()
            .any(|v| v["term"] == "rust" && v["present"] == true));
        assert!(terms
            .iter()
            .any(|v| v["term"] == "python" && v["present"] == false));
        assert!(result["reading_order_preview"].as_array().unwrap().len() >= 2);
        assert!(
            terms.iter().find(|v| v["term"] == "python").unwrap()["matching_qualifications"]
                .as_array()
                .unwrap()
                .iter()
                .any(|fact| fact == "Used Python for research")
        );
        assert!(report("cv.txt", b"", "A role", &[]).is_err());
    }

    #[test]
    fn uppercase_two_letter_acronyms_are_compared_without_matching_lowercase_words() {
        let result = report(
            "cv.txt",
            b"Fictional applicant: AI and ML; working in research.",
            "AI ML QA IT and in-house work",
            &[],
        )
        .unwrap();
        let terms = result["terms"].as_array().unwrap();
        for term in ["ai", "ml"] {
            assert!(
                terms
                    .iter()
                    .any(|item| item["term"] == term && item["present"] == true),
                "{term}"
            );
        }
        for term in ["qa", "it"] {
            assert!(
                terms
                    .iter()
                    .any(|item| item["term"] == term && item["present"] == false),
                "{term}"
            );
        }
        assert!(!terms.iter().any(|item| item["term"] == "in"));
    }

    #[test]
    fn short_technology_names_and_dotnet_remain_distinct() {
        assert_eq!(
            words("C C# C++ F# R .NET net example.net a I"),
            vec!["c", "c#", "c++", "f#", "r", ".net", "net", "example", "net"]
        );
        let result = report(
            "cv.txt",
            b"Fictional engineer: C#, C++, R and .NET.",
            "Experience with C#, C++, R, .NET and F#; net income reporting.",
            &["Used F# for analysis".into()],
        )
        .unwrap();
        let terms = result["terms"].as_array().unwrap();
        for term in ["c#", "c++", "r", ".net"] {
            assert!(
                terms
                    .iter()
                    .any(|item| item["term"] == term && item["present"] == true),
                "{term}"
            );
        }
        let fsharp = terms.iter().find(|item| item["term"] == "f#").unwrap();
        assert_eq!(fsharp["present"], false);
        assert_eq!(fsharp["matching_qualifications"][0], "Used F# for analysis");
        assert_eq!(
            terms.iter().find(|item| item["term"] == "net").unwrap()["present"],
            false
        );
    }
}
