#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Local browser interface for the existing Waypoint workspace.
mod answer_task;
mod application_check;
pub mod application_email;
mod ats_check;
mod attempt_record;
mod candidate_task;
mod convert_task;
mod discovery_task;
mod doc_suggest;
mod document_convert;
mod document_extract;
mod document_visual;
mod docx_edit;
mod docx_edit_task;
mod draft_task;
mod duplicate_compare;
mod duplicate_search;
mod email_draft;
pub mod email_worker;
mod extract_task;
mod fit_task;
mod form_links;
mod form_preview;
mod form_task;
mod gmail_connect;
mod gmail_mailbox;
mod gmail_net;
mod gmail_oauth;
mod gmail_password;
mod gmail_self_test;
mod letter_export;
mod link_rank;
mod manual_job;
mod pdf_extract;
mod pdf_job;
mod pdf_preview_task;
mod pdf_probe_task;
mod pdf_setup;
mod resident_inference;
mod search_planner;
mod smtp_handoff;
mod storage_settings;
mod suggestions;
mod template_suggest;
#[cfg(windows)]
mod tray;
mod visual_review;
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
const MAX_FORM_FILL_REQUEST: usize = 1024 * 1024;
const MAX_DOCUMENT: usize = 10 * 1024 * 1024;
const SESSION_TTL: Duration = Duration::from_secs(120);
const VISIBLE_SESSION_TTL: Duration = Duration::from_secs(600);

fn remaining_session_seconds(ttl: Duration, elapsed: Duration) -> u64 {
    ttl.saturating_sub(elapsed).as_secs()
}

fn inspection_failure_reason(error: &str) -> String {
    let reason: String = error
        .chars()
        .take(240)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if reason.trim().is_empty() {
        "Application inspection failed".to_string()
    } else {
        format!("Inspection failed: {reason}")
    }
}

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
    completed_selectors: Vec<String>,
    answer_drafts: std::collections::HashMap<String, SavedFormAnswer>,
    visible: bool,
    staged_files: Vec<tempfile::TempDir>,
    // Attachment verification is scoped to the inspection session in which
    // the control was observed; later wizard sections can reuse selectors.
    attachments: Vec<(String, std::path::PathBuf, String)>,
    last_visual_freshness_key: Option<String>,
    record: Option<attempt_record::AttemptRecord>,
}

struct SavedFormAnswer {
    selector: String,
    digest: String,
    answer: Value,
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

fn required_attempt_record(
    ws: &Workspace,
    job: &JobIdentityId,
) -> Result<attempt_record::AttemptRecord, String> {
    attempt_record::AttemptRecord::start(ws, job, &random_id()?, now())?
        .ok_or("A disk-backed workspace is required for an application record".into())
}
const PROFILE_FIELDS: &[&str] = &[
    "full_name",
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
    "auto_assess_ai",
];

fn search_plan_context(
    ws: &Workspace,
) -> Result<(Vec<String>, String, String, Vec<String>), String> {
    let facts: Vec<String> = ws
        .reviewed_facts_bounded(20, 1000)
        .map_err(|e| e.to_string())?
        .into_iter()
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
    let protected = search_planner::protected_identifiers(&profile);
    let digest = Sha256Digest::of(
        &json!({"facts":facts,"instructions":instructions,"protected":protected}).to_string(),
    );
    Ok((facts, instructions, digest.as_str().to_owned(), protected))
}

fn check_search_plan_binding(ws: &Workspace, body: &Value) -> Result<(), String> {
    let Some(value) = body.get("plan_digest").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let digest = value.as_str().ok_or("Invalid search plan binding")?;
    if digest != search_plan_context(ws)?.2 {
        return Err("Profile, qualifications or search instructions changed; generate new phrases or edit the search manually.".into());
    }
    Ok(())
}

fn fit_inputs(
    ws: &Workspace,
    job_id: &JobIdentityId,
) -> Result<(Vec<String>, String, String, String), String> {
    let (facts, instructions) = fit_candidate_context(ws)?;
    let (facts, listing, digest) = fit_listing_digest(ws, job_id, &facts, &instructions)?;
    Ok((facts, instructions, listing, digest))
}

/// Bind the full bounded fact inventory, not just the pool selected for the
/// model, so edits outside the pool invalidate a saved assessment too.
fn fit_assessment_inputs(
    ws: &Workspace,
    job_id: &JobIdentityId,
) -> Result<(Vec<String>, usize, String, String, String), String> {
    let (facts, instructions) = fit_candidate_context(ws)?;
    let (pool, listing, digest) = fit_assessment_digest(ws, job_id, &facts, &instructions)?;
    Ok((pool, facts.len(), instructions, listing, digest))
}

fn fit_assessment_digest(
    ws: &Workspace,
    job_id: &JobIdentityId,
    facts: &[String],
    instructions: &str,
) -> Result<(Vec<String>, String, String), String> {
    let (_, listing, _) = fit_listing_digest(ws, job_id, facts, instructions)?;
    let pool = fit_semantic_pool(facts, &listing);
    let description = ws
        .job_description(job_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(text) => serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let full_instructions = profile["instructions"].as_str().unwrap_or("");
    let digest = Sha256Digest::of(&json!({"scoring_version":"criteria-v2-semantic-pool-v31","facts":facts,"pool":pool,"instructions":instructions,"listing":listing,"full_description":description,"full_instructions":full_instructions}).to_string()).as_str().to_owned();
    Ok((pool, listing, digest))
}

fn fit_source_coverage(
    ws: &Workspace,
    job_id: &JobIdentityId,
    listing: &str,
    instructions: &str,
) -> Result<Value, String> {
    let job = ws
        .job(job_id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let description = ws
        .job_description(job_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let full_listing = format!(
        "Title: {}\nEmployer: {}\nLocation: {}\nDescription: {}",
        job.title, job.employer, job.location, description
    );
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(text) => serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let full_instructions = profile["instructions"].as_str().unwrap_or("");
    Ok(
        json!({"listing_used_chars":listing.chars().count(),"listing_total_chars":full_listing.chars().count(),"instructions_used_chars":instructions.chars().count(),"instructions_total_chars":full_instructions.chars().count()}),
    )
}

/// Keep strong lexical candidates, then sample across the bounded inventory
/// so the model can identify semantic relationships without a word match.
fn fit_semantic_pool(facts: &[String], listing: &str) -> Vec<String> {
    let mut pool = select_fit_facts(facts, listing);
    if facts.len() <= 20 {
        return pool;
    }
    for i in 0..40 {
        let index = i * facts.len() / 40;
        if let Some(fact) = facts.get(index) {
            if !pool.contains(fact) {
                pool.push(fact.clone());
            }
        }
    }
    pool
}

fn draft_priority_facts(ws: &Workspace, value: &Value) -> Result<Vec<String>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let ids = value
        .as_array()
        .filter(|ids| ids.len() <= 3)
        .ok_or("Choose at most three qualifications to prioritize")?;
    let available = ws
        .reviewed_facts_bounded(500, 1000)
        .map_err(|error| error.to_string())?;
    let mut seen = std::collections::HashSet::new();
    let mut facts = Vec::new();
    for id in ids {
        let id = id
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or("Invalid qualification identity")?;
        if !seen.insert(id) {
            return Err("A prioritized qualification was selected twice".into());
        }
        let fact = available.iter().find(|fact| fact.id == id).ok_or("A prioritized qualification is no longer reviewed or available; refresh and select again")?;
        if !facts.contains(&fact.text) {
            facts.push(fact.text.clone());
        }
    }
    Ok(facts)
}

fn fit_candidate_context(ws: &Workspace) -> Result<(Vec<String>, String), String> {
    let facts: Vec<String> = ws
        .reviewed_facts_bounded(500, 1000)
        .map_err(|e| e.to_string())?
        .into_iter()
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
    Ok((facts, instructions))
}

fn fit_listing_digest(
    ws: &Workspace,
    job_id: &JobIdentityId,
    facts: &[String],
    instructions: &str,
) -> Result<(Vec<String>, String, String), String> {
    let job = ws
        .job(job_id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
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
    let selected = select_fit_facts(facts, &listing);
    let digest = Sha256Digest::of(
        &json!({"scoring_version":"criteria-v1","facts":selected,"instructions":instructions,"listing":listing}).to_string(),
    )
    .as_str()
    .to_owned();
    Ok((selected, listing, digest))
}

fn select_fit_facts(facts: &[String], listing: &str) -> Vec<String> {
    use std::collections::HashSet;
    fn words(text: &str) -> HashSet<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
            .map(str::to_lowercase)
            .filter(|word| {
                word.chars().count() >= 3 || ["c", "c#", "f#", "go", "r"].contains(&word.as_str())
            })
            .filter(|word| {
                ![
                    "the", "and", "for", "with", "from", "that", "this", "have", "was", "are",
                    "our", "your",
                ]
                .contains(&word.as_str())
            })
            .collect()
    }
    let terms = words(listing);
    let mut selected: Vec<(usize, usize)> = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        let score = words(fact).intersection(&terms).count();
        let at = selected
            .iter()
            .position(|(other, _)| score > *other)
            .unwrap_or(selected.len());
        if at < 20 {
            selected.insert(at, (score, index));
            selected.truncate(20);
        }
    }
    selected
        .into_iter()
        .map(|(_, index)| facts[index].clone())
        .collect()
}

/// Select reviewed evidence relevant to the listing without loading the full fact history.
#[cfg(test)]
fn draft_qualifications(
    ws: &Workspace,
    job: &JobIdentityId,
    kind: &str,
) -> Result<Vec<String>, String> {
    let (mut facts, _, _, _) = fit_inputs(ws, job)?;
    match kind {
        "cv" => facts.truncate(18),
        "cover_letter" => {}
        _ => return Err("Unknown draft type".into()),
    }
    if facts.is_empty() {
        return Err("Add verified qualifications before drafting".into());
    }
    Ok(facts)
}

fn document_visual_context(ws: &Workspace, job: &JobIdentityId) -> Result<String, String> {
    let (facts, _, listing, _) = fit_inputs(ws, job)?;
    Ok(format!(
        "Posting: {}\nReviewed qualifications: {}",
        listing.chars().take(5000).collect::<String>(),
        facts.join(" | ").chars().take(5000).collect::<String>()
    ))
}

fn verified_visual_review(
    ws: &Workspace,
    job: &JobIdentityId,
    attempt: &str,
    bytes: &[u8],
) -> Result<Value, String> {
    let review = attempt_record::document_visual_review(ws, job, attempt)?;
    let context = document_visual_context(ws, job)?;
    if review["document_sha256"].as_str() != Some(waypoint_domain::sha256_hex(bytes).as_str())
        || review["context_sha256"].as_str() != Some(Sha256Digest::of(&context).as_str())
        || review["status"] != "complete"
        || review["report"]["complete"] != true
    {
        return Err("The selected visual review does not match this PDF and current job context; review again".into());
    }
    Ok(review)
}

fn draft_context_digest(
    ws: &Workspace,
    job_id: &JobIdentityId,
    kind: &str,
) -> Result<String, String> {
    let job = ws
        .job(job_id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let description = ws.job_description(job_id).map_err(|e| e.to_string())?;
    let facts_digest = ws.active_facts_digest().map_err(|e| e.to_string())?;
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(value) => serde_json::from_str(&value).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let key = if kind == "cv" {
        "cv_template_id"
    } else if kind == "cover_letter" {
        "cover_template_id"
    } else {
        return Err("Unknown draft type".into());
    };
    let template_hash = if let Some(id) = profile[key].as_str().filter(|id| !id.is_empty()) {
        let bytes = ws
            .document_content(id)
            .map_err(|e| e.to_string())?
            .ok_or("Selected template is unavailable")?;
        Some(waypoint_domain::sha256_hex(&bytes))
    } else {
        None
    };
    Ok(Sha256Digest::of(&json!({"job":job,"description":description,"facts_digest":facts_digest,"profile":profile,"template_hash":template_hash,"kind":kind}).to_string()).as_str().to_owned())
}

fn read_fit(ws: &Workspace, job_id: &JobIdentityId) -> Result<Value, String> {
    read_fit_with_context(ws, job_id, &mut None)
}

fn read_fit_with_context(
    ws: &Workspace,
    job_id: &JobIdentityId,
    context: &mut Option<(Vec<String>, String)>,
) -> Result<Value, String> {
    match ws.fit_assessment(job_id).map_err(|e| e.to_string())? {
        Some(data) => {
            let value: Value =
                serde_json::from_slice(&data).map_err(|_| "Stored assessment is invalid")?;
            if context.is_none() {
                *context = Some(fit_candidate_context(ws)?);
            }
            let (facts, instructions) = context.as_ref().ok_or("Candidate context unavailable")?;
            let (_, _, digest) = fit_assessment_digest(ws, job_id, facts, instructions)?;
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
        .filter(|a| a.len() <= suggestions::MAX_FIELDS)
        .ok_or("Answers must be a list of at most 200 fields")?;
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
            || preview.model_review_selectors.contains(selector)
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
            (FormFieldType::MultiSelect { options }, Value::Array(items)) => {
                let values: Option<Vec<String>> = items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .filter(|value| value.len() <= 4000)
                            .map(str::to_owned)
                    })
                    .collect();
                let values = values.ok_or("Invalid multiple-choice answer")?;
                if !waypoint_applications::form_mapping::valid_multi_select(options, &values) {
                    return Err("Multiple-choice answer is invalid or ambiguous".into());
                }
                PacketValue::TextList(values)
            }
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

fn prepare_reviewed_fill(
    preview: &form_preview::FormPreview,
    body: &Value,
) -> Result<(form_preview::FormPreview, bool), String> {
    let mut prepared = preview.clone();
    if let Some(selection) = body.get("selectors") {
        let selected = selection
            .as_array()
            .ok_or("Selected fields must be a list")?;
        if selected.len() > suggestions::MAX_FIELDS {
            return Err("Select at most 200 fields to fill".into());
        }
        let mut selectors = std::collections::HashSet::new();
        for value in selected {
            let selector = value.as_str().ok_or("Invalid selected field")?;
            if !selectors.insert(selector)
                || !preview
                    .suggestions
                    .iter()
                    .any(|mapping| mapping.field_selector == selector)
            {
                return Err("Selected fields contain a duplicate or unknown field".into());
            }
        }
        prepared
            .suggestions
            .retain(|mapping| selectors.contains(mapping.field_selector.as_str()));
    }
    if let Some(answers) = body.get("answers") {
        let answers = reviewed_form_answers(answers, &prepared)?;
        prepared.suggestions.extend(answers);
    }
    if prepared.suggestions.is_empty() {
        return Err("Select at least one field to fill".into());
    }
    if prepared.suggestions.len() > suggestions::MAX_FIELDS {
        return Err("Select at most 200 fields to fill".into());
    }
    for mapping in &prepared.suggestions {
        if let Some(field) = prepared
            .schema
            .fields
            .iter()
            .find(|f| f.selector == mapping.field_selector)
        {
            if matches!(
                field.field_type,
                waypoint_applications::form_mapping::FormFieldType::Text
                    | waypoint_applications::form_mapping::FormFieldType::Email
                    | waypoint_applications::form_mapping::FormFieldType::TextArea
            ) {
                if let waypoint_applications::form_mapping::PacketValue::Text(text) = &mapping.value
                {
                    if !waypoint_applications::form_mapping::text_fits_field(field, text) {
                        return Err(format!("The answer for {} exceeds the page's {} UTF-16-unit limit; shorten it before filling", field.label, field.max_length_utf16.unwrap_or(0)));
                    }
                }
            }
        }
    }
    let omitted = preview.suggestions.iter().any(|original| {
        !prepared
            .suggestions
            .iter()
            .any(|mapping| mapping.field_selector == original.field_selector)
    });
    Ok((prepared, omitted))
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
        || session.preview.model_review_selectors.contains(selector)
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
    let (all_facts, search_instructions) = fit_candidate_context(ws)?;
    if all_facts.is_empty() {
        return Err("Add reviewed qualifications before drafting an answer".into());
    }
    let (_, listing, _) =
        fit_listing_digest(ws, &session.job_id, &all_facts, &search_instructions)?;
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
    let facts = fit_semantic_pool(&all_facts, &format!("{question}\n{listing}"));
    let available_facts = all_facts.len();
    let digest = Sha256Digest::of(&json!({"version":"answer-semantic-v1","session":session.id,"schema":session.preview.schema.schema_hash_input(),"selector":selector,"question":question,"listing":listing,"all_facts":all_facts,"facts":facts,"instructions":instructions}).to_string()).as_str().to_owned();
    Ok(answer_task::Inputs {
        max_length_utf16: field.max_length_utf16,
        session_id: session.id.clone(),
        selector: selector.to_owned(),
        digest,
        question,
        listing,
        facts,
        available_facts,
        instructions,
    })
}

/// Link only exact, current session drafts to the requested answer. Edited
/// text remains fillable but must not inherit the original draft's citations.
fn attach_answer_provenance(
    ws: &Workspace,
    session: &ReviewSession,
    body: &Value,
    fields: &mut [Value],
) -> Result<(), String> {
    let Some(answers) = body.get("answers").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut used = std::collections::HashSet::new();
    for answer in answers {
        let id = match answer.get("draft_id") {
            None => continue,
            Some(Value::String(id)) if !id.is_empty() => id.as_str(),
            _ => return Err("Invalid linked draft identifier".into()),
        };
        if !used.insert(id) {
            return Err("A draft cannot be linked to multiple answers".into());
        }
        let selector = answer["selector"].as_str().ok_or("Missing answer field")?;
        let draft = session
            .answer_drafts
            .get(id)
            .ok_or("The linked answer draft is no longer available; review the answer again")?;
        let fresh = answer_inputs(ws, session, selector)?;
        if draft.selector != selector
            || draft.digest != fresh.digest
            || draft.answer["text"].as_str() != answer["value"].as_str()
        {
            return Err(
                "The linked draft or its supporting information changed; review the answer again"
                    .into(),
            );
        }
        let field = fields
            .iter_mut()
            .find(|field| field["selector"].as_str() == Some(selector))
            .ok_or("Linked answer field is missing")?;
        field["source_draft"] = json!({"task_id":id,"sources":draft.answer["sources"],"selection":draft.answer["selection"],"evidence_check":draft.answer["evidence_check"],"note":"The reviewed answer exactly matched this local draft. Model citations and checks are advisory."});
    }
    Ok(())
}

struct GeneratedEvidenceLink<'a> {
    document_id: &'a str,
    document_name: &'a str,
    selector: Option<&'a str>,
    pdf: &'a [u8],
    context_current: bool,
    lookup_incomplete: bool,
}

fn record_generated_evidence(
    record: &mut attempt_record::AttemptRecord,
    evidence: &attempt_record::GeneratedEvidence,
    link: GeneratedEvidenceLink<'_>,
) -> Result<&'static str, String> {
    let evidence_name = evidence
        .evidence
        .as_ref()
        .map(|bytes| record.material("generated-draft-evidence.json", bytes))
        .transpose()?;
    let template_name = evidence
        .template
        .as_ref()
        .map(|(name, bytes)| record.material(&format!("original-{name}"), bytes))
        .transpose()?;
    record.event(json!({"type":"document_draft_evidence_linked","time":now(),"source_attempt":evidence.attempt_id,"selector":link.selector,"document":link.document_name,"document_id":link.document_id,"pdf_sha256":waypoint_domain::sha256_hex(link.pdf),"evidence":evidence_name,"template":template_name,"context_current":link.context_current,"lookup_incomplete":link.lookup_incomplete,"note":"Any draft evidence describes the generated draft; it may not cover later edits and does not prove the file was attached or submitted."}))?;
    Ok(match (evidence.evidence.is_some(), link.context_current) {
        (true, true) => "linked_current",
        (true, false) => "linked_historical",
        (false, true) => "template_only_current",
        (false, false) => "template_only_historical",
    })
}

fn email_answer_inputs(ws: &Workspace, id: &JobIdentityId) -> Result<answer_task::Inputs, String> {
    let job = ws
        .job(id)
        .map_err(|error| error.to_string())?
        .ok_or("Job not found")?;
    if !matches!(
        job.state,
        ApplicationState::Shortlisted | ApplicationState::Prepared
    ) {
        return Err("Shortlist the job before drafting an email".into());
    }
    if ws
        .has_duplicate_hold(id)
        .map_err(|error| error.to_string())?
    {
        return Err("Review possible duplicate jobs before drafting an email".into());
    }
    let (all_facts, search_instructions) = fit_candidate_context(ws)?;
    if all_facts.is_empty() {
        return Err("Add reviewed qualifications before drafting an email".into());
    }
    let (_, listing, _) = fit_listing_digest(ws, id, &all_facts, &search_instructions)?;
    let profile: Value = match ws.candidate_profile().map_err(|error| error.to_string())? {
        Some(text) => serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let instructions: String = profile["document_instructions"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(2000)
        .collect();
    let question = format!("Write a concise application email body for the role {} at {}. Use only supported candidate facts, do not invent contact information, and leave the recipient and subject to the candidate.", job.title, job.employer);
    let facts = fit_semantic_pool(&all_facts, &format!("{question}\n{listing}"));
    let available_facts = all_facts.len();
    let digest = Sha256Digest::of(&json!({"version":"answer-semantic-v1","job_id":id.as_str(),"question":question,"listing":listing,"all_facts":all_facts,"facts":facts,"instructions":instructions}).to_string()).as_str().to_owned();
    Ok(answer_task::Inputs {
        max_length_utf16: None,
        session_id: id.as_str().to_owned(),
        selector: "email_body".into(),
        digest,
        question,
        listing,
        facts,
        available_facts,
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
    let listing = |job: &waypoint_store::StoredJob| -> Result<(String, String, bool), String> {
        let description = ws
            .job_description(&job.id)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let full = format!(
            "Title: {}\nEmployer: {}\nLocation: {}\nRequisition: {}\nDescription: {}",
            job.title,
            job.employer,
            job.location,
            job.requisition_id.as_deref().unwrap_or(""),
            description
        );
        let short: String = full.chars().take(1800).collect();
        let shortened = short.len() < full.len();
        Ok((
            short,
            waypoint_domain::sha256_hex(full.as_bytes()),
            shortened,
        ))
    };
    let job = ws
        .job(id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let (target, target_digest, target_shortened) = listing(&job)?;
    let (neighbors, truncated) = ws.duplicate_neighbors(id).map_err(|e| e.to_string())?;
    let mut candidates = Vec::with_capacity(neighbors.len());
    let mut source_digests = Vec::with_capacity(neighbors.len());
    let mut source_texts_shortened = usize::from(target_shortened);
    for job in neighbors {
        let (text, digest, shortened) = listing(&job)?;
        source_texts_shortened += usize::from(shortened);
        source_digests.push((job.id.as_str().to_owned(), digest));
        candidates.push((job.id.as_str().to_owned(), text));
    }
    let digest = Sha256Digest::of(
        &json!({"target":target,"target_digest":target_digest,"candidates":candidates,"source_digests":source_digests,"truncated":truncated}).to_string(),
    )
    .as_str()
    .to_owned();
    Ok(duplicate_search::Inputs {
        job_id: id.as_str().to_owned(),
        target,
        candidates,
        digest,
        truncated,
        source_texts_shortened,
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
    let job_context = docx_edit_task::job_context_digest(ws, job_id.as_str())?;
    let digest=Sha256Digest::of(&json!({"source":waypoint_domain::sha256_hex(&source),"before":before,"job":job_id.as_str(),"listing":listing,"facts":facts,"instructions":instructions,"job_context":job_context}).to_string()).as_str().to_owned();
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

fn validate_template_edit_bindings(
    ws: &Workspace,
    document_id: &str,
    job_id: Option<&str>,
    edits: &[Value],
) -> Result<(), String> {
    for edit in edits {
        let Some(digest) = edit.get("suggestion_digest") else {
            continue;
        };
        let digest = digest
            .as_str()
            .filter(|value| value.len() == 64)
            .ok_or("Invalid suggestion context")?;
        let job = job_id.ok_or("Select the job used for the suggestion")?;
        if edit["suggestion_job_id"].as_str() != Some(job) {
            return Err("The job for a queued suggestion changed; suggest again".into());
        }
        let before = edit["before"].as_str().ok_or("Missing original text")?;
        let job = JobIdentityId::new(job).map_err(|e| e.to_string())?;
        let current = template_suggestion_inputs(ws, document_id, &job, before)?;
        if current.digest != digest {
            return Err(
                "The template, job or supporting information changed; suggest again".into(),
            );
        }
    }
    Ok(())
}

fn rehearse_saved_gmail(ws: &mut Workspace, body: &Value) -> Result<Value, String> {
    let id = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
        .map_err(|e| e.to_string())?;
    let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
    let approval = body["approval_id"].as_str().ok_or("missing approval")?;
    let (review, packet) = attempt_record::load_gmail_review(ws, &id, attempt)?;
    if application_email::connected_sender(ws)? != review.sender {
        return Err("The connected Gmail account changed; prepare a new packet".into());
    }
    let rehearsal = random_id()?;
    let timestamp = now();
    packet.rehearse(ws, &id, approval, &rehearsal, timestamp)?;
    let bytes = application_email::rehearse_smtp(&packet)?;
    let recorded =
        attempt_record::record_email_rehearsal(ws, &id, attempt, &rehearsal, timestamp, bytes)
            .is_ok();
    Ok(
        json!({"simulated":true,"sent":false,"approval_consumed":false,"smtp_bytes_simulated":bytes,"record_saved":recorded}),
    )
}

fn approve_saved_gmail(ws: &mut Workspace, body: &Value) -> Result<Value, String> {
    if body["reviewed"].as_bool() != Some(true) {
        return Err("Explicit review is required".into());
    }
    let id = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
        .map_err(|e| e.to_string())?;
    let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
    let expected_message = body["mime_sha256"]
        .as_str()
        .ok_or("missing reviewed message hash")?;
    let expected_context = body["context_sha256"]
        .as_str()
        .ok_or("missing reviewed context hash")?;
    let (review, packet) = attempt_record::load_gmail_review(ws, &id, attempt)?;
    if review.mime_sha256 != expected_message
        || ws.email_review_context(&id)?.as_str() != expected_context
    {
        return Err("The packet or supporting information changed; review again".into());
    }
    if application_email::connected_sender(ws)? != review.sender {
        return Err("The connected Gmail account changed; prepare a new packet".into());
    }
    if ws.has_duplicate_hold(&id).map_err(|e| e.to_string())? {
        return Err("Review possible duplicate jobs before approving".into());
    }
    let approval = random_id()?;
    packet.approve(
        ws,
        &id,
        &approval,
        now(),
        true,
        body["acknowledge_incomplete_duplicates"].as_bool() == Some(true),
    )?;
    Ok(json!({"approval_id":approval,"expires_in_seconds":600,"sent":false}))
}

fn check_answer_page(session: &mut ReviewSession) -> Result<(), String> {
    let live = session
        .driver
        .inspect_current()
        .map_err(|e| e.to_string())?;
    if !form_preview::same_reviewable_walls(&session.preview, &live.walls)
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
    let status = resident_model_status()?;
    Ok(status["model"].as_str().map(str::to_owned))
}

fn resident_model_status() -> Result<Value, String> {
    let addr = "127.0.0.1:11434"
        .parse()
        .map_err(|_| "invalid runtime address")?;
    let (status, bytes) = http_request(addr, "GET", "/api/ps", None, 3000)?;
    if status != 200 {
        return Err(format!("Ollama returned HTTP {status}"));
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid Ollama response")?;
    model_diagnostics(&data)
}

fn installed_model_inventory(data: &Value) -> Result<Value, String> {
    let models = data["models"]
        .as_array()
        .ok_or("Ollama did not report installed models")?;
    let mut entries = Vec::new();
    let mut invalid = 0usize;
    for model in models.iter().take(100) {
        let Some(name) = model["name"].as_str().filter(|name| {
            !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
        }) else {
            invalid += 1;
            continue;
        };
        entries.push(json!({"name":name,"disk_bytes":model["size"].as_u64()}));
    }
    Ok(
        json!({"models":entries,"reported_count":models.len(),"examined":models.len().min(100),"invalid":invalid}),
    )
}

fn model_diagnostics(data: &Value) -> Result<Value, String> {
    let eligible = eligible_resident_model(data)?;
    let models = data["models"]
        .as_array()
        .ok_or("Ollama did not report resident models")?;
    if let Some(name) = eligible {
        return Ok(
            json!({"model":name,"state":"ready","reported_bytes":models[0]["size"],"reported_vram_bytes":models[0]["size_vram"]}),
        );
    }
    let state = if models.is_empty() {
        "no_model"
    } else if models.len() > 1 {
        "multiple_models"
    } else {
        let size = models[0]["size"].as_u64().unwrap_or(0);
        let vram = models[0]["size_vram"].as_u64().unwrap_or(0);
        if size == 0 || models[0]["name"].as_str().is_none_or(str::is_empty) {
            "incomplete_report"
        } else if vram > 14 * 1024 * 1024 * 1024 {
            "over_budget"
        } else if vram != size {
            "not_fully_gpu"
        } else {
            "incomplete_report"
        }
    };
    Ok(json!({"model":null,"state":state,"loaded_models":models.len()}))
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

fn email_document_reply(stream: &mut TcpStream, body: &[u8], evidence_status: &'static str) {
    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Waypoint-Document-Evidence: {evidence_status}\r\nConnection: close\r\n\r\n", body.len());
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
    } else if method == "POST" && path == "/api/form/fill" {
        MAX_FORM_FILL_REQUEST
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
        let skills: Vec<String> = facts
            .iter()
            .filter(|fact| !fact.requires_review && !fact.text.trim().is_empty())
            .map(|fact| fact.text.clone())
            .collect();
        let ranked = ws
            .ranked(&CampaignConstraints::default(), &skills, now())
            .map_err(|e| e.to_string())?;
        let jobs: Vec<Value> = ranked.strict.iter().map(|row| {
            let basis = if skills.is_empty() { "no_reviewed_qualifications" } else if !row.description_available { "no_description" } else { "text_overlap" };
            json!({"job":row.job,"score":if basis == "text_overlap" { Some(row.score) } else { None },"matched":if basis == "text_overlap" { row.matched_skills.clone() } else { vec![] },"match_basis":basis,"unknown_constraints":row.unknown_constraints})
        }).collect();
        let facts: Vec<Value> = facts
            .iter()
            .map(|f| json!({"id":f.id,"text":f.text,"requires_review":f.requires_review}))
            .collect();
        let documents: Vec<Value> = ws.documents().map_err(|e| e.to_string())?.iter().map(|doc| json!({"id":doc.id,"name":doc.name,"attachment_name":attempt_record::attachment_filename(&doc.name),"size":doc.size,"created_at":doc.created_at})).collect();
        let reviews: Vec<Value> = ws
            .manual_reviews()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|review| json!({"job_id":review.job_id,"reason":review.reason,"revision":review.revision,"reason_count":review.reason_count}))
            .collect();
        let profile = match ws.candidate_profile().map_err(|e| e.to_string())? {
            Some(content) => {
                serde_json::from_str::<Value>(&content).map_err(|_| "stored profile is invalid")?
            }
            None => json!({}),
        };
        let (duplicate_total, duplicate_pending, duplicate_bounded, duplicate_missing) =
            ws.duplicate_coverage_counts().map_err(|e| e.to_string())?;
        json_reply(
            stream,
            "200 OK",
            json!({"jobs": jobs, "facts": facts, "documents":documents, "reviews":reviews, "duplicates":ws.possible_duplicates().map_err(|e| e.to_string())?, "duplicate_index_pending":ws.duplicate_index_pending().map_err(|e|e.to_string())?, "duplicate_coverage":{"total":duplicate_total,"pending":duplicate_pending,"bounded":duplicate_bounded,"missing_employer":duplicate_missing}, "profile":profile, "sealing": ws.sealing_status()}),
        );
        return Ok(());
    }
    if method == "GET" && path == "/api/license-notices" {
        json_reply(
            stream,
            "200 OK",
            json!({
                "sqlcipher": include_str!("../../../docs/SQLCIPHER_LICENSE.txt"),
                "credits": include_str!("../../../docs/REJECTION_REJECTOR_LICENSE.md"),
                "inventory": include_str!("../../../docs/dependency-license-inventory.txt")
            }),
        );
        return Ok(());
    }
    if method == "GET" && path == "/api/model-status" {
        let status = resident_model_status();
        json_reply(
            stream,
            "200 OK",
            match status {
                Ok(status) => status,
                Err(error) => json!({"model":null,"state":"unreachable","error":error}),
            },
        );
        return Ok(());
    }
    if method == "GET" && path == "/api/model-inventory" {
        let addr = "127.0.0.1:11434"
            .parse()
            .map_err(|_| "invalid runtime address")?;
        let (status, bytes) = http_request(addr, "GET", "/api/tags", None, 3000)?;
        if status != 200 {
            return Err(format!("Ollama returned HTTP {status}"));
        }
        let data: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid Ollama response")?;
        json_reply(stream, "200 OK", installed_model_inventory(&data)?);
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
        "/api/duplicates/search-pending" => {
            json_reply(stream, "200 OK", duplicate_search::pending()?);
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
            if result["status"] == "complete" && result["saved"] != true {
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
                    let added = ws.add_ai_duplicates(&job, &flagged).map_err(|e| {
                        format!(
                            "Could not save duplicate candidates; retry this search result: {e}"
                        )
                    })?;
                    result["added"] = json!(added);
                    result["saved"] = json!(true);
                }
            }
            if result["status"] != "running" {
                duplicate_search::complete(id, result.clone())?;
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
            if result["status"] != "running" {
                result = duplicate_compare::complete(id, result)?;
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
        "/api/gmail/password-start" => {
            let email = body["email"].as_str().ok_or("missing Gmail address")?;
            let password = body["password"].as_str().ok_or("missing app password")?;
            let id = random_id()?;
            gmail_password::start(id.clone(), email.to_owned(), password.to_owned(), ws)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/gmail/password-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", gmail_password::poll(id, ws)?);
        }
        "/api/gmail/password-cancel" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            gmail_password::cancel(id)?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/gmail/inbox-start" => {
            let id = random_id()?;
            gmail_mailbox::start(id.clone(), ws)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/gmail/inbox-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", gmail_mailbox::poll(id, ws)?);
        }
        "/api/gmail/self-test-start" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Explicit confirmation is required to send a test email".into());
            }
            let id = random_id()?;
            gmail_self_test::start(id.clone(), ws)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/gmail/self-test-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", gmail_self_test::poll(id)?);
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
            if gmail_connect::busy() || gmail_password::busy() {
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
            let allow_external = body["allow_external"].as_bool().unwrap_or(false);
            application_check::start(id.clone(), job_id.as_str().to_owned(), url, allow_external)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/application-check/pending" => {
            let pending = application_check::pending()?;
            json_reply(
                stream,
                "200 OK",
                match pending {
                    Some((task_id, job_id)) => json!({"task_id":task_id,"job_id":job_id}),
                    None => json!({"task_id":null}),
                },
            );
        }
        "/api/application-check/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            match application_check::poll(id)? {
                Some((job_id, checked_url, result, review)) => {
                    let job_id = JobIdentityId::new(&job_id).map_err(|e| e.to_string())?;
                    let job = ws.job(&job_id).map_err(|e| e.to_string())?;
                    let current = job.as_ref().is_some_and(|job| {
                        job.canonical_url.as_deref() == Some(checked_url.as_str())
                    });
                    let message = if current {
                        result
                    } else {
                        "The listing changed during the application check; inspect its current URL again.".into()
                    };
                    if review && job.is_some() {
                        ws.mark_manual_review(&job_id, &message, now())
                            .map_err(|e| e.to_string())?;
                    }
                    application_check::acknowledge(id)?;
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"complete","result":message,"checked_url":checked_url,"current":current}),
                    );
                }
                None => json_reply(stream, "200 OK", json!({"status":"running"})),
            }
        }
        "/api/discover/start" => {
            check_search_plan_binding(ws, &body)?;
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
            let acknowledged = body["acknowledged"].as_u64().unwrap_or(0);
            let acknowledged =
                usize::try_from(acknowledged).map_err(|_| "Invalid batch acknowledgement")?;
            json_reply(stream, "200 OK", web_search_batch::poll(id, acknowledged)?);
        }
        "/api/web-search/batch-pending" => {
            json_reply(stream, "200 OK", web_search_batch::pending()?);
        }
        "/api/web-search/batch-cancel" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            web_search_batch::cancel(id)?;
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/web-search/start" => {
            check_search_plan_binding(ws, &body)?;
            let provider = body["provider"].as_str().ok_or("missing provider")?;
            let query = body["query"].as_str().ok_or("missing query")?;
            let id = random_id()?;
            web_search::start(id.clone(), provider.to_owned(), query.to_owned())?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/web-search/pending" => {
            json_reply(stream, "200 OK", json!({"task_id":web_search::pending()?}));
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
        "/api/listing-inspect/pending" => {
            let pending = candidate_task::pending()?;
            json_reply(
                stream,
                "200 OK",
                match pending {
                    Some((task_id, url)) => json!({"task_id":task_id,"url":url}),
                    None => json!({"task_id":null}),
                },
            );
        }
        "/api/listing-salary-evidence" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let job = ws
                .job(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let metadata = match ws.job_source_only(&id).map_err(|e| e.to_string())? {
                Some(source) => Some(source),
                None => ws.job_capture_metadata(&id).map_err(|e| e.to_string())?,
            };
            let (evidence, captured_at, matches_current_url) = if let Some(metadata) = metadata {
                let evidence = candidate_task::captured_posting_evidence(&metadata)?;
                let capture: Value = serde_json::from_slice(&metadata)
                    .map_err(|_| "Saved listing metadata is invalid")?;
                let captured_at = capture["captured_at"]
                    .as_i64()
                    .filter(|time| *time > 0 && *time <= 253_402_300_799);
                let source_url = capture["source_url"].as_str().filter(|url| !url.is_empty());
                let matches_current_url = source_url
                    .zip(job.canonical_url.as_deref().filter(|url| !url.is_empty()))
                    .map(|(source, current)| source == current);
                (evidence, captured_at, matches_current_url)
            } else {
                (None, None, None)
            };
            json_reply(
                stream,
                "200 OK",
                json!({"salary_evidence":evidence.as_ref().map(|v| &v["salary_evidence"]),"posting_date_evidence":evidence.as_ref().map(|v| &v["posting_date_evidence"]),"valid_through_evidence":evidence.as_ref().map(|v| &v["valid_through_evidence"]),"captured_at":captured_at,"matches_current_url":matches_current_url}),
            );
        }
        "/api/listing-inspect/discard" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            candidate_task::discard(id)?;
            json_reply(stream, "200 OK", json!({"discarded":true}));
        }
        "/api/listing-inspect/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            if let Some(result) = candidate_task::completed(id)? {
                json_reply(stream, "200 OK", result);
                return Ok(());
            }
            match candidate_task::poll(id)? {
                None => json_reply(stream, "200 OK", json!({"status":"running"})),
                Some(Err(error)) => {
                    let result = json!({"status":"failed","error":error});
                    candidate_task::complete(id, result.clone())?;
                    json_reply(stream, "200 OK", result)
                }
                Some(Ok(inspected)) => {
                    let job = &inspected.job;
                    let result = manual_job::add(ws, job, now())?;
                    let job_id =
                        JobIdentityId::new(result["id"].as_str().ok_or("missing saved id")?)
                            .map_err(|e| e.to_string())?;
                    let capture = if inspected.capture.segments.is_empty() {
                        let has_source = inspected.capture.metadata["structured_source_status"]
                            == "captured"
                            || matches!(
                                inspected.capture.metadata["page_text_status"].as_str(),
                                Some("captured" | "truncated")
                            );
                        if !has_source {
                            json!({"saved":false,"complete":false,"source_saved":false,"reason":"Listing screenshot and source were unavailable"})
                        } else {
                            let metadata = serde_json::to_vec(&inspected.capture.metadata)
                                .map_err(|e| e.to_string())?;
                            match ws.save_job_source_only(&job_id, &metadata) {
                                Ok(saved) => {
                                    json!({"saved":false,"complete":false,"source_saved":saved,"structured_source":if saved { inspected.capture.metadata["structured_source_status"].clone() } else { Value::Null },"reason":"Listing screenshot unavailable; source metadata retained when possible"})
                                }
                                Err(_) => {
                                    json!({"saved":false,"complete":false,"source_saved":false,"reason":"Listing screenshot unavailable and source could not be saved securely"})
                                }
                            }
                        }
                    } else {
                        let metadata = serde_json::to_vec(&inspected.capture.metadata)
                            .map_err(|e| e.to_string())?;
                        match ws.save_job_capture(&job_id, &metadata, &inspected.capture.segments) {
                            Ok(true) => {
                                json!({"saved":true,"complete":inspected.capture.metadata["full_dimensions_captured"],"structured_source":inspected.capture.metadata["structured_source_status"]})
                            }
                            Ok(false) => {
                                let existing = ws
                                    .job_capture_metadata(&job_id)
                                    .ok()
                                    .flatten()
                                    .and_then(|metadata| {
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
                    {
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
                    if job["extraction_warnings"]
                        .as_array()
                        .is_some_and(|warnings| !warnings.is_empty())
                    {
                        ws.mark_manual_review(&job_id, "Some listing details are missing or could not be verified against the page", now())
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
                    let response =
                        json!({"status":"complete","job":job,"saved":result,"capture":capture});
                    candidate_task::complete(id, response.clone())?;
                    json_reply(stream, "200 OK", response);
                }
            }
        }
        "/api/fit/start" => {
            let job_id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let (facts, available, instructions, listing, digest) =
                fit_assessment_inputs(ws, &job_id)?;
            if facts.is_empty() && instructions.trim().is_empty() {
                return Err("Add reviewed qualifications or search instructions first".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            let job = ws
                .job(&job_id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let description = ws
                .job_description(&job_id)
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            // The model still receives only its bounded excerpt. The separate
            // source is used solely for deterministic citation checks.
            let source_bytes = [
                job.title.len(),
                job.employer.len(),
                job.location.len(),
                description.len(),
                "Title: \nEmployer: \nLocation: \nDescription: ".len(),
            ]
            .into_iter()
            .try_fold(0usize, usize::checked_add);
            let verification_text = source_bytes.filter(|size| *size <= 1_048_576).map(|_| {
                format!(
                    "Title: {}\nEmployer: {}\nLocation: {}\nDescription: {}",
                    job.title, job.employer, job.location, description
                )
            });
            fit_task::start(
                task_id.clone(),
                model,
                job_id.as_str().to_owned(),
                digest,
                fit_task::FactPool { facts, available },
                instructions,
                fit_task::FitListing {
                    model_text: listing,
                    verification_text,
                },
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/fit/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = fit_task::poll(id)?;
            if result["status"] == "complete" {
                let job_id = JobIdentityId::new(result["job_id"].as_str().ok_or("missing job")?)
                    .map_err(|e| e.to_string())?;
                let (_, _, instructions, listing, digest) = fit_assessment_inputs(ws, &job_id)?;
                if result["input_digest"].as_str() != Some(digest.as_str()) {
                    result = json!({"status":"failed","error":"Job or candidate inputs changed during assessment; assess again"});
                } else if result["saved"].is_null() {
                    result["assessment"]["source_coverage"] =
                        fit_source_coverage(ws, &job_id, &listing, &instructions)?;
                    if let Some(previous) = ws.fit_assessment(&job_id).map_err(|e| e.to_string())? {
                        if let Ok(previous) = serde_json::from_slice::<Value>(&previous) {
                            if let Some(comparison) =
                                fit_task::repeatability_comparison(&previous, &result)
                            {
                                result["assessment"]["repeatability"] = comparison;
                            }
                        }
                    }
                    ws.save_fit_assessment(&job_id, result.to_string().as_bytes())
                        .map_err(|e| format!("Could not save assessment; retry after resolving the workspace error: {e}"))?;
                    result["saved"] = json!(true);
                }
            }
            if result["status"] != "running" {
                fit_task::complete(id, result.clone())?;
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/fit/read" => {
            let job_id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", read_fit(ws, &job_id)?);
        }
        "/api/fit/discard" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            fit_task::discard(id)?;
            json_reply(stream, "200 OK", json!({"discarded":true}));
        }
        "/api/fit/pending" => {
            json_reply(stream, "200 OK", fit_task::pending()?);
        }
        "/api/fit/batch" => {
            let ids = body["ids"]
                .as_array()
                .filter(|ids| ids.len() <= 20)
                .ok_or("Supply at most 20 job IDs")?;
            let mut results = Vec::with_capacity(ids.len());
            let mut context = None;
            for id in ids {
                let id = JobIdentityId::new(id.as_str().ok_or("Invalid job ID")?)
                    .map_err(|e| e.to_string())?;
                let item = match read_fit_with_context(ws, &id, &mut context) {
                    Ok(value) if value["status"] == "current" => {
                        json!({"id":id.as_str(),"status":"current","score":value["assessment"]["score"],"score_range":value["assessment"]["score_range"]})
                    }
                    Ok(value) => json!({"id":id.as_str(),"status":value["status"]}),
                    Err(_) => json!({"id":id.as_str(),"status":"unavailable"}),
                };
                results.push(item);
            }
            json_reply(stream, "200 OK", json!({"results":results}));
        }
        "/api/search-plan/start" => {
            let (facts, instructions, digest, protected) = search_plan_context(ws)?;
            if facts.is_empty() && instructions.trim().is_empty() {
                return Err("Add qualifications or search instructions first".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let id = random_id()?;
            search_planner::start(id.clone(), model, facts, instructions, digest, protected)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/search-plan/pending" => {
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":search_planner::pending()?}),
            );
        }
        "/api/search-plan/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let result = search_planner::poll(id)?;
            if result["status"] == "complete"
                && result["input_digest"].as_str() != Some(search_plan_context(ws)?.2.as_str())
            {
                json_reply(
                    stream,
                    "200 OK",
                    json!({"status":"stale","error":"Profile, qualifications or search instructions changed; generate new phrases."}),
                );
            } else {
                json_reply(stream, "200 OK", result);
            }
        }
        "/api/search-plan/current" => {
            let digest = body["input_digest"]
                .as_str()
                .ok_or("missing input digest")?;
            json_reply(
                stream,
                "200 OK",
                json!({"current":digest == search_plan_context(ws)?.2}),
            );
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
            if !["", "on", "off"].contains(&profile["auto_assess_ai"].as_str().unwrap_or("")) {
                return Err("Invalid automatic assessment setting".into());
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
        "/api/email-draft" | "/api/gmail/prepare-application" => {
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
            let visual_inputs = match body.get("visual_reviews") {
                None | Some(Value::Null) => &[][..],
                Some(Value::Array(items)) if items.len() <= 5 => items.as_slice(),
                _ => return Err("Invalid visual review selections".into()),
            };
            let mut linked = std::collections::HashMap::new();
            for item in visual_inputs {
                let doc_id = item["document_id"]
                    .as_str()
                    .ok_or("Missing reviewed document")?;
                let attempt = item["attempt_id"].as_str().ok_or("Missing visual review")?;
                if item["reviewed"].as_bool() != Some(true) {
                    return Err("Review visual findings before linking them".into());
                }
                let (_, _, bytes) = attachments
                    .iter()
                    .find(|(id, _, _)| id == doc_id)
                    .ok_or("Visual review refers to an unselected attachment")?;
                if linked.contains_key(doc_id) {
                    return Err("Duplicate visual review selection".into());
                }
                linked.insert(
                    doc_id.to_owned(),
                    (
                        attempt.to_owned(),
                        verified_visual_review(ws, &id, attempt, bytes)?,
                    ),
                );
            }
            let recipient = body["recipient"].as_str().ok_or("missing recipient")?;
            let subject = body["subject"].as_str().ok_or("missing subject")?;
            let text = body["message"].as_str().ok_or("missing message")?;
            let pairs: Vec<_> = attachments
                .iter()
                .map(|(_, name, bytes)| (name.clone(), bytes.clone()))
                .collect();
            let attempt_id = random_id()?;
            let gmail_sender = if path == "/api/gmail/prepare-application" {
                Some(application_email::connected_sender(ws)?)
            } else {
                None
            };
            let draft = if let Some(sender) = gmail_sender.as_deref() {
                application_email::PreparedApplicationEmail::prepare(
                    sender,
                    recipient,
                    subject,
                    text,
                    &pairs,
                    &attempt_id,
                )?
                .mime()
                .to_vec()
            } else {
                email_draft::build(recipient, subject, text, &pairs, &attempt_id)?
            };
            // Reuse one bounded listing for all selected files; do not rescan
            // the workspace separately for each attachment.
            let evidence_listing = (!attachments.is_empty()).then(|| attempt_record::list(ws, &id));
            let mut record = attempt_record::AttemptRecord::start(ws, &id, &attempt_id, now())?
                .ok_or("Email draft requires a disk-backed workspace")?;
            let mut saved_materials = Vec::new();
            let mut missing_visual = false;
            let (mut evidence_incomplete, mut evidence_historical, mut evidence_linked) =
                (false, false, false);
            for (doc_id, name, bytes) in &attachments {
                let visual_review_file = if let Some((attempt, review)) = linked.get(doc_id) {
                    let file = record.document_visual_evidence(attempt, review)?;
                    record.event(json!({"type":"document_visual_review_linked","time":now(),"source_attempt":attempt,"file":file,"document":name,"note":"Advisory review, not proof of correctness."}))?;
                    Some(file)
                } else {
                    missing_visual = true;
                    None
                };
                let lookup = match evidence_listing.as_ref() {
                    Some(Ok(listing)) => attempt_record::matching_generated_evidence_in_listing(
                        ws, &id, doc_id, bytes, listing,
                    )
                    .unwrap_or(attempt_record::GeneratedEvidenceLookup {
                        found: None,
                        incomplete: true,
                    }),
                    _ => attempt_record::GeneratedEvidenceLookup {
                        found: None,
                        incomplete: true,
                    },
                };
                evidence_incomplete |= lookup.incomplete;
                if let Some(evidence) = lookup.found {
                    let current = draft_context_digest(ws, &id, &evidence.kind)
                        .ok()
                        .as_deref()
                        == Some(evidence.context_sha256.as_str());
                    evidence_historical |= !current;
                    record_generated_evidence(
                        &mut record,
                        &evidence,
                        GeneratedEvidenceLink {
                            document_id: doc_id,
                            document_name: name,
                            selector: None,
                            pdf: bytes,
                            context_current: current,
                            lookup_incomplete: lookup.incomplete,
                        },
                    )?;
                    evidence_linked = true;
                }
                let stored_name = record.material(name, bytes)?;
                saved_materials.push(attempt_record::GmailReviewAttachment {
                    name: name.clone(),
                    stored_name,
                    visual_review_file,
                });
            }
            if missing_visual {
                record.status("manual_review", now())?;
                ws.mark_manual_review(&id, "Email attachment needs visual review", now())
                    .map_err(|e| e.to_string())?;
                record.event(json!({"type":"document_visual_review_missing","time":now(),"note":"At least one email attachment has no linked visual review."}))?;
            }
            if evidence_incomplete || evidence_historical {
                record.status("manual_review", now())?;
                let reason = if evidence_incomplete {
                    "Email attachment draft evidence lookup was incomplete"
                } else {
                    "Email attachment draft evidence is from an older context"
                };
                ws.mark_manual_review(&id, reason, now())
                    .map_err(|e| e.to_string())?;
                record.event(json!({"type":"document_draft_evidence_review_needed","time":now(),"lookup_incomplete":evidence_incomplete,"historical":evidence_historical,"note":"Review document provenance and current qualifications manually."}))?;
            }
            record.email_draft(&draft)?;
            if let Some(sender) = gmail_sender.as_ref() {
                record.gmail_review(&attempt_record::GmailReview {
                    job_id: id.as_str().to_owned(),
                    attempt_id: attempt_id.clone(),
                    sender: sender.clone(),
                    recipient: recipient.trim().to_owned(),
                    subject: subject.to_owned(),
                    message: text.to_owned(),
                    attachments: saved_materials,
                    mime_sha256: waypoint_domain::sha256_hex(&draft),
                })?;
            }
            record.event(json!({"type":"email_draft_created", "time":now(), "sender":gmail_sender, "recipient":recipient, "subject":subject, "attachments":attachments.len(), "mime_sha256":waypoint_domain::sha256_hex(&draft), "sent":false}))?;
            let evidence_status = match (evidence_incomplete, evidence_historical, evidence_linked)
            {
                (true, true, _) => "incomplete-historical",
                (true, false, _) => "incomplete",
                (false, true, _) => "historical",
                (false, false, true) => "linked",
                (false, false, false) => "none",
            };
            email_document_reply(stream, &draft, evidence_status);
        }
        "/api/gmail/review-application" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
            json_reply(
                stream,
                "200 OK",
                attempt_record::gmail_review(ws, &id, attempt)?,
            );
        }
        "/api/gmail/approve-application" => {
            json_reply(stream, "200 OK", approve_saved_gmail(ws, &body)?);
        }
        "/api/gmail/rehearse-application" => {
            json_reply(stream, "200 OK", rehearse_saved_gmail(ws, &body)?);
        }
        "/api/email-answer/start" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|error| error.to_string())?;
            let input = email_answer_inputs(ws, &id)?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            answer_task::start(task_id.clone(), model, input)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/email-answer/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = answer_task::poll(id)?;
            if result["status"] == "complete" {
                let current = result["session_id"]
                    .as_str()
                    .and_then(|job| JobIdentityId::new(job).ok())
                    .and_then(|job| email_answer_inputs(ws, &job).ok());
                if result["selector"].as_str() != Some("email_body")
                    || current.as_ref().map(|input| input.digest.as_str())
                        != result["digest"].as_str()
                {
                    result = json!({"status":"failed","error":"The job, qualifications or instructions changed; draft the email again"});
                }
            }
            if result["status"] != "running" {
                result = answer_task::complete(id, result)?;
            }
            json_reply(stream, "200 OK", result);
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
            let kind = if lower.ends_with(".docx") {
                convert_task::ConversionKind::Docx
            } else if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
            {
                convert_task::ConversionKind::Image
            } else if lower.ends_with(".txt") || lower.ends_with(".md") {
                convert_task::ConversionKind::PlainText
            } else {
                return Err(
                    "Only DOCX, UTF-8 text, Markdown, PNG and JPEG conversion is supported".into(),
                );
            };
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
            convert_task::start(task_id.clone(), name, bytes, kind)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/convert-pdf-pending" => {
            json_reply(stream, "200 OK", convert_task::pending()?);
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
                let ((document, job), before) = result["document_id"]
                    .as_str()
                    .zip(result["job_id"].as_str())
                    .zip(result["before"].as_str())
                    .ok_or("Template suggestion context is unavailable")?;
                let job = JobIdentityId::new(job)
                    .map_err(|_| "Template suggestion context is invalid")?;
                let current = template_suggestion_inputs(ws, document, &job, before)
                    .map_err(|_| "Could not recheck template suggestion context; retry or inspect the document and job")?;
                if Some(current.digest.as_str()) != result["digest"].as_str() {
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
            let job_id = match body
                .get("job_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                Some(value) => {
                    let job = JobIdentityId::new(value).map_err(|e| e.to_string())?;
                    if ws.job(&job).map_err(|e| e.to_string())?.is_none() {
                        return Err("Selected job not found".into());
                    }
                    Some(value.to_owned())
                }
                None => None,
            };
            if let Some(items) = body.get("edits").and_then(Value::as_array) {
                validate_template_edit_bindings(ws, id, job_id.as_deref(), items)?;
            }
            let task_id = random_id()?;
            docx_edit_task::start_batch(
                ws,
                task_id.clone(),
                id.to_owned(),
                info.name,
                source,
                job_id,
                edits,
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/edit-template-pending" => {
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":docx_edit_task::pending()?}),
            );
        }
        "/api/documents/edit-template-status" => {
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", docx_edit_task::poll(task_id, ws)?);
        }
        "/api/pdf-probe/start" => {
            let id = random_id()?;
            pdf_probe_task::start(id.clone())?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/pdf-probe/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", pdf_probe_task::poll(id)?);
        }
        "/api/pdf-setup/start" => {
            let python = pdf_setup::select_python(
                body["python"].as_str().unwrap_or(""),
                pdf_extract::python_path,
            )?;
            let id = random_id()?;
            pdf_setup::start(id.clone(), python)?;
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
        "/api/documents/visual-review-start" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let job = JobIdentityId::new(body["job_id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let context = document_visual_context(ws, &job)?;
            let name = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|d| d.id == id)
                .ok_or("Document not found")?
                .name;
            if !name.to_ascii_lowercase().ends_with(".pdf") {
                return Err("Convert the final document to PDF first".into());
            }
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("Document not found")?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            // Visual model output is advisory, including when every page is processed.
            // Record the need for human review before starting the background worker,
            // so a failure or interruption does not silently leave the job unflagged.
            ws.mark_manual_review(
                &job,
                "Review the document and visual inspection results",
                now(),
            )
            .map_err(|e| e.to_string())?;
            document_visual::start(task_id.clone(), model, bytes, context, ws, &job, &name)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/visual-review-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", document_visual::poll(id)?);
        }
        "/api/documents/visual-review-candidates" => {
            let job = JobIdentityId::new(body["job_id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let id = body["document_id"].as_str().ok_or("missing document id")?;
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("Document not found")?;
            let context = document_visual_context(ws, &job)?;
            let candidates = attempt_record::matching_document_reviews(
                ws,
                &job,
                &waypoint_domain::sha256_hex(&bytes),
                Sha256Digest::of(&context).as_str(),
            )?;
            json_reply(stream, "200 OK", candidates);
        }
        "/api/documents/visual-review-read" => {
            let job = JobIdentityId::new(body["job_id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
            json_reply(
                stream,
                "200 OK",
                attempt_record::document_visual_review(ws, &job, attempt)?,
            );
        }
        "/api/documents/ats-check-start" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let job = JobIdentityId::new(body["job_id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let job_data = ws
                .job(&job)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let description = ws
                .job_description(&job)
                .map_err(|e| e.to_string())?
                .filter(|s| !s.trim().is_empty())
                .ok_or("This job has no saved description to compare")?;
            if description.len() > 200_000 {
                return Err("Job description exceeds the CV check limit".into());
            }
            let info = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|doc| doc.id == id)
                .ok_or("Document not found")?;
            if !info.name.to_ascii_lowercase().ends_with(".pdf")
                && !info.name.to_ascii_lowercase().ends_with(".docx")
            {
                return Err("Choose a PDF or DOCX CV".into());
            }
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("Document not found")?;
            let facts = fit_inputs(ws, &job)?.0;
            let task_id = random_id()?;
            ats_check::start(
                task_id.clone(),
                info.name,
                bytes,
                format!("{}\n{}", job_data.title, description),
                facts,
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/ats-check-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", ats_check::poll(id)?);
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
        "/api/duplicate-coverage" => {
            let (total, pending, bounded, missing_employer) =
                ws.duplicate_coverage_counts().map_err(|e| e.to_string())?;
            json_reply(
                stream,
                "200 OK",
                json!({"total":total,"pending":pending,"bounded":bounded,"missing_employer":missing_employer}),
            );
        }
        "/api/attempts/list" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            ws.job(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let mut listing = attempt_record::list(ws, &id)?;
            let (handoffs, truncated) = ws.email_handoff_history(&id)?;
            listing["email_handoffs"] = json!(handoffs.into_iter().map(|item| json!({"attempt_id":item.id,"state":item.state,"reconciliation":item.reconciliation,"created_at":item.created_at})).collect::<Vec<_>>());
            listing["email_handoffs_truncated"] = json!(truncated);
            json_reply(stream, "200 OK", listing);
        }
        "/api/gmail/reconcile-handoff" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Confirm that you checked the mailbox first".into());
            }
            if email_worker::busy() {
                return Err(
                    "Wait for the active email worker and collect its result before reconciling"
                        .into(),
                );
            }
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let attempt = body["attempt_id"].as_str().ok_or("missing handoff id")?;
            let (decision, label) = match body["decision"].as_str() {
                Some("sent") => (
                    waypoint_store::email_handoff::EmailReconciliation::UserReportsSent,
                    "user_reports_sent",
                ),
                Some("not_sent") => (
                    waypoint_store::email_handoff::EmailReconciliation::UserReportsNotSent,
                    "user_reports_not_sent",
                ),
                _ => return Err("Choose whether you found evidence of sending".into()),
            };
            let timestamp = now();
            ws.reconcile_email_handoff(&job, attempt, decision, timestamp, true)?;
            let record_saved =
                attempt_record::record_email_reconciliation(ws, &job, attempt, label, timestamp)
                    .is_ok();
            json_reply(
                stream,
                "200 OK",
                json!({"ok":true,"record_saved":record_saved,"email_sent_by_action":false}),
            );
        }
        "/api/review" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let required = body["required"].as_bool().ok_or("missing review status")?;
            let changed = if required {
                ws.mark_manual_review(&id, "Manual review requested", now())
                    .map_err(|e| e.to_string())?;
                true
            } else {
                let revision = body["revision"]
                    .as_i64()
                    .filter(|value| *value >= 0)
                    .ok_or("missing or invalid review revision")?;
                ws.clear_manual_review_if_current(&id, revision)
                    .map_err(|e| e.to_string())?
            };
            json_reply(stream, "200 OK", json!({"ok":true,"changed":changed}));
        }
        "/api/review/reasons" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let reasons = ws.manual_review_reasons(&id).map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"reasons": reasons}));
        }
        "/api/review/resolve" => {
            let id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let reason = body["reason"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or("missing review reason")?;
            let revision = body["revision"]
                .as_i64()
                .filter(|value| *value >= 0)
                .ok_or("missing or invalid review revision")?;
            let removed = ws
                .clear_manual_review_reason(&id, reason, revision)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"removed": removed}));
        }
        "/api/form/inspect-start" => {
            if application_check::busy()
                || form_task::busy()
                || candidate_task::busy()
                || form_links::busy()
            {
                return Err("Wait for the current browser inspection to finish".into());
            }
            if body["require_no_session"].as_bool() == Some(true)
                && session
                    .as_ref()
                    .is_some_and(|current| current.created.elapsed() <= current.ttl)
            {
                return Err(
                    "Close the active review session before opening the next queued job".into(),
                );
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
            let model = resident_model().ok().flatten();
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            interrupt_session(ws, session, "Another application was opened")?;
            let task_id = random_id()?;
            form_task::start(task_id.clone(), id, url, profile, model, visible)?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/inspect-status" => {
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            if let Some(error) = form_task::completed_failure(task_id)? {
                json_reply(stream, "200 OK", json!({"status":"failed","error":error}));
                return Ok(());
            }
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
                    ws.mark_manual_review(&job_id, &inspection_failure_reason(&error), now())
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
                    form_task::acknowledge_failure(task_id, error.clone())?;
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
            let (links, other_links, other_links_available) =
                form_links::links_for_review(&mut driver, &mut preview.manual_review);
            let mut record = match random_id()
                .and_then(|id| attempt_record::AttemptRecord::start(ws, &job_id, &id, now()))
            {
                Ok(Some(record)) => Some(record),
                Ok(None) => {
                    preview.manual_review.push(
                        "Attempt record unavailable: a disk-backed workspace is required".into(),
                    );
                    None
                }
                Err(error) => {
                    preview
                        .manual_review
                        .push(format!("Attempt record unavailable: {error}"));
                    None
                }
            };
            if let Some(record) = &mut record {
                if let Err(error) = attempt_record::capture_inspection(
                    &mut driver,
                    record,
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
                json!({"status":"complete","fields":preview.fields_for_review(),"repeated_label_selectors":preview.repeated_label_selectors(),"suggestions":preview.suggestions,"manual_review":preview.manual_review,"session_id":session_id,"origin":preview.schema.origin,"expires_in_seconds":ttl.as_secs(),"visible":visible,"screenshot":screenshot,"links":links,"other_links":other_links,"other_links_available":other_links_available}),
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
                completed_selectors: vec![],
                answer_drafts: Default::default(),
                visible,
                staged_files: vec![],
                attachments: vec![],
                last_visual_freshness_key: None,
                record,
            });
        }
        "/api/form/link-rank-start" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            let (context, links, snapshot_digest) = link_rank::snapshot(&mut current.driver)?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            link_rank::start(
                task_id.clone(),
                current.id.clone(),
                model,
                links,
                context,
                snapshot_digest,
            )?;
            current.created = Instant::now();
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/link-rank-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = link_rank::poll(id)?;
            if matches!(result["status"].as_str(), Some("running" | "complete")) {
                let current = session.as_mut().filter(|s| {
                    Some(s.id.as_str()) == result["session_id"].as_str()
                        && s.created.elapsed() <= s.ttl
                });
                if let Some(current) = current {
                    current.created = Instant::now();
                    if result["status"] == "complete" {
                        let fresh = link_rank::snapshot(&mut current.driver);
                        if fresh.as_ref().ok().map(|(_, _, digest)| digest.as_str())
                            != result["snapshot_digest"].as_str()
                        {
                            ws.mark_manual_review(
                                &current.job_id,
                                "Application page changed during link ranking",
                                now(),
                            )
                            .map_err(|e| e.to_string())?;
                            let note = "The application page changed or could not be rechecked; inspect it again.";
                            current.preview.manual_review.push(note.into());
                            if let Some(record) = &mut current.record {
                                let _ = record.event(
                                    json!({"type":"link_ranking_stale","time":now(),"note":note}),
                                );
                            }
                            result =
                                json!({"status":"failed","session_id":current.id,"error":note});
                        }
                    }
                } else if result["status"] == "complete" {
                    result = json!({"status":"failed","session_id":result["session_id"],"error":"The review session ended or changed; inspect the page again"});
                } else {
                    // Keep the task recoverable until its worker finishes. The
                    // final result is discarded because this session is gone.
                    result["session_unavailable"] = json!(true);
                }
            }
            if result["status"] != "running" {
                link_rank::complete(id, result.clone())?;
            }
            if let Some(object) = result.as_object_mut() {
                object.remove("snapshot_digest");
            }
            json_reply(stream, "200 OK", result);
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
            let allow_other = body["allow_other"].as_bool() == Some(true);
            let reviewed_label = if allow_other {
                Some(
                    body["reviewed_label"]
                        .as_str()
                        .filter(|label| label.chars().count() <= 200)
                        .ok_or("Missing or invalid reviewed link label")?
                        .to_owned(),
                )
            } else {
                None
            };
            let task_id = random_id()?;
            let current = session.take().ok_or("Form session expired")?;
            if let Err((current, error)) = form_links::start(
                task_id.clone(),
                current,
                Some(target.to_owned()),
                allow_other,
                reviewed_label,
                profile,
            ) {
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
                    if !error.review_saved {
                        ws.mark_manual_review(
                            &error.job_id,
                            "Application navigation stopped; review the application manually",
                            now(),
                        )
                        .map_err(|e| {
                            format!("Navigation failed and its review flag could not be saved: {e}")
                        })?;
                    }
                    form_links::acknowledge_failure(id)?;
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"failed","error":error.reason}),
                    )
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
                    let (links, other_links, other_links_available) = form_links::links_for_review(
                        &mut current.driver,
                        &mut current.preview.manual_review,
                    );
                    let screenshot = current.driver.screenshot_base64().ok();
                    if screenshot.is_none() {
                        current
                            .preview
                            .manual_review
                            .push("Inspection screenshot is unavailable".into());
                    }
                    if let Some(record) = &mut current.record {
                        if let Err(error) = attempt_record::capture_inspection(
                            &mut current.driver,
                            record,
                            &current.preview.schema.origin,
                            &current.preview.manual_review,
                            screenshot.as_deref(),
                            now(),
                        ) {
                            current
                                .preview
                                .manual_review
                                .push(format!("Inspection evidence incomplete: {error}"));
                        }
                    }
                    if !current.preview.manual_review.is_empty() {
                        ws.mark_manual_review(
                            &current.job_id,
                            "Application needs manual review",
                            now(),
                        )
                        .map_err(|e| e.to_string())?;
                    }
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"complete","fields":current.preview.fields_for_review(),"repeated_label_selectors":current.preview.repeated_label_selectors(),"suggestions":current.preview.suggestions,"manual_review":current.preview.manual_review,"session_id":current.id,"origin":current.preview.schema.origin,"expires_in_seconds":remaining_session_seconds(current.ttl,current.created.elapsed()),"visible":current.visible,"screenshot":screenshot,"links":links,"other_links":other_links,"other_links_available":other_links_available}),
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
                form_links::start(task_id.clone(), current, None, false, None, profile)
            {
                *session = Some(*current);
                return Err(error);
            }
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/visual-review-start" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let segment = body["segment"].as_u64().unwrap_or(0);
            if segment > 5 {
                return Err("Visual segment is outside the capture limit".into());
            }
            let half = match body.get("half") {
                None | Some(Value::Null) => None,
                Some(Value::Number(value)) if value.as_u64().is_some_and(|n| n <= 1) => {
                    Some(value.as_u64().unwrap() as usize)
                }
                _ => return Err("Invalid visual segment subdivision".into()),
            };
            let (width, height) = current
                .driver
                .page_dimensions()
                .map_err(|e| e.to_string())?;
            let (clip_width, y, clip_height, total, coverage_complete) =
                visual_review::segment_half_geometry(width, height, segment as usize, half)?;
            let url = current.driver.current_url().map_err(|e| e.to_string())?;
            let signature = visual_review::page_signature(&url, width, height);
            let screenshot = current
                .driver
                .screenshot_segment(clip_width, y, clip_height)
                .map_err(|e| e.to_string())?;
            let task_id = random_id()?;
            let (expected, comparisons_truncated) = visual_review::expected_from_preview(
                &current.preview,
                &current.completed_selectors,
            );
            let result_path = current
                .record
                .as_ref()
                .and_then(|record| record.visual_result_path(&task_id).ok());
            visual_review::start(
                task_id.clone(),
                current.id.clone(),
                (signature.as_str().to_owned(), segment as usize, half),
                model,
                screenshot.clone(),
                expected,
                result_path,
            )?;
            let record_saved = current.record.as_mut().is_some_and(|record| {
                record.screenshot_with_name(&screenshot).and_then(|name| {
                    record.event(json!({"type":"visual_capture","time":now(),"task_id":task_id,"segment":segment,"half":half,"segments":total,"coverage_complete":coverage_complete,"comparisons_truncated":comparisons_truncated,"page_signature":signature.as_str(),"screenshot":format!("screenshots/{name}"),"result_file":format!("visual-result-{task_id}.json")}))
                }).is_ok()
            });
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":task_id,"screenshot":screenshot,"segment":segment,"half":half,"segments":total,"coverage_complete":coverage_complete,"comparisons_truncated":comparisons_truncated,"page_signature":signature.as_str(),"record_saved":record_saved}),
            );
        }
        "/api/form/visual-review-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = visual_review::poll(id)?;
            if matches!(result["status"].as_str(), Some("running" | "complete")) {
                let current = session
                    .as_mut()
                    .filter(|s| {
                        Some(s.id.as_str()) == result["session_id"].as_str()
                            && s.created.elapsed() <= s.ttl
                    })
                    .ok_or(
                        "The form session expired; this visual observation is no longer current",
                    )?;
                if result["status"] == "complete" {
                    let url = current.driver.current_url().map_err(|e| e.to_string())?;
                    let (width, height) = current
                        .driver
                        .page_dimensions()
                        .map_err(|e| e.to_string())?;
                    if result["page_signature"].as_str()
                        != Some(visual_review::page_signature(&url, width, height).as_str())
                    {
                        ws.mark_manual_review(&current.job_id, "Form changed during visual review", now())
                            .map_err(|e| format!("The page changed, and its manual-review flag could not be saved: {e}"))?;
                        let saved = current.record.as_mut().is_some_and(|record| record.event(json!({"type":"visual_page_changed","time":now(),"task_id":id,"note":"The page URL or dimensions changed after the reviewed capture; inspect the live page."})).is_ok());
                        let record_note = if saved {
                            ""
                        } else {
                            " The stale-review event could not be saved to the application record."
                        };
                        return Err(format!("The page URL or dimensions changed; this visual observation is no longer current.{record_note}"));
                    }
                    if result["visual_review"]["observation_limit_reached"] == true {
                        let reason = "Visual segment reached the observation limit; inspect remaining fields manually";
                        ws.mark_manual_review(&current.job_id, reason, now())
                            .map_err(|e| format!("Visual coverage may be incomplete, but its manual-review flag could not be saved: {e}"))?;
                        if !current
                            .preview
                            .manual_review
                            .iter()
                            .any(|item| item == reason)
                        {
                            current.preview.manual_review.push(reason.into());
                        }
                    }
                    let fresh_screenshot = result["segment"].as_u64().and_then(|segment| {
                        let (clip_width, y, clip_height, _, _) =
                            visual_review::segment_half_geometry(
                                width,
                                height,
                                segment as usize,
                                result["half"]
                                    .as_u64()
                                    .and_then(|h| usize::try_from(h).ok()),
                            )
                            .ok()?;
                        current
                            .driver
                            .screenshot_segment(clip_width, y, clip_height)
                            .ok()
                    });
                    let (verified, unconfirmed) = visual_review::crosscheck_live_values(
                        &mut current.driver,
                        &current.preview,
                        &current.completed_selectors,
                        &mut result["visual_review"],
                    );
                    let final_screenshot = if verified + unconfirmed == 0 {
                        fresh_screenshot.clone()
                    } else {
                        result["segment"].as_u64().and_then(|segment| {
                            let (clip_width, y, clip_height, _, _) =
                                visual_review::segment_half_geometry(
                                    width,
                                    height,
                                    segment as usize,
                                    result["half"]
                                        .as_u64()
                                        .and_then(|h| usize::try_from(h).ok()),
                                )
                                .ok()?;
                            current
                                .driver
                                .screenshot_segment(clip_width, y, clip_height)
                                .ok()
                        })
                    };
                    let mut freshness = visual_review::interval_freshness(
                        result["screenshot_hash"].as_str(),
                        fresh_screenshot.as_deref(),
                        final_screenshot.as_deref(),
                    );
                    let mut final_signature_status = "not_rechecked";
                    if verified + unconfirmed > 0 && freshness != "changed" {
                        let final_signature = current.driver.current_url().ok().and_then(|url| {
                            current
                                .driver
                                .page_dimensions()
                                .ok()
                                .map(|(width, height)| {
                                    visual_review::page_signature(&url, width, height)
                                })
                        });
                        final_signature_status = match final_signature {
                            Some(signature)
                                if Some(signature.as_str())
                                    == result["page_signature"].as_str() =>
                            {
                                "same"
                            }
                            Some(_) => "changed",
                            None => "unavailable",
                        };
                        if final_signature_status == "changed" {
                            freshness = "changed";
                        }
                        if final_signature_status == "unavailable" {
                            freshness = "unavailable";
                        }
                    }
                    let changed_image = if visual_review::image_freshness(
                        result["screenshot_hash"].as_str(),
                        final_screenshot.as_deref(),
                    ) == "changed"
                    {
                        final_screenshot.as_deref()
                    } else {
                        fresh_screenshot.as_deref()
                    };
                    result["visual_freshness"] = json!(freshness);
                    result["final_signature_status"] = json!(final_signature_status);
                    if unconfirmed > 0 {
                        let reason = "A visually transcribed field could not be verified against its live value";
                        ws.mark_manual_review(&current.job_id, reason, now())
                            .map_err(|e| format!("Live field verification needs review, but its flag could not be saved: {e}"))?;
                        if !current
                            .preview
                            .manual_review
                            .iter()
                            .any(|item| item == reason)
                        {
                            current.preview.manual_review.push(reason.into());
                        }
                    }
                    if freshness != "same" {
                        let reason = if freshness == "changed" {
                            "Form changed during visual review"
                        } else {
                            "Form could not be recaptured after visual review"
                        };
                        ws.mark_manual_review(&current.job_id, reason, now())
                            .map_err(|e| format!("The visual check needs manual review, but its flag could not be saved: {e}"))?;
                        if !current
                            .preview
                            .manual_review
                            .iter()
                            .any(|item| item == reason)
                        {
                            current.preview.manual_review.push(reason.into());
                        }
                    }
                    let image_key = visual_review::freshness_record_key(
                        id,
                        freshness,
                        if freshness == "changed" {
                            changed_image
                        } else {
                            final_screenshot.as_deref()
                        },
                    );
                    let freshness_key = Sha256Digest::of(&json!({"image_key":image_key,"final_signature_status":final_signature_status,"comparisons":result["visual_review"]["comparisons"]}).to_string()).as_str().to_owned();
                    if current.last_visual_freshness_key.as_deref() != Some(freshness_key.as_str())
                    {
                        let saved = current.record.as_mut().is_some_and(|record| {
                            let captured = if freshness == "changed" {
                                changed_image.and_then(|image| record.screenshot_with_name(image).ok())
                            } else { None };
                            if freshness == "changed" && captured.is_none() { return false; }
                            record.event(json!({"type":"visual_freshness_checked","time":now(),"task_id":id,"status":freshness,"final_signature_status":final_signature_status,"screenshot":captured.map(|name| format!("screenshots/{name}")),"live_values_verified":verified,"live_values_unconfirmed":unconfirmed,"comparisons":result["visual_review"]["comparisons"],"note":"Image and live-value checks do not prove correctness; values can change."})).is_ok()
                        });
                        if saved {
                            current.last_visual_freshness_key = Some(freshness_key);
                        }
                        result["freshness_record_saved"] = json!(saved);
                    } else {
                        // The same task, status and image were already recorded.
                        result["freshness_record_saved"] = json!(false);
                    }
                }
                current.created = Instant::now();
            }
            json_reply(stream, "200 OK", result);
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
            if result["status"] != "running" {
                result = answer_task::complete(id, result)?;
            }
            if result["status"] == "complete" {
                let current = session
                    .as_mut()
                    .filter(|s| Some(s.id.as_str()) == result["session_id"].as_str())
                    .ok_or("Form session expired")?;
                let available =
                    current.answer_drafts.contains_key(id) || current.answer_drafts.len() < 20;
                if available {
                    current.answer_drafts.insert(
                        id.to_owned(),
                        SavedFormAnswer {
                            selector: result["selector"]
                                .as_str()
                                .ok_or("Missing answer field")?
                                .to_owned(),
                            digest: result["digest"]
                                .as_str()
                                .ok_or("Missing answer digest")?
                                .to_owned(),
                            answer: result["answer"].clone(),
                        },
                    );
                }
                result["provenance_available"] = json!(available);
            } else if result["status"] == "failed" {
                if let Some(current) = session.as_mut() {
                    current.answer_drafts.remove(id);
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
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(profile) => {
                    serde_json::from_str(&profile).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            if Sha256Digest::of(&profile.to_string()) != current.profile_digest {
                return Err("Profile changed; inspect the current page again".into());
            }
            let (prepared, omitted) = prepare_reviewed_fill(&current.preview, &body)?;
            let mut requested_fields = form_preview::requested_field_records(&prepared)?;
            attach_answer_provenance(ws, current, &body, &mut requested_fields)?;
            let intent =
                IntentId::new(format!("prefill-{}", current.id)).map_err(|e| e.to_string())?;
            let grant_id = GrantId::new(random_id()?).map_err(|e| e.to_string())?;
            let payload =
                serde_json::to_string(&prepared.suggestions).map_err(|e| e.to_string())?;
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
            let mut current = session.take().ok_or("Form session expired")?;
            if current.created.elapsed() > current.ttl {
                return Err("Form session expired; inspect it again".into());
            }
            let original_preview = current.preview.clone();
            if omitted {
                if let Err(error) =
                    ws.mark_manual_review(&current.job_id, "Application needs manual review", now())
                {
                    *session = Some(current);
                    return Err(error.to_string());
                }
                current
                    .preview
                    .manual_review
                    .push("Some suggested fields were left for manual review".into());
            }
            current.preview.suggestions = prepared.suggestions;
            form_preview::refresh_required_review(&mut current.preview);
            if current.record.is_none() {
                match required_attempt_record(ws, &current.job_id) {
                    Ok(record) => current.record = Some(record),
                    Err(error) => {
                        let review_error = ws
                            .mark_manual_review(
                                &current.job_id,
                                "Application record could not be created; no fields were filled",
                                now(),
                            )
                            .err();
                        current.preview = original_preview;
                        *session = Some(current);
                        let note = review_error
                            .map(|e| format!(" The manual-review flag could not be saved: {e}."))
                            .unwrap_or_default();
                        return Err(format!("No fields were filled because the attempt record could not be created: {error}. The review session remains open.{note}"));
                    }
                }
            }
            if let Some(record) = &mut current.record {
                if let Err(error) = record.event(json!({"type":"fields_requested","time":now(),"origin":current.preview.schema.origin,"answers":current.preview.suggestions,"fields":requested_fields,"note":"These are requested answers, not proof that the fields were filled or submitted."})) {
                    let review_error = ws.mark_manual_review(&current.job_id, "Application record could not be written; no fields were filled", now()).err();
                    current.preview = original_preview;
                    *session = Some(current);
                    let note = review_error.map(|e| format!(" The manual-review flag could not be saved: {e}.")).unwrap_or_default();
                    return Err(format!("No fields were filled because the attempt record could not be written: {error}. The review session remains open.{note}"));
                }
            }
            current.filled = true;
            let mut completed = Vec::new();
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
                    completed: &mut completed,
                },
            );
            current.completed_selectors = completed
                .iter()
                .filter_map(|item| item["selector"].as_str().map(str::to_owned))
                .collect();
            let mut attachment_changed = false;
            for (selector, path, _) in current
                .attachments
                .iter()
                .filter(|(_, _, inspection)| inspection == &current.id)
            {
                if current.driver.verify_local_file(selector, path).is_err() {
                    attachment_changed = true;
                    current.preview.manual_review.push(format!(
                        "Attachment at {selector} could not be verified after filling; inspect it manually"
                    ));
                    if let Some(record) = &mut current.record {
                        let _ = record.event(json!({"type":"attachment_verification_failed","time":now(),"selector":selector}));
                    }
                }
            }
            let filled = match result {
                Ok(count) => count,
                Err(error) => {
                    if let Some(record) = &mut current.record {
                        let _ = record.event(json!({"type":"fill_incomplete","time":now(),"error":error,"completed_commands":completed,"note":"The failing field may have changed and earlier values may have been changed by the site; verify all fields."}));
                        if let Ok(image) = current.driver.screenshot_base64() {
                            if let Ok(name) = record.screenshot_with_name(&image) {
                                let _ = record.event(json!({"type":"fill_failure_viewport","time":now(),"screenshot":format!("screenshots/{name}")}));
                            }
                        }
                        if let Err(capture_error) =
                            attempt_record::capture_page(&mut current.driver, record, now())
                        {
                            let _ = record.event(json!({"type":"capture_incomplete","time":now(),"error":capture_error}));
                        }
                    }
                    let review_error = ws
                        .mark_manual_review(
                            &current.job_id,
                            "Application needs manual review",
                            now(),
                        )
                        .err();
                    current.created = Instant::now();
                    *session = Some(current);
                    let flag_note = review_error
                        .map(|e| format!(" The manual-review flag could not be saved: {e}."))
                        .unwrap_or_default();
                    let attachment_note = if attachment_changed {
                        " An attachment could not be verified; check it manually."
                    } else {
                        ""
                    };
                    return Err(format!(
                        "Autofill stopped after {} successful field commands; the failing field and earlier fields may have changed. Review the browser: {error}.{attachment_note}{flag_note}", completed.len()
                    ));
                }
            };
            let screenshot = match current.driver.screenshot_base64() {
                Ok(image) => image,
                Err(error) => {
                    let review_error = ws
                        .mark_manual_review(
                            &current.job_id,
                            "Application needs manual review",
                            now(),
                        )
                        .err();
                    current.created = Instant::now();
                    *session = Some(current);
                    let flag_note = review_error
                        .map(|e| format!(" The manual-review flag could not be saved: {e}."))
                        .unwrap_or_default();
                    return Err(format!(
                        "Fields were filled, but the review screenshot failed: {error}.{flag_note} Inspect the open browser manually."
                    ));
                }
            };
            if let Some(record) = &mut current.record {
                if let Err(error) = record
                    .screenshot_with_name(&screenshot)
                    .and_then(|name| {
                        record.event(json!({"type":"fields_filled","time":now(),"count":filled,"screenshot":format!("screenshots/{name}")}))
                    })
                    .and_then(|_| attempt_record::capture_page(&mut current.driver, record, now()))
                {
                    let _ = record
                        .event(json!({"type":"capture_incomplete","time":now(),"error":error}));
                    current
                        .preview
                        .manual_review
                        .push("Attempt screenshot capture was incomplete".into());
                }
            }
            if let Err(error) = ws.mark_manual_review(
                &current.job_id,
                "Visual inspection and user review required",
                now(),
            ) {
                current.created = Instant::now();
                *session = Some(current);
                return Err(format!("Fields were filled, but the review flag could not be saved: {error}. Inspect the form manually."));
            }
            if let Some(record) = &mut current.record {
                if let Err(error) = record.status("manual_review", now()) {
                    current.created = Instant::now();
                    *session = Some(current);
                    return Err(format!("Fields were filled, but the attempt status could not be saved: {error}. Inspect the form manually."));
                }
            }
            current.preview.manual_review.push("Visual verification and user review are required; model observations may be incomplete or wrong".into());
            current.created = Instant::now();
            let (filled_fields, required_not_filled) =
                form_preview::fill_coverage(&current.preview);
            json_reply(
                stream,
                "200 OK",
                json!({"filled":filled,"filled_fields":filled_fields,"required_not_filled":required_not_filled,"screenshot":screenshot,"manual_review":current.preview.manual_review,"expires_in_seconds":remaining_session_seconds(current.ttl,current.created.elapsed())}),
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
            if !form_preview::same_reviewable_walls(&current.preview, &live.walls)
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
                || current.preview.model_review_selectors.contains(selector)
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
            let name = attempt_record::attachment_filename(&doc.name);
            if body["expected_attachment_name"]
                .as_str()
                .is_some_and(|expected| expected != name)
            {
                return Err("Attachment filename changed; review it before attaching".into());
            }
            let visual_review = match body["visual_review_attempt"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                Some(attempt) => {
                    if body["reviewed_visual_findings"].as_bool() != Some(true) {
                        return Err("Review the visual findings before linking them".into());
                    }
                    let review = verified_visual_review(ws, &current.job_id, attempt, &bytes)?;
                    Some((attempt, review))
                }
                None => None,
            };
            let lookup = attempt_record::matching_generated_evidence(
                ws,
                &current.job_id,
                document_id,
                &bytes,
            )
            .unwrap_or(attempt_record::GeneratedEvidenceLookup {
                found: None,
                incomplete: true,
            });
            let provenance_incomplete = lookup.incomplete;
            let generated_evidence = lookup.found;
            let context_current = generated_evidence.as_ref().is_some_and(|evidence| {
                draft_context_digest(ws, &current.job_id, &evidence.kind)
                    .ok()
                    .as_deref()
                    == Some(evidence.context_sha256.as_str())
            });
            let mut draft_provenance_status = if provenance_incomplete {
                "lookup_incomplete"
            } else {
                "not_found"
            };
            if current.record.is_none() {
                current.record = Some(required_attempt_record(ws, &current.job_id)?);
            }
            if visual_review.is_none() {
                ws.mark_manual_review(
                    &current.job_id,
                    "Attached document needs visual review",
                    now(),
                )
                .map_err(|e| e.to_string())?;
                current.preview.manual_review.push(
                    "This attachment has no linked current visual review; inspect it manually"
                        .into(),
                );
            }
            if provenance_incomplete {
                ws.mark_manual_review(
                    &current.job_id,
                    "Generated document evidence lookup was incomplete",
                    now(),
                )
                .map_err(|e| e.to_string())?;
                current.preview.manual_review.push("Generated document evidence lookup was incomplete; inspect the application record manually".into());
            }
            if generated_evidence.is_some() && !context_current {
                ws.mark_manual_review(
                    &current.job_id,
                    "Attached generated document is from an older context",
                    now(),
                )
                .map_err(|e| e.to_string())?;
            }
            if generated_evidence.is_some() && current.record.is_none() {
                draft_provenance_status = "record_unavailable";
            }
            if let Some(record) = &mut current.record {
                if let Some((attempt, review)) = &visual_review {
                    let path = record.document_visual_evidence(attempt, review)?;
                    record.event(json!({"type":"document_visual_review_linked","time":now(),"source_attempt":attempt,"file":path,"note":"Advisory review, not proof of correctness."}))?;
                } else {
                    record.status("manual_review", now())?;
                }
                if let Some(evidence) = &generated_evidence {
                    draft_provenance_status = record_generated_evidence(
                        record,
                        evidence,
                        GeneratedEvidenceLink {
                            document_id,
                            document_name: &doc.name,
                            selector: Some(selector),
                            pdf: &bytes,
                            context_current,
                            lookup_incomplete: provenance_incomplete,
                        },
                    )?;
                    if !context_current {
                        current.preview.manual_review.push("Attached generated document is from an older context; review the document and qualifications".into());
                    }
                }
                let stored = record.material(&doc.name, &bytes)?;
                record.event(json!({"type":"document_requested","time":now(),"selector":selector,"name":doc.name,"attachment_name":name,"material":stored}))?;
            }
            let dir = tempfile::Builder::new()
                .prefix("waypoint-document-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            let path = dir.path().join(&name);
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
            current.attachments.retain(|(previous, _, inspection)| {
                previous != selector || inspection != &current.id
            });
            current
                .attachments
                .push((selector.to_owned(), path, current.id.clone()));
            if let Some(record) = &mut current.record {
                record.event(json!({"type":"document_attached","time":now(),"selector":selector,"name":doc.name,"attachment_name":name}))?;
                if let Ok(image) = current.driver.screenshot_base64() {
                    let name = record.screenshot_with_name(&image)?;
                    record.event(json!({"type":"attachment_viewport","time":now(),"selector":selector,"screenshot":format!("screenshots/{name}")}))?;
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
                json!({"ok":true,"name":doc.name,"attachment_name":name,"draft_provenance":draft_provenance_status,"provenance_lookup_incomplete":provenance_incomplete,"manual_review":current.preview.manual_review}),
            );
        }
        "/api/form/session-status" => {
            let current = session
                .as_ref()
                .filter(|current| current.created.elapsed() <= current.ttl);
            let pending_inspection = form_task::pending()?;
            json_reply(
                stream,
                "200 OK",
                match current {
                    Some(current) => {
                        json!({"active":true,"session_id":current.id,"job_id":current.job_id.as_str(),"visible":current.visible})
                    }
                    None => {
                        json!({"active":false,"inspection":pending_inspection.map(|(task,job)| json!({"task":task,"job":job}))})
                    }
                },
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
            let digest = body["context_digest"]
                .as_str()
                .ok_or("Draft context is missing; draft again")?;
            if draft_context_digest(ws, &id, kind)? != digest {
                return Err(
                    "The job, qualifications, profile or template changed; draft and review again"
                        .into(),
                );
            }
            let draft_provenance = match body.get("draft_id") {
                None | Some(Value::Null) => None,
                Some(Value::String(draft_id)) if !draft_id.is_empty() && draft_id.len() <= 128 => {
                    match draft_task::poll(draft_id) {
                        Ok(cached) if cached["status"] == "complete" => {
                            Some(draft_task::export_provenance(
                                &cached,
                                draft_id,
                                id.as_str(),
                                kind,
                                digest,
                                &text,
                            )?)
                        }
                        Ok(_) => None,
                        Err(error) if error == "Draft task not found" => None,
                        Err(error) => return Err(error),
                    }
                }
                _ => return Err("Invalid draft identifier".into()),
            };
            let provenance_available = draft_provenance.is_some();
            let task_id = random_id()?;
            letter_export::start(
                task_id.clone(),
                id.as_str().to_owned(),
                kind.to_owned(),
                text,
                digest.to_owned(),
                draft_provenance,
            )?;
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":task_id,"provenance_available":provenance_available}),
            );
        }
        "/api/letter/export-pending" => {
            let pending = letter_export::pending()?;
            json_reply(
                stream,
                "200 OK",
                match pending {
                    Some((task_id, job_id)) => json!({"task_id":task_id,"job_id":job_id}),
                    None => json!({"task_id":null}),
                },
            );
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
            let (all_facts, _) = fit_candidate_context(ws)?;
            if all_facts.is_empty() {
                return Err("Add verified qualifications before drafting".into());
            }
            let priority_facts = draft_priority_facts(ws, &body["priority_ids"])?;
            let mut contact_facts = Vec::new();
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            if kind == "cv" {
                for (key, label) in [
                    ("full_name", "Full name"),
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
                        contact_facts.push(format!("{label}: {value}"));
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
            let selection_listing = format!(
                "Title: {}\nEmployer: {}\nLocation: {}\nDescription: {}",
                job.title, job.employer, job.location, description
            );
            let facts = fit_semantic_pool(&all_facts, &selection_listing);
            let model =
                resident_model()?.ok_or("No eligible already-loaded local model is available")?;
            let task_id = random_id()?;
            let digest = draft_context_digest(ws, &id, kind)?;
            draft_task::start(
                task_id.clone(),
                model,
                draft_task::Inputs {
                    job_id: id.as_str().to_owned(),
                    digest,
                    job: job_info,
                    kind: kind.to_owned(),
                    facts,
                    available_facts: all_facts.len(),
                    contact_facts,
                    priority_facts,
                    selection_listing,
                    instructions,
                    template,
                },
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/draft/pending" => {
            json_reply(
                stream,
                "200 OK",
                match draft_task::pending()? {
                    Some((task_id, job_id)) => json!({"task_id":task_id,"job_id":job_id}),
                    None => json!({"task_id":null}),
                },
            );
        }
        "/api/draft/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = draft_task::poll(id)?;
            if result["status"] == "complete" {
                let job_id =
                    JobIdentityId::new(result["job_id"].as_str().ok_or("missing draft job")?)
                        .map_err(|e| e.to_string())?;
                let kind = result["kind"].as_str().ok_or("missing draft type")?;
                if draft_context_digest(ws, &job_id, kind).ok().as_deref()
                    != result["digest"].as_str()
                {
                    result = json!({"status":"failed","error":"The job, qualifications, profile or template changed while drafting; draft again"});
                }
            }
            json_reply(stream, "200 OK", result);
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

fn main() {
    if let Err(error) = run() {
        let message = startup_error_message(&error.to_string());
        eprintln!("{message}");
        #[cfg(windows)]
        show_startup_error(&message);
        std::process::exit(1);
    }
}

fn startup_error_message(error: &str) -> String {
    let detail: String = error
        .chars()
        .take(1000)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let detail = detail.trim();
    if detail.is_empty() {
        "Waypoint could not start. Check the workspace and try again.".into()
    } else {
        format!("Waypoint could not start. Check the workspace and try again.\n\n{detail}")
    }
}

/// Use a fixed system command and stdin for the message; user data is never
/// interpolated into a shell command. The child has no console window.
#[cfg(windows)]
fn show_startup_error(message: &str) {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let Some(system_root) = std::env::var_os("SystemRoot") else {
        return;
    };
    let executable = std::path::PathBuf::from(system_root)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; $message = [Console]::In.ReadToEnd(); Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show($message, 'Waypoint startup error', 'OK', 'Error') | Out-Null";
    if let Ok(mut child) = Command::new(executable)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(message.as_bytes());
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
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
    let mut next_email_collect = Instant::now();
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        if Instant::now() >= next_email_collect {
            match email_worker::collect_finished(&mut ws) {
                Ok(Some(email_worker::WorkerStatus::Finished {
                    record_saved: Some(false),
                    ..
                })) => eprintln!(
                    "Email outcome was saved in the database but not in its attempt folder"
                ),
                Ok(_) => {}
                Err(error) => eprintln!("Email outcome could not be recorded: {error}"),
            }
            next_email_collect = Instant::now() + Duration::from_secs(1);
        }
        if session
            .as_ref()
            .is_some_and(|s| s.created.elapsed() > s.ttl)
        {
            if let Err(error) = interrupt_session(&mut ws, &mut session, "Review session expired") {
                eprintln!("Could not record expired review session: {error}");
                session = None;
            }
        }
        if let Err(error) = link_rank::collect_orphaned(session.as_ref().map(|s| s.id.as_str())) {
            eprintln!("Could not release an expired link ranking: {error}");
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
    #[test]
    fn application_record_is_required_for_memory_only_workspace() {
        let ws = Workspace::in_memory().unwrap();
        let job = JobIdentityId::new("fictional-job").unwrap();
        let error = match required_attempt_record(&ws, &job) {
            Ok(_) => panic!("an in-memory workspace cannot create the required record"),
            Err(error) => error,
        };
        assert!(error.contains("disk-backed workspace"));
    }
    #[test]
    #[ignore = "uses installed LibreOffice to convert a fictional saved Markdown file"]
    fn markdown_conversion_endpoint_saves_plain_text_pdf() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let _guard = convert_task::TEST_LOCK.lock().unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        ws.add_document(
            "fictional-text",
            "fictional.md",
            "# Fictional CV\nMünchen\n".as_bytes(),
            1,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"id":"fictional-text"}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/documents/convert-pdf-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let start: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        let id = start["task_id"].as_str().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/documents/convert-pdf-pending HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: 2\r\n\r\n{{}}").unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let pending: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(pending["task_id"], id);
        assert_eq!(pending["active"], true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
        loop {
            let result = convert_task::poll(id, &ws).unwrap();
            if result["status"] == "complete" {
                assert_eq!(result["name"], "fictional.pdf");
                assert!(ws
                    .document_content(result["id"].as_str().unwrap())
                    .unwrap()
                    .unwrap()
                    .starts_with(b"%PDF-"));
                assert_eq!(ws.documents().unwrap().len(), 2);
                break;
            }
            assert_eq!(result["status"], "running", "{result}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    fn search_plan_digest_tracks_reviewed_facts_and_instructions() {
        let mut ws = Workspace::in_memory().unwrap();
        let id = ws
            .add_fact("Fictional engineer", FactOrigin::UserAttested)
            .unwrap();
        ws.save_candidate_profile(&json!({"instructions":"Remote"}).to_string())
            .unwrap();
        let digest = search_plan_context(&ws).unwrap().2;
        assert!(check_search_plan_binding(&ws, &json!({"plan_digest":digest})).is_ok());
        ws.save_candidate_profile(&json!({"instructions":"Hybrid"}).to_string())
            .unwrap();
        assert_ne!(search_plan_context(&ws).unwrap().2, digest);
        ws.save_candidate_profile(&json!({"instructions":"Remote"}).to_string())
            .unwrap();
        assert_eq!(search_plan_context(&ws).unwrap().2, digest);
        ws.save_candidate_profile(
            &json!({"instructions":"Remote","first_name":"Ada","last_name":"Example"}).to_string(),
        )
        .unwrap();
        assert_ne!(search_plan_context(&ws).unwrap().2, digest);
        ws.save_candidate_profile(&json!({"instructions":"Remote"}).to_string())
            .unwrap();
        ws.correct_fact(&id, "Fictional analyst", 1).unwrap();
        assert_ne!(search_plan_context(&ws).unwrap().2, digest);
        assert!(check_search_plan_binding(&ws, &json!({"plan_digest":digest})).is_err());
        assert!(check_search_plan_binding(&ws, &json!({})).is_ok());
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};
        for (path, body) in [
            (
                "/api/web-search/start",
                json!({"provider":"http://127.0.0.1:9/","query":"fictional engineer","plan_digest":digest}),
            ),
            (
                "/api/discover/start",
                json!({"targets":[{"source":"jobicy","board":"fictional engineer"}],"plan_digest":digest}),
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let body = body.to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            let error = serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap_err();
            assert!(error.contains("changed"));
        }
    }
    #[cfg(windows)]
    #[test]
    fn saved_salary_evidence_endpoint_reads_captured_source() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let directory = tempfile::tempdir_in(root).unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let added = manual_job::add(
            &mut ws,
            &json!({"url":"https://example.test/job","title":"Engineer","employer":"Example"}),
            1,
        )
        .unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let posting = json!({"@type":"JobPosting","title":"Engineer","hiringOrganization":{"name":"Example"},"baseSalary":{"currency":"EUR","value":{"minValue":60000,"unitText":"YEAR"}},"datePosted":"2026-10-01","validThrough":"2026-11-30T23:59:00+01:00"}).to_string();
        let metadata = json!({"source_url":"https://example.test/older-job","captured_at":1750000000,"structured_source_status":"captured","structured_source_scripts":[posting]}).to_string();
        assert!(ws.save_job_source_only(&id, metadata.as_bytes()).unwrap());
        let later = json!({"source_url":"https://example.test/job","captured_at":1750000100,"structured_source_status":"unavailable"}).to_string();
        ws.save_job_capture(
            &id,
            later.as_bytes(),
            &[b"\x89PNG\r\n\x1a\nfixture".to_vec()],
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"id": id.as_str()}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/listing-salary-evidence HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["salary_evidence"]["currency"], "EUR");
        assert_eq!(result["salary_evidence"]["value"]["minValue"], 60000);
        assert_eq!(result["posting_date_evidence"], "2026-10-01");
        assert_eq!(
            result["valid_through_evidence"],
            "2026-11-30T23:59:00+01:00"
        );
        assert_eq!(result["captured_at"], 1750000000);
        assert_eq!(result["matches_current_url"], false);
    }
    #[test]
    #[ignore = "queries the existing local Ollama inventory without loading a model"]
    fn model_inventory_http_reads_local_ollama() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "GET /api/model-inventory HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\n\r\n").unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let mut ws = Workspace::in_memory().unwrap();
        serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(result["reported_count"].as_u64().is_some());
        assert!(result["models"].as_array().is_some());
    }
    #[test]
    fn model_inventory_is_bounded_and_reports_invalid_entries() {
        let mut models: Vec<Value> = (0..101)
            .map(|index| json!({"name":format!("model-{index}"),"size":1024}))
            .collect();
        models[1] = json!({"name":"invalid\nname","size":12});
        models[2] = json!({"name":"no-size"});
        let result = installed_model_inventory(&json!({"models":models})).unwrap();
        assert_eq!(result["reported_count"], 101);
        assert_eq!(result["examined"], 100);
        assert_eq!(result["invalid"], 1);
        assert_eq!(result["models"].as_array().unwrap().len(), 99);
        assert!(result["models"][1]["disk_bytes"].is_null());
        assert!(installed_model_inventory(&json!({})).is_err());
    }
    #[test]
    fn fit_fact_selection_prefers_listing_terms_and_keeps_a_bounded_set() {
        let mut facts: Vec<String> = (0..25)
            .map(|index| format!("Experience number {index}"))
            .collect();
        facts.push("Built Rust compilers for embedded devices".into());
        let selected = super::select_fit_facts(&facts, "Embedded Rust compiler engineer");
        assert_eq!(selected.len(), 20);
        assert_eq!(selected[0], "Built Rust compilers for embedded devices");
        assert_eq!(super::select_fit_facts(&facts, "unmatched")[0], facts[0]);
    }

    #[test]
    fn startup_error_message_is_bounded_and_removes_control_characters() {
        let result = startup_error_message(&format!("  invalid\nsetting {}", "x".repeat(2000)));
        assert!(result.contains("invalid setting"));
        assert!(result.chars().count() < 1100);
        assert!(!result.contains('\r'));
        assert_eq!(
            startup_error_message("\n\t"),
            "Waypoint could not start. Check the workspace and try again."
        );
    }

    #[test]
    fn assessment_digest_binds_facts_outside_semantic_pool() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(
            &mut ws,
            &json!({"url":"https://example.org/role","title":"Engineer","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let mut facts: Vec<_> = (0..120)
            .map(|i| format!("Fictional qualification {i}"))
            .collect();
        let (pool, _, before) = fit_assessment_digest(&ws, &id, &facts, "").unwrap();
        assert!(pool.len() <= 60);
        let omitted = facts.iter().position(|fact| !pool.contains(fact)).unwrap();
        facts[omitted].push_str(" corrected");
        let (_, _, after) = fit_assessment_digest(&ws, &id, &facts, "").unwrap();
        assert_ne!(before, after);
    }

    #[test]
    fn assessment_tracks_and_reports_inputs_beyond_model_limits() {
        let mut ws = Workspace::in_memory().unwrap();
        let description = format!("{} final requirement", "é".repeat(8100));
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/long-role","title":"Engineer","employer":"Fixture","description":description}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let prefix = "x".repeat(2000);
        ws.save_candidate_profile(&json!({"instructions":format!("{prefix} first")}).to_string())
            .unwrap();
        let (_, _, instructions, listing, before) = fit_assessment_inputs(&ws, &id).unwrap();
        let coverage = fit_source_coverage(&ws, &id, &listing, &instructions).unwrap();
        assert_eq!(coverage["listing_used_chars"], 8000);
        assert!(coverage["listing_total_chars"].as_u64().unwrap() > 8000);
        assert_eq!(coverage["instructions_used_chars"], 2000);
        assert!(coverage["instructions_total_chars"].as_u64().unwrap() > 2000);
        ws.save_candidate_profile(&json!({"instructions":format!("{prefix} changed")}).to_string())
            .unwrap();
        let (_, _, new_instructions, new_listing, after) = fit_assessment_inputs(&ws, &id).unwrap();
        assert_eq!(instructions, new_instructions);
        assert_eq!(listing, new_listing);
        assert_ne!(before, after);
    }

    #[test]
    fn fit_fact_selection_preserves_short_programming_languages() {
        for language in ["C++", "C#", "Go", "R"] {
            let mut facts: Vec<String> = (0..21)
                .map(|index| format!("Unrelated history {index}"))
                .collect();
            facts.push(format!("Experience with {language}"));
            let selected =
                super::select_fit_facts(&facts, &format!("Seeking {language} expertise"));
            assert_eq!(selected[0], format!("Experience with {language}"));
        }
        let mut facts: Vec<String> = (0..21)
            .map(|index| format!("Unrelated history {index}"))
            .collect();
        facts.push("Experience with C#".into());
        assert!(!super::select_fit_facts(&facts, "Seeking C++ expertise")
            .contains(&"Experience with C#".into()));
    }
    #[test]
    fn inspection_failure_reason_is_readable_and_bounded() {
        assert_eq!(
            super::inspection_failure_reason("  \n"),
            "Application inspection failed"
        );
        assert_eq!(
            super::inspection_failure_reason("Could not open\npage"),
            "Inspection failed: Could not open page"
        );
        assert_eq!(
            super::inspection_failure_reason(&"é".repeat(300))
                .chars()
                .count(),
            "Inspection failed: ".chars().count() + 240
        );
    }

    #[test]
    fn review_session_remaining_time_is_bounded_by_elapsed_time() {
        use std::time::Duration;
        let ttl = Duration::from_secs(120);
        assert_eq!(
            super::remaining_session_seconds(ttl, Duration::from_secs(35)),
            85
        );
        assert_eq!(
            super::remaining_session_seconds(ttl, Duration::from_secs(120)),
            0
        );
        assert_eq!(
            super::remaining_session_seconds(ttl, Duration::from_secs(130)),
            0
        );
    }

    use super::*;

    #[test]
    fn review_reason_endpoints_resolve_only_the_requested_reason() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/fixture","title":"Engineer","employer":"Fixture"}), 1).unwrap();
            let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
            ws.mark_manual_review(&id, "First\nissue", 1).unwrap();
            ws.mark_manual_review(&id, "Second issue", 2).unwrap();
            tx.send(id.as_str().to_owned()).unwrap();
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
            }
            let remaining = ws.manual_review_reasons(&id).unwrap();
            assert_eq!(remaining.len(), 1);
            assert_eq!(remaining[0].reason, "Second issue");
            assert_eq!(ws.manual_reviews().unwrap().len(), 1);
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
            }
            assert!(ws.manual_reviews().unwrap().is_empty());
        });
        let id = rx.recv().unwrap();
        let request = |path: &str, body: Value| -> Value {
            let body = body.to_string();
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        let before = request("/api/review/reasons", json!({"id":id}));
        assert_eq!(before["reasons"].as_array().unwrap().len(), 2);
        let revision = before["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["reason"] == "First\nissue")
            .unwrap()["revision"]
            .as_i64()
            .unwrap();
        let outcome = request(
            "/api/review/resolve",
            json!({"id":id,"reason":"First\nissue","revision":revision}),
        );
        assert_eq!(outcome["removed"], true);
        let after = request("/api/review/reasons", json!({"id":id}));
        assert_eq!(after["reasons"][0]["reason"], "Second issue");
        let stale_revision = after["reasons"][0]["revision"].as_i64().unwrap();
        assert_eq!(
            request("/api/review", json!({"id":id,"required":true}))["changed"],
            true
        );
        assert_eq!(
            request(
                "/api/review",
                json!({"id":id,"required":false,"revision":stale_revision})
            )["changed"],
            false
        );
        let current = request("/api/review/reasons", json!({"id":id}));
        let current_revision = current["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["revision"].as_i64().unwrap())
            .max()
            .unwrap();
        assert_eq!(
            request(
                "/api/review",
                json!({"id":id,"required":false,"revision":current_revision})
            )["changed"],
            true
        );
        worker.join().unwrap();
    }

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
            model_review_selectors: Default::default(),
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
                        editing_mode: None,
                        max_length_utf16: None,
                        autocomplete: None,
                    },
                    FormField {
                        selector: "#country".into(),
                        label: "Country".into(),
                        field_type: FormFieldType::Select {
                            options: vec!["Germany".into(), "France".into()],
                        },
                        required: true,
                        sensitive: false,
                        editing_mode: None,
                        max_length_utf16: None,
                        autocomplete: None,
                    },
                    FormField {
                        selector: "#visa".into(),
                        label: "Work authorization".into(),
                        field_type: FormFieldType::Text,
                        required: false,
                        sensitive: true,
                        editing_mode: None,
                        max_length_utf16: None,
                        autocomplete: None,
                    },
                    FormField {
                        selector: "#start".into(),
                        label: "Available date".into(),
                        field_type: FormFieldType::Date,
                        required: false,
                        sensitive: false,
                        editing_mode: None,
                        max_length_utf16: None,
                        autocomplete: None,
                    },
                    FormField {
                        selector: "#skills".into(),
                        label: "Skills".into(),
                        field_type: FormFieldType::MultiSelect {
                            options: vec!["Rust".into(), "Python".into()],
                        },
                        required: true,
                        sensitive: false,
                        editing_mode: None,
                        max_length_utf16: None,
                        autocomplete: None,
                    },
                ],
            },
            suggestions: vec![],
            manual_review: vec![],
        };
        assert_eq!(reviewed_form_answers(&json!([{"selector":"#question","value":"Research interests"},{"selector":"#country","value":"Germany"}]), &preview).unwrap().len(), 2);
        let mut model_flagged = preview.clone();
        model_flagged
            .model_review_selectors
            .insert("#question".into());
        assert!(!model_flagged.schema.fields[0].sensitive);
        assert!(model_flagged.fields_for_review()[0].sensitive);
        assert!(reviewed_form_answers(
            &json!([{"selector":"#question","value":"Research interests"}]),
            &model_flagged
        )
        .is_err());
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
        assert!(reviewed_form_answers(
            &json!([{"selector":"#skills","value":["Rust","Python"]}]),
            &preview
        )
        .is_ok());
        assert!(reviewed_form_answers(
            &json!([{"selector":"#skills","value":["Rust","rust"]}]),
            &preview
        )
        .is_err());
        assert!(reviewed_form_answers(
            &json!([{"selector":"#skills","value":["Rust","Unknown"]}]),
            &preview
        )
        .is_err());
    }

    #[test]
    fn reviewed_fill_supports_bounded_long_forms() {
        use waypoint_applications::form_mapping::{FormField, FormFieldType, FormSchema};
        let fields: Vec<_> = (0..201)
            .map(|index| FormField {
                selector: format!("#field-{index}"),
                label: format!("Field {index}"),
                field_type: FormFieldType::Text,
                required: false,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                autocomplete: None,
            })
            .collect();
        let preview = form_preview::FormPreview {
            model_review_selectors: Default::default(),
            schema: FormSchema {
                origin: "https://example.org".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![],
            manual_review: vec![],
        };
        let answers: Vec<_> = (0..200)
            .map(|index| json!({"selector":format!("#field-{index}"),"value":"Example"}))
            .collect();
        let (prepared, _) = prepare_reviewed_fill(&preview, &json!({"answers":answers})).unwrap();
        assert_eq!(prepared.suggestions.len(), 200);
        let too_many: Vec<_> = (0..201)
            .map(|index| json!({"selector":format!("#field-{index}"),"value":"Example"}))
            .collect();
        assert!(prepare_reviewed_fill(&preview, &json!({"answers":too_many})).is_err());
    }

    #[test]
    fn reviewed_fill_respects_native_utf16_limit_before_filling() {
        use waypoint_applications::form_mapping::{FormField, FormFieldType, FormSchema};
        let preview = form_preview::FormPreview {
            schema: FormSchema {
                origin: "https://example.org".into(),
                fields: vec![FormField {
                    selector: "#answer".into(),
                    label: "Answer".into(),
                    field_type: FormFieldType::Text,
                    required: false,
                    sensitive: false,
                    editing_mode: None,
                    max_length_utf16: Some(3),
                    autocomplete: None,
                }],
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        assert!(prepare_reviewed_fill(
            &preview,
            &json!({"answers":[{"selector":"#answer","value":"A😀"}]})
        )
        .is_ok());
        let error = prepare_reviewed_fill(
            &preview,
            &json!({"answers":[{"selector":"#answer","value":"AB😀"}]}),
        )
        .err()
        .unwrap();
        assert!(error.contains("3 UTF-16-unit limit"));
    }
    #[test]
    fn queued_template_suggestion_requires_its_original_context() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        ws.add_fact(
            "Fictional software experience",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        ws.add_document("template", "fixture.docx", b"fictional document", 1)
            .unwrap();
        let digest = template_suggestion_inputs(&ws, "template", &job, "Original passage")
            .unwrap()
            .digest;
        let edits = vec![
            json!({"before":"Original passage","after":"Revised passage","suggestion_digest":digest,"suggestion_job_id":job.as_str()}),
        ];
        assert!(
            validate_template_edit_bindings(&ws, "template", Some(job.as_str()), &edits).is_ok()
        );
        assert!(validate_template_edit_bindings(&ws, "template", None, &edits).is_err());
        assert!(
            validate_template_edit_bindings(&ws, "template", Some("other-job"), &edits).is_err()
        );
        ws.save_candidate_profile(&json!({"document_instructions":"Different tone"}).to_string())
            .unwrap();
        assert!(
            validate_template_edit_bindings(&ws, "template", Some(job.as_str()), &edits).is_err()
        );
        assert!(validate_template_edit_bindings(
            &ws,
            "template",
            None,
            &[json!({"before":"Original", "after":"Manual"})]
        )
        .is_ok());
    }
    #[test]
    fn draft_priorities_require_current_reviewed_claims() {
        let mut ws = Workspace::in_memory().unwrap();
        let reviewed = ws
            .add_fact("Fictional reviewed skill", FactOrigin::UserAttested)
            .unwrap();
        let unreviewed = ws
            .add_fact("Unreviewed claim", FactOrigin::ModelInferred)
            .unwrap();
        assert_eq!(
            draft_priority_facts(&ws, &json!([reviewed])).unwrap(),
            vec!["Fictional reviewed skill"]
        );
        assert!(draft_priority_facts(&ws, &json!([unreviewed])).is_err());
        assert!(draft_priority_facts(&ws, &json!([reviewed, reviewed])).is_err());
        assert!(draft_priority_facts(&ws, &json!(["a", "b", "c", "d"])).is_err());
        ws.correct_fact(&reviewed, "Corrected fictional skill", 1)
            .unwrap();
        assert!(draft_priority_facts(&ws, &json!([reviewed])).is_err());
    }

    #[test]
    fn draft_qualifications_prioritize_reviewed_relevant_facts() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Quantum researcher","employer":"Fixture","description":"Quantum optics research"}), 1).unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        for index in 0..25 {
            ws.add_fact(
                &format!("Unrelated experience {index}"),
                waypoint_workspace::FactOrigin::UserAttested,
            )
            .unwrap();
        }
        ws.add_fact(
            "Quantum optics laboratory research",
            waypoint_workspace::FactOrigin::UserAttested,
        )
        .unwrap();
        ws.add_fact(
            "Quantum optics unreviewed claim",
            waypoint_workspace::FactOrigin::ModelInferred,
        )
        .unwrap();
        for kind in ["cv", "cover_letter"] {
            let facts = draft_qualifications(&ws, &job, kind).unwrap();
            assert!(facts.contains(&"Quantum optics laboratory research".to_owned()));
            assert!(!facts.iter().any(|fact| fact.contains("unreviewed claim")));
            assert!(facts.len() <= if kind == "cv" { 18 } else { 20 });
        }
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

    #[test]
    fn later_relevant_fact_changes_assessment_inputs() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Rust Engineer","employer":"Example","description":"Embedded Rust systems"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        for index in 0..25 {
            ws.add_fact(
                &format!("Fictional management experience {index}"),
                FactOrigin::UserAttested,
            )
            .unwrap();
        }
        let (_, _, _, before) = fit_inputs(&ws, &id).unwrap();
        ws.add_fact("Built embedded Rust systems", FactOrigin::UserAttested)
            .unwrap();
        let (selected, _, _, after) = fit_inputs(&ws, &id).unwrap();
        assert_ne!(before, after);
        assert!(selected
            .iter()
            .any(|fact| fact == "Built embedded Rust systems"));
    }

    #[cfg(all(windows, feature = "sqlcipher"))]
    #[test]
    fn bounded_reviewed_facts_match_the_sealed_full_read() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        let mut ws = Workspace::open_encrypted(dir.path()).unwrap();
        let original = ws
            .add_fact("Fictional original", FactOrigin::UserAttested)
            .unwrap();
        ws.correct_fact(&original, "Fictional correction", 1)
            .unwrap();
        ws.add_fact("Unreviewed model text", FactOrigin::ModelInferred)
            .unwrap();
        ws.add_fact(&"x".repeat(1001), FactOrigin::UserAttested)
            .unwrap();
        ws.add_fact("Fictional reviewed skill", FactOrigin::UserAttested)
            .unwrap();
        for index in 0..25 {
            ws.add_fact(
                &format!("Fictional skill {index}"),
                FactOrigin::UserAttested,
            )
            .unwrap();
        }
        let expected: Vec<_> = ws
            .facts()
            .unwrap()
            .into_iter()
            .filter(|fact| !fact.requires_review && fact.text.len() <= 1000)
            .take(500)
            .map(|fact| fact.text)
            .collect();
        let actual: Vec<_> = ws
            .reviewed_facts_bounded(500, 1000)
            .unwrap()
            .into_iter()
            .map(|fact| fact.text)
            .collect();
        assert_eq!(actual, expected);
        assert!(actual.contains(&"Fictional correction".to_string()));
        assert!(!actual.contains(&"Fictional original".to_string()));
        assert!(!actual.contains(&"Unreviewed model text".to_string()));
        let planning: Vec<_> = ws
            .reviewed_facts_bounded(20, 1000)
            .unwrap()
            .into_iter()
            .map(|fact| fact.text)
            .collect();
        assert_eq!(planning.len(), 20);
        assert_eq!(planning, expected[..20]);
        assert!(ws.reviewed_facts_bounded(0, 1000).unwrap().is_empty());
        let before = ws.active_facts_digest().unwrap();
        assert_eq!(before, ws.active_facts_digest().unwrap());
        ws.add_fact("New unreviewed fact", FactOrigin::ModelInferred)
            .unwrap();
        let unreviewed = ws.active_facts_digest().unwrap();
        assert_ne!(before, unreviewed);
        let new_fact = ws
            .add_fact("New reviewed fact", FactOrigin::UserAttested)
            .unwrap();
        let added = ws.active_facts_digest().unwrap();
        assert_ne!(unreviewed, added);
        ws.correct_fact(&new_fact, "Corrected reviewed fact", 2)
            .unwrap();
        assert_ne!(added, ws.active_facts_digest().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn saved_fit_is_not_reported_current_after_preferences_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let (_, _, _, _, digest) = fit_assessment_inputs(&ws, &id).unwrap();
        let saved = json!({"input_digest":digest,"assessment":{"score":73},"model":"fixture"});
        ws.save_fit_assessment(&id, saved.to_string().as_bytes())
            .unwrap();
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "current");
        ws.save_candidate_profile(&json!({"instructions":"Remote only"}).to_string())
            .unwrap();
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "stale");
    }

    #[cfg(windows)]
    #[test]
    fn batch_fit_context_preserves_per_job_results_and_freshness() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let mut ids = Vec::new();
        for (title, url) in [
            ("Engineer", "https://example.org/one"),
            ("Analyst", "https://example.org/two"),
        ] {
            let added = manual_job::add(&mut ws, &json!({"url":url,"title":title,"employer":"Fixture","description":"Fictional role"}), 1).unwrap();
            let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
            let (_, _, _, _, digest) = fit_assessment_inputs(&ws, &id).unwrap();
            ws.save_fit_assessment(
                &id,
                json!({"input_digest":digest,"assessment":{"score":73},"model":"fixture"})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
            ids.push(id);
        }
        let mut context = None;
        for id in &ids {
            assert_eq!(
                read_fit_with_context(&ws, id, &mut context).unwrap(),
                read_fit(&ws, id).unwrap()
            );
        }
        ws.save_candidate_profile(&json!({"instructions":"Remote only"}).to_string())
            .unwrap();
        let mut next_request_context = None;
        for id in &ids {
            assert_eq!(
                read_fit_with_context(&ws, id, &mut next_request_context).unwrap()["status"],
                "stale"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn gmail_packet_api_saves_exact_unsent_message_and_materials() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::open(&root).unwrap();
            let id = JobIdentityId::new("fictional-email").unwrap();
            ws.import_job(waypoint_store::StoredJob {
                id: id.clone(),
                title: "Role".into(),
                employer: "Fixture".into(),
                location: "".into(),
                state: ApplicationState::Shortlisted,
                canonical_url: None,
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "fixture".into(),
                requisition_id: None,
                posted_at: None,
            })
            .unwrap();
            ws.add_document("fixture-pdf", "resume.pdf", b"%PDF-fictional", 1)
                .unwrap();
            ws.add_document(
                "fixture-template",
                "template.docx",
                b"fictional template",
                1,
            )
            .unwrap();
            ws.save_candidate_profile(&json!({"cv_template_id":"fixture-template"}).to_string())
                .unwrap();
            ws.save_gmail_credentials(&serde_json::to_vec(&json!({"auth_type":"app_password","email":"fictional@gmail.com","password":"abcdefghijklmnop"})).unwrap()).unwrap();
            ws.index_next_duplicate_job("").unwrap();
            let context = draft_context_digest(&ws, &id, "cv").unwrap();
            letter_export::record_generated_fixture(
                &ws,
                &id,
                "fixture-pdf",
                b"%PDF-fictional",
                &context,
            )
            .unwrap();
            let review_id = "0123456789abcdef0123456789abcdef";
            let mut review = attempt_record::AttemptRecord::start(&ws, &id, review_id, 1)
                .unwrap()
                .unwrap();
            let material = review.material("resume.pdf", b"%PDF-fictional").unwrap();
            review.document_visual_report(&json!({"status":"complete","material":material,"document_sha256":waypoint_domain::sha256_hex(b"%PDF-fictional"),"context_sha256":Sha256Digest::of(&document_visual_context(&ws,&id).unwrap()).as_str(),"report":{"complete":true,"findings":[]}})).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap_err();
            assert!(error.contains("Review visual findings"));
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            let (mut stream, _) = listener.accept().unwrap();
            if let Err(error) = serve(&mut stream, &mut ws, &mut None, "test-token", port) {
                json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            }
        });
        let mut request = json!({"id":"fictional-email","recipient":"hiring@example.invalid","subject":"Role","message":"Hello","documents":["fixture-pdf"],"visual_reviews":[{"document_id":"fixture-pdf","attempt_id":"0123456789abcdef0123456789abcdef","reviewed":false}]});
        let body = request.to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream,"POST /api/gmail/prepare-application HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let mut rejected = String::new();
        stream.read_to_string(&mut rejected).unwrap();
        assert!(rejected.contains("Review visual findings"));
        request["visual_reviews"][0]["reviewed"] = json!(true);
        let body = request.to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream,"POST /api/gmail/prepare-application HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        worker.join().unwrap();
        let split = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200"));
        assert!(String::from_utf8_lossy(&response[..split])
            .contains("X-Waypoint-Document-Evidence: linked"));
        let mime = &response[split + 4..];
        assert!(mime.starts_with(b"From: fictional@gmail.com\r\n"));
        let records = directory.path().join("application-records");
        let attempt = std::fs::read_dir(records)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.join("email-draft.eml").exists())
            .unwrap();
        assert_eq!(
            std::fs::read(attempt.join("email-draft.eml")).unwrap(),
            mime
        );
        let gmail_review: Value =
            serde_json::from_slice(&std::fs::read(attempt.join("gmail-review.json")).unwrap())
                .unwrap();
        assert_eq!(
            std::fs::read(
                attempt.join("materials").join(
                    gmail_review["attachments"][0]["stored_name"]
                        .as_str()
                        .unwrap()
                )
            )
            .unwrap(),
            b"%PDF-fictional"
        );
        assert!(attempt.join("original-job.json").exists());
        let events = std::fs::read_to_string(attempt.join("events.jsonl")).unwrap();
        assert!(events.contains(&waypoint_domain::sha256_hex(mime)));
        assert!(events.contains("\"sent\":false"));
        assert!(events.contains("document_visual_review_linked"));
        let draft_link = events
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|event| event["type"] == "document_draft_evidence_linked")
            .unwrap();
        assert_eq!(draft_link["document_id"], "fixture-pdf");
        assert_eq!(draft_link["context_current"], true);
        let evidence: Value = serde_json::from_slice(
            &std::fs::read(
                attempt
                    .join("materials")
                    .join(draft_link["evidence"].as_str().unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(evidence["draft"]["facts"][0], "Fictional qualification");
        assert_eq!(
            std::fs::read(
                attempt
                    .join("materials")
                    .join(draft_link["template"].as_str().unwrap())
            )
            .unwrap(),
            b"fictional template"
        );
        assert!(attempt.join("document-visual-0001.json").exists());
        let meta: Value =
            serde_json::from_slice(&std::fs::read(attempt.join("attempt.json")).unwrap()).unwrap();
        let attempt_id = meta["attempt_id"].as_str().unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let job = JobIdentityId::new("fictional-email").unwrap();
        let review = attempt_record::gmail_review(&ws, &job, attempt_id).unwrap();
        assert_eq!(review["message"], "Hello");
        assert_eq!(review["attachments"][0]["name"], "resume.pdf");
        assert_eq!(review["attachments"][0]["visual_review_status"], "current");
        let request = json!({"id":"fictional-email","attempt_id":attempt_id,"mime_sha256":review["mime_sha256"],"context_sha256":review["context_sha256"],"reviewed":true});
        let mut without_gesture = request.clone();
        without_gesture["reviewed"] = json!(false);
        assert!(approve_saved_gmail(&mut ws, &without_gesture).is_err());
        ws.save_candidate_profile("{\"city\":\"Changed\"}").unwrap();
        assert!(approve_saved_gmail(&mut ws, &request).is_err());
        ws.add_fact("Fictional new qualification", FactOrigin::UserAttested)
            .unwrap();
        let fresh = attempt_record::gmail_review(&ws, &job, attempt_id).unwrap();
        assert_eq!(fresh["attachments"][0]["visual_review_status"], "stale");
        let fresh_request = json!({"id":"fictional-email","attempt_id":attempt_id,"mime_sha256":fresh["mime_sha256"],"context_sha256":fresh["context_sha256"],"reviewed":true});
        let approval = approve_saved_gmail(&mut ws, &fresh_request).unwrap();
        assert_eq!(approval["sent"], false);
        let rehearsal_request = json!({"id":"fictional-email","attempt_id":attempt_id,"approval_id":approval["approval_id"]});
        for _ in 0..2 {
            let rehearsal = rehearse_saved_gmail(&mut ws, &rehearsal_request).unwrap();
            assert_eq!(rehearsal["simulated"], true);
            assert_eq!(rehearsal["sent"], false);
            assert_eq!(rehearsal["approval_consumed"], false);
            assert_eq!(rehearsal["record_saved"], true);
        }
        assert!(ws.email_handoff_history(&job).unwrap().0.is_empty());
        attempt_record::record_email_outcome(
            &ws,
            &job,
            attempt_id,
            waypoint_store::email_handoff::EmailHandoffOutcome::Uncertain,
            2,
        )
        .unwrap();
        let outcome: Value =
            serde_json::from_slice(&std::fs::read(attempt.join("email-handoff.json")).unwrap())
                .unwrap();
        assert_eq!(outcome["state"], "uncertain");
        assert!(attempt_record::record_email_outcome(
            &ws,
            &job,
            attempt_id,
            waypoint_store::email_handoff::EmailHandoffOutcome::Accepted,
            3
        )
        .is_err());
        attempt_record::record_email_reconciliation(
            &ws,
            &job,
            attempt_id,
            "user_reports_not_sent",
            4,
        )
        .unwrap();
        let reconciliation: Value = serde_json::from_slice(
            &std::fs::read(attempt.join("email-reconciliation.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(reconciliation["decision"], "user_reports_not_sent");
        assert!(attempt_record::record_email_reconciliation(
            &ws,
            &job,
            attempt_id,
            "user_reports_sent",
            5
        )
        .is_err());
        std::fs::write(
            attempt.join("materials").join(
                gmail_review["attachments"][0]["stored_name"]
                    .as_str()
                    .unwrap(),
            ),
            b"%PDF-changed",
        )
        .unwrap();
        assert!(attempt_record::gmail_review(&ws, &job, attempt_id).is_err());
    }

    #[test]
    fn email_draft_reports_historical_generated_evidence_without_sending() {
        let directory = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let job = JobIdentityId::new("fictional-historical-email").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: job.clone(),
            title: "Role".into(),
            employer: "Example".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: None,
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "fixture".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        ws.add_document("pdf", "resume.pdf", b"%PDF-fictional", 1)
            .unwrap();
        ws.index_next_duplicate_job("").unwrap();
        let context = draft_context_digest(&ws, &job, "cv").unwrap();
        letter_export::record_generated_fixture(&ws, &job, "pdf", b"%PDF-fictional", &context)
            .unwrap();
        ws.add_fact("New fictional qualification", FactOrigin::UserAttested)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body=json!({"id":job.as_str(),"recipient":"hiring@example.invalid","subject":"Role","message":"Hello","documents":["pdf"]}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/email-draft HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut None, "test-token", port).unwrap();
        drop(server);
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        let end = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        assert!(String::from_utf8_lossy(&response[..end])
            .contains("X-Waypoint-Document-Evidence: historical"));
        let attempt = std::fs::read_dir(directory.path().join("application-records"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.join("email-draft.eml").exists())
            .unwrap();
        let events = std::fs::read_to_string(attempt.join("events.jsonl")).unwrap();
        let link: Value = events
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|event| event["type"] == "document_draft_evidence_linked")
            .unwrap();
        assert_eq!(link["context_current"], false);
        assert!(events.contains("document_draft_evidence_review_needed"));
        assert!(events.contains("\"sent\":false"));
        assert!(ws
            .manual_reviews()
            .unwrap()
            .iter()
            .any(|review| review.reason.contains("older context")));
        let source = std::fs::read_dir(directory.path().join("application-records"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.join("document-export.json").exists())
            .unwrap();
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(source.join("document-export.json")).unwrap())
                .unwrap();
        std::fs::write(
            source
                .join("materials")
                .join(receipt["evidence"].as_str().unwrap()),
            b"tampered",
        )
        .unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client,"POST /api/email-draft HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut None, "test-token", port).unwrap();
        drop(server);
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        let end = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        assert!(String::from_utf8_lossy(&response[..end])
            .contains("X-Waypoint-Document-Evidence: incomplete"));
        assert!(ws
            .manual_reviews()
            .unwrap()
            .iter()
            .any(|review| review.reason.contains("lookup was incomplete")));
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
        for (data, expected) in [
            (json!({"models":[]}), "no_model"),
            (json!({"models":[{},{}]}), "multiple_models"),
            (
                json!({"models":[{"name":"m","size":10,"size_vram":9}]}),
                "not_fully_gpu",
            ),
            (
                json!({"models":[{"name":"m","size":15_032_385_537u64,"size_vram":15_032_385_537u64}]}),
                "over_budget",
            ),
            (json!({"models":[{"name":"m"}]}), "incomplete_report"),
        ] {
            let status = model_diagnostics(&data).unwrap();
            assert_eq!(status["state"], expected);
            assert!(status["model"].is_null());
        }
        let status = model_diagnostics(&good).unwrap();
        assert_eq!(status["state"], "ready");
        assert_eq!(status["reported_vram_bytes"], 1_000_000);
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
    fn bundled_license_notices_are_available_to_the_local_workspace() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "GET /api/license-notices HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        worker.join().unwrap();
        let body = response.split_once("\r\n\r\n").unwrap().1;
        let value: Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            value["sqlcipher"],
            include_str!("../../../docs/SQLCIPHER_LICENSE.txt")
        );
        assert_eq!(
            value["inventory"],
            include_str!("../../../docs/dependency-license-inventory.txt")
        );
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

    #[cfg(windows)]
    #[test]
    #[ignore = "requires explicitly supplied test Gmail credentials; authenticates without sending"]
    fn live_gmail_password_api_connects_and_disconnects() {
        let email = zeroize::Zeroizing::new(
            std::env::var("WAYPOINT_TEST_GMAIL_USER").expect("test account is required"),
        );
        let password = zeroize::Zeroizing::new(
            std::env::var("WAYPOINT_TEST_GMAIL_PASSWORD").expect("test password is required"),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let dir = tempfile::tempdir().unwrap();
            let mut ws = Workspace::open(dir.path()).unwrap();
            let mut saw_credentials = false;
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                if let Err(error) = serve(&mut stream, &mut ws, &mut None, "test-token", port) {
                    json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                }
                let has_credentials = ws
                    .gmail_credentials()
                    .unwrap()
                    .map(zeroize::Zeroizing::new)
                    .is_some();
                if saw_credentials && !has_credentials {
                    break;
                }
                saw_credentials |= has_credentials;
            }
        });
        let post = |path: &str, body: &str| -> Value {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        let body = zeroize::Zeroizing::new(
            serde_json::to_string(&json!({"email":email.as_str(),"password":password.as_str()}))
                .unwrap(),
        );
        let started = post("/api/gmail/password-start", &body);
        let id = started["task_id"]
            .as_str()
            .expect("connection task should start");
        let deadline = Instant::now() + Duration::from_secs(55);
        loop {
            let status = post(
                "/api/gmail/password-status",
                &json!({"task_id":id}).to_string(),
            );
            match status["status"].as_str() {
                Some("complete") => break,
                Some("running") if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(250))
                }
                _ => panic!(
                    "Gmail API connection failed: {}",
                    status["error"].as_str().unwrap_or("timeout")
                ),
            }
        }
        let state = post("/api/gmail/status", "{}");
        assert_eq!(state["connected"], true);
        assert_eq!(state["sending_available"], false);
        let inbox = post("/api/gmail/inbox-start", "{}");
        let inbox_id = inbox["task_id"].as_str().expect("inbox task should start");
        let deadline = Instant::now() + Duration::from_secs(55);
        loop {
            let status = post(
                "/api/gmail/inbox-status",
                &json!({"task_id":inbox_id}).to_string(),
            );
            match status["status"].as_str() {
                Some("complete") => {
                    assert!(status["messages"].as_u64().is_some());
                    assert!(status["unseen"].as_u64().is_some());
                    break;
                }
                Some("running") if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(250))
                }
                _ => panic!(
                    "Gmail inbox check failed: {}",
                    status["error"].as_str().unwrap_or("timeout")
                ),
            }
        }
        assert_eq!(post("/api/gmail/disconnect", "{}")["ok"], true);
        worker.join().unwrap();
    }

    #[test]
    fn profile_rejects_unknown_fields_and_wrong_types() {
        assert!(request(
            "/api/profile",
            "test-token",
            r#"{"full_name":"Example, Avery","first_name":"Avery","instructions":"Remote work"}"#
        )
        .contains("200 OK"));
        assert!(request("/api/profile", "test-token", r#"{"unknown":"x"}"#)
            .contains("unknown profile field"));
        assert!(request("/api/profile", "test-token", r#"{"email":42}"#)
            .contains("profile fields must be text"));
        assert!(request("/api/profile", "test-token", r#"{"full_name":42}"#)
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
    fn draft_context_changes_when_template_or_preferences_change() {
        let mut ws = Workspace::in_memory().unwrap();
        let id = JobIdentityId::new("draft-context").unwrap();
        ws.import_manual_job(
            waypoint_store::StoredJob {
                id: id.clone(),
                title: "Engineer".into(),
                employer: "Example".into(),
                location: String::new(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some("https://example.org/job".into()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional role"),
        )
        .unwrap();
        ws.add_document("template-a", "a.docx", b"first template", 1)
            .unwrap();
        ws.add_document("template-b", "b.docx", b"second template", 1)
            .unwrap();
        ws.save_candidate_profile(
            &json!({"cv_template_id":"template-a", "document_instructions":"Concise"}).to_string(),
        )
        .unwrap();
        let first = draft_context_digest(&ws, &id, "cv").unwrap();
        ws.save_candidate_profile(
            &json!({"cv_template_id":"template-b", "document_instructions":"Concise"}).to_string(),
        )
        .unwrap();
        let second = draft_context_digest(&ws, &id, "cv").unwrap();
        assert_ne!(first, second);
        ws.save_candidate_profile(
            &json!({"cv_template_id":"template-b", "document_instructions":"Detailed"}).to_string(),
        )
        .unwrap();
        assert_ne!(second, draft_context_digest(&ws, &id, "cv").unwrap());
    }

    #[test]
    fn email_suggestion_context_tracks_reviewed_facts_and_writing_preferences() {
        let mut ws = Workspace::in_memory().unwrap();
        let id = JobIdentityId::new("email-context").unwrap();
        ws.import_manual_job(
            waypoint_store::StoredJob {
                id: id.clone(),
                title: "Engineer".into(),
                employer: "Example".into(),
                location: String::new(),
                state: ApplicationState::Shortlisted,
                canonical_url: Some("https://example.org/job".into()),
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: false,
                source: "user_supplied_url".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional role"),
        )
        .unwrap();
        assert!(email_answer_inputs(&ws, &id).is_err());
        ws.add_fact("Experience with research", FactOrigin::UserAttested)
            .unwrap();
        let first = email_answer_inputs(&ws, &id).unwrap();
        assert!(first.question.contains("Engineer"));
        ws.save_candidate_profile(&json!({"document_instructions":"Concise"}).to_string())
            .unwrap();
        let second = email_answer_inputs(&ws, &id).unwrap();
        assert_ne!(first.digest, second.digest);
        assert_eq!(second.selector, "email_body");
        ws.add_fact(
            "Additional unrelated reviewed qualification",
            FactOrigin::UserAttested,
        )
        .unwrap();
        let third = email_answer_inputs(&ws, &id).unwrap();
        assert_ne!(second.digest, third.digest);
        assert_eq!(third.available_facts, 2);
    }

    #[test]
    fn answer_pool_prioritizes_question_terms_and_keeps_bounded_sampling() {
        let mut facts: Vec<String> = (0..60)
            .map(|i| format!("Maintained unrelated project schedule {i}"))
            .collect();
        facts[59] = "Moderated focus groups with customers".into();
        let question = "Describe your experience moderating focus groups";
        let listing = "Research engineer role";
        assert!(!select_fit_facts(&facts, listing).contains(&facts[59]));
        let pool = fit_semantic_pool(&facts, &format!("{question}\n{listing}"));
        assert!(pool[..20].contains(&facts[59]));
        assert!(pool.len() <= 60);
    }

    #[test]
    fn state_does_not_score_jobs_without_reviewed_facts_or_a_description() {
        use waypoint_store::StoredJob;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            for (id, description) in [
                ("with-description", Some("Python research")),
                ("no-description", None),
            ] {
                ws.import_manual_job(
                    StoredJob {
                        id: JobIdentityId::new(id).unwrap(),
                        title: "Engineer".into(),
                        employer: "Example".into(),
                        location: "Remote".into(),
                        state: ApplicationState::Discovered,
                        canonical_url: Some(format!("https://example.org/{id}")),
                        content_hash: None,
                        first_seen: None,
                        last_seen: None,
                        remote: true,
                        source: "user_supplied_url".into(),
                        requisition_id: None,
                        posted_at: None,
                    },
                    description,
                )
                .unwrap();
            }
            for index in 0..2 {
                if index == 1 {
                    ws.add_fact("Python", waypoint_workspace::FactOrigin::UserAttested)
                        .unwrap();
                }
                let (mut stream, _) = listener.accept().unwrap();
                serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
            }
        });
        let request = || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\n\r\n").unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        let first = request();
        assert!(first["jobs"].as_array().unwrap().iter().all(
            |row| row["score"].is_null() && row["match_basis"] == "no_reviewed_qualifications"
        ));
        let second = request();
        let rows = second["jobs"].as_array().unwrap();
        let described = rows
            .iter()
            .find(|row| row["job"]["id"] == "with-description")
            .unwrap();
        let missing = rows
            .iter()
            .find(|row| row["job"]["id"] == "no-description")
            .unwrap();
        assert_eq!(described["score"], 1.0);
        assert_eq!(missing["match_basis"], "no_description");
        assert!(missing["score"].is_null());
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
    fn state_shows_the_filename_that_would_be_attached() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            let name = format!("{}.pdf", "x".repeat(251));
            ws.add_document("long-name", &name, b"%PDF-fictional", 1)
                .unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let state: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        let doc = &state["documents"][0];
        let original = format!("{}.pdf", "x".repeat(251));
        assert_eq!(doc["name"], original);
        assert_eq!(
            doc["attachment_name"],
            attempt_record::attachment_filename(&original)
        );
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
            let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
            let workspace_dir = tempfile::tempdir_in(base).unwrap();
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
            let profile = json!({"first_name":"Synthetic","country":"Germany","cv_template_id":"fixture-template"});
            ws.save_candidate_profile(&profile.to_string()).unwrap();
            ws.add_fact("Fictional research experience", FactOrigin::UserAttested)
                .unwrap();
            ws.add_document(
                "fixture-template",
                "fictional-template.docx",
                b"fictional template",
                1,
            )
            .unwrap();
            let doc_name = format!("{}.pdf", "x".repeat(251));
            let staged_name = attempt_record::attachment_filename(&doc_name);
            ws.add_document("fictional-doc", &doc_name, b"%PDF-fictional resume", 1)
                .unwrap();
            let draft_context = draft_context_digest(&ws, &id, "cv").unwrap();
            letter_export::record_generated_fixture(
                &ws,
                &id,
                "fictional-doc",
                b"%PDF-fictional resume",
                &draft_context,
            )
            .unwrap();
            let review_attempt = "0123456789abcdef0123456789abcdef";
            let pdf = b"%PDF-fictional resume";
            let mut review_record =
                attempt_record::AttemptRecord::start(&ws, &id, review_attempt, 1)
                    .unwrap()
                    .unwrap();
            let material = review_record.material("fictional-cv.pdf", pdf).unwrap();
            review_record.document_visual_report(&json!({"status":"complete","material":material,"document_sha256":waypoint_domain::sha256_hex(pdf),"context_sha256":Sha256Digest::of(&document_visual_context(&ws,&id).unwrap()).as_str(),"report":{"complete":true,"findings":[]}})).unwrap();
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
                model_review_selectors: Default::default(),
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
                manual_review: vec![
                    "Required field needs review: Given name".into(),
                    "A separate issue needs review".into(),
                ],
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
                completed_selectors: vec![],
                answer_drafts: Default::default(),
                visible: false,
                staged_files: vec![],
                attachments: vec![],
                last_visual_freshness_key: None,
                record: None,
            });
            let input = answer_inputs(&ws, session.as_ref().unwrap(), "#candidate").unwrap();
            answer_task::set_completed_fixture(
                "fixture-draft",
                input,
                json!({"text":"Revised","sources":["Fictional research experience"],"evidence_check":{"status":"completed","claims":[]}}),
            );
            let draft_listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let draft_port = draft_listener.local_addr().unwrap().port();
            let draft_body = json!({"task_id":"fixture-draft"}).to_string();
            let mut draft_client = TcpStream::connect(("127.0.0.1", draft_port)).unwrap();
            write!(draft_client, "POST /api/form/answer-status HTTP/1.1\r\nHost: 127.0.0.1:{draft_port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{draft_body}", draft_body.len()).unwrap();
            let (mut draft_server, _) = draft_listener.accept().unwrap();
            serve(
                &mut draft_server,
                &mut ws,
                &mut session,
                "test-token",
                draft_port,
            )
            .unwrap();
            drop(draft_server);
            let mut draft_response = String::new();
            draft_client.read_to_string(&mut draft_response).unwrap();
            let draft_response: Value =
                serde_json::from_str(draft_response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(draft_response["provenance_available"], true);
            let current = session.as_mut().unwrap();
            let request = json!({"selectors":[],"answers":[{"selector":"#candidate","value":"Revised","draft_id":"fixture-draft"}]});
            let (prepared, _) = prepare_reviewed_fill(&current.preview, &request).unwrap();
            let mut fields = form_preview::requested_field_records(&prepared).unwrap();
            attach_answer_provenance(&ws, current, &request, &mut fields).unwrap();
            assert_eq!(
                fields[0]["source_draft"]["sources"][0],
                "Fictional research experience"
            );
            let edited = json!({"selectors":[],"answers":[{"selector":"#candidate","value":"Edited","draft_id":"fixture-draft"}]});
            let (prepared, _) = prepare_reviewed_fill(&current.preview, &edited).unwrap();
            let mut fields = form_preview::requested_field_records(&prepared).unwrap();
            assert!(attach_answer_provenance(&ws, current, &edited, &mut fields).is_err());
            let digest = current.answer_drafts["fixture-draft"].digest.clone();
            current
                .answer_drafts
                .get_mut("fixture-draft")
                .unwrap()
                .digest = "stale".into();
            let (prepared, _) = prepare_reviewed_fill(&current.preview, &request).unwrap();
            let mut fields = form_preview::requested_field_records(&prepared).unwrap();
            assert!(attach_answer_provenance(&ws, current, &request, &mut fields).is_err());
            current
                .answer_drafts
                .get_mut("fixture-draft")
                .unwrap()
                .digest = digest;
            let wrong_field = json!({"selectors":[],"answers":[{"selector":"#later-answer","value":"Revised","draft_id":"fixture-draft"}]});
            let (prepared, _) = prepare_reviewed_fill(&current.preview, &wrong_field).unwrap();
            let mut fields = form_preview::requested_field_records(&prepared).unwrap();
            assert!(attach_answer_provenance(&ws, current, &wrong_field, &mut fields).is_err());
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(error.contains("Close the active review session"));
            assert_eq!(session.as_ref().unwrap().id, "test-session");
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            ws.save_candidate_profile(&json!({"first_name":"Updated"}).to_string())
                .unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(error.contains("Profile changed"), "{error}");
            assert_eq!(session.as_ref().unwrap().id, "test-session");
            assert!(!session.as_ref().unwrap().filled);
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            ws.save_candidate_profile(&profile.to_string()).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(error.contains("invalid value"), "{error}");
            let current = session.as_ref().unwrap();
            assert!(!current.filled);
            assert_eq!(current.preview.suggestions.len(), 3);
            assert!(current
                .preview
                .suggestions
                .iter()
                .any(|mapping| mapping.field_selector == "#candidate"));
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            // Replace only the fixture's records directory with a file so record
            // creation fails without changing host permissions.
            let records = workspace_dir.path().join("application-records");
            let records_backup = workspace_dir.path().join("fixture-records-backup");
            std::fs::rename(&records, &records_backup).unwrap();
            std::fs::write(&records, b"fixture blocker").unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(
                error.contains("attempt record could not be created"),
                "{error}"
            );
            assert!(ws
                .manual_reviews()
                .unwrap()
                .iter()
                .any(|review| review.reason.contains("record could not be created")));
            let current = session.as_mut().unwrap();
            assert!(!current.filled);
            assert!(current
                .preview
                .suggestions
                .iter()
                .any(|mapping| mapping.field_selector == "#candidate"
                    && mapping.value == PacketValue::Text("Synthetic".into())));
            assert_eq!(
                current
                    .driver
                    .read_attribute("#candidate", "data-seen")
                    .unwrap(),
                None
            );
            std::fs::remove_file(&records).unwrap();
            std::fs::rename(&records_backup, &records).unwrap();
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            // Make the event path a directory to simulate a record-write failure
            // inside this disposable workspace, without changing host permissions.
            let broken_id = "fedcba9876543210fedcba9876543210";
            let broken_record = attempt_record::AttemptRecord::start(&ws, &id, broken_id, 1)
                .unwrap()
                .unwrap();
            let hash = Sha256Digest::of(id.as_str());
            let broken_events = ws
                .db_path
                .as_ref()
                .unwrap()
                .parent()
                .unwrap()
                .join("application-records")
                .join(format!("{}-{broken_id}", &hash.as_str()[..16]))
                .join("events.jsonl");
            std::fs::create_dir(&broken_events).unwrap();
            session.as_mut().unwrap().record = Some(broken_record);
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(error.contains("No fields were filled"), "{error}");
            assert!(ws
                .manual_reviews()
                .unwrap()
                .iter()
                .any(|review| review.reason.contains("record could not be written")));
            let current = session.as_mut().unwrap();
            assert!(!current.filled);
            assert!(current
                .preview
                .suggestions
                .iter()
                .any(|mapping| mapping.field_selector == "#candidate"
                    && mapping.value == PacketValue::Text("Synthetic".into())));
            assert_eq!(
                current
                    .driver
                    .read_attribute("#candidate", "data-seen")
                    .unwrap(),
                None
            );
            current.record = None;
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap();
            assert!(session.as_ref().unwrap().filled);
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#skills", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("rust,python")
            );
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .read_attribute("#candidate", "data-seen")
                    .unwrap()
                    .as_deref(),
                Some("Revised")
            );
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
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(error.contains("Review the visual findings"));
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(error.contains("Attachment filename changed"));
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
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
                Some(staged_name.as_str())
            );
            let staged_path = current.staged_files[0].path().join(&staged_name);
            assert_eq!(
                std::fs::read(&staged_path).unwrap(),
                b"%PDF-fictional resume"
            );
            drop(stream);
            // Reuse the fixture session to exercise a later page fill that
            // changes an earlier value; the production UI starts a rescan.
            current.filled = false;
            let (mut stream, _) = listener.accept().unwrap();
            let error = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(error.contains("2 successful field commands"), "{error}");
            assert!(error.contains("Given name"), "{error}");
            assert!(
                error.contains("An attachment could not be verified"),
                "{error}"
            );
            assert_eq!(session.as_ref().unwrap().completed_selectors.len(), 2);
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            session
                .as_mut()
                .unwrap()
                .driver
                .attach_local_file("#resume", &staged_path)
                .unwrap();
            // An earlier wizard section can contain an unrelated upload control.
            // It must not be verified against the currently displayed page.
            session.as_mut().unwrap().attachments.push((
                "#earlier-upload".into(),
                staged_path.clone(),
                "previous-inspection".into(),
            ));
            session.as_mut().unwrap().filled = false;
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap();
            assert!(session
                .as_ref()
                .unwrap()
                .preview
                .manual_review
                .iter()
                .any(|reason| reason.contains("Attachment at #resume could not be verified")));
            assert!(!session
                .as_ref()
                .unwrap()
                .preview
                .manual_review
                .iter()
                .any(|reason| reason.contains("#earlier-upload")));
            drop(stream);
            let _vision_guard = visual_review::TEST_TASK_LOCK.lock().unwrap();
            let current = session.as_mut().unwrap();
            current
                .driver
                .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                    selector: "#candidate".into(),
                    value: PacketValue::Text("Before visual capture".into()),
                })
                .unwrap();
            let (width, height) = current.driver.page_dimensions().unwrap();
            let (clip_width, y, clip_height, _, _) =
                visual_review::segment_geometry(width, height, 0).unwrap();
            let image = current
                .driver
                .screenshot_segment(clip_width, y, clip_height)
                .unwrap();
            let current_url = current.driver.current_url().unwrap();
            let signature = visual_review::page_signature(&current_url, width, height);
            visual_review::set_completed_fixture(
                "fixture-vision",
                &current.id,
                signature.as_str(),
                &image,
            );
            current
                .driver
                .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                    selector: "#candidate".into(),
                    value: PacketValue::Text("Changed visually".into()),
                })
                .unwrap();
            assert_eq!(
                (width, height),
                current.driver.page_dimensions().unwrap(),
                "fixture page dimensions changed"
            );
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap();
            assert!(ws
                .manual_review_reasons(&id)
                .unwrap()
                .iter()
                .any(|reason| reason.reason == "Form changed during visual review"));
            drop(stream);
            session
                .as_mut()
                .unwrap()
                .driver
                .execute(&waypoint_applications::form_mapping::BrowserCommand::Fill {
                    selector: "#candidate".into(),
                    value: PacketValue::Text("Changed again visually".into()),
                })
                .unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap();
            drop(stream);
            visual_review::set_completed_fixture(
                "fixture-vision-stale",
                "test-session",
                "fictional-old-signature",
                &image,
            );
            let (mut stream, _) = listener.accept().unwrap();
            let stale = serve(&mut stream, &mut ws, &mut session, "test-token", port).unwrap_err();
            assert!(stale.contains("no longer current"));
            json_reply(&mut stream, "400 Bad Request", json!({"error":stale}));
            drop(stream);
            drop(session);
            assert!(!staged_path.exists());
            let records = workspace_dir.path().join("application-records");
            let record = std::fs::read_dir(records)
                .unwrap()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    std::fs::read_to_string(path.join("events.jsonl"))
                        .is_ok_and(|events| events.contains("fields_requested"))
                })
                .unwrap();
            let events = std::fs::read_to_string(record.join("events.jsonl")).unwrap();
            let freshness_events: Vec<Value> = events
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter(|event| {
                    event["type"] == "visual_freshness_checked"
                        && event["task_id"] == "fixture-vision"
                })
                .collect();
            assert_eq!(freshness_events.len(), 2);
            assert_ne!(
                freshness_events[0]["screenshot"],
                freshness_events[1]["screenshot"]
            );
            let first = record.join(freshness_events[0]["screenshot"].as_str().unwrap());
            let second = record.join(freshness_events[1]["screenshot"].as_str().unwrap());
            assert_ne!(
                std::fs::read(first).unwrap(),
                std::fs::read(second).unwrap()
            );
            assert!(
                events.contains("Revised")
                    && events.contains("manual@example.invalid")
                    && events.contains("document_attached")
            );
            assert!(events.contains("\"segments\":2"), "{events}");
            assert!(events.contains("document_visual_review_linked"));
            assert!(events.contains("attachment_verification_failed"));
            assert!(events.contains("visual_freshness_checked"));
            assert!(events.contains("visual_page_changed"));
            assert!(std::fs::read_dir(&record)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("document-visual-")));

            assert!(
                events.contains("fill_incomplete") && events.contains("completed_commands"),
                "{events}"
            );
            let event_values: Vec<Value> = events
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let draft_link = event_values
                .iter()
                .find(|event| event["type"] == "document_draft_evidence_linked")
                .unwrap();
            assert_eq!(draft_link["document_id"], "fictional-doc");
            assert_eq!(draft_link["context_current"], true);
            let linked_draft: Value = serde_json::from_slice(
                &std::fs::read(
                    record
                        .join("materials")
                        .join(draft_link["evidence"].as_str().unwrap()),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(linked_draft["draft"]["facts"][0], "Fictional qualification");
            assert_eq!(
                std::fs::read(
                    record
                        .join("materials")
                        .join(draft_link["template"].as_str().unwrap())
                )
                .unwrap(),
                b"fictional template"
            );
            let requested = event_values
                .iter()
                .find(|event| {
                    event["type"] == "fields_requested"
                        && event["fields"].as_array().is_some_and(|fields| {
                            fields.iter().any(|field| {
                                field["selector"] == "#candidate"
                                    && field["value"]["text"] == "Revised"
                            })
                        })
                })
                .unwrap();
            let field = requested["fields"]
                .as_array()
                .unwrap()
                .iter()
                .find(|field| field["selector"] == "#candidate")
                .unwrap();
            assert_eq!(field["question"], "Given name");
            assert_eq!(field["review_label"], "Given name");
            assert_eq!(field["source_draft"]["task_id"], "fixture-draft");
            assert_eq!(
                field["source_draft"]["sources"][0],
                "Fictional research experience"
            );
            assert!(requested["note"].as_str().unwrap().contains("not proof"));
            let incomplete = event_values
                .iter()
                .position(|event| event["type"] == "fill_incomplete")
                .unwrap();
            assert_eq!(
                event_values[incomplete]["completed_commands"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert!(event_values
                .iter()
                .skip(incomplete + 1)
                .any(|event| event["type"] == "page_capture" && event["segments"] == 2));
            let failure_viewport = event_values
                .iter()
                .skip(incomplete + 1)
                .find(|event| event["type"] == "fill_failure_viewport")
                .unwrap();
            assert!(record
                .join(failure_viewport["screenshot"].as_str().unwrap())
                .is_file());
            let capture_end = event_values
                .iter()
                .enumerate()
                .skip(incomplete + 1)
                .find(|(_, event)| event["type"] == "page_capture")
                .unwrap()
                .0;
            let captured: Vec<_> = event_values[incomplete + 1..capture_end]
                .iter()
                .filter(|event| event["type"] == "page_capture_segment")
                .collect();
            assert_eq!(captured.len(), 2);
            for event in captured {
                assert!(record.join(event["screenshot"].as_str().unwrap()).is_file());
            }
            assert!(
                std::fs::read_dir(record.join("screenshots"))
                    .unwrap()
                    .count()
                    >= 6
            );
            assert!(std::fs::read_dir(record.join("materials")).unwrap().count() >= 1);
        });
        let body = r#"{"id":"fixture-web","require_no_session":true}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/inspect-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("Close the active review session"));
        let body = r##"{"session_id":"test-session","reviewed":true,"selectors":["#country"],"answers":[{"selector":"#candidate","value":"Revised","draft_id":"fixture-draft"},{"selector":"#email","value":"manual@example.invalid"},{"selector":"#available","value":"2029-02-28"},{"selector":"#travel","value":false},{"selector":"#onsite","value":"Remote"},{"selector":"#skills","value":["Rust","Python"]}]}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("Profile changed"), "{response}");
        let invalid = r##"{"session_id":"test-session","reviewed":true,"selectors":["#country"],"answers":[{"selector":"#available","value":"not-a-date"}]}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{invalid}", invalid.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("invalid value"), "{response}");
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("attempt record could not be created"),
            "{response}"
        );
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("attempt record could not be written"),
            "{response}"
        );
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let payload: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["filled"], 7);
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
        let warnings = payload["manual_review"].as_array().unwrap();
        assert!(warnings
            .iter()
            .any(|warning| warning == "Required field needs review: Work authorization"));
        assert!(!warnings
            .iter()
            .any(|warning| warning == "Required field needs review: Given name"));
        assert!(warnings
            .iter()
            .any(|warning| warning == "A separate issue needs review"));
        assert!(payload["screenshot"].as_str().unwrap().starts_with("iVBOR"));
        let body = r##"{"session_id":"test-session","reviewed":true,"selector":"#resume","document_id":"fictional-doc","visual_review_attempt":"0123456789abcdef0123456789abcdef"}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/attach HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("Review the visual findings"));
        let body = r##"{"session_id":"test-session","reviewed":true,"selector":"#resume","document_id":"fictional-doc","visual_review_attempt":"0123456789abcdef0123456789abcdef","reviewed_visual_findings":true,"expected_attachment_name":"stale.pdf"}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/attach HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("Attachment filename changed"));
        let staged_name = attempt_record::attachment_filename(&format!("{}.pdf", "x".repeat(251)));
        let body = json!({"session_id":"test-session","reviewed":true,"selector":"#resume","document_id":"fictional-doc","visual_review_attempt":"0123456789abcdef0123456789abcdef","reviewed_visual_findings":true,"expected_attachment_name":staged_name}).to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/attach HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"), "{response}");
        assert_eq!(
            serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()
                ["attachment_name"],
            staged_name
        );
        let body = r##"{"session_id":"test-session","reviewed":true,"selectors":["#candidate"],"answers":[{"selector":"#later-answer","value":"Changed"}]}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("400 Bad Request") && response.contains("Given name"),
            "{response}"
        );
        let body = r##"{"session_id":"test-session","reviewed":true,"selectors":[],"answers":[{"selector":"#clear-attachment","value":"Changed"}]}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("Attachment at #resume could not be verified"),
            "{response}"
        );
        let body = r#"{"task_id":"fixture-vision"}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/visual-review-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let payload: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["visual_freshness"], "changed");
        assert_eq!(payload["freshness_record_saved"], true);
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/visual-review-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let payload: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(payload["visual_freshness"], "changed");
        assert_eq!(payload["freshness_record_saved"], true);
        let body = r#"{"task_id":"fixture-vision-stale"}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/visual-review-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("no longer current"), "{response}");
        worker.join().unwrap();
    }
}
