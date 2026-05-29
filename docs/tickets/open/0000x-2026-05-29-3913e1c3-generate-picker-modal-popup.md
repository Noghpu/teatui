---
id: 0000x-2026-05-29-3913e1c3-generate-picker-modal-popup
created_at: 2026-05-29T12:42:00+02:00
created_by_model: claude-opus-4-7/high
state: open
---
# Move Picker Editing into a Centered Modal Popup

## Goal
Replace the inline picker UI in the Generate PR form with a centered modal popup that overlays the rest of the screen while a picker field is being edited. The popup shows the picker filter, options, selection state, and a compact key-hint footer. All existing picker behavior (single/multi-select, filter, highlight, commit/cancel) stays identical â€” only the presentation changes.

## Context
Pickers were added in ticket `0000t`. The current implementation lives in `src/generate.rs` (`PickerFieldState`) and renders inline inside `src/ui.rs::render_generate_field` (~lines 1320â€“1364): when `field.picker_is_editing()` is true, the field expands with a `selected: â€¦` line, a `filter: â€¦` line, and up to 5 option rows. This works but mixes picker UX into the scrollable form column, which makes the picker feel cramped and hard to scan when the form is also scrolled.

The user has confirmed the picker functionality is fine; only the surface should change. A centered modal makes editing pickers visually distinct from text editing and reuses the existing `PickerFieldState` machinery (`begin_edit`, `commit`, `cancel`, `input`, `visible_options`, `picker_filter`, etc.). No state-shape changes are needed.

`rat-dialog` was evaluated and rejected for this ticket: it is not a standalone widget. It depends on the entire `rat-salsa` runtime (`rat-salsa`, `rat-widget`, `rat-theme2`) and would require rewriting the app around `AppWidget`/`AppState` traits â€” explicitly out of scope per ticket `0000e`'s non-goals. `docs/design.md` already notes `rat-dialog` as a deferred candidate for *stacked* modals; with only one modal needed here it remains deferred. The plain-ratatui path (`Clear` + a centered `Rect`) is enough.

## Non-Goals
- Do not add `rat-dialog`, `rat-salsa`, `rat-widget`, `tui-popup`, or any other new dependency. Use ratatui's built-in `Clear` widget + `Rect` math.
- Do not change `PickerFieldState` data shape, input handling, filter logic, highlight logic, single/multi-select semantics, or validation.
- Do not change cursor rendering for text fields (separate ticket).
- Do not introduce a generic modal-stack abstraction. One modal at a time is sufficient. If a future feature needs stacked modals, revisit `rat-dialog` then.
- Do not move help/status into the modal. The global help bar stays as-is.

## Design Decisions
- The modal renders only when `app.screen() == Screen::Generate`, `Focus::Form` is active, the selected field is a picker, and `field.picker_is_editing()` is true. Otherwise the form pane renders unchanged.
- The modal is centered over the *entire frame area*, not just the form pane. Width: `min(60, frame.width.saturating_sub(8))`. Height: enough for header + filter row + visible options (capped to e.g. 10 rows) + footer, clamped to `frame.height.saturating_sub(4)`. Provide a small `centered_rect(width, height, area)` helper in `src/ui.rs`.
- Render `Clear` over the modal Rect first, then a themed `Block` (border + title matching the field label), then the inner layout:
  - Row 1: filter line â€” `Filter: <text>` (placeholder `(none)` when empty), styled muted.
  - Rows 2..N-1: scrollable options, each as `â–¶ [x] label` (single-select uses `[â€¢]` / blank, multi-select uses `[x]` / `[ ]`). Highlighted row gets `colors::ACCENT`; disabled rows muted with `(disabled)` suffix. Show up to `popup_visible_options` rows; if more exist, append `(â€¦ N more)` muted line.
  - Bottom row: compact footer key hint, single line, muted.
- Footer text, kept tight to fit narrow widths:
  - Single-select: `â†‘â†“ move Â· type filter Â· Enter ok Â· Esc cancel`
  - Multi-select: `â†‘â†“ move Â· Space toggle Â· Enter ok Â· Esc cancel`
- The inline picker rendering block in `render_generate_field` is replaced with a *summary-only* representation when editing: the field row shows `â–¶ Label: <selected values or (none)>  (editingâ€¦)` so the form pane indicates which field is open without expanding. Non-editing pickers render as today.
- The modal does not need its own scroll state in this ticket: highlight movement already keeps the visible window centered around `highlighted` via `visible_options()`. If `visible_options()` returns more rows than the popup can show, slice locally around `highlighted`. Do not add new state to `PickerFieldState`.
- Theming: reuse `colors::ACCENT`, `colors::MUTED`, `colors::GOOD`, `colors::BAD`, `colors::WARN`, `themed_block`, `focused_title` helpers already in `src/ui.rs`. The modal block is always "focused" (highlighted border) because it owns input.
- Render order in `render`: status / menu / work / preview / help as today, then the picker modal last, so it overlays everything.

## Implementation Plan
1. Add `centered_rect(width: u16, height: u16, area: Rect) -> Rect` helper in `src/ui.rs`.
2. Add `render_picker_modal(frame: &mut Frame, app: &App)` in `src/ui.rs`. It is a no-op unless the gating conditions hold.
3. Invoke `render_picker_modal` from `pub fn render` (`src/ui.rs`) as the last call so it overlays the rest. Use `Clear` over the modal Rect before drawing the block.
4. In `render_generate_field`, replace the inline picker editing block (~lines 1331â€“1364) with a single-line `editingâ€¦` summary while preserving the non-editing layout (selected: â€¦, options-loaded warnings, error lines).
5. Add a small private helper to compute the visible slice of `picker_visible_options()` around `highlighted` given a max-row budget. Keep it pure and unit-testable.
6. Update the Generate screen help text to mention modal keys when a picker is open (e.g. footer-only is fine if the help bar would be redundant).
7. Add focused unit tests for: the option-slice helper (window stays around highlighted; respects bounds), and that `centered_rect` produces a Rect inside the input area (math sanity).
8. Run `just verify`.
9. Manual TUI probe on Windows: open each picker (Head, Base, Labels, Assignees, Milestone); verify the modal appears centered, filter typing narrows options, highlight moves, Space toggles in multi-select, Enter commits, Esc cancels, focus returns to the form afterward.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/design.md",
    "docs/tickets/reviewed/0000t-2026-05-28-b1c06ef8-generate-non-freeform-picker-fields.md",
    "docs/tickets/reviewed/0000e-2026-05-26-69baa5fc-ratatui-textarea-editing.md"
  ],
  "likely_files": [
    "src/ui.rs",
    "src/generate.rs"
  ],
  "verification_commands": [
    "just verify"
  ],
  "review_focus": [
    "No new dependencies are added; only ratatui built-ins (Clear, Rect, Block) are used.",
    "PickerFieldState shape, behavior, validation, and key handling are unchanged.",
    "The modal is gated strictly on Screen::Generate + Focus::Form + selected picker + picker_is_editing.",
    "Inline picker rendering is reduced to a one-line summary while editing so the form pane does not double-render the option list.",
    "Render order ensures the modal overlays status, menu, work, preview, and help panes.",
    "Footer text fits common terminal widths and lists keys for the relevant single/multi-select mode."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- Activating edit on any picker field (Head, Base, Labels, Assignees, Milestone) opens a centered modal popup over the entire frame.
- The modal shows: a title with the field label, a filter row, visible option rows with highlight and selection markers, and a single-line key-hint footer.
- Filter typing, highlight movement, Space toggle (multi-select), Enter commit, and Esc cancel behave identically to the pre-ticket inline picker.
- While the modal is open, the underlying form pane shows the active picker row as `Label: <values>  (editingâ€¦)` without inline option expansion.
- Non-editing pickers render exactly as before (selected summary + warnings/errors).
- No new dependencies are added to `Cargo.toml` or `Cargo.lock` beyond what ratatui ships.
- `just verify` passes.

## Verification Plan
- Unit test the visible-slice helper and `centered_rect` math.
- `just verify` for fmt, check, clippy, tests.
- Manual TUI probe: open each picker, exercise single- and multi-select flows, confirm Esc returns to the form with no committed changes and Enter commits as before.

## Files Likely Touched
- `src/ui.rs`
- `src/generate.rs` (only if a tiny accessor is needed; avoid behavior changes)

## Risks
- Render order: if the modal is drawn before other panes it will be overdrawn. Mitigation: draw last in `pub fn render`.
- Clipping: very narrow or short terminals may shrink the modal below useful size. Mitigation: clamp height/width to sensible minimums and gracefully truncate options/footer.
- Footer length: at minimum widths the footer text may not fit. Mitigation: shorter key hints (`Space tog Â· Ent ok Â· Esc x`) under a width threshold, or truncate with `â€¦`.
- Double-render: if the inline picker block is not replaced by a one-line summary, options will appear both inline and in the modal. Mitigation: explicitly collapse the inline expansion while editing.
- Theme contrast: modal border must be visibly distinct from the underlying panes. Reuse `themed_block` with the focused style to keep the Catppuccin palette consistent.
