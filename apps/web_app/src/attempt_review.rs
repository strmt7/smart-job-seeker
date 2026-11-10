//! Bounded, read-only activity summaries. No reconstruction of current form state.
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const BYTE_LIMIT: u64 = 2 * 1024 * 1024;
const EVENT_LIMIT: usize = 5000;
const DISPLAY_LIMIT: usize = 40;

#[derive(Serialize)]
pub struct Summary {
    available: bool,
    coverage: &'static str,
    bytes_read: usize,
    file_bytes_at_open: u64,
    parsed_events: usize,
    unreadable_events: usize,
    other_events: usize,
    omitted_activity: usize,
    submission_status: &'static str,
    resolution_status: &'static str,
    activity: VecDeque<Activity>,
}

#[derive(Serialize)]
struct Activity {
    kind: String,
    time: Option<i64>,
    title: &'static str,
    guidance: &'static str,
    needs_review: bool,
}

fn describe(kind: &str) -> Option<(&'static str, &'static str, bool)> {
    Some(match kind {
        "review_interrupted" => ("Review interrupted", "Inspect the application again before continuing. Saved answers and screenshots remain available below.", true),
        "review_session_closed" => ("Browser session closed", "The saved record cannot restore a browser session or establish what the site retained.", false),
        "fill_incomplete" => ("Field entry stopped", "Check the saved answer outcome and screenshots. Some earlier fields may have been entered; do not repeat the batch without inspecting the page.", true),
        "attachment_incomplete" | "attachment_verification_failed" => ("Attachment needs review", "Check which files the site retained before attaching again. A selected file is not proof of acceptance.", true),
        "attachment_network_preflight_unavailable" | "fill_network_preflight_unavailable" | "fill_network_observation_unavailable" => ("Request observation unavailable", "Check the employer site before retrying. The record cannot establish whether requests occurred.", true),
        "non_read_request_observed_during_fill" | "non_read_request_observed_during_attachment" => ("A request was observed", "Its purpose and delivery were not established. Check the site or mailbox before retrying any action.", true),
        "capture_incomplete" | "navigation_capture_incomplete" | "fill_screenshot_failed" | "attachment_viewport_unavailable" => ("Capture incomplete", "Some visual evidence is missing. Inspect the saved screenshots and current page before continuing.", true),
        "visual_page_changed" | "page_capture_changed" | "control_visual_stale" => ("Page changed during review", "Earlier screenshots or field checks may be stale. Inspect the current page again.", true),
        "custom_choice_verification_failed" => ("Choice could not be verified", "Check the selected option and any dependent questions on the current page.", true),
        "material_manifest_incomplete" => ("Document archive incomplete", "Review the saved materials folder. Some document copies or their recorded details may be missing.", true),
        "document_visual_review_missing" | "document_visual_followup_needed" | "document_draft_evidence_review_needed" => ("Document review needed", "Inspect the final PDF, its factual support and any saved visual findings before use.", true),
        "lesson_mapping_invalidated" | "control_advice_stale" | "link_ranking_stale" => ("Earlier advice became stale", "Inspect again and request fresh suggestions. Earlier advice does not authorize a retry.", true),
        "page_inspected" | "fields_inspected" => ("Page inspected", "Observed questions are saved separately. Inspection does not establish that fields were answered.", false),
        "fields_requested" => ("Answers selected", "These are intended answers, not proof that the site retained them.", false),
        "fill_commands_verified" => ("Field commands checked", "These checks describe that moment. Later page changes or server validation may invalidate them.", false),
        "fields_filled" => ("Field entry recorded", "Review the saved answers, screenshots and any later failures. Entry does not establish submission.", false),
        "document_attached" => ("Document selected on form", "Check the attachment and any validation on the current page. Selection does not establish delivery.", false),
        "visual_result" | "control_visual_result" => ("Visual review recorded", "Open the saved findings with the screenshots. Model observations can be incomplete or wrong.", false),
        "navigation_capture_complete" => ("Section capture recorded", "The saved capture describes an earlier section, not its current state.", false),
        _ => return None,
    })
}

/// Called only after the owning job and attempt directory have been verified.
pub fn read(directory: &Path) -> Result<Summary, String> {
    let mut result = Summary {
        available: false,
        coverage: "unavailable",
        bytes_read: 0,
        file_bytes_at_open: 0,
        parsed_events: 0,
        unreadable_events: 0,
        other_events: 0,
        omitted_activity: 0,
        submission_status: "not_assessed",
        resolution_status: "not_assessed",
        activity: VecDeque::new(),
    };
    let path = directory.join("events.jsonl");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(_) => return Err("Cannot inspect saved review activity".into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Invalid review activity file".into());
    }
    let mut file = fs::File::open(path).map_err(|_| "Cannot read saved review activity")?;
    // Bound the handle read too; appended bytes belong to the next refresh.
    let end = file
        .metadata()
        .map_err(|_| "Cannot inspect saved review activity")?
        .len();
    let start = end.saturating_sub(BYTE_LIMIT);
    file.seek(SeekFrom::Start(start))
        .map_err(|_| "Cannot read saved review activity")?;
    let mut bytes = Vec::new();
    file.take(end - start)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read saved review activity")?;
    result.available = true;
    result.file_bytes_at_open = end;
    result.bytes_read = bytes.len();
    let mut partial = start > 0 || bytes.len() as u64 != end - start;
    let mut data = bytes.as_slice();
    if start > 0 {
        data = data
            .iter()
            .position(|b| *b == b'\n')
            .map(|at| &data[at + 1..])
            .unwrap_or(&[]);
    }
    if !data.is_empty() && !data.ends_with(b"\n") {
        partial = true;
        result.unreadable_events += 1;
        data = data
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|at| &data[..=at])
            .unwrap_or(&[]);
    }
    // Keep the newest complete lines, preserving append order rather than trusting timestamps.
    let lines: Vec<_> = data
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() > EVENT_LIMIT {
        partial = true;
    }
    for line in lines.iter().skip(lines.len().saturating_sub(EVENT_LIMIT)) {
        let value: serde_json::Value = match serde_json::from_slice(line) {
            Ok(value) => value,
            Err(_) => {
                result.unreadable_events += 1;
                continue;
            }
        };
        let Some(kind) = value["type"].as_str() else {
            result.unreadable_events += 1;
            continue;
        };
        result.parsed_events += 1;
        let Some((title, guidance, needs_review)) = describe(kind) else {
            result.other_events += 1;
            continue;
        };
        if result.activity.len() == DISPLAY_LIMIT {
            result.activity.pop_front();
            result.omitted_activity += 1;
        }
        result.activity.push_back(Activity {
            kind: kind.into(),
            time: value["time"].as_i64().filter(|n| *n > 0),
            title,
            guidance,
            needs_review,
        });
    }
    result.coverage = if partial || result.unreadable_events > 0 {
        "partial"
    } else {
        "complete"
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn save(root: &Path, values: &[serde_json::Value]) {
        let bytes = values.iter().map(|v| format!("{v}\n")).collect::<String>();
        fs::write(root.join("events.jsonl"), bytes).unwrap();
    }
    #[test]
    fn review_summary_preserves_order_and_never_copies_candidate_values_or_infers_resolution() {
        let temp = tempfile::tempdir().unwrap();
        save(
            temp.path(),
            &[
                json!({"type":"fill_incomplete","time":10,"error":"fictional-private-answer"}),
                json!({"type":"fill_commands_verified","time":2,"fields":[{"value":"fictional-private-answer"}]}),
                json!({"type":"review_session_closed","time":0}),
            ],
        );
        let summary = read(temp.path()).unwrap();
        assert_eq!(
            summary
                .activity
                .iter()
                .map(|v| v.kind.as_str())
                .collect::<Vec<_>>(),
            [
                "fill_incomplete",
                "fill_commands_verified",
                "review_session_closed"
            ]
        );
        assert!(summary.activity[0].needs_review);
        assert_eq!(summary.activity[2].time, None);
        assert_eq!(summary.coverage, "complete");
        let value = serde_json::to_string(&summary).unwrap();
        assert!(!value.contains("fictional-private-answer"));
        assert_eq!(summary.submission_status, "not_assessed");
        assert_eq!(summary.resolution_status, "not_assessed");
    }
    #[test]
    fn review_summary_keeps_recent_events_and_explicit_denominators() {
        let temp = tempfile::tempdir().unwrap();
        let values: Vec<_> = (1..=50)
            .map(|i| json!({"type":"review_interrupted","time":i}))
            .collect();
        save(temp.path(), &values);
        let result = read(temp.path()).unwrap();
        assert_eq!(result.parsed_events, 50);
        assert_eq!(result.omitted_activity, 10);
        assert_eq!(result.activity.len(), 40);
        assert_eq!(result.activity.front().unwrap().time, Some(11));
    }
    #[test]
    fn review_summary_reports_missing_damaged_partial_and_unclassified_evidence() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!read(temp.path()).unwrap().available);
        fs::write(
            temp.path().join("events.jsonl"),
            b"{bad}\n{\"type\":\"future_event\"}\n{\"type\":\"fill_incomplete\"}",
        )
        .unwrap();
        let result = read(temp.path()).unwrap();
        assert_eq!(result.coverage, "partial");
        assert_eq!(result.unreadable_events, 2);
        assert_eq!(result.other_events, 1);
        assert!(result.activity.is_empty());
        fs::remove_file(temp.path().join("events.jsonl")).unwrap();
        fs::create_dir(temp.path().join("events.jsonl")).unwrap();
        assert!(read(temp.path()).is_err());
    }
    #[test]
    fn review_summary_tail_is_byte_bounded_and_includes_latest_issue() {
        let temp = tempfile::tempdir().unwrap();
        let mut data = vec![b'x'; BYTE_LIMIT as usize + 100];
        data.extend_from_slice(b"\n{\"type\":\"capture_incomplete\",\"time\":5}\n");
        fs::write(temp.path().join("events.jsonl"), data).unwrap();
        let result = read(temp.path()).unwrap();
        assert_eq!(result.bytes_read, BYTE_LIMIT as usize);
        assert_eq!(result.coverage, "partial");
        assert_eq!(result.activity[0].kind, "capture_incomplete");
    }
    #[test]
    fn review_summary_event_limit_prefers_latest_complete_lines() {
        let temp = tempfile::tempdir().unwrap();
        let mut values: Vec<_> = (0..5000).map(|_| json!({"type":"other"})).collect();
        values.push(json!({"type":"attachment_incomplete"}));
        save(temp.path(), &values);
        let result = read(temp.path()).unwrap();
        assert_eq!(result.parsed_events, 5000);
        assert_eq!(result.other_events, 4999);
        assert_eq!(result.coverage, "partial");
        assert_eq!(result.activity[0].kind, "attachment_incomplete");
    }
}
