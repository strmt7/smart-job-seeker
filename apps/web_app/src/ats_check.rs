//! Local, bounded evidence for reviewing a CV against one saved posting.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use waypoint_domain::Sha256Digest;

struct Task {
    id: String,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .filter(|word| word.chars().count() >= 3 && word.len() <= 48)
        .map(|word| word.to_lowercase())
        .collect()
}
fn structure_notes(name: &str, bytes: &[u8]) -> Result<Vec<String>, String> {
    if !name.to_ascii_lowercase().ends_with(".docx") {
        return Ok(vec![
            "PDF visual layout, columns, images and headers are not assessed by this text check."
                .into(),
        ]);
    }
    use std::io::Read;
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "Invalid DOCX archive")?;
    let headers = (0..zip.len()).any(|i| {
        zip.by_index_raw(i)
            .ok()
            .is_some_and(|f| f.name().starts_with("word/header") && f.name().ends_with(".xml"))
    });
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
    let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
    let mut tables = 0;
    let mut text_boxes = 0;
    loop {
        match reader.read_event().map_err(|_| "Malformed DOCX XML")? {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                if e.name().as_ref() == b"w:tbl" {
                    tables += 1;
                }
                if e.name().as_ref() == b"w:txbxContent" {
                    text_boxes += 1;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => (),
        }
    }
    let mut notes = Vec::new();
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
    if headers {
        notes.push("DOCX has header files; this extractor only reads the document body, so check that contact details are also present there.".into());
    }
    notes.push("DOCX visual layout and column order are not fully assessed.".into());
    Ok(notes)
}

fn report(name: &str, bytes: &[u8], listing: &str) -> Result<Value, String> {
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
    let terms: Vec<_> = terms.into_iter().take(30).map(|(term,count)| json!({"present":resume_words.contains(&term),"term":term,"posting_occurrences":count})).collect();
    let lines: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(12)
        .map(|line| line.chars().take(240).collect::<String>())
        .collect();
    Ok(
        json!({"document_sha256":waypoint_domain::sha256_hex(bytes),"job_text_sha256":Sha256Digest::of(listing).as_str(),"extracted_characters":text.chars().count(),"extracted_words":words(&text).len(),"reading_order_preview":lines,"structure_notes":structure,"terms":terms,"terms_truncated":truncated,"limitations":"One local text extractor and literal single-word matching; not a real ATS result. Inspect the extracted reading order, layout, headers and all claims. Missing terms may be irrelevant or not true for you; do not add unsupported claims."}),
    )
}
pub fn start(id: String, name: String, bytes: Vec<u8>, listing: String) -> Result<(), String> {
    let mut task = TASK.lock().map_err(|_| "CV check unavailable")?;
    if task.as_ref().is_some_and(|t| t.result.is_none()) {
        return Err("A CV check is already running".into());
    }
    *task = Some(Task {
        id: id.clone(),
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| report(&name, &bytes, &listing))
            .unwrap_or_else(|_| Err("CV check stopped unexpectedly".into()));
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|t| t.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "CV check unavailable")?;
    let task = tasks
        .as_mut()
        .filter(|t| t.id == id)
        .ok_or("CV check not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running"}));
    };
    *tasks = None;
    Ok(match result {
        Ok(report) => json!({"status":"complete","report":report}),
        Err(error) => json!({"status":"failed","error":error}),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a local PDF worker and fictional PDF fixture"]
    fn checks_extractable_pdf_against_posting() {
        let path = std::env::var("WAYPOINT_PDF_FIXTURE").unwrap();
        let bytes = std::fs::read(path).unwrap();
        let result = report("fictional.pdf", &bytes, "Synthetic experience and Rust").unwrap();
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
    fn detects_docx_table_text_box_and_header() {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(br#"<w:document xmlns:w="urn:fixture"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Rust experience</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:txbxContent><w:p><w:r><w:t>Example</w:t></w:r></w:p></w:txbxContent></w:body></w:document>"#).unwrap();
        zip.start_file("word/header1.xml", options).unwrap();
        zip.write_all(b"<header/>").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let result = report("fixture.docx", &bytes, "Rust role").unwrap();
        let notes = result["structure_notes"].to_string();
        assert!(notes.contains("table") && notes.contains("text box") && notes.contains("header"));
        assert!(result["reading_order_preview"]
            .to_string()
            .contains("Rust experience"));
    }
    #[test]
    fn reports_extractable_text_and_literal_posting_terms() {
        let result = report(
            "cv.txt",
            b"Fictional Candidate\nRust engineer\nResearch",
            "Rust engineer required; Python and Python",
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
        assert!(report("cv.txt", b"", "A role").is_err());
    }
}
