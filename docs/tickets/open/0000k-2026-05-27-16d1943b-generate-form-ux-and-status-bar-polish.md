---
id: 0000k-2026-05-27-16d1943b-generate-form-ux-and-status-bar-polish
created_at: 2026-05-27T15:29:07+02:00
created_by_model: claude-sonnet-4-6
state: open
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
