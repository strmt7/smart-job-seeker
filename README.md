# smart-job-seeker

## Rust skeleton (main app parts)
This repository now includes an initial Rust workspace skeleton for the core job-seeker engine and a Windows app entrypoint.

### Workspace layout
- `/crates/smart_job_core` — core domain models, scoring, and ranking pipeline
- `/apps/windows_app` — Windows-oriented app entrypoint using the core crate
- `/docs/` — research, planning, and design deliverables

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
