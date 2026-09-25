# UI/System Design Pack (Agent-Ready)

## Design system goals
- Enterprise-grade clarity, fast keyboard workflows, high information density.
- Visual style: modern Windows-native feel, dark/light themes, accessible contrast.
- Core principles: evidence-first UI, confidence indicators, action traceability.

## Primary windows
1. **Mission Control Dashboard**
   - KPI strip: applications sent, response rate, interview rate, offer pipeline.
   - Priority queue: top recommended next actions with confidence + rationale.
2. **Opportunity Intelligence Board**
   - Job cards with fit score, risk score, freshness, source citations.
   - Side panel with detailed explainability and missing-skill map.
3. **Application Studio**
   - Resume tailoring diff view (before/after), ATS-compatibility checks.
   - Cover letter and recruiter-message composer with policy checks.
4. **Interview & Negotiation Lab**
   - Mock interview simulator, answer critique, negotiation playbook.
5. **Audit & Policy Center**
   - Full model/action log with replay and provenance graph.

## Image-generation prompt pack (for GPT image model)
Use these prompts to generate high-fidelity mockups and export PNGs/SVGs.

### Prompt A — Dashboard
"Design a visually stunning Windows desktop app dashboard for an AI job-seeker platform. Enterprise-grade UI, dark mode, card-based analytics, confidence badges, source-citation chips, and action queue. Include charts for application funnel, callback trend, interview conversion. Polished, modern, clean typography, accessibility-first contrast, realistic shadows, 16:9 canvas."

### Prompt B — Opportunity Intelligence Board
"Create a Windows desktop screen for job opportunity triage. Left column filters, middle list of job cards with fit/risk/freshness scores, right explainability panel with citation links and missing skills radar chart. Professional enterprise design system, high data density, elegant spacing, productivity-focused, dark mode and glass accents."

### Prompt C — Application Studio
"Create a desktop UI for AI-assisted resume tailoring. Side-by-side diff editor, ATS score meter, highlight of rewritten bullets, policy compliance checks, and one-click export workflow. Premium enterprise software aesthetic, Rust-powered app branding, subtle gradients, crisp iconography, keyboard shortcut hints."

### Prompt D — Audit & Provenance
"Design a Windows screen showing AI decision provenance graph: recommendation node linked to sources, prompts, model version, confidence and action logs. Include timeline and replay button. Look like enterprise observability + governance software, visually appealing and trustworthy."

## Packaging expectations for agents
- Generate final renders in `docs/design/mockups/`.
- Keep both light and dark variants for key windows.
- Maintain consistent spacing scale and typography across screens.
