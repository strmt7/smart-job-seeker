//! Bounded, local-only advisory transcription of one form-page segment.
use serde_json::{json, Value};
use std::sync::Mutex;

struct Task {
    id: String,
    session: String,
    page_signature: String,
    segment: usize,
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

fn validate(output: &Value) -> Result<Value, String> {
    let observations = output
        .get("observations")
        .and_then(Value::as_array)
        .filter(|items| items.len() <= 20)
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
    fn normalized_label(label: &str) -> String {
        label
            .split([' ', '\t', '\n', '\r', '\x0c', '\x0b'])
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase()
    }
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
    json!({"observations":items,"comparisons":comparisons})
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

pub fn start(
    id: String,
    session: String,
    capture: (String, usize),
    model: String,
    image: String,
    expected: Vec<(String, String)>,
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
            analyze(
                "127.0.0.1:11434"
                    .parse()
                    .map_err(|_| "Invalid local runtime address")?,
                &model,
                &image,
                &expected,
            )
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

fn analyze(
    addr: std::net::SocketAddr,
    model: &str,
    image: &str,
    expected: &[(String, String)],
) -> Result<Value, String> {
    let prompt = "Transcribe only visible form field labels and their displayed values in the image. For a visible checkbox use checked or unchecked; for a multi-select list the visible selected labels separated by comma and space. Return JSON: {\"observations\":[{\"label\":\"...\",\"visible_value\":\"...\"}]}. Do not infer hidden or unreadable values. An empty array is allowed.";
    let result = generate_vision(addr, model, image, prompt)?;
    let observed = validate(&result)?;
    Ok(compare(&observed, expected))
}

pub(crate) fn generate_vision(
    addr: std::net::SocketAddr,
    model: &str,
    image: &str,
    prompt: &str,
) -> Result<Value, String> {
    if image.is_empty() || image.len() > 4 * 1024 * 1024 || prompt.len() > 24_000 {
        return Err("Vision input exceeds limits".into());
    }
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
    let body = json!({"model":model,"prompt":prompt,"images":[image],"stream":false,"format":"json","think":false,"options":{"num_predict":1024,"temperature":0}});
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
    let screenshot_hash = task.screenshot_hash.clone();
    let record_saved = task.record_saved;
    let output = match result {
        Ok(observed) => {
            json!({"status":"complete","session_id":session,"page_signature":page_signature,"segment":segment,"screenshot_hash":screenshot_hash,"visual_review":observed,"record_saved":record_saved})
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
    fn sends_image_to_a_local_vision_fixture_and_compares_observations() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for step in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.contains(if step == 0 {
                    "/api/show"
                } else {
                    "/api/generate"
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
                let body: Value = serde_json::from_slice(&body).unwrap();
                let response = if step == 0 { json!({"capabilities":["vision"]}) } else {
                    assert_eq!(body["images"][0], "fictional-image");
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
        let different = validate(
            &json!({"observations":[{"label":"\u{feff}First name","visible_value":"Example"}]}),
        )
        .unwrap();
        let result = compare(&different, &[("First name".into(), "Example".into())]);
        assert_eq!(result["comparisons"][0]["status"], "unobserved");
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
