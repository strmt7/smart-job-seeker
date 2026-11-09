# Workspace design iteration — 2026-11-09

## Direction and review rounds

1. Generated `workspace-concept.png` with the built-in image generation tool:
   warm neutral canvas, forest-teal actions, job selection on the left and a
   focused preparation workspace on the right.
2. Generated `settings-concept.png` to refine the settings hierarchy, local-model
   controls and resource presentation.
3. Rendered the existing application headlessly, implemented shared visual tokens
   in `workspace-theme.css`, then reviewed desktop and narrow screenshots. A
   second implementation pass corrected workspace-tab spacing after inspection.

The images are design references containing fictional data. Their generated
version, status badges, resource readings and feature descriptions are not
product evidence. Neither image is a screenshot of the implementation. The
implemented pass refines the existing workspace, profile, settings and license
surfaces; it does not reproduce every element in either concept.

## Implemented and checked

- Warm canvas and separate job cards with clearer selected states and less
  whitespace around advisory text.
- A wider opportunity rail, a consistent header height, visible focus outlines,
  clearer workspace tabs, and a more distinct settings navigation surface.
- Shared theme tokens with an embedded Rust asset route, keeping styles local.
- A headless isolated Windows app session with fictional job rows; desktop
  (1440 × 1000) and narrow (390 × 844) layouts had no script errors or horizontal
  page overflow. A keyboard path through navigation was exercised. The compiled
  theme route was checked through its applied CSS token.

Screenshots remain under ignored `target/` because settings can display local
paths. No employer application pages were contacted during this design work.

## Next design and implementation rounds

1. Simplify search/filter density and preserve filter state across workspaces.
2. Extend the searchable document library and selected-template badges into a
   focused role-specific review layout, including
   PDF preview, factual review, revisions and outstanding checks. PDF rows now
   expose saved draft-context and visual-review evidence for a selected job;
   this remains an evidence snapshot rather than a readiness certificate. Saved
   visual findings can now be opened after a document-byte check, with historical
   context distinguished from current context.
3. Reorganize local-tool setup around actual detected states and distinct actions;
   avoid implying a dependency is installed before checking it.
4. Extend the mobile Jobs/Workspace dock with long-form review usability checks.
   Its initial implementation preserves the active section, hash and filter and
   was checked headlessly with keyboard activation and narrow screenshots.
5. Refactor remaining legacy CSS rules after broader state coverage, including
   empty, loading, error, lengthy labels, dialogs and keyboard focus.

## Generation prompt set

Both references used built-in image generation, with an opaque background and
no external API key. The prompts requested original, achievable HTML/CSS UI,
fictional job data, a warm ivory canvas, ink typography and forest-teal accents.

**Workspace prompt:** High-fidelity local-first Windows job seeking interface,
compact Waypoint top bar, left job-selection list and right selected-role detail;
Overview / Fit evidence / Materials / Form review tabs; CV and cover-letter PDF
rows; restrained review indicators; Prepare selected and Review fields actions;
precise alignment, readable typography, no photography, glassmorphism or
decorative backgrounds.

**Settings refinement prompt:** High-fidelity Waypoint web-app settings screen,
job list retained on the left, secondary navigation for personal details,
preferences, documents, storage and local AI; model selection and start/status
controls, resource estimates explicitly distinguished from reservations, document
tool status actions, no native window chrome or version claims, consistent
spacing and rounded neutral surfaces.
