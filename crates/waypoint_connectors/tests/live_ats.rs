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
#[ignore = "network test; run explicitly with --ignored"]
fn live_lever_board_parses() {
    // Verified live board (cloudflare/netflix 404 on this endpoint).
    let body = get("https://api.lever.co/v0/postings/spotify?mode=json").unwrap();
    let parsed = parse_lever("spotify", &body).unwrap();
    assert!(!parsed.is_empty(), "spotify lever board should list jobs");
    assert!(parsed.iter().all(|p| p.identity.starts_with("lever::")));
    eprintln!("lever live rows: {}", parsed.len());
}
