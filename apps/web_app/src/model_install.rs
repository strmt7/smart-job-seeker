//! Bounded installation of reviewed local Ollama models.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read},
    net::SocketAddr,
    sync::Mutex,
    time::Duration,
};

#[derive(Clone, Copy)]
struct Spec {
    name: &'static str,
    digest: &'static str,
}
const DEFAULT_DIGEST: &str = "552b71d645d94564cfacdce245ba98de155c265de8ec3aa7c5e515e9590e0d8f";
const QWEN_SOURCE: Spec = Spec {
    name: "hf.co/unsloth/Qwen3.8-27B-GGUF:UD-Q3_K_XL",
    digest: "56d747cb510c031f215eea0c2cef98662ef76c95177667b11b0080e2a63f7bf0",
};
const SPECS: [Spec; 4] = [
    Spec {
        name: "gemma4:12b-it-qat",
        digest: "38044be4f923e5a55264ed7df4eaac2676651a905f735197c504045140c02bd3",
    },
    Spec {
        name: "ministral-3:14b",
        digest: "4760c35aeb9d9e9c6174c2492562c0b999e80a222804fd96b1915ab72bbcdcf7",
    },
    Spec {
        name: "qwen3.5:9b",
        digest: "c97eb11d70b1acdc88af01eef566c1fe4f7fbe93eb1afc06871132f293ff425a",
    },
    Spec {
        name: waypoint_inference::generation_profile::DEFAULT_MODEL,
        digest: DEFAULT_DIGEST,
    },
];

pub(crate) fn validate_default(choice: &crate::model_preferences::Choice) -> Result<(), String> {
    if choice.name != waypoint_inference::generation_profile::DEFAULT_MODEL
        || choice.digest != DEFAULT_DIGEST
    {
        return Err("The default Qwen version differs from the reviewed installation; repair or explicitly select a model in Settings".into());
    }
    Ok(())
}
struct Task {
    id: String,
    phase: String,
    completed: Option<u64>,
    total: Option<u64>,
    result: Option<Result<Value, String>>,
}
static TASK: Mutex<Option<Task>> = Mutex::new(None);
static COMPLETED: Mutex<Option<(String, Value)>> = Mutex::new(None);

fn spec(name: &str) -> Option<Spec> {
    SPECS.iter().copied().find(|entry| entry.name == name)
}
fn inventory(addr: SocketAddr, spec: Spec) -> Result<Option<bool>, String> {
    let (status, bytes) =
        waypoint_inference::ollama::http_request(addr, "GET", "/api/tags", None, 3000)?;
    if status != 200 {
        return Err(format!("Local runtime inventory returned HTTP {status}"));
    }
    let data: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid installed-model inventory")?;
    let models = data["models"]
        .as_array()
        .filter(|items| items.len() <= 1000)
        .ok_or("Installed-model inventory is unavailable or too large")?;
    let matches: Vec<_> = models
        .iter()
        .filter(|item| item["name"].as_str() == Some(spec.name))
        .collect();
    if matches.len() > 1 {
        return Err("The model inventory contains duplicate entries".into());
    }
    Ok(matches
        .first()
        .map(|item| item["digest"].as_str() == Some(spec.digest)))
}

fn run(addr: SocketAddr, selected: Spec, id: &str) -> Result<Value, String> {
    if addr.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
        return Err("Runtime must use IPv4 loopback".into());
    }
    if let Some(verified) = inventory(addr, selected)? {
        return Ok(json!({"model":selected.name,"already_installed":true,"verified":verified}));
    }
    if selected.name == waypoint_inference::generation_profile::DEFAULT_MODEL {
        let source = run(addr, QWEN_SOURCE, id)?;
        if source["verified"] != true {
            return Err("The Qwen source differs from the reviewed weights; the existing model was preserved".into());
        }
        let body = serde_json::to_vec(&json!({"model":selected.name,"from":QWEN_SOURCE.name,
            "renderer":"qwen3.8","parser":"qwen3.5","stream":false,
            "parameters":{"stop":[],"num_ctx":8192,"temperature":1.0,"top_p":0.95,"top_k":20,"min_p":0.0,"presence_penalty":0.0,"repeat_penalty":1.0}}))
            .map_err(|_| "Cannot prepare the native Qwen profile")?;
        let (code, value) = waypoint_inference::ollama::http_request(
            addr,
            "POST",
            "/api/create",
            Some(&body),
            60_000,
        )?;
        let value: Value = serde_json::from_slice(&value)
            .map_err(|_| "Invalid Qwen profile installation response")?;
        if code != 200 || value["status"] != "success" || inventory(addr, selected)? != Some(true) {
            return Err("The native Qwen profile did not match the reviewed installation; no model was loaded".into());
        }
        let request = serde_json::to_vec(&json!({"model":selected.name}))
            .map_err(|_| "Cannot inspect installed Qwen profile")?;
        let (code, details) = waypoint_inference::ollama::http_request(
            addr,
            "POST",
            "/api/show",
            Some(&request),
            3000,
        )?;
        let details: Value =
            serde_json::from_slice(&details).map_err(|_| "Invalid installed Qwen capabilities")?;
        if code != 200 {
            return Err("Installed Qwen capabilities could not be inspected".into());
        }
        waypoint_inference::generation_profile::GenerationProfile::Qwen38
            .validate_details(&details)?;
        return Ok(json!({"model":selected.name,"already_installed":false,"verified":true}));
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(3600))
        .build()
        .map_err(|_| "Could not configure local runtime connection")?;
    let response = client
        .post(format!("http://{addr}/api/pull"))
        .json(&json!({"model":selected.name,"stream":true,"insecure":false}))
        .send()
        .map_err(|_| "Model download did not complete; check installed models before retrying")?;
    if !response.status().is_success() {
        return Err(format!(
            "Local model download returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut reader = BufReader::new(response);
    let mut success = false;
    loop {
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take(16_385)
            .read_until(b'\n', &mut line)
            .map_err(|_| {
                "Model download stream was interrupted; check installed models before retrying"
            })?;
        if count == 0 {
            break;
        }
        if line.len() > 16_384 {
            return Err("Model download returned an oversized progress message".into());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let progress: Value = serde_json::from_slice(&line)
            .map_err(|_| "Model download returned invalid progress")?;
        if progress.get("error").is_some() {
            return Err("The local runtime reported a model download error; check installed models before retrying".into());
        }
        let phase = progress["status"]
            .as_str()
            .filter(|s| s.len() <= 200 && !s.chars().any(char::is_control))
            .unwrap_or("Downloading");
        if phase == "success" {
            success = true;
        }
        if let Ok(mut tasks) = TASK.lock() {
            if let Some(task) = tasks.as_mut().filter(|task| task.id == id) {
                task.phase = phase.into();
                task.completed = progress["completed"].as_u64();
                task.total = progress["total"].as_u64();
            }
        }
    }
    if !success {
        return Err(
            "The download ended without a success result; check installed models before retrying"
                .into(),
        );
    }
    match inventory(addr, selected)? {
        Some(true) => Ok(json!({"model":selected.name,"already_installed":false,"verified":true})),
        Some(false) => Err("Downloaded model digest differs from the reviewed version; it was left in the local runtime for inspection".into()),
        None => Err("The downloaded model was not found in the local inventory".into()),
    }
}

pub fn start(id: String, name: &str) -> Result<(), String> {
    let selected = spec(name).ok_or("Choose a supported local model")?;
    let mut task = TASK.lock().map_err(|_| "Model installation unavailable")?;
    if task.is_some() {
        return Err("A model installation is running or awaiting its result".into());
    }
    let lease = crate::doc_suggest::lease()?;
    *task = Some(Task {
        id: id.clone(),
        phase: "Preparing local runtime".into(),
        completed: None,
        total: None,
        result: None,
    });
    drop(task);
    std::thread::spawn(move || {
        let _lease = lease;
        let result = std::panic::catch_unwind(|| {
            crate::runtime_start::ensure_started()?;
            run(
                "127.0.0.1:11434".parse().expect("literal loopback"),
                selected,
                &id,
            )
        })
        .unwrap_or_else(|_| {
            Err(
                "Model installation stopped unexpectedly; check installed models before retrying"
                    .into(),
            )
        });
        if let Ok(mut tasks) = TASK.lock() {
            if let Some(task) = tasks.as_mut().filter(|task| task.id == id) {
                task.result = Some(result);
            }
        }
    });
    Ok(())
}
pub fn active() -> Result<Option<String>, String> {
    Ok(TASK
        .lock()
        .map_err(|_| "Model installation unavailable")?
        .as_ref()
        .map(|task| task.id.clone()))
}
pub fn poll(id: &str) -> Result<Value, String> {
    let mut tasks = TASK.lock().map_err(|_| "Model installation unavailable")?;
    let Some(task) = tasks.as_mut().filter(|task| task.id == id) else {
        return COMPLETED
            .lock()
            .map_err(|_| "Model installation unavailable")?
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| "Model installation not found".into());
    };
    let Some(result) = task.result.take() else {
        return Ok(
            json!({"status":"running","phase":task.phase,"completed":task.completed,"total":task.total}),
        );
    };
    let result = match result {
        Ok(value) => json!({"status":"complete","result":value}),
        Err(error) => json!({"status":"failed","error":error}),
    };
    *COMPLETED
        .lock()
        .map_err(|_| "Model installation unavailable")? = Some((id.into(), result.clone()));
    *tasks = None;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    fn serve(replies: Vec<(String, String)>) -> (SocketAddr, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (expected_path, response) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 1024];
                loop {
                    let count = stream.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                    assert!(bytes.len() < 64 * 1024);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(request.lines().next().unwrap().contains(&expected_path));
                requests.push(request);
                if expected_path == "/api/pull" {
                    write!(stream, "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n").unwrap();
                    for part in response.as_bytes().chunks(11) {
                        write!(stream, "{:x}\r\n", part.len()).unwrap();
                        stream.write_all(part).unwrap();
                        write!(stream, "\r\n").unwrap();
                    }
                    write!(stream, "0\r\n\r\n").unwrap();
                } else {
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
                }
            }
            requests
        });
        (addr, handle)
    }
    #[test]
    fn installation_does_not_replace_an_existing_model() {
        let selected = SPECS[0];
        let body = json!({"models":[{"name":selected.name,"digest":"different"}]}).to_string();
        let (addr, server) = serve(vec![("/api/tags".into(), body)]);
        let result = run(addr, selected, "fixture").unwrap();
        assert_eq!(result["already_installed"], true);
        assert_eq!(result["verified"], false);
        assert_eq!(server.join().unwrap().len(), 1);
    }
    #[test]
    fn installation_checks_success_and_exact_reviewed_manifest() {
        let selected = SPECS[0];
        let before = json!({"models":[]}).to_string();
        let after = json!({"models":[{"name":selected.name,"digest":selected.digest}]}).to_string();
        let stream =
            "{\"status\":\"downloading\",\"completed\":5,\"total\":10}\n{\"status\":\"success\"}\n"
                .to_owned();
        let (addr, server) = serve(vec![
            ("/api/tags".into(), before),
            ("/api/pull".into(), stream),
            ("/api/tags".into(), after),
        ]);
        let result = run(addr, selected, "fixture").unwrap();
        let requests = server.join().unwrap();
        assert_eq!(result["verified"], true);
        let body = requests[1].split_once("\r\n\r\n").unwrap().1;
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], selected.name);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["insecure"], false);
    }
    #[test]
    fn changed_download_manifest_is_not_reported_as_verified() {
        let selected = SPECS[0];
        let (addr, server) = serve(vec![
            ("/api/tags".into(), json!({"models":[]}).to_string()),
            ("/api/pull".into(), "{\"status\":\"success\"}\n".into()),
            (
                "/api/tags".into(),
                json!({"models":[{"name":selected.name,"digest":"changed"}]}).to_string(),
            ),
        ]);
        assert!(run(addr, selected, "fixture")
            .unwrap_err()
            .contains("digest differs"));
        assert_eq!(server.join().unwrap().len(), 3);
    }
    #[test]
    fn incomplete_download_is_not_reported_as_success() {
        let (addr, server) = serve(vec![
            ("/api/tags".into(), json!({"models":[]}).to_string()),
            ("/api/pull".into(), "{\"status\":\"downloading\"}\n".into()),
        ]);
        let error = run(addr, SPECS[0], "fixture").unwrap_err();
        assert!(error.contains("without a success"));
        assert_eq!(server.join().unwrap().len(), 2);
    }
    #[test]
    #[ignore = "requires the reviewed model already installed in the local runtime"]
    fn live_existing_reviewed_model_is_not_pulled_again() {
        let addr = "127.0.0.1:11434".parse().unwrap();
        let selected = SPECS[0];
        assert_eq!(inventory(addr, selected).unwrap(), Some(true));
        let before = crate::resident_model_status().unwrap();
        let result = run(addr, selected, "fixture").unwrap();
        assert_eq!(result["already_installed"], true);
        assert_eq!(result["verified"], true);
        assert_eq!(crate::resident_model_status().unwrap(), before);
    }
    #[test]
    #[ignore = "requires the reviewed 14B model already installed in the local runtime"]
    fn live_existing_14b_manifest_is_verified_without_loading() {
        let addr = "127.0.0.1:11434".parse().unwrap();
        let selected = spec("ministral-3:14b").unwrap();
        let before = crate::resident_model_status().unwrap();
        let result = run(addr, selected, "fixture").unwrap();
        assert_eq!(result["already_installed"], true);
        assert_eq!(result["verified"], true);
        assert_eq!(crate::resident_model_status().unwrap(), before);
    }

    #[test]
    fn download_rejects_cloud_or_unreviewed_model_names() {
        assert!(spec("anything-cloud").is_none());
        assert!(spec("gemma4:12b-it-qat").is_some());
        assert!(spec("qwen3.5:9b").is_some());
        assert!(spec("ministral-3:14b").is_some());
        assert!(spec(waypoint_inference::generation_profile::DEFAULT_MODEL).is_some());
    }

    #[test]
    fn default_recipe_verifies_source_and_native_profile_without_loading() {
        let selected = spec(waypoint_inference::generation_profile::DEFAULT_MODEL).unwrap();
        let (addr, server) = serve(vec![
            ("/api/tags".into(), json!({"models":[]}).to_string()),
            ("/api/tags".into(), json!({"models":[{"name":QWEN_SOURCE.name,"digest":QWEN_SOURCE.digest}]}).to_string()),
            ("/api/create".into(), json!({"status":"success"}).to_string()),
            ("/api/tags".into(), json!({"models":[{"name":selected.name,"digest":selected.digest}]}).to_string()),
            ("/api/show".into(), json!({"renderer":"qwen3.8","parser":"qwen3.5","capabilities":["vision","tools","thinking"],"thinking":{"values":[false,"low","medium","xhigh"]}}).to_string()),
        ]);
        let result = run(addr, selected, "fixture").unwrap();
        assert_eq!(result["verified"], true);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 5);
        let body = requests[2].split_once("\r\n\r\n").unwrap().1;
        let value: Value = serde_json::from_str(body).unwrap();
        assert_eq!(value["from"], QWEN_SOURCE.name);
        assert_eq!(value["renderer"], "qwen3.8");
        assert_eq!(value["parser"], "qwen3.5");
        assert_eq!(value["parameters"]["num_ctx"], 8192);
        assert_eq!(value["parameters"]["temperature"], 1.0);
        assert!(requests
            .iter()
            .all(|request| !request.starts_with("POST /api/generate")));
    }

    #[test]
    fn changed_source_is_never_derived_into_the_default_model() {
        let selected = spec(waypoint_inference::generation_profile::DEFAULT_MODEL).unwrap();
        let (addr, server) = serve(vec![
            ("/api/tags".into(), json!({"models":[]}).to_string()),
            (
                "/api/tags".into(),
                json!({"models":[{"name":QWEN_SOURCE.name,"digest":"a".repeat(64)}]}).to_string(),
            ),
        ]);
        assert!(run(addr, selected, "fixture")
            .unwrap_err()
            .contains("source differs"));
        assert_eq!(server.join().unwrap().len(), 2);
    }
}
