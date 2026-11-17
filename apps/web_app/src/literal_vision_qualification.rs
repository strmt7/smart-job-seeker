//! Paired, image-only qualification for the native text-detail readback.
use super::*;
use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    python: PathBuf,
    selection: crate::private_inference::Selection,
    cases: Vec<Case>,
    #[serde(default)]
    literal_only: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    image: PathBuf,
    image_sha256: String,
    label: String,
    visible: String,
    contrary: String,
}

#[test]
#[ignore = "requires a bound fictional image corpus and private native vision runtime"]
fn compares_literal_and_deliberative_readback() {
    let path = PathBuf::from(std::env::var_os("WAYPOINT_LITERAL_VISION_FIXTURE").unwrap())
        .canonicalize()
        .unwrap();
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .canonicalize()
        .unwrap();
    assert!(path.starts_with(&target));
    let bytes = crate::recorded_preview::read(&path, 128 * 1024).unwrap();
    let fixture_sha256 = waypoint_domain::sha256_hex(&bytes);
    assert_eq!(
        fixture_sha256,
        std::env::var("WAYPOINT_LITERAL_VISION_FIXTURE_SHA256").unwrap()
    );
    let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
    assert!((2..=8).contains(&fixture.cases.len()));
    let mut case_ids = std::collections::HashSet::new();
    let input: Vec<_> = fixture
        .cases
        .iter()
        .map(|case| {
            assert!(!case.id.is_empty() && case_ids.insert(&case.id));
            assert!(!case.label.is_empty() && case.visible != case.contrary);
            let image = case.image.canonicalize().unwrap();
            assert!(image.starts_with(&target));
            let bytes = crate::recorded_preview::read(&image, 2 * 1024 * 1024).unwrap();
            assert_eq!(waypoint_domain::sha256_hex(&bytes), case.image_sha256);
            base64::engine::general_purpose::STANDARD.encode(bytes)
        })
        .collect();

    let root = path.parent().unwrap();
    let task = "literal-vision-qualification";
    let attempt = root.join(task);
    let screenshots = attempt.join("screenshots");
    std::fs::create_dir(&attempt).unwrap();
    std::fs::create_dir(&screenshots).unwrap();
    let _lease = crate::doc_suggest::lease().unwrap();
    assert_eq!(crate::resident_model_status().unwrap()["state"], "no_model");
    crate::private_inference::create_namespace(&attempt, task).unwrap();
    let mut session =
        crate::private_inference::Session::admit(&fixture.selection, &fixture.python, &screenshots)
            .unwrap();
    assert_eq!(session.metadata["mode"], "private");
    let identity = crate::resident_model_identity_at(session.addr)
        .unwrap()
        .expect("private vision identity");
    let results = {
        let binding = session.runtime.as_ref().expect("private runtime binding");
        let request = VisionRequest {
            context: None,
            guard: Some(&binding.guard),
            runner: Some(binding.runner),
        };
        let schema = transcription_schema();
        let literal_schema = literal_readback_schema();
        let interpretation_prompt = blind_value_coverage_prompt();
        let mut results = Vec::new();
        for (ordinal, (case, image)) in fixture.cases.iter().zip(&input).enumerate() {
            let order = if ordinal % 2 == 0 {
                [
                    ("interpretation", false),
                    ("literal", true),
                    ("literal_repeat", true),
                ]
            } else {
                [
                    ("literal", true),
                    ("interpretation", false),
                    ("literal_repeat", true),
                ]
            };
            for (mode, literal) in order {
                if fixture.literal_only && !literal {
                    continue;
                }
                let started = Instant::now();
                let (prompt, format) = if literal {
                    (
                        LITERAL_READBACK_PROMPT,
                        VisionFormat::LiteralReadback(&literal_schema),
                    )
                } else {
                    (
                        interpretation_prompt.as_str(),
                        VisionFormat::Interpretation(Some(&schema)),
                    )
                };
                let response = generate_vision_with_format_guarded(
                    session.addr,
                    &identity.name,
                    image,
                    prompt,
                    format,
                    request,
                )
                .and_then(|response| {
                    if literal {
                        normalize_literal_readback(&response)
                    } else {
                        validate(&response)
                    }
                });
                let mut row = json!({"case":case.id,"mode":mode,"image_sha256":case.image_sha256,
                    "elapsed_seconds":started.elapsed().as_secs_f64()});
                match response {
                    Ok(observed) => {
                        let compare = |expected: &str| {
                            compare_recorded_labels(
                                &observed,
                                &[(case.label.clone(), expected.to_owned())],
                                false,
                                ValueComparison::Exact,
                                None,
                            )
                        };
                        let match_report = compare(&case.visible);
                        row["status"] = json!(format!("{:?}", recorded_outcome(&match_report)));
                        row["observation"] = match_report;
                        row["false_confirmation"] = json!(
                            recorded_outcome(&compare(&case.contrary))
                                == RecordedVisionOutcome::Matched
                        );
                    }
                    Err(reason) => {
                        row["status"] = json!("Failed");
                        row["error"] = json!(reason);
                    }
                }
                results.push(row);
            }
        }
        results
    };
    let stopped = session.stop();
    let recovery = crate::private_inference::cleanup_namespace(&attempt, task);
    let report = json!({"fixture_sha256":fixture_sha256,"model":session.metadata,
        "stopped":stopped.is_ok(),"namespace_drained":recovery.is_ok(),"results":results});
    std::fs::write(
        root.join("literal-vision-result.private.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    stopped.unwrap();
    recovery.unwrap();
    let reads_per_case = if fixture.literal_only { 2 } else { 3 };
    assert_eq!(results.len(), fixture.cases.len() * reads_per_case);
    assert!(
        results
            .iter()
            .filter(|row| row["mode"] != "interpretation")
            .all(|row| { row["status"] == "Matched" && row["false_confirmation"] == false }),
        "Literal samples did not meet the frozen acceptance criteria"
    );
}
