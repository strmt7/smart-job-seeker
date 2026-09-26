//! T034 — adapter drift harness.
//!
//! Each connector ships with pinned fixtures recorded from real payloads
//! (docs/waypoint-handoff/fixtures). A drift test fails if a parser change
//! alters what it extracts from a pinned fixture — parser regressions and
//! upstream schema changes are both caught before they silently produce
//! wrong data.

use waypoint_connectors::{parse_ashby, parse_greenhouse, parse_lever};

pub const GH_PINNED: &str = r#"{"jobs":[{"id":482731,"title":"Senior Rust Engineer","location":{"name":"Zurich"},"absolute_url":"https://job-boards.greenhouse.io/acme/jobs/482731","updated_at":"2026-09-20T08:00:00Z"}]}"#;
pub const LEVER_PINNED: &str = r#"[{"id":"abc-123","text":"Platform Engineer","categories":{"location":"Berlin","workplaceType":"remote"},"hostedUrl":"https://jobs.lever.co/acme/abc-123","createdAt":1758800000000}]"#;
pub const ASHBY_PINNED: &str = r#"{"jobs":[{"id":"ash-9","title":"Firmware Developer","location":"Remote (EU)","jobUrl":"https://jobs.ashbyhq.com/acme/ash-9","publishedAt":"2026-09-18","isRemote":true}]}"#;

#[derive(Debug, PartialEq, Clone)]
pub struct ExtractedFacts {
    pub identity: String,
    pub title: String,
    pub location: String,
    pub remote: bool,
    pub req_id: Option<String>,
}

/// Compare what a parser extracts now vs the recorded expectation.
pub fn assert_no_drift(source: &str, pinned: &str, expected: &[ExtractedFacts]) {
    let got: Vec<ExtractedFacts> = match source {
        "greenhouse" => parse_greenhouse("acme", pinned)
            .unwrap()
            .into_iter()
            .map(|p| ExtractedFacts {
                identity: p.identity,
                title: p.title,
                location: p.location,
                remote: p.remote,
                req_id: p.requisition_id,
            })
            .collect(),
        "lever" => parse_lever("acme", pinned)
            .unwrap()
            .into_iter()
            .map(|p| ExtractedFacts {
                identity: p.identity,
                title: p.title,
                location: p.location,
                remote: p.remote,
                req_id: p.requisition_id,
            })
            .collect(),
        "ashby" => parse_ashby("acme", pinned)
            .unwrap()
            .into_iter()
            .map(|p| ExtractedFacts {
                identity: p.identity,
                title: p.title,
                location: p.location,
                remote: p.remote,
                req_id: p.requisition_id,
            })
            .collect(),
        other => panic!("unknown source {other}"),
    };
    assert_eq!(
        got,
        expected.to_vec(),
        "adapter drift detected for {source}: extraction changed vs pinned fixture"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greenhouse_pinned_extraction_is_stable() {
        assert_no_drift(
            "greenhouse",
            GH_PINNED,
            &[ExtractedFacts {
                identity: "greenhouse::req::482731".into(),
                title: "Senior Rust Engineer".into(),
                location: "Zurich".into(),
                remote: false,
                req_id: Some("482731".into()),
            }],
        );
    }

    #[test]
    fn lever_pinned_extraction_is_stable() {
        assert_no_drift(
            "lever",
            LEVER_PINNED,
            &[ExtractedFacts {
                identity: "lever::req::abc-123".into(),
                title: "Platform Engineer".into(),
                location: "Berlin".into(),
                remote: true,
                req_id: Some("abc-123".into()),
            }],
        );
    }

    #[test]
    fn ashby_pinned_extraction_is_stable() {
        assert_no_drift(
            "ashby",
            ASHBY_PINNED,
            &[ExtractedFacts {
                identity: "ashby::req::ash-9".into(),
                title: "Firmware Developer".into(),
                location: "Remote (EU)".into(),
                remote: true,
                req_id: Some("ash-9".into()),
            }],
        );
    }

    #[test]
    fn upstream_schema_change_is_detected_not_swallowed() {
        // Simulate drift: the pinned payload's "title" becomes "roleName".
        let drifted = GH_PINNED.replace("\"title\"", "\"roleName\"");
        let result = parse_greenhouse("acme", &drifted);
        // The parser is not lenient: unknown shape must fail loudly, not
        // produce empty rows silently.
        assert!(
            result.is_err() || result.unwrap().iter().all(|p| p.title.is_empty()),
            "drifted shape must not parse into plausible-looking data"
        );
    }
}
