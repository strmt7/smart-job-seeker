# Smart Jobseeker interface refinement — 2026-11-09

The subsequent [career-history pass](../../CANDIDATE_HISTORY.md) adds an original
`career-history-reference.png`, a responsive employment/education editor and
populated-state workflow, resize and actual-browser-zoom checks. That reference
is a hierarchy guide; measured behavior and remaining integration are recorded
in the linked capability note.

`settings-reference.png` is an original generated design reference, not a product
screenshot. Three image-generation rounds refined the workbench: an overall
jobs/documents layout, a renamed settings screen, then a settings hierarchy
matching the implemented Appearance and Storage sections. Discarded variants
contained illustrative controls and were not used as implementation evidence.

The current interface implements the shared ivory/forest-teal palette, settings
navigation, spacing and motion controls, mouse-drag dividers, resized PDF/text
review and bounded license dialog. The generated reference still includes
illustrative job filters and a folder-picker icon that are not implemented as
pictured. Those differences remain design work; the image is not a readiness
or pixel-parity claim.

## Final prompt and method

Built-in image generation edited the preceding generated reference, with an
opaque background. The final prompt requested Smart Jobseeker branding, primary
Profile/Documents/Applications/Settings navigation, a vertical settings menu,
Appearance dropdowns for Standard/Compact spacing and System/Reduced motion,
per-tab persistence wording, Reset panel sizes, a Windows workspace-path field,
Storage and Email cards, and a restrained mouse resize grip. It excluded tiny
model suggestions, fabricated resource readings, readiness claims, chatbot
ornaments, personal information and credentials.

## Interaction contract

- `workspace-theme.css` owns shared colors, focus/hover/pressed states, short
  transition timings and reduced-motion behavior. Avoid isolated animation rules.
- `layout.js` owns pane sizing and appearance preferences. Resizing uses mouse
  dragging with visible grips/cursors and immediate movement; double-click resets.
  Bounds protect both neighboring panes and switch to stacked layouts when needed.
- Standard and compact spacing and reduced motion work across the current UI.
  These preferences and pane proportions persist for this browser tab. They do
  not change candidate facts, approvals or application state.
- Secondary buttons use a shared treatment. Settings navigation marks the current
  location. Switching workspace sections resets stale pane scroll; reopening the
  current section preserves its reading position.
- Application labels describe actual actions and states. Do not create decorative
  settings, fake model-success readings or unimplemented actions to match a mockup.

## Verification and limits

The compiled local app was exercised headlessly with fictional jobs/documents,
mocked PDF preview responses and actual Chromium browser zoom. The matrix spans
physical viewports from 320 to 3840 pixels, 80–400% zoom where effective space is
at least 320 × 240 CSS pixels, four workspace sections and the license dialog.
Separate gestures cover main/library/PDF dividers, native text/PDF height and
license-dialog resizing, interrupted drags, reset and tab-local preferences.
The focused Edge pass also checks computed transition and pressed styles with
application and system reduced-motion preferences.
Screenshots informed follow-up fixes for narrow settings and clipped headings.

The resulting 295 Chrome cases and 16 focused Edge cases check selected geometry
and interactions, not every
possible UI state, browser, DPI configuration or accessibility behavior. They do
not qualify employer forms, local-model accuracy or clean-Windows installation.
Screenshots and raw results stay under ignored `target/` because storage settings
can show local paths. No real application was opened or submitted.

The desktop patterns were reviewed from [Microsoft GridSplitter guidance](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/controls/how-to-resize-columns-with-a-gridsplitter)
and [VS Code layout documentation](https://code.visualstudio.com/docs/configure/custom-layout).
No implementation code was copied from them.
