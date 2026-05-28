---
id: 0000m-2026-05-27-70bb2caf-generate-screen-polish
created_at: 2026-05-27T20:56:42+02:00
created_by_model: claude-sonnet-4-6/normal
state: implemented
state_updated_at: 2026-05-28T08:09:16+02:00
---
# Generate Screen Polish: Left Col Changes, Center Width, Right Col Cleanup

## Goal

Three improvements to the Generate PR screen:

1. **Left column**: Replace the revset-label+count+bookmarks list display with a change-centric display that shows useful identity information (description, bookmark, or change_id) instead of raw revset expressions.
2. **Center column**: Fix the TextArea editor so it fills the full inner column width and correctly resizes after a terminal resize event.
3. **Right column**: Remove the `"focused field: ..."` debug line that was only useful during development.

## Context

All rendering lives in `src/ui.rs`. `src/generate.rs` owns `RevsetSummary` and the `GenerateState` types.

### Left col: current vs desired

Current rendering (`ui.rs` `render_menu`, Generate branch, ~line 228-252):

```rust
let label = if bookmarks.is_empty() {
    format!("{}  {} commits", revset.label(), revset.commit_count())
} else {
    format!("{}  {} commits  {}", revset.label(), revset.commit_count(), bookmarks)
};
```

This shows raw jj revset expressions (`@`, `@-`, `heads(trunk()..)`) which are not meaningful at a glance.

The `RevsetSummary` struct (`generate.rs:349`) has:
- `label()` â€” revset expression (e.g. `@`, `@-`)
- `description()` â€” first commit's first-line description (e.g. `"feat: add config loader"`)
- `bookmarks()` â€” slice of bookmark name strings
- `change_ids()` â€” slice of change ID short strings
- `commit_count()` â€” number of commits

Desired display logic per list item (one line, fits the 28-char column, ~22 inner chars after border + padding):

1. **Primary identifier** (in TEXT colour for unselected, ACCENT for selected):
   - If `description()` is non-empty and not a jj default like `"(no description set)"`: use description, truncated.
   - Else if `bookmarks()` is non-empty: use the first bookmark name.
   - Else: use the first entry of `change_ids()` abbreviated.
2. **Secondary tag** (muted, appended after primary):
   - If primary is description and bookmarks non-empty: show first bookmark in `[...]`.
   - If primary is description and no bookmarks: show nothing.
   - If primary is bookmark or change_id: show revset label in muted.
3. Selection marker: use existing `list_item` helper; format the label string as `{primary} {secondary_tag}`.

Because the column width is 28 chars but terminal-dependent, the primary identifier should be truncated to a safe maximum. Pass `area` to the render function so it can compute `inner_width = area.width.saturating_sub(4)` (2 borders + 2 padding).

The title of the List widget should change from `"Revsets"` to `"Changes"`.

### Center col: TextArea resize fix

The `TextArea` is stored in `FieldState::editor` (`generate.rs`). When editing, `render_generate_editor` renders it into `editor_area`, a `Rect` derived from the current `frame.area()`. The layout recalculates on every frame, so `editor_area.width` is always current. However, `ratatui_textarea` maintains an internal scroll/viewport state tied to cursor position. After an initial render in a narrow window, the viewport offset may remain stale after the terminal widens.

Fix: `AppEvent::Resize` is already delivered to `app.rs:160` but does nothing (`AppEvent::Tick | AppEvent::Resize => {}`). Add a resize handler that iterates all `FieldState` editors and resets their viewport. If `ratatui_textarea::TextArea` exposes a viewport reset method, use it. If not, recreating the editor from its current text via `textarea_from_text` (which resets cursor to end) is the safe fallback.

The separator line in `render_generate_fields`:

```rust
Line::from("  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€  ").fg(colors::BORDER)
```

is a hardcoded 36-char string. Change it to use a dynamically computed length so the separator fills the column. This requires passing `area` to `render_generate_fields` and `render_generate_field`.

### Right col: remove debug line

`render_generate_preview` (`ui.rs:1010`):

```rust
Line::from(format!("focused field: {}", generate.selected_field_name())),
```

Remove this line. It was a dev-time aid that echoes the currently selected form field name and has no user value.

## Non-Goals

- Redesigning the list layout with multi-line items per revset.
- Adding a full-width styled input box around each form field in normal (non-editing) mode.
- Changing how `RevsetSummary` is populated in `jj.rs`.

## Design Decisions

- The jj default description `"(no description set)"` should be treated as empty (fall through to bookmark or change_id). Check with a case-insensitive contains or exact equality on a trimmed value.
- `AppEvent::Resize` already exists; the resize fix adds handling there rather than touching the TextArea on every tick.
- Separator width: `area.width.saturating_sub(6)` accounts for 2 borders + 2 padding + 2 leading spaces in the line.
- Truncation: use `inner_width.saturating_sub(tag_len + 1)` chars for the primary identifier where `inner_width = area.width.saturating_sub(4)`.

## Implementation Plan

1. `src/ui.rs` `render_menu`: Rename List title from `"Revsets"` to `"Changes"`. Replace the label format logic with the smart display function. Pass `area` to compute truncation width.
2. `src/ui.rs` `render_generate_fields` / `render_generate_field`: Add `area: Rect` parameter. Replace the hardcoded separator string with a dynamic-width version.
3. `src/ui.rs` `render_work`: Update call sites for `render_generate_fields` to pass `area`.
4. `src/app.rs` resize handler: On `AppEvent::Resize`, iterate `generate.form` fields and reset each editor viewport.
5. `src/generate.rs` `PrForm`: Add a method like `editors_mut()` that yields all `FieldState` mutably, so the resize handler can reset them without enumerating by `FieldId` manually.
6. `src/ui.rs` `render_generate_preview`: Delete the `"focused field: ..."` line (~line 1010).

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/ui.rs", "src/generate.rs", "src/app.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "Left col list title is Changes not Revsets",
    "Left col items show description > bookmark > change_id with muted secondary tag; raw revset labels are gone from item text",
    "Description matching (no description set) treated as absent",
    "Separator line width is dynamic based on area width",
    "TextArea resize: test by making the window narrow with editor active, then widening - text should fill the new width",
    "Right col focused field line is removed",
    "No compilation errors; just verify passes"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria

- Left column in Generate PR shows change descriptions (or bookmarks/change_ids when no description) instead of raw revset expressions. Title reads "Changes".
- Separator lines in the form fill the column width at any terminal size.
- After resizing the terminal from narrow to wide while editing a field, the TextArea visually fills the new column width.
- The right column no longer shows a "focused field: ..." line in any Generate sub-state.

## Verification Plan

- `just verify` passes.
- Visual check: launch app; left col shows descriptions/bookmarks instead of `@  3 commits`.
- Visual check: resize terminal narrow then wide while in the Generate screen; separator lines and editor area fill the column.
- Visual check: right column while in Generate/DraftReady no longer shows "focused field".

## Files Likely Touched

- `src/ui.rs` â€” left col list, form separator, editor resize handling, right col line removal
- `src/generate.rs` â€” possibly adding a helper method to `RevsetSummary` or `PrForm` for smart display label or editor iteration
- `src/app.rs` â€” resize handler to reset TextArea viewports

## Risks

- `render_generate_fields` currently takes `app: &App` not `area: Rect` â€” passing area through requires a signature change and updates to callers in `render_work`.
- TextArea viewport reset API may not be public in `ratatui_textarea`. Check the crate docs before writing the handler; if unavailable, recreating the editor from text is the safe fallback (cursor resets to end, acceptable).
- The 28-char column width is a `Constraint::Length(28)` in the horizontal layout. The actual inner width is `28 - 2 (borders) - 2 (padding) = 24` â€” use this as the baseline for truncation.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-28T08:09:16+02:00
- state: implemented

## What was completed

All three improvements to the Generate PR screen were implemented:

1. **Left column - change-centric display**: Replaced raw revset expression labels with a smart display that shows description > bookmark > change_id as the primary identifier, with muted secondary tags. The list title was renamed from "Revsets" to "Changes". A `revset_display_label()` function was added in `ui.rs` with `is_jj_default_description()` helper. The `RevsetSummary` type was added to imports.

2. **Center column - dynamic separator width**: Changed `render_generate_fields` signature to accept `area: Rect` and replaced the hardcoded `"  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€  "` separator with a dynamically computed string using `"â”€".repeat(area.width.saturating_sub(6))`. The call site in `render_work` was updated to pass `area`.

3. **Right column - debug line removed**: Removed the `Line::from(format!("focused field: {}", generate.selected_field_name()))` line from `render_generate_preview`.

4. **TextArea resize fix**: Added `FieldState::reset_editor_viewport()` method in `generate.rs` that recreates the editor from the current buffer text (safe fallback since `ratatui_textarea` 0.3.2 has no public viewport reset API). Added `PrForm::editors_mut()` iterator method. On `AppEvent::Resize` in `app.rs`, a new `handle_resize()` method iterates all form field editors and resets their viewports.

## Deviations from plan

None significant. Used `reset_editor_viewport()` as a method on `FieldState` rather than a public `textarea_from_text_pub` function, which is cleaner. The `editors_mut()` method returns an `impl Iterator<Item = &mut FieldState>` over a fixed-size array, as planned.

## Verification

`just verify` passes: fmt, check, clippy, and all 66 tests + 4 integration tests.

## Important files changed

- `src/ui.rs` - `revset_display_label()`, `is_jj_default_description()`, `render_generate_fields(area)`, updated `render_menu` (title and label logic), removed "focused field" line from `render_generate_preview`
- `src/generate.rs` - `FieldState::reset_editor_viewport()`, `PrForm::editors_mut()`
- `src/app.rs` - `AppEvent::Resize` now calls `self.handle_resize()`, new `handle_resize()` method

## Residual risks

- The TextArea viewport reset recreates the editor, which moves cursor to end. If the user was in the middle of editing and resizes, cursor position is lost (acceptable per ticket).
- The primary identifier truncation uses byte-based indexing (`&primary[..primary_max]`). If the description contains multi-byte UTF-8 characters and `primary_max` falls in the middle of one, this could panic. A future improvement would use `char`-based truncation.
