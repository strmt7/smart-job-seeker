//! Live-payload validation of the ATS parsers against real public endpoints.
//! These tests hit the network; they are `#[ignore]` by default and run
//! explicitly with `cargo test -p waypoint_connectors -- --ignored` when a
//! live check is wanted. They never submit anything — read-only public GETs.

use waypoint_connectors::{parse_ashby, parse_greenhouse, parse_lever};

fn get(url: &str) -> Option<String> {
    // minimal std-only HTTP GET via std::net TcpStream is not worth the TLS
    // complexity; tests shell out to the system curl binary in CI/dev instead.
    let out = std::process::Command::new("curl")
        .args(["-sSf", "--max-time", "30", url])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

#[test]
#[ignore = "network test; run explicitly with --ignored"]
fn live_greenhouse_board_parses() {
    let body = get("https://boards-api.greenhouse.io/v1/boards/vercel/jobs").unwrap();
    let parsed = parse_greenhouse("vercel", &body).unwrap();
    assert!(!parsed.is_empty(), "vercel board should list jobs");
    assert!(parsed
        .iter()
        .all(|p| p.identity.starts_with("greenhouse::")));
    eprintln!("greenhouse live rows: {}", parsed.len());
}

#[test]
#[ignore = "network test; run explicitly with --ignored"]
fn live_ashby_board_parses() {
    let body = get("https://api.ashbyhq.com/posting-api/job-board/ashby").unwrap();
    let parsed = parse_ashby("ashby", &body).unwrap();
    assert!(!parsed.is_empty(), "ashby board should list jobs");
    assert!(parsed.iter().all(|p| p.identity.starts_with("ashby::")));
    eprintln!("ashby live rows: {}", parsed.len());
}

#[test]
#[ignore = "requires a separately captured public Ashby API response"]
fn captured_ashby_locations_preserve_all_alternatives() {
    let path = std::env::var("WAYPOINT_ASHBY_CAPTURE").expect("capture path");
    let body = std::fs::read_to_string(path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&body).unwrap();
    let jobs = raw["jobs"].as_array().unwrap();
    let parsed = parse_ashby("ashby", &body).unwrap();
    assert_eq!(parsed.len(), jobs.len());
    let mut with_secondary = 0;
    let mut with_application_route = 0;
    for (row, source) in parsed.iter().zip(jobs) {
        if let Some(route) = &row.application_url {
            assert_eq!(Some(route.as_str()), source["applyUrl"].as_str());
            with_application_route += 1;
        }
        let Some(locations) = source["secondaryLocations"].as_array() else {
            continue;
        };
        if !locations.is_empty() {
            with_secondary += 1;
        }
        for location in locations
            .iter()
            .filter_map(|item| item["location"].as_str())
        {
            let normalized = location.split_whitespace().collect::<Vec<_>>().join(" ");
            if !normalized.is_empty() {
                assert!(row
                    .location
                    .lines()
                    .any(|actual| actual.eq_ignore_ascii_case(&normalized)));
            }
        }
    }
    assert!(with_secondary > 0);
    assert!(with_application_route > 0);
    println!("Checked {with_secondary} captured listings with secondary locations");
    println!("Retained {with_application_route} source-reported application URLs");
}

#[test]
#[ignore = "network test; run explicitly with --ignored"]
fn live_lever_board_parses() {
    // Verified live board (cloudflare/netflix 404 on this endpoint).
    let body = get("https://api.lever.co/v0/postings/spotify?mode=json").unwrap();
    let parsed = parse_lever("spotify", &body).unwrap();
    assert!(!parsed.is_empty(), "spotify lever board should list jobs");
    assert!(parsed.iter().all(|p| p.identity.starts_with("lever::")));
    eprintln!("lever live rows: {}", parsed.len());
}

#[test]
#[ignore = "requires a separately captured public Lever API response"]
fn captured_lever_workplace_metadata_matches_payload() {
    let path = std::env::var("WAYPOINT_LEVER_CAPTURE").expect("capture path");
    let body = std::fs::read_to_string(path).unwrap();
    let raw: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    let parsed = parse_lever("spotify", &body).unwrap();
    assert_eq!(parsed.len(), raw.len());
    let mut checked = 0;
    for (row, source) in parsed.iter().zip(raw.iter()) {
        let nested = source
            .pointer("/categories/workplaceType")
            .and_then(|v| v.as_str());
        if nested.is_some_and(|value| !value.trim().is_empty()) {
            continue;
        }
        let expected = match source["workplaceType"]
            .as_str()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("remote") => Some(true),
            Some("on-site" | "onsite") => Some(false),
            _ => None,
        };
        assert_eq!(row.remote_evidence, expected);
        checked += 1;
    }
    assert!(checked > 0);
    println!("Compared {checked} captured public postings");
}

#[test]
#[ignore = "requires a separately captured public Lever API response"]
fn captured_lever_descriptions_include_requirement_sections() {
    let body =
        std::fs::read_to_string(std::env::var("WAYPOINT_LEVER_CAPTURE").expect("capture path"))
            .unwrap();
    let raw: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    let parsed = parse_lever("spotify", &body).unwrap();
    let mut checked = 0;
    for (row, source) in parsed.iter().zip(raw.iter()) {
        let Some(first) = source["lists"].as_array().and_then(|lists| lists.first()) else {
            continue;
        };
        let Some(content) = first["content"].as_str() else {
            continue;
        };
        let expected = waypoint_connectors::description_text(content);
        if expected.is_empty() {
            continue;
        }
        let flattened = row
            .description
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(flattened.contains(&expected));
        checked += 1;
    }
    assert!(checked > 0);
    println!("Checked requirement sections in {checked} captured public postings");
}

#[test]
#[ignore = "requires a separately captured public Lever API response"]
fn captured_lever_locations_preserve_all_alternatives() {
    let body =
        std::fs::read_to_string(std::env::var("WAYPOINT_LEVER_CAPTURE").expect("capture path"))
            .unwrap();
    let raw: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
    let parsed = parse_lever("spotify", &body).unwrap();
    let mut multiple = 0;
    for (row, source) in parsed.iter().zip(raw.iter()) {
        let Some(locations) = source
            .pointer("/categories/allLocations")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        if locations.len() > 1 {
            multiple += 1;
        }
        for location in locations {
            let Some(text) = location.as_str() else {
                continue;
            };
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                assert!(row
                    .location
                    .lines()
                    .any(|actual| actual.eq_ignore_ascii_case(&text)));
            }
        }
    }
    assert!(multiple > 0);
    println!("Checked {multiple} captured postings with multiple locations");
}
