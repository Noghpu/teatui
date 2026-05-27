---
id: 0000k-2026-05-27-16d1943b-generate-form-ux-and-status-bar-polish
created_at: 2026-05-27T15:29:07+02:00
created_by_model: claude-sonnet-4-6
state: reviewed
state_updated_at: 2026-05-27T16:03:36+02:00
---
# Generate Form UX and Status Bar Polish

## Goal
Improve the Generate screen's form pane with visual field separators and a field index counter, and clean up the status bar with `â”‚` dividers between segments.

## Context
The Generate form currently renders all fields as a continuous list of `label: value` lines with no visual separation. The status bar segments are space-joined with no dividers. This ticket applies targeted layout improvements to make the form scannable and the status bar easier to read at a glance.

This ticket depends on ticket `0000i` (Catppuccin Mocha colors) for the `colors::*` constants.

## Non-Goals
- No changes to form logic, field validation, or data model
- No changes to the landing hero screen (covered by `0000j`)
- No new fields or field types
- No changes to the preview pane

## Design Decisions
- **Field separators**: In `render_generate_fields`, after each field's lines (except the last), push a `Line::from("â”€".repeat(available_width))` in MUTED color. Since we don't know the exact width at render time, use a fixed-length dim separator: `Line::from("  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€  ").fg(colors::BORDER)`. This avoids needing to thread `Rect` dimensions into the function.
- **Field index on focused field**: In `render_generate_field`, when the field is both `selected` and `focused`, append `  (N / M)` to the header string, where `N` is `selected + 1` and `M` is `FieldId::ALL.len()`. Pass `selected_index` and `total_fields` as parameters, or compute them in the caller.
- **Status bar `â”‚` dividers**: In `render_status`, replace the plain `Vec` of spans with explicit `â”‚` separator spans between each segment. The `â”‚` spans use SURFACE1 fg color for a subtle visual break.

## Implementation Plan
1. **Field separator in `render_generate_fields`**:
   - After collecting lines for each field (except the last), push `Line::from("  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€  ").fg(colors::BORDER)`.

2. **Field index in `render_generate_field`**:
   - Add `total_fields: usize` parameter to `render_generate_field`.
   - When `focused` is true, change the header to include `  (N/M)` suffix: `format!("{header}  ({}/{})", index + 1, total_fields)`.
   - Update all call sites to pass `FieldId::ALL.len()`.

3. **Status bar `â”‚` dividers in `render_status`**:
   - After building the `segments` vec, interleave `" â”‚ ".fg(colors::SURFACE1)` spans between each segment.
   - Or build the `Line` directly with explicit separator spans inline.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "src/ui.rs", "src/generate.rs"],
  "likely_files": ["src/ui.rs"],
  "verification_commands": ["cargo build", "cargo check"],
  "review_focus": [
    "Separator lines appear between fields, not after the last one",
    "Field index counter shows (N/M) on focused field header only",
    "Status bar segments have â”‚ dividers between them",
    "No logic changes â€” purely visual",
    "colors::BORDER and colors::SURFACE1 used for subtle separators"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- A dim separator line appears between each field in the Generate form
- The focused field header shows `(N/M)` field index
- Status bar segments are separated by `â”‚` in SURFACE1 color
- `cargo build` clean with no new warnings

## Verification Plan
- `cargo build` clean
- Manual: open Generate screen, navigate fields â€” separators and index counter visible
- Manual: confirm separator does NOT appear after the last field
- Manual: check status bar shows `â”‚` dividers between all segments

## Files Likely Touched
- `src/ui.rs`

## Risks
- `render_generate_field` signature change (adding `total_fields`) requires updating all three call sites in `render_generate_fields` and `render_generate_editor`; easy to miss one
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-27T16:00:13+02:00
- state: implemented

## What was completed

All three visual improvements from the ticket plan were implemented in `src/ui.rs`:

1. **Field separators in `render_generate_fields`**: After each field's lines (except the last), a `Line::from("  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€  ").fg(colors::BORDER)` separator is pushed. Used `last = total.saturating_sub(1)` guard to skip the separator after the final field.

2. **Field index counter in `render_generate_field`**: Added `total_fields: usize` parameter. When `focused` is true, computes `index_suffix = format!("  ({n}/{})", total_fields)` using `generate.selected_field + 1` as N. The suffix is appended to the header string in all format branches. Updated all three call sites (in `render_generate_fields` and `render_generate_editor`'s `before`/`after` closures) to pass `FieldId::ALL.len()`.

3. **Status bar `â”‚` dividers in `render_status`**: Renamed the direct `segments` vec to `raw_segments`, then built a new `segments` vec by interleaving `Span::styled(" â”‚ ", Style::new().fg(colors::SURFACE1))` clones between each raw segment before rendering.

## Deviations from plan

None. Implementation followed the plan exactly. The separator string uses `â”€` (U+2500) box-drawing characters as intended by the ticket.

## Verification

- `cargo build` clean, no warnings.
- Manual inspection of logic: separator guard `index < last` correctly skips last field; focused index only shows when `focused == true`; dividers interleaved between all segments.

## Important files changed

- `C:\Users\pdao\projects\teatui-rs\teatui\src\ui.rs`

## Residual risks / follow-up

- Separator line is fixed-width (30 `â”€` chars). If the form pane is wider, it won't span the full width, but this matches the design decision in the ticket to use a fixed-length dim separator.
- The `(N/M)` counter uses `generate.selected_field` directly, which is correct when the focused field is the selected field (the only case where `focused=true` is passed).
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-27T16:03:36+02:00
- state: reviewed

# Review postmortem: 0000k generate form ux and status bar polish

## Verdict
Accepted with one minor polish applied during review.

## What was verified
- `cargo build` clean, no warnings.
- `cargo check` clean.
- Read full `src/ui.rs` and compared all touched functions against the ticket plan and acceptance criteria.

## Acceptance criteria
- Dim separator between each field (not after last): present at `render_generate_fields` lines 423-445. Uses `last = total.saturating_sub(1)` and `index < last` guard. Correct.
- `(N/M)` index counter on focused field header: present in `render_generate_field` lines 936-989. Only emitted when `focused == true`. All three call sites pass `FieldId::ALL.len()` for `total_fields`.
- Status bar `â”‚` dividers in SURFACE1: present in `render_status` lines 586-630 via interleaved `Span::styled(" â”‚ ", Style::new().fg(colors::SURFACE1))`, with the final divider correctly omitted via `i < last_idx`.
- `cargo build` clean with no new warnings: confirmed.

## Changes applied during review
The original implementation only added the `(N/M)` counter via `render_generate_field`. In Editing mode, `render_generate_editor` renders its own header (`â–¶ {label}`) for the focused field, bypassing `render_generate_field`, so the counter was missing exactly when the user is most engaged with a field. Added the suffix to the editor-mode header for consistent UX:

- `src/ui.rs` `render_generate_editor`: header now `format!("â–¶ {}  ({}/{})", selected.label(), app.generate().selected_field + 1, total)` using the already-computed `total = FieldId::ALL.len()`.

This is a literal extension of the ticket's intent ("field index counter shows (N/M) on focused field header") â€” the editor-mode header is the most focused state â€” and required no new parameters or surface changes.

## Observations (no action taken)
- Separator is a fixed 30-char run as the ticket explicitly designed. If the form pane is wider it won't span the full width; ticket intentionally avoids threading `Rect` width.
- `Span` clone of the divider in `render_status` is cheap (interior `Cow`/`Style`) â€” fine.
- `index_suffix` correctness relies on the call-site invariant that `focused == true` implies `selected == true` (so `generate.selected_field` is the right N). All three current call sites uphold this; if a future caller passes `focused=true` for a non-selected field the suffix would be wrong. Low risk given the small surface; not worth refactoring to take N as a parameter for now.

## Files touched in review
- `C:\Users\pdao\projects\teatui-rs\teatui\src\ui.rs`
