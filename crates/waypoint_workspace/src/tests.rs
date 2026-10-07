//! Pipeline tests. These are the product's honesty contract: every failure
//! mode must be *reported*, never silently converted into an empty result.

use super::*;

const GH_URL: &str = "https://boards-api.greenhouse.io/v1/boards/vercel/jobs";
const LEVER_URL: &str = "https://api.lever.co/v0/postings/spotify?mode=json";
const ASHBY_URL: &str = "https://api.ashbyhq.com/posting-api/job-board/ashby";

fn greenhouse_payload(title: &str) -> String {
    format!(
        r#"{{"jobs":[
          {{"id":401,"title":"{title}","absolute_url":"https://boards.greenhouse.io/vercel/jobs/401",
            "location":{{"name":"Remote"}},"updated_at":"2026-09-20T10:00:00Z"}},
          {{"id":402,"title":"Data Engineer","absolute_url":"https://boards.greenhouse.io/vercel/jobs/402",
            "location":{{"name":"Berlin, Germany"}},"updated_at":"2026-09-21T10:00:00Z"}}
        ]}}"#
    )
}

fn lever_payload() -> String {
    // Field names mirror Lever's real payload (camelCase).
    r#"[{"id":"lv-1","text":"Site Reliability Engineer","hostedUrl":"https://jobs.lever.co/spotify/lv-1",
        "categories":{"location":"Stockholm","workplaceType":"remote"},"createdAt":1758900000000}]"#
        .to_string()
}

fn ashby_payload() -> String {
    // Field names mirror Ashby's real payload (camelCase).
    r#"{"jobs":[{"id":"ab-1","title":"Rust Platform Engineer","jobUrl":"https://jobs.ashbyhq.com/ashby/ab-1",
        "location":"Zurich","isRemote":false,"publishedAt":"2026-09-19"}]}"#
        .to_string()
}

fn fetcher_all() -> StaticFetcher {
    StaticFetcher::default()
        .route(GH_URL, &greenhouse_payload("Backend Engineer"))
        .route(LEVER_URL, &lever_payload())
        .route(ASHBY_URL, &ashby_payload())
}

fn targets() -> Vec<DiscoveryTarget> {
    vec![
        DiscoveryTarget::new("greenhouse", "vercel"),
        DiscoveryTarget::new("lever", "spotify"),
        DiscoveryTarget::new("ashby", "ashby"),
    ]
}

#[test]
fn discovery_persists_parses_and_dedups_across_runs() {
    let mut ws = Workspace::in_memory().unwrap();
    let f = fetcher_all();

    let first = ws.discover(&targets(), &f, 1_758_900_000);
    assert_eq!(first.inserted, 4, "4 postings across three boards");
    assert_eq!(first.parsed, 4);
    assert_eq!(first.drift, 0);
    assert_eq!(first.rejected_urls, 0);
    assert!(
        ws.coverage().all_fresh_and_ok(),
        "no degraded source expected"
    );
    assert_eq!(ws.job_count().unwrap(), 4);

    // Identical re-run: nothing new, nothing duplicated.
    let second = ws.discover(&targets(), &f, 1_758_900_600);
    assert_eq!(second.inserted, 0);
    assert_eq!(second.unchanged, 4);
    assert_eq!(ws.job_count().unwrap(), 4, "re-run must not duplicate rows");

    // The summary always carries a denominator.
    let summary = second.summary();
    assert!(summary.contains("from 3 sources"), "got: {summary}");
}

#[test]
fn changed_posting_content_creates_a_new_observation_not_a_duplicate() {
    let mut ws = Workspace::in_memory().unwrap();
    let f1 = fetcher_all();
    ws.discover(&targets(), &f1, 1_758_900_000);

    // Same requisition id 401, revised title → new observation, same job row.
    let f2 = StaticFetcher::default().route(GH_URL, &greenhouse_payload("Senior Backend Engineer"));
    let second = ws.discover(
        &[DiscoveryTarget::new("greenhouse", "vercel")],
        &f2,
        1_758_906_400,
    );
    assert_eq!(second.updated, 1, "only req 401's content changed");
    assert_eq!(second.unchanged, 1, "req 402 is byte-identical");
    assert_eq!(
        ws.job_count().unwrap(),
        4,
        "identity is the requisition, not the text"
    );

    let all = ws.jobs().unwrap();
    let j401 = all
        .iter()
        .find(|j| j.requisition_id.as_deref() == Some("401"))
        .expect("req 401 present");
    assert_eq!(j401.title, "Senior Backend Engineer");
    assert_eq!(j401.last_seen.as_deref(), Some("1758906400"));
}

#[test]
fn re_discovery_never_regresses_an_advanced_application() {
    let mut ws = Workspace::in_memory().unwrap();
    let f = fetcher_all();
    ws.discover(&targets(), &f, 1_758_900_000);

    let id = ws
        .jobs()
        .unwrap()
        .into_iter()
        .find(|j| j.requisition_id.as_deref() == Some("401"))
        .unwrap()
        .id;
    assert!(ws.set_state(&id, ApplicationState::Shortlisted).unwrap());
    assert!(ws.set_state(&id, ApplicationState::Prepared).unwrap());

    ws.discover(&targets(), &f, 1_758_900_600);

    let after = ws.job(&id).unwrap().unwrap();
    assert_eq!(
        after.state,
        ApplicationState::Prepared,
        "re-discovery must not reset a user-driven state"
    );
}

#[test]
fn illegal_state_transition_is_refused() {
    let mut ws = Workspace::in_memory().unwrap();
    ws.discover(&targets(), &fetcher_all(), 1_758_900_000);
    let id = ws.jobs().unwrap()[0].id.clone();

    // Discovered -> Confirmed skips the whole pipeline and must be refused.
    let err = ws.set_state(&id, ApplicationState::Confirmed).unwrap_err();
    assert!(matches!(err, WorkspaceError::IllegalTransition { .. }));
    assert_eq!(
        ws.job(&id).unwrap().unwrap().state,
        ApplicationState::Discovered
    );

    let missing = JobIdentityId::new("nope").unwrap();
    assert!(matches!(
        ws.set_state(&missing, ApplicationState::Shortlisted)
            .unwrap_err(),
        WorkspaceError::NotFound(_)
    ));
}

#[test]
fn forbidden_source_is_reported_with_a_reason_never_silently_skipped() {
    let mut ws = Workspace::in_memory().unwrap();
    let out = ws.discover(
        &[DiscoveryTarget::new("linkedin", "any")],
        &StaticFetcher::default(),
        1_758_900_000,
    );
    assert_eq!(out.fetched, 0, "policy must prevent the fetch entirely");
    let s = &out.coverage.sources[0];
    assert_eq!(s.state, SourceFetchState::Unsupported);
    assert!(
        s.detail.as_deref().unwrap_or("").contains("not permitted"),
        "reason must be user-visible: {s:?}"
    );
    assert!(!ws.coverage().all_fresh_and_ok());
}

#[test]
fn review_gated_source_is_also_reported_not_attempted() {
    let mut ws = Workspace::in_memory().unwrap();
    let out = ws.discover(
        &[DiscoveryTarget::new("workday", "acme")],
        &StaticFetcher::default(),
        1_758_900_000,
    );
    assert_eq!(out.fetched, 0);
    assert_eq!(out.coverage.sources[0].state, SourceFetchState::Unsupported);
}

#[test]
fn fetch_failures_are_classified_as_denied_or_network() {
    let mut ws = Workspace::in_memory().unwrap();
    let f = StaticFetcher::default()
        .failing(GH_URL, FetchFailure::AccessDenied("http 404".into()))
        .failing(LEVER_URL, FetchFailure::Network("timeout".into()))
        .route(ASHBY_URL, &ashby_payload());

    let out = ws.discover(&targets(), &f, 1_758_900_000);
    let by_source = |name: &str| {
        out.coverage
            .sources
            .iter()
            .find(|s| s.source == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        by_source("greenhouse").state,
        SourceFetchState::AccessDenied
    );
    assert_eq!(by_source("lever").state, SourceFetchState::NetworkError);
    assert_eq!(by_source("ashby").state, SourceFetchState::Ok);
    assert_eq!(by_source("ashby").results, 1);
    assert!(!out.coverage.all_fresh_and_ok());
}

#[test]
fn parser_drift_is_reported_as_drift_not_as_empty() {
    let mut ws = Workspace::in_memory().unwrap();
    let f = StaticFetcher::default().route(GH_URL, "<html>maintenance page</html>");
    let out = ws.discover(
        &[DiscoveryTarget::new("greenhouse", "vercel")],
        &f,
        1_758_900_000,
    );

    assert_eq!(out.drift, 1);
    let s = &out.coverage.sources[0];
    assert_eq!(s.state, SourceFetchState::ParserDrift);
    assert_ne!(
        s.state,
        SourceFetchState::Empty,
        "drift is not 'no jobs today'"
    );
    assert!(s.detail.is_some());
}

#[test]
fn genuinely_empty_board_is_reported_as_empty_and_verified() {
    let mut ws = Workspace::in_memory().unwrap();
    let f = StaticFetcher::default().route(GH_URL, r#"{"jobs":[]}"#);
    let out = ws.discover(
        &[DiscoveryTarget::new("greenhouse", "vercel")],
        &f,
        1_758_900_000,
    );
    assert_eq!(out.coverage.sources[0].state, SourceFetchState::Empty);
    assert!(
        out.coverage.all_fresh_and_ok(),
        "empty-but-verified is not degraded"
    );
}

#[test]
fn exhausted_fetch_budget_is_refused_and_reported() {
    let mut ws = Workspace::in_memory()
        .unwrap()
        .with_budgets(ResourceBudgets {
            global_fetch: 0,
            ..ResourceBudgets::default()
        });
    let out = ws.discover(&targets(), &fetcher_all(), 1_758_900_000);
    assert_eq!(out.budget_refused, 3);
    assert_eq!(out.inserted, 0);
    for s in &out.coverage.sources {
        assert_eq!(s.state, SourceFetchState::NetworkError);
        assert!(s.detail.as_deref().unwrap_or("").contains("budget"));
    }
    assert!(
        !out.coverage.all_fresh_and_ok(),
        "a refused run must never look like a clean empty search"
    );
}

#[test]
fn urls_arriving_inside_third_party_payloads_are_distrusted() {
    let mut ws = Workspace::in_memory().unwrap();
    // A payload that tries to make us store a loopback/private URL.
    let hostile = r#"{"jobs":[
      {"id":1,"title":"Internal","absolute_url":"http://127.0.0.1:8080/admin",
       "location":{"name":"Remote"},"updated_at":"2026-09-20"},
      {"id":2,"title":"Also internal","absolute_url":"https://10.0.0.7/jobs/2",
       "location":{"name":"Remote"},"updated_at":"2026-09-20"},
      {"id":3,"title":"Legit","absolute_url":"https://boards.greenhouse.io/vercel/jobs/3",
       "location":{"name":"Remote"},"updated_at":"2026-09-20"}
    ]}"#;
    let f = StaticFetcher::default().route(GH_URL, hostile);
    let out = ws.discover(
        &[DiscoveryTarget::new("greenhouse", "vercel")],
        &f,
        1_758_900_000,
    );

    assert_eq!(out.rejected_urls, 2, "private URLs must be dropped");
    assert_eq!(out.inserted, 1);
    assert_eq!(ws.job_count().unwrap(), 1);
    let stored = &ws.jobs().unwrap()[0];
    assert!(stored
        .canonical_url
        .as_deref()
        .unwrap()
        .starts_with("https://boards.greenhouse.io/"));
}

#[test]
fn same_requisition_under_a_new_url_becomes_an_alias_not_a_second_job() {
    let mut ws = Workspace::in_memory().unwrap();
    let first_url = "https://boards.greenhouse.io/vercel/jobs/401";
    let second_url = "https://boards.greenhouse.io/vercel/jobs/401?gh_src=mirror";
    let payload = |url: &str| {
        format!(
            r#"{{"jobs":[{{"id":401,"title":"Backend Engineer","absolute_url":"{url}",
                "location":{{"name":"Remote"}},"updated_at":"2026-09-20"}}]}}"#
        )
    };

    ws.discover(
        &[DiscoveryTarget::new("greenhouse", "vercel")],
        &StaticFetcher::default().route(GH_URL, &payload(first_url)),
        1_758_900_000,
    );
    let out = ws.discover(
        &[DiscoveryTarget::new("greenhouse", "vercel")],
        &StaticFetcher::default().route(GH_URL, &payload(second_url)),
        1_758_900_600,
    );

    assert_eq!(
        ws.job_count().unwrap(),
        1,
        "same requisition is one opportunity"
    );
    assert_eq!(out.inserted, 0);
    let id = ws.jobs().unwrap()[0].id.clone();
    // The alias graph is usable by the caller: a mirrored URL resolves back.
    assert_eq!(
        ws.resolve_alias(second_url).unwrap(),
        Some(id.as_str().to_string())
    );
}

#[test]
fn ranked_separates_strict_results_from_the_excluded_bucket() {
    let mut ws = Workspace::in_memory().unwrap();
    ws.discover(&targets(), &fetcher_all(), 1_758_900_000);

    let skills = vec!["rust".to_string(), "sqlite".to_string()];

    // No constraints: everything is strict, and matching is explainable.
    let all = ws
        .ranked(&CampaignConstraints::default(), &skills, 1_758_900_100)
        .unwrap();
    assert_eq!(all.strict.len(), 4);
    assert!(all.excluded.is_empty());
    assert_eq!(
        all.strict[0].job.title, "Rust Platform Engineer",
        "best evidence match ranks first"
    );
    assert!(all.strict[0].matched_skills.contains(&"rust".to_string()));
    assert!(!all.strict[0].matched_skills.contains(&"sqlite".to_string()));

    // Remote-only: exactly one posting states remote, so exactly one is strict.
    let remote_only = CampaignConstraints {
        remote_only: true,
        ..CampaignConstraints::default()
    };
    let r1 = ws.ranked(&remote_only, &skills, 1_758_900_100).unwrap();
    assert_eq!(r1.strict.len(), 1, "only the lever posting states remote");
    assert_eq!(r1.strict[0].job.source, "lever");
    assert_eq!(r1.excluded.len(), 3, "exclusions are visible, never silent");
    assert!(
        r1.excluded
            .iter()
            .all(|e| e.reasons.iter().any(|x| x.starts_with("remote:"))),
        "each exclusion must carry its remote: reason"
    );

    // Adding a salary floor: no adapter states a salary, so *nothing* may pass.
    // This is the "unknown never silently passes a hard constraint" rule.
    let with_salary = CampaignConstraints {
        remote_only: true,
        salary: Some((10_000_000, 20_000_000)),
        ..CampaignConstraints::default()
    };
    let r2 = ws.ranked(&with_salary, &skills, 1_758_900_100).unwrap();
    assert!(
        r2.strict.is_empty(),
        "an unstated salary must never satisfy a stated salary floor"
    );
    assert_eq!(r2.excluded.len(), 4);
    assert!(r2
        .excluded
        .iter()
        .any(|e| e.reasons.iter().any(|x| x.starts_with("salary:"))));
}

#[test]
fn freshness_uses_the_observation_time_not_the_publication_date() {
    let mut ws = Workspace::in_memory().unwrap();
    ws.discover(&targets(), &fetcher_all(), 1_758_900_000);
    let fresh = ws
        .ranked(&CampaignConstraints::default(), &[], 1_758_900_100)
        .unwrap();
    assert!(matches!(fresh.strict[0].freshness, Freshness::Fresh { .. }));

    // Same data, viewed 10 days later: stale, and it says so.
    let later = ws
        .ranked(
            &CampaignConstraints::default(),
            &[],
            1_758_900_000 + 10 * 24 * 3600,
        )
        .unwrap();
    assert!(matches!(later.strict[0].freshness, Freshness::Stale { .. }));
}

/// Live end-to-end run: real HTTPS, real adapters, real store.
/// `cargo test -p waypoint_workspace -- --ignored`
#[test]
#[ignore = "requires network access"]
fn live_discovery_end_to_end() {
    let mut ws = Workspace::in_memory().unwrap();
    let out = ws.discover(
        &[
            DiscoveryTarget::new("greenhouse", "vercel"),
            DiscoveryTarget::new("ashby", "ashby"),
        ],
        &LiveFetcher::default(),
        1_758_900_000,
    );
    println!("{}", out.summary());
    assert!(
        out.inserted > 10,
        "expected real postings, got {}",
        out.inserted
    );
    assert_eq!(out.drift, 0, "live payloads must match the pinned adapters");
    assert_eq!(out.rejected_urls, 0);

    let ranked = ws
        .ranked(
            &CampaignConstraints::default(),
            &["rust".to_string(), "python".to_string()],
            1_758_900_000,
        )
        .unwrap();
    assert_eq!(ranked.strict.len(), out.inserted);
    println!(
        "top: {} @ {} (score {:.2}, matched {:?})",
        ranked.strict[0].job.title,
        ranked.strict[0].job.employer,
        ranked.strict[0].score,
        ranked.strict[0].matched_skills
    );
}
