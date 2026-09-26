use waypoint_desktop::{JobRow, Screen, ShellViewModel};
use waypoint_domain::ApplicationState;

fn main() -> eframe::Result<()> {
    // 50k-row fixture to exercise G1 virtualization targets on first launch.
    let rows: Vec<JobRow> = (0..50_000)
        .map(|i| JobRow {
            identity: format!("job-{i}"),
            title: format!("Embedded Linux Engineer {i}"),
            employer: format!("Employer {}", i % 97),
            location: if i % 3 == 0 {
                "Remote (EU)".into()
            } else {
                "Zurich".into()
            },
            state: ApplicationState::Discovered,
            state_label: "discovered".into(),
            last_checked: Some("2026-09-25T10:00:00Z".into()),
        })
        .collect();

    waypoint_desktop::run(ShellViewModel {
        screen: Screen::Discovery,
        total_jobs: rows.len(),
        rows,
        notice: Some("G1 qualification fixture — no live sources connected".into()),
        coverage: None,
        model_status: Some(
            "Qualified on this machine: qwen3.5:9b — 13.23 GiB VRAM, zero CPU offload,              ~77 tok/s, structured output + reasoning pass (see docs/g1/QUALIFICATION.md)"
                .into(),
        ),
    })
}
