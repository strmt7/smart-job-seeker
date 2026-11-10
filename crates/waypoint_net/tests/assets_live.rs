//! Explicit native transport qualification against the policy-permitted public API.
#[test]
#[ignore = "public API network access; requires same-URL robots-checked Crawl4AI receipt"]
fn public_board_asset_preserves_headers_framing_and_json_bytes() {
    let url = "https://boards-api.greenhouse.io/v1/boards/vercel/jobs";
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let permission: serde_json::Value = serde_json::from_str(include_str!(
        "../../waypoint_policy/data/source_permissions.json"
    ))
    .unwrap();
    assert!(permission
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["source"] == "greenhouse" && v["read"] == "allowed_public"));
    let receipt = PathBuf::from(
        std::env::var_os("WAYPOINT_ASSET_ROBOTS_RECEIPT").expect("explicit robots-checked receipt"),
    )
    .canonicalize()
    .unwrap();
    assert!(receipt.starts_with(repo.join("target")));
    let bytes = std::fs::read(receipt).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let checked: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(checked["requested_url"], url);
    assert_eq!(checked["native_result"]["success"], true);
    assert_eq!(checked["native_result"]["status_code"], 200);
    let response = waypoint_net::get_asset_capped(url, 20_000, waypoint_net::DEFAULT_MAX_BYTES)
        .expect("native public asset read");
    assert_eq!(response.status, 200);
    assert!(matches!(
        response.framing,
        waypoint_net::BodyFraming::ContentLength | waypoint_net::BodyFraming::Chunked
    ));
    assert!(response
        .headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("content-type")
            && h.value.starts_with(b"application/json")));
    let data: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    let jobs = data["jobs"].as_array().unwrap();
    assert!(!jobs.is_empty());
    let folder = repo.join("target").join(format!(
        "http-asset-live-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("body.private.json"), &response.body).unwrap();
    let report = serde_json::json!({"complete":true,"url":url,"status":response.status,"framing":format!("{:?}",response.framing),"body_bytes":response.body.len(),"postings":jobs.len(),"headers":response.headers.iter().map(|h|serde_json::json!({"name":h.name,"value_bytes":h.value})).collect::<Vec<_>>(),"scope":"one public JSON API response; no browser form, authentication or write"});
    std::fs::write(
        folder.join("result.private.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("Public asset evidence: {}", folder.display());
}
use std::path::{Path, PathBuf};
