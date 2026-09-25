# smart-job-seeker

## Rust skeleton (main app parts)
This repository contains a Rust-first app skeleton for a high-efficiency job-seeker platform.

### Workspace layout
- `/crates/smart_job_core`
  - domain models
  - scoring + ranking pipeline
  - repository abstraction
  - in-memory adapter
  - SQLite adapter
  - application service layer
- `/apps/windows_app` — Windows-oriented executable entrypoint
- `/docs/` — research, planning, and design deliverables

### Architecture principles used
- **Separation of concerns** between domain, persistence, and orchestration.
- **Dependency inversion** via repository traits.
- **Deterministic outputs** for stable ranking behavior.
- **Local-first persistence** with SQLite adapter.

### Quick start
From repository root:

```bash
cargo test
cargo run -p windows_app
```

## Research and planning deliverables
- Research summary: `/docs/research/open-source-job-seeker-landscape.md`
- Rust-first platform plan: `/docs/plan/rust-enterprise-job-seeker-platform-plan.md`
- UI/system design + image prompts: `/docs/design/ui-system-design-and-image-prompts.md`
- Zipped design pack: `/docs/design/job-seeker-design-pack.zip`
