#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Local browser interface for the existing Waypoint workspace.
mod answer_task;
mod application_check;
mod attempt_record;
mod candidate_task;
mod convert_task;
mod discovery_task;
mod doc_suggest;
mod document_convert;
mod document_extract;
mod docx_edit;
mod docx_edit_task;
mod draft_task;
mod duplicate_compare;
mod duplicate_search;
mod email_draft;
mod extract_task;
mod fit_task;
mod form_links;
mod form_preview;
mod form_task;
mod gmail_connect;
mod gmail_net;
mod gmail_oauth;
mod letter_export;
mod manual_job;
mod pdf_extract;
mod pdf_job;
mod pdf_preview_task;
mod pdf_setup;
mod search_planner;
mod storage_settings;
mod suggestions;
mod template_suggest;
#[cfg(windows)]
mod tray;
mod web_search;
mod web_search_batch;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use waypoint_applications::driver::BrowserDriver;
use waypoint_domain::{
    ApplicationState, ApprovalGrant, GrantId, GrantOperation, IntentId, JobIdentityId, Sha256Digest,
};
use waypoint_inference::ollama::http_request;
use waypoint_search::CampaignConstraints;
use waypoint_workspace::{DiscoveryTarget, FactOrigin, LiveFetcher, Workspace};

const MAX_REQUEST: usize = 64 * 1024;
const MAX_DOCUMENT: usize = 10 * 1024 * 1024;
const SESSION_TTL: Duration = Duration::from_secs(120);
const VISIBLE_SESSION_TTL: Duration = Duration::from_secs(600);

struct ReviewSession {
    id: String,
    job_id: JobIdentityId,
    url: String,
    profile_digest: Sha256Digest,
    preview: form_preview::FormPreview,
    driver: waypoint_browser::CdpDriver,
    created: Instant,
    ttl: Duration,
    filled: bool,
    visible: bool,
    staged_files: Vec<tempfile::TempDir>,
    record: Option<attempt_record::AttemptRecord>,
}

fn interrupt_session(
    ws: &mut Workspace,
    session: &mut Option<ReviewSession>,
    reason: &str,
) -> Result<(), String> {
    if let Some(current) = session.as_mut() {
        ws.mark_manual_review(
            &current.job_id,
            "Application review session interrupted",
            now(),
        )
        .map_err(|e| e.to_string())?;
        if let Some(record) = &mut current.record {
            record.event(json!({"type":"review_interrupted","time":now(),"reason":reason}))?;
            record.status("manual_review", now())?;
        }
    }
    *session = None;
    Ok(())
}

fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("random id generation failed: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
const PROFILE_FIELDS: &[&str] = &[
    "first_name",
    "last_name",
    "email",
    "phone",
    "address",
    "city",
    "region",
    "postal_code",
    "country",
    "linkedin",
    "website",
    "work_authorization",
    "instructions",
    "cv_template_id",
    "cover_template_id",
    "document_instructions",
];

fn fit_inputs(
    ws: &Workspace,
    job_id: &JobIdentityId,
) -> Result<(Vec<String>, String, String, String), String> {
    let job = ws
        .job(job_id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let facts: Vec<String> = ws
        .facts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|fact| !fact.requires_review && fact.text.len() <= 1000)
        .take(20)
        .map(|fact| fact.text)
        .collect();
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(text) => serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let instructions: String = profile["instructions"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(2000)
        .collect();
    let description = ws
        .job_description(job_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let listing: String = format!(
        "Title: {}\nEmployer: {}\nLocation: {}\nDescription: {}",
        job.title, job.employer, job.location, description
    )
    .chars()
    .take(8000)
    .collect();
    let digest = Sha256Digest::of(
        &json!({"facts":facts,"instructions":instructions,"listing":listing}).to_string(),
    )
    .as_str()
    .to_owned();
    Ok((facts, instructions, listing, digest))
}

fn read_fit(ws: &Workspace, job_id: &JobIdentityId) -> Result<Value, String> {
    match ws.fit_assessment(job_id).map_err(|e| e.to_string())? {
        Some(data) => {
            let value: Value =
                serde_json::from_slice(&data).map_err(|_| "Stored assessment is invalid")?;
            let (_, _, _, digest) = fit_inputs(ws, job_id)?;
            if value["input_digest"].as_str() == Some(digest.as_str()) {
                Ok(
                    json!({"status":"current","assessment":value["assessment"],"model":value["model"]}),
                )
            } else {
                Ok(json!({"status":"stale"}))
            }
        }
        None => Ok(json!({"status":"missing"})),
    }
}

fn reviewed_form_answers(
    answers: &Value,
    preview: &form_preview::FormPreview,
) -> Result<Vec<waypoint_applications::form_mapping::ProposedMapping>, String> {
    use waypoint_applications::form_mapping::{FormFieldType, PacketValue, ProposedMapping};
    let answers = answers
        .as_array()
        .filter(|a| a.len() <= 80)
        .ok_or("Answers must be a list of at most 80 fields")?;
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for answer in answers {
        let selector = answer["selector"].as_str().ok_or("Missing answer field")?;
        let field = preview
            .schema
            .fields
            .iter()
            .find(|f| f.selector == selector)
            .ok_or("Unknown answer field")?;
        if field.sensitive
            || !seen.insert(selector)
            || preview
                .suggestions
                .iter()
                .any(|m| m.field_selector == selector)
        {
            return Err("Answer requires manual handling or duplicates another field".into());
        }
        let value = match (&field.field_type, &answer["value"]) {
            (FormFieldType::Checkbox, Value::Bool(value)) => PacketValue::Bool(*value),
            (kind, Value::String(value)) if !value.trim().is_empty() && value.len() <= 4000 => {
                match kind {
                    FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea => {
                        PacketValue::Text(value.clone())
                    }
                    FormFieldType::Date
                        if waypoint_applications::form_mapping::valid_iso_date(value) =>
                    {
                        PacketValue::Date(value.clone())
                    }
                    FormFieldType::Select { options } | FormFieldType::Radio { options }
                        if options
                            .iter()
                            .filter(|option| option.trim().eq_ignore_ascii_case(value.trim()))
                            .count()
                            == 1 =>
                    {
                        PacketValue::Text(value.clone())
                    }
                    _ => {
                        return Err("Answer requires manual handling or has an invalid value".into())
                    }
                }
            }
            _ => return Err("Answer requires manual handling or has an invalid value".into()),
        };
        result.push(ProposedMapping {
            field_selector: selector.to_owned(),
            value,
        });
    }
    Ok(result)
}

fn answer_inputs(
    ws: &Workspace,
    session: &ReviewSession,
    selector: &str,
) -> Result<answer_task::Inputs, String> {
    use waypoint_applications::form_mapping::FormFieldType;
    if session.created.elapsed() > session.ttl || session.filled {
        return Err("Inspect the current form again".into());
    }
    let field = session
        .preview
        .schema
        .fields
        .iter()
        .find(|f| f.selector == selector)
        .ok_or("Field is no longer available")?;
    if field.sensitive
        || !matches!(
            field.field_type,
            FormFieldType::Text | FormFieldType::TextArea
        )
        || field.label.len() > 1000
    {
        return Err("This question requires manual handling".into());
    }
    let job = ws
        .job(&session.job_id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    if !matches!(
        job.state,
        ApplicationState::Shortlisted | ApplicationState::Prepared
    ) || job.canonical_url.as_deref() != Some(session.url.as_str())
    {
        return Err("Job changed; inspect the form again".into());
    }
    let (facts, _, listing, _) = fit_inputs(ws, &session.job_id)?;
    if facts.is_empty() {
        return Err("Add reviewed qualifications before drafting an answer".into());
    }
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(text) => serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    if Sha256Digest::of(&profile.to_string()) != session.profile_digest {
        return Err("Profile changed; inspect the form again".into());
    }
    let instructions: String = profile["document_instructions"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(2000)
        .collect();
    let question = field.label.clone();
    let digest = Sha256Digest::of(&json!({"session":session.id,"schema":session.preview.schema.schema_hash_input(),"selector":selector,"question":question,"listing":listing,"facts":facts,"instructions":instructions}).to_string()).as_str().to_owned();
    Ok(answer_task::Inputs {
        session_id: session.id.clone(),
        selector: selector.to_owned(),
        digest,
        question,
        listing,
        facts,
        instructions,
    })
}

fn duplicate_inputs(
    ws: &Workspace,
    first: &JobIdentityId,
    second: &JobIdentityId,
) -> Result<duplicate_compare::Inputs, String> {
    if first == second {
        return Err("Choose two different listings".into());
    }
    if !ws
        .pending_duplicate_pair(first, second)
        .map_err(|e| e.to_string())?
    {
        return Err("This pair is no longer awaiting duplicate review".into());
    }
    let listing = |id: &JobIdentityId| -> Result<String, String> {
        let job = ws
            .job(id)
            .map_err(|e| e.to_string())?
            .ok_or("Job no longer exists")?;
        let description = ws
            .job_description(id)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        Ok(format!(
            "Title: {}\nEmployer: {}\nLocation: {}\nRequisition: {}\nPosted: {}\nDescription: {}",
            job.title,
            job.employer,
            job.location,
            job.requisition_id.unwrap_or_default(),
            job.posted_at.unwrap_or_default(),
            description
        )
        .chars()
        .take(8000)
        .collect())
    };
    let first_text = listing(first)?;
    let second_text = listing(second)?;
    let digest = Sha256Digest::of(&json!({"first":first_text,"second":second_text}).to_string())
        .as_str()
        .to_owned();
    Ok(duplicate_compare::Inputs {
        first_id: first.as_str().to_owned(),
        second_id: second.as_str().to_owned(),
        first: first_text,
        second: second_text,
        digest,
    })
}

fn duplicate_search_inputs(
    ws: &Workspace,
    id: &JobIdentityId,
) -> Result<duplicate_search::Inputs, String> {
    if ws.duplicate_index_pending().map_err(|e| e.to_string())? {
        return Err(
            "Older jobs are still being indexed; try duplicate search after indexing finishes"
                .into(),
        );
    }
    let listing = |job: &waypoint_store::StoredJob| -> Result<String, String> {
        let description = ws
            .job_description(&job.id)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        Ok(format!(
            "Title: {}\nEmployer: {}\nLocation: {}\nRequisition: {}\nDescription: {}",
            job.title,
            job.employer,
            job.location,
            job.requisition_id.as_deref().unwrap_or(""),
            description
        )
        .chars()
        .take(1800)
        .collect())
    };
    let job = ws
        .job(id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let target = listing(&job)?;
    let (neighbors, truncated) = ws.duplicate_neighbors(id).map_err(|e| e.to_string())?;
    let mut candidates = Vec::with_capacity(neighbors.len());
    for job in neighbors {
        candidates.push((job.id.as_str().to_owned(), listing(&job)?));
    }
    let digest = Sha256Digest::of(
        &json!({"target":target,"candidates":candidates,"truncated":truncated}).to_string(),
    )
    .as_str()
    .to_owned();
    Ok(duplicate_search::Inputs {
        job_id: id.as_str().to_owned(),
        target,
        candidates,
        digest,
        truncated,
    })
}

fn template_suggestion_inputs(
    ws: &Workspace,
    document_id: &str,
    job_id: &JobIdentityId,
    before: &str,
) -> Result<template_suggest::Inputs, String> {
    if before.is_empty() || before.len() > 1000 || before.chars().any(char::is_control) {
        return Err("Choose a single-line passage up to 1000 bytes".into());
    }
    let document = ws
        .documents()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.id == document_id)
        .ok_or("Document not found")?;
    if !document.name.to_ascii_lowercase().ends_with(".docx")
        && !document.name.to_ascii_lowercase().ends_with(".pdf")
    {
        return Err("Choose a PDF or DOCX template".into());
    }
    let source = ws
        .document_content(document_id)
        .map_err(|e| e.to_string())?
        .ok_or("Document not found")?;
    let (facts, _, listing, _) = fit_inputs(ws, job_id)?;
    if facts.is_empty() {
        return Err("Add reviewed qualifications before requesting a suggestion".into());
    }
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(value) => serde_json::from_str(&value).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let instructions: String = profile["document_instructions"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(2000)
        .collect();
    let digest=Sha256Digest::of(&json!({"source":waypoint_domain::sha256_hex(&source),"before":before,"job":job_id.as_str(),"listing":listing,"facts":facts,"instructions":instructions}).to_string()).as_str().to_owned();
    Ok(template_suggest::Inputs {
        document_id: document_id.to_owned(),
        document_name: document.name,
        job_id: job_id.as_str().to_owned(),
        digest,
        source,
        before: before.to_owned(),
        listing,
        facts,
        instructions,
    })
}

fn check_answer_page(session: &mut ReviewSession) -> Result<(), String> {
    let live = session
        .driver
        .inspect_current()
        .map_err(|e| e.to_string())?;
    if !live.walls.is_empty()
        || live.schema != session.preview.schema
        || live.origin != session.preview.schema.origin
    {
        return Err("The page changed or needs manual attention; inspect again".into());
    }
    Ok(())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs() as i64)
        .unwrap_or(0)
}

/// Only use one already-resident model that Ollama reports as fully GPU
/// resident and within the existing 14 GiB budget. This check is a snapshot,
/// not a reservation; Ollama can change its state after it is read.
fn resident_model() -> Result<Option<String>, String> {
    let addr = "127.0.0.1:11434"
        .parse()
        .map_err(|_| "invalid runtime address")?;
    let (status, bytes) = http_request(addr, "GET", "/api/ps", None, 3000)?;
    if status != 200 {
        return Err(format!("Ollama returned HTTP {status}"));
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid Ollama response")?;
    eligible_resident_model(&data)
}

fn eligible_resident_model(data: &Value) -> Result<Option<String>, String> {
    let models = data["models"]
        .as_array()
        .ok_or("Ollama did not report resident models")?;
    if models.len() != 1 {
        return Ok(None);
    }
    let model = &models[0];
    let size = model["size"].as_u64().unwrap_or(0);
    let vram = model["size_vram"].as_u64().unwrap_or(0);
    if size == 0 || vram != size || vram > 14 * 1024 * 1024 * 1024 {
        return Ok(None);
    }
    Ok(model["name"]
        .as_str()
        .filter(|name| !name.is_empty())
        .map(str::to_owned))
}

fn reply(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let header = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn json_reply(stream: &mut TcpStream, status: &str, value: Value) {
    reply(
        stream,
        status,
        "application/json; charset=utf-8",
        &value.to_string(),
    );
}

fn document_reply(stream: &mut TcpStream, body: &[u8]) {
    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn serve(
    stream: &mut TcpStream,
    ws: &mut Workspace,
    session: &mut Option<ReviewSession>,
    token: &str,
    port: u16,
) -> Result<(), String> {
    // Accepted sockets can inherit the listener's nonblocking mode on some
    // platforms; requests use a bounded blocking read instead.
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 4096];
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("incomplete request".into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_REQUEST {
            return Err("request too large".into());
        }
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = std::str::from_utf8(&buf[..header_end]).map_err(|_| "invalid headers")?;
    let mut lines = head.split("\r\n");
    let mut request = lines.next().ok_or("missing request")?.split_whitespace();
    let method = request.next().ok_or("missing method")?.to_owned();
    let path = request.next().ok_or("missing path")?.to_owned();
    let mut host = String::new();
    let mut supplied_token = String::new();
    let mut origin = String::new();
    let mut filename = String::new();
    let mut length = 0usize;
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            match key.to_ascii_lowercase().as_str() {
                "host" => host = value.trim().to_owned(),
                "origin" => origin = value.trim().to_owned(),
                "x-document-name" => filename = value.trim().to_owned(),
                "x-workspace-token" => supplied_token = value.trim().to_owned(),
                "content-length" => length = value.trim().parse().map_err(|_| "invalid length")?,
                _ => (),
            }
        }
    }
    if host != format!("127.0.0.1:{port}") {
        return Err("invalid host".into());
    }
    if path == "/" && method == "GET" {
        reply(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            include_str!("../web/index.html"),
        );
        return Ok(());
    }
    if path == "/app.js" && method == "GET" {
        reply(
            stream,
            "200 OK",
            "text/javascript; charset=utf-8",
            include_str!("../web/app.js"),
        );
        return Ok(());
    }
    if path == "/style.css" && method == "GET" {
        reply(
            stream,
            "200 OK",
            "text/css; charset=utf-8",
            include_str!("../web/style.css"),
        );
        return Ok(());
    }
    if supplied_token != token
        || (!origin.is_empty() && origin != format!("http://127.0.0.1:{port}"))
    {
        return Err("unauthorized".into());
    }
    let body_limit = if method == "POST" && path == "/api/documents/upload" {
        MAX_DOCUMENT
    } else {
        MAX_REQUEST - header_end
    };
    if length > body_limit {
        return Err("request too large".into());
    }
    while buf.len() < header_end + length {
        let mut chunk = [0; 4096];
        let remaining = header_end + length - buf.len();
        let n = stream
            .read(&mut chunk[..remaining.min(4096)])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("incomplete body".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    if method == "GET" && path == "/api/state" {
        let facts = ws.facts().map_err(|e| e.to_string())?;
        let skills: Vec<String> = facts.iter().map(|f| f.text.clone()).collect();
        let ranked = ws
            .ranked(&CampaignConstraints::default(), &skills, now())
            .map_err(|e| e.to_string())?;
        let jobs: Vec<Value> = ranked.strict.iter().map(|row| json!({"job": row.job, "score": if skills.is_empty() {None} else {Some(row.score)}, "matched": row.matched_skills})).collect();
        let facts: Vec<Value> = facts
            .iter()
            .map(|f| json!({"id":f.id,"text":f.text,"requires_review":f.requires_review}))
            .collect();
        let documents: Vec<Value> = ws.documents().map_err(|e| e.to_string())?.iter().map(|doc| json!({"id":doc.id,"name":doc.name,"size":doc.size,"created_at":doc.created_at})).collect();
        let reviews: Vec<Value> = ws
            .manual_reviews()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|review| json!({"job_id":review.job_id,"reason":review.reason}))
            .collect();
        let profile = match ws.candidate_profile().map_err(|e| e.to_string())? {
            Some(content) => {
                serde_json::from_str::<Value>(&content).map_err(|_| "stored profile is invalid")?
            }
            None => json!({}),
        };
        json_reply(
            stream,
            "200 OK",
            json!({"jobs": jobs, "facts": facts, "documents":documents, "reviews":reviews, "duplicates":ws.possible_duplicates().map_err(|e| e.to_string())?, "duplicate_index_pending":ws.duplicate_index_pending().map_err(|e|e.to_string())?, "profile":profile, "sealing": ws.sealing_status()}),
        );
        return Ok(());
    }
    if method == "GET" && path == "/api/model-status" {
        let status = resident_model();
        json_reply(
            stream,
            "200 OK",
            match status {
                Ok(model) => json!({"model":model}),
                Err(error) => json!({"model":null,"error":error}),
            },
        );
        return Ok(());
    }
    if method == "GET" && path == "/api/tool-status" {
        let office = document_convert::office_path();
        let browser = waypoint_browser::cdp::BrowserProcess::discover_binaries()
            .into_iter()
            .next();
        json_reply(
            stream,
            "200 OK",
            json!({"libreoffice":office.map(|p| p.display().to_string()),"browser":browser.map(|p| p.display().to_string()),"pdf_python":pdf_extract::python_path().map(|p|p.display().to_string())}),
        );
        return Ok(());
    }
    if method != "POST" {
        return Err("unknown endpoint".into());
    }
    if path == "/api/documents/upload" {
        let name = percent_encoding::percent_decode_str(&filename)
            .decode_utf8()
            .map_err(|_| "invalid document name")?;
        if name.is_empty()
            || name.len() > 255
            || name
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
            || length == 0
        {
            return Err("invalid document name or empty file".into());
        }
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|e| format!("document id generation failed: {e}"))?;
        let id: String = random.iter().map(|b| format!("{b:02x}")).collect();
        ws.add_document(&id, &name, &buf[header_end..header_end + length], now())
            .map_err(|e| e.to_string())?;
        json_reply(stream, "200 OK", json!({"id":id}));
        return Ok(());
    }
    let body: Value = serde_json::from_slice(&buf[header_end..header_end + length])
        .map_err(|_| "invalid JSON")?;
    match path.as_str() {
        "/api/jobs/add" => {
            let result = manual_job::add(ws, &body, now())?;
            json_reply(stream, "200 OK", result);
        }
        "/api/storage/status" => {
            let control = storage_settings::control_dir()?;
            let default = storage_settings::default_workspace()?;
            let configured = storage_settings::load_at(&control, &default)?;
            let active = ws
                .db_path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().into_owned());
            json_reply(
                stream,
                "200 OK",
                json!({"active":active,"configured":configured.to_string_lossy(),"default":default.to_string_lossy()}),
            );
        }
        "/api/storage/configure" => {
            let value = body["path"].as_str().ok_or("missing folder")?;
            let selected = storage_settings::save_at(&storage_settings::control_dir()?, value)?;
            json_reply(
                stream,
                "200 OK",
                json!({"configured":selected.to_string_lossy(),"restart_required":true}),
            );
        }
        "/api/duplicates/search-start" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing job")?)
                .map_err(|e| e.to_string())?;
            let input = duplicate_search_inputs(ws, &job)?;
            if input.candidates.is_empty() {
                return Err("No unreviewed related-employer candidates were found".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            duplicate_search::start(task_id.clone(), model, input)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/duplicates/search-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = duplicate_search::poll(id)?;
            if result["status"] == "complete" {
                let job = JobIdentityId::new(result["job_id"].as_str().ok_or("missing job")?)
                    .map_err(|e| e.to_string())?;
                let input = duplicate_search_inputs(ws, &job)?;
                if Some(input.digest.as_str()) != result["digest"].as_str() {
                    result = json!({"status":"failed","error":"Listings or duplicate decisions changed; search again"});
                } else {
                    let mut flagged = Vec::new();
                    for item in result["matches"].as_array().ok_or("invalid results")? {
                        let candidate = item["id"].as_str().ok_or("missing candidate")?;
                        if !input.candidates.iter().any(|(id, _)| id == candidate) {
                            return Err("Candidate changed; search again".into());
                        }
                        let reason=format!("Local AI flagged a possible duplicate (review required): {}. Listing excerpts: {} / {}",item["reason"].as_str().unwrap_or(""),item["target_excerpt"].as_str().unwrap_or(""),item["candidate_excerpt"].as_str().unwrap_or(""));
                        flagged.push((candidate.to_owned(), reason));
                    }
                    let added = ws
                        .add_ai_duplicates(&job, &flagged)
                        .map_err(|e| e.to_string())?;
                    result["added"] = json!(added);
                }
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/duplicates/compare-start" => {
            let a = JobIdentityId::new(body["first"].as_str().ok_or("missing first job")?)
                .map_err(|e| e.to_string())?;
            let b = JobIdentityId::new(body["second"].as_str().ok_or("missing second job")?)
                .map_err(|e| e.to_string())?;
            let input = duplicate_inputs(ws, &a, &b)?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            duplicate_compare::start(task_id.clone(), model, input)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/duplicates/compare-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = duplicate_compare::poll(id)?;
            if result["status"] == "complete" {
                let first = result["first_id"]
                    .as_str()
                    .and_then(|s| JobIdentityId::new(s).ok());
                let second = result["second_id"]
                    .as_str()
                    .and_then(|s| JobIdentityId::new(s).ok());
                let current = first
                    .zip(second)
                    .and_then(|(a, b)| duplicate_inputs(ws, &a, &b).ok());
                if current.as_ref().map(|input| input.digest.as_str()) != result["digest"].as_str()
                {
                    result = json!({"status":"failed","error":"The listings or duplicate decision changed; compare again"});
                }
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/duplicates/decide" => {
            let a = JobIdentityId::new(body["keep_or_first"].as_str().ok_or("missing job")?)
                .map_err(|e| e.to_string())?;
            let b = JobIdentityId::new(body["other"].as_str().ok_or("missing other job")?)
                .map_err(|e| e.to_string())?;
            let same = body["same"].as_bool().ok_or("missing decision")?;
            if !ws
                .decide_duplicate(&a, &b, same)
                .map_err(|e| e.to_string())?
            {
                return Err("Duplicate pair not found".into());
            }
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/gmail/connect-start" => {
            let client = body["client_json"]
                .as_str()
                .ok_or("missing OAuth client JSON")?;
            let id = random_id()?;
            gmail_connect::start(id.clone(), client.as_bytes().to_vec(), ws)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/gmail/connect-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", gmail_connect::poll(id, ws)?);
        }
        "/api/gmail/connect-cancel" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            gmail_connect::cancel(id)?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/gmail/status" => {
            let credentials = ws
                .gmail_credentials()
                .map_err(|_| "Cannot read Gmail credentials")?
                .map(zeroize::Zeroizing::new);
            let connected = credentials.is_some();
            json_reply(
                stream,
                "200 OK",
                json!({"connected":connected,"secure_storage":ws.can_store_gmail_credentials(),"sending_available":false}),
            );
        }
        "/api/gmail/disconnect" => {
            if gmail_connect::busy() {
                return Err("Cancel the active Gmail connection and wait for it to finish".into());
            }
            ws.clear_gmail_credentials()
                .map_err(|_| "Cannot clear Gmail credentials")?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/application-check/start" => {
            if session.is_some()
                || form_task::busy()
                || candidate_task::busy()
                || form_links::busy()
            {
                return Err(
                    "Close or finish the active form inspection before checking another page"
                        .into(),
                );
            }
            let job_id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let job = ws
                .job(&job_id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            if job.source == "jobicy" {
                return Err(
                    "Jobicy supplies a listing; open it to find the employer application URL"
                        .into(),
                );
            }
            waypoint_workspace::default_registry()
                .check_public_route(&job.source, waypoint_policy::Operation::Read)
                .map_err(|e| e.to_string())?;
            let url = job.canonical_url.ok_or("Job has no URL")?;
            if !url.starts_with("https://") || !waypoint_connectors::is_public_host_url(&url) {
                return Err("Application URL is not a public HTTPS URL".into());
            }
            let id = random_id()?;
            application_check::start(id.clone(), job_id.as_str().to_owned(), url)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/application-check/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            match application_check::poll(id)? {
                Some((job_id, result, review)) => {
                    if review {
                        let job_id = JobIdentityId::new(&job_id).map_err(|e| e.to_string())?;
                        ws.mark_manual_review(
                            &job_id,
                            "Application page needs manual review",
                            now(),
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"complete","result":result}),
                    );
                }
                None => json_reply(stream, "200 OK", json!({"status":"running"})),
            }
        }
        "/api/discover/start" => {
            let targets = body["targets"]
                .as_array()
                .ok_or("missing boards")?
                .iter()
                .map(|entry| {
                    Ok(DiscoveryTarget::new(
                        entry["source"].as_str().ok_or("missing source")?,
                        entry["board"].as_str().ok_or("missing board")?,
                    ))
                })
                .collect::<Result<Vec<_>, &str>>()?;
            let id = random_id()?;
            discovery_task::start(id.clone(), targets, ws)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/web-search/batch-start" => {
            let provider = body["provider"].as_str().ok_or("missing provider")?;
            let queries = body["queries"]
                .as_array()
                .ok_or("missing search phrases")?
                .iter()
                .map(|q| {
                    q.as_str()
                        .map(str::to_owned)
                        .ok_or("Search phrases must be text".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let id = random_id()?;
            let pages = body
                .get("pages")
                .map_or(Some(1), Value::as_u64)
                .ok_or("Invalid page count")?;
            if !(1..=5).contains(&pages) {
                return Err("Choose 1 to 5 pages per search phrase".into());
            }
            web_search_batch::start(id.clone(), provider.to_owned(), queries, pages as u8)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/web-search/batch-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", web_search_batch::poll(id)?);
        }
        "/api/web-search/batch-cancel" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            web_search_batch::cancel(id)?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/web-search/start" => {
            let provider = body["provider"].as_str().ok_or("missing provider")?;
            let query = body["query"].as_str().ok_or("missing query")?;
            let id = random_id()?;
            web_search::start(id.clone(), provider.to_owned(), query.to_owned())?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/web-search/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", web_search::poll(id)?);
        }
        "/api/listing-inspect/start" => {
            if session.is_some()
                || form_task::busy()
                || application_check::busy()
                || candidate_task::busy()
                || form_links::busy()
            {
                return Err(
                    "Finish the current browser session before inspecting a listing".into(),
                );
            }
            let raw = body["url"].as_str().ok_or("missing listing URL")?;
            let parsed = url::Url::parse(raw).map_err(|_| "Invalid listing URL")?;
            if parsed.scheme() != "https"
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || !waypoint_connectors::is_public_host_url(parsed.as_str())
            {
                return Err("Listing URL must be public HTTPS".into());
            }
            let host = parsed.host_str().ok_or("Listing URL has no host")?;
            let source = manual_job::source(host);
            waypoint_workspace::default_registry()
                .check_public_route(source, waypoint_policy::Operation::Read)
                .map_err(|e| e.to_string())?;
            let id = random_id()?;
            candidate_task::start(id.clone(), parsed.to_string())?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/listing-inspect/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            match candidate_task::poll(id)? {
                None => json_reply(stream, "200 OK", json!({"status":"running"})),
                Some(Err(error)) => {
                    json_reply(stream, "200 OK", json!({"status":"failed","error":error}))
                }
                Some(Ok(inspected)) => {
                    let job = inspected.job;
                    let result = manual_job::add(ws, &job, now())?;
                    let job_id =
                        JobIdentityId::new(result["id"].as_str().ok_or("missing saved id")?)
                            .map_err(|e| e.to_string())?;
                    let capture = if inspected.capture.segments.is_empty() {
                        json!({"saved":false,"complete":false,"reason":"Listing capture unavailable"})
                    } else {
                        let metadata = serde_json::to_vec(&inspected.capture.metadata)
                            .map_err(|e| e.to_string())?;
                        match ws.save_job_capture(&job_id, &metadata, &inspected.capture.segments) {
                            Ok(true) => {
                                json!({"saved":true,"complete":inspected.capture.metadata["full_dimensions_captured"],"structured_source":inspected.capture.metadata["structured_source_status"]})
                            }
                            Ok(false) => {
                                let existing = ws
                                    .job_capture(&job_id)
                                    .ok()
                                    .flatten()
                                    .and_then(|(metadata, _)| {
                                        serde_json::from_slice::<Value>(&metadata).ok()
                                    })
                                    .unwrap_or(Value::Null);
                                json!({"saved":true,"preserved_existing":true,"complete":existing["full_dimensions_captured"].as_bool().unwrap_or(false),"structured_source":existing["structured_source_status"].as_str().unwrap_or("unavailable")})
                            }
                            Err(_) => {
                                json!({"saved":false,"complete":false,"reason":"Listing capture could not be saved securely"})
                            }
                        }
                    };
                    if result["existing"].as_bool() == Some(false) {
                        let reason = if job["extraction"] == "local_model" {
                            if job["page_text_truncated"] == true {
                                "Review locally extracted listing; page text was truncated and application route is unverified"
                            } else {
                                "Review locally extracted listing and unverified application route"
                            }
                        } else {
                            "Review structured listing and unverified application route"
                        };
                        ws.mark_manual_review(&job_id, reason, now())
                            .map_err(|e| e.to_string())?;
                    }
                    if capture["saved"] == false || capture["complete"] == false {
                        ws.mark_manual_review(
                            &job_id,
                            "Original listing capture is incomplete or unavailable",
                            now(),
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"complete","job":job,"saved":result,"capture":capture}),
                    );
                }
            }
        }
        "/api/fit/start" => {
            let job_id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let (facts, instructions, listing, digest) = fit_inputs(ws, &job_id)?;
            if facts.is_empty() && instructions.trim().is_empty() {
                return Err("Add reviewed qualifications or search instructions first".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            fit_task::start(
                task_id.clone(),
                model,
                job_id.as_str().to_owned(),
                digest,
                facts,
                instructions,
                listing,
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/fit/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = fit_task::poll(id)?;
            if result["status"] == "complete" {
                let job_id = JobIdentityId::new(result["job_id"].as_str().ok_or("missing job")?)
                    .map_err(|e| e.to_string())?;
                let (_, _, _, digest) = fit_inputs(ws, &job_id)?;
                if result["input_digest"].as_str() != Some(digest.as_str()) {
                    result = json!({"status":"failed","error":"Job or candidate inputs changed during assessment; assess again"});
                } else {
                    result["saved"] = json!(ws
                        .save_fit_assessment(&job_id, result.to_string().as_bytes())
                        .is_ok());
                }
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/fit/read" => {
            let job_id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", read_fit(ws, &job_id)?);
        }
        "/api/fit/batch" => {
            let ids = body["ids"]
                .as_array()
                .filter(|ids| ids.len() <= 20)
                .ok_or("Supply at most 20 job IDs")?;
            let mut results = Vec::with_capacity(ids.len());
            for id in ids {
                let id = JobIdentityId::new(id.as_str().ok_or("Invalid job ID")?)
                    .map_err(|e| e.to_string())?;
                let item = match read_fit(ws, &id) {
                    Ok(value) if value["status"] == "current" => {
                        json!({"id":id.as_str(),"status":"current","score":value["assessment"]["score"]})
                    }
                    Ok(value) => json!({"id":id.as_str(),"status":value["status"]}),
                    Err(_) => json!({"id":id.as_str(),"status":"unavailable"}),
                };
                results.push(item);
            }
            json_reply(stream, "200 OK", json!({"results":results}));
        }
        "/api/search-plan/start" => {
            let facts: Vec<String> = ws
                .facts()
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|fact| !fact.requires_review && fact.text.len() <= 1000)
                .take(20)
                .map(|fact| fact.text)
                .collect();
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            let instructions: String = profile["instructions"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(2000)
                .collect();
            if facts.is_empty() && instructions.trim().is_empty() {
                return Err("Add qualifications or search instructions first".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let id = random_id()?;
            search_planner::start(id.clone(), model, facts, instructions)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/search-plan/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", search_planner::poll(id)?);
        }
        "/api/discover/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", discovery_task::poll(id, ws)?);
        }
        "/api/discover/cancel" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            discovery_task::cancel(id)?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/discover" => {
            let source = body["source"].as_str().ok_or("missing source")?;
            let board = body["board"].as_str().ok_or("missing board")?;
            if !["greenhouse", "lever", "ashby"].contains(&source)
                || board.is_empty()
                || board.len() > 100
                || !board
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                return Err("invalid source or board".into());
            }
            let outcome = ws.discover(
                &[DiscoveryTarget::new(source, board)],
                &LiveFetcher::default(),
                now(),
            );
            json_reply(stream, "200 OK", json!({"summary":outcome.summary()}));
        }
        "/api/facts" => {
            let text = body["text"].as_str().ok_or("missing fact")?;
            if text.len() > 4000 {
                return Err("fact too long".into());
            }
            let id = ws
                .add_fact(text, FactOrigin::UserAttested)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"id": id}));
        }
        "/api/profile" => {
            let input = body.as_object().ok_or("profile must be an object")?;
            if input
                .keys()
                .any(|key| !PROFILE_FIELDS.contains(&key.as_str()))
            {
                return Err("unknown profile field".into());
            }
            let mut profile = serde_json::Map::new();
            for &field in PROFILE_FIELDS {
                let value = match input.get(field) {
                    Some(value) => value.as_str().ok_or("profile fields must be text")?,
                    None => "",
                };
                let limit = if matches!(field, "instructions" | "document_instructions") {
                    16000
                } else {
                    2000
                };
                if value.len() > limit {
                    return Err(format!("{field} exceeds the size limit"));
                }
                profile.insert(field.into(), json!(value));
            }
            for key in ["cv_template_id", "cover_template_id"] {
                let id = profile[key].as_str().unwrap_or("");
                if !id.is_empty() {
                    let valid = ws
                        .documents()
                        .map_err(|e| e.to_string())?
                        .iter()
                        .any(|doc| {
                            doc.id == id
                                && (doc.name.to_lowercase().ends_with(".pdf")
                                    || doc.name.to_lowercase().ends_with(".docx"))
                        });
                    if !valid {
                        return Err(format!("{key} must refer to a saved PDF or DOCX"));
                    }
                }
            }
            ws.save_candidate_profile(&Value::Object(profile).to_string())
                .map_err(|e| e.to_string())?;
            interrupt_session(ws, session, "Profile changed")?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/documents/download" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("document not found")?;
            document_reply(stream, &bytes);
        }
        "/api/email-draft" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            if ws.has_duplicate_hold(&id).map_err(|e| e.to_string())? {
                return Err(
                    "Review possible duplicate jobs before preparing an email application".into(),
                );
            }
            let job = ws
                .job(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            if !matches!(
                job.state,
                ApplicationState::Shortlisted | ApplicationState::Prepared
            ) {
                return Err("Shortlist the job before creating an email draft".into());
            }
            let selected = body["documents"].as_array().ok_or("missing documents")?;
            if selected.len() > 5 {
                return Err("Choose at most five PDF files".into());
            }
            let documents = ws.documents().map_err(|e| e.to_string())?;
            let mut attachments = Vec::new();
            let mut total = 0usize;
            for value in selected {
                let doc_id = value.as_str().ok_or("invalid document id")?;
                if attachments
                    .iter()
                    .any(|(id, _, _): &(String, String, Vec<u8>)| id == doc_id)
                {
                    return Err("Duplicate attachment".into());
                }
                let doc = documents
                    .iter()
                    .find(|doc| doc.id == doc_id)
                    .ok_or("document not found")?;
                total = total.checked_add(doc.size).ok_or("Attachments too large")?;
                if total > 10 * 1024 * 1024 {
                    return Err("Attachments exceed 10 MiB".into());
                }
                let bytes = ws
                    .document_content(doc_id)
                    .map_err(|e| e.to_string())?
                    .ok_or("document not found")?;
                attachments.push((doc_id.to_owned(), doc.name.clone(), bytes));
            }
            let recipient = body["recipient"].as_str().ok_or("missing recipient")?;
            let subject = body["subject"].as_str().ok_or("missing subject")?;
            let text = body["message"].as_str().ok_or("missing message")?;
            let pairs: Vec<_> = attachments
                .iter()
                .map(|(_, name, bytes)| (name.clone(), bytes.clone()))
                .collect();
            let attempt_id = random_id()?;
            let draft = email_draft::build(recipient, subject, text, &pairs, &attempt_id)?;
            let mut record = attempt_record::AttemptRecord::start(ws, &id, &attempt_id, now())?
                .ok_or("Email draft requires a disk-backed workspace")?;
            for (_, name, bytes) in &attachments {
                record.material(name, bytes)?;
            }
            record.email_draft(&draft)?;
            record.event(json!({"type":"email_draft_created", "time":now(), "recipient":recipient, "subject":subject, "attachments":attachments.len(), "sent":false}))?;
            document_reply(stream, &draft);
        }
        "/api/documents/convert-pdf-start" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let doc = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|d| d.id == id)
                .ok_or("document not found")?;
            let lower = doc.name.to_ascii_lowercase();
            let image =
                lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg");
            if !lower.ends_with(".docx") && !image {
                return Err("Only DOCX, PNG and JPEG conversion is supported".into());
            }
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("document not found")?;
            let name = format!(
                "{}.pdf",
                doc.name
                    .rsplit_once('.')
                    .map(|(stem, _)| stem)
                    .unwrap_or("document")
            );
            let task_id = random_id()?;
            convert_task::start(task_id.clone(), name, bytes, image)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/convert-pdf-status" => {
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", convert_task::poll(task_id, ws)?);
        }
        "/api/documents/template-suggest-start" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let job = JobIdentityId::new(body["job_id"].as_str().ok_or("missing job")?)
                .map_err(|e| e.to_string())?;
            let before = body["before"].as_str().ok_or("missing passage")?;
            let input = template_suggestion_inputs(ws, id, &job, before)?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            template_suggest::start(task_id.clone(), model, input)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/template-suggest-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = template_suggest::poll(id)?;
            if result["status"] == "complete" {
                let current = result["document_id"]
                    .as_str()
                    .zip(result["job_id"].as_str())
                    .zip(result["before"].as_str())
                    .and_then(|((doc, job), before)| {
                        JobIdentityId::new(job)
                            .ok()
                            .and_then(|id| template_suggestion_inputs(ws, doc, &id, before).ok())
                    });
                if current.as_ref().map(|input| input.digest.as_str()) != result["digest"].as_str()
                {
                    result = json!({"status":"failed","error":"The document, job or supporting information changed; suggest again"});
                }
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/documents/edit-template-start" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Review the replacement before exporting".into());
            }
            let id = body["id"].as_str().ok_or("missing document id")?;
            let info = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|d| d.id == id)
                .ok_or("Document not found")?;
            if !info.name.to_ascii_lowercase().ends_with(".docx")
                && !info.name.to_ascii_lowercase().ends_with(".pdf")
            {
                return Err("Choose a PDF or DOCX template".into());
            }
            let source = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("Document not found")?;
            let edits = if let Some(items) = body.get("edits") {
                let items = items.as_array().ok_or("Invalid changes")?;
                if items.is_empty() || items.len() > 5 {
                    return Err("Queue between one and five changes".into());
                }
                items
                    .iter()
                    .map(|item| {
                        Ok((
                            item["before"]
                                .as_str()
                                .ok_or("missing original text")?
                                .to_owned(),
                            item["after"]
                                .as_str()
                                .ok_or("missing replacement text")?
                                .to_owned(),
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?
            } else {
                vec![(
                    body["before"]
                        .as_str()
                        .ok_or("missing original text")?
                        .to_owned(),
                    body["after"]
                        .as_str()
                        .ok_or("missing replacement text")?
                        .to_owned(),
                )]
            };
            let task_id = random_id()?;
            docx_edit_task::start_batch(task_id.clone(), id.to_owned(), info.name, source, edits)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/edit-template-status" => {
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", docx_edit_task::poll(task_id, ws)?);
        }
        "/api/pdf-setup/start" => {
            let python = body["python"]
                .as_str()
                .ok_or("Choose a Python executable")?;
            let id = random_id()?;
            pdf_setup::start(id.clone(), std::path::PathBuf::from(python))?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/pdf-setup/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", pdf_setup::poll(id)?);
        }
        "/api/documents/preview-start" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let page = body["page"].as_u64().ok_or("missing page number")?;
            if page >= 30 {
                return Err("Page number exceeds 30-page limit".into());
            }
            let info = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|doc| doc.id == id)
                .ok_or("document not found")?;
            if !info.name.to_ascii_lowercase().ends_with(".pdf") {
                return Err("Choose a PDF".into());
            }
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("document not found")?;
            let task_id = random_id()?;
            pdf_preview_task::start(task_id.clone(), bytes, page as usize)?;
            json_reply(stream, "200 OK", json!({"task_id": task_id}));
        }
        "/api/documents/preview-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", pdf_preview_task::poll(id)?);
        }
        "/api/documents/extract-start" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let info = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|doc| doc.id == id)
                .ok_or("document not found")?;
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("document not found")?;
            let task_id = random_id()?;
            extract_task::start(task_id.clone(), info.name.clone(), bytes)?;
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":task_id,"id":id,"name":info.name}),
            );
        }
        "/api/documents/extract-status" => {
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", extract_task::poll(task_id)?);
        }
        "/api/documents/extract" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let info = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|d| d.id == id)
                .ok_or("document not found")?;
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("document not found")?;
            let text = document_extract::extract(&info.name, &bytes)?;
            json_reply(
                stream,
                "200 OK",
                json!({"id":id,"name":info.name,"text":text}),
            );
        }
        "/api/documents/suggest" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let info = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|doc| doc.id == id)
                .ok_or("document not found")?;
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("document not found")?;
            let model =
                resident_model()?.ok_or("No eligible already-loaded local model is available")?;
            let task_id = random_id()?;
            doc_suggest::start(task_id.clone(), model, info.name, bytes)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/suggest-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", doc_suggest::poll(id)?);
        }
        "/api/shortlist" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            ws.set_state(&id, ApplicationState::Shortlisted)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/review" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let required = body["required"].as_bool().ok_or("missing review status")?;
            if required {
                ws.mark_manual_review(&id, "Manual review requested", now())
                    .map_err(|e| e.to_string())?;
            } else {
                ws.clear_manual_review(&id).map_err(|e| e.to_string())?;
            }
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/form/inspect-start" => {
            if application_check::busy()
                || form_task::busy()
                || candidate_task::busy()
                || form_links::busy()
            {
                return Err("Wait for the current browser inspection to finish".into());
            }
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let job = ws
                .job(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            if !matches!(
                job.state,
                ApplicationState::Shortlisted | ApplicationState::Prepared
            ) {
                return Err("Shortlist the job before inspecting its application".into());
            }
            waypoint_workspace::default_registry()
                .check_public_route(&job.source, waypoint_policy::Operation::Read)
                .map_err(|e| e.to_string())?;
            let url = job.canonical_url.ok_or("Job has no application URL")?;
            if !url.starts_with("https://") || !waypoint_connectors::is_public_host_url(&url) {
                return Err("Application URL is not a public HTTPS URL".into());
            }
            let visible = body["visible"].as_bool().unwrap_or(false);
            let model = match resident_model() {
                Ok(model) => model,
                Err(_) if visible => None,
                Err(error) => return Err(error),
            };
            if model.is_none() && !visible {
                return Err("No single fully GPU-resident Ollama model is available within the 14 GiB budget; select visible browser for manual review".into());
            }
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None if visible && model.is_none() => json!({}),
                None => return Err("Save your personal details before inspecting a form".into()),
            };
            interrupt_session(ws, session, "Another application was opened")?;
            let task_id = random_id()?;
            form_task::start(task_id.clone(), id, url, profile, model, visible)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/inspect-status" => {
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            let Some(finished) = form_task::poll(task_id)? else {
                json_reply(stream, "200 OK", json!({"status":"running"}));
                return Ok(());
            };
            let form_task::Finished {
                job_id,
                url,
                profile_digest,
                visible,
                result,
            } = finished;
            let (mut driver, mut preview) = match result {
                Ok(value) => value,
                Err(error) => {
                    ws.mark_manual_review(&job_id, "Application needs manual review", now())
                        .map_err(|e| e.to_string())?;
                    let record_error = match random_id().and_then(|id| {
                        attempt_record::AttemptRecord::start(ws, &job_id, &id, now())
                    }) {
                        Ok(Some(mut record)) => record
                            .inspection("", std::slice::from_ref(&error), None, now())
                            .err(),
                        Ok(None) => Some("Attempt record requires a disk-backed workspace".into()),
                        Err(problem) => Some(problem),
                    };
                    let error = if let Some(problem) = record_error {
                        format!("{error}; attempt record unavailable: {problem}")
                    } else {
                        error
                    };
                    json_reply(stream, "200 OK", json!({"status":"failed","error":error}));
                    return Ok(());
                }
            };
            let job = ws
                .job(&job_id)
                .map_err(|e| e.to_string())?
                .ok_or("Job no longer exists")?;
            if !matches!(
                job.state,
                ApplicationState::Shortlisted | ApplicationState::Prepared
            ) || job.canonical_url.as_deref() != Some(url.as_str())
            {
                return Err("Job or destination changed; inspect again".into());
            }
            if !preview.manual_review.is_empty() {
                ws.mark_manual_review(&job_id, "Application needs manual review", now())
                    .map_err(|e| e.to_string())?;
            }
            let ttl = if visible {
                VISIBLE_SESSION_TTL
            } else {
                SESSION_TTL
            };
            let session_id = random_id()?;
            let screenshot = driver.screenshot_base64().ok();
            if screenshot.is_none() {
                preview
                    .manual_review
                    .push("Inspection screenshot is unavailable".into());
            }
            let links = form_links::candidates(&mut driver).unwrap_or_default();
            let mut record = match random_id()
                .and_then(|id| attempt_record::AttemptRecord::start(ws, &job_id, &id, now()))
            {
                Ok(record) => record,
                Err(error) => {
                    preview
                        .manual_review
                        .push(format!("Attempt record unavailable: {error}"));
                    None
                }
            };
            if let Some(record) = &mut record {
                if let Err(error) = record.inspection(
                    &preview.schema.origin,
                    &preview.manual_review,
                    screenshot.as_deref(),
                    now(),
                ) {
                    preview
                        .manual_review
                        .push(format!("Inspection evidence incomplete: {error}"));
                }
            }
            if !preview.manual_review.is_empty() {
                ws.mark_manual_review(&job_id, "Application needs manual review", now())
                    .map_err(|e| e.to_string())?;
            }
            json_reply(
                stream,
                "200 OK",
                json!({"status":"complete","fields":preview.schema.fields,"suggestions":preview.suggestions,"manual_review":preview.manual_review,"session_id":session_id,"origin":preview.schema.origin,"expires_in_seconds":ttl.as_secs(),"visible":visible,"screenshot":screenshot,"links":links}),
            );
            *session = Some(ReviewSession {
                id: session_id,
                job_id,
                url,
                profile_digest,
                preview,
                driver,
                created: Instant::now(),
                ttl,
                filled: false,
                visible,
                staged_files: vec![],
                record,
            });
        }
        "/api/form/open-link-start" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Review the destination before opening it".into());
            }
            let requested = body["session_id"].as_str().ok_or("missing session")?;
            let target = body["target"]
                .as_str()
                .filter(|s| s.len() <= 2048)
                .ok_or("missing or invalid target")?;
            let current = session
                .as_ref()
                .ok_or("Form session expired; inspect again")?;
            if current.id != requested || current.created.elapsed() > current.ttl {
                return Err("Form session expired or does not match".into());
            }
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            let task_id = random_id()?;
            let current = session.take().ok_or("Form session expired")?;
            if let Err((current, error)) =
                form_links::start(task_id.clone(), current, Some(target.to_owned()), profile)
            {
                *session = Some(*current);
                return Err(error);
            }
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/open-link-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            match form_links::poll(id)? {
                None => json_reply(stream, "200 OK", json!({"status":"running"})),
                Some(Err(error)) => {
                    json_reply(stream, "200 OK", json!({"status":"failed","error":error}))
                }
                Some(Ok(mut current)) => {
                    let job = ws.job(&current.job_id).map_err(|e| e.to_string())?;
                    let job_unchanged = job.as_ref().is_some_and(|job| {
                        matches!(
                            job.state,
                            ApplicationState::Shortlisted | ApplicationState::Prepared
                        ) && job.canonical_url.as_deref() == Some(current.url.as_str())
                    });
                    let profile = match ws.candidate_profile().map_err(|e| e.to_string())? {
                        Some(text) => serde_json::from_str::<Value>(&text).ok(),
                        None => Some(json!({})),
                    };
                    let profile_unchanged = profile.is_some_and(|profile| {
                        Sha256Digest::of(&profile.to_string()) == current.profile_digest
                    });
                    if !job_unchanged || !profile_unchanged {
                        current.preview.suggestions.clear();
                        current.preview.manual_review.push(
                            "The job or profile changed during inspection; inspect again".into(),
                        );
                    }
                    if !current.preview.manual_review.is_empty() {
                        ws.mark_manual_review(
                            &current.job_id,
                            "Application needs manual review",
                            now(),
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    let links = form_links::candidates(&mut current.driver).unwrap_or_default();
                    let screenshot = current.driver.screenshot_base64().ok();
                    if screenshot.is_none() {
                        current
                            .preview
                            .manual_review
                            .push("Inspection screenshot is unavailable".into());
                        ws.mark_manual_review(
                            &current.job_id,
                            "Application needs manual review",
                            now(),
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    if let Some(record) = &mut current.record {
                        if let Err(error) = record.inspection(
                            &current.preview.schema.origin,
                            &current.preview.manual_review,
                            screenshot.as_deref(),
                            now(),
                        ) {
                            current
                                .preview
                                .manual_review
                                .push(format!("Inspection evidence incomplete: {error}"));
                            ws.mark_manual_review(
                                &current.job_id,
                                "Application needs manual review",
                                now(),
                            )
                            .map_err(|e| e.to_string())?;
                        }
                    }
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"complete","fields":current.preview.schema.fields,"suggestions":current.preview.suggestions,"manual_review":current.preview.manual_review,"session_id":current.id,"origin":current.preview.schema.origin,"expires_in_seconds":current.ttl.as_secs(),"visible":current.visible,"screenshot":screenshot,"links":links}),
                    );
                    *session = Some(current);
                }
            }
        }
        "/api/form/rescan-start" => {
            let requested = body["session_id"].as_str().ok_or("missing session")?;
            let current = session
                .as_ref()
                .ok_or("Form session expired; inspect again")?;
            if current.id != requested || current.created.elapsed() > current.ttl {
                return Err("Form session expired or does not match".into());
            }
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            let task_id = random_id()?;
            let current = session.take().ok_or("Form session expired")?;
            if let Err((current, error)) =
                form_links::start(task_id.clone(), current, None, profile)
            {
                *session = Some(*current);
                return Err(error);
            }
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/answer-start" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"].as_str().ok_or("missing field")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id)
                .ok_or("Form session expired")?;
            check_answer_page(current)?;
            let input = answer_inputs(ws, current, selector)?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            answer_task::start(task_id.clone(), model, input)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/answer-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = answer_task::poll(id)?;
            if result["status"] == "complete" {
                let current = session
                    .as_mut()
                    .filter(|s| Some(s.id.as_str()) == result["session_id"].as_str());
                let fresh = current.and_then(|s| {
                    if check_answer_page(s).is_err() {
                        return None;
                    }
                    result["selector"]
                        .as_str()
                        .and_then(|selector| answer_inputs(ws, s, selector).ok())
                });
                if fresh.as_ref().map(|input| input.digest.as_str()) != result["digest"].as_str() {
                    result = json!({"status":"failed","error":"The form or supporting information changed; inspect and draft again"});
                }
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/form/fill" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Explicit review is required".into());
            }
            let requested = body["session_id"].as_str().ok_or("missing session")?;
            let current = session
                .as_ref()
                .ok_or("Form session expired; inspect it again")?;
            if ws
                .has_duplicate_hold(&current.job_id)
                .map_err(|e| e.to_string())?
            {
                return Err(
                    "Review possible duplicate jobs before filling this application".into(),
                );
            }
            if current.id != requested {
                return Err("Form session does not match".into());
            }
            if current.filled {
                return Err(
                    "This session has already been filled; inspect again to start over".into(),
                );
            }
            let mut current = session.take().ok_or("Form session expired")?;
            if current.created.elapsed() > current.ttl {
                return Err("Form session expired; inspect it again".into());
            }
            if let Some(selection) = body.get("selectors") {
                let selected = selection
                    .as_array()
                    .ok_or("Selected fields must be a list")?;
                let mut selectors = std::collections::HashSet::new();
                for value in selected {
                    let Some(selector) = value.as_str() else {
                        *session = Some(current);
                        return Err("Invalid selected field".into());
                    };
                    if !selectors.insert(selector)
                        || !current
                            .preview
                            .suggestions
                            .iter()
                            .any(|m| m.field_selector == selector)
                    {
                        *session = Some(current);
                        return Err("Selected fields contain a duplicate or unknown field".into());
                    }
                }
                if selectors.len() < current.preview.suggestions.len() {
                    if let Err(error) = ws.mark_manual_review(
                        &current.job_id,
                        "Application needs manual review",
                        now(),
                    ) {
                        *session = Some(current);
                        return Err(error.to_string());
                    }
                    current
                        .preview
                        .manual_review
                        .push("Some suggested fields were left for manual review".into());
                }
                current
                    .preview
                    .suggestions
                    .retain(|m| selectors.contains(m.field_selector.as_str()));
            }
            if let Some(answers) = body.get("answers") {
                match reviewed_form_answers(answers, &current.preview) {
                    Ok(answers) => current.preview.suggestions.extend(answers),
                    Err(error) => {
                        *session = Some(current);
                        return Err(error);
                    }
                }
            }
            if current.preview.suggestions.is_empty() {
                *session = Some(current);
                return Err("Select at least one field to fill".into());
            }
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(profile) => {
                    serde_json::from_str(&profile).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            if Sha256Digest::of(&profile.to_string()) != current.profile_digest {
                return Err("Profile changed; inspect the form again".into());
            }
            let intent =
                IntentId::new(format!("prefill-{}", current.id)).map_err(|e| e.to_string())?;
            let grant_id = GrantId::new(random_id()?).map_err(|e| e.to_string())?;
            let payload =
                serde_json::to_string(&current.preview.suggestions).map_err(|e| e.to_string())?;
            let mut grant = ApprovalGrant {
                id: grant_id,
                intent_id: IntentId::new(intent.as_str()).map_err(|e| e.to_string())?,
                bound_payload_sha256: Sha256Digest::of(&payload),
                profile_revision: 0,
                packet_sha256: Sha256Digest::of(&profile.to_string()),
                form_schema_sha256: Sha256Digest::of(&current.preview.schema.schema_hash_input()),
                destination_origin: current.preview.schema.origin.clone(),
                operation: GrantOperation::Prefill,
                nonce: current.id.clone(),
                explicit_user_gesture: true,
                consumed: false,
                expired: false,
            };
            if current.record.is_none() {
                current.record = attempt_record::AttemptRecord::start(
                    ws,
                    &current.job_id,
                    &random_id()?,
                    now(),
                )?;
            }
            if let Some(record) = &mut current.record {
                record.event(json!({"type":"fields_requested","time":now(),"origin":current.preview.schema.origin,"answers":current.preview.suggestions}))?;
            }
            current.filled = true;
            let result = form_preview::apply_reviewed_text_fields(
                ws,
                &current.job_id,
                &mut current.driver,
                &current.url,
                &current.preview,
                &mut grant,
                form_preview::FillContext {
                    intent: &intent,
                    packet_digest: &current.profile_digest,
                    source_reviewed: true,
                    timestamp: now(),
                },
            );
            let filled = match result {
                Ok(count) => count,
                Err(error) => {
                    if let Some(record) = &mut current.record {
                        let _ = record
                            .event(json!({"type":"fill_incomplete","time":now(),"error":error}));
                        if let Ok(image) = current.driver.screenshot_base64() {
                            let _ = record.screenshot(&image);
                        }
                    }
                    ws.mark_manual_review(
                        &current.job_id,
                        "Application needs manual review",
                        now(),
                    )
                    .map_err(|e| e.to_string())?;
                    current.created = Instant::now();
                    *session = Some(current);
                    return Err(format!(
                        "Autofill could be incomplete; review the open browser: {error}"
                    ));
                }
            };
            let screenshot = match current.driver.screenshot_base64() {
                Ok(image) => image,
                Err(error) => {
                    ws.mark_manual_review(
                        &current.job_id,
                        "Application needs manual review",
                        now(),
                    )
                    .map_err(|e| e.to_string())?;
                    current.created = Instant::now();
                    *session = Some(current);
                    return Err(format!(
                        "Fields were filled, but the review screenshot failed: {error}"
                    ));
                }
            };
            if let Some(record) = &mut current.record {
                if let Err(error) = record
                    .screenshot(&screenshot)
                    .and_then(|_| {
                        record.event(json!({"type":"fields_filled","time":now(),"count":filled}))
                    })
                    .and_then(|_| attempt_record::capture_page(&mut current.driver, record, now()))
                {
                    let _ = record
                        .event(json!({"type":"capture_incomplete","time":now(),"error":error}));
                    current
                        .preview
                        .manual_review
                        .push("Attempt screenshot capture was incomplete".into());
                    ws.mark_manual_review(
                        &current.job_id,
                        "Application needs manual review",
                        now(),
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            current.created = Instant::now();
            let (filled_fields, required_not_filled) =
                form_preview::fill_coverage(&current.preview);
            json_reply(
                stream,
                "200 OK",
                json!({"filled":filled,"filled_fields":filled_fields,"required_not_filled":required_not_filled,"screenshot":screenshot,"manual_review":current.preview.manual_review,"expires_in_seconds":current.ttl.as_secs()}),
            );
            *session = Some(current);
        }
        "/api/form/attach" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Explicit document review is required".into());
            }
            let requested = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"].as_str().ok_or("missing file field")?;
            let document_id = body["document_id"].as_str().ok_or("missing document")?;
            let current = session
                .as_mut()
                .ok_or("Form session expired; inspect again")?;
            if ws
                .has_duplicate_hold(&current.job_id)
                .map_err(|e| e.to_string())?
            {
                return Err("Review possible duplicate jobs before attaching documents".into());
            }
            if current.id != requested || current.created.elapsed() > current.ttl {
                return Err("Form session expired or does not match".into());
            }
            if current.staged_files.len() >= 5 {
                return Err("This session has reached its five-document limit".into());
            }
            let job = ws
                .job(&current.job_id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            if !matches!(
                job.state,
                ApplicationState::Shortlisted | ApplicationState::Prepared
            ) || job.canonical_url.as_deref() != Some(&current.url)
            {
                return Err("Job or destination changed; inspect again".into());
            }
            let live = current
                .driver
                .inspect_current()
                .map_err(|e| e.to_string())?;
            if !live.walls.is_empty()
                || live.schema != current.preview.schema
                || live.origin != current.preview.schema.origin
            {
                return Err("Form changed or requires manual attention; inspect again".into());
            }
            form_links::check_job_and_destination(
                &job.source,
                &live.origin,
                waypoint_policy::Operation::Upload,
            )?;
            let field = live
                .schema
                .fields
                .iter()
                .find(|field| field.selector == selector)
                .ok_or("Unknown file field")?;
            if field.sensitive
                || !matches!(
                    field.field_type,
                    waypoint_applications::form_mapping::FormFieldType::File
                )
            {
                return Err("Selected field cannot receive a document".into());
            }
            let doc = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|doc| doc.id == document_id)
                .ok_or("Document not found")?;
            let bytes = ws
                .document_content(document_id)
                .map_err(|e| e.to_string())?
                .ok_or("Document not found")?;
            if bytes.is_empty() || bytes.len() > MAX_DOCUMENT {
                return Err("Document size is outside the permitted range".into());
            }
            if !email_draft::is_pdf_attachment(&doc.name, &bytes) {
                return Err(
                    "Choose a PDF. Convert DOCX, PNG or JPEG files to PDF and review them before attaching"
                        .into(),
                );
            }
            if current.record.is_none() {
                current.record = attempt_record::AttemptRecord::start(
                    ws,
                    &current.job_id,
                    &random_id()?,
                    now(),
                )?;
            }
            if let Some(record) = &mut current.record {
                let stored = record.material(&doc.name, &bytes)?;
                record.event(json!({"type":"document_requested","time":now(),"selector":selector,"name":doc.name,"material":stored}))?;
            }
            let name: String = doc
                .name
                .chars()
                .map(|c| {
                    if c.is_control()
                        || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                    {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            let name = if name.trim_matches('.').trim().is_empty() {
                "document"
            } else {
                name.as_str()
            };
            let dir = tempfile::Builder::new()
                .prefix("waypoint-document-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            let path = dir.path().join(name);
            std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
            let grant_id = random_id()?;
            let payload = json!({"selector":selector,"document_id":document_id,"document_sha256":waypoint_domain::sha256_hex(&bytes)}).to_string();
            ws.record_consumed_prefill_grant(&waypoint_store::prefill_grants::PrefillGrantRecord {
                id: grant_id.clone(),
                job_id: current.job_id.as_str().into(),
                intent_id: format!("document-{grant_id}"),
                payload_hash: Sha256Digest::of(&payload).as_str().into(),
                profile_hash: current.profile_digest.as_str().into(),
                schema_hash: Sha256Digest::of(&live.schema.schema_hash_input())
                    .as_str()
                    .into(),
                destination: live.origin,
                created_at: now(),
            })
            .map_err(|e| e.to_string())?;
            current.staged_files.push(dir);
            if let Err(error) = current.driver.attach_local_file(selector, &path) {
                if let Some(record) = &mut current.record {
                    let _ = record.event(json!({"type":"attachment_incomplete","time":now(),"selector":selector,"error":error.to_string()}));
                }
                ws.mark_manual_review(&current.job_id, "Application needs manual review", now())
                    .map_err(|e| e.to_string())?;
                return Err(format!(
                    "Document attachment may be incomplete; review the form: {error}"
                ));
            }
            if let Some(record) = &mut current.record {
                record.event(json!({"type":"document_attached","time":now(),"selector":selector,"name":doc.name}))?;
                if let Ok(image) = current.driver.screenshot_base64() {
                    record.screenshot(&image)?;
                }
                if let Err(error) = attempt_record::capture_page(&mut current.driver, record, now())
                {
                    let _ = record
                        .event(json!({"type":"capture_incomplete","time":now(),"error":error}));
                    ws.mark_manual_review(
                        &current.job_id,
                        "Application needs manual review",
                        now(),
                    )
                    .map_err(|e| e.to_string())?;
                    current
                        .preview
                        .manual_review
                        .push("Attempt screenshot capture was incomplete".into());
                }
            }
            current.created = Instant::now();
            json_reply(
                stream,
                "200 OK",
                json!({"ok":true,"name":doc.name,"manual_review":current.preview.manual_review}),
            );
        }
        "/api/form/close" => {
            let requested = body["session_id"].as_str().ok_or("missing session")?;
            if session.as_ref().is_none_or(|s| s.id != requested) {
                return Err("Form session expired or does not match".into());
            }
            if let Some(current) = session.as_mut() {
                if let Some(record) = &mut current.record {
                    record.event(json!({"type":"review_session_closed","time":now(),"submitted_by_waypoint":false}))?;
                    record.status("review_session_closed", now())?;
                }
            }
            *session = None;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/letter/export-start" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Review the letter before exporting".into());
            }
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let job = ws
                .job(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            if !matches!(
                job.state,
                ApplicationState::Shortlisted | ApplicationState::Prepared
            ) {
                return Err("Shortlist the job before exporting".into());
            }
            let kind = body["kind"].as_str().unwrap_or("cover_letter");
            if !matches!(kind, "cv" | "cover_letter") {
                return Err("Unknown document type".into());
            }
            let text = body["text"]
                .as_str()
                .ok_or("missing document text")?
                .to_owned();
            let task_id = random_id()?;
            letter_export::start(
                task_id.clone(),
                id.as_str().to_owned(),
                kind.to_owned(),
                text,
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/letter/export-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", letter_export::poll(id, ws)?);
        }
        "/api/draft/start" => {
            let kind = body["kind"].as_str().unwrap_or("cover_letter");
            if !matches!(kind, "cover_letter" | "cv") {
                return Err("Unknown draft type".into());
            }
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let job = ws
                .job(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            if !matches!(
                job.state,
                ApplicationState::Shortlisted | ApplicationState::Prepared
            ) {
                return Err("Shortlist the job before drafting".into());
            }
            let claims = ws.facts().map_err(|e| e.to_string())?;
            let mut facts: Vec<String> = claims
                .into_iter()
                .filter(|f| !f.requires_review && f.text.len() <= 1000)
                .take(if kind == "cv" { 18 } else { 25 })
                .map(|f| f.text)
                .collect();
            if facts.is_empty() {
                return Err("Add verified qualifications before drafting".into());
            }
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            if kind == "cv" {
                for (key, label) in [
                    ("first_name", "First name"),
                    ("last_name", "Last name"),
                    ("email", "Email"),
                    ("phone", "Phone"),
                    ("city", "City"),
                    ("linkedin", "LinkedIn"),
                    ("website", "Website"),
                ] {
                    if let Some(value) = profile[key]
                        .as_str()
                        .filter(|value| !value.trim().is_empty() && value.len() <= 500)
                    {
                        facts.push(format!("{label}: {value}"));
                    }
                }
            }
            let instructions = profile["document_instructions"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(4000)
                .collect::<String>();
            let template_key = if kind == "cv" {
                "cv_template_id"
            } else {
                "cover_template_id"
            };
            let template =
                if let Some(id) = profile[template_key].as_str().filter(|s| !s.is_empty()) {
                    let doc = ws
                        .documents()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|doc| doc.id == id)
                        .ok_or("Selected template is unavailable")?;
                    let bytes = ws
                        .document_content(id)
                        .map_err(|e| e.to_string())?
                        .ok_or("Selected template is unavailable")?;
                    Some((doc.name, bytes))
                } else {
                    None
                };
            let description = ws
                .job_description(&id)
                .map_err(|e| e.to_string())?
                .unwrap_or_default()
                .chars()
                .take(8000)
                .collect::<String>();
            let job_info = json!({"title":job.title,"employer":job.employer,"location":job.location,"description":description});
            let model =
                resident_model()?.ok_or("No eligible already-loaded local model is available")?;
            let task_id = random_id()?;
            draft_task::start(
                task_id.clone(),
                model,
                draft_task::Inputs {
                    job: job_info,
                    kind: kind.to_owned(),
                    facts,
                    instructions,
                    template,
                },
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/draft/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", draft_task::poll(id)?);
        }
        "/api/packet/read" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let packet = ws
                .packet(&id)
                .map_err(|e| e.to_string())?
                .ok_or("No draft exists for this job")?;
            let readiness = ws
                .packet_readiness(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Draft readiness unavailable")?;
            json_reply(
                stream,
                "200 OK",
                json!({"body":packet.body,"supported_spans":readiness.supported_spans,"unsupported":readiness.unsupported,"invalidated_at":readiness.invalidated_at,"profile_revision":packet.profile_revision}),
            );
        }
        "/api/packet/prepare" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            // A preparation request must never silently replace an existing draft.
            if ws.packet(&id).map_err(|e| e.to_string())?.is_some() {
                return Err("A draft already exists for this job".into());
            }
            let packet = ws.prepare_packet(&id, now()).map_err(|e| e.to_string())?;
            json_reply(
                stream,
                "200 OK",
                json!({"body":packet.body,"supported_spans":packet.supported_spans,"unsupported":packet.unsupported,"profile_revision":packet.profile_revision,"invalidated_at":null}),
            );
        }
        _ => return Err("unknown endpoint".into()),
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = storage_settings::load_at(
        &storage_settings::control_dir()?,
        &storage_settings::default_workspace()?,
    )?;
    #[cfg(all(feature = "sqlcipher", windows))]
    let ws = Workspace::open_encrypted(&data)?;
    #[cfg(not(all(feature = "sqlcipher", windows)))]
    let ws = Workspace::open(&data)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("token generation failed: {e}"))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let url = format!("http://127.0.0.1:{port}/#{token}");
    println!("Waypoint: {url}");
    listener.set_nonblocking(true)?;
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = std::thread::spawn(move || serve_loop(ws, listener, token, port, worker_stop));
    if std::env::var_os("WAYPOINT_NO_BROWSER").is_none() {
        let _ = webbrowser::open(&url);
    }
    #[cfg(windows)]
    {
        if std::env::var_os("WAYPOINT_NO_TRAY").is_none() {
            return tray::run(url, stop, worker).map_err(Into::into);
        }
    }
    let _ = worker.join();
    Ok(())
}

fn serve_loop(
    mut ws: Workspace,
    listener: TcpListener,
    token: String,
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut session: Option<ReviewSession> = None;
    let mut duplicate_cursor = String::new();
    let mut next_duplicate_index = Instant::now();
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        if session
            .as_ref()
            .is_some_and(|s| s.created.elapsed() > s.ttl)
        {
            if let Err(error) = interrupt_session(&mut ws, &mut session, "Review session expired") {
                eprintln!("Could not record expired review session: {error}");
                session = None;
            }
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(error) = serve(&mut stream, &mut ws, &mut session, &token, port) {
                    json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= next_duplicate_index {
                    match ws.index_next_duplicate_job(&duplicate_cursor) {
                        Ok(Some(id)) => {
                            duplicate_cursor = id;
                            next_duplicate_index = Instant::now() + Duration::from_secs(2);
                        }
                        Ok(None) => {
                            duplicate_cursor.clear();
                            next_duplicate_index = Instant::now() + Duration::from_secs(30);
                        }
                        Err(error) => {
                            eprintln!("Duplicate indexing paused: {error}");
                            duplicate_cursor.clear();
                            next_duplicate_index = Instant::now() + Duration::from_secs(30);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(200))
            }
            Err(error) => {
                eprintln!("Connection failed: {error}");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_comparison_is_invalid_after_a_user_decision() {
        let mut ws = Workspace::in_memory().unwrap();
        let first = manual_job::add(&mut ws, &json!({"url":"https://example.org/one","title":"Senior Engineer","employer":"Acme Inc"}), 1).unwrap();
        let second = manual_job::add(
            &mut ws,
            &json!({"url":"https://example.net/two","title":"Sr Engineer","employer":"Acme Ltd"}),
            2,
        )
        .unwrap();
        let first = JobIdentityId::new(first["id"].as_str().unwrap()).unwrap();
        let second = JobIdentityId::new(second["id"].as_str().unwrap()).unwrap();
        assert!(duplicate_inputs(&ws, &first, &second).is_ok());
        assert!(ws.decide_duplicate(&first, &second, false).unwrap());
        assert!(duplicate_inputs(&ws, &first, &second).is_err());
    }

    #[test]
    fn reviewed_answers_require_known_non_sensitive_fields_and_unique_options() {
        use waypoint_applications::form_mapping::{FormField, FormFieldType, FormSchema};
        let preview = form_preview::FormPreview {
            schema: FormSchema {
                origin: "https://example.org".into(),
                multi_step: false,
                steps: 1,
                fields: vec![
                    FormField {
                        selector: "#question".into(),
                        label: "Why this role?".into(),
                        field_type: FormFieldType::TextArea,
                        required: true,
                        sensitive: false,
                    },
                    FormField {
                        selector: "#country".into(),
                        label: "Country".into(),
                        field_type: FormFieldType::Select {
                            options: vec!["Germany".into(), "France".into()],
                        },
                        required: true,
                        sensitive: false,
                    },
                    FormField {
                        selector: "#visa".into(),
                        label: "Work authorization".into(),
                        field_type: FormFieldType::Text,
                        required: false,
                        sensitive: true,
                    },
                    FormField {
                        selector: "#start".into(),
                        label: "Available date".into(),
                        field_type: FormFieldType::Date,
                        required: false,
                        sensitive: false,
                    },
                ],
            },
            suggestions: vec![],
            manual_review: vec![],
        };
        assert_eq!(reviewed_form_answers(&json!([{"selector":"#question","value":"Research interests"},{"selector":"#country","value":"Germany"}]), &preview).unwrap().len(), 2);
        assert!(
            reviewed_form_answers(&json!([{"selector":"#visa","value":"Yes"}]), &preview).is_err()
        );
        assert!(reviewed_form_answers(
            &json!([{"selector":"#country","value":"Unknown"}]),
            &preview
        )
        .is_err());
        assert!(reviewed_form_answers(
            &json!([{"selector":"#question","value":"A"},{"selector":"#question","value":"B"}]),
            &preview
        )
        .is_err());
        assert!(reviewed_form_answers(
            &json!([{"selector":"#start","value":"2028-02-29"}]),
            &preview
        )
        .is_ok());
        assert!(reviewed_form_answers(
            &json!([{"selector":"#start","value":"2029-02-29"}]),
            &preview
        )
        .is_err());
    }

    #[test]
    fn fit_input_digest_changes_when_preferences_change() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let (_, _, _, before) = fit_inputs(&ws, &id).unwrap();
        ws.save_candidate_profile(&json!({"instructions":"Remote only"}).to_string())
            .unwrap();
        let (_, _, _, after) = fit_inputs(&ws, &id).unwrap();
        assert_ne!(before, after);
    }

    #[cfg(windows)]
    #[test]
    fn saved_fit_is_not_reported_current_after_preferences_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let (_, _, _, digest) = fit_inputs(&ws, &id).unwrap();
        let saved = json!({"input_digest":digest,"assessment":{"score":73},"model":"fixture"});
        ws.save_fit_assessment(&id, saved.to_string().as_bytes())
            .unwrap();
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "current");
        ws.save_candidate_profile(&json!({"instructions":"Remote only"}).to_string())
            .unwrap();
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "stale");
    }

    #[test]
    fn unresolved_duplicate_blocks_email_draft() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            let first = manual_job::add(&mut ws, &json!({"url":"https://example.org/one","title":"Senior Engineer","employer":"Acme Inc"}), 1).unwrap();
            manual_job::add(&mut ws, &json!({"url":"https://example.net/two","title":"Sr Engineer","employer":"Acme Ltd"}), 2).unwrap();
            let id = first["id"].as_str().unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap_err();
            assert!(error.contains("Review possible duplicate"));
            assert!(ws
                .has_duplicate_hold(&JobIdentityId::new(id).unwrap())
                .unwrap());
        });
        let body = json!({"id":format!("manual-{}",waypoint_domain::sha256_hex(b"https://example.org/one")),"recipient":"fictional@example.org","subject":"Hello","message":"Hello","documents":[]}).to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream,"POST /api/email-draft HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn server_stop_releases_its_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            serve_loop(
                Workspace::in_memory().unwrap(),
                listener,
                "fixture-token".into(),
                addr.port(),
                worker_stop,
            )
        });
        stop.store(true, std::sync::atomic::Ordering::Release);
        worker.join().unwrap();
        assert!(TcpListener::bind(addr).is_ok());
    }

    #[test]
    fn model_selection_requires_one_fully_resident_model_within_budget() {
        let good = json!({"models":[{"name":"local","size":1_000_000,"size_vram":1_000_000}]});
        assert_eq!(
            eligible_resident_model(&good).unwrap().as_deref(),
            Some("local")
        );
        for bad in [
            json!({"models":[]}),
            json!({"models":[{"name":"m","size":10,"size_vram":9}]}),
            json!({"models":[{"name":"m","size":15_032_385_537u64,"size_vram":15_032_385_537u64}]}),
            json!({"models":[{},{}]}),
        ] {
            assert_eq!(eligible_resident_model(&bad).unwrap(), None);
        }
        assert!(eligible_resident_model(&json!({})).is_err());
    }

    fn request(path: &str, token: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut workspace = Workspace::in_memory().unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            if let Err(error) = serve(&mut stream, &mut workspace, &mut None, "test-token", port) {
                json_reply(&mut stream, "400 Bad Request", json!({"error": error}));
            }
            workspace.facts().unwrap().len()
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: {token}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let facts = worker.join().unwrap();
        format!("{response}\nfacts={facts}")
    }

    #[test]
    fn mutation_requires_token_and_submit_is_unavailable() {
        let body = r#"{"text":"fixture skill"}"#;
        let denied = request("/api/facts", "wrong", body);
        assert!(denied.contains("400 Bad Request"));
        assert!(denied.contains("facts=0"));
        let allowed = request("/api/facts", "test-token", body);
        assert!(allowed.contains("200 OK"));
        assert!(allowed.contains("facts=1"));
        let submit = request("/api/submit", "test-token", "{}");
        assert!(submit.contains("unknown endpoint"));
        assert!(submit.contains("facts=0"));
    }

    #[test]
    fn profile_rejects_unknown_fields_and_wrong_types() {
        assert!(request(
            "/api/profile",
            "test-token",
            r#"{"first_name":"Example","instructions":"Remote work"}"#
        )
        .contains("200 OK"));
        assert!(request("/api/profile", "test-token", r#"{"unknown":"x"}"#)
            .contains("unknown profile field"));
        assert!(request("/api/profile", "test-token", r#"{"email":42}"#)
            .contains("profile fields must be text"));
    }

    #[test]
    fn packet_requires_shortlist_and_preserves_existing_draft() {
        use waypoint_store::StoredJob;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            let id = JobIdentityId::new("fixture-job").unwrap();
            ws.import_job(StoredJob {
                id,
                title: "Engineer".into(),
                employer: "Fixture".into(),
                location: "Remote".into(),
                state: ApplicationState::Discovered,
                canonical_url: None,
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: true,
                source: "fixture".into(),
                requisition_id: None,
                posted_at: None,
            })
            .unwrap();
            ws.add_fact("Designed useful systems", FactOrigin::UserAttested)
                .unwrap();
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                if let Err(error) = serve(&mut stream, &mut ws, &mut None, "test-token", port) {
                    json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                }
            }
        });
        let call = |path: &str| {
            let body = r#"{"id":"fixture-job"}"#;
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };
        assert!(call("/api/packet/prepare").contains("400 Bad Request"));
        assert!(call("/api/shortlist").contains("200 OK"));
        let prepared = call("/api/packet/prepare");
        assert!(prepared.contains("Designed useful systems"));
        assert!(prepared.contains("Position: Engineer at Fixture."));
        assert!(!prepared.contains("Applied for"));
        assert!(call("/api/packet/prepare").contains("already exists"));
        worker.join().unwrap();
    }

    #[test]
    fn uploaded_bytes_can_be_downloaded() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                if let Err(error) = serve(&mut stream, &mut ws, &mut None, "test-token", port) {
                    json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                }
            }
        });
        let bytes: Vec<u8> = (0..=255).cycle().take(100_000).collect();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/documents/upload HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nX-Document-Name: R%C3%A9sum%C3%A9.pdf\r\nContent-Length: {}\r\n\r\n", bytes.len()).unwrap();
        stream.write_all(&bytes).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let body = response.split_once("\r\n\r\n").unwrap().1;
        let id = serde_json::from_str::<Value>(body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let request = json!({"id":id}).to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/documents/download HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{request}", request.len()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(&response[split..], bytes);
        worker.join().unwrap();
    }

    #[test]
    fn stored_document_text_can_be_reviewed_without_creating_facts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            ws.add_document("synthetic", "cv.txt", b"Rust engineer", 1)
                .unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
            assert!(ws.facts().unwrap().is_empty());
        });
        let body = r#"{"id":"synthetic"}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/documents/extract HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("Rust engineer"));
        worker.join().unwrap();
    }

    #[test]
    #[ignore = "starts one headless browser against a local fixture"]
    fn reviewed_prefill_endpoint_fills_fixture_and_returns_screenshot() {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::{PacketValue, ProposedMapping};
        use waypoint_store::StoredJob;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let workspace_dir = tempfile::tempdir().unwrap();
            let mut ws = Workspace::open(workspace_dir.path()).unwrap();
            let fixture =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
            let url = format!(
                "file:///{}",
                fixture.display().to_string().replace('\\', "/")
            );
            let id = JobIdentityId::new("fixture-web").unwrap();
            ws.import_job(StoredJob {
                id: id.clone(),
                title: "Role".into(),
                employer: "Fixture".into(),
                location: "".into(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some(url.clone()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            })
            .unwrap();
            let profile = json!({"first_name":"Synthetic","country":"Germany"});
            ws.save_candidate_profile(&profile.to_string()).unwrap();
            ws.add_document(
                "fictional-doc",
                "fictional-cv.pdf",
                b"%PDF-fictional resume",
                1,
            )
            .unwrap();
            let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
            let form = driver.open(&url).unwrap();
            assert!(driver
                .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                    selector: "#available".into(),
                    value: PacketValue::Date("2040-01-01".into()),
                })
                .is_err());
            assert_eq!(
                driver.read_attribute("#available", "data-seen").unwrap(),
                None
            );
            assert_eq!(
                form.schema
                    .fields
                    .iter()
                    .find(|field| field.selector == "#email")
                    .unwrap()
                    .label,
                "Email address"
            );
            driver
                .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                    selector: "#work-permit".into(),
                    value: PacketValue::Text("Already entered manually".into()),
                })
                .unwrap();
            let preview = form_preview::FormPreview {
                schema: form.schema,
                suggestions: vec![
                    ProposedMapping {
                        field_selector: "#candidate".into(),
                        value: PacketValue::Text("Synthetic".into()),
                    },
                    ProposedMapping {
                        field_selector: "#country".into(),
                        value: PacketValue::Text("Germany".into()),
                    },
                    ProposedMapping {
                        field_selector: "#email".into(),
                        value: PacketValue::Text("fictional@example.invalid".into()),
                    },
                ],
                manual_review: vec![],
            };
            let mut session = Some(ReviewSession {
                id: "test-session".into(),
                job_id: id.clone(),
                url,
                profile_digest: Sha256Digest::of(&profile.to_string()),
                preview,
                driver,
                created: Instant::now(),
                ttl: SESSION_TTL,
                filled: false,
                visible: false,
                staged_files: vec![],
                record: None,
            });
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap();
            assert!(session.as_ref().unwrap().filled);
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#country", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("de")
            );
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#available", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("2029-02-28")
            );
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#travel", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("false")
            );
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#remote", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("true")
            );
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#email", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("manual@example.invalid")
            );
            assert!(!ws.manual_reviews().unwrap().is_empty());
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#work-permit", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("Already entered manually")
            );
            assert_eq!(
                ws.job(&id).unwrap().unwrap().state,
                ApplicationState::Shortlisted
            );
            drop(stream);
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap();
            let current = session.as_mut().unwrap();
            assert_eq!(
                current
                    .driver
                    .read_attribute("#resume", "data-attached")
                    .unwrap()
                    .as_deref(),
                Some("fictional-cv.pdf")
            );
            let staged_path = current.staged_files[0].path().join("fictional-cv.pdf");
            assert_eq!(
                std::fs::read(&staged_path).unwrap(),
                b"%PDF-fictional resume"
            );
            drop(stream);
            drop(session);
            assert!(!staged_path.exists());
            let records = workspace_dir.path().join("application-records");
            let record = std::fs::read_dir(records)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let events = std::fs::read_to_string(record.join("events.jsonl")).unwrap();
            assert!(
                events.contains("Synthetic")
                    && events.contains("manual@example.invalid")
                    && events.contains("document_attached")
            );
            assert!(events.contains("\"segments\":2"), "{events}");
            assert!(
                std::fs::read_dir(record.join("screenshots"))
                    .unwrap()
                    .count()
                    >= 2
            );
            assert!(std::fs::read_dir(record.join("materials")).unwrap().count() >= 1);
        });
        let body = r##"{"session_id":"test-session","reviewed":true,"selectors":["#candidate","#country"],"answers":[{"selector":"#email","value":"manual@example.invalid"},{"selector":"#available","value":"2029-02-28"},{"selector":"#travel","value":false},{"selector":"#onsite","value":"Remote"}]}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let payload: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["filled"], 6);
        assert!(payload["filled_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "Given name"));
        assert!(payload["required_not_filled"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "Work authorization"));
        assert!(payload["screenshot"].as_str().unwrap().starts_with("iVBOR"));
        let body = r##"{"session_id":"test-session","reviewed":true,"selector":"#resume","document_id":"fictional-doc"}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/attach HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"), "{response}");
        worker.join().unwrap();
    }
}
