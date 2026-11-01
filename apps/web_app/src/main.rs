#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Local browser interface for the existing Waypoint workspace.
mod answer_task;
mod application_check;
pub mod application_email;
mod ats_check;
mod attempt_record;
mod candidate_selection;
mod candidate_task;
mod choice_advice;
mod contact_advice;
mod control_rank;
mod control_vision;
mod convert_task;
mod custom_choice_action;
mod discovery_task;
mod doc_suggest;
mod document_convert;
mod document_extract;
mod document_visual;
mod docx_edit;
mod docx_edit_task;
mod docx_revision;
mod draft_task;
mod duplicate_compare;
mod duplicate_search;
mod email_draft;
pub mod email_worker;
mod experience_inference;
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
mod gpu_resources;
mod letter_export;
mod link_rank;
mod manual_job;
mod model_install;
mod model_start;
mod pdf_extract;
mod pdf_job;
mod pdf_preview_task;
mod pdf_probe_task;
mod pdf_setup;
mod pdf_transcribe;
mod resident_inference;
mod runtime_start;
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
    inspection_task_id: String,
    inspection_pending_revision: i64,
    url: String,
    profile_digest: Sha256Digest,
    preview: form_preview::FormPreview,
    driver: waypoint_browser::CdpDriver,
    created: Instant,
    ttl: Duration,
    filled: bool,
    /// Sticky across rescans and navigation within this browser session.
    fill_attempted: bool,
    completed_selectors: Vec<String>,
    selected_choices: Vec<(String, String)>,
    answer_drafts: std::collections::HashMap<String, SavedFormAnswer>,
    visible: bool,
    staged_files: Vec<tempfile::TempDir>,
    // Attachment verification is scoped to the inspection session in which
    // the control was observed; later wizard sections can reuse selectors.
    attachments: Vec<(String, std::path::PathBuf, String, String)>,
    pending_choice_visual: Option<(String, String, i64, Option<String>)>,
    last_visual_freshness_key: Option<String>,
    record: Option<attempt_record::AttemptRecord>,
}

fn current_attachment_groups(
    attachments: &[(String, std::path::PathBuf, String, String)],
    inspection: &str,
) -> Vec<(String, Vec<std::path::PathBuf>)> {
    let mut groups: Vec<(String, Vec<std::path::PathBuf>)> = Vec::new();
    for (selector, path, _, _) in attachments.iter().filter(|(_, _, id, _)| id == inspection) {
        if let Some((_, paths)) = groups.iter_mut().find(|(key, _)| key == selector) {
            paths.push(path.clone());
        } else {
            groups.push((selector.clone(), vec![path.clone()]));
        }
    }
    groups
}

fn capture_field_label(
    schema: &waypoint_applications::form_mapping::FormSchema,
    selector: &str,
) -> Option<String> {
    let mut matches = schema
        .fields
        .iter()
        .filter(|field| field.selector == selector);
    let first = matches.next()?;
    if matches.next().is_some()
        || first.label.trim().is_empty()
        || first.label.len() > 500
        || first.label.chars().any(char::is_control)
    {
        return None;
    }
    Some(first.label.clone())
}

fn staged_file_matches(path: &std::path::Path, expected_sha256: &str) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    if file.metadata().map_or(true, |metadata| {
        !metadata.is_file() || metadata.len() > MAX_DOCUMENT as u64
    }) {
        return false;
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_DOCUMENT as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > MAX_DOCUMENT
    {
        return false;
    }
    waypoint_domain::sha256_hex(&bytes) == expected_sha256
}

fn staged_attachments_unchanged(
    attachments: &[(String, std::path::PathBuf, String, String)],
    inspection: &str,
    selector: &str,
) -> bool {
    attachments
        .iter()
        .filter(|(field, _, id, _)| field == selector && id == inspection)
        .all(|(_, path, _, digest)| staged_file_matches(path, digest))
}

struct SavedFormAnswer {
    selector: String,
    digest: String,
    answer: Value,
}

fn active_form_review_reasons(
    job: &JobIdentityId,
    session: &Option<ReviewSession>,
) -> Result<Vec<String>, String> {
    let mut reasons = form_links::protected_reasons(job)?;
    reasons.extend(custom_choice_action::protected_reasons(job)?);
    if let Some(current) = session.as_ref().filter(|current| &current.job_id == job) {
        reasons.push(form_task::pending_reason(&current.inspection_task_id));
    }
    if let Some((task, task_job)) = form_task::pending()? {
        if task_job == job.as_str() {
            reasons.push(form_task::pending_reason(&task));
        }
    }
    Ok(reasons)
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
        form_task::finish_pending(
            ws,
            &current.job_id,
            &current.inspection_task_id,
            current.inspection_pending_revision,
        )?;
    }
    *session = None;
    Ok(())
}

fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("random id generation failed: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn experience_snapshot_current(
    revisions: &[(String, u64)],
    current: &[waypoint_workspace::form_experience::ExperienceMatch],
) -> bool {
    revisions.iter().all(|(id, revision)| {
        current
            .iter()
            .any(|item| item.id == *id && item.revision == *revision && item.experience.enabled)
    })
}

fn protect_navigation_experience_snapshot(
    preview: &mut form_preview::FormPreview,
    revisions: &[(String, u64)],
    current: Option<&[waypoint_workspace::form_experience::ExperienceMatch]>,
) {
    if !revisions.is_empty()
        && !current.is_some_and(|items| experience_snapshot_current(revisions, items))
    {
        preview.suggestions.clear();
        preview.manual_review.push("A lesson changed or could not be checked during inspection; suggestions were discarded. Inspect again.".into());
    }
}

/// A listing source is only a retrieval hint; the application may be hosted elsewhere.
fn navigation_experiences(
    ws: &Workspace,
    id: &JobIdentityId,
) -> Result<Vec<waypoint_workspace::form_experience::ExperienceMatch>, String> {
    use waypoint_workspace::form_experience::{Control, Stage};
    let job = ws
        .job(id)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let platform = experience_platform_hint(&job.source);
    let mut result = ws
        .related_form_experiences(Stage::EnterInformation, Control::Text, platform)
        .map_err(|e| e.to_string())?;
    result.extend(
        ws.related_form_experiences(Stage::EnterInformation, Control::Choice, platform)
            .map_err(|e| e.to_string())?,
    );
    Ok(result)
}

fn experience_platform_hint(source: &str) -> Option<waypoint_workspace::form_experience::Platform> {
    use waypoint_workspace::form_experience::Platform;
    match source {
        "greenhouse" => Some(Platform::Greenhouse),
        "lever" => Some(Platform::Lever),
        "ashby" => Some(Platform::Ashby),
        "workday" => Some(Platform::Workday),
        _ => None,
    }
}

fn draft_request_id(value: &Value) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(id)
            if id.len() == 34
                && id.starts_with("b-")
                && id[2..]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
        {
            Ok(Some(id.clone()))
        }
        _ => Err("Invalid draft request identifier".into()),
    }
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
    "search_remote_only",
    "search_locations",
];

fn campaign_constraints(profile: &Value) -> Result<CampaignConstraints, String> {
    let remote = profile
        .get("search_remote_only")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !["", "on"].contains(&remote) {
        return Err("Invalid remote-only setting".into());
    }
    let locations = profile
        .get("search_locations")
        .and_then(Value::as_str)
        .unwrap_or("");
    let locations: Vec<String> = locations
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if locations.len() > 50 || locations.iter().any(|line| line.len() > 200) {
        return Err("Enter at most 50 locations, each at most 200 bytes".into());
    }
    Ok(CampaignConstraints {
        remote_only: remote == "on",
        locations,
        ..Default::default()
    })
}

fn profile_fit_instructions(profile: &Value) -> Result<String, String> {
    let constraints = campaign_constraints(profile)?;
    let mut parts = Vec::new();
    if constraints.remote_only {
        parts.push("Remote work is required.".to_owned());
    }
    if !constraints.locations.is_empty() {
        parts.push(format!(
            "Allowed job locations (any one): {}.",
            constraints.locations.join("; ")
        ));
    }
    if let Some(text) = profile["instructions"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
    {
        parts.push(text.to_owned());
    }
    Ok(parts.join("\n"))
}

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
    let full_instructions = profile_fit_instructions(&profile)?;
    let instructions: String = full_instructions.chars().take(2000).collect();
    let protected = search_planner::protected_identifiers(&profile);
    // All active facts affect freshness, including facts omitted from the prompt.
    let facts_digest = ws.active_facts_digest().map_err(|e| e.to_string())?;
    let digest = Sha256Digest::of(
        &json!({"facts":facts,"active_facts_digest":facts_digest,"instructions":instructions,"full_instructions":full_instructions,"protected":protected}).to_string(),
    );
    Ok((facts, instructions, digest.as_str().to_owned(), protected))
}

fn search_plan_coverage(
    ws: &Workspace,
    facts: &[String],
    instructions: &str,
) -> Result<Value, String> {
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(content) => serde_json::from_str(&content).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let full = profile_fit_instructions(&profile)?;
    let (total, oversized) = ws.reviewed_fact_coverage(1000).map_err(|e| e.to_string())?;
    Ok(
        json!({"reviewed_total":total,"included":facts.len(),"oversized":oversized,
        "over_limit":total.saturating_sub(oversized).saturating_sub(facts.len()),
        "instructions_used_chars":instructions.chars().count(),"instructions_total_chars":full.chars().count()}),
    )
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

/// Bind all active facts while keeping the model's evidence pool bounded.
/// Omitted facts invalidate prior assessments but are not evaluated by the model.
fn fit_assessment_inputs(
    ws: &Workspace,
    job_id: &JobIdentityId,
) -> Result<(Vec<String>, usize, String, String, String), String> {
    let (facts, instructions) = fit_candidate_context(ws)?;
    let facts_digest = ws.active_facts_digest().map_err(|e| e.to_string())?;
    let (pool, listing, digest) =
        fit_assessment_digest(ws, job_id, &facts, &instructions, &facts_digest)?;
    Ok((pool, facts.len(), instructions, listing, digest))
}

fn fit_assessment_digest(
    ws: &Workspace,
    job_id: &JobIdentityId,
    facts: &[String],
    instructions: &str,
    facts_digest: &str,
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
    let full_instructions = profile_fit_instructions(&profile)?;
    let digest = Sha256Digest::of(&json!({"scoring_version":"criteria-v2-semantic-pool-v49","active_facts_digest":facts_digest,"facts":facts,"pool":pool,"instructions":instructions,"listing":listing,"full_description":description,"full_instructions":full_instructions}).to_string()).as_str().to_owned();
    Ok((pool, listing, digest))
}

fn fit_remote_status_text(status: Option<bool>) -> &'static str {
    match status {
        Some(true) => "Source indicates remote work; eligible locations are not established by this flag",
        Some(false) => "Structured remote flag is false; some remote work may still be possible in a hybrid arrangement, which this flag does not confirm or exclude",
        None => "Remote status is not established by structured source data",
    }
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
    let remote = fit_remote_status_text(ws.job_remote_status(job_id).map_err(|e| e.to_string())?);
    let full_listing = format!(
        "Title: {}\nEmployer: {}\nLocation: {}\nRemote status: {}\nDescription: {}",
        job.title, job.employer, job.location, remote, description
    );
    let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
        Some(text) => serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?,
        None => json!({}),
    };
    let full_instructions = profile_fit_instructions(&profile)?;
    let (reviewed_total, oversized) = ws.reviewed_fact_coverage(1000).map_err(|e| e.to_string())?;
    let eligible = reviewed_total.saturating_sub(oversized);
    let bounded = eligible.min(500);
    let (expected_listing, used_chars) = fit_listing_excerpt(&full_listing);
    let sampling = if listing != expected_listing {
        "unavailable"
    } else if used_chars < full_listing.chars().count() {
        "beginning_middle_end"
    } else {
        "complete"
    };
    let used_chars = if listing == expected_listing {
        used_chars
    } else {
        0
    };
    Ok(
        json!({"listing_used_chars":used_chars,"listing_total_chars":full_listing.chars().count(),"listing_sampling":sampling,"description_present":!description.trim().is_empty(),"instructions_used_chars":instructions.chars().count(),"instructions_total_chars":full_instructions.chars().count(),"reviewed_facts_total":reviewed_total,"reviewed_facts_bounded":bounded,"reviewed_facts_oversized":oversized,"reviewed_facts_over_limit":eligible.saturating_sub(bounded)}),
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
    let instructions: String = profile_fit_instructions(&profile)?
        .chars()
        .take(2000)
        .collect();
    Ok((facts, instructions))
}

/// Sample the beginning, middle and end without presenting the gaps as source text.
fn fit_listing_excerpt(full: &str) -> (String, usize) {
    const LIMIT: usize = 8000;
    const TAIL: usize = 2000;
    const MIDDLE: usize = 2000;
    const GAP: &str = "\n[Listing text omitted; inspect the full original.]\n";
    let total = full.chars().count();
    if total <= LIMIT {
        return (full.to_owned(), total);
    }
    let head = LIMIT - TAIL - MIDDLE - 2 * GAP.chars().count();
    let omitted = total - (head + MIDDLE + TAIL);
    let middle_start = head + omitted / 2;
    let prefix: String = full.chars().take(head).collect();
    let middle: String = full.chars().skip(middle_start).take(MIDDLE).collect();
    let suffix: String = full.chars().skip(total - TAIL).collect();
    (
        format!("{prefix}{GAP}{middle}{GAP}{suffix}"),
        head + MIDDLE + TAIL,
    )
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
    let remote = fit_remote_status_text(ws.job_remote_status(job_id).map_err(|e| e.to_string())?);
    let full_listing = format!(
        "Title: {}\nEmployer: {}\nLocation: {}\nRemote status: {}\nDescription: {}",
        job.title, job.employer, job.location, remote, description
    );
    let (listing, _) = fit_listing_excerpt(&full_listing);
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

struct DocumentVisualContext {
    prompt: String,
    digest: String,
}

fn document_visual_context(
    ws: &Workspace,
    job: &JobIdentityId,
) -> Result<DocumentVisualContext, String> {
    let (facts, _, listing, _) = fit_inputs(ws, job)?;
    let posting = ws
        .job(job)
        .map_err(|e| e.to_string())?
        .ok_or("Job not found")?;
    let description = ws
        .job_description(job)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let facts_digest = ws.active_facts_digest().map_err(|e| e.to_string())?;
    let profile = ws
        .candidate_profile()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "{}".into());
    let profile_value: Value =
        serde_json::from_str(&profile).map_err(|_| "stored profile is invalid")?;
    let instructions = profile_value["document_instructions"]
        .as_str()
        .unwrap_or("");
    let digest = Sha256Digest::of(&json!({"version":"document-visual-context-v2","title":posting.title,"employer":posting.employer,"location":posting.location,"description":description,"facts_digest":facts_digest,"profile":profile_value}).to_string()).as_str().to_owned();
    let prompt = format!(
        "Posting: {}\nReviewed qualifications: {}\nDocument instructions: {}",
        listing.chars().take(5000).collect::<String>(),
        facts.join(" | ").chars().take(5000).collect::<String>(),
        instructions.chars().take(2000).collect::<String>()
    );
    Ok(DocumentVisualContext { prompt, digest })
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
        || review["context_sha256"].as_str() != Some(context.digest.as_str())
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

fn load_draft_result(ws: &Workspace, job: &JobIdentityId, id: &str) -> Result<Value, String> {
    match draft_task::poll(id) {
        Ok(value) => Ok(value),
        Err(error) if error == "Draft task not found" => {
            let bytes = ws
                .generated_draft(id, job)
                .map_err(|e| e.to_string())?
                .ok_or("Draft task not found")?;
            serde_json::from_slice(&bytes).map_err(|_| "Saved draft is invalid".into())
        }
        Err(error) => Err(error),
    }
}

fn fit_review_revision(value: &Value) -> String {
    Sha256Digest::of(
        &json!({"assessment":value["assessment"],"user_review":value["user_review"]}).to_string(),
    )
    .as_str()
    .to_owned()
}

fn read_fit(ws: &Workspace, job_id: &JobIdentityId) -> Result<Value, String> {
    read_fit_with_context(ws, job_id, &mut None)
}

fn read_fit_with_context(
    ws: &Workspace,
    job_id: &JobIdentityId,
    context: &mut Option<(Vec<String>, String, String)>,
) -> Result<Value, String> {
    match ws.fit_assessment(job_id).map_err(|e| e.to_string())? {
        Some(data) => {
            let value: Value =
                serde_json::from_slice(&data).map_err(|_| "Stored assessment is invalid")?;
            if context.is_none() {
                let (facts, instructions) = fit_candidate_context(ws)?;
                let facts_digest = ws.active_facts_digest().map_err(|e| e.to_string())?;
                *context = Some((facts, instructions, facts_digest));
            }
            let (facts, instructions, facts_digest) =
                context.as_ref().ok_or("Candidate context unavailable")?;
            let (_, listing, digest) =
                fit_assessment_digest(ws, job_id, facts, instructions, facts_digest)?;
            if value["input_digest"].as_str() == Some(digest.as_str()) {
                let mut assessment = value["assessment"].clone();
                if !assessment.is_object() {
                    return Err("Stored assessment is invalid".into());
                }
                assessment["source_coverage"] =
                    fit_source_coverage(ws, job_id, &listing, instructions)?;
                let coverage = if assessment["preference_audit"].is_null()
                    && instructions.trim().is_empty()
                    && assessment["source_coverage"]["instructions_total_chars"] == 0
                {
                    "not_requested"
                } else {
                    fit_task::preference_coverage(&assessment)
                };
                assessment["preference_coverage"] = json!(coverage);
                assessment["requirement_coverage"] =
                    json!(fit_task::requirement_coverage(&assessment));
                assessment["requirement_audit_incomplete"] =
                    json!(fit_task::requirement_audit_incomplete(&assessment));
                Ok(
                    json!({"status":"current","assessment":assessment,"model":value["model"],"user_review":value["user_review"],"review_history":value["review_history"],"review_history_omitted":value["review_history_omitted"],"review_revision":fit_review_revision(&value)}),
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
                    FormFieldType::Month
                        if waypoint_applications::form_mapping::valid_iso_month(value) =>
                    {
                        PacketValue::Date(value.clone())
                    }
                    FormFieldType::Time
                        if waypoint_applications::form_mapping::valid_iso_time(value) =>
                    {
                        PacketValue::Date(value.clone())
                    }
                    FormFieldType::DateTimeLocal
                        if waypoint_applications::form_mapping::valid_iso_datetime_local(value) =>
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
                        if !text.is_empty()
                            && field.min_length_utf16.is_some_and(|minimum| {
                                text.encode_utf16().count() < minimum as usize
                            })
                        {
                            return Err(format!("The answer for {} is shorter than the page's {} UTF-16-unit minimum; revise it before filling", field.label, field.min_length_utf16.unwrap_or(0)));
                        }
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
        min_length_utf16: field.min_length_utf16,
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

fn require_duplicate_index_ready(ws: &Workspace) -> Result<(), String> {
    if ws.duplicate_index_pending().map_err(|e| e.to_string())? {
        return Err("Duplicate checks are still running; wait for indexing before interacting with an application".into());
    }
    Ok(())
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
        min_length_utf16: None,
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
    let listing = |id: &JobIdentityId| -> Result<(String, String, bool), String> {
        let job = ws
            .job(id)
            .map_err(|e| e.to_string())?
            .ok_or("Job no longer exists")?;
        let description = ws
            .job_description(id)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let full = format!(
            "Title: {}\nEmployer: {}\nLocation: {}\nRequisition: {}\nPosted: {}\nDescription: {}",
            job.title,
            job.employer,
            job.location,
            job.requisition_id.unwrap_or_default(),
            job.posted_at.unwrap_or_default(),
            description
        );
        let full_digest = Sha256Digest::of(
            &json!({"text":full,"url":job.canonical_url,"remote":job.remote}).to_string(),
        )
        .as_str()
        .to_owned();
        // Keep both descriptions and the response within the local model's
        // bounded context; count JSON escaping and preserve UTF-8 boundaries.
        let truncated = answer_task::json_prefix(&full, 2600).len() < full.len();
        let sample = duplicate_search::bounded_listing_sample(&full, 2600, [1100, 600, 700]);
        Ok((sample, full_digest, truncated))
    };
    let (first_text, first_digest, first_truncated) = listing(first)?;
    let (second_text, second_digest, second_truncated) = listing(second)?;
    let digest =
        Sha256Digest::of(&json!({"first":first_digest,"second":second_digest}).to_string())
            .as_str()
            .to_owned();
    Ok(duplicate_compare::Inputs {
        first_id: first.as_str().to_owned(),
        second_id: second.as_str().to_owned(),
        first: first_text,
        second: second_text,
        digest,
        first_truncated,
        second_truncated,
    })
}

#[cfg(test)]
fn duplicate_search_inputs(
    ws: &Workspace,
    id: &JobIdentityId,
) -> Result<duplicate_search::Inputs, String> {
    duplicate_search_inputs_page(ws, id, None)
}
fn duplicate_search_inputs_page(
    ws: &Workspace,
    id: &JobIdentityId,
    after: Option<&str>,
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
        let short = duplicate_search::sample_listing(&full);
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
    let (neighbors, truncated, next_cursor) = if let Some(after) = after {
        let (neighbors, next) = ws
            .duplicate_neighbors_page(id, after)
            .map_err(|e| e.to_string())?;
        (neighbors, next.is_some(), next)
    } else {
        let (neighbors, truncated) = ws.duplicate_neighbors(id).map_err(|e| e.to_string())?;
        (neighbors, truncated, None)
    };
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
        &json!({"target":target,"target_digest":target_digest,"candidates":candidates,"source_digests":source_digests,"truncated":truncated,"page_after":after,"next_cursor":next_cursor}).to_string(),
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
        page_after: after.map(str::to_owned),
        next_cursor,
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
    verify_selected_choices(session)?;
    Ok(())
}

fn verify_selected_choices(session: &mut ReviewSession) -> Result<(), String> {
    verify_selected_choices_except(session, None)
}

fn verify_selected_choices_except(
    session: &mut ReviewSession,
    excluded: Option<&str>,
) -> Result<(), String> {
    let choices: Vec<_> = session
        .selected_choices
        .iter()
        .filter(|(selector, _)| excluded != Some(selector.as_str()))
        .map(|(selector, value)| (selector.as_str(), value.as_str()))
        .collect();
    session
        .driver
        .with_review_timeout(|driver| driver.verify_custom_choice_values(&choices))
        .map_err(|_| {
            "A previously selected choice changed or could not be verified; inspect the form again"
                .to_owned()
        })
}

fn custom_choice_snapshot(
    driver: &mut waypoint_browser::CdpDriver,
) -> Result<(waypoint_browser::CustomChoices, String), String> {
    let live = driver
        .inspect_current()
        .map_err(|error| error.to_string())?;
    let url = driver.current_url().map_err(|error| error.to_string())?;
    let choices = driver
        .page_custom_choices()
        .map_err(|error| error.to_string())?;
    let digest = Sha256Digest::of(
        &json!({"url":url,"schema":live.schema,"walls":live.walls,"choices":choices}).to_string(),
    );
    Ok((choices, digest.as_str().to_owned()))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs() as i64)
        .unwrap_or(0)
}

/// Reserve a conservative share of RAM before beginning another inference.
/// This is a snapshot, not a reservation or a guarantee against later pressure.
#[cfg(any(windows, test))]
fn inference_memory_gate(available: u64, total: u64) -> Result<(), String> {
    const GIB: u64 = 1024 * 1024 * 1024;
    if total == 0 || available > total {
        return Err("System memory could not be sampled reliably; local inference paused".into());
    }
    let floor = (total / 4).clamp(4 * GIB, 10 * GIB);
    if available < floor {
        return Err(format!("Local inference paused: {} MiB system memory available; at least {} MiB is required before starting another request. Close other work and retry.", available / (1024 * 1024), floor.div_ceil(1024 * 1024)));
    }
    Ok(())
}

#[cfg(windows)]
fn check_inference_memory() -> Result<(), String> {
    let mut system = sysinfo::System::new()
        .map_err(|_| "System memory could not be sampled; local inference paused")?;
    system.refresh_memory();
    inference_memory_gate(system.available_memory(), system.total_memory())
}

/// Only use one already-resident model that Ollama reports as fully GPU
/// resident and within the existing 14 GiB budget. This check is a snapshot,
/// not a reservation; Ollama can change its state after it is read.
fn resident_model() -> Result<Option<String>, String> {
    #[cfg(windows)]
    check_inference_memory()?;
    let status = resident_model_status()?;
    Ok(status["model"].as_str().map(str::to_owned))
}

fn form_model_from_status(status: &Value) -> Result<Option<String>, String> {
    match status["state"].as_str() {
        Some("ready") => status["model"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(|name| Some(name.to_owned()))
            .ok_or_else(|| "Model status is incomplete".into()),
        Some("no_model") => Ok(None),
        Some("multiple_models") => {
            Err("Multiple models are resident; use one eligible model".into())
        }
        Some("not_fully_gpu") => Err("The resident model is not fully GPU-resident".into()),
        Some("over_budget") => Err("The resident model exceeds the GPU memory budget".into()),
        _ => Err("Resident model status is incomplete or unavailable".into()),
    }
}

fn form_resident_model() -> Result<Option<String>, String> {
    #[cfg(windows)]
    check_inference_memory()?;
    form_model_from_status(&resident_model_status()?)
}

fn resident_model_status() -> Result<Value, String> {
    let addr = "127.0.0.1:11434"
        .parse()
        .map_err(|_| "invalid runtime address")?;
    resident_model_status_at(addr)
}

fn resident_model_status_at(addr: std::net::SocketAddr) -> Result<Value, String> {
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
        entries.push(json!({"name":name,"disk_bytes":model["size"].as_u64(),"format":model["details"]["format"].as_str().filter(|format| format.len() <= 64 && !format.chars().any(char::is_control))}));
    }
    Ok(
        json!({"models":entries,"reported_count":models.len(),"examined":models.len().min(100),"invalid":invalid}),
    )
}

#[cfg(any(windows, test))]
fn model_inventory_memory_snapshot(
    inventory: &mut Value,
    tags: &Value,
    system_available: u64,
    gpu: &Value,
) {
    if let Some(models) = inventory["models"].as_array_mut() {
        for model in models {
            let Some(name) = model["name"].as_str() else {
                continue;
            };
            let check = model_start::installed_size(tags, name)
                .and_then(|size| model_start::resource_gate(size, system_available, gpu));
            model["memory_check"] = match check {
                Ok(()) => json!({"state":"fits_snapshot"}),
                Err(reason) => json!({"state":"not_ready","reason":reason}),
            };
        }
    }
}

fn model_diagnostics(data: &Value) -> Result<Value, String> {
    let eligible = eligible_resident_model(data)?;
    let models = data["models"]
        .as_array()
        .ok_or("Ollama did not report resident models")?;
    if let Some(name) = eligible {
        return Ok(
            json!({"model":name,"model_digest":models[0]["digest"].as_str().filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())),"state":"ready","reported_bytes":models[0]["size"],"reported_vram_bytes":models[0]["size_vram"],"context_length":models[0]["context_length"].as_u64()}),
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

/// `/api/show` describes a model's advertised capabilities, not the accuracy
/// of those capabilities or whether the model remains resident afterward.
fn vision_capability(data: &Value) -> Option<bool> {
    let items = data.get("capabilities")?.as_array()?;
    if items.len() > 100 || items.iter().any(|item| item.as_str().is_none()) {
        return None;
    }
    Some(items.iter().any(|item| item.as_str() == Some("vision")))
}

fn model_status_with_capabilities() -> Result<Value, String> {
    let addr = "127.0.0.1:11434"
        .parse()
        .map_err(|_| "invalid runtime address")?;
    model_status_with_capabilities_at(addr)
}

fn model_status_with_capabilities_at(addr: std::net::SocketAddr) -> Result<Value, String> {
    let mut status = resident_model_status_at(addr)?;
    let Some(model) = status["model"].as_str().map(str::to_owned) else {
        return Ok(status);
    };
    let request = serde_json::to_vec(&json!({"model": model})).map_err(|e| e.to_string())?;
    let vision = http_request(addr, "POST", "/api/show", Some(&request), 3000)
        .ok()
        .and_then(|(code, bytes)| (code == 200).then_some(bytes))
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|data| vision_capability(&data));
    // Discard capability claims if the resident model changed between calls.
    let after = resident_model_status_at(addr).ok();
    if after.as_ref().is_some_and(|after| {
        status["model_digest"].is_string()
            && after["model_digest"] == status["model_digest"]
            && after["model"] == status["model"]
            && after["reported_bytes"] == status["reported_bytes"]
            && after["reported_vram_bytes"] == status["reported_vram_bytes"]
            && after["context_length"] == status["context_length"]
    }) {
        status["vision"] = json!(vision);
    } else {
        status = after.unwrap_or_else(|| json!({"model":null,"state":"unreachable"}));
        status["vision"] = Value::Null;
    }
    Ok(status)
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
        let profile = match ws.candidate_profile().map_err(|e| e.to_string())? {
            Some(content) => {
                serde_json::from_str::<Value>(&content).map_err(|_| "stored profile is invalid")?
            }
            None => json!({}),
        };
        let constraints = campaign_constraints(&profile)?;
        let ranked = ws
            .ranked(&constraints, &skills, now())
            .map_err(|e| e.to_string())?;
        let mut jobs: Vec<Value> = ranked.strict.iter().map(|row| {
            let basis = if skills.is_empty() { "no_reviewed_qualifications" } else if !row.description_available { "no_description" } else { "text_overlap" };
            json!({"job":row.job,"score":if basis == "text_overlap" { Some(row.score) } else { None },"matched":if basis == "text_overlap" { row.matched_skills.clone() } else { vec![] },"match_basis":basis,"remote_status":row.remote_status,"unknown_constraints":row.unknown_constraints})
        }).collect();
        jobs.extend(ranked.excluded.iter().map(|row| {
            let basis = if skills.is_empty() { "no_reviewed_qualifications" } else if !row.description_available { "no_description" } else { "text_overlap" };
            json!({"job":row.job,"score":if basis == "text_overlap" { Some(row.score) } else { None },"matched":if basis == "text_overlap" { row.matched_skills.clone() } else { vec![] },"match_basis":basis,"remote_status":row.remote_status,"unknown_constraints":row.unknown_constraints,"excluded_reasons":row.reasons})
        }));
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
        let status = model_status_with_capabilities();
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
    if method == "GET" && path == "/api/runtime-start/active" {
        json_reply(
            stream,
            "200 OK",
            json!({"task_id":runtime_start::active()?}),
        );
        return Ok(());
    }
    if method == "GET" && path == "/api/model-install/active" {
        json_reply(
            stream,
            "200 OK",
            json!({"task_id":model_install::active()?}),
        );
        return Ok(());
    }
    if method == "GET" && path == "/api/model-start/active" {
        json_reply(stream, "200 OK", json!({"task_id":model_start::active()?}));
        return Ok(());
    }
    if method == "GET" && path == "/api/gpu-status" {
        let result = std::panic::catch_unwind(gpu_resources::sample)
            .unwrap_or_else(|_| json!({"state":"unavailable","reported_adapters":0,"adapters":[]}));
        json_reply(stream, "200 OK", result);
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
        #[allow(unused_mut)]
        let mut inventory = installed_model_inventory(&data)?;
        #[cfg(windows)]
        if let Ok(mut system) = sysinfo::System::new() {
            system.refresh_memory();
            let gpu = std::panic::catch_unwind(gpu_resources::sample)
                .unwrap_or_else(|_| json!({"state":"unavailable"}));
            model_inventory_memory_snapshot(&mut inventory, &data, system.available_memory(), &gpu);
        }
        json_reply(stream, "200 OK", inventory);
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
        "/api/form-experiences/list" => {
            let entries = ws.form_experiences().map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"experiences":entries}));
        }
        "/api/form-experiences/related" => {
            use waypoint_workspace::form_experience::{Control, Platform, Stage};
            let stage: Stage =
                serde_json::from_value(body["stage"].clone()).map_err(|_| "Invalid stage")?;
            let control: Control =
                serde_json::from_value(body["control"].clone()).map_err(|_| "Invalid control")?;
            let platform: Option<Platform> =
                serde_json::from_value(body["platform"].clone()).map_err(|_| "Invalid platform")?;
            let entries = ws
                .related_form_experiences(stage, control, platform)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"experiences":entries}));
        }
        "/api/form-experiences/save" => {
            use waypoint_workspace::form_experience::Experience;
            if body["confirmed"] != true {
                return Err("Review and confirm this experience before saving".into());
            }
            let mut value = body["experience"].clone();
            let object = value.as_object_mut().ok_or("Missing experience")?;
            object.insert("version".into(), json!(1));
            object.insert("reviewed".into(), json!(true));
            object.insert("enabled".into(), json!(true));
            object.insert("observed_at".into(), json!(now()));
            let experience: Experience =
                serde_json::from_value(value).map_err(|_| "Invalid experience categories")?;
            let id = random_id()?;
            let revision = ws
                .save_form_experience(&id, 0, &experience)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"id":id,"revision":revision}));
        }
        "/api/form-experiences/revoke" => {
            if form_task::busy()
                || session
                    .as_ref()
                    .is_some_and(|s| s.created.elapsed() <= s.ttl)
            {
                return Err("Close the active form review before changing lessons".into());
            }
            if body["confirmed"] != true {
                return Err("Confirm removal before revoking this experience".into());
            }
            let id = body["id"].as_str().ok_or("Missing experience ID")?;
            let revision = body["revision"]
                .as_u64()
                .ok_or("Missing experience revision")?;
            if !ws
                .revoke_form_experience(id, revision)
                .map_err(|e| e.to_string())?
            {
                return Err("Experience changed or was already removed; reload".into());
            }
            json_reply(stream, "200 OK", json!({"ok":true}));
        }
        "/api/form-experiences/correct-outcome" => {
            use waypoint_workspace::form_experience::Outcome;
            if form_task::busy()
                || session
                    .as_ref()
                    .is_some_and(|s| s.created.elapsed() <= s.ttl)
            {
                return Err("Close the active form review before changing lessons".into());
            }
            if body["confirmed"] != true {
                return Err("Confirm the corrected outcome".into());
            }
            let id = body["id"].as_str().ok_or("Missing experience ID")?;
            let revision = body["revision"]
                .as_u64()
                .ok_or("Missing experience revision")?;
            let outcome: Outcome =
                serde_json::from_value(body["outcome"].clone()).map_err(|_| "Invalid outcome")?;
            let revision = ws
                .correct_form_experience_outcome(id, revision, outcome)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"revision":revision}));
        }
        "/api/form-experiences/correct-note" => {
            if form_task::busy()
                || session
                    .as_ref()
                    .is_some_and(|s| s.created.elapsed() <= s.ttl)
            {
                return Err("Close the active form review before changing lessons".into());
            }
            if body["confirmed"] != true {
                return Err("Confirm the corrected context".into());
            }
            let id = body["id"].as_str().ok_or("Missing experience ID")?;
            let revision = body["revision"]
                .as_u64()
                .ok_or("Missing experience revision")?;
            let note = match body.get("note").ok_or("Missing context")? {
                Value::Null => None,
                Value::String(text) => Some(text.clone()),
                _ => return Err("Invalid context".into()),
            };
            let use_note_in_model = body["use_note_in_model"]
                .as_bool()
                .ok_or("Choose whether the local model may use this note")?;
            let revision = ws
                .correct_form_experience_note(id, revision, note, use_note_in_model)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"revision":revision}));
        }
        "/api/form-experiences/set-enabled" => {
            if form_task::busy()
                || session
                    .as_ref()
                    .is_some_and(|s| s.created.elapsed() <= s.ttl)
            {
                return Err("Close the active form review before changing lessons".into());
            }
            if body["confirmed"] != true {
                return Err("Confirm the lesson change".into());
            }
            let id = body["id"].as_str().ok_or("Missing experience ID")?;
            let revision = body["revision"]
                .as_u64()
                .ok_or("Missing experience revision")?;
            let enabled = body["enabled"].as_bool().ok_or("Missing enabled state")?;
            let revision = ws
                .set_form_experience_enabled(id, revision, enabled)
                .map_err(|e| e.to_string())?;
            json_reply(stream, "200 OK", json!({"revision":revision}));
        }
        "/api/runtime-start/start" => {
            let id = random_id()?;
            runtime_start::start(id.clone())?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/runtime-start/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", runtime_start::poll(id)?);
        }
        "/api/model-install/start" => {
            let model = body["model"].as_str().ok_or("missing model")?;
            let id = random_id()?;
            model_install::start(id.clone(), model)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/model-install/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", model_install::poll(id)?);
        }
        "/api/model-start/start" => {
            let model = body["model"].as_str().ok_or("missing model")?.to_owned();
            let id = random_id()?;
            model_start::start(id.clone(), model)?;
            json_reply(stream, "200 OK", json!({"task_id":id}));
        }
        "/api/model-start/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", model_start::poll(id)?);
        }
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
            let after = match body.get("after") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) if value.is_empty() => Some(""),
                Some(Value::String(value)) => {
                    JobIdentityId::new(value).map_err(|e| e.to_string())?;
                    Some(value.as_str())
                }
                _ => return Err("Invalid duplicate search cursor".into()),
            };
            let input = duplicate_search_inputs_page(ws, &job, after)?;
            if input.candidates.is_empty() && input.next_cursor.is_none() {
                return Err("No unreviewed employer or requisition candidates were found".into());
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
                let input = duplicate_search_inputs_page(ws, &job, result["page_after"].as_str())?;
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
            let revision = body["revision"]
                .as_i64()
                .filter(|revision| *revision > 0)
                .ok_or("Refresh the duplicate pair before deciding")?;
            if !ws
                .decide_duplicate_at_revision(&a, &b, same, revision)
                .map_err(|e| e.to_string())?
            {
                return Err(
                    "Duplicate pair changed or was already decided; refresh and review it again"
                        .into(),
                );
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
                || custom_choice_action::busy()
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
                    let current = result != application_check::STALE_CHECK
                        && job.as_ref().is_some_and(|job| {
                            job.canonical_url.as_deref() == Some(checked_url.as_str())
                        });
                    let message = if current {
                        result
                    } else {
                        application_check::invalidate(id)?;
                        application_check::STALE_CHECK.into()
                    };
                    if review && job.is_some() {
                        ws.mark_manual_review(&job_id, &message, now())
                            .map_err(|e| e.to_string())?;
                    }
                    let contacts = if current {
                        application_check::contacts(id)?
                    } else {
                        Vec::new()
                    };
                    application_check::acknowledge(id)?;
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"complete","result":message,"checked_url":checked_url,"current":current,"contacts":contacts}),
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
            check_search_plan_binding(ws, &body)?;
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
            // Make the plan recoverable before acknowledging the start. Search
            // can continue in memory if secure persistence is unavailable, but
            // the client must be told that restart recovery is not yet assured.
            let checkpoint_saved = matches!(web_search_batch::persist_progress(ws), Ok(true));
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":id,"checkpoint_saved":checkpoint_saved}),
            );
        }
        "/api/web-search/batch-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let acknowledged = body["acknowledged"].as_u64().unwrap_or(0);
            let acknowledged =
                usize::try_from(acknowledged).map_err(|_| "Invalid batch acknowledgement")?;
            let mut result = web_search_batch::poll(id, acknowledged)?;
            if matches!(result["status"].as_str(), Some("complete" | "failed")) {
                let _ = web_search_batch::persist_completed(ws);
                result = web_search_batch::poll(id, acknowledged)?;
            } else if result["status"] == "running" && web_search_batch::checkpoint_failed(id)? {
                result["checkpoint_failed"] = json!(true);
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/web-search/batch-pending" => {
            json_reply(stream, "200 OK", web_search_batch::pending_with_saved(ws)?);
        }
        "/api/web-search/selection" => match body["action"].as_str() {
            Some("load") => {
                let (revision, items) = candidate_selection::load(ws)?;
                json_reply(stream, "200 OK", json!({"revision":revision,"items":items}));
            }
            Some("save") => {
                let revision = body["revision"]
                    .as_u64()
                    .ok_or("Missing selection revision")?;
                let revision = candidate_selection::save(ws, revision, &body["items"])?;
                json_reply(stream, "200 OK", json!({"revision":revision}));
            }
            _ => return Err("Invalid selection action".into()),
        },
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
                || custom_choice_action::busy()
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
            let context = candidate_selection::search_context(&body["search_context"])?;
            candidate_task::start(id.clone(), parsed.to_string(), context)?;
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
            let description = ws.job_description(&id).map_err(|e| e.to_string())?;
            let metadata = match ws.job_source_only(&id).map_err(|e| e.to_string())? {
                Some(source) => Some(source),
                None => ws.job_capture_metadata(&id).map_err(|e| e.to_string())?,
            };
            let (
                evidence,
                captured_at,
                matches_current_url,
                provenance,
                email_links,
                mut search_context,
                mut search_context_status,
                mut search_context_matches_url,
            ) = if let Some(metadata) = metadata {
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
                let current = json!({"title":job.title,"employer":job.employer,"location":job.location,"description":description,"requisition_id":job.requisition_id,"remote":job.remote});
                let provenance = candidate_task::extraction_provenance(&capture, &current);
                let email_links = candidate_task::captured_email_links(&capture);
                let search_context =
                    candidate_selection::search_context(&capture["search_context"])?;
                let search_context_matches_url = search_context.as_ref().and_then(|_| {
                    capture["requested_url"]
                        .as_str()
                        .filter(|url| !url.is_empty())
                        .zip(job.canonical_url.as_deref().filter(|url| !url.is_empty()))
                        .map(|(source, current)| source == current)
                });
                let search_context_status = capture["search_context_status"]
                    .as_str()
                    .filter(|value| *value == "omitted_size_limit")
                    .map(str::to_owned);
                (
                    evidence,
                    captured_at,
                    matches_current_url,
                    provenance,
                    email_links,
                    search_context,
                    search_context_status,
                    search_context_matches_url,
                )
            } else {
                (None, None, None, None, None, None, None, None)
            };
            if search_context.is_none() {
                if let Some(metadata) = ws.job_capture_metadata(&id).map_err(|e| e.to_string())? {
                    let capture: Value = serde_json::from_slice(&metadata)
                        .map_err(|_| "Saved listing metadata is invalid")?;
                    if let Some(context) =
                        candidate_selection::search_context(&capture["search_context"])?
                    {
                        search_context_matches_url = capture["requested_url"]
                            .as_str()
                            .filter(|url| !url.is_empty())
                            .zip(job.canonical_url.as_deref().filter(|url| !url.is_empty()))
                            .map(|(source, current)| source == current);
                        search_context = Some(context);
                        search_context_status = None;
                    }
                }
            }
            let email_links =
                candidate_task::listing_email_contacts(email_links, description.as_deref());
            json_reply(
                stream,
                "200 OK",
                json!({"salary_evidence":evidence.as_ref().map(|v| &v["salary_evidence"]),"posting_date_evidence":evidence.as_ref().map(|v| &v["posting_date_evidence"]),"valid_through_evidence":evidence.as_ref().map(|v| &v["valid_through_evidence"]),"captured_at":captured_at,"matches_current_url":matches_current_url,"extraction_provenance":provenance,"email_links":email_links,"search_context":search_context,"search_context_status":search_context_status,"search_context_matches_url":search_context_matches_url}),
            );
        }
        "/api/listing-contacts/pending" => {
            json_reply(stream, "200 OK", contact_advice::pending()?);
        }
        "/api/listing-contacts/start" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            let (contacts, digest) = contact_advice::inputs(ws, &job)?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task = random_id()?;
            contact_advice::start(task.clone(), job.as_str().into(), digest, model, contacts)?;
            json_reply(stream, "200 OK", json!({"task_id":task}));
        }
        "/api/listing-contacts/status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", contact_advice::poll(id, ws)?);
        }
        "/api/listing-inspect/discard" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            candidate_task::discard(id)?;
            json_reply(stream, "200 OK", json!({"discarded":true}));
        }
        "/api/listing-inspect/apply-description" => {
            if session.is_some()
                || form_task::busy()
                || application_check::busy()
                || candidate_task::busy()
                || form_links::busy()
                || custom_choice_action::busy()
                || email_worker::busy()
            {
                return Err(
                    "Finish the active application or inspection before updating this listing"
                        .into(),
                );
            }
            if body["reviewed"] != true {
                return Err("Review both descriptions before applying this update".into());
            }
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            let result = candidate_task::completed(task_id)?
                .ok_or("Inspection is no longer available; inspect the listing again")?;
            if result["status"] != "complete"
                || result["saved"]["existing"] != true
                || result["previous_description"]["truncated"] != false
            {
                return Err("This inspection cannot be applied; review the listing again".into());
            }
            let id = JobIdentityId::new(result["saved"]["id"].as_str().ok_or("missing saved id")?)
                .map_err(|e| e.to_string())?;
            let job = ws
                .job(&id)
                .map_err(|e| e.to_string())?
                .ok_or("Listing no longer exists")?;
            let job_digest =
                waypoint_domain::sha256_hex(&serde_json::to_vec(&job).map_err(|e| e.to_string())?);
            if result["previous_description"]["job_digest"].as_str() != Some(job_digest.as_str()) {
                return Err("The saved listing changed since inspection; inspect it again".into());
            }
            let current = ws.job_description(&id).map_err(|e| e.to_string())?;
            let description_digest = waypoint_domain::sha256_hex(
                &serde_json::to_vec(&current).map_err(|e| e.to_string())?,
            );
            if result["previous_description"]["digest"].as_str()
                != Some(description_digest.as_str())
            {
                return Err(
                    "The saved description changed since inspection; inspect it again".into(),
                );
            }
            let replacement = match body["source"].as_str().unwrap_or("extracted") {
                "extracted" => result["job"]["description"].as_str(),
                "observed_section" if result["job"]["observed_section"]["truncated"] == false => {
                    result["job"]["observed_section"]["text"].as_str()
                }
                _ => return Err("The selected source text is unavailable or incomplete".into()),
            }
            .map(str::trim)
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 30_000
                    && !s
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            })
            .ok_or("No usable inspected description is available")?;
            match ws
                .replace_reviewed_description(
                    &id,
                    current.as_deref(),
                    replacement,
                    &now().to_string(),
                )
                .map_err(|e| e.to_string())?
            {
                waypoint_store::ReviewedDescriptionResult::Updated => {
                    json_reply(stream, "200 OK", json!({"updated":true}))
                }
                waypoint_store::ReviewedDescriptionResult::Unchanged => {
                    return Err("Description has not changed".into())
                }
                waypoint_store::ReviewedDescriptionResult::Stale => {
                    return Err(
                        "The saved description changed since inspection; inspect it again".into(),
                    )
                }
                waypoint_store::ReviewedDescriptionResult::NotFound => {
                    return Err("Listing no longer exists".into())
                }
                waypoint_store::ReviewedDescriptionResult::ApplicationInProgressOrComplete => {
                    return Err(
                        "Descriptions cannot be replaced after application submission begins"
                            .into(),
                    )
                }
            }
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
                    let previous_description = if result["existing"] == true {
                        let existing = ws.job_description(&job_id).map_err(|e| e.to_string())?;
                        let stored_job = ws
                            .job(&job_id)
                            .map_err(|e| e.to_string())?
                            .ok_or("Listing no longer exists")?;
                        let digest = waypoint_domain::sha256_hex(
                            &serde_json::to_vec(&existing).map_err(|e| e.to_string())?,
                        );
                        let job_digest = waypoint_domain::sha256_hex(
                            &serde_json::to_vec(&stored_job).map_err(|e| e.to_string())?,
                        );
                        let text = existing.as_deref().unwrap_or_default();
                        let end = text.floor_char_boundary(text.len().min(30_000));
                        Some(
                            json!({"text":&text[..end],"truncated":end < text.len(),"digest":digest,"job_digest":job_digest}),
                        )
                    } else {
                        None
                    };
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
                                "Review locally extracted listing: the page may contain multiple roles, page text was truncated, and the application route is unverified"
                            } else {
                                "Review locally extracted listing: the page may contain multiple roles and the application route is unverified"
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
                    let search_context_supplied =
                        inspected.capture.metadata.get("search_context").is_some()
                            || inspected.capture.metadata["search_context_status"]
                                == "omitted_size_limit";
                    let search_context_saved = search_context_supplied.then(|| {
                        inspected.capture.metadata.get("search_context").is_some()
                            && (capture["source_saved"] == true
                                || (capture["saved"] == true
                                    && capture["preserved_existing"] != true))
                    });
                    let role_suggestions_saved = job["other_roles_status"].is_string().then(|| {
                        inspected.capture.metadata["extraction_provenance"]["other_roles_status"]
                            .is_string()
                            && (capture["source_saved"] == true
                                || (capture["saved"] == true
                                    && capture["preserved_existing"] != true
                                    && matches!(ws.job_source_only(&job_id), Ok(None))))
                    });
                    let response = json!({"status":"complete","job":job,"saved":result,"capture":capture,"search_context_saved":search_context_saved,"role_suggestions_saved":role_suggestions_saved,"previous_description":previous_description});
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
            let model_status = resident_model_status()?;
            let model = model_status["model"].as_str().map(str::to_owned).ok_or(
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
                fit_task::FitModel {
                    name: model,
                    digest: model_status["model_digest"].as_str().map(str::to_owned),
                    context_length: model_status["context_length"].as_u64(),
                },
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
                    result["assessment"]["preference_coverage"] =
                        json!(fit_task::preference_coverage(&result["assessment"]));
                    result["assessment"]["requirement_coverage"] =
                        json!(fit_task::requirement_coverage(&result["assessment"]));
                    result["assessment"]["requirement_audit_incomplete"] = json!(
                        fit_task::requirement_audit_incomplete(&result["assessment"])
                    );
                    if let Some(previous) = ws.fit_assessment(&job_id).map_err(|e| e.to_string())? {
                        let previous: Value = serde_json::from_slice(&previous).map_err(|_| {
                            "Previous saved assessment is invalid; it was not replaced"
                        })?;
                        if let Some(comparison) =
                            fit_task::repeatability_comparison(&previous, &result)
                        {
                            result["assessment"]["repeatability"] = comparison;
                        }
                        let (history, omitted) = fit_task::review_history(&previous)?;
                        result["review_history"] = json!(history);
                        result["review_history_omitted"] = json!(omitted);
                    }
                    ws.save_fit_assessment(&job_id, result.to_string().as_bytes())
                        .map_err(|e| format!("Could not save assessment; retry after resolving the workspace error: {e}"))?;
                    result["saved"] = json!(true);
                }
            }
            if result["status"] == "complete" {
                let job_id = JobIdentityId::new(result["job_id"].as_str().ok_or("missing job")?)
                    .map_err(|e| e.to_string())?;
                let bytes = ws
                    .fit_assessment(&job_id)
                    .map_err(|e| e.to_string())?
                    .ok_or("Saved assessment is unavailable")?;
                let saved: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "Stored assessment is invalid")?;
                if saved["assessment"] != result["assessment"]
                    || saved["input_digest"] != result["input_digest"]
                {
                    result = json!({"status":"failed","error":"The saved assessment changed; reopen the current assessment"});
                } else {
                    result["user_review"] = saved["user_review"].clone();
                    result["review_history"] = saved["review_history"].clone();
                    result["review_history_omitted"] = saved["review_history_omitted"].clone();
                    result["review_revision"] = json!(fit_review_revision(&saved));
                }
            }
            if result["status"] != "running" {
                fit_task::complete(id, result.clone())?;
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/fit/review" => {
            if body["reviewed"] != true {
                return Err("Confirm that you reviewed the criteria".into());
            }
            let job_id = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let bytes = ws
                .fit_assessment(&job_id)
                .map_err(|e| e.to_string())?
                .ok_or("Saved assessment is unavailable")?;
            let mut saved: Value =
                serde_json::from_slice(&bytes).map_err(|_| "Stored assessment is invalid")?;
            let (_, _, _, _, digest) = fit_assessment_inputs(ws, &job_id)?;
            if saved["input_digest"].as_str() != Some(digest.as_str()) {
                return Err("The assessment is stale; assess the current job again".into());
            }
            if body["revision"].as_str() != Some(fit_review_revision(&saved).as_str()) {
                return Err("The assessment or review changed; reload it before saving".into());
            }
            let mut review = fit_task::reviewed_rubric(&saved["assessment"], &body["decisions"])?;
            review["reviewed_at"] = json!(now());
            saved["user_review"] = review.clone();
            ws.save_fit_assessment(&job_id, saved.to_string().as_bytes())
                .map_err(|e| e.to_string())?;
            json_reply(
                stream,
                "200 OK",
                json!({"user_review":review,"review_revision":fit_review_revision(&saved)}),
            );
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
                        json!({"id":id.as_str(),"status":"current","score":value["assessment"]["score"],"score_range":value["assessment"]["score_range"],"preference_coverage":value["assessment"]["preference_coverage"],"requirement_coverage":value["assessment"]["requirement_coverage"],"requirement_audit_incomplete":value["assessment"]["requirement_audit_incomplete"],"review_score":value["user_review"]["score"],"review_range":value["user_review"]["score_range"]})
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
            let mut result = search_planner::poll(id)?;
            if result["status"] == "complete" {
                let (facts, instructions, digest, _) = search_plan_context(ws)?;
                if result["input_digest"].as_str() != Some(digest.as_str()) {
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"stale","error":"Profile, qualifications or search instructions changed; generate new phrases."}),
                    );
                    return Ok(());
                }
                result["source_coverage"] = search_plan_coverage(ws, &facts, &instructions)?;
            }
            json_reply(stream, "200 OK", result);
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
        "/api/discover/pending" => {
            json_reply(stream, "200 OK", discovery_task::pending()?);
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
            campaign_constraints(&Value::Object(profile.clone()))?;
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
            if linked
                .values()
                .any(|(_, review)| document_visual::needs_followup(review))
            {
                record.status("manual_review", now())?;
                ws.mark_manual_review(
                    &id,
                    "Email attachment visual review has findings or incomplete checks",
                    now(),
                )
                .map_err(|e| e.to_string())?;
                record.event(json!({"type":"document_visual_followup_needed","time":now(),"note":"A linked review contains findings or unavailable checks; inspect the PDF."}))?;
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
        "/api/documents/transcribe-start" => {
            let id = body["id"].as_str().ok_or("missing document id")?;
            let page = body["page"]
                .as_u64()
                .filter(|page| *page < 30)
                .ok_or("invalid page number")? as usize;
            let region = pdf_transcribe::requested_region(body.get("region"))?;
            let info = ws
                .documents()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|doc| doc.id == id)
                .ok_or("Document not found")?;
            if !info.name.to_ascii_lowercase().ends_with(".pdf") {
                return Err("Choose a PDF".into());
            }
            let bytes = ws
                .document_content(id)
                .map_err(|e| e.to_string())?
                .ok_or("Document not found")?;
            let model = resident_model()?
                .ok_or("No single fully GPU-resident local vision model is available")?;
            let task_id = random_id()?;
            pdf_transcribe::start(
                task_id.clone(),
                id.to_owned(),
                model,
                bytes,
                page,
                region.to_owned(),
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/transcribe-pending" => {
            json_reply(stream, "200 OK", pdf_transcribe::pending()?);
        }
        "/api/documents/transcribe-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            json_reply(stream, "200 OK", pdf_transcribe::poll(id)?);
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
                &context.digest,
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
                id.to_owned(),
                job.as_str().to_owned(),
                info.name,
                bytes,
                format!("{}\n{}", job_data.title, description),
                facts,
            )?;
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/documents/ats-check-active" => {
            json_reply(stream, "200 OK", json!({"active":ats_check::active()?}));
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
            let (text, coverage) = document_extract::extract_for_review(&info.name, &bytes)?;
            json_reply(
                stream,
                "200 OK",
                json!({"id":id,"name":info.name,"text":text,"coverage":coverage}),
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
                json!({"total":total,"pending":pending,"bounded":bounded,"missing_employer":missing_employer,"index_pending":ws.duplicate_index_pending().map_err(|e| e.to_string())?}),
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
        "/api/attempts/answers" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            ws.job(&job)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
            let after = match body.get("after") {
                None | Some(Value::Null) => 0,
                Some(value) => value
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("Invalid answer history cursor")?,
            };
            json_reply(
                stream,
                "200 OK",
                attempt_record::answer_history(ws, &job, attempt, after)?,
            );
        }
        "/api/attempts/screenshots" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            ws.job(&job)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
            let after = match body.get("after") {
                None | Some(Value::Null) => 0,
                Some(value) => value
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("Invalid screenshot cursor")?,
            };
            json_reply(
                stream,
                "200 OK",
                attempt_record::screenshot_index(ws, &job, attempt, after)?,
            );
        }
        "/api/attempts/screenshot" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            ws.job(&job)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
            let name = body["name"].as_str().ok_or("missing screenshot name")?;
            json_reply(
                stream,
                "200 OK",
                attempt_record::screenshot_image(ws, &job, attempt, name)?,
            );
        }
        "/api/attempts/visual-result" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing job id")?)
                .map_err(|e| e.to_string())?;
            ws.job(&job)
                .map_err(|e| e.to_string())?
                .ok_or("Job not found")?;
            let attempt = body["attempt_id"].as_str().ok_or("missing attempt id")?;
            let task = body["task_id"].as_str().ok_or("missing visual review id")?;
            json_reply(
                stream,
                "200 OK",
                attempt_record::saved_visual_result(ws, &job, attempt, task)?,
            );
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
                if !active_form_review_reasons(&id, session)?.is_empty() {
                    return Err("Finish or close the active application review before clearing all review reasons".into());
                }
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
            if active_form_review_reasons(&id, session)?
                .iter()
                .any(|active| active == reason)
            {
                return Err("Finish or close the active application review before clearing its pending reason".into());
            }
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
                || custom_choice_action::busy()
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
            let model = form_resident_model();
            let experiences = if matches!(&model, Ok(Some(_))) {
                use waypoint_workspace::form_experience::{Control, Stage};
                let platform_hint = experience_platform_hint(&job.source);
                let mut items = ws
                    .related_form_experiences(Stage::EnterInformation, Control::Text, platform_hint)
                    .map_err(|e| e.to_string())?;
                items.extend(
                    ws.related_form_experiences(
                        Stage::EnterInformation,
                        Control::Choice,
                        platform_hint,
                    )
                    .map_err(|e| e.to_string())?,
                );
                items
            } else {
                Vec::new()
            };
            let profile: Value = match ws.candidate_profile().map_err(|e| e.to_string())? {
                Some(text) => {
                    serde_json::from_str(&text).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            interrupt_session(ws, session, "Another application was opened")?;
            let task_id = random_id()?;
            let pending_revision = form_task::begin_pending(ws, &id, &task_id, now())?;
            if let Err(error) = form_task::start(
                task_id.clone(),
                id.clone(),
                url,
                profile,
                model,
                visible,
                (pending_revision, experiences),
            ) {
                if let Err(cleanup) = form_task::finish_pending(ws, &id, &task_id, pending_revision)
                {
                    return Err(format!("{error}; {cleanup}"));
                }
                return Err(error);
            }
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
                pending_revision,
                url,
                profile_digest,
                visible,
                experience_revisions,
                result,
            } = finished;
            let (mut driver, mut preview) = match result {
                Ok(value) => value,
                Err(error) => {
                    ws.mark_manual_review(&job_id, &inspection_failure_reason(&error), now())
                        .map_err(|e| e.to_string())?;
                    form_task::finish_pending(ws, &job_id, task_id, pending_revision)?;
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
            if !experience_revisions.is_empty() {
                let current = ws.form_experiences().map_err(|e| e.to_string())?;
                if !experience_snapshot_current(&experience_revisions, &current) {
                    preview.suggestions.clear();
                    preview.manual_review.push("A lesson changed during inspection; suggestions were discarded. Inspect again.".into());
                }
            }
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
                    &preview.fields_for_review(),
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
                json!({"status":"complete","fields":preview.fields_for_review(),"model_review_selectors":preview.model_review_selectors,"repeated_label_selectors":preview.repeated_label_selectors(),"suggestions":preview.suggestions,"manual_review":preview.manual_review,"session_id":session_id,"origin":preview.schema.origin,"expires_in_seconds":ttl.as_secs(),"visible":visible,"fill_attempted":false,"screenshot":screenshot,"links":links,"other_links":other_links,"other_links_available":other_links_available}),
            );
            *session = Some(ReviewSession {
                id: session_id,
                job_id,
                inspection_task_id: task_id.to_owned(),
                inspection_pending_revision: pending_revision,
                url,
                profile_digest,
                preview,
                driver,
                created: Instant::now(),
                ttl,
                filled: false,
                fill_attempted: false,
                completed_selectors: vec![],
                answer_drafts: Default::default(),
                visible,
                staged_files: vec![],
                attachments: vec![],
                pending_choice_visual: None,
                selected_choices: vec![],
                last_visual_freshness_key: None,
                record,
            });
        }
        "/api/form/custom-choices" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            let (choices, digest) = custom_choice_snapshot(&mut current.driver)?;
            let can_open = check_answer_page(current).is_ok();
            let pending_selected =
                current
                    .pending_choice_visual
                    .as_ref()
                    .and_then(|(selector, _, _, option)| {
                        option
                            .as_ref()
                            .map(|option| json!({"selector":selector,"option":option}))
                    });
            json_reply(
                stream,
                "200 OK",
                json!({"choices":choices.choices,"incomplete":choices.incomplete,"snapshot_digest":digest,"can_open":can_open,"pending_selected":pending_selected}),
            );
        }
        "/api/form/custom-choice-advice-pending" => {
            json_reply(stream, "200 OK", choice_advice::pending()?);
        }
        "/api/form/custom-choice-advice-start" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"]
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 4096)
                .ok_or("Invalid choice selector")?;
            let answer = body["answer"]
                .as_str()
                .filter(|value| !value.trim().is_empty() && value.len() <= 600)
                .ok_or("Provide an answer of at most 600 bytes")?;
            let expected = body["snapshot_digest"]
                .as_str()
                .filter(|value| value.len() == 64)
                .ok_or("Missing choice snapshot")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            let (inventory, digest) = custom_choice_snapshot(&mut current.driver)?;
            if digest != expected {
                return Err("The page changed; inspect choices again".into());
            }
            let matches: Vec<_> = inventory
                .choices
                .iter()
                .filter(|choice| choice.selector == selector)
                .collect();
            if matches.len() != 1
                || !matches[0].expanded
                || matches[0].label == "Unlabelled choice"
                || matches[0].options.is_empty()
            {
                return Err("Open an unambiguous, labelled choice before requesting advice".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            choice_advice::start(
                task_id.clone(),
                current.id.clone(),
                digest,
                model,
                matches[0].label.clone(),
                answer.to_owned(),
                matches[0].options.clone(),
            )?;
            current.created = Instant::now();
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/custom-choice-advice-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = choice_advice::poll(id)?;
            if matches!(result["status"].as_str(), Some("running" | "complete")) {
                if let Some(current) = session.as_mut().filter(|s| {
                    Some(s.id.as_str()) == result["session_id"].as_str()
                        && s.created.elapsed() <= s.ttl
                }) {
                    current.created = Instant::now();
                    if result["status"] == "complete"
                        && custom_choice_snapshot(&mut current.driver)
                            .ok()
                            .map(|(_, digest)| digest)
                            != result["snapshot_digest"].as_str().map(str::to_owned)
                    {
                        result = json!({"status":"failed","session_id":current.id,"error":"The page changed; inspect the choice again"});
                    }
                } else if result["status"] == "complete" {
                    result = json!({"status":"failed","session_id":result["session_id"],"error":"The review session ended or changed"});
                }
            }
            if result["status"] != "running" {
                choice_advice::complete(id, result.clone())?;
            }
            if let Some(object) = result.as_object_mut() {
                object.remove("snapshot_digest");
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/form/custom-choice-vision-start"
        | "/api/form/custom-choice-option-vision-start"
        | "/api/form/custom-choice-selected-vision-start" => {
            let option_review = path == "/api/form/custom-choice-option-vision-start";
            let selected_review = path == "/api/form/custom-choice-selected-vision-start";
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"]
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 4096)
                .ok_or("Invalid choice selector")?;
            let expected = body["snapshot_digest"]
                .as_str()
                .filter(|value| value.len() == 64)
                .ok_or("Missing choice snapshot")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            let (inventory, digest) = custom_choice_snapshot(&mut current.driver)?;
            if digest != expected {
                return Err("The page changed; inspect choices again".into());
            }
            let matches: Vec<_> = inventory
                .choices
                .iter()
                .filter(|choice| choice.selector == selector)
                .collect();
            if matches.len() != 1 {
                return Err("The choice is unavailable or ambiguous".into());
            }
            let label = if selected_review {
                current
                    .pending_choice_visual
                    .as_ref()
                    .filter(|pending| pending.0 == selector)
                    .and_then(|pending| pending.3.clone())
                    .ok_or("No selected value is awaiting visual review")?
            } else if option_review {
                let option = body["option"]
                    .as_str()
                    .filter(|value| {
                        !value.is_empty() && value.len() <= 200 && *value == value.trim()
                    })
                    .ok_or("Invalid choice option")?;
                if !matches[0].expanded
                    || matches[0]
                        .options
                        .iter()
                        .filter(|value| value.as_str() == option)
                        .count()
                        != 1
                {
                    return Err("Choice option is unavailable or ambiguous; inspect again".into());
                }
                option.to_owned()
            } else {
                matches[0].label.clone()
            };
            let normalized = |value: &str| {
                value
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            };
            let ambiguous = if selected_review {
                false
            } else if option_review {
                matches[0]
                    .options
                    .iter()
                    .filter(|option| normalized(option) == normalized(&label))
                    .count()
                    != 1
            } else {
                label == "Unlabelled choice"
                    || inventory
                        .choices
                        .iter()
                        .filter(|choice| normalized(&choice.label) == normalized(&label))
                        .count()
                        != 1
            };
            let image = current
                .driver
                .with_review_timeout(|driver| {
                    if selected_review {
                        driver.screenshot_control(selector)
                    } else if option_review {
                        driver.screenshot_custom_choice_option(selector, &label)
                    } else {
                        driver.screenshot_field_context(selector)
                    }
                })
                .map_err(|error| format!("Could not capture the choice: {error}"))?;
            if custom_choice_snapshot(&mut current.driver)?.1 != digest {
                return Err("The page changed during capture; inspect choices again".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            let capture_saved = current.record.as_mut().is_some_and(|record| record.screenshot_with_name(&image).and_then(|name| record.event(json!({"type":"custom_choice_visual_capture","time":now(),"task_id":task_id,"screenshot":format!("screenshots/{name}"),"note":"Advisory choice crop; no control was clicked."}))).is_ok());
            let revision = control_vision::begin_pending(ws, &current.job_id, &task_id, now())?;
            if let Err(error) = control_vision::start(control_vision::Request {
                id: task_id.clone(),
                session: current.id.clone(),
                rank_task: if selected_review {
                    "custom-choice-selected"
                } else if option_review {
                    "custom-choice-option"
                } else {
                    "custom-choice"
                }
                .into(),
                control_index: 0,
                job_id: current.job_id.as_str().into(),
                pending_revision: revision,
                model,
                selector: selector.into(),
                digest,
                image: image.clone(),
                label,
                ambiguous,
                capture_saved,
                choice: true,
            }) {
                control_vision::finish_pending(ws, &current.job_id, &task_id, revision)?;
                return Err(error);
            }
            current.created = Instant::now();
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":task_id,"screenshot":image,"capture_saved":capture_saved}),
            );
        }
        "/api/form/custom-choice-open"
        | "/api/form/custom-choice-select"
        | "/api/form/custom-choice-scroll" => {
            let selecting = path == "/api/form/custom-choice-select";
            let scrolling = path == "/api/form/custom-choice-scroll";
            let scroll = if scrolling {
                Some(match body["direction"].as_str() {
                    Some("down") => true,
                    Some("up") => false,
                    _ => return Err("Choose a valid scroll direction".into()),
                })
            } else {
                None
            };
            if body["reviewed"] != true || body["source_reviewed"] != true {
                return Err("Review the destination and choice before interacting".into());
            }
            if custom_choice_action::busy() {
                return Err("A choice action is already running".into());
            }
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let task_id = body["visual_task_id"]
                .as_str()
                .ok_or("missing visual review")?;
            let evidence = control_vision::poll(task_id)?;
            if evidence["status"] != "complete"
                || evidence["choice"] != true
                || evidence["rank_task_id"]
                    != if selecting {
                        "custom-choice-option"
                    } else {
                        "custom-choice"
                    }
                || evidence["comparison"] != "label_observed"
                || evidence["session_id"].as_str() != Some(id)
                || evidence["capture_saved"] != true
                || evidence["result_saved"] != true
                || evidence["pending_finished"] != true
                || evidence["action_consumed"] == true
            {
                return Err("A saved, current choice visual review is required".into());
            }
            let selector = evidence["selector"]
                .as_str()
                .ok_or("Choice selector unavailable")?
                .to_owned();
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            if ws
                .has_duplicate_hold(&current.job_id)
                .map_err(|error| error.to_string())?
            {
                return Err(
                    "Review possible duplicate jobs before interacting with this application"
                        .into(),
                );
            }
            require_duplicate_index_ready(ws)?;
            let job = ws
                .job(&current.job_id)
                .map_err(|error| error.to_string())?
                .ok_or("Job not found")?;
            if !matches!(
                job.state,
                ApplicationState::Shortlisted | ApplicationState::Prepared
            ) || job.canonical_url.as_deref() != Some(current.url.as_str())
            {
                return Err("Job or destination changed; inspect again".into());
            }
            let profile: Value = match ws.candidate_profile().map_err(|error| error.to_string())? {
                Some(profile) => {
                    serde_json::from_str(&profile).map_err(|_| "stored profile is invalid")?
                }
                None => json!({}),
            };
            if Sha256Digest::of(&profile.to_string()) != current.profile_digest {
                return Err("Profile changed; inspect the current page again".into());
            }
            check_answer_page(current)?;
            if let Some(first) = current.completed_selectors.first().cloned() {
                let check = current.driver.with_review_timeout(|driver| {
                    form_preview::verify_completed_fields(
                        driver,
                        &current.preview,
                        &current.completed_selectors,
                        &first,
                    )
                });
                if !check.selected_retained
                    || !check.failed_labels.is_empty()
                    || check.unverified != 0
                {
                    if let Some(detail) = check.failed_details.first() {
                        return Err(format!("A previously filled field needs review: {detail}"));
                    }
                    return Err(
                        "Previously filled fields changed or could not be checked; inspect again"
                            .into(),
                    );
                }
            }
            let live = current
                .driver
                .inspect_current()
                .map_err(|error| error.to_string())?;
            form_links::check_job_and_destination(
                &job.source,
                &live.origin,
                waypoint_policy::Operation::Prefill,
            )?;
            let (choices, before) = custom_choice_snapshot(&mut current.driver)?;
            let matches: Vec<_> = choices
                .choices
                .iter()
                .filter(|choice| choice.selector == selector)
                .collect();
            if matches.len() != 1 {
                return Err("Choice changed; inspect again".into());
            }
            let option = if selecting {
                let value = evidence["label"]
                    .as_str()
                    .ok_or("Option evidence missing")?;
                if current.selected_choices.len() >= 30
                    && !current
                        .selected_choices
                        .iter()
                        .any(|(previous, _)| previous == &selector)
                {
                    return Err("This section has reached the limit for tracked custom choices; inspect it manually".into());
                }
                if !matches[0].expanded
                    || matches[0]
                        .options
                        .iter()
                        .filter(|option| option.as_str() == value)
                        .count()
                        != 1
                {
                    return Err("Choice option changed; inspect again".into());
                }
                Some(value.to_owned())
            } else {
                if matches[0].expanded != scrolling
                    || evidence["label"].as_str() != Some(matches[0].label.as_str())
                {
                    return Err("Choice changed or is already open; inspect again".into());
                }
                None
            };
            let image = current
                .driver
                .with_review_timeout(|driver| {
                    if selecting {
                        driver.screenshot_custom_choice_option(
                            &selector,
                            option.as_deref().unwrap_or(""),
                        )
                    } else {
                        driver.screenshot_field_context(&selector)
                    }
                })
                .map_err(|error| error.to_string())?;
            let after = custom_choice_snapshot(&mut current.driver)?.1;
            if !control_vision::capture_is_current(
                evidence["snapshot_digest"].as_str(),
                Some(&before),
                Some(&after),
                evidence["image_hash"].as_str(),
                Some(&image),
            ) {
                return Err("Choice or visual evidence changed; inspect again".into());
            }
            if let Some(down) = scroll {
                if !current
                    .driver
                    .with_review_timeout(|driver| {
                        driver.custom_choice_scroll_available(&selector, down)
                    })
                    .map_err(|error| error.to_string())?
                {
                    return Err("The choice list is already at that boundary".into());
                }
            }
            let record = current.record.as_mut().ok_or(
                "A disk-backed application record is required before interacting with a choice",
            )?;
            record.event(json!({"type":"custom_choice_action_requested","time":now(),"selector":selector,"option":option,"scroll_down":scroll,"visual_task_id":task_id,"note":"Explicitly reviewed; no submission authorized."}))?;
            control_vision::consume_choice_evidence(task_id)?;
            let grant_id = random_id()?;
            let payload = json!({"action":if selecting {"select_custom_choice"} else if scrolling {"scroll_custom_choice"} else {"open_custom_choice"},"selector":selector,"option":option,"scroll_down":scroll,"snapshot_digest":before,"visual_task_id":task_id}).to_string();
            ws.record_consumed_prefill_grant(&waypoint_store::prefill_grants::PrefillGrantRecord {
                id: grant_id.clone(),
                job_id: current.job_id.as_str().into(),
                intent_id: format!("choice-{grant_id}"),
                payload_hash: Sha256Digest::of(&payload).as_str().into(),
                profile_hash: current.profile_digest.as_str().into(),
                schema_hash: Sha256Digest::of(&live.schema.schema_hash_input())
                    .as_str()
                    .into(),
                destination: live.origin,
                created_at: now(),
            })
            .map_err(|error| error.to_string())?;
            let action_id = random_id()?;
            let revision = custom_choice_action::begin_pending(ws, &current.job_id, &action_id)?;
            let current = session.take().ok_or("Form session expired")?;
            if let Err((current, error)) = custom_choice_action::start(
                action_id.clone(),
                current,
                selector,
                option,
                scroll,
                revision,
            ) {
                let cleanup =
                    custom_choice_action::finish_pending(ws, &current.job_id, &action_id, revision);
                *session = Some(*current);
                cleanup?;
                return Err(error);
            }
            json_reply(stream, "200 OK", json!({"task_id":action_id}));
        }
        "/api/form/custom-choice-action-pending" => {
            json_reply(stream, "200 OK", custom_choice_action::pending()?);
        }
        "/api/form/custom-choice-action-status" => {
            let id = body["task_id"].as_str().ok_or("missing choice task")?;
            let result = match custom_choice_action::poll(id)? {
                custom_choice_action::Poll::Running => json!({"status":"running"}),
                custom_choice_action::Poll::Cached(value) => value,
                custom_choice_action::Poll::Lost(job, revision) => {
                    ws.mark_manual_review(&job, "Custom choice action outcome is uncertain", now())
                        .map_err(|error| error.to_string())?;
                    custom_choice_action::finish_pending(ws, &job, id, revision)?;
                    let value = json!({"status":"uncertain","error":"The review session ended; inspect the application manually"});
                    custom_choice_action::remember(id, value.clone())?;
                    value
                }
                custom_choice_action::Poll::Complete(returned, mut value, revision) => {
                    let job = returned.job_id.clone();
                    if session.is_some() {
                        ws.mark_manual_review(
                            &job,
                            "Custom choice session was replaced; inspect manually",
                            now(),
                        )
                        .map_err(|error| error.to_string())?;
                        value = json!({"status":"uncertain","error":"The review session changed; inspect the application manually"});
                        custom_choice_action::finish_pending(ws, &job, id, revision)?;
                    } else {
                        *session = Some(*returned);
                        if value["status"] != "complete"
                            || value["record_saved"] != true
                            || value["previous_fields_verified"] != true
                        {
                            ws.mark_manual_review(
                                &job,
                                "Custom choice action needs manual review",
                                now(),
                            )
                            .map_err(|error| error.to_string())?;
                        }
                    }
                    custom_choice_action::remember(id, value.clone())?;
                    value
                }
            };
            json_reply(stream, "200 OK", result);
        }
        "/api/form/control-rank-pending" => {
            json_reply(stream, "200 OK", control_rank::pending()?);
        }
        "/api/form/control-rank-start" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            let (controls, digest, context) =
                control_rank::snapshot_with_context(&mut current.driver)?;
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            control_rank::start(
                task_id.clone(),
                current.id.clone(),
                model,
                controls,
                digest,
                context,
            )?;
            current.created = Instant::now();
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/control-rank-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = control_rank::poll(id)?;
            if matches!(result["status"].as_str(), Some("running" | "complete")) {
                let current = session.as_mut().filter(|s| {
                    Some(s.id.as_str()) == result["session_id"].as_str()
                        && s.created.elapsed() <= s.ttl
                });
                if let Some(current) = current {
                    current.created = Instant::now();
                    if result["status"] == "complete" {
                        let fresh = control_rank::snapshot(&mut current.driver);
                        if fresh.as_ref().ok().map(|(_, digest)| digest.as_str())
                            != result["snapshot_digest"].as_str()
                        {
                            let reason = "Application page changed during control advice";
                            ws.mark_manual_review(&current.job_id, reason, now())
                                .map_err(|e| e.to_string())?;
                            if !current
                                .preview
                                .manual_review
                                .iter()
                                .any(|item| item == reason)
                            {
                                current.preview.manual_review.push(reason.into());
                            }
                            let record_failed = current.record.as_mut().is_some_and(|record| record.event(json!({"type":"control_advice_stale","time":now(),"task_id":id,"note":"The page changed or could not be rechecked; no advice was returned."})).is_err());
                            let error = if record_failed {
                                "The page changed or could not be rechecked; the attempt record could not be updated"
                            } else {
                                "The page changed or could not be rechecked; inspect it again"
                            };
                            result =
                                json!({"status":"failed","session_id":current.id,"error":error});
                        }
                    }
                } else if result["status"] == "complete" {
                    result = json!({"status":"failed","session_id":result["session_id"],"error":"The review session ended or changed; inspect the page again"});
                } else {
                    result["session_unavailable"] = json!(true);
                }
            }
            if result["status"] != "running" {
                control_rank::complete(id, result.clone())?;
            }
            if let Some(object) = result.as_object_mut() {
                object.remove("snapshot_digest");
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/form/control-focus" => {
            let task_id = body["task_id"].as_str().ok_or("missing task id")?;
            let index = body["index"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value < 30)
                .ok_or("Invalid control index")?;
            let advice = control_rank::poll(task_id)?;
            if advice["status"] != "complete" {
                return Err("Control advice is not available; inspect the page again".into());
            }
            let current = session
                .as_mut()
                .filter(|s| {
                    Some(s.id.as_str()) == advice["session_id"].as_str()
                        && s.created.elapsed() <= s.ttl
                })
                .ok_or("Form session expired")?;
            let selector = advice["controls"]["controls"][index]["selector"]
                .as_str()
                .filter(|selector| !selector.is_empty() && selector.len() <= 4096)
                .ok_or("Control is unavailable; inspect again")?
                .to_owned();
            let digest = advice["snapshot_digest"]
                .as_str()
                .ok_or("Control snapshot is unavailable")?;
            let current_snapshot = control_rank::snapshot(&mut current.driver);
            if current_snapshot
                .as_ref()
                .ok()
                .map(|(_, value)| value.as_str())
                != Some(digest)
            {
                ws.mark_manual_review(
                    &current.job_id,
                    "Application page changed before locating a control",
                    now(),
                )
                .map_err(|e| e.to_string())?;
                current
                    .preview
                    .manual_review
                    .push("Application page changed before locating a control".into());
                return Err("The page changed or could not be rechecked; inspect it again".into());
            }
            if let Err(error) = current.driver.scroll_field_into_view(&selector) {
                ws.mark_manual_review(
                    &current.job_id,
                    "Control location failed; inspect the form manually",
                    now(),
                )
                .map_err(|e| e.to_string())?;
                current
                    .preview
                    .manual_review
                    .push("Control location failed; inspect the form manually".into());
                return Err(format!(
                    "Could not locate the control; inspect the live form: {error}"
                ));
            }
            let check = current.completed_selectors.first().map(|first| {
                current.driver.with_review_timeout(|driver| {
                    form_preview::verify_completed_fields(
                        driver,
                        &current.preview,
                        &current.completed_selectors,
                        first,
                    )
                })
            });
            let relevant = current_attachment_groups(&current.attachments, &current.id);
            let total_attachments: usize = relevant.iter().map(|(_, paths)| paths.len()).sum();
            let (mut checked_attachments, mut failed_attachments) = (0usize, 0usize);
            let started = Instant::now();
            for (selector, paths) in relevant.iter().take(8) {
                if started.elapsed() >= Duration::from_secs(4) {
                    break;
                }
                checked_attachments += paths.len();
                if !staged_attachments_unchanged(&current.attachments, &current.id, selector)
                    || current
                        .driver
                        .with_review_timeout(|driver| driver.verify_local_files(selector, paths))
                        .is_err()
                {
                    failed_attachments += paths.len();
                }
            }
            let unverified_attachments = total_attachments.saturating_sub(checked_attachments);
            let screenshot = match current
                .driver
                .with_review_timeout(|driver| driver.screenshot_base64())
            {
                Ok(image) => image,
                Err(error) => {
                    ws.mark_manual_review(
                        &current.job_id,
                        "Control location screenshot unavailable",
                        now(),
                    )
                    .map_err(|e| e.to_string())?;
                    current
                        .preview
                        .manual_review
                        .push("Control location screenshot unavailable".into());
                    return Err(format!("The control was scrolled, but the screenshot failed; inspect the live form: {error}"));
                }
            };
            let after = control_rank::snapshot(&mut current.driver);
            let same = after.as_ref().ok().map(|(_, value)| value.as_str()) == Some(digest);
            let needs_review = !same
                || failed_attachments > 0
                || unverified_attachments > 0
                || check
                    .as_ref()
                    .is_some_and(|value| !value.failed_labels.is_empty() || value.unverified > 0);
            if needs_review {
                let reason = "Application changed or filled fields could not be verified after locating a control";
                ws.mark_manual_review(&current.job_id, reason, now())
                    .map_err(|e| e.to_string())?;
                if !current
                    .preview
                    .manual_review
                    .iter()
                    .any(|item| item == reason)
                {
                    current.preview.manual_review.push(reason.into());
                }
            }
            let saved = current.record.as_mut().is_some_and(|record| record.screenshot_with_name(&screenshot).and_then(|name| record.event(json!({"type":"control_focus_capture","time":now(),"task_id":task_id,"index":index,"page_unchanged":same,"filled_verified":check.as_ref().map(|value| value.verified),"filled_unverified":check.as_ref().map(|value| value.unverified),"failed_fields":check.as_ref().map(|value| &value.failed_labels),"failed_details":check.as_ref().map(|value| &value.failed_details),"failed_attachments":failed_attachments,"unverified_attachments":unverified_attachments,"screenshot":format!("screenshots/{name}"),"note":"Scrolled only; no control was clicked."}))).is_ok());
            current.created = Instant::now();
            json_reply(
                stream,
                "200 OK",
                json!({"screenshot":screenshot,"record_saved":saved,"page_unchanged":same,"verified_count":check.as_ref().map_or(0, |value| value.verified),"unverified_count":check.as_ref().map_or(0, |value| value.unverified),"failed_fields":check.as_ref().map_or_else(Vec::new, |value| value.failed_labels.clone()),"failed_details":check.as_ref().map_or_else(Vec::new, |value| value.failed_details.clone()),"failed_attachments":failed_attachments,"unverified_attachments":unverified_attachments}),
            );
        }
        "/api/form/control-vision-pending" => {
            json_reply(stream, "200 OK", control_vision::pending()?);
        }
        "/api/form/control-vision-start" => {
            let source = body["task_id"].as_str().ok_or("missing advice task")?;
            let index = body["index"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value < 30)
                .ok_or("Invalid control index")?;
            let advice = control_rank::poll(source)?;
            if advice["status"] != "complete" {
                return Err("Control advice is unavailable; inspect again".into());
            }
            let current = session
                .as_mut()
                .filter(|s| {
                    Some(s.id.as_str()) == advice["session_id"].as_str()
                        && s.created.elapsed() <= s.ttl
                })
                .ok_or("Form session expired")?;
            let controls = advice["controls"]["controls"]
                .as_array()
                .ok_or("Control inventory unavailable")?;
            let control = controls.get(index).ok_or("Control is unavailable")?;
            let label = control["label"]
                .as_str()
                .filter(|label| !label.is_empty())
                .ok_or("Control label unavailable")?
                .to_owned();
            let selector = control["selector"]
                .as_str()
                .filter(|selector| !selector.is_empty() && selector.len() <= 4096)
                .ok_or("Control selector unavailable")?
                .to_owned();
            let digest = advice["snapshot_digest"]
                .as_str()
                .ok_or("Control snapshot unavailable")?
                .to_owned();
            let normalized = |value: &str| {
                value
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            };
            let ambiguous = label == "Unlabelled control"
                || controls
                    .iter()
                    .filter(|item| {
                        item["label"]
                            .as_str()
                            .is_some_and(|value| normalized(value) == normalized(&label))
                    })
                    .count()
                    != 1;
            if control_rank::snapshot(&mut current.driver)
                .ok()
                .map(|(_, value)| value)
                .as_deref()
                != Some(digest.as_str())
            {
                return Err("The page changed; inspect it again".into());
            }
            let image = current
                .driver
                .with_review_timeout(|driver| driver.screenshot_control(&selector))
                .map_err(|e| format!("Could not capture the control: {e}"))?;
            if control_rank::snapshot(&mut current.driver)
                .ok()
                .map(|(_, value)| value)
                .as_deref()
                != Some(digest.as_str())
            {
                return Err("The page changed during capture; inspect it again".into());
            }
            let model = resident_model()?.ok_or(
                "No single fully GPU-resident Ollama model is available within the 14 GiB budget",
            )?;
            let task_id = random_id()?;
            let capture_saved = current.record.as_mut().is_some_and(|record| record.screenshot_with_name(&image).and_then(|name| record.event(json!({"type":"control_visual_capture","time":now(),"task_id":task_id,"screenshot":format!("screenshots/{name}"),"note":"Advisory crop; no control was clicked."}))).is_ok());
            let pending_revision =
                control_vision::begin_pending(ws, &current.job_id, &task_id, now())?;
            if let Err(error) = control_vision::start(control_vision::Request {
                id: task_id.clone(),
                session: current.id.clone(),
                rank_task: source.to_owned(),
                control_index: index,
                job_id: current.job_id.as_str().to_owned(),
                pending_revision,
                model,
                selector,
                digest,
                image: image.clone(),
                label,
                ambiguous,
                capture_saved,
                choice: false,
            }) {
                control_vision::finish_pending(ws, &current.job_id, &task_id, pending_revision)?;
                return Err(error);
            }
            current.created = Instant::now();
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":task_id,"screenshot":image,"capture_saved":capture_saved}),
            );
        }
        "/api/form/control-vision-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = control_vision::poll(id)?;
            if matches!(result["status"].as_str(), Some("running" | "complete")) {
                let current = session.as_mut().filter(|s| {
                    Some(s.id.as_str()) == result["session_id"].as_str()
                        && s.created.elapsed() <= s.ttl
                });
                if let Some(current) = current {
                    current.created = Instant::now();
                    if result["status"] == "complete" {
                        let choice = result["choice"] == true;
                        let current_snapshot = if choice {
                            custom_choice_snapshot(&mut current.driver).map(|(_, digest)| digest)
                        } else {
                            control_rank::snapshot(&mut current.driver).map(|(_, digest)| digest)
                        }
                        .ok();
                        let image = result["selector"].as_str().and_then(|selector| {
                            current
                                .driver
                                .with_review_timeout(|driver| {
                                    if result["rank_task_id"] == "custom-choice-selected" {
                                        driver.screenshot_control(selector)
                                    } else if result["rank_task_id"] == "custom-choice-option" {
                                        driver.screenshot_custom_choice_option(
                                            selector,
                                            result["label"].as_str().unwrap_or(""),
                                        )
                                    } else if choice {
                                        driver.screenshot_field_context(selector)
                                    } else {
                                        driver.screenshot_control(selector)
                                    }
                                })
                                .ok()
                        });
                        let final_snapshot = if choice {
                            custom_choice_snapshot(&mut current.driver).map(|(_, digest)| digest)
                        } else {
                            control_rank::snapshot(&mut current.driver).map(|(_, digest)| digest)
                        }
                        .ok();
                        let same = control_vision::capture_is_current(
                            result["snapshot_digest"].as_str(),
                            current_snapshot.as_deref(),
                            final_snapshot.as_deref(),
                            result["image_hash"].as_str(),
                            image.as_deref(),
                        );
                        let previously_stale = result["comparison"] == "stale";
                        if !same || previously_stale {
                            result["comparison"] = json!("stale");
                        }
                        if result["comparison"] != "label_observed" {
                            let reason =
                                "Control visual review is uncertain or stale; inspect manually";
                            ws.mark_manual_review(&current.job_id, reason, now())
                                .map_err(|e| e.to_string())?;
                            if !current
                                .preview
                                .manual_review
                                .iter()
                                .any(|item| item == reason)
                            {
                                current.preview.manual_review.push(reason.into());
                            }
                        }
                        if result["result_saved"].is_null() {
                            let saved = current.record.as_mut().is_some_and(|record| record.event(json!({"type":"control_visual_result","time":now(),"task_id":id,"label":result["label"],"buttons":result["buttons"],"comparison":result["comparison"],"note":"Model transcription is advisory and does not prove a control's effect."})).is_ok());
                            result["result_saved"] = json!(saved);
                        } else if !same && !previously_stale {
                            let saved = current.record.as_mut().is_some_and(|record| record.event(json!({"type":"control_visual_stale","time":now(),"task_id":id})).is_ok());
                            result["result_saved"] = json!(saved && result["result_saved"] == true);
                        }
                    }
                } else if result["status"] == "complete" {
                    result = json!({"status":"failed","session_id":result["session_id"],"job_id":result["job_id"],"pending_revision":result["pending_revision"],"error":"The review session ended; inspect the page manually"});
                } else {
                    result["session_unavailable"] = json!(true);
                }
            }
            if result["status"] == "failed" && result["failure_recorded"].is_null() {
                let job =
                    JobIdentityId::new(result["job_id"].as_str().ok_or("missing review job")?)
                        .map_err(|e| e.to_string())?;
                let reason = "Control visual review failed; inspect manually";
                ws.mark_manual_review(&job, reason, now())
                    .map_err(|e| e.to_string())?;
                if let Some(current) = session.as_mut().filter(|s| s.job_id == job) {
                    if !current
                        .preview
                        .manual_review
                        .iter()
                        .any(|item| item == reason)
                    {
                        current.preview.manual_review.push(reason.into());
                    }
                }
                result["failure_recorded"] = json!(true);
            }
            if result["status"] != "running" {
                if result["pending_finished"].is_null() {
                    let job =
                        JobIdentityId::new(result["job_id"].as_str().ok_or("missing review job")?)
                            .map_err(|e| e.to_string())?;
                    let revision = result["pending_revision"]
                        .as_i64()
                        .ok_or("missing pending review revision")?;
                    control_vision::finish_pending(ws, &job, id, revision)?;
                    result["pending_finished"] = json!(true);
                }
                if result["choice"] == true
                    && matches!(
                        result["rank_task_id"].as_str(),
                        Some("custom-choice" | "custom-choice-selected")
                    )
                    && result["comparison"] == "label_observed"
                    && result["capture_saved"] == true
                    && result["result_saved"] == true
                    && result["pending_finished"] == true
                {
                    if let Some(current) = session
                        .as_mut()
                        .filter(|s| Some(s.id.as_str()) == result["session_id"].as_str())
                    {
                        if let Some((selector, action_id, revision, selected)) =
                            current.pending_choice_visual.clone()
                        {
                            if result["selector"].as_str() == Some(selector.as_str()) {
                                let (inventory, digest) =
                                    custom_choice_snapshot(&mut current.driver)?;
                                let expanded = inventory
                                    .choices
                                    .iter()
                                    .filter(|choice| choice.selector == selector && choice.expanded)
                                    .count()
                                    == 1;
                                let expected_visual = match selected.as_deref() {
                                    Some(value) => {
                                        result["rank_task_id"] == "custom-choice-selected"
                                            && result["label"].as_str() == Some(value)
                                    }
                                    None => result["rank_task_id"] == "custom-choice" && expanded,
                                };
                                if expected_visual
                                    && result["snapshot_digest"].as_str() == Some(digest.as_str())
                                {
                                    if let Some(value) = selected.as_deref() {
                                        verify_selected_choices_except(current, Some(&selector))?;
                                        current
                                            .driver
                                            .with_review_timeout(|driver| {
                                                driver.verify_custom_choice_value(&selector, value)
                                            })
                                            .map_err(|_| {
                                                "Selected value changed during visual review"
                                            })?;
                                    }
                                    custom_choice_action::finish_pending(
                                        ws,
                                        &current.job_id,
                                        &action_id,
                                        revision,
                                    )?;
                                    if let Some(value) = selected {
                                        current
                                            .selected_choices
                                            .retain(|(previous, _)| previous != &selector);
                                        current.selected_choices.push((selector, value));
                                    }
                                    current.pending_choice_visual = None;
                                    current.preview.manual_review.retain(|reason| {
                                        reason != form_preview::POST_CHOICE_VISUAL_REQUIRED
                                    });
                                    result["post_choice_review_finished"] = json!(true);
                                }
                            }
                        }
                    }
                }
                control_vision::complete(id, result.clone())?;
            }
            if let Some(object) = result.as_object_mut() {
                object.remove("selector");
                object.remove("snapshot_digest");
                object.remove("image_hash");
            }
            json_reply(stream, "200 OK", result);
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
            let experiences = navigation_experiences(ws, &current.job_id)?;
            let task_id = random_id()?;
            let pending_revision = form_links::begin_pending(ws, &current.job_id, &task_id, now())?;
            let current = session.take().ok_or("Form session expired")?;
            if let Err((current, error)) = form_links::start(
                task_id.clone(),
                current,
                Some(target.to_owned()),
                allow_other,
                reviewed_label,
                profile,
                (pending_revision, experiences),
            ) {
                let cleanup =
                    form_links::finish_pending(ws, &current.job_id, &task_id, pending_revision);
                *session = Some(*current);
                cleanup?;
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
                    form_links::finish_pending(ws, &error.job_id, id, error.pending_revision)?;
                    form_links::acknowledge_failure(id)?;
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"failed","error":error.reason}),
                    )
                }
                Some(Ok((mut current, pending_revision, experience_revisions))) => {
                    if !experience_revisions.is_empty() {
                        let lessons = ws.form_experiences();
                        protect_navigation_experience_snapshot(
                            &mut current.preview,
                            &experience_revisions,
                            lessons.as_deref().ok(),
                        );
                    }
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
                            &current.preview.fields_for_review(),
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
                    form_links::finish_pending(ws, &current.job_id, id, pending_revision)?;
                    json_reply(
                        stream,
                        "200 OK",
                        json!({"status":"complete","fields":current.preview.fields_for_review(),"model_review_selectors":current.preview.model_review_selectors,"repeated_label_selectors":current.preview.repeated_label_selectors(),"suggestions":current.preview.suggestions,"manual_review":current.preview.manual_review,"session_id":current.id,"origin":current.preview.schema.origin,"expires_in_seconds":remaining_session_seconds(current.ttl,current.created.elapsed()),"visible":current.visible,"fill_attempted":current.fill_attempted,"screenshot":screenshot,"links":links,"other_links":other_links,"other_links_available":other_links_available}),
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
            let experiences = navigation_experiences(ws, &current.job_id)?;
            let task_id = random_id()?;
            let pending_revision = form_links::begin_pending(ws, &current.job_id, &task_id, now())?;
            let current = session.take().ok_or("Form session expired")?;
            if let Err((current, error)) = form_links::start(
                task_id.clone(),
                current,
                None,
                false,
                None,
                profile,
                (pending_revision, experiences),
            ) {
                let cleanup =
                    form_links::finish_pending(ws, &current.job_id, &task_id, pending_revision);
                *session = Some(*current);
                cleanup?;
                return Err(error);
            }
            json_reply(stream, "200 OK", json!({"task_id":task_id}));
        }
        "/api/form/visual-focus" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"]
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 8192)
                .ok_or("Invalid field selector")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired")?;
            if !current
                .completed_selectors
                .iter()
                .any(|item| item == selector)
            {
                return Err(
                    "Only fields filled in this review session can be brought into view".into(),
                );
            }
            let fresh = current
                .driver
                .inspect_current()
                .map_err(|e| e.to_string())?;
            if fresh.schema != current.preview.schema
                || fresh.origin != current.preview.schema.origin
                || !form_preview::same_reviewable_walls(&current.preview, &fresh.walls)
            {
                return Err("Form changed or requires manual attention; inspect again".into());
            }
            current
                .driver
                .scroll_field_into_view(selector)
                .map_err(|e| e.to_string())?;
            let before = match current
                .driver
                .with_review_timeout(|driver| driver.screenshot_base64())
            {
                Ok(image) => image,
                Err(error) => {
                    ws.mark_manual_review(
                        &current.job_id,
                        "Visual focus screenshot unavailable",
                        now(),
                    )
                    .map_err(|e| {
                        format!("Screenshot failed and the review flag could not be saved: {e}")
                    })?;
                    return Err(format!(
                        "Could not capture the focused form; inspect it manually: {error}"
                    ));
                }
            };
            let check = current.driver.with_review_timeout(|driver| {
                form_preview::verify_completed_fields(
                    driver,
                    &current.preview,
                    &current.completed_selectors,
                    selector,
                )
            });
            let relevant = current_attachment_groups(&current.attachments, &current.id);
            let total_attachments: usize = relevant.iter().map(|(_, paths)| paths.len()).sum();
            let (mut checked_attachments, mut failed_attachments) = (0usize, 0usize);
            let started = Instant::now();
            for (selector, paths) in relevant.iter().take(8) {
                if started.elapsed() >= Duration::from_secs(4) {
                    break;
                }
                checked_attachments += paths.len();
                if !staged_attachments_unchanged(&current.attachments, &current.id, selector)
                    || current
                        .driver
                        .with_review_timeout(|driver| driver.verify_local_files(selector, paths))
                        .is_err()
                {
                    failed_attachments += paths.len();
                }
            }
            let unverified_attachments = total_attachments.saturating_sub(checked_attachments);
            let screenshot = match current
                .driver
                .with_review_timeout(|driver| driver.screenshot_base64())
            {
                Ok(image) => image,
                Err(error) => {
                    ws.mark_manual_review(
                        &current.job_id,
                        "Visual focus screenshot unavailable",
                        now(),
                    )
                    .map_err(|e| {
                        format!("Screenshot failed and the review flag could not be saved: {e}")
                    })?;
                    return Err(format!(
                        "Could not recapture the focused form; inspect it manually: {error}"
                    ));
                }
            };
            let before_hash = Sha256Digest::of(&before);
            let freshness =
                visual_review::image_freshness(Some(before_hash.as_str()), Some(&screenshot));
            if !check.failed_labels.is_empty()
                || check.unverified > 0
                || failed_attachments > 0
                || unverified_attachments > 0
                || freshness != "same"
            {
                let reason =
                    "Filled fields, attachments or the visible form changed, or could not all be verified after visual navigation";
                ws.mark_manual_review(&current.job_id, reason, now()).map_err(|e| format!("Field verification was incomplete and the review flag could not be saved: {e}"))?;
                if !current
                    .preview
                    .manual_review
                    .iter()
                    .any(|item| item == reason)
                {
                    current.preview.manual_review.push(reason.into());
                }
            }
            let field_label = capture_field_label(&current.preview.schema, selector);
            let saved = current.record.as_mut().is_some_and(|record| {
                let original = if freshness != "same" { record.screenshot_with_name(&before).map(Some) } else { Ok(None) };
                original.and_then(|original| record.screenshot_with_name(&screenshot).and_then(|name| record.event(json!({"type":"visual_focus_capture","time":now(),"selector":selector,"field_label":field_label,"value_retained":check.selected_retained && freshness == "same","visual_freshness":freshness,"before_screenshot":original.map(|name| format!("screenshots/{name}")),"verified_count":check.verified,"unverified_count":check.unverified,"failed_fields":check.failed_labels,"failed_details":check.failed_details,"failed_attachments":failed_attachments,"unverified_attachments":unverified_attachments,"screenshot":format!("screenshots/{name}")})))).is_ok()
            });
            json_reply(
                stream,
                "200 OK",
                json!({"screenshot":screenshot,"record_saved":saved,"value_retained":check.selected_retained && freshness == "same","visual_freshness":freshness,"verified_count":check.verified,"unverified_count":check.unverified,"failed_fields":check.failed_labels,"failed_details":check.failed_details,"failed_attachments":failed_attachments,"unverified_attachments":unverified_attachments}),
            );
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
            let target = match body.get("target_selector") {
                None | Some(Value::Null) => None,
                Some(Value::String(selector)) if segment == 0 && half.is_none() => {
                    if !current.completed_selectors.contains(selector) {
                        return Err("Select a field filled in this session".into());
                    }
                    let field = current
                        .preview
                        .schema
                        .fields
                        .iter()
                        .find(|field| &field.selector == selector)
                        .ok_or("Field is no longer present")?;
                    if !matches!(
                        field.field_type,
                        waypoint_applications::form_mapping::FormFieldType::Text
                            | waypoint_applications::form_mapping::FormFieldType::Email
                            | waypoint_applications::form_mapping::FormFieldType::TextArea
                            | waypoint_applications::form_mapping::FormFieldType::Select { .. }
                    ) {
                        return Err("Focused visual transcription currently supports text and single-select fields; inspect this field manually".into());
                    }
                    if field.label.trim().is_empty() || field.label.len() > 500 {
                        return Err("Field has no usable label for visual review".into());
                    }
                    Some((selector.clone(), field.label.clone()))
                }
                _ => return Err("Invalid targeted visual review".into()),
            };
            let (width, height) = current
                .driver
                .page_dimensions()
                .map_err(|e| e.to_string())?;
            let (x, clip_width, y, clip_height, total, page_coverage_complete) =
                visual_review::segment_half_geometry(width, height, segment as usize, half)?;
            let coverage_complete = target.is_none()
                && page_coverage_complete
                && !current.preview.manual_review.iter().any(|reason| {
                    reason == waypoint_browser::UNINSPECTED_FRAME_REVIEW
                        || reason == waypoint_browser::SHADOW_FORM_REVIEW
                });
            let url = current.driver.current_url().map_err(|e| e.to_string())?;
            let signature = visual_review::page_signature(&url, width, height);
            let screenshot = if let Some((selector, _)) = &target {
                current.driver.screenshot_field_context(selector)
            } else {
                current
                    .driver
                    .screenshot_tile(x, y, clip_width, clip_height)
            }
            .map_err(|e| e.to_string())?;
            let task_id = random_id()?;
            let target_selectors = target.as_ref().map(|(selector, _)| vec![selector.clone()]);
            let (expected, comparisons_truncated) = visual_review::expected_from_preview(
                &current.preview,
                target_selectors
                    .as_ref()
                    .unwrap_or(&current.completed_selectors),
            );
            let omitted_labels = expected.omitted_labels.clone();
            let result_path = current
                .record
                .as_ref()
                .and_then(|record| record.visual_result_path(&task_id).ok());
            let pending_revision =
                visual_review::begin_pending_review(ws, &current.job_id, &task_id, now())?;
            if let Err(error) = visual_review::start(
                task_id.clone(),
                current.id.clone(),
                (
                    signature.as_str().to_owned(),
                    segment as usize,
                    half,
                    current.job_id.as_str().to_owned(),
                    pending_revision,
                ),
                model,
                screenshot.clone(),
                (expected, target.clone()),
                result_path,
            ) {
                visual_review::finish_pending_review(
                    ws,
                    &current.job_id,
                    &task_id,
                    pending_revision,
                )?;
                return Err(error);
            }
            let field_label = target
                .as_ref()
                .and_then(|(selector, _)| capture_field_label(&current.preview.schema, selector));
            let record_saved = current.record.as_mut().is_some_and(|record| {
                record.screenshot_with_name(&screenshot).and_then(|name| {
                    record.event(json!({"type":"visual_capture","time":now(),"task_id":task_id,"target_selector":target.as_ref().map(|(selector, _)| selector),"field_label":field_label,"segment":segment,"half":half,"clip":if target.is_none() { json!({"x":x,"y":y,"width":clip_width,"height":clip_height}) } else { Value::Null },"segments":if target.is_some() { 1 } else { total },"coverage_complete":coverage_complete,"comparisons_truncated":comparisons_truncated,"omitted_labels":omitted_labels,"page_signature":signature.as_str(),"screenshot":format!("screenshots/{name}"),"result_file":format!("visual-result-{task_id}.json")}))
                }).is_ok()
            });
            json_reply(
                stream,
                "200 OK",
                json!({"task_id":task_id,"screenshot":screenshot,"target_selector":target.as_ref().map(|(selector, _)| selector),"segment":segment,"half":half,"segments":if target.is_some() { 1 } else { total },"coverage_complete":coverage_complete,"comparisons_truncated":comparisons_truncated,"omitted_labels":omitted_labels,"page_signature":signature.as_str(),"record_saved":record_saved}),
            );
        }
        "/api/form/visual-review-status" => {
            let id = body["task_id"].as_str().ok_or("missing task id")?;
            let mut result = visual_review::poll(id)?;
            if result["status"] == "failed" {
                let job = JobIdentityId::new(
                    result["job_id"]
                        .as_str()
                        .ok_or("Visual review has no job")?,
                )
                .map_err(|e| e.to_string())?;
                visual_review::flag_visual_failure(ws, &job, "failed", now())?;
                if let Some(revision) = result["pending_revision"].as_i64() {
                    visual_review::finish_pending_review(ws, &job, id, revision)?;
                }
                if let Some(current) = session.as_mut().filter(|s| {
                    Some(s.id.as_str()) == result["session_id"].as_str()
                        && s.created.elapsed() <= s.ttl
                        && s.job_id == job
                }) {
                    let reason = visual_review::VISUAL_FAILURE_REASON;
                    if !current
                        .preview
                        .manual_review
                        .iter()
                        .any(|item| item == reason)
                    {
                        current.preview.manual_review.push(reason.into());
                    }
                }
            }
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
                    let (url, (width, height)) = current
                        .driver
                        .with_review_timeout(|driver| {
                            Ok::<_, waypoint_applications::driver::DriverError>((
                                driver.current_url()?,
                                driver.page_dimensions()?,
                            ))
                        })
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
                    let capture_target = result["target_selector"].as_str().map(str::to_owned);
                    let capture_segment = result["segment"].as_u64();
                    let capture_half = result["half"]
                        .as_u64()
                        .and_then(|h| usize::try_from(h).ok());
                    let capture = |driver: &mut waypoint_browser::CdpDriver| {
                        if let Some(selector) = capture_target.as_deref() {
                            driver.screenshot_field_context(selector).ok()
                        } else {
                            let segment = capture_segment?;
                            let (x, clip_width, y, clip_height, _, _) =
                                visual_review::segment_half_geometry(
                                    width,
                                    height,
                                    segment as usize,
                                    capture_half,
                                )
                                .ok()?;
                            driver.screenshot_tile(x, y, clip_width, clip_height).ok()
                        }
                    };
                    let fresh_screenshot = current.driver.with_review_timeout(capture);
                    let (verified, unconfirmed) = current.driver.with_review_timeout(|driver| {
                        visual_review::crosscheck_live_values(
                            driver,
                            &current.preview,
                            &current.completed_selectors,
                            &mut result["visual_review"],
                        )
                    });
                    let final_screenshot = if verified + unconfirmed == 0 {
                        fresh_screenshot.clone()
                    } else {
                        current.driver.with_review_timeout(capture)
                    };
                    let mut freshness = visual_review::interval_freshness(
                        result["screenshot_hash"].as_str(),
                        fresh_screenshot.as_deref(),
                        final_screenshot.as_deref(),
                    );
                    let mut final_signature_status = "not_rechecked";
                    if verified + unconfirmed > 0 && freshness != "changed" {
                        let final_signature = current.driver.with_review_timeout(|driver| {
                            driver.current_url().ok().and_then(|url| {
                                driver.page_dimensions().ok().map(|(width, height)| {
                                    visual_review::page_signature(&url, width, height)
                                })
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
                    if visual_review::flag_transcription_mismatch(
                        ws,
                        &current.job_id,
                        &result["visual_review"],
                        now(),
                    )? {
                        let reason = visual_review::TRANSCRIPTION_MISMATCH_REASON;
                        if !current
                            .preview
                            .manual_review
                            .iter()
                            .any(|item| item == reason)
                        {
                            current.preview.manual_review.push(reason.into());
                        }
                    }
                    let target_unobserved = visual_review::flag_unobserved_target(
                        ws,
                        &current.job_id,
                        capture_target.is_some(),
                        &result["visual_review"],
                        now(),
                    )?;
                    result["target_unobserved"] = json!(target_unobserved);
                    if target_unobserved {
                        let reason = visual_review::TARGET_UNOBSERVED_REASON;
                        if !current
                            .preview
                            .manual_review
                            .iter()
                            .any(|item| item == reason)
                        {
                            current.preview.manual_review.push(reason.into());
                        }
                    }
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
                    if let Some(revision) = result["pending_revision"].as_i64() {
                        visual_review::finish_pending_review(ws, &current.job_id, id, revision)?;
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
        "/api/form/selection-snapshot" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"].as_str().ok_or("missing field")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && !s.filled && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired or already filled")?;
            let field = current
                .preview
                .schema
                .fields
                .iter()
                .find(|f| f.selector == selector)
                .ok_or("Field is no longer present")?;
            if field.sensitive
                || current.preview.model_review_selectors.contains(selector)
                || !matches!(
                    field.field_type,
                    waypoint_applications::form_mapping::FormFieldType::Select { .. }
                )
            {
                return Err("This field requires manual handling".into());
            }
            check_answer_page(current)?;
            let snapshot = current
                .driver
                .selected_option_snapshot(selector)
                .map_err(|e| e.to_string())?;
            let digest = form_preview::selection_snapshot_digest(
                &format!("prefill-{}", current.id),
                selector,
                &snapshot,
            );
            json_reply(
                stream,
                "200 OK",
                json!({"selection":snapshot,"digest":digest}),
            );
        }
        "/api/form/text-snapshot" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"].as_str().ok_or("missing field")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && !s.filled && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired or already filled")?;
            let field = current
                .preview
                .schema
                .fields
                .iter()
                .find(|f| f.selector == selector)
                .ok_or("Field is no longer present")?;
            if field.sensitive
                || current.preview.model_review_selectors.contains(selector)
                || !matches!(
                    field.field_type,
                    waypoint_applications::form_mapping::FormFieldType::Text
                        | waypoint_applications::form_mapping::FormFieldType::Email
                        | waypoint_applications::form_mapping::FormFieldType::TextArea
                        | waypoint_applications::form_mapping::FormFieldType::Date
                        | waypoint_applications::form_mapping::FormFieldType::Month
                        | waypoint_applications::form_mapping::FormFieldType::Time
                        | waypoint_applications::form_mapping::FormFieldType::DateTimeLocal
                )
            {
                return Err("This field requires manual handling".into());
            }
            check_answer_page(current)?;
            let snapshot = current
                .driver
                .text_value_snapshot(selector)
                .map_err(|e| e.to_string())?;
            let digest = form_preview::text_snapshot_digest(
                &format!("prefill-{}", current.id),
                selector,
                &snapshot,
            );
            json_reply(stream, "200 OK", json!({"text":snapshot,"digest":digest}));
        }
        "/api/form/choice-snapshot" => {
            let id = body["session_id"].as_str().ok_or("missing session")?;
            let selector = body["selector"].as_str().ok_or("missing field")?;
            let current = session
                .as_mut()
                .filter(|s| s.id == id && !s.filled && s.created.elapsed() <= s.ttl)
                .ok_or("Form session expired or already filled")?;
            let field = current
                .preview
                .schema
                .fields
                .iter()
                .find(|f| f.selector == selector)
                .ok_or("Field is no longer present")?;
            if field.sensitive || current.preview.model_review_selectors.contains(selector) {
                return Err("This field requires manual handling".into());
            }
            let kind = match field.field_type {
                waypoint_applications::form_mapping::FormFieldType::MultiSelect { .. } => "multi",
                waypoint_applications::form_mapping::FormFieldType::Radio { .. } => "radio",
                waypoint_applications::form_mapping::FormFieldType::Checkbox => "checkbox",
                _ => return Err("This field requires manual handling".into()),
            };
            check_answer_page(current)?;
            let snapshot = current
                .driver
                .choice_snapshot(selector, kind)
                .map_err(|e| e.to_string())?;
            let digest = form_preview::choice_snapshot_digest(
                &format!("prefill-{}", current.id),
                selector,
                &snapshot,
            );
            json_reply(stream, "200 OK", json!({"choice":snapshot,"digest":digest}));
        }
        "/api/form/fill" => {
            if body["reviewed"].as_bool() != Some(true) {
                return Err("Explicit review is required".into());
            }
            let requested = body["session_id"].as_str().ok_or("missing session")?;
            let current = session
                .as_mut()
                .ok_or("Form session expired; inspect it again")?;
            if ws
                .has_duplicate_hold(&current.job_id)
                .map_err(|e| e.to_string())?
            {
                return Err(
                    "Review possible duplicate jobs before filling this application".into(),
                );
            }
            require_duplicate_index_ready(ws)?;
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
            verify_selected_choices(current)?;
            let (prepared, omitted) = prepare_reviewed_fill(&current.preview, &body)?;
            let mut replacements = form_preview::ReviewedReplacements::new();
            if let Some(items) = body.get("reviewed_replacements") {
                let items = items
                    .as_array()
                    .filter(|v| v.len() <= 20)
                    .ok_or("Invalid replacement approvals")?;
                for item in items {
                    let selector = item["selector"]
                        .as_str()
                        .ok_or("Invalid replacement field")?;
                    let digest = item["digest"]
                        .as_str()
                        .filter(|v| {
                            v.len() == 64
                                && v.bytes()
                                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        })
                        .ok_or("Invalid replacement digest")?;
                    let field = prepared
                        .schema
                        .fields
                        .iter()
                        .find(|f| f.selector == selector)
                        .ok_or("Unknown replacement field")?;
                    if item["reviewed"].as_bool() != Some(true)
                        || field.sensitive
                        || prepared.model_review_selectors.contains(selector)
                        || !matches!(
                            field.field_type,
                            waypoint_applications::form_mapping::FormFieldType::Select { .. }
                                | waypoint_applications::form_mapping::FormFieldType::Text
                                | waypoint_applications::form_mapping::FormFieldType::Email
                                | waypoint_applications::form_mapping::FormFieldType::TextArea
                                | waypoint_applications::form_mapping::FormFieldType::Date
                                | waypoint_applications::form_mapping::FormFieldType::Month
                                | waypoint_applications::form_mapping::FormFieldType::Time
                                | waypoint_applications::form_mapping::FormFieldType::DateTimeLocal
                                | waypoint_applications::form_mapping::FormFieldType::MultiSelect { .. }
                                | waypoint_applications::form_mapping::FormFieldType::Radio { .. }
                                | waypoint_applications::form_mapping::FormFieldType::Checkbox
                        )
                        || !prepared
                            .suggestions
                            .iter()
                            .any(|m| m.field_selector == selector)
                        || replacements
                            .insert(selector.to_owned(), digest.to_owned())
                            .is_some()
                    {
                        return Err(
                            "Replacement approval must identify a reviewed, selected field".into(),
                        );
                    }
                }
            }
            let mut requested_fields = form_preview::requested_field_records(&prepared)?;
            attach_answer_provenance(ws, current, &body, &mut requested_fields)?;
            let intent =
                IntentId::new(format!("prefill-{}", current.id)).map_err(|e| e.to_string())?;
            let grant_id = GrantId::new(random_id()?).map_err(|e| e.to_string())?;
            let mut grant = ApprovalGrant {
                id: grant_id,
                intent_id: IntentId::new(intent.as_str()).map_err(|e| e.to_string())?,
                bound_payload_sha256: form_preview::fill_payload_digest(&prepared, &replacements)?,
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
            if let Err(error) = form_preview::preflight_existing_values(
                &mut current.driver,
                &current.preview,
                intent.as_str(),
                &replacements,
            ) {
                let review_error = ws
                    .mark_manual_review(
                        &current.job_id,
                        "Existing form value needs manual review",
                        now(),
                    )
                    .err();
                current.preview = original_preview;
                current.created = Instant::now();
                *session = Some(current);
                let note = review_error
                    .map(|e| format!(" The manual-review flag could not be saved: {e}."))
                    .unwrap_or_default();
                return Err(format!("No fields were filled because an existing value needs review: {error}. Unselect that field or inspect the browser. For supported fields, you can review a replacement in this panel, then retry.{note}"));
            }
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
            let mut requested_answer_file = None;
            if let Some(record) = &mut current.record {
                let requested = json!({"type":"fields_requested","time":now(),"origin":current.preview.schema.origin,"answers":current.preview.suggestions,"reviewed_replacements":replacements,"fields":requested_fields,"note":"These are requested answers, not proof that the fields were filled or submitted."});
                let saved = record.requested_answers(&requested).and_then(|path| {
                    let mut event = requested;
                    event["answer_file"] = json!(path);
                    record.event(event)?;
                    requested_answer_file = Some(path);
                    Ok(())
                });
                if let Err(error) = saved {
                    let review_error = ws
                        .mark_manual_review(
                            &current.job_id,
                            "Application record could not be written; no fields were filled",
                            now(),
                        )
                        .err();
                    current.preview = original_preview;
                    *session = Some(current);
                    let note = review_error
                        .map(|e| format!(" The manual-review flag could not be saved: {e}."))
                        .unwrap_or_default();
                    return Err(format!("No fields were filled because the attempt record could not be written: {error}. The review session remains open.{note}"));
                }
            }
            current.filled = true;
            current.fill_attempted = true;
            let mut completed = Vec::new();
            let result = form_preview::apply_reviewed_text_fields_with_replacements(
                ws,
                &current.job_id,
                &mut current.driver,
                &current.url,
                (&current.preview, &replacements),
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
            for (selector, paths) in current_attachment_groups(&current.attachments, &current.id) {
                if !staged_attachments_unchanged(&current.attachments, &current.id, &selector)
                    || current
                        .driver
                        .verify_local_files(&selector, &paths)
                        .is_err()
                {
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
                    let mut outcome_error = None;
                    if let Some(record) = &mut current.record {
                        let outcome = json!({"type":"fill_incomplete","time":now(),"answer_file":requested_answer_file,"error":error,"completed_commands":completed,"note":"These commands completed before the failure; they are not proof that all values remain correct or that an application was submitted."});
                        if let Err(problem) = record.incomplete_fill(outcome, now()) {
                            outcome_error = Some(problem);
                        }
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
                    let outcome_note = outcome_error
                        .map(|e| format!(" The attempt record could not be fully updated: {e}."))
                        .unwrap_or_default();
                    return Err(format!(
                        "Autofill stopped after {} successful field commands; the failing field and earlier fields may have changed. Review the browser: {error}.{attachment_note}{flag_note}{outcome_note}", completed.len()
                    ));
                }
            };
            if verify_selected_choices(&mut current).is_err() {
                let reason =
                    "A previously selected choice changed after filling; inspect the form manually";
                current.preview.manual_review.push(reason.into());
                let recorded = current.record.as_mut().is_some_and(|record| record.event(json!({"type":"custom_choice_verification_failed","time":now(),"note":reason})).is_ok());
                let review = ws.mark_manual_review(&current.job_id, reason, now());
                current.created = Instant::now();
                *session = Some(current);
                if !recorded || review.is_err() {
                    return Err(format!(
                        "{reason}. The review status or record could not be saved."
                    ));
                }
                return Err(reason.into());
            }
            if let Some(record) = &mut current.record {
                if let Err(error) = record.answer_outcome(json!({"type":"fill_commands_verified","time":now(),"answer_file":requested_answer_file,"completed_commands":completed,"note":"All requested fields matched when checked after the fill commands. The site may change values later; this does not prove submission."})) {
                    let status_error = record.status("manual_review", now()).err();
                    let review_error = ws.mark_manual_review(&current.job_id, "Application record could not be written after filling", now()).err();
                    current.created = Instant::now();
                    *session = Some(current);
                    let detail = status_error.or_else(|| review_error.map(|e| e.to_string())).map(|e| format!(" The review status could not be saved: {e}.")).unwrap_or_default();
                    return Err(format!("Fields were filled, but their completion could not be recorded: {error}. Inspect the open browser manually.{detail}"));
                }
            }
            let screenshot = match current.driver.screenshot_base64() {
                Ok(image) => image,
                Err(error) => {
                    let record_error = current.record.as_mut().and_then(|record| {
                        let event_error = record.event(json!({"type":"fill_screenshot_failed","time":now(),"note":"The review screenshot was unavailable; inspect the live form manually."})).err();
                        let status_error = record.status("manual_review", now()).err();
                        event_error.or(status_error)
                    });
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
                    let record_note = record_error
                        .map(|e| format!(" The screenshot failure could not be recorded: {e}."))
                        .unwrap_or_default();
                    return Err(format!(
                        "Fields were filled, but the review screenshot failed: {error}.{flag_note}{record_note} Inspect the open browser manually."
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
                json!({"filled":filled,"filled_fields":filled_fields,"completed_selectors":current.completed_selectors,"required_not_filled":required_not_filled,"screenshot":screenshot,"manual_review":current.preview.manual_review,"expires_in_seconds":remaining_session_seconds(current.ttl,current.created.elapsed())}),
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
            require_duplicate_index_ready(ws)?;
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
            verify_selected_choices(current)?;
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
            let multiple = current
                .driver
                .file_input_allows_multiple(selector)
                .map_err(|e| e.to_string())?;
            let previous = current_attachment_groups(&current.attachments, &current.id)
                .into_iter()
                .find(|(field, _)| field == selector)
                .map(|(_, paths)| paths)
                .unwrap_or_default();
            let mut files = if multiple {
                previous.clone()
            } else {
                Vec::new()
            };
            if !previous.is_empty() {
                if !staged_attachments_unchanged(&current.attachments, &current.id, selector) {
                    return Err("A staged document changed; review the field manually".into());
                }
                current
                    .driver
                    .verify_local_files(selector, &previous)
                    .map_err(|_| "Previously attached files changed; review the field manually")?;
            }
            if files.iter().any(|path| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(&name))
            }) {
                return Err("Choose PDFs with distinct attachment names for this field".into());
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
            if visual_review
                .as_ref()
                .is_some_and(|(_, review)| document_visual::needs_followup(review))
            {
                ws.mark_manual_review(
                    &current.job_id,
                    "Attached document visual review has findings or incomplete checks",
                    now(),
                )
                .map_err(|e| e.to_string())?;
                current.preview.manual_review.push("The linked visual review contains findings or unavailable checks; inspect the PDF".into());
                if let Some(record) = &mut current.record {
                    record.status("manual_review", now())?;
                    record.event(json!({"type":"document_visual_followup_needed","time":now(),"note":"The linked review contains findings or unavailable checks; inspect the PDF."}))?;
                }
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
            let document_sha256 = waypoint_domain::sha256_hex(&bytes);
            if !staged_file_matches(&path, &document_sha256) {
                return Err(
                    "The staged document changed; review the document before attaching".into(),
                );
            }
            files.push(path.clone());
            let grant_id = random_id()?;
            let payload = json!({"selector":selector,"document_id":document_id,"document_sha256":document_sha256}).to_string();
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
            if let Err(error) = current
                .driver
                .attach_local_files_if_unchanged(selector, &files, &previous)
            {
                if let Some(record) = &mut current.record {
                    let _ = record.event(json!({"type":"attachment_incomplete","time":now(),"selector":selector,"error":error.to_string()}));
                }
                ws.mark_manual_review(&current.job_id, "Application needs manual review", now())
                    .map_err(|e| e.to_string())?;
                return Err(format!(
                    "Document attachment may be incomplete; review the form: {error}"
                ));
            }
            if !staged_file_matches(&path, &document_sha256)
                || !staged_attachments_unchanged(&current.attachments, &current.id, selector)
            {
                ws.mark_manual_review(&current.job_id, "Application needs manual review", now())
                    .map_err(|e| e.to_string())?;
                return Err(
                    "A staged document changed while attaching; inspect the form manually".into(),
                );
            }
            if !multiple {
                current.attachments.retain(|(previous, _, inspection, _)| {
                    previous != selector || inspection != &current.id
                });
            }
            current.attachments.push((
                selector.to_owned(),
                path,
                current.id.clone(),
                document_sha256,
            ));
            let field_label = capture_field_label(&current.preview.schema, selector);
            if let Some(record) = &mut current.record {
                record.event(json!({"type":"document_attached","time":now(),"selector":selector,"name":doc.name,"attachment_name":name}))?;
                let screenshot = current
                    .driver
                    .screenshot_base64()
                    .map_err(|e| e.to_string());
                if let Err(error) = attempt_record::attachment_viewport(
                    record,
                    selector,
                    field_label.as_deref(),
                    screenshot,
                    now(),
                ) {
                    let _ = record.event(json!({"type":"attachment_viewport_unavailable","time":now(),"selector":selector,"error":error}));
                    ws.mark_manual_review(
                        &current.job_id,
                        "Post-attachment screenshot unavailable",
                        now(),
                    )
                    .map_err(|e| e.to_string())?;
                    current.preview.manual_review.push(
                        "Post-attachment screenshot unavailable; inspect the file field manually"
                            .into(),
                    );
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
            if verify_selected_choices(current).is_err() {
                let reason = "A previously selected choice changed after attachment; inspect the form manually";
                current.preview.manual_review.push(reason.into());
                let recorded = current.record.as_mut().is_some_and(|record| record.event(json!({"type":"custom_choice_verification_failed","time":now(),"note":reason})).is_ok());
                let review = ws.mark_manual_review(&current.job_id, reason, now());
                if !recorded || review.is_err() {
                    return Err(format!(
                        "{reason}. The review status or record could not be saved."
                    ));
                }
                return Err(reason.into());
            }
            current.created = Instant::now();
            json_reply(
                stream,
                "200 OK",
                json!({"ok":true,"name":doc.name,"attachment_name":name,"attachment_count":files.len(),"draft_provenance":draft_provenance_status,"provenance_lookup_incomplete":provenance_incomplete,"manual_review":current.preview.manual_review}),
            );
        }
        "/api/form/session-status" => {
            let current = session
                .as_ref()
                .filter(|current| current.created.elapsed() <= current.ttl);
            let pending_inspection = form_task::pending()?;
            let pending_navigation = form_links::pending()?;
            json_reply(
                stream,
                "200 OK",
                match current {
                    Some(current) => {
                        json!({"active":true,"session_id":current.id,"job_id":current.job_id.as_str(),"visible":current.visible,"fill_attempted":current.fill_attempted})
                    }
                    None => {
                        json!({"active":false,"inspection":pending_inspection.map(|(task,job)| json!({"task":task,"job":job})),"navigation":pending_navigation.map(|(task,job)| json!({"task":task,"job":job}))})
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
                form_task::finish_pending(
                    ws,
                    &current.job_id,
                    &current.inspection_task_id,
                    current.inspection_pending_revision,
                )?;
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
                    match load_draft_result(ws, &id, draft_id) {
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
            let task_id = match draft_request_id(&body["request_id"])? {
                Some(id) => id,
                None => random_id()?,
            };
            if ws
                .generated_draft(&task_id, &id)
                .map_err(|e| e.to_string())?
                .is_some()
            {
                return Err("Draft request identifier already exists".into());
            }
            let digest = draft_context_digest(ws, &id, kind)?;
            let reviewed_template = if let Some(text) = body["reviewed_template_text"].as_str() {
                if body["template_reviewed"] != true {
                    return Err("Review the transcription before redrafting".into());
                }
                let previous = body["template_draft_id"]
                    .as_str()
                    .ok_or("Original draft is required")?;
                let cached = load_draft_result(ws, &id, previous)?;
                Some(draft_task::reviewed_template(
                    &cached,
                    id.as_str(),
                    kind,
                    &digest,
                    text,
                )?)
            } else {
                None
            };
            let template_scan_page = match &body["template_scan_page"] {
                Value::Null => None,
                Value::Number(number)
                    if reviewed_template.is_none() || body["retain_reviewed_pages"] == true =>
                {
                    let page = number.as_u64().ok_or("Invalid scanned template page")?;
                    let previous = body["template_draft_id"]
                        .as_str()
                        .ok_or("Original draft is required")?;
                    let cached = load_draft_result(ws, &id, previous)?;
                    Some(draft_task::requested_scanned_template_page(
                        &cached,
                        id.as_str(),
                        kind,
                        &digest,
                        page,
                    )?)
                }
                _ => {
                    return Err("Choose one scanned template page or reviewed transcription".into())
                }
            };
            if body["retain_reviewed_pages"] == true
                && (reviewed_template.is_none() || template_scan_page.is_none())
            {
                return Err(
                    "Review the current transcription and choose another scanned page".into(),
                );
            }
            let template_style_focus = match &body["template_style_focus"] {
                Value::Null => None,
                Value::String(text)
                    if !text.trim().is_empty()
                        && answer_task::json_prefix(text, 350).len() == text.len()
                        && template.is_some() =>
                {
                    Some(text.clone())
                }
                _ => {
                    return Err(
                        "Choose a short passage from the selected template for style guidance"
                            .into(),
                    )
                }
            };
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
                    reviewed_template,
                    template_style_focus,
                    template_scan_page,
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
                let current = draft_context_digest(ws, &job_id, kind).ok().as_deref()
                    == result["digest"].as_str();
                let bytes = serde_json::to_vec(&result).map_err(|_| "Could not serialize draft")?;
                result["saved"] = json!(ws
                    .save_generated_draft(id, &job_id, kind, now(), &bytes)
                    .is_ok());
                result["current"] = json!(current);
            }
            json_reply(stream, "200 OK", result);
        }
        "/api/draft/list" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let mut entries = ws.generated_drafts(&job).map_err(|e| e.to_string())?;
            let truncated = entries.len() > 50;
            entries.truncate(50);
            json_reply(
                stream,
                "200 OK",
                json!({"drafts":entries.into_iter().map(|(id, kind, created_at)| json!({"id":id,"kind":kind,"created_at":created_at})).collect::<Vec<_>>(),"truncated":truncated}),
            );
        }
        "/api/draft/read" => {
            let job = JobIdentityId::new(body["id"].as_str().ok_or("missing id")?)
                .map_err(|e| e.to_string())?;
            let id = body["draft_id"].as_str().ok_or("missing draft id")?;
            let bytes = ws
                .generated_draft(id, &job)
                .map_err(|e| e.to_string())?
                .ok_or("Saved draft not found")?;
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| "Saved draft is invalid")?;
            let kind = value["kind"].as_str().ok_or("Saved draft is invalid")?;
            if value["job_id"].as_str() != Some(job.as_str())
                || !matches!(kind, "cv" | "cover_letter")
                || value["status"] != "complete"
            {
                return Err("Saved draft is invalid".into());
            }
            let current =
                draft_context_digest(ws, &job, kind).ok().as_deref() == value["digest"].as_str();
            json_reply(stream, "200 OK", json!({"current":current,"result":value}));
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

fn save_uncollected_draft(ws: &Workspace) -> Result<bool, String> {
    let Some((id, result)) = draft_task::completed_for_save()? else {
        return Ok(false);
    };
    let job = JobIdentityId::new(result["job_id"].as_str().ok_or("missing draft job")?)
        .map_err(|e| e.to_string())?;
    let kind = result["kind"].as_str().ok_or("missing draft type")?;
    let bytes = serde_json::to_vec(&result).map_err(|_| "Could not serialize draft")?;
    ws.save_generated_draft(&id, &job, kind, now(), &bytes)
        .map_err(|e| e.to_string())?;
    draft_task::mark_saved(&id)?;
    Ok(true)
}

fn duplicate_index_delay(compared: bool, more_pending: bool) -> Duration {
    if compared {
        Duration::from_secs(2)
    } else if more_pending {
        Duration::from_millis(200)
    } else {
        Duration::from_secs(30)
    }
}

fn serve_loop(
    mut ws: Workspace,
    listener: TcpListener,
    token: String,
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut index_worker = match ws.duplicate_indexer() {
        Ok(indexer) => {
            let index_stop = stop.clone();
            Some(std::thread::spawn(move || {
                let mut cursor = String::new();
                let mut next = Instant::now();
                while !index_stop.load(std::sync::atomic::Ordering::Acquire) {
                    if Instant::now() >= next {
                        let delay = match indexer.index_next_step(&cursor) {
                            Ok(step) => {
                                cursor = step.cursor.unwrap_or_default();
                                duplicate_index_delay(step.compared, step.more_pending)
                            }
                            Err(waypoint_workspace::WorkspaceError::DuplicateIndexBusy) => {
                                Duration::from_secs(2)
                            }
                            Err(error) => {
                                eprintln!("Duplicate indexing paused: {error}");
                                cursor.clear();
                                Duration::from_secs(30)
                            }
                        };
                        next = Instant::now() + delay;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }))
        }
        Err(error) => {
            eprintln!("Background duplicate indexing unavailable: {error}; using idle indexing");
            None
        }
    };
    let mut session: Option<ReviewSession> = None;
    let mut duplicate_cursor = String::new();
    let mut next_duplicate_index = Instant::now();
    let mut next_email_collect = Instant::now();
    let mut next_draft_save = Instant::now();
    let mut next_discovery_collect = Instant::now();
    let mut next_web_search_save = Instant::now();
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        if Instant::now() >= next_web_search_save {
            next_web_search_save = Instant::now()
                + match web_search_batch::persist_progress(&ws)
                    .and_then(|_| web_search_batch::persist_completed(&ws))
                {
                    Ok(_) => Duration::from_secs(5),
                    Err(error) => {
                        eprintln!("Web search history could not be saved: {error}");
                        Duration::from_secs(30)
                    }
                };
        }
        if Instant::now() >= next_discovery_collect {
            if let Err(error) = discovery_task::collect_background(&mut ws) {
                eprintln!("Background discovery collection failed: {error}");
            }
            if let Err(error) = web_search_batch::collect_background() {
                eprintln!("Background web search collection failed: {error}");
            }
            next_discovery_collect = Instant::now() + Duration::from_secs(1);
        }
        if index_worker
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            if let Some(worker) = index_worker.take() {
                let _ = worker.join();
                eprintln!("Background duplicate indexing stopped; using idle indexing");
            }
        }
        if Instant::now() >= next_draft_save {
            next_draft_save = Instant::now()
                + match save_uncollected_draft(&ws) {
                    Ok(_) => Duration::from_secs(1),
                    Err(error) => {
                        eprintln!("Completed draft could not be saved securely: {error}");
                        Duration::from_secs(30)
                    }
                };
        }
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
        if let Err(error) = control_rank::collect_orphaned(session.as_ref().map(|s| s.id.as_str()))
        {
            eprintln!("Could not release expired control advice: {error}");
        }
        if let Err(error) = choice_advice::collect_orphaned(session.as_ref().map(|s| s.id.as_str()))
        {
            eprintln!("Could not release expired choice advice: {error}");
        }
        if let Err(error) =
            control_vision::collect_orphaned(session.as_ref().map(|s| s.id.as_str()))
        {
            eprintln!("Could not release expired control visual review: {error}");
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(error) = serve(&mut stream, &mut ws, &mut session, &token, port) {
                    json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if index_worker.is_none() && Instant::now() >= next_duplicate_index {
                    match ws.index_next_duplicate_step(&duplicate_cursor) {
                        Ok(step) => {
                            duplicate_cursor = step.cursor.unwrap_or_default();
                            let delay = duplicate_index_delay(step.compared, step.more_pending);
                            next_duplicate_index = Instant::now() + delay;
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
    if let Err(error) = web_search_batch::collect_background()
        .and_then(|_| web_search_batch::persist_progress(&ws).map(|_| ()))
        .and_then(|_| web_search_batch::persist_completed(&ws).map(|_| ()))
    {
        eprintln!("Web search history could not be saved during shutdown: {error}");
    }
    if let Some(worker) = index_worker {
        if worker.join().is_err() {
            eprintln!("Background duplicate indexing stopped unexpectedly");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn duplicate_index_scheduling_keeps_comparisons_throttled() {
        use std::time::Duration;
        assert_eq!(
            super::duplicate_index_delay(true, true),
            Duration::from_secs(2)
        );
        assert_eq!(
            super::duplicate_index_delay(false, true),
            Duration::from_millis(200)
        );
        assert_eq!(
            super::duplicate_index_delay(false, false),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn screenshot_field_label_requires_a_unique_usable_schema_field() {
        use waypoint_applications::form_mapping::{FormField, FormSchema};
        let field: FormField = serde_json::from_value(json!({"selector":"#resume","label":"Resume","field_type":"file","required":true,"sensitive":false})).unwrap();
        let mut schema = FormSchema {
            origin: "https://example.invalid".into(),
            fields: vec![field.clone()],
            multi_step: false,
            steps: 1,
        };
        assert_eq!(
            super::capture_field_label(&schema, "#resume").as_deref(),
            Some("Resume")
        );
        schema.fields.push(field);
        assert!(super::capture_field_label(&schema, "#resume").is_none());
        assert!(super::capture_field_label(&schema, "#missing").is_none());
    }
    #[test]
    fn staged_attachment_digest_detects_same_size_changes_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resume.pdf");
        std::fs::write(&path, b"original").unwrap();
        let attachments = vec![(
            "#upload".into(),
            path.clone(),
            "current".into(),
            waypoint_domain::sha256_hex(b"original"),
        )];
        assert!(super::staged_attachments_unchanged(
            &attachments,
            "current",
            "#upload"
        ));
        std::fs::write(&path, b"modified").unwrap();
        assert!(!super::staged_attachments_unchanged(
            &attachments,
            "current",
            "#upload"
        ));
        assert!(super::staged_attachments_unchanged(
            &attachments,
            "old",
            "#upload"
        ));
        std::fs::remove_file(path).unwrap();
        assert!(!super::staged_attachments_unchanged(
            &attachments,
            "current",
            "#upload"
        ));
    }
    #[test]
    fn oversized_staged_document_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oversized.pdf");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(super::MAX_DOCUMENT as u64 + 1)
            .unwrap();
        let digest = waypoint_domain::sha256_hex(&vec![0; super::MAX_DOCUMENT + 1]);
        assert!(!super::staged_file_matches(&path, &digest));
        assert!(!super::staged_file_matches(dir.path(), &digest));
    }
    #[test]
    fn attachment_groups_preserve_order_and_exclude_old_inspections() {
        use std::path::PathBuf;
        let attachments = vec![
            (
                "#files".into(),
                PathBuf::from("one.pdf"),
                "current".into(),
                "a".into(),
            ),
            (
                "#files".into(),
                PathBuf::from("old.pdf"),
                "old".into(),
                "b".into(),
            ),
            (
                "#other".into(),
                PathBuf::from("other.pdf"),
                "current".into(),
                "c".into(),
            ),
            (
                "#files".into(),
                PathBuf::from("two.pdf"),
                "current".into(),
                "d".into(),
            ),
        ];
        assert_eq!(
            super::current_attachment_groups(&attachments, "current"),
            vec![
                (
                    "#files".into(),
                    vec![PathBuf::from("one.pdf"), PathBuf::from("two.pdf")]
                ),
                ("#other".into(), vec![PathBuf::from("other.pdf")]),
            ]
        );
    }
    #[test]
    #[ignore = "starts one browser on a fictional local form"]
    fn closing_review_session_clears_only_its_pending_marker() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use waypoint_applications::driver::BrowserDriver;

        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(base).unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added = manual_job::add(
            &mut ws,
            &json!({"url":"https://example.org/fictional","title":"Role","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let revision = form_task::begin_pending(&ws, &job, "fixture-inspection", 2).unwrap();
        ws.mark_manual_review(&job, "Another review issue", 3)
            .unwrap();
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/form-preview.html");
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let preview = form_preview::suggest_without_model(live, &json!({}), "fixture").unwrap();
        let mut session = Some(ReviewSession {
            id: "fixture-session".into(),
            job_id: job.clone(),
            inspection_task_id: "fixture-inspection".into(),
            inspection_pending_revision: revision,
            url,
            profile_digest: Sha256Digest::of("fixture"),
            preview,
            driver,
            created: Instant::now(),
            ttl: SESSION_TTL,
            filled: false,
            fill_attempted: false,
            completed_selectors: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: None,
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        for (path, body) in [
            (
                "/api/review/resolve",
                json!({"id":job.as_str(),"reason":form_task::pending_reason("fixture-inspection"),"revision":revision}),
            ),
            (
                "/api/review",
                json!({"id":job.as_str(),"required":false,"revision":0}),
            ),
        ] {
            let body = body.to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            let error =
                serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap_err();
            assert!(error.contains("active application review"), "{error}");
        }
        ws.mark_manual_review(&job, "Resolved while active", 4)
            .unwrap();
        let other_revision = ws
            .manual_review_reasons(&job)
            .unwrap()
            .into_iter()
            .find(|entry| entry.reason == "Resolved while active")
            .unwrap()
            .revision;
        let body =
            json!({"id":job.as_str(),"reason":"Resolved while active","revision":other_revision})
                .to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/review/resolve HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("\"removed\":true"));
        let body = json!({"session_id":"fixture-session"}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/form/close HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut session, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"));
        assert!(session.is_none());
        let reasons = ws.manual_review_reasons(&job).unwrap();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].reason, "Another review issue");
        let interrupted = form_task::begin_pending(&ws, &job, "interrupted-fixture", 4).unwrap();
        drop(ws);
        let ws = Workspace::open(dir.path()).unwrap();
        assert!(ws
            .manual_review_reasons(&job)
            .unwrap()
            .iter()
            .any(|reason| reason.revision == interrupted));
    }

    #[test]
    #[ignore = "starts a local browser and verifies a fictional visual-focus request"]
    fn visual_focus_only_accepts_completed_fields_and_keeps_their_value() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::{BrowserCommand, PacketValue};
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/waypoint_browser/tests/fixtures/form_iframe_scroll.html");
        let html = std::fs::read_to_string(fixture).unwrap().replace("</body></html>", "<style>@keyframes fixturemove{from{transform:translateX(0)}to{transform:translateX(110px)}}body.animate #marker{animation:fixturemove 0.35s infinite linear}#marker{position:fixed;top:250px;left:0;width:30px;height:30px;background:#a01}</style><div id='marker'></div><input id='fixture-upload' type='file'><button id='arm-animation' type='button' onclick=\"document.body.classList.add('animate')\">Animate fictional marker</button></body></html>");
        let mut fixture_file = tempfile::Builder::new().suffix(".html").tempfile().unwrap();
        fixture_file.write_all(html.as_bytes()).unwrap();
        fixture_file.flush().unwrap();
        let url = url::Url::from_file_path(fixture_file.path().canonicalize().unwrap())
            .unwrap()
            .to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let selector = live
            .schema
            .fields
            .iter()
            .find(|field| field.label == "Far name")
            .unwrap()
            .selector
            .clone();
        let other = live
            .schema
            .fields
            .iter()
            .find(|field| field.label == "Far email")
            .unwrap()
            .selector
            .clone();
        driver
            .execute(&BrowserCommand::Fill {
                selector: selector.clone(),
                value: PacketValue::Text("Fictional".into()),
            })
            .unwrap();
        driver
            .execute(&BrowserCommand::Fill {
                selector: other.clone(),
                value: PacketValue::Text("fictional@example.invalid".into()),
            })
            .unwrap();
        let mut preview = form_preview::suggest_without_model(
            live,
            &json!({"first_name":"Fictional","email":"fictional@example.invalid"}),
            "fixture",
        )
        .unwrap();
        preview.suggestions.retain(|mapping| {
            mapping.field_selector != selector && mapping.field_selector != other
        });
        preview.suggestions.extend([
            waypoint_applications::form_mapping::ProposedMapping {
                field_selector: selector.clone(),
                value: PacketValue::Text("Fictional".into()),
            },
            waypoint_applications::form_mapping::ProposedMapping {
                field_selector: other.clone(),
                value: PacketValue::Text("fictional@example.invalid".into()),
            },
        ]);
        let mut session = Some(ReviewSession {
            id: "fixture-session".into(),
            job_id: JobIdentityId::new("fixture-job").unwrap(),
            inspection_task_id: "fixture".into(),
            inspection_pending_revision: 0,
            url,
            profile_digest: Sha256Digest::of("fixture"),
            preview,
            driver,
            created: Instant::now(),
            ttl: SESSION_TTL,
            filled: true,
            fill_attempted: true,
            completed_selectors: vec![selector.clone(), other.clone()],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: None,
        });
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id: JobIdentityId::new("fixture-job").unwrap(),
            title: "Role".into(),
            employer: "Fictional".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: None,
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        session.as_mut().unwrap().record = attempt_record::AttemptRecord::start(
            &ws,
            &JobIdentityId::new("fixture-job").unwrap(),
            "focus-fixture",
            1,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let attachment = dir.path().join("fictional-document.pdf");
        std::fs::write(&attachment, b"fictional document").unwrap();
        {
            let current = session.as_mut().unwrap();
            current
                .driver
                .attach_local_files("#fixture-upload", std::slice::from_ref(&attachment))
                .unwrap();
            current.attachments.push((
                "#fixture-upload".into(),
                attachment.clone(),
                "fixture-session".into(),
                Sha256Digest::of("fictional document").as_str().to_owned(),
            ));
        }
        {
            let current = session.as_mut().unwrap();
            form_preview::verify_filled_field(&mut current.driver, &current.preview, &selector)
                .expect("Fixture must start with a verifiable reviewed answer");
        }
        for (step, (selected, allowed)) in [
            ("#other", false),
            (selector.as_str(), true),
            (other.as_str(), true),
            (selector.as_str(), true),
            (selector.as_str(), true),
            (selector.as_str(), true),
        ]
        .into_iter()
        .enumerate()
        {
            if step == 3 {
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .execute(&BrowserCommand::Click {
                        selector: "#arm-change".into(),
                    })
                    .unwrap();
                std::thread::sleep(Duration::from_millis(200));
            }
            if step == 4 {
                let driver = &mut session.as_mut().unwrap().driver;
                driver
                    .execute(&BrowserCommand::Fill {
                        selector: selector.clone(),
                        value: PacketValue::Text("Fictional".into()),
                    })
                    .unwrap();
                driver
                    .execute(&BrowserCommand::Click {
                        selector: "#arm-other".into(),
                    })
                    .unwrap();
                std::thread::sleep(Duration::from_millis(200));
            }
            if step == 5 {
                let current = session.as_mut().unwrap();
                std::fs::remove_file(&attachment).unwrap();
                let driver = &mut current.driver;
                driver
                    .execute(&BrowserCommand::Fill {
                        selector: other.clone(),
                        value: PacketValue::Text("fictional@example.invalid".into()),
                    })
                    .unwrap();
                driver
                    .execute(&BrowserCommand::Click {
                        selector: "#arm-animation".into(),
                    })
                    .unwrap();
            }
            let body = json!({"session_id":"fixture-session","selector":selected}).to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client, "POST /api/form/visual-focus HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            let result = serve(&mut server, &mut ws, &mut session, "fixture-token", port);
            if !allowed {
                assert!(result.unwrap_err().contains("Only fields filled"));
                continue;
            }
            result.unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            assert!(response.contains("200 OK") && response.contains("\"screenshot\""));
            if step == 1 {
                let response_body: Value =
                    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
                assert!(
                    response.contains("\"value_retained\":true"),
                    "freshness={}, verified={}, unverified={}, failed={}",
                    response_body["visual_freshness"],
                    response_body["verified_count"],
                    response_body["unverified_count"],
                    response_body["failed_fields"]
                );
                assert!(response.contains("\"visual_freshness\":\"same\""));
            }
            if step == 2 {
                assert!(response.contains("\"value_retained\":true"));
                assert!(response.contains("\"record_saved\":true"));
                assert!(response.contains("\"failed_attachments\":0"));
                assert!(response.contains("\"unverified_attachments\":0"));
            }
            if step == 3 {
                assert!(response.contains("\"value_retained\":false"));
            }
            if step == 4 {
                assert!(response.contains("\"value_retained\":true"));
                assert!(response.contains("Far email"));
            }
            if step == 5 {
                assert!(
                    response.contains("\"visual_freshness\":\"changed\""),
                    "{response}"
                );
                assert!(response.contains("\"value_retained\":false"));
                assert!(response.contains("\"record_saved\":true"));
                assert!(response.contains("\"failed_attachments\":1"));
                assert!(response.contains("\"unverified_attachments\":0"));
            }
        }
        let record_dir = std::fs::read_dir(dir.path().join("application-records"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let events = std::fs::read_to_string(record_dir.join("events.jsonl")).unwrap();
        let captures: Vec<Value> = events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|event: &Value| event["type"] == "visual_focus_capture")
            .collect();
        for field in [&selector, &other] {
            let event = captures
                .iter()
                .find(|event| event["selector"] == *field && event["value_retained"] == true)
                .unwrap();
            assert!(record_dir
                .join(event["screenshot"].as_str().unwrap())
                .is_file());
        }
        let changed = events
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|event| {
                event["type"] == "visual_focus_capture" && event["visual_freshness"] == "changed"
            })
            .unwrap();
        assert_eq!(changed["failed_attachments"], 1);
        assert!(record_dir
            .join(changed["before_screenshot"].as_str().unwrap())
            .is_file());
        assert!(record_dir
            .join(changed["screenshot"].as_str().unwrap())
            .is_file());
        assert_ne!(
            std::fs::read(record_dir.join(changed["before_screenshot"].as_str().unwrap())).unwrap(),
            std::fs::read(record_dir.join(changed["screenshot"].as_str().unwrap())).unwrap()
        );
        assert!(ws
            .manual_review_reasons(&JobIdentityId::new("fixture-job").unwrap())
            .unwrap()
            .iter()
            .any(|reason| reason.reason.contains("after visual navigation")));
        let current = session.as_mut().unwrap();
        current
            .driver
            .verify_text_value(&selector, "Fictional")
            .unwrap();
        assert!(current
            .driver
            .verify_text_value(&other, "fictional@example.invalid")
            .is_ok());
        assert!(current
            .driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
    }

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
    fn search_plan_becomes_stale_when_an_omitted_fact_changes() {
        let mut ws = Workspace::in_memory().unwrap();
        ws.add_fact("Fictional engineer", FactOrigin::UserAttested)
            .unwrap();
        let long_id = ws
            .add_fact(&format!("{}A", "x".repeat(1000)), FactOrigin::UserAttested)
            .unwrap();
        let (facts, instructions, digest, _) = search_plan_context(&ws).unwrap();
        let coverage = search_plan_coverage(&ws, &facts, &instructions).unwrap();
        assert_eq!(coverage["oversized"], 1);
        ws.correct_fact(&long_id, &format!("{}B", "x".repeat(1000)), 1)
            .unwrap();
        let (new_facts, new_instructions, new_digest, _) = search_plan_context(&ws).unwrap();
        assert_eq!(facts, new_facts);
        assert_eq!(instructions, new_instructions);
        assert_ne!(digest, new_digest);
        assert!(check_search_plan_binding(&ws, &json!({"plan_digest":digest})).is_err());
    }

    #[test]
    fn search_plan_uses_saved_filters_and_binds_unseen_instruction_tail() {
        let ws = Workspace::in_memory().unwrap();
        let initial = format!("{}A", "x".repeat(2100));
        let changed = format!("{}B", "x".repeat(2100));
        ws.save_candidate_profile(&json!({"instructions":initial,"search_remote_only":"on","search_locations":"Berlin\nParis"}).to_string()).unwrap();
        let (_, prompt, digest, _) = search_plan_context(&ws).unwrap();
        assert!(prompt.contains("Remote work is required"));
        assert!(prompt.contains("Berlin; Paris"));
        assert_eq!(prompt.chars().count(), 2000);
        let coverage = search_plan_coverage(&ws, &[], &prompt).unwrap();
        assert!(
            coverage["instructions_total_chars"].as_u64().unwrap()
                > coverage["instructions_used_chars"].as_u64().unwrap()
        );
        ws.save_candidate_profile(&json!({"instructions":changed,"search_remote_only":"on","search_locations":"Berlin\nParis"}).to_string()).unwrap();
        let (_, next_prompt, next_digest, _) = search_plan_context(&ws).unwrap();
        assert_eq!(prompt, next_prompt);
        assert_ne!(digest, next_digest);
        assert!(check_search_plan_binding(&ws, &json!({"plan_digest":digest})).is_err());
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
    #[test]
    fn saved_description_contacts_endpoint_without_capture() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/job","title":"Engineer","employer":"Example","description":"Apply to jobs@example.org"}), 1).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"id":added["id"]}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/listing-salary-evidence HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            result["email_links"]["contacts"][0]["source"],
            "saved_listing"
        );
        assert_eq!(
            result["email_links"]["contacts"][0]["address"],
            "jobs@example.org"
        );
        assert!(result["matches_current_url"].is_null());
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
        let later = json!({"source_url":"https://example.test/job","requested_url":"https://example.test/search-result","captured_at":1750000100,"structured_source_status":"unavailable","search_context":{"engines":["fixture"],"published_date":"2026-10-02","alternative_published_dates":[],"engines_limited":false,"dates_limited":false}}).to_string();
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
        assert_eq!(result["search_context"]["engines"], json!(["fixture"]));
        assert_eq!(result["search_context_matches_url"], false);
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
        let models = result["models"].as_array().unwrap();
        #[cfg(windows)]
        for model in models {
            assert!(matches!(
                model["memory_check"]["state"].as_str(),
                Some("fits_snapshot" | "not_ready")
            ));
            if matches!(
                model["name"].as_str(),
                Some("gemma4:12b-it-qat" | "qwen3.5:9b")
            ) {
                println!("{}: {}", model["name"], model["memory_check"]);
            }
        }
        #[cfg(not(windows))]
        let _ = models;
    }
    #[test]
    fn model_inventory_memory_checks_share_the_startup_gate() {
        let gib = 1024_u64 * 1024 * 1024;
        let tags = json!({"models":[{"name":"small","size":6*gib,"details":{"format":"gguf"}},{"name":"large","size":9*gib,"details":{"format":"gguf"}},{"name":"remote-cloud","size":6*gib,"details":{"format":"gguf"}}]});
        let mut inventory = installed_model_inventory(&tags).unwrap();
        let gpu = json!({"state":"sampled","reported_adapters":1,"adapters":[{"total_bytes":16*gib,"used_bytes":6*gib}]});
        model_inventory_memory_snapshot(&mut inventory, &tags, 24 * gib, &gpu);
        assert_eq!(
            inventory["models"][0]["memory_check"]["state"],
            "fits_snapshot"
        );
        assert_eq!(inventory["models"][1]["memory_check"]["state"], "not_ready");
        assert!(inventory["models"][1]["memory_check"]["reason"]
            .as_str()
            .unwrap()
            .contains("GPU memory"));
        assert_eq!(inventory["models"][2]["memory_check"]["state"], "not_ready");
    }

    #[test]
    fn model_inventory_is_bounded_and_reports_invalid_entries() {
        let mut models: Vec<Value> = (0..101)
            .map(|index| json!({"name":format!("model-{index}"),"size":1024}))
            .collect();
        models[0] = json!({"name":"local","size":1024,"details":{"format":"gguf"}});
        models[1] = json!({"name":"invalid\nname","size":12});
        models[2] = json!({"name":"no-size"});
        let result = installed_model_inventory(&json!({"models":models})).unwrap();
        assert_eq!(result["reported_count"], 101);
        assert_eq!(result["examined"], 100);
        assert_eq!(result["invalid"], 1);
        assert_eq!(result["models"].as_array().unwrap().len(), 99);
        assert!(result["models"][1]["disk_bytes"].is_null());
        assert_eq!(result["models"][0]["format"], "gguf");
        assert!(result["models"][1]["format"].is_null());
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
    fn inference_memory_gate_rejects_pressure_and_invalid_samples() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert!(inference_memory_gate(10 * GIB, 64 * GIB).is_ok());
        assert!(inference_memory_gate(10 * GIB - 1, 64 * GIB)
            .unwrap_err()
            .contains("paused"));
        assert!(inference_memory_gate(4 * GIB, 16 * GIB).is_ok());
        assert!(inference_memory_gate(4 * GIB - 1, 16 * GIB).is_err());
        assert!(inference_memory_gate(1, 0).is_err());
        assert!(inference_memory_gate(17 * GIB, 16 * GIB).is_err());
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
        let (pool, _, before) =
            fit_assessment_digest(&ws, &id, &facts, "", &ws.active_facts_digest().unwrap())
                .unwrap();
        assert!(pool.len() <= 60);
        let omitted = facts.iter().position(|fact| !pool.contains(fact)).unwrap();
        facts[omitted].push_str(" corrected");
        let (_, _, after) =
            fit_assessment_digest(&ws, &id, &facts, "", &ws.active_facts_digest().unwrap())
                .unwrap();
        assert_ne!(before, after);
    }

    #[test]
    fn saved_fit_from_previous_scoring_pipeline_is_stale() {
        let directory = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Engineer","employer":"Example","description":"Python required"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let (facts, instructions) = fit_candidate_context(&ws).unwrap();
        let facts_digest = ws.active_facts_digest().unwrap();
        let (pool, listing, current_digest) =
            fit_assessment_digest(&ws, &id, &facts, &instructions, &facts_digest).unwrap();
        let previous_digest = Sha256Digest::of(&json!({"scoring_version":"criteria-v2-semantic-pool-v46","active_facts_digest":facts_digest,"facts":facts,"pool":pool,"instructions":instructions,"listing":listing,"full_description":"Python required","full_instructions":""}).to_string()).as_str().to_owned();
        assert_ne!(previous_digest, current_digest);
        ws.save_fit_assessment(
            &id,
            json!({"input_digest":previous_digest,"assessment":{"score":0},"model":"fixture"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "stale");
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
        assert_eq!(coverage["description_present"], true);
        assert!(coverage["listing_used_chars"].as_u64().unwrap() < 8000);
        assert!(listing.contains("final requirement"));
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

    #[cfg(all(windows, feature = "sqlcipher"))]
    #[test]
    fn saved_generated_draft_reopens_and_marks_changed_context() {
        let _guard = draft_task::TEST_LOCK.lock().unwrap();
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        let mut ws = Workspace::open_encrypted(dir.path()).unwrap();
        let job = manual_job::add(
            &mut ws,
            &json!({"url":"https://example.org/role","title":"Engineer","employer":"Fixture"}),
            1,
        )
        .unwrap();
        let id = JobIdentityId::new(job["id"].as_str().unwrap()).unwrap();
        let digest = draft_context_digest(&ws, &id, "cv").unwrap();
        draft_task::set_uncollected_fixture(
            "uncollected",
            id.as_str(),
            &digest,
            json!({"kind":"cv","paragraphs":[]}),
        );
        assert!(save_uncollected_draft(&ws).unwrap());
        assert!(!save_uncollected_draft(&ws).unwrap());
        assert_eq!(
            draft_task::poll("uncollected").unwrap()["status"],
            "complete"
        );
        drop(ws);
        let mut ws = Workspace::open_encrypted(dir.path()).unwrap();
        assert!(ws.generated_draft("uncollected", &id).unwrap().is_some());
        let result = json!({"status":"complete","job_id":id.as_str(),"kind":"cv","digest":digest,"draft":{"kind":"cv","paragraphs":[{"text":"Fictional experience","facts":[1]}]}});
        draft_task::set_completed_fixture("draft-fixture", result.clone());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let request = |ws: &mut Workspace, path: &str, body: Value| -> Value {
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = body.to_string();
            write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            serve(&mut server, ws, &mut None, "fixture-token", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        assert_eq!(
            request(
                &mut ws,
                "/api/draft/status",
                json!({"task_id":"draft-fixture"})
            )["saved"],
            true
        );
        assert_eq!(
            request(
                &mut ws,
                "/api/draft/status",
                json!({"task_id":"draft-fixture"})
            )["saved"],
            true
        );
        assert_eq!(
            request(&mut ws, "/api/draft/list", json!({"id":id.as_str()}))["drafts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        drop(ws);
        let mut ws = Workspace::open_encrypted(dir.path()).unwrap();
        draft_task::set_completed_fixture("unrelated", json!({"status":"failed"}));
        assert_eq!(
            load_draft_result(&ws, &id, "draft-fixture").unwrap(),
            result
        );
        let read = request(
            &mut ws,
            "/api/draft/read",
            json!({"id":id.as_str(),"draft_id":"draft-fixture"}),
        );
        assert_eq!(read["current"], true);
        assert_eq!(
            read["result"]["draft"]["paragraphs"][0]["text"],
            "Fictional experience"
        );
        ws.save_candidate_profile(&json!({"document_instructions":"Different style"}).to_string())
            .unwrap();
        assert_eq!(
            request(
                &mut ws,
                "/api/draft/read",
                json!({"id":id.as_str(),"draft_id":"draft-fixture"})
            )["current"],
            false
        );
        draft_task::set_completed_fixture("late-draft", result.clone());
        let late = request(
            &mut ws,
            "/api/draft/status",
            json!({"task_id":"late-draft"}),
        );
        assert_eq!(late["status"], "complete");
        assert_eq!(late["current"], false);
        assert_eq!(late["saved"], true);
        let reopened = request(
            &mut ws,
            "/api/draft/read",
            json!({"id":id.as_str(),"draft_id":"late-draft"}),
        );
        assert_eq!(reopened["current"], false);
        assert_eq!(reopened["result"], result);
    }

    #[test]
    fn draft_request_id_requires_a_bounded_random_identifier() {
        assert!(draft_request_id(&Value::Null).unwrap().is_none());
        assert_eq!(
            draft_request_id(&json!("b-0123456789abcdef0123456789abcdef"))
                .unwrap()
                .as_deref(),
            Some("b-0123456789abcdef0123456789abcdef")
        );
        for invalid in ["b-short", "../file", "b-0123456789ABCDEF0123456789abcdef"] {
            assert!(draft_request_id(&json!(invalid)).is_err());
        }
    }

    #[cfg(all(windows, feature = "sqlcipher"))]
    #[test]
    #[ignore = "requires a resource-gated resident local model"]
    fn live_sequential_drafts_save_for_two_fictional_jobs() {
        let _guard = draft_task::TEST_LOCK.lock().unwrap();
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        assert!(resident_model().unwrap().is_some());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        let mut ws = Workspace::open_encrypted(dir.path()).unwrap();
        ws.add_fact(
            "Built Rust services and tested them with users.",
            FactOrigin::UserAttested,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let request = |ws: &mut Workspace, path: &str, body: Value| -> Value {
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = body.to_string();
            write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            serve(&mut server, ws, &mut None, "fixture-token", port).unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        for (index, title) in ["Rust developer", "Software engineer"].iter().enumerate() {
            let added = manual_job::add(&mut ws, &json!({"url":format!("https://example.org/role-{index}"),"title":title,"employer":"Fictional employer","description":"Build and test Rust services."}), 1).unwrap();
            let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
            ws.set_state(&id, ApplicationState::Shortlisted).unwrap();
            let request_id = format!("b-{index:032x}");
            let started = request(
                &mut ws,
                "/api/draft/start",
                json!({"id":id.as_str(),"kind":"cv","priority_ids":[],"request_id":request_id}),
            );
            let task = started["task_id"].as_str().unwrap();
            assert_eq!(task, request_id);
            let deadline = Instant::now() + Duration::from_secs(240);
            loop {
                let result = request(&mut ws, "/api/draft/status", json!({"task_id":task}));
                if result["status"] != "running" {
                    assert_eq!(result["status"], "complete", "{result}");
                    assert_eq!(result["saved"], true);
                    assert_eq!(result["current"], true);
                    assert!(!result["draft"]["paragraphs"].as_array().unwrap().is_empty());
                    let saved = request(
                        &mut ws,
                        "/api/draft/read",
                        json!({"id":id.as_str(),"draft_id":task}),
                    );
                    assert_eq!(saved["current"], true);
                    break;
                }
                assert!(Instant::now() < deadline, "draft timed out");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
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
                        min_length_utf16: None,
                        pattern: None,
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
                        min_length_utf16: None,
                        pattern: None,
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
                        min_length_utf16: None,
                        pattern: None,
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
                        min_length_utf16: None,
                        pattern: None,
                        autocomplete: None,
                    },
                    FormField {
                        selector: "#month".into(),
                        label: "Employment start month".into(),
                        field_type: FormFieldType::Month,
                        required: false,
                        sensitive: false,
                        editing_mode: None,
                        max_length_utf16: None,
                        min_length_utf16: None,
                        pattern: None,
                        autocomplete: None,
                    },
                    FormField {
                        selector: "#time".into(),
                        label: "Interview time".into(),
                        field_type: FormFieldType::Time,
                        required: false,
                        sensitive: false,
                        editing_mode: None,
                        max_length_utf16: None,
                        min_length_utf16: None,
                        pattern: None,
                        autocomplete: None,
                    },
                    FormField {
                        selector: "#datetime".into(),
                        label: "Appointment".into(),
                        field_type: FormFieldType::DateTimeLocal,
                        required: false,
                        sensitive: false,
                        editing_mode: None,
                        max_length_utf16: None,
                        min_length_utf16: None,
                        pattern: None,
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
                        min_length_utf16: None,
                        pattern: None,
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
        assert!(
            reviewed_form_answers(&json!([{"selector":"#month","value":"2026-11"}]), &preview)
                .is_ok()
        );
        assert!(
            reviewed_form_answers(&json!([{"selector":"#month","value":"2026-13"}]), &preview)
                .is_err()
        );
        assert!(reviewed_form_answers(
            &json!([{"selector":"#month","value":"2026-11-01"}]),
            &preview
        )
        .is_err());
        assert!(
            reviewed_form_answers(&json!([{"selector":"#time","value":"13:30"}]), &preview).is_ok()
        );
        assert!(
            reviewed_form_answers(&json!([{"selector":"#time","value":"24:30"}]), &preview)
                .is_err()
        );
        assert!(reviewed_form_answers(
            &json!([{"selector":"#datetime","value":"2026-11-01T13:30"}]),
            &preview
        )
        .is_ok());
        assert!(reviewed_form_answers(
            &json!([{"selector":"#datetime","value":"2026-02-30T13:30"}]),
            &preview
        )
        .is_err());
        assert!(reviewed_form_answers(
            &json!([{"selector":"#datetime","value":"2026-11-01T13:30Z"}]),
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
                min_length_utf16: None,
                pattern: None,
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
        let mut preview = form_preview::FormPreview {
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
                    min_length_utf16: None,
                    pattern: None,
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
        preview.schema.fields[0].min_length_utf16 = Some(3);
        let error = prepare_reviewed_fill(
            &preview,
            &json!({"answers":[{"selector":"#answer","value":"😀"}]}),
        )
        .err()
        .unwrap();
        assert!(error.contains("3 UTF-16-unit minimum"));
        assert!(prepare_reviewed_fill(
            &preview,
            &json!({"answers":[{"selector":"#answer","value":"A😀"}]})
        )
        .is_ok());
        preview.schema.fields[0].min_length_utf16 = Some(20);
        preview.schema.fields[0].max_length_utf16 = Some(35);
        let long = "Maintained Python backend services for three years.";
        let edited = "Maintained Python backend services";
        assert!(prepare_reviewed_fill(
            &preview,
            &json!({"answers":[{"selector":"#answer","value":long}]})
        )
        .is_err());
        assert!(prepare_reviewed_fill(
            &preview,
            &json!({"answers":[{"selector":"#answer","value":edited}]})
        )
        .is_ok());
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
    fn fit_listing_reports_remote_evidence_and_invalidates_changed_assessment() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let (_, _, before_listing, before) = fit_inputs(&ws, &id).unwrap();
        assert!(before_listing.contains("Remote status is not established"));
        let mut job = ws.job(&id).unwrap().unwrap();
        job.remote = true;
        ws.import_job(job).unwrap();
        let (_, _, after_listing, after) = fit_inputs(&ws, &id).unwrap();
        assert!(after_listing.contains("Source indicates remote work"));
        assert!(after_listing.contains("eligible locations are not established"));
        assert_ne!(before, after);
        assert!(fit_remote_status_text(Some(false)).contains("hybrid arrangement"));
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

    #[test]
    fn fit_coverage_counts_oversized_and_inventory_limited_facts() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/coverage","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        for index in 0..502 {
            ws.add_fact(&format!("Fictional fact {index}"), FactOrigin::UserAttested)
                .unwrap();
        }
        ws.add_fact(&"long fact ".repeat(110), FactOrigin::UserAttested)
            .unwrap();
        ws.add_fact("Unreviewed fictional claim", FactOrigin::ModelInferred)
            .unwrap();
        let (_, _, instructions, listing, _) = fit_assessment_inputs(&ws, &id).unwrap();
        let coverage = fit_source_coverage(&ws, &id, &listing, &instructions).unwrap();
        assert_eq!(coverage["reviewed_facts_total"], 503);
        assert_eq!(coverage["reviewed_facts_bounded"], 500);
        assert_eq!(coverage["reviewed_facts_oversized"], 1);
        assert_eq!(coverage["reviewed_facts_over_limit"], 2);
    }

    #[test]
    fn fit_source_coverage_reports_missing_description() {
        let mut ws = Workspace::in_memory().unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/no-description","title":"Engineer","employer":"Example"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let (_, _, instructions, listing, _) = fit_assessment_inputs(&ws, &id).unwrap();
        let coverage = fit_source_coverage(&ws, &id, &listing, &instructions).unwrap();
        assert_eq!(coverage["description_present"], false);
        let assessment = json!({"source_coverage":coverage,"requirement_audit":{"status":"completed","listing_truncated":false,"criterion_limit_reached":false,"audit_limit_reached":false},"rubric":[{"source":"listing","importance":"required","status":"supported"}]});
        assert_eq!(fit_task::requirement_coverage(&assessment), "incomplete");
    }

    #[test]
    fn long_fit_listing_includes_tail_without_claiming_full_coverage() {
        let mut ws = Workspace::in_memory().unwrap();
        let description = format!(
            "Opening context {} Final requirement: fluent Japanese.",
            "α".repeat(9000)
        );
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/long-fit","title":"Engineer","employer":"Example","description":description}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        let (_, _, instructions, listing, _) = fit_assessment_inputs(&ws, &id).unwrap();
        assert_eq!(listing.chars().count(), 8000);
        assert!(listing.starts_with("Title: Engineer"));
        assert!(listing.contains("Final requirement: fluent Japanese."));
        assert!(listing.contains("Listing text omitted"));
        let coverage = fit_source_coverage(&ws, &id, &listing, &instructions).unwrap();
        assert!(coverage["listing_used_chars"].as_u64().unwrap() < 8000);
        assert_eq!(coverage["listing_sampling"], "beginning_middle_end");
        assert!(
            coverage["listing_used_chars"].as_u64().unwrap()
                < coverage["listing_total_chars"].as_u64().unwrap()
        );
        let assessment = json!({"source_coverage":coverage,"requirement_audit":{"status":"completed","listing_truncated":false,"criterion_limit_reached":false,"audit_limit_reached":false},"rubric":[{"source":"listing","importance":"required","status":"supported"}]});
        assert_eq!(fit_task::requirement_coverage(&assessment), "incomplete");
    }

    #[test]
    fn fit_listing_sampling_never_overlaps_at_the_cutoff() {
        let full = format!("{}B{}", "A".repeat(4000), "C".repeat(4000));
        let (excerpt, used) = fit_listing_excerpt(&full);
        assert_eq!(excerpt.chars().count(), 8000);
        assert!(used < full.chars().count());
        assert_eq!(
            excerpt
                .matches("[Listing text omitted; inspect the full original.]")
                .count(),
            2
        );
        assert!(excerpt.matches('B').count() <= 1);
        assert_eq!(fit_listing_excerpt("short"), ("short".to_string(), 5));
    }

    #[cfg(windows)]
    #[test]
    fn saved_long_fit_reopens_with_sampled_coverage() {
        let directory = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(directory.path()).unwrap();
        let description = format!(
            "{} Final requirement: Japanese.",
            "Fictional background. ".repeat(450)
        );
        let saved = manual_job::add(&mut ws, &json!({"url":"https://example.org/long-saved-fit","title":"Engineer","employer":"Example","description":description}), 1).unwrap();
        let id = JobIdentityId::new(saved["id"].as_str().unwrap()).unwrap();
        let (_, _, _, _, digest) = fit_assessment_inputs(&ws, &id).unwrap();
        ws.save_fit_assessment(
            &id,
            json!({"input_digest":digest,"assessment":{"score":50,"rubric":[]},"model":"fixture"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        drop(ws);
        let ws = Workspace::open(directory.path()).unwrap();
        let result = read_fit(&ws, &id).unwrap();
        assert_eq!(result["status"], "current");
        assert_eq!(
            result["assessment"]["source_coverage"]["listing_sampling"],
            "beginning_middle_end"
        );
        assert!(
            result["assessment"]["source_coverage"]["listing_used_chars"]
                .as_u64()
                .unwrap()
                < result["assessment"]["source_coverage"]["listing_total_chars"]
                    .as_u64()
                    .unwrap()
        );
        assert_eq!(result["assessment"]["requirement_audit_incomplete"], true);
    }

    #[cfg(windows)]
    #[test]
    fn saved_fit_becomes_stale_when_omitted_long_fact_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/role","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        ws.add_fact("Fictional reviewed qualification", FactOrigin::UserAttested)
            .unwrap();
        let (before_pool, _, _, _, before) = fit_assessment_inputs(&ws, &id).unwrap();
        ws.save_fit_assessment(
            &id,
            json!({"input_digest":before,"assessment":{"score":73},"model":"fixture"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "current");
        ws.add_fact(&"Long qualification ".repeat(100), FactOrigin::UserAttested)
            .unwrap();
        let (after_pool, _, _, _, after) = fit_assessment_inputs(&ws, &id).unwrap();
        assert_eq!(before_pool, after_pool);
        assert_ne!(before, after);
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "stale");
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
        let current = read_fit(&ws, &id).unwrap();
        assert_eq!(current["status"], "current");
        assert_eq!(
            current["assessment"]["preference_coverage"],
            "not_requested"
        );
        ws.save_candidate_profile(&json!({"instructions":"Remote only"}).to_string())
            .unwrap();
        assert_eq!(read_fit(&ws, &id).unwrap()["status"], "stale");
    }

    #[test]
    fn fit_batch_reports_recomputed_preference_conflict_without_preference_text() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        let dir = tempfile::tempdir().unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/fictional-role","title":"Engineer","employer":"Example","description":"Fictional role"}), 1).unwrap();
        let id = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
        ws.save_candidate_profile(
            &json!({"instructions":"Fictional remote-only preference"}).to_string(),
        )
        .unwrap();
        let (_, _, _, _, digest) = fit_assessment_inputs(&ws, &id).unwrap();
        let assessment = json!({"score":50,"score_range":{"lower":0,"upper":100},"preference_coverage":"included","preference_audit":{"status":"completed"},"rubric":[{"source":"preference","importance":"required","status":"contradicted","excerpt":"Fictional remote-only preference"},{"source":"listing","importance":"required","status":"unknown","excerpt":"Fictional requirement"}]});
        ws.save_fit_assessment(
            &id,
            json!({"input_digest":digest,"assessment":assessment,"model":"fixture","user_review":{"score":75,"score_range":{"lower":50,"upper":100},"decisions":[{"private":"Fictional private review"}]}})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = json!({"ids":[id.as_str()]}).to_string();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(client, "POST /api/fit/batch HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: fixture-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        serve(&mut server, &mut ws, &mut None, "fixture-token", port).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let json: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            json["results"][0]["preference_coverage"],
            "possible_conflict"
        );
        assert_eq!(json["results"][0]["requirement_coverage"], "needs_review");
        assert_eq!(json["results"][0]["requirement_audit_incomplete"], true);
        assert_eq!(json["results"][0]["review_score"], 75);
        assert_eq!(
            json["results"][0]["review_range"],
            json!({"lower":50,"upper":100})
        );
        assert!(!response.contains("Fictional private review"));
        assert!(!response.contains("Fictional remote-only preference"));
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
            review.document_visual_report(&json!({"status":"complete","material":material,"document_sha256":waypoint_domain::sha256_hex(b"%PDF-fictional"),"context_sha256":document_visual_context(&ws,&id).unwrap().digest,"report":{"complete":true,"pages":1,"findings":[{"page":1,"vision_status":"complete","issues":[{"category":"layout","description":"Fictional overlap"}],"text_geometry":{"status":"checked","overlapping_pairs":1}}]}})).unwrap();
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
        assert!(events.contains("document_visual_followup_needed"));
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
    fn pending_duplicate_index_blocks_external_form_actions() {
        let mut ws = Workspace::in_memory().unwrap();
        assert!(require_duplicate_index_ready(&ws).is_ok());
        let id = JobIdentityId::new("fictional-unindexed").unwrap();
        ws.import_job(waypoint_store::StoredJob {
            id,
            title: "Software Engineering".into(),
            employer: "Example Labs".into(),
            location: String::new(),
            state: ApplicationState::Shortlisted,
            canonical_url: Some("https://example.invalid/job".into()),
            content_hash: None,
            first_seen: None,
            last_seen: None,
            remote: false,
            source: "user_supplied_url".into(),
            requisition_id: None,
            posted_at: None,
        })
        .unwrap();
        assert!(require_duplicate_index_ready(&ws)
            .unwrap_err()
            .contains("still running"));
        assert_eq!(
            ws.index_next_duplicate_job("").unwrap().as_deref(),
            Some("fictional-unindexed")
        );
        assert!(require_duplicate_index_ready(&ws).is_ok());
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
    fn model_status_reports_only_well_formed_digest() {
        let mut report = json!({"models":[{"name":"local","size":10,"size_vram":10,"digest":"a".repeat(64),"context_length":4096}]});
        assert_eq!(
            model_diagnostics(&report).unwrap()["model_digest"],
            "a".repeat(64)
        );
        report["models"][0]["digest"] = json!("not-a-digest");
        assert!(model_diagnostics(&report).unwrap()["model_digest"].is_null());
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

    #[test]
    fn model_vision_capability_requires_a_well_formed_report() {
        assert_eq!(
            vision_capability(&json!({"capabilities":["completion","vision"]})),
            Some(true)
        );
        assert_eq!(
            vision_capability(&json!({"capabilities":["completion"]})),
            Some(false)
        );
        assert_eq!(vision_capability(&json!({})), None);
        assert_eq!(
            vision_capability(&json!({"capabilities":["vision", null]})),
            None
        );
        assert_eq!(
            vision_capability(&json!({"capabilities":vec!["vision";101]})),
            None
        );
    }

    // A TCP read is not an HTTP message boundary. Drain the request body
    // before responding so a split POST cannot be reset on socket close.
    fn read_capability_fixture_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
        use std::io::BufRead;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        let mut length = None;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
        }
        let mut body = vec![0; length.expect("content length")];
        reader.read_exact(&mut body).unwrap();
        (request, body)
    }

    #[test]
    fn model_capability_is_discarded_when_resident_model_changes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let replies = [
                json!({"models":[{"name":"first","size":10,"size_vram":10,"context_length":4096}]}),
                json!({"capabilities":["vision"]}),
                json!({"models":[{"name":"second","size":10,"size_vram":10,"context_length":4096}]}),
            ];
            for (index, body) in replies.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                let (request, _) = read_capability_fixture_request(&mut stream);
                assert!(request.starts_with(if index == 1 {
                    "POST /api/show "
                } else {
                    "GET /api/ps "
                }));
                let body = body.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let result = model_status_with_capabilities_at(addr).unwrap();
        worker.join().unwrap();
        assert_eq!(result["model"], "second");
        assert!(result["vision"].is_null());
    }

    #[test]
    fn model_capability_requires_a_stable_reported_digest() {
        for (before, after, expected) in [
            (Some("a".repeat(64)), Some("a".repeat(64)), json!(true)),
            (Some("a".repeat(64)), Some("b".repeat(64)), Value::Null),
            (None, None, Value::Null),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let worker = std::thread::spawn(move || {
                let report = |digest: Option<String>| json!({"models":[{"name":"local","size":10,"size_vram":10,"context_length":4096,"digest":digest}]});
                for (index, body) in [
                    report(before),
                    json!({"capabilities":["vision"]}),
                    report(after),
                ]
                .into_iter()
                .enumerate()
                {
                    let (mut stream, _) = listener.accept().unwrap();
                    let (request, body_bytes) = read_capability_fixture_request(&mut stream);
                    assert!(request.starts_with(if index == 1 {
                        "POST /api/show "
                    } else {
                        "GET /api/ps "
                    }));
                    if index == 1 {
                        assert_eq!(
                            serde_json::from_slice::<Value>(&body_bytes).unwrap(),
                            json!({"model":"local"})
                        );
                    }
                    let body = body.to_string();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
            });
            let result = model_status_with_capabilities_at(addr).unwrap();
            worker.join().unwrap();
            assert_eq!(result["vision"], expected);
        }
    }

    #[test]
    #[ignore = "requires one preloaded local vision model and resource preflight"]
    fn live_model_status_reports_vision_without_loading_another_model() {
        let model = std::env::var("WAYPOINT_VISION_BENCHMARK_MODEL")
            .unwrap_or_else(|_| "qwen3.5:9b".into());
        let before = resident_model_status().unwrap();
        assert_eq!(before["model"], model);
        let status = model_status_with_capabilities().unwrap();
        assert_eq!(status["model"], model);
        assert_eq!(status["vision"], true);
        assert_eq!(status["context_length"], 4096);
        let after = resident_model_status().unwrap();
        assert_eq!(before, after);
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

    #[cfg(windows)]
    #[test]
    fn reviewed_form_experience_api_persists_and_revokes() {
        let _guard = crate::form_task::TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let path = dir.path().to_owned();
        let worker = std::thread::spawn(move || {
            let mut workspace = Workspace::open(&path).unwrap();
            let mut session = None;
            for _ in 0..17 {
                let (mut stream, _) = listener.accept().unwrap();
                if let Err(error) = serve(
                    &mut stream,
                    &mut workspace,
                    &mut session,
                    "test-token",
                    port,
                ) {
                    json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                }
            }
        });
        let send = |path: &str, value: Value| -> Value {
            let body = value.to_string();
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        };
        let experience = json!({"stage":"enter_information","control":"text","platform":"lever","observation":"value_changed","response":"verify_value_again","outcome":"did_not_help","note":"Check the preceding section"});
        assert!(send(
            "/api/form-experiences/save",
            json!({"experience":experience})
        )["error"]
            .is_string());
        let saved = send(
            "/api/form-experiences/save",
            json!({"confirmed":true,"experience":experience}),
        );
        let listed = send("/api/form-experiences/list", json!({}));
        assert_eq!(listed["experiences"].as_array().unwrap().len(), 1);
        assert_eq!(listed["experiences"][0]["id"], saved["id"]);
        assert_eq!(
            listed["experiences"][0]["experience"]["outcome"],
            "did_not_help"
        );
        let related = send(
            "/api/form-experiences/related",
            json!({"stage":"enter_information","control":"text","platform":"lever"}),
        );
        assert_eq!(related["experiences"][0]["id"], saved["id"]);
        assert_eq!(related["experiences"][0]["same_platform"], true);
        let corrected = send(
            "/api/form-experiences/correct-outcome",
            json!({"confirmed":true,"id":saved["id"],"revision":saved["revision"],"outcome":"helped"}),
        );
        assert_eq!(corrected["revision"], 2);
        assert!(send("/api/form-experiences/correct-outcome", json!({"confirmed":true,"id":saved["id"],"revision":saved["revision"],"outcome":"unknown"}))["error"].is_string());
        let updated = send("/api/form-experiences/list", json!({}));
        assert_eq!(updated["experiences"][0]["experience"]["outcome"], "helped");
        assert_eq!(
            updated["experiences"][0]["experience"]["note"],
            "Check the preceding section"
        );
        assert_eq!(
            updated["experiences"][0]["experience"]["observed_at"],
            listed["experiences"][0]["experience"]["observed_at"]
        );
        let corrected_note = send(
            "/api/form-experiences/correct-note",
            json!({"confirmed":true,"id":saved["id"],"revision":corrected["revision"],"note":"Check the grouping","use_note_in_model":true}),
        );
        assert_eq!(corrected_note["revision"], 3);
        assert!(send(
            "/api/form-experiences/correct-note",
            json!({"confirmed":true,"id":saved["id"],"revision":corrected["revision"],"note":null,"use_note_in_model":false})
        )["error"]
            .is_string());
        assert_eq!(
            send("/api/form-experiences/list", json!({}))["experiences"][0]["experience"]["note"],
            "Check the grouping"
        );
        let disabled = send(
            "/api/form-experiences/set-enabled",
            json!({"confirmed":true,"id":saved["id"],"revision":corrected_note["revision"],"enabled":false}),
        );
        assert_eq!(disabled["revision"], 4);
        assert!(send(
            "/api/form-experiences/related",
            json!({"stage":"enter_information","control":"text","platform":"lever"})
        )["experiences"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(send("/api/form-experiences/set-enabled",json!({"confirmed":true,"id":saved["id"],"revision":corrected_note["revision"],"enabled":true}))["error"].is_string());
        let enabled = send(
            "/api/form-experiences/set-enabled",
            json!({"confirmed":true,"id":saved["id"],"revision":disabled["revision"],"enabled":true}),
        );
        assert_eq!(enabled["revision"], 5);
        assert_eq!(
            send(
                "/api/form-experiences/revoke",
                json!({"confirmed":true,"id":saved["id"],"revision":enabled["revision"]})
            )["ok"],
            true
        );
        assert!(send("/api/form-experiences/list", json!({}))["experiences"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(send(
            "/api/form-experiences/related",
            json!({"stage":"enter_information","control":"text","platform":"lever"})
        )["experiences"]
            .as_array()
            .unwrap()
            .is_empty());
        worker.join().unwrap();
    }

    #[test]
    fn changed_or_revoked_experience_invalidates_snapshot() {
        use waypoint_workspace::form_experience::{
            Control, Experience, ExperienceMatch, Observation, Outcome, Response, Stage,
        };
        let item = ExperienceMatch {
            id: "fixture".into(),
            revision: 1,
            same_platform: false,
            experience: Experience {
                version: 1,
                reviewed: true,
                enabled: true,
                observed_at: 1,
                platform: None,
                stage: Stage::EnterInformation,
                control: Control::Text,
                observation: Observation::ValueChanged,
                response: Response::VerifyValueAgain,
                outcome: Outcome::Unknown,
                note: None,
                use_note_in_model: false,
            },
        };
        let snapshot = vec![(item.id.clone(), item.revision)];
        assert!(experience_snapshot_current(
            &snapshot,
            std::slice::from_ref(&item)
        ));
        let mut changed = item.clone();
        changed.revision = 2;
        assert!(!experience_snapshot_current(
            &snapshot,
            std::slice::from_ref(&changed)
        ));
        let mut disabled = item.clone();
        disabled.experience.enabled = false;
        assert!(!experience_snapshot_current(
            &snapshot,
            std::slice::from_ref(&disabled)
        ));
        assert!(!experience_snapshot_current(&snapshot, &[]));
        let preview = || form_preview::FormPreview {
            schema: waypoint_applications::form_mapping::FormSchema {
                origin: "https://example.org".into(),
                fields: vec![],
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![waypoint_applications::form_mapping::ProposedMapping {
                field_selector: "#fictional".into(),
                value: waypoint_applications::form_mapping::PacketValue::Text("fictional".into()),
            }],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        for current in [
            Some(vec![changed]),
            Some(vec![disabled]),
            Some(vec![]),
            None,
        ] {
            let mut result = preview();
            protect_navigation_experience_snapshot(&mut result, &snapshot, current.as_deref());
            assert!(result.suggestions.is_empty());
            assert!(result
                .manual_review
                .iter()
                .any(|reason| reason.contains("could not be checked")));
        }
        let mut result = preview();
        protect_navigation_experience_snapshot(&mut result, &snapshot, Some(&[item]));
        assert_eq!(result.suggestions.len(), 1);
        assert!(result.manual_review.is_empty());
    }

    #[test]
    fn form_model_status_explains_ineligible_runtime_states() {
        assert_eq!(
            form_model_from_status(&json!({"state":"no_model"})).unwrap(),
            None
        );
        assert_eq!(
            form_model_from_status(&json!({"state":"ready","model":"fixture"})).unwrap(),
            Some("fixture".into())
        );
        for state in [
            "multiple_models",
            "not_fully_gpu",
            "over_budget",
            "incomplete_report",
            "unexpected",
        ] {
            assert!(
                form_model_from_status(&json!({"state":state})).is_err(),
                "{state}"
            );
        }
        assert!(form_model_from_status(&json!({"state":"ready"})).is_err());
    }

    #[test]
    fn experience_platform_hint_uses_exact_source_categories_only() {
        use waypoint_workspace::form_experience::Platform;
        assert_eq!(experience_platform_hint("lever"), Some(Platform::Lever));
        assert_eq!(
            experience_platform_hint("greenhouse"),
            Some(Platform::Greenhouse)
        );
        assert_eq!(experience_platform_hint("ashby"), Some(Platform::Ashby));
        assert_eq!(experience_platform_hint("workday"), Some(Platform::Workday));
        assert_eq!(experience_platform_hint("not-lever"), None);
        assert_eq!(experience_platform_hint("web_search"), None);
        assert_eq!(experience_platform_hint("Lever"), None);
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
        assert!(request(
            "/api/profile",
            "test-token",
            r#"{"search_remote_only":"invalid"}"#
        )
        .contains("Invalid remote-only setting"));
        assert!(request(
            "/api/profile",
            "test-token",
            r#"{"search_remote_only":"on","search_locations":"Berlin\nParis"}"#
        )
        .contains("200 OK"));
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
    fn visual_review_context_tracks_unshown_source_and_candidate_changes() {
        fn workspace(description: &str) -> (Workspace, JobIdentityId) {
            let mut ws = Workspace::in_memory().unwrap();
            let id = JobIdentityId::new("visual-context").unwrap();
            ws.import_manual_job(
                waypoint_store::StoredJob {
                    id: id.clone(),
                    title: "Engineer".into(),
                    employer: "Example".into(),
                    location: "Remote".into(),
                    state: ApplicationState::Shortlisted,
                    canonical_url: None,
                    content_hash: None,
                    first_seen: None,
                    last_seen: None,
                    remote: true,
                    source: "fixture".into(),
                    requisition_id: None,
                    posted_at: None,
                },
                Some(description),
            )
            .unwrap();
            for n in 0..25 {
                ws.add_fact(
                    &format!("Engineer qualification {n}"),
                    FactOrigin::UserAttested,
                )
                .unwrap();
            }
            (ws, id)
        }
        let prefix = "x".repeat(9000);
        let (mut ws, id) = workspace(&format!("{prefix} old"));
        let (other, other_id) = workspace(&format!("{prefix} new"));
        let baseline = document_visual_context(&ws, &id).unwrap();
        let changed_listing = document_visual_context(&other, &other_id).unwrap();
        assert_eq!(baseline.prompt, changed_listing.prompt);
        assert_ne!(baseline.digest, changed_listing.digest);
        ws.add_fact("Unselected extra qualification", FactOrigin::UserAttested)
            .unwrap();
        let changed_fact = document_visual_context(&ws, &id).unwrap();
        assert_eq!(baseline.prompt, changed_fact.prompt);
        assert_ne!(baseline.digest, changed_fact.digest);
        ws.save_candidate_profile(
            &json!({"document_instructions":"Fictional preference"}).to_string(),
        )
        .unwrap();
        let changed_profile = document_visual_context(&ws, &id).unwrap();
        assert_ne!(changed_fact.digest, changed_profile.digest);
        assert!(changed_profile.prompt.contains("Fictional preference"));
    }

    #[test]
    fn saved_visual_review_becomes_stale_after_fact_correction() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let dir = tempfile::tempdir_in(root).unwrap();
        let mut ws = Workspace::open(dir.path()).unwrap();
        let id = JobIdentityId::new("visual-review-stale").unwrap();
        ws.import_manual_job(
            waypoint_store::StoredJob {
                id: id.clone(),
                title: "Engineer".into(),
                employer: "Example".into(),
                location: "Remote".into(),
                state: ApplicationState::Shortlisted,
                canonical_url: None,
                content_hash: None,
                first_seen: None,
                last_seen: None,
                remote: true,
                source: "fixture".into(),
                requisition_id: None,
                posted_at: None,
            },
            Some("Fictional job"),
        )
        .unwrap();
        let fact = ws
            .add_fact("Fictional qualification", FactOrigin::UserAttested)
            .unwrap();
        let attempt = "0123456789abcdef0123456789abcdef";
        let pdf = b"%PDF-fictional";
        let mut record = attempt_record::AttemptRecord::start(&ws, &id, attempt, 1)
            .unwrap()
            .unwrap();
        let material = record.material("resume.pdf", pdf).unwrap();
        let context = document_visual_context(&ws, &id).unwrap();
        record.document_visual_report(&json!({"status":"complete","material":material,"document_sha256":waypoint_domain::sha256_hex(pdf),"context_sha256":context.digest,"report":{"complete":true}})).unwrap();
        assert!(verified_visual_review(&ws, &id, attempt, pdf).is_ok());
        ws.correct_fact(&fact, "Corrected fictional qualification", 2)
            .unwrap();
        assert!(verified_visual_review(&ws, &id, attempt, pdf).is_err());
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
    fn duplicate_decision_rejects_listing_changed_since_state_read() {
        use waypoint_store::StoredJob;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            for id in ["first", "second"] {
                ws.import_manual_job(
                    StoredJob {
                        id: JobIdentityId::new(id).unwrap(),
                        title: "Engineer".into(),
                        employer: "Example".into(),
                        location: "Berlin".into(),
                        state: ApplicationState::Discovered,
                        canonical_url: Some(format!("https://example.org/{id}")),
                        content_hash: None,
                        first_seen: None,
                        last_seen: None,
                        remote: false,
                        source: "user_supplied_url".into(),
                        requisition_id: None,
                        posted_at: None,
                    },
                    Some("Fictional duties"),
                )
                .unwrap();
            }
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
            drop(stream);
            let mut changed = ws
                .job(&JobIdentityId::new("first").unwrap())
                .unwrap()
                .unwrap();
            changed.title = "Senior Engineer".into();
            ws.import_job(changed).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            if let Err(error) = serve(&mut stream, &mut ws, &mut None, "test-token", port) {
                json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            }
            assert!(ws
                .pending_duplicate_pair(
                    &JobIdentityId::new("first").unwrap(),
                    &JobIdentityId::new("second").unwrap()
                )
                .unwrap());
        });
        let mut get = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(get, "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\n\r\n").unwrap();
        let mut response = String::new();
        get.read_to_string(&mut response).unwrap();
        let state: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        let revision = state["duplicates"][0]["revision"].as_i64().unwrap();
        let body =
            json!({"keep_or_first":"first","other":"second","same":false,"revision":revision})
                .to_string();
        let mut post = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(post, "POST /api/duplicates/decide HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let mut response = String::new();
        post.read_to_string(&mut response).unwrap();
        assert!(response.contains("Duplicate pair changed or was already decided"));
        worker.join().unwrap();
    }

    #[test]
    fn incomplete_duplicate_decision_can_be_reviewed_through_endpoint() {
        use waypoint_store::StoredJob;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let root = tempfile::tempdir_in(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            )
            .unwrap();
            let mut ws = Workspace::open(root.path()).unwrap();
            for id in ["first", "second"] {
                ws.import_manual_job(
                    StoredJob {
                        id: JobIdentityId::new(id).unwrap(),
                        title: "Engineer".into(),
                        employer: "Example".into(),
                        location: "Berlin".into(),
                        state: ApplicationState::Discovered,
                        canonical_url: Some(format!("https://example.invalid/{id}")),
                        content_hash: None,
                        first_seen: None,
                        last_seen: None,
                        remote: false,
                        source: "user_supplied_url".into(),
                        requisition_id: None,
                        posted_at: None,
                    },
                    Some("Fictional duties"),
                )
                .unwrap();
            }
            let db = rusqlite::Connection::open(ws.db_path.as_ref().unwrap()).unwrap();
            assert_eq!(
                db.execute(
                    "UPDATE possible_duplicates SET status='same',preferred_job=NULL",
                    []
                )
                .unwrap(),
                1
            );
            drop(db);
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                if let Err(error) = serve(&mut stream, &mut ws, &mut None, "test-token", port) {
                    json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
                }
            }
            assert!(!ws
                .has_duplicate_hold(&JobIdentityId::new("first").unwrap())
                .unwrap());
            assert!(ws
                .has_duplicate_hold(&JobIdentityId::new("second").unwrap())
                .unwrap());
        });
        let request = |method: &str, path: &str, body: &str| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };
        let state = request("GET", "/api/state", "");
        let state: Value = serde_json::from_str(state.split_once("\r\n\r\n").unwrap().1).unwrap();
        let pair = &state["duplicates"][0];
        assert_eq!(pair["status"], "pending");
        assert!(pair["reason"].as_str().unwrap().contains("review it again"));
        let revision = pair["revision"].as_i64().unwrap();
        let body = |revision| {
            json!({"keep_or_first":"first","other":"second","same":true,"revision":revision})
                .to_string()
        };
        assert!(
            request("POST", "/api/duplicates/decide", &body(revision + 1))
                .contains("already decided")
        );
        assert!(request("POST", "/api/duplicates/decide", &body(revision)).contains("\"ok\":true"));
        assert!(
            request("POST", "/api/duplicates/decide", &body(revision)).contains("already decided")
        );
        worker.join().unwrap();
    }

    #[test]
    fn saved_search_filters_reach_state_without_hiding_excluded_jobs() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut ws = Workspace::in_memory().unwrap();
            let body = r#"{"jobs":[{"id":"onsite","title":"Engineer","jobUrl":"https://jobs.ashbyhq.com/ashby/onsite","location":"Paris","isRemote":false},{"id":"unknown","title":"Engineer","jobUrl":"https://jobs.ashbyhq.com/ashby/unknown","location":"Paris"}]}"#;
            let fetcher = waypoint_workspace::StaticFetcher::default()
                .route("https://api.ashbyhq.com/posting-api/job-board/ashby", body);
            assert_eq!(
                ws.discover(&[DiscoveryTarget::new("ashby", "ashby")], &fetcher, 1)
                    .inserted,
                2
            );
            ws.save_candidate_profile(
                &json!({"search_remote_only":"on","search_locations":"Berlin\nParis"}).to_string(),
            )
            .unwrap();
            let (facts, instructions) = fit_candidate_context(&ws).unwrap();
            assert!(facts.is_empty());
            assert!(instructions.contains("Remote work is required"));
            assert!(instructions.contains("Berlin; Paris"));
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let state: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        let rows = state["jobs"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["job"]["requisition_id"], "unknown");
        assert!(rows[0]["unknown_constraints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value
                .as_str()
                .unwrap()
                .contains("remote status not established")));
        assert_eq!(rows[1]["job"]["requisition_id"], "onsite");
        assert!(rows[1]["excluded_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap().contains("not remote")));
        assert_eq!(state["profile"]["search_remote_only"], "on");
        worker.join().unwrap();
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
    fn failed_visual_status_queues_review_without_active_session() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let _guard = visual_review::TEST_TASK_LOCK.lock().unwrap();
            let mut ws = Workspace::in_memory().unwrap();
            let added = manual_job::add(&mut ws, &json!({"url":"https://example.org/failed-visual","title":"Role","employer":"Fixture"}), 1).unwrap();
            let job = JobIdentityId::new(added["id"].as_str().unwrap()).unwrap();
            let revision =
                visual_review::begin_pending_review(&ws, &job, "fixture-failed-visual", 2).unwrap();
            visual_review::set_failed_fixture(
                "fixture-failed-visual",
                "expired-session",
                job.as_str(),
                revision,
            );
            let (mut stream, _) = listener.accept().unwrap();
            serve(&mut stream, &mut ws, &mut None, "test-token", port).unwrap();
            let reasons = ws.manual_review_reasons(&job).unwrap();
            assert!(reasons
                .iter()
                .any(|r| r.reason == visual_review::VISUAL_FAILURE_REASON));
            assert!(!reasons
                .iter()
                .any(|r| r.reason == visual_review::pending_reason("fixture-failed-visual")));
        });
        let body = r#"{"task_id":"fixture-failed-visual"}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/visual-review-status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let result: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["error"], "Fictional vision failure");
        worker.join().unwrap();
    }

    #[test]
    #[ignore = "starts a local browser against a fictional prefilled form"]
    fn existing_value_preflight_preserves_session_for_reviewed_retry() {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::{PacketValue, ProposedMapping};
        use waypoint_store::StoredJob;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let root = tempfile::tempdir_in(base).unwrap();
        let mut ws = Workspace::open(root.path()).unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-existing-value.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(fixture).unwrap().to_string();
        let job = JobIdentityId::new("existing-value-endpoint").unwrap();
        ws.import_job(StoredJob {
            id: job.clone(),
            title: "Fictional role".into(),
            employer: "Example".into(),
            location: String::new(),
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
        let profile = json!({});
        ws.save_candidate_profile(&profile.to_string()).unwrap();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let preview = form_preview::FormPreview {
            schema: live.schema,
            suggestions: vec![
                ProposedMapping {
                    field_selector: "#first".into(),
                    value: PacketValue::Text("First answer".into()),
                },
                ProposedMapping {
                    field_selector: "#second".into(),
                    value: PacketValue::Text("Different answer".into()),
                },
                ProposedMapping {
                    field_selector: "#existing-choice".into(),
                    value: PacketValue::Text("Beta".into()),
                },
                ProposedMapping {
                    field_selector: "#previous-check".into(),
                    value: PacketValue::Bool(false),
                },
                ProposedMapping {
                    field_selector: "#previous-multi".into(),
                    value: PacketValue::TextList(vec!["Beta".into()]),
                },
                ProposedMapping {
                    field_selector: "#previous-radio".into(),
                    value: PacketValue::Text("Beta".into()),
                },
            ],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let mut session = Some(ReviewSession {
            id: "existing-session".into(),
            job_id: job.clone(),
            inspection_task_id: "inspection".into(),
            inspection_pending_revision: 0,
            url,
            profile_digest: Sha256Digest::of(&profile.to_string()),
            preview,
            driver,
            created: Instant::now(),
            ttl: SESSION_TTL,
            filled: false,
            fill_attempted: false,
            completed_selectors: vec![],
            answer_drafts: Default::default(),
            visible: false,
            staged_files: vec![],
            attachments: vec![],
            pending_choice_visual: None,
            selected_choices: vec![],
            last_visual_freshness_key: None,
            record: None,
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let request = |ws: &mut Workspace,
                       session: &mut Option<ReviewSession>,
                       path: &str,
                       body: Value|
         -> Result<Value, String> {
            let body = body.to_string();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(client,"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            serve(&mut server, ws, session, "test-token", port)?;
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1)
                .map_err(|e| e.to_string())
        };
        let fill = |selectors: Value, replacements: Value| json!({"session_id":"existing-session","reviewed":true,"selectors":selectors,"answers":[],"reviewed_replacements":replacements});
        let error = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(json!(["#first", "#second"]), json!([])),
        )
        .unwrap_err();
        assert!(
            error.starts_with("No fields were filled because an existing value needs review:"),
            "{error}"
        );
        let choice_error = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(json!(["#first", "#existing-choice"]), json!([])),
        )
        .unwrap_err();
        assert!(
            choice_error.contains("different selection"),
            "{choice_error}"
        );
        let field = session
            .as_mut()
            .unwrap()
            .preview
            .schema
            .fields
            .iter_mut()
            .find(|field| field.selector == "#second")
            .unwrap();
        field.sensitive = true;
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/text-snapshot",
            json!({"session_id":"existing-session","selector":"#second"})
        )
        .is_err());
        session
            .as_mut()
            .unwrap()
            .preview
            .schema
            .fields
            .iter_mut()
            .find(|field| field.selector == "#second")
            .unwrap()
            .sensitive = false;
        let text_snapshot = request(
            &mut ws,
            &mut session,
            "/api/form/text-snapshot",
            json!({"session_id":"existing-session","selector":"#second"}),
        )
        .unwrap();
        assert_eq!(text_snapshot["text"]["value"], "Existing answer");
        let text_approval =
            json!({"selector":"#second","digest":text_snapshot["digest"],"reviewed":true});
        let text_command =
            |value: &str| waypoint_applications::form_mapping::BrowserCommand::Fill {
                selector: "#second".into(),
                value: PacketValue::Text(value.into()),
            };
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&text_command("Changed by page"))
            .unwrap();
        let stale_text = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(json!(["#first", "#second"]), json!([text_approval.clone()])),
        )
        .unwrap_err();
        assert!(stale_text.contains("changed since"), "{stale_text}");
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&text_command("Existing answer"))
            .unwrap();
        let snapshot = request(
            &mut ws,
            &mut session,
            "/api/form/selection-snapshot",
            json!({"session_id":"existing-session","selector":"#existing-choice"}),
        )
        .unwrap();
        assert_eq!(snapshot["selection"]["label"], "Alpha");
        let approvals =
            json!([{"selector":"#existing-choice","digest":snapshot["digest"],"reviewed":true}]);
        let not_reviewed =
            json!([{"selector":"#existing-choice","digest":snapshot["digest"],"reviewed":false}]);
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(json!(["#first", "#existing-choice"]), not_reviewed)
        )
        .is_err());
        let command =
            |label: &str| waypoint_applications::form_mapping::BrowserCommand::SelectOption {
                selector: "#existing-choice".into(),
                label: label.into(),
            };
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&command("Beta"))
            .unwrap();
        let stale = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(json!(["#first", "#existing-choice"]), approvals.clone()),
        )
        .unwrap_err();
        assert!(stale.contains("changed since"), "{stale}");
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&command("Alpha"))
            .unwrap();
        let choice_snapshot = request(
            &mut ws,
            &mut session,
            "/api/form/choice-snapshot",
            json!({"session_id":"existing-session","selector":"#previous-check"}),
        )
        .unwrap();
        assert_eq!(choice_snapshot["choice"]["checked"], true);
        let checkbox_approval = json!({"selector":"#previous-check","digest":choice_snapshot["digest"],"reviewed":true});
        let checkbox = |value| waypoint_applications::form_mapping::BrowserCommand::Fill {
            selector: "#previous-check".into(),
            value: PacketValue::Bool(value),
        };
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&checkbox(false))
            .unwrap();
        let stale_choice = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(
                json!(["#previous-check"]),
                json!([checkbox_approval.clone()]),
            ),
        )
        .unwrap_err();
        assert!(stale_choice.contains("changed since"), "{stale_choice}");
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&checkbox(true))
            .unwrap();
        let multi_snapshot = request(
            &mut ws,
            &mut session,
            "/api/form/choice-snapshot",
            json!({"session_id":"existing-session","selector":"#previous-multi"}),
        )
        .unwrap();
        assert_eq!(multi_snapshot["choice"]["selections"][0]["label"], "Alpha");
        let multi_approval =
            json!({"selector":"#previous-multi","digest":multi_snapshot["digest"],"reviewed":true});
        let multi =
            |label: &str| waypoint_applications::form_mapping::BrowserCommand::SelectOptions {
                selector: "#previous-multi".into(),
                labels: vec![label.into()],
            };
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&multi("Beta"))
            .unwrap();
        let stale_multi = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(json!(["#previous-multi"]), json!([multi_approval.clone()])),
        )
        .unwrap_err();
        assert!(stale_multi.contains("changed since"), "{stale_multi}");
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&multi("Alpha"))
            .unwrap();
        let radio_snapshot = request(
            &mut ws,
            &mut session,
            "/api/form/choice-snapshot",
            json!({"session_id":"existing-session","selector":"#previous-radio"}),
        )
        .unwrap();
        assert_eq!(radio_snapshot["choice"]["selections"][0]["label"], "Alpha");
        let radio_approval =
            json!({"selector":"#previous-radio","digest":radio_snapshot["digest"],"reviewed":true});
        let radio =
            |label: &str| waypoint_applications::form_mapping::BrowserCommand::SelectRadio {
                selector: "#previous-radio".into(),
                label: label.into(),
            };
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&radio("Beta"))
            .unwrap();
        let stale_radio = request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(json!(["#previous-radio"]), json!([radio_approval.clone()])),
        )
        .unwrap_err();
        assert!(stale_radio.contains("changed since"), "{stale_radio}");
        session
            .as_mut()
            .unwrap()
            .driver
            .execute(&radio("Alpha"))
            .unwrap();
        let combined = json!([text_approval, {"selector":"#existing-choice","digest":snapshot["digest"],"reviewed":true}, checkbox_approval, multi_approval, radio_approval]);
        assert!(request(
            &mut ws,
            &mut session,
            "/api/form/fill",
            fill(
                json!([
                    "#first",
                    "#second",
                    "#existing-choice",
                    "#previous-check",
                    "#previous-multi",
                    "#previous-radio"
                ]),
                combined
            )
        )
        .is_ok());
        let current = session.as_mut().unwrap();
        assert!(current.filled);
        current
            .driver
            .verify_text_value("#first", "First answer")
            .unwrap();
        current
            .driver
            .verify_text_value("#second", "Different answer")
            .unwrap();
        current
            .driver
            .verify_choice(
                &waypoint_applications::form_mapping::BrowserCommand::SelectOption {
                    selector: "#existing-choice".into(),
                    label: "Beta".into(),
                },
            )
            .unwrap();
        current.driver.verify_choice(&checkbox(false)).unwrap();
        current.driver.verify_choice(&multi("Beta")).unwrap();
        current.driver.verify_choice(&radio("Beta")).unwrap();
        assert!(current
            .driver
            .trace
            .iter()
            .all(|event| !event.contains("submitted")));
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
            // This fixture tests record failures and a later page mutation;
            // existing-value conflicts have their own dedicated endpoint test.
            let fixture_copy = workspace_dir.path().join("form-preview-fixture.html");
            let fixture_html = std::fs::read_to_string(&fixture).unwrap();
            assert!(fixture_html.contains("value=\"Old value\""));
            std::fs::write(
                &fixture_copy,
                fixture_html
                    .replace("value=\"Old value\"", "value=\"\"")
                    .replace(
                        "id=\"travel\" type=\"checkbox\" checked",
                        "id=\"travel\" type=\"checkbox\"",
                    )
                    .replace("value=\"onsite\" checked", "value=\"onsite\""),
            )
            .unwrap();
            let url = format!(
                "file:///{}",
                fixture_copy.display().to_string().replace('\\', "/")
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
            assert!(require_duplicate_index_ready(&ws).is_err());
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
            review_record.document_visual_report(&json!({"status":"complete","material":material,"document_sha256":waypoint_domain::sha256_hex(pdf),"context_sha256":document_visual_context(&ws,&id).unwrap().digest,"report":{"complete":true,"findings":[]}})).unwrap();
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
            let inspection_pending_revision =
                form_task::begin_pending(&ws, &id, "fixture-inspection", 1).unwrap();
            let mut session = Some(ReviewSession {
                id: "test-session".into(),
                job_id: id.clone(),
                inspection_task_id: "fixture-inspection".into(),
                inspection_pending_revision,
                url,
                profile_digest: Sha256Digest::of(&profile.to_string()),
                preview,
                driver,
                created: Instant::now(),
                ttl: SESSION_TTL,
                filled: false,
                fill_attempted: false,
                completed_selectors: vec![],
                answer_drafts: Default::default(),
                visible: false,
                staged_files: vec![],
                attachments: vec![],
                pending_choice_visual: None,
                selected_choices: vec![],
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
            assert!(
                error.contains("Duplicate checks are still running"),
                "{error}"
            );
            assert_eq!(
                session
                    .as_mut()
                    .unwrap()
                    .driver
                    .text_value_snapshot("#candidate")
                    .unwrap()
                    .value,
                ""
            );
            json_reply(&mut stream, "400 Bad Request", json!({"error":error}));
            drop(stream);
            assert_eq!(
                ws.index_next_duplicate_job("").unwrap().as_deref(),
                Some(id.as_str())
            );
            assert!(require_duplicate_index_ready(&ws).is_ok());
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
            assert!(session.as_ref().unwrap().fill_attempted);
            let status_listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let status_port = status_listener.local_addr().unwrap().port();
            let mut status_client = TcpStream::connect(("127.0.0.1", status_port)).unwrap();
            let body = "{}";
            write!(status_client,"POST /api/form/session-status HTTP/1.1\r\nHost: 127.0.0.1:{status_port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut status_server, _) = status_listener.accept().unwrap();
            serve(
                &mut status_server,
                &mut ws,
                &mut session,
                "test-token",
                status_port,
            )
            .unwrap();
            drop(status_server);
            let mut status_response = String::new();
            status_client.read_to_string(&mut status_response).unwrap();
            let status: Value =
                serde_json::from_str(status_response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(status["fill_attempted"], true);
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
                Some("true")
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
            assert!(ws
                .manual_review_reasons(&id)
                .unwrap()
                .iter()
                .any(|reason| reason.revision == inspection_pending_revision));
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
                waypoint_domain::sha256_hex(&std::fs::read(&staged_path).unwrap()),
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
            let (x, clip_width, y, clip_height, _, _) =
                visual_review::segment_geometry(width, height, 0).unwrap();
            let image = current
                .driver
                .screenshot_tile(x, y, clip_width, clip_height)
                .unwrap();
            let current_url = current.driver.current_url().unwrap();
            let signature = visual_review::page_signature(&current_url, width, height);
            let pending_revision =
                visual_review::begin_pending_review(&ws, &id, "fixture-vision", now()).unwrap();
            visual_review::set_completed_fixture(
                "fixture-vision",
                &current.id,
                id.as_str(),
                Some(pending_revision),
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
            assert!(!ws
                .manual_review_reasons(&id)
                .unwrap()
                .iter()
                .any(|reason| reason.reason == visual_review::pending_reason("fixture-vision")));
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
                id.as_str(),
                None,
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
            assert!(events.contains("document_visual_followup_needed"));
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
            let verified_index = event_values
                .iter()
                .position(|event| event["type"] == "fill_commands_verified")
                .unwrap();
            let screenshot_index = event_values
                .iter()
                .position(|event| event["type"] == "fields_filled")
                .unwrap();
            assert!(verified_index < screenshot_index);
            assert!(event_values[verified_index]["completed_commands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field["selector"] == "#candidate"));
            assert!(event_values[verified_index]["note"]
                .as_str()
                .unwrap()
                .contains("does not prove submission"));
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
            let answer_path = requested["answer_file"].as_str().unwrap();
            assert!(answer_path.starts_with("answers/"));
            let snapshot: Value =
                serde_json::from_slice(&std::fs::read(record.join(answer_path)).unwrap()).unwrap();
            assert_eq!(snapshot["fields"], requested["fields"]);
            assert_eq!(snapshot["note"], requested["note"]);
            let attempt_meta: Value =
                serde_json::from_slice(&std::fs::read(record.join("attempt.json")).unwrap())
                    .unwrap();
            let answers_listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let answers_port = answers_listener.local_addr().unwrap().port();
            let body =
                json!({"id":id.as_str(),"attempt_id":attempt_meta["attempt_id"]}).to_string();
            let mut client = TcpStream::connect(("127.0.0.1", answers_port)).unwrap();
            write!(client,"POST /api/attempts/answers HTTP/1.1\r\nHost: 127.0.0.1:{answers_port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let (mut server, _) = answers_listener.accept().unwrap();
            let mut empty_session = None;
            serve(
                &mut server,
                &mut ws,
                &mut empty_session,
                "test-token",
                answers_port,
            )
            .unwrap();
            drop(server);
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            let history: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert!(history["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["file"] == answer_path
                    && item["data"]["fields"] == requested["fields"]));
            let incomplete = event_values
                .iter()
                .position(|event| event["type"] == "fill_incomplete")
                .unwrap();
            for event in [&event_values[verified_index], &event_values[incomplete]] {
                assert!(event["answer_file"]
                    .as_str()
                    .unwrap()
                    .starts_with("answers/"));
                let outcome_path = event["outcome_file"].as_str().unwrap();
                assert!(outcome_path.starts_with("answers/"));
                let outcome: Value =
                    serde_json::from_slice(&std::fs::read(record.join(outcome_path)).unwrap())
                        .unwrap();
                assert_eq!(outcome["answer_file"], event["answer_file"]);
                assert_eq!(outcome["completed_commands"], event["completed_commands"]);
            }
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
                .filter(|event| {
                    event["type"] == "page_capture_segment"
                        && event["capture_id"] == event_values[capture_end]["capture_id"]
                })
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
        let pending = r##"{"session_id":"test-session","reviewed":true,"selectors":["#candidate"],"answers":[]}"##;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/fill HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{pending}", pending.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.contains("Duplicate checks are still running"),
            "{response}"
        );
        let body = r#"{"id":"fixture-web","require_no_session":true}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "POST /api/form/inspect-start HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Workspace-Token: test-token\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("Close the active review session"));
        let body = r##"{"session_id":"test-session","reviewed":true,"selectors":["#country"],"answers":[{"selector":"#candidate","value":"Revised","draft_id":"fixture-draft"},{"selector":"#email","value":"manual@example.invalid"},{"selector":"#available","value":"2029-02-28"},{"selector":"#travel","value":true},{"selector":"#onsite","value":"Remote"},{"selector":"#skills","value":["Rust","Python"]}]}"##;
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
