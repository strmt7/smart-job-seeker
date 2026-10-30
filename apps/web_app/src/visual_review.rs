//! Bounded, local-only advisory transcription of one form-page segment.
use serde_json::{json, Value};
use std::sync::Mutex;

struct Task {
    id: String,
    session: String,
    page_signature: String,
    segment: usize,
    half: Option<usize>,
    screenshot_hash: String,
    result: Option<Result<Value, String>>,
    completed: Option<std::time::Instant>,
    record_saved: bool,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

#[cfg(test)]
pub static TEST_TASK_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub fn set_completed_fixture(id: &str, session: &str, signature: &str, image: &str) {
    *TASK.lock().unwrap() = Some(Task {
        id: id.into(),
        session: session.into(),
        page_signature: signature.into(),
        segment: 0,
        half: None,
        screenshot_hash: waypoint_domain::Sha256Digest::of(image).as_str().into(),
        result: Some(Ok(json!({"observations":[],"comparisons":[]}))),
        completed: Some(std::time::Instant::now()),
        record_saved: false,
    });
}

pub fn page_signature(url: &str, width: f64, height: f64) -> waypoint_domain::Sha256Digest {
    waypoint_domain::Sha256Digest::of(&json!({"url":url,"width":width,"height":height}).to_string())
}

pub fn image_freshness(original_hash: Option<&str>, fresh_image: Option<&str>) -> &'static str {
    match (original_hash, fresh_image) {
        (Some(original), Some(image))
            if original == waypoint_domain::Sha256Digest::of(image).as_str() =>
        {
            "same"
        }
        (Some(_), Some(_)) => "changed",
        _ => "unavailable",
    }
}

/// Check captures on both sides of live-value verification. An observed change
/// remains a change even if the page later returns to its original appearance.
pub fn interval_freshness(
    original_hash: Option<&str>,
    before: Option<&str>,
    after: Option<&str>,
) -> &'static str {
    let first = image_freshness(original_hash, before);
    let last = image_freshness(original_hash, after);
    if first == "changed" || last == "changed" {
        "changed"
    } else if first == "same" && last == "same" {
        "same"
    } else {
        "unavailable"
    }
}

/// Identify an observed freshness state without storing screenshot bytes here.
/// Re-polling the same task after another visual change must create a new
/// record, while an identical recapture need not duplicate it.
pub fn freshness_record_key(task_id: &str, freshness: &str, image: Option<&str>) -> String {
    let image_hash = image.map(|data| waypoint_domain::Sha256Digest::of(data).as_str().to_owned());
    waypoint_domain::Sha256Digest::of(
        &json!({"task_id":task_id,"freshness":freshness,"image_hash":image_hash}).to_string(),
    )
    .as_str()
    .to_owned()
}

/// Limit reviewed answers independently of the number of observations per image.
pub const MAX_EXPECTED_FIELDS: usize = 200;

pub struct ExpectedFields {
    pub values: Vec<(String, String)>,
    pub ambiguous_labels: Vec<String>,
}

pub fn expected_from_preview(
    preview: &crate::form_preview::FormPreview,
    completed: &[String],
) -> (ExpectedFields, bool) {
    let (values, truncated) = bounded_expected(
        preview
            .suggestions
            .iter()
            .filter(|mapping| completed.contains(&mapping.field_selector))
            .filter_map(|mapping| {
                let field = preview
                    .schema
                    .fields
                    .iter()
                    .find(|field| field.selector == mapping.field_selector)?;
                expected_value(&mapping.value).map(|value| (field.label.clone(), value))
            }),
    );
    let repeated: std::collections::HashSet<_> =
        preview.repeated_label_selectors().into_iter().collect();
    let ambiguous_labels = preview
        .schema
        .fields
        .iter()
        .filter(|field| repeated.contains(&field.selector))
        .map(|field| field.label.clone())
        .collect();
    (
        ExpectedFields {
            values,
            ambiguous_labels,
        },
        truncated,
    )
}

fn normalized_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A screenshot cannot identify which of several similarly labeled fields is
/// shown, even if only one of them was selected for filling.
fn mark_ambiguous(result: &mut Value, ambiguous_labels: &[String]) {
    let labels: std::collections::HashSet<_> = ambiguous_labels
        .iter()
        .map(|label| normalized_label(label))
        .collect();
    if let Some(comparisons) = result["comparisons"].as_array_mut() {
        for item in comparisons {
            if item["label"]
                .as_str()
                .is_some_and(|label| labels.contains(&normalized_label(label)))
            {
                item["status"] = json!("unobserved");
                item["ambiguous"] = json!(true);
            }
        }
    }
}

pub fn bounded_expected(
    values: impl IntoIterator<Item = (String, String)>,
) -> (Vec<(String, String)>, bool) {
    let mut values: Vec<_> = values.into_iter().take(MAX_EXPECTED_FIELDS + 1).collect();
    let truncated = values.len() > MAX_EXPECTED_FIELDS;
    values.truncate(MAX_EXPECTED_FIELDS);
    (values, truncated)
}

/// Values used for advisory screenshot comparison, not proof of a valid fill.
pub fn expected_value(value: &waypoint_applications::form_mapping::PacketValue) -> Option<String> {
    use waypoint_applications::form_mapping::PacketValue;
    let text = match value {
        PacketValue::Text(value) | PacketValue::Date(value) => value.clone(),
        PacketValue::Bool(value) => if *value { "checked" } else { "unchecked" }.into(),
        PacketValue::TextList(values) if values.len() <= 100 => values.join(", "),
        _ => return None,
    };
    (text.len() <= 1000).then_some(text)
}

const MAX_OBSERVATIONS: usize = 20;

fn validate(output: &Value) -> Result<Value, String> {
    let observations = output
        .get("observations")
        .and_then(Value::as_array)
        .filter(|items| items.len() <= MAX_OBSERVATIONS)
        .ok_or("Vision model did not return bounded observations")?;
    let mut clean = Vec::new();
    for item in observations {
        let label = item
            .get("label")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && s.len() <= 200)
            .ok_or("Vision observation has an invalid label")?;
        let value = item
            .get("visible_value")
            .and_then(Value::as_str)
            .filter(|s| s.len() <= 1000)
            .ok_or("Vision observation has an invalid value")?;
        clean.push(json!({"label":label,"visible_value":value}));
    }
    Ok(json!({"observations":clean}))
}

fn compare(observed: &Value, expected: &[(String, String)]) -> Value {
    let items = observed["observations"]
        .as_array()
        .expect("validated observations");
    let comparisons: Vec<_> = expected
        .iter()
        .map(|(label, value)| {
            let normalized = normalized_label(label);
            let matches: Vec<_> = items
                .iter()
                .filter(|item| {
                    item["label"]
                        .as_str()
                        .is_some_and(|name| normalized_label(name) == normalized)
                })
                .collect();
            let unique_expected = expected
                .iter()
                .filter(|(name, _)| normalized_label(name) == normalized)
                .count()
                == 1;
            let status = if !unique_expected || matches.len() != 1 {
                "unobserved"
            } else if matches[0]["visible_value"].as_str() == Some(value.as_str()) {
                "matches_transcription"
            } else {
                "differs_from_transcription"
            };
            json!({"label":label,"expected":value,"status":status})
        })
        .collect();
    json!({"observations":items,"comparisons":comparisons,"observation_limit_reached":items.len() == MAX_OBSERVATIONS})
}

/// A matching transcription alone may be a placeholder rather than an entered
/// value. Independently check matching fields against the live DOM. This does
/// not make the vision model's interpretation or the site trustworthy.
pub fn crosscheck_live_values(
    driver: &mut dyn waypoint_applications::driver::BrowserDriver,
    preview: &crate::form_preview::FormPreview,
    completed: &[String],
    result: &mut Value,
) -> (usize, usize) {
    use waypoint_applications::form_mapping::{BrowserCommand, FormFieldType, PacketValue};
    let Some(comparisons) = result["comparisons"].as_array_mut() else {
        return (0, 0);
    };
    let (mut verified, mut unconfirmed) = (0, 0);
    let mut stop = false;
    for item in comparisons {
        if item["status"] != "matches_transcription" {
            continue;
        }
        let label = item["label"].as_str().unwrap_or("");
        let matching: Vec<_> = preview
            .schema
            .fields
            .iter()
            .filter(|field| normalized_label(&field.label) == normalized_label(label))
            .collect();
        let check = if !stop && matching.len() == 1 {
            let field = matching[0];
            let mapping = preview.suggestions.iter().find(|mapping| {
                mapping.field_selector == field.selector
                    && completed.contains(&field.selector)
                    && expected_value(&mapping.value).as_deref() == item["expected"].as_str()
            });
            match mapping.map(|mapping| (&field.field_type, &mapping.value)) {
                Some((
                    FormFieldType::Text | FormFieldType::Email | FormFieldType::TextArea,
                    PacketValue::Text(value),
                )) => driver.verify_text_value(&field.selector, value),
                Some((FormFieldType::Date, PacketValue::Date(value))) => {
                    driver.verify_text_value(&field.selector, value)
                }
                Some((FormFieldType::Checkbox, PacketValue::Bool(value))) => {
                    driver.verify_choice(&BrowserCommand::Fill {
                        selector: field.selector.clone(),
                        value: PacketValue::Bool(*value),
                    })
                }
                Some((FormFieldType::Select { .. }, PacketValue::Text(value))) => driver
                    .verify_choice(&BrowserCommand::SelectOption {
                        selector: field.selector.clone(),
                        label: value.clone(),
                    }),
                Some((FormFieldType::Radio { .. }, PacketValue::Text(value))) => driver
                    .verify_choice(&BrowserCommand::SelectRadio {
                        selector: field.selector.clone(),
                        label: value.clone(),
                    }),
                Some((FormFieldType::MultiSelect { .. }, PacketValue::TextList(value))) => driver
                    .verify_choice(&BrowserCommand::SelectOptions {
                        selector: field.selector.clone(),
                        labels: value.clone(),
                    }),
                _ => Err(waypoint_applications::driver::DriverError(
                    "Live value unavailable".into(),
                )),
            }
            .is_ok()
        } else {
            false
        };
        if check {
            item["dom_status"] = json!("matches");
            verified += 1;
        } else {
            item["dom_status"] = json!("unconfirmed");
            item["status"] = json!("unobserved");
            unconfirmed += 1;
            // A disconnected browser or stale page can make every call slow.
            stop = true;
        }
    }
    (verified, unconfirmed)
}

/// Derive one bounded clip without scrolling or clicking. Larger pages are explicitly incomplete.
pub fn segment_geometry(
    width: f64,
    height: f64,
    index: usize,
) -> Result<(f64, f64, f64, usize, bool), String> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("Invalid page dimensions".into());
    }
    let capped_height = height.min(12000.0);
    let total = (capped_height / 2000.0).ceil() as usize;
    if index >= total {
        return Err("Visual segment is outside the page".into());
    }
    let y = (index as f64) * 2000.0;
    Ok((
        width.min(2048.0),
        y,
        (capped_height - y).min(2000.0),
        total,
        width <= 2048.0 && height <= 12000.0,
    ))
}

/// A dense segment can be rechecked in two bounded, non-overlapping halves.
pub fn segment_half_geometry(
    width: f64,
    height: f64,
    index: usize,
    half: Option<usize>,
) -> Result<(f64, f64, f64, usize, bool), String> {
    let (width, y, height, total, complete) = segment_geometry(width, height, index)?;
    match half {
        None => Ok((width, y, height, total, complete)),
        Some(0) => Ok((width, y, height / 2.0, total, complete)),
        Some(1) => Ok((width, y + height / 2.0, height / 2.0, total, complete)),
        _ => Err("Invalid visual segment subdivision".into()),
    }
}

pub fn start(
    id: String,
    session: String,
    capture: (String, usize, Option<usize>),
    model: String,
    image: String,
    expected: ExpectedFields,
    result_path: Option<std::path::PathBuf>,
) -> Result<(), String> {
    if image.len() > 4 * 1024 * 1024 || image.is_empty() {
        return Err("Screenshot is unavailable or too large for local visual review".into());
    }
    let lease = crate::doc_suggest::lease()?;
    let mut task = TASK.lock().map_err(|_| "Visual review unavailable")?;
    if task.as_ref().is_some_and(|t| {
        t.completed
            .is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(600))
    }) {
        *task = None;
    }
    if task.is_some() {
        return Err("A visual review is running or awaiting its result".into());
    }
    *task = Some(Task {
        id: id.clone(),
        session,
        page_signature: capture.0,
        segment: capture.1,
        half: capture.2,
        screenshot_hash: waypoint_domain::Sha256Digest::of(&image)
            .as_str()
            .to_owned(),
        result: None,
        completed: None,
        record_saved: false,
    });
    drop(task);
    std::thread::spawn(move || {
        let _lease = lease;
        let result = std::panic::catch_unwind(|| -> Result<Value, String> {
            if crate::resident_model()?.as_deref() != Some(model.as_str()) {
                return Err("Resident model changed; visual review was not started".into());
            }
            let mut result = analyze(
                "127.0.0.1:11434"
                    .parse()
                    .map_err(|_| "Invalid local runtime address")?,
                &model,
                &image,
                &expected.values,
            )?;
            mark_ambiguous(&mut result, &expected.ambiguous_labels);
            Ok(result)
        })
        .unwrap_or_else(|_| Err("Visual review stopped unexpectedly".into()));
        let record_saved = result_path.as_deref().is_some_and(|path| {
            crate::attempt_record::write_visual_result(path, &id, &result, crate::now()).is_ok()
        });
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|t| t.id == id) {
                task.record_saved = record_saved;
                task.result = Some(result);
                task.completed = Some(std::time::Instant::now());
            }
        }
    });
    Ok(())
}

pub(crate) fn analyze(
    addr: std::net::SocketAddr,
    model: &str,
    image: &str,
    expected: &[(String, String)],
) -> Result<Value, String> {
    analyze_with_context(addr, model, image, expected, None)
}

fn analyze_with_context(
    addr: std::net::SocketAddr,
    model: &str,
    image: &str,
    expected: &[(String, String)],
    context: Option<u32>,
) -> Result<Value, String> {
    let prompt = "Transcribe up to 20 visible form field labels and their displayed values in the image. For a visible checkbox use checked or unchecked; for a multi-select list the visible selected labels separated by comma and space. Return JSON: {\"observations\":[{\"label\":\"...\",\"visible_value\":\"...\"}]}. Do not infer hidden or unreadable values. An empty array is allowed.";
    let result = generate_vision_with_context(addr, model, image, prompt, context)?;
    let observed = validate(&result)?;
    Ok(compare(&observed, expected))
}

pub(crate) fn generate_vision(
    addr: std::net::SocketAddr,
    model: &str,
    image: &str,
    prompt: &str,
) -> Result<Value, String> {
    generate_vision_with_context(addr, model, image, prompt, None)
}

fn generate_vision_with_context(
    addr: std::net::SocketAddr,
    model: &str,
    image: &str,
    prompt: &str,
    context: Option<u32>,
) -> Result<Value, String> {
    if image.is_empty() || image.len() > 4 * 1024 * 1024 || prompt.len() > 24_000 {
        return Err("Vision input exceeds limits".into());
    }
    let context = match context {
        Some(value) if (1024..=131_072).contains(&value) => value,
        Some(_) => return Err("Invalid visual review context size".into()),
        None => waypoint_inference::ollama::resident_context(addr, model)?,
    };
    let details = serde_json::to_vec(&json!({"model":model})).map_err(|e| e.to_string())?;
    let (status, details) =
        waypoint_inference::ollama::http_request(addr, "POST", "/api/show", Some(&details), 3000)?;
    let details: Value =
        serde_json::from_slice(&details).map_err(|_| "Cannot verify local model capabilities")?;
    if status != 200
        || !details["capabilities"]
            .as_array()
            .is_some_and(|items| items.iter().any(|v| v.as_str() == Some("vision")))
    {
        return Err("The resident model did not report vision capability; inspect manually".into());
    }
    let mut body = json!({"model":model,"prompt":prompt,"images":[image],"stream":false,"format":"json","think":false,"options":{"num_predict":1024,"temperature":0}});
    body["options"]["num_ctx"] = context.into();
    let bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let (status, data) = waypoint_inference::ollama::http_request(
        addr,
        "POST",
        "/api/generate",
        Some(&bytes),
        120_000,
    )?;
    if status != 200 {
        return Err(format!("Local vision model returned HTTP {status}"));
    }
    let response: Value =
        serde_json::from_slice(&data).map_err(|_| "Invalid vision model response")?;
    if response["done"] != true || response["done_reason"] != "stop" {
        return Err("Visual review did not finish; inspect manually".into());
    }
    let text = response["response"]
        .as_str()
        .filter(|s| s.len() <= 32_000)
        .ok_or("Invalid vision output")?;
    serde_json::from_str(text).map_err(|_| "Vision output was not JSON".into())
}

pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Visual review unavailable")?;
    let Some(task) = tasks.as_mut().filter(|t| t.id == id) else {
        let completed = COMPLETED.lock().map_err(|_| "Visual review unavailable")?;
        return completed
            .as_ref()
            .filter(|(task_id, _)| task_id == id)
            .map(|(_, result)| result.clone())
            .ok_or_else(|| "Visual review not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running","session_id":task.session}));
    };
    let session = task.session.clone();
    let page_signature = task.page_signature.clone();
    let segment = task.segment;
    let half = task.half;
    let screenshot_hash = task.screenshot_hash.clone();
    let record_saved = task.record_saved;
    let output = match result {
        Ok(observed) => {
            json!({"status":"complete","session_id":session,"page_signature":page_signature,"segment":segment,"half":half,"screenshot_hash":screenshot_hash,"visual_review":observed,"record_saved":record_saved})
        }
        Err(error) => {
            json!({"status":"failed","session_id":session,"error":error,"record_saved":record_saved})
        }
    };
    *COMPLETED.lock().map_err(|_| "Visual review unavailable")? = Some((id.into(), output.clone()));
    *tasks = None;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_spans_the_live_value_checks() {
        let hash = waypoint_domain::Sha256Digest::of("original");
        assert_eq!(
            interval_freshness(Some(hash.as_str()), Some("original"), Some("original")),
            "same"
        );
        assert_eq!(
            interval_freshness(Some(hash.as_str()), Some("changed"), Some("original")),
            "changed"
        );
        assert_eq!(
            interval_freshness(Some(hash.as_str()), Some("original"), Some("changed")),
            "changed"
        );
        assert_eq!(
            interval_freshness(Some(hash.as_str()), Some("changed"), None),
            "changed"
        );
        assert_eq!(
            interval_freshness(Some(hash.as_str()), Some("original"), None),
            "unavailable"
        );
        assert_eq!(
            interval_freshness(None, Some("original"), Some("original")),
            "unavailable"
        );
    }

    #[test]
    fn live_value_check_downgrades_false_visual_matches_and_stops_after_failure() {
        use waypoint_applications::driver::{BrowserDriver, DriverError, LiveForm, SiteAck};
        use waypoint_applications::form_mapping::{
            BrowserCommand, FormField, FormFieldType, FormSchema, PacketValue, ProposedMapping,
        };
        struct Driver {
            calls: usize,
        }
        impl BrowserDriver for Driver {
            fn open(&mut self, _: &str) -> Result<LiveForm, DriverError> {
                unreachable!()
            }
            fn execute(&mut self, _: &BrowserCommand) -> Result<(), DriverError> {
                unreachable!()
            }
            fn commit(&mut self) -> Result<SiteAck, DriverError> {
                unreachable!()
            }
            fn verify_text_value(&mut self, _: &str, _: &str) -> Result<(), DriverError> {
                self.calls += 1;
                Err(DriverError("field changed".into()))
            }
        }
        let fields = ["Email", "Name"]
            .iter()
            .map(|name| FormField {
                selector: format!("#{name}"),
                label: (*name).into(),
                field_type: FormFieldType::Text,
                required: true,
                sensitive: false,
                editing_mode: None,
                max_length_utf16: None,
                autocomplete: None,
            })
            .collect();
        let suggestions = ["Email", "Name"]
            .iter()
            .map(|name| ProposedMapping {
                field_selector: format!("#{name}"),
                value: PacketValue::Text("fixture".into()),
            })
            .collect();
        let preview = crate::form_preview::FormPreview {
            schema: FormSchema {
                origin: "https://example.invalid".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions,
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let mut result = json!({"comparisons":[{"label":"Email","expected":"fixture","status":"matches_transcription"},{"label":"Name","expected":"fixture","status":"matches_transcription"}]});
        let mut driver = Driver { calls: 0 };
        assert_eq!(
            crosscheck_live_values(
                &mut driver,
                &preview,
                &["#Email".into(), "#Name".into()],
                &mut result
            ),
            (0, 2)
        );
        assert_eq!(driver.calls, 1);
        assert_eq!(result["comparisons"][0]["dom_status"], "unconfirmed");
        assert_eq!(result["comparisons"][1]["status"], "unobserved");
    }

    #[test]
    fn freshness_record_identity_tracks_the_actual_recapture() {
        let original = freshness_record_key("task", "same", Some("image-a"));
        assert_eq!(
            original,
            freshness_record_key("task", "same", Some("image-a"))
        );
        assert_ne!(
            original,
            freshness_record_key("task", "changed", Some("image-b"))
        );
        assert_ne!(
            freshness_record_key("task", "changed", Some("image-b")),
            freshness_record_key("task", "changed", Some("image-c"))
        );
        assert_ne!(
            original,
            freshness_record_key("another-task", "same", Some("image-a"))
        );
        assert_ne!(original, freshness_record_key("task", "unavailable", None));
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_visual_review_does_not_verify_placeholder_as_entered_value() {
        use waypoint_applications::driver::BrowserDriver;
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fictional-visual-placeholder.html");
        let url = format!(
            "file:///{}",
            fixture.display().to_string().replace('\\', "/")
        );
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        driver.open(&url).unwrap();
        let image = driver.screenshot_base64().unwrap();
        let expected = vec![
            ("Full name".into(), "Avery Example".into()),
            ("Email".into(), "avery@example.invalid".into()),
        ];
        let mut result = analyze_with_context(
            "127.0.0.1:11434".parse().unwrap(),
            "qwen3.5:9b",
            &image,
            &expected,
            Some(4096),
        )
        .unwrap();
        let schema = driver.inspect_current().unwrap().schema;
        let preview = crate::form_preview::FormPreview {
            schema,
            suggestions: vec![
                waypoint_applications::form_mapping::ProposedMapping {
                    field_selector: "#name".into(),
                    value: waypoint_applications::form_mapping::PacketValue::Text(
                        "Avery Example".into(),
                    ),
                },
                waypoint_applications::form_mapping::ProposedMapping {
                    field_selector: "#email".into(),
                    value: waypoint_applications::form_mapping::PacketValue::Text(
                        "avery@example.invalid".into(),
                    ),
                },
            ],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let (verified, unconfirmed) = crosscheck_live_values(
            &mut driver,
            &preview,
            &["#name".into(), "#email".into()],
            &mut result,
        );
        println!("live DOM checks: verified={verified}, unconfirmed={unconfirmed}");
        println!("fictional placeholder comparison: {result}");
        assert!(
            result["comparisons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["label"] == "Email" && item["status"] != "matches_transcription"),
            "placeholder must not verify an entered value: {result}"
        );
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_visual_review_dense_form_checks_late_mismatches() {
        use base64::Engine;
        let addr = "127.0.0.1:11434".parse().unwrap();
        let (status, bytes) =
            waypoint_inference::ollama::http_request(addr, "GET", "/api/ps", None, 3000).unwrap();
        let ps: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status, 200);
        assert_eq!(
            crate::eligible_resident_model(&ps).unwrap().as_deref(),
            Some("qwen3.5:9b")
        );
        assert_eq!(ps["models"][0]["context_length"].as_u64(), Some(4096));
        let image = base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../tests/fictional-dense-form.png"));
        let expected: Vec<_> = (1..=16)
            .map(|i| {
                let value = if i == 7 || i == 16 {
                    format!("Wrong {i:02}")
                } else {
                    format!("Value {i:02}")
                };
                (format!("Question {i:02}"), value)
            })
            .collect();
        let result = analyze(addr, "qwen3.5:9b", &image, &expected).unwrap();
        println!("fictional dense visual result: {result}");
        let comparisons = result["comparisons"].as_array().unwrap();
        assert_eq!(comparisons.len(), 16);
        for i in 1..=16 {
            let status = if i == 7 || i == 16 {
                "differs_from_transcription"
            } else {
                "matches_transcription"
            };
            assert!(
                comparisons
                    .iter()
                    .any(|v| v["label"] == format!("Question {i:02}") && v["status"] == status),
                "unexpected comparison for Question {i:02}"
            );
        }
    }

    #[test]
    #[ignore = "requires a preloaded local qwen3.5:9b model and resource preflight"]
    fn live_visual_review_detects_a_fictional_mismatch() {
        use base64::Engine;
        let addr = "127.0.0.1:11434".parse().unwrap();
        let (status, bytes) =
            waypoint_inference::ollama::http_request(addr, "GET", "/api/ps", None, 3000).unwrap();
        let ps: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status, 200);
        assert_eq!(
            crate::eligible_resident_model(&ps).unwrap().as_deref(),
            Some("qwen3.5:9b")
        );
        assert_eq!(ps["models"][0]["context_length"].as_u64(), Some(4096));
        let image = base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../tests/fictional-form.png"));
        let expected = vec![
            ("Full name".into(), "Avery Example".into()),
            ("Email".into(), "different@example.invalid".into()),
            ("Location".into(), "Berlin".into()),
            ("Available to travel".into(), "checked".into()),
        ];
        let result =
            analyze_with_context(addr, "qwen3.5:9b", &image, &expected, Some(4096)).unwrap();
        println!("fictional visual result: {result}");
        let comparisons = result["comparisons"].as_array().unwrap();
        for label in ["Full name", "Location", "Available to travel"] {
            assert!(
                comparisons
                    .iter()
                    .any(|v| v["label"] == label && v["status"] == "matches_transcription"),
                "unverified field: {label}"
            );
        }
        assert!(
            comparisons
                .iter()
                .any(|v| v["label"] == "Email" && v["status"] == "differs_from_transcription"),
            "mismatch was not detected"
        );
    }
    #[test]
    fn image_freshness_reports_changes_and_missing_capture() {
        let digest = waypoint_domain::Sha256Digest::of("fictional-image");
        assert_eq!(
            image_freshness(Some(digest.as_str()), Some("fictional-image")),
            "same"
        );
        assert_eq!(
            image_freshness(Some(digest.as_str()), Some("changed-image")),
            "changed"
        );
        assert_eq!(image_freshness(Some(digest.as_str()), None), "unavailable");
        assert_eq!(
            image_freshness(None, Some("fictional-image")),
            "unavailable"
        );
    }
    #[test]
    fn visual_result_can_be_retried_after_a_lost_response() {
        let _guard = TEST_TASK_LOCK.lock().unwrap();
        *TASK.lock().unwrap() = Some(Task {
            id: "fixture-review".into(),
            session: "fixture-session".into(),
            page_signature: "fixture-page".into(),
            segment: 0,
            half: None,
            screenshot_hash: "fixture-hash".into(),
            result: Some(Ok(json!({"observations":[],"comparisons":[]}))),
            completed: Some(std::time::Instant::now()),
            record_saved: true,
        });
        let first = poll("fixture-review").unwrap();
        assert_eq!(first, poll("fixture-review").unwrap());
        assert!(poll("different-review").is_err());
    }
    #[test]
    fn visual_comparisons_cover_later_filled_fields_with_a_bounded_limit() {
        let values = (0..201).map(|i| (format!("Field {i}"), format!("Answer {i}")));
        let (expected, truncated) = bounded_expected(values);
        assert_eq!(expected.len(), 200);
        assert!(truncated);
        let observed =
            validate(&json!({"observations":[{"label":"Field 125","visible_value":"Answer 125"}]}))
                .unwrap();
        let result = compare(&observed, &expected);
        assert_eq!(
            result["comparisons"][125]["status"],
            "matches_transcription"
        );
        let (smaller, truncated) = bounded_expected(expected);
        assert_eq!(smaller.len(), 200);
        assert!(!truncated);
    }
    #[test]
    fn visual_expected_values_include_dates_and_choices() {
        use waypoint_applications::form_mapping::PacketValue;
        assert_eq!(
            expected_value(&PacketValue::Bool(true)).as_deref(),
            Some("checked")
        );
        assert_eq!(
            expected_value(&PacketValue::Bool(false)).as_deref(),
            Some("unchecked")
        );
        assert_eq!(
            expected_value(&PacketValue::Date("2026-11-01".into())).as_deref(),
            Some("2026-11-01")
        );
        assert_eq!(
            expected_value(&PacketValue::TextList(vec!["Rust".into(), "Python".into()])).as_deref(),
            Some("Rust, Python")
        );
        assert!(expected_value(&PacketValue::Text("x".repeat(1001))).is_none());
        assert!(expected_value(&PacketValue::FileRef {
            name: "cv.pdf".into()
        })
        .is_none());
    }
    #[test]
    fn page_signature_changes_with_url_or_dimensions() {
        let original = page_signature("https://example.org/a", 800.0, 1000.0);
        assert_ne!(
            original,
            page_signature("https://example.org/b", 800.0, 1000.0)
        );
        assert_ne!(
            original,
            page_signature("https://example.org/a", 800.0, 1200.0)
        );
    }
    #[test]
    fn segments_cover_bounded_pages_and_report_limits() {
        assert_eq!(
            segment_geometry(1200.0, 2600.0, 0).unwrap(),
            (1200.0, 0.0, 2000.0, 2, true)
        );
        assert_eq!(
            segment_geometry(1200.0, 2600.0, 1).unwrap(),
            (1200.0, 2000.0, 600.0, 2, true)
        );
        assert!(segment_geometry(1200.0, 2600.0, 2).is_err());
        assert_eq!(
            segment_geometry(3000.0, 15000.0, 5).unwrap(),
            (2048.0, 10000.0, 2000.0, 6, false)
        );
        assert!(segment_geometry(f64::NAN, 1.0, 0).is_err());
    }
    #[test]
    fn dense_segment_halves_cover_the_original_without_overlap() {
        assert_eq!(
            segment_half_geometry(1200.0, 2600.0, 1, Some(0)).unwrap(),
            (1200.0, 2000.0, 300.0, 2, true)
        );
        assert_eq!(
            segment_half_geometry(1200.0, 2600.0, 1, Some(1)).unwrap(),
            (1200.0, 2300.0, 300.0, 2, true)
        );
        assert!(segment_half_geometry(1200.0, 2600.0, 1, Some(2)).is_err());
    }
    #[test]
    fn sends_image_to_a_local_vision_fixture_and_compares_observations() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for step in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.contains(match step {
                    0 => "/api/ps",
                    1 => "/api/show",
                    _ => "/api/generate",
                }));
                let mut length = 0;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body: Value = if body.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice(&body).unwrap()
                };
                let response = if step == 0 { json!({"models":[{"name":"fictional-model","size":1000,"size_vram":1000,"context_length":4096}]}) } else if step == 1 { json!({"capabilities":["vision"]}) } else {
                    assert_eq!(body["images"][0], "fictional-image");
                    assert_eq!(body["options"]["num_ctx"], 4096);
                    json!({"done":true,"done_reason":"stop","response":json!({"observations":[{"label":"Name","visible_value":"Example"},{"label":"Travel","visible_value":"checked"},{"label":"Skills","visible_value":"Rust, Python"}]}).to_string()})
                }.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            }
        });
        let result = analyze(
            addr,
            "fictional-model",
            "fictional-image",
            &[
                ("Name".into(), "Example".into()),
                ("Travel".into(), "checked".into()),
                ("Skills".into(), "Rust, Python".into()),
            ],
        )
        .unwrap();
        assert_eq!(result["comparisons"][0]["status"], "matches_transcription");
        assert_eq!(result["comparisons"][1]["status"], "matches_transcription");
        assert_eq!(result["comparisons"][2]["status"], "matches_transcription");
        server.join().unwrap();
    }
    #[test]
    fn compares_only_unambiguous_visible_transcriptions() {
        let observed =
            validate(&json!({"observations":[{"label":"Name","visible_value":"Example"}]}))
                .unwrap();
        let result = compare(
            &observed,
            &[
                ("Name".into(), "Example".into()),
                ("Email".into(), "a@example.invalid".into()),
            ],
        );
        assert_eq!(result["comparisons"][0]["status"], "matches_transcription");
        assert_eq!(result["comparisons"][1]["status"], "unobserved");
        let formatted = validate(
            &json!({"observations":[{"label":" First\n  NAME ","visible_value":"Example"}]}),
        )
        .unwrap();
        let result = compare(&formatted, &[("First name".into(), "Example".into())]);
        assert_eq!(result["comparisons"][0]["status"], "matches_transcription");
        let result = compare(
            &formatted,
            &[
                ("First name".into(), "Example".into()),
                ("FIRST\nNAME".into(), "Example".into()),
            ],
        );
        assert_eq!(result["comparisons"][0]["status"], "unobserved");
        let duplicate = validate(&json!({"observations":[{"label":"First name","visible_value":"Example"},{"label":"FIRST\nNAME","visible_value":"Example"}]})).unwrap();
        let result = compare(&duplicate, &[("First name".into(), "Example".into())]);
        assert_eq!(result["comparisons"][0]["status"], "unobserved");
        let international = validate(
            &json!({"observations":[{"label":" PRÉNOM\u{a0}COMPLET ","visible_value":"Élodie"}]}),
        )
        .unwrap();
        let result = compare(
            &international,
            &[("Prénom complet".into(), "Élodie".into())],
        );
        assert_eq!(result["comparisons"][0]["status"], "matches_transcription");
        let result = compare(
            &international,
            &[("Prénom complet".into(), "élodie".into())],
        );
        assert_eq!(
            result["comparisons"][0]["status"],
            "differs_from_transcription"
        );
        let result = compare(
            &international,
            &[
                ("Prénom complet".into(), "Élodie".into()),
                ("PRÉNOM\u{a0}COMPLET".into(), "Élodie".into()),
            ],
        );
        assert_eq!(result["comparisons"][0]["status"], "unobserved");
        let different = validate(
            &json!({"observations":[{"label":"\u{feff}First name","visible_value":"Example"}]}),
        )
        .unwrap();
        let result = compare(&different, &[("First name".into(), "Example".into())]);
        assert_eq!(result["comparisons"][0]["status"], "unobserved");
    }

    #[test]
    fn unfilled_duplicate_label_cannot_make_filled_field_appear_unique() {
        use waypoint_applications::form_mapping::{
            FormField, FormFieldType, FormSchema, PacketValue, ProposedMapping,
        };
        let fields = [
            ("#one", "Prénom complet"),
            ("#two", "PRÉNOM\u{a0}COMPLET"),
            ("#three", "City"),
        ]
        .into_iter()
        .map(|(selector, label)| FormField {
            selector: selector.into(),
            label: label.into(),
            field_type: FormFieldType::Text,
            required: false,
            sensitive: false,
            editing_mode: None,
            max_length_utf16: None,
            autocomplete: None,
        })
        .collect();
        let preview = crate::form_preview::FormPreview {
            schema: FormSchema {
                origin: "https://example.test".into(),
                fields,
                multi_step: false,
                steps: 1,
            },
            suggestions: vec![ProposedMapping {
                field_selector: "#one".into(),
                value: PacketValue::Text("Élodie".into()),
            }],
            manual_review: vec![],
            model_review_selectors: Default::default(),
        };
        let (expected, truncated) = expected_from_preview(&preview, &["#one".into()]);
        assert!(!truncated);
        assert_eq!(expected.values.len(), 1);
        let observed = validate(
            &json!({"observations":[{"label":"PRÉNOM\u{a0}COMPLET","visible_value":"Élodie"}]}),
        )
        .unwrap();
        let mut result = compare(&observed, &expected.values);
        assert_eq!(result["comparisons"][0]["status"], "matches_transcription");
        mark_ambiguous(&mut result, &expected.ambiguous_labels);
        assert_eq!(result["comparisons"][0]["status"], "unobserved");
        assert_eq!(result["comparisons"][0]["ambiguous"], true);
        assert_eq!(result["comparisons"].as_array().unwrap().len(), 1);
    }

    #[test]
    #[ignore = "starts one local browser and requires a preloaded local vision model"]
    fn live_visual_review_keeps_a_single_filled_duplicate_ambiguous() {
        use waypoint_applications::driver::BrowserDriver;
        use waypoint_applications::form_mapping::BrowserCommand;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/form-similar-labels.html")
            .canonicalize()
            .unwrap();
        let url = url::Url::from_file_path(path).unwrap().to_string();
        let mut driver = waypoint_browser::CdpDriver::launch(vec![]).unwrap();
        let live = driver.open(&url).unwrap();
        let preview = crate::form_preview::suggest_without_model(
            live,
            &json!({"email":"fictional@example.invalid"}),
            "Model unavailable",
        )
        .unwrap();
        let chosen = &preview.suggestions[0];
        driver
            .execute(&BrowserCommand::Fill {
                selector: chosen.field_selector.clone(),
                value: chosen.value.clone(),
            })
            .unwrap();
        let (expected, truncated) =
            expected_from_preview(&preview, std::slice::from_ref(&chosen.field_selector));
        assert!(!truncated);
        let mut result = analyze(
            "127.0.0.1:11434".parse().unwrap(),
            "qwen3.5:9b",
            &driver.screenshot_base64().unwrap(),
            &expected.values,
        )
        .unwrap();
        println!("fictional visual observations: {}", result["observations"]);
        mark_ambiguous(&mut result, &expected.ambiguous_labels);
        assert_eq!(result["comparisons"][0]["status"], "unobserved");
        assert_eq!(result["comparisons"][0]["ambiguous"], true);
        assert!(driver
            .trace
            .iter()
            .all(|entry| !entry.contains("submitted")));
    }
    #[test]
    fn observation_limit_is_reported_as_possible_incomplete_coverage() {
        let observations = (0..MAX_OBSERVATIONS)
            .map(|i| json!({"label":format!("Field {i}"),"visible_value":"value"}))
            .collect::<Vec<_>>();
        let result = compare(
            &validate(&json!({"observations":observations})).unwrap(),
            &[],
        );
        assert_eq!(result["observation_limit_reached"], true);
        let result = compare(&validate(&json!({"observations":[]})).unwrap(), &[]);
        assert_eq!(result["observation_limit_reached"], false);
    }
    #[test]
    fn rejects_unbounded_or_malformed_observations() {
        assert!(
            validate(&json!({"observations":[{"label":"Name","visible_value":"Example"}]})).is_ok()
        );
        assert!(validate(&json!({"observations":[{"label":"","visible_value":"x"}]})).is_err());
        assert!(validate(
            &json!({"observations":vec![json!({"label":"x","visible_value":"y"});21]})
        )
        .is_err());
    }
}
