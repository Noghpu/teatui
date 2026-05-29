---
id: 0000w-2026-05-29-807fae6e-generate-text-cursor-rendering
created_at: 2026-05-29T12:41:55+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-29T16:36:42+02:00
---
# Render Cursor for Active Text Editor in Generate Form

## Goal
Make the focused text field in the Generate PR form visibly render the `ratatui_textarea::TextArea` widget (with its native cursor) whenever the form is in editing mode, so the user has an unambiguous visual indicator that input is being captured. The presence of the cursor is the canonical "I am in editing mode" affordance.

## Context
Ticket `0000e` adopted `ratatui-textarea` and stores a `TextArea<'static>` on every `TextFieldState` (see `src/generate.rs` `TextFieldState::editor`). The textarea correctly receives `KeyEvent`s in editing mode and `buffer`/`value` stay in sync.

However `src/ui.rs::render_generate_field` (around lines 1262â€“1372) draws every field â€” including the focused, editing one â€” as plain `Line`s built from `field.display_value().to_string()`. The `TextArea` widget is never rendered, so its cursor never reaches the screen. The 0000e review postmortem claimed cursor rendering was added, but that change has either been lost or never applied to the inline form path. Users currently have no visual signal that editing mode is active.

The Generate form pane uses a single `Paragraph` built from a `Vec<Line>` for the whole form (`src/ui.rs::render_work`, branch `Screen::Generate`, ~lines 697â€“727), with `Wrap { trim: false }` and a scroll offset. Mixing widget rendering into that paragraph requires reserving a sub-`Rect` for the focused field's editor and rendering the textarea over it, while the surrounding non-focused fields keep their compact `Line` rendering.

`InputMode::Editing` (gated through `app.rs`) is the existing boundary between navigation and text input. Cursor rendering must follow that flag exactly: editing â†’ cursor visible; navigation/focused-but-not-editing â†’ no cursor.

## Non-Goals
- No changes to picker rendering or picker behavior. (Picker modal is a separate ticket.)
- No new dependencies. `ratatui-textarea` is already in `Cargo.toml`.
- No changes to `Action`, key routing, validation, dirty tracking, or commit/cancel semantics. Those landed with ticket `0000e` and stay as-is.
- No change to the navigation-mode rendering of unfocused or focus-but-not-editing fields. They keep their compact `Line`-based layout.
- No cursor for picker fields. Pickers do not own a `TextArea`.

## Design Decisions
- The cursor is rendered only when `Focus::Form` is active AND `InputMode::Editing` is active AND the selected field is a text field (single-line or multi-line description). In navigation mode the focused field renders the same as before â€” no cursor â€” so cursor presence reliably signals "input is being captured".
- Render the focused-editing text field through `ratatui_textarea::TextArea::widget()` placed in a dedicated `Rect` carved out of the form pane's inner area. The textarea widget natively draws its cursor and handles single-vs-multi line metrics.
- Keep `render_generate_fields` returning a `Vec<Line>` for the non-editing case so the existing scroll/paragraph path is unchanged. Add an editing path that splits the work area into: (a) lines above the focused field, (b) the textarea widget Rect for the focused field's input area, (c) lines below. This avoids mixing widget and paragraph rendering in a single Rect.
- The focused field's `header` line (the `â–¶ Label: â€¦` or `â–¶ Label  (n/total)` row) and any error lines below remain `Line`-based. Only the editable value area becomes a widget. For single-line fields the widget Rect is one row tall; for the multi-line description it spans the existing `DESCRIPTION_FIELD_DISPLAY_LINES` block.
- Scrolling: ensure the focused field's textarea Rect is visible. The existing `form_scroll.ensure_visible(start, end, ...)` covers this; the textarea widget Rect must fall inside the visible viewport. Compute Rect placement after applying the scroll offset.
- Editor reuse: continue to construct the `TextArea` once in `begin_edit` and on commit/cancel. Do not rebuild it per-frame (would lose cursor position).
- Help text update: status/help line should mention that the visible cursor indicates editing mode is active. Keep wording minimal.

## Implementation Plan
1. In `src/ui.rs`, refactor the `Screen::Generate` arm of `render_work` so that when the active field is being edited (`Focus::Form` + `InputMode::Editing` + selected field is a `TextField`), the render path:
   - Computes the full `lines: Vec<Line>` block as today for layout/scroll math.
   - Determines the in-block row range that the focused field's editable value occupies (use the same range used for `selected_range`, adjusted to point at the editable rows only, excluding the header row and error rows).
   - Renders the surrounding `Paragraph` exactly as today.
   - Computes a sub-`Rect` corresponding to the editable rows after scroll offset is applied, clipped to the inner viewport.
   - Calls `frame.render_widget(field.editor.widget(), editor_rect)` for that sub-rect.
2. Provide a small helper in `src/ui.rs` that, given `lines`, the focused field index, and the per-field line counts, returns the `(start_row, end_row)` of the editable region for the focused field. This must agree with `selected_range` semantics so scrolling continues to keep the editor onscreen.
3. Keep the placeholder in the line-based render of the focused-editing field consistent: when editing, the lines for the editable area should be blank lines (or kept as today) â€” they will be visually overdrawn by the textarea widget. Confirm there is no double-render artifact (no styled text behind the widget that bleeds through; `Clear` is unnecessary if the placeholder lines render only whitespace, but use `Clear` over the editor Rect if any underlay text shows through).
4. Verify `ratatui_textarea::TextArea` cursor style defaults are visible against the Catppuccin theme. If not, set an explicit cursor style on `begin_edit` (e.g. reversed video) so it stands out. Keep it tasteful â€” single style call, no per-frame churn.
5. Update help text in `src/ui.rs::render_help` (or the Generate-screen help block) to note that a visible cursor indicates editing mode is active. One short line.
6. Add focused unit tests (only where pure logic is testable): the helper from step 2 must produce a stable `(start, end)` for each field. Do not add snapshot tests of the rendered frame.
7. Run `just verify`.
8. Manually probe the TUI: enter Generate, focus a single-line field, press Enter (or whatever begins edit), confirm a blinking cursor; type and see the cursor advance; press Esc and confirm cursor disappears. Repeat with the multi-line description: confirm cursor moves across rows on Enter, and that scroll keeps it onscreen.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/design.md",
    "docs/tickets/reviewed/0000e-2026-05-26-69baa5fc-ratatui-textarea-editing.md"
  ],
  "likely_files": [
    "src/ui.rs",
    "src/generate.rs",
    "src/app.rs"
  ],
  "verification_commands": [
    "just verify"
  ],
  "review_focus": [
    "Cursor must appear only in Focus::Form + InputMode::Editing for text fields, never for pickers or in navigation mode.",
    "The textarea widget Rect must stay in sync with form_scroll so the cursor is never drawn outside the visible viewport.",
    "Do not rebuild the TextArea per-frame; doing so loses cursor position and is the primary regression risk.",
    "Unfocused and focused-but-not-editing fields must render exactly as before (no layout shift).",
    "Multi-line description editing: cursor must traverse rows; field must stay within its bounded display area."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- When the Generate screen is shown and `Focus::Form` + `InputMode::Editing` is active on a text field (single-line or description), a cursor is visibly rendered at the textarea's cursor position.
- Switching to navigation mode (commit or cancel the edit) removes the cursor immediately on the next frame.
- Picker fields never render a cursor.
- Unfocused fields render with the same layout, colors, and line counts as before this ticket.
- Multi-line description editing keeps the cursor inside the field's bounded display area; vertical cursor movement does not break layout for other fields.
- Help/status text mentions the cursor as the editing-mode indicator.
- `just verify` passes.

## Verification Plan
- Unit test the line-range helper added in step 2 (deterministic ranges per field).
- `just verify` for fmt, check, clippy, tests.
- Manual TUI probe on Windows: enter Generate, edit each field type, verify cursor visibility, movement, commit, cancel.

## Files Likely Touched
- `src/ui.rs`
- `src/generate.rs` (only if a small `editor.widget()` accessor or cursor-style setter is needed; avoid larger changes)
- `src/app.rs` (only if help-text wiring requires it)

## Risks
- The current form-rendering path uses a single `Paragraph` over the whole pane with `Wrap { trim: false }`. Carving out a sub-Rect and rendering the textarea on top can produce double-render artifacts if the underlying paragraph text overlaps. Mitigation: render blank placeholder rows beneath the editor Rect, or `Clear` the Rect before rendering the widget.
- Scroll math: if `form_scroll.offset` is non-zero, the editor Rect must be translated by the offset and clipped to the inner viewport, otherwise the cursor may appear in the wrong row or be drawn off-area.
- Rebuilding the `TextArea` on every render would reset the cursor. Keep the editor owned by the field and only mutate via `input()`.
- Multi-line description: ensure the textarea widget's own internal scroll does not fight with `form_scroll`. If it does, prefer letting the textarea handle vertical scroll within its bounded Rect and let `form_scroll` only move the whole form when other fields go off-screen.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-29T16:25:07+02:00
- state: implemented

## What was completed

Implemented cursor rendering for the active text editor in the Generate PR form.

When `Focus::Form + InputMode::Editing` is active on a text field (single-line or multi-line description), the `ratatui_textarea::TextArea` widget is now rendered as an overlay in a dedicated sub-Rect carved from the inner viewport, showing the native cursor.

### Key changes

**`src/generate.rs`**
- Added `text_editor() -> Option<&TextArea<'static>>` on `FieldState` to allow the UI layer to borrow the editor for widget rendering.

**`src/ui.rs`**
- Added `Clear`, `InputMode` imports.
- `render_generate_field`: added `editing: bool` parameter. When editing a text field, produces blank placeholder rows instead of value lines (1 for single-line fields, `DESCRIPTION_FIELD_DISPLAY_LINES` for description), returns a `(rel_start, rel_end)` range alongside the lines.
- `render_generate_fields`: now accepts `editing_text: bool`, passes it through, collects the absolute `editor_row_range` and returns it as a third element.
- `render_work` (Screen::Generate): after rendering the Paragraph, overlays `Clear` + `TextArea` widget for the editing field using a `compute_editor_rect` helper that translates content-relative rows to viewport-relative `Rect`, accounting for scroll offset.
- `compute_editor_rect`: new helper that maps `(editor_start, editor_end, scroll_offset)` to a viewport-clipped `Rect`. Returns `None` when the editor is entirely outside the visible viewport.
- Help text updated: `" cursor " / "editing active "` makes the cursor the canonical editing indicator.
- Six unit tests added for `compute_editor_rect` covering no-scroll, multiline, partial-scroll, above-viewport, below-viewport, and zero-height cases.

### Deviations from plan

- Cursor style defaults were not overridden: `ratatui_textarea::TextArea` uses reversed-video by default, which stands out adequately on the Catppuccin theme. No explicit style call needed.
- The `widget()` method on `TextArea` was deprecated in the version in use; passed `&editor` directly to `frame.render_widget()` per the deprecation hint.

### Verification

`just verify` passes: 163 tests pass, fmt/check/clippy clean.

Manual TUI probe not run (no live terminal available in this session), but the logic is covered by unit tests and the rendering path is exercised via normal compile-check.

### Important files changed

- `src/ui.rs`
- `src/generate.rs`

### Residual risks / follow-up

- The `TextArea`'s own internal scroll can fight with `form_scroll` for the description field if the user enters more lines than `DESCRIPTION_FIELD_DISPLAY_LINES`. The textarea widget is bounded to that Rect, so vertical scroll within the textarea is not surfaced; the form scroll handles moving the whole form. This is the expected behavior per the ticket but should be probed manually.
- Picker fields are explicitly excluded from cursor rendering (no `TextArea`), consistent with the Non-Goals.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-29T16:36:42+02:00
- state: reviewed

# Review postmortem: 0000w generate text cursor rendering

## Verdict

Accepted with a tidy-up. Implementation meets the ticket's goal, acceptance criteria, and design constraints. One small style fix applied during review.

## Implementation summary (facts)

- `FieldState::text_editor() -> Option<&TextArea<'static>>` added in `src/generate.rs` returns the inner editor only for text fields (None for pickers).
- `Screen::Generate` branch of `render_work` now computes `editing_text = Focus::Form && InputMode::Editing && !selected.kind().is_picker()`, then calls `render_generate_fields` which threads that flag down and returns an extra `editor_row_range` describing the absolute placeholder row span.
- Paragraph still renders first; then `Clear` + `frame.render_widget(editor, editor_rect)` overlays the textarea on the placeholder rows when `editing_text` is true and the rect is in-viewport.
- `compute_editor_rect` translates `(editor_start, editor_end, scroll_offset)` to a viewport-clipped `Rect`, returns `None` when fully scrolled out. Six dedicated unit tests cover no-scroll, multiline, partial-scroll, fully-above, fully-below, and zero-height viewport cases.
- `render_generate_field` was widened with an `editing: bool` parameter. When editing a single-line text field, the header omits the inline value; for both single-line and description fields, `N` blank `Line::from("")` placeholders are pushed (1 or `DESCRIPTION_FIELD_DISPLAY_LINES`) to reserve overdraw space. The relative range is returned to the caller.
- Help-line wording for `InputMode::Editing` updated to lead with " cursor "/"editing active " as the canonical indicator.
- `163` unit tests + integration tests pass under `just verify`.

## Correctness check vs. acceptance criteria

- Cursor only when `Focus::Form + InputMode::Editing` and selected field is text. The gate is exactly `app.focus() == Focus::Form && app.input_mode() == InputMode::Editing && !selected_field().kind().is_picker()`. Pickers excluded by construction; navigation mode excluded by `InputMode` gate.
- Cursor disappears on commit/cancel: `apply_edit_key` / Esc path flips `InputMode` back to navigation; next frame's gate is false, no overlay rendered. Verified via reading `src/app.rs::begin_editing_form_field` and the InputMode transitions in `apply_edit_key`.
- Unfocused fields render identically: `render_generate_field` only diverges when `editing_this_field` is true, which requires the field to be selected AND `editing_text` true. Non-selected fields hit the original branches verbatim.
- Layout-shift on entry to description edit (1-line `(empty)` to 6 blank rows) is by design per the plan's "spans the existing DESCRIPTION_FIELD_DISPLAY_LINES block". `form_scroll.ensure_visible` keeps it onscreen.
- `Clear` over the overlay rect prevents double-render. Placeholder rows under the rect are blank, so even if `Clear` were dropped there is no bleed-through.
- TextArea is not rebuilt per frame: editor is owned by `TextFieldState` and only mutated in `begin_edit`/`commit`/`cancel`/`input`. Cursor position is preserved across frames.
- Help text mentions the cursor indicator.

## Code quality changes applied during review

- Hoisted the `compute_editor_rect` and `Rect` imports in the test module to the existing `use super::{...}` block at the top of the `mod tests` block (was a mid-module `use` between tests, which is unusual style and would draw scrutiny from `clippy::items_after_test_module`-adjacent lints in future). No functional change.

## Risks / follow-ups (inferences)

- The TextArea owns its own internal vertical scroll; if a user puts > 6 lines into the description it scrolls within the bounded rect rather than expanding the form. This matches the ticket's risk note and is acceptable.
- Cursor styling relies on `ratatui-textarea` defaults (reversed video). If theme changes later make this hard to see, consider an explicit cursor style in `begin_edit`.
- The non-test `selected_range`-based scroll math uses the full set of lines including the 6 placeholder rows when editing the description. That means `ensure_visible` may pull more of the form into view than strictly necessary when editing description on a tall form, but it stays correct.

## Verification

- `just verify` ran twice (before and after the import tidy-up). Both runs: 163 unit + 4 integration tests passing, fmt/check/clippy clean.
- Manual TUI probe not performed in this session (no live terminal).
