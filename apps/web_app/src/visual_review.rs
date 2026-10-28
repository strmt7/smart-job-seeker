//! Bounded, local-only advisory transcription of one form-page segment.
use serde_json::{json, Value};
use std::sync::Mutex;

struct Task {
    id: String,
    session: String,
    result: Option<Result<Value, String>>,
    completed: Option<std::time::Instant>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);

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
    let items = observed["observations"]
        .as_array()
        .expect("validated observations");
    let comparisons: Vec<_> = expected
        .iter()
        .map(|(label, value)| {
            let matches: Vec<_> = items
                .iter()
                .filter(|item| {
                    item["label"]
                        .as_str()
                        .is_some_and(|name| name.trim().eq_ignore_ascii_case(label.trim()))
                })
                .collect();
            let unique_expected = expected
                .iter()
                .filter(|(name, _)| name.trim().eq_ignore_ascii_case(label.trim()))
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
    model: String,
    image: String,
    expected: Vec<(String, String)>,
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
        result: None,
        completed: None,
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
        if let Ok(mut task) = TASK.lock() {
            if let Some(task) = task.as_mut().filter(|t| t.id == id) {
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
    let prompt = "Transcribe only visible form field labels and their displayed values in the image. Return JSON: {\"observations\":[{\"label\":\"...\",\"visible_value\":\"...\"}]}. Do not infer hidden or unreadable values. An empty array is allowed.";
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
    let task = tasks
        .as_mut()
        .filter(|t| t.id == id)
        .ok_or("Visual review not found")?;
    let Some(result) = task.result.take() else {
        return Ok(json!({"status":"running","session_id":task.session}));
    };
    let session = task.session.clone();
    *tasks = None;
    Ok(match result {
        Ok(observed) => json!({"status":"complete","session_id":session,"visual_review":observed}),
        Err(error) => json!({"status":"failed","error":error}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
                    json!({"done":true,"done_reason":"stop","response":json!({"observations":[{"label":"Name","visible_value":"Example"}]}).to_string()})
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
            &[("Name".into(), "Example".into())],
        )
        .unwrap();
        assert_eq!(result["comparisons"][0]["status"], "matches_transcription");
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
