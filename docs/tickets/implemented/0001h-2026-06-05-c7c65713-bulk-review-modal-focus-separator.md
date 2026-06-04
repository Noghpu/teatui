---
id: 0001h-2026-06-05-c7c65713-bulk-review-modal-focus-separator
created_at: 2026-06-05T12:32:32+02:00
created_by_model: gpt-5/medium
state: implemented
state_updated_at: 2026-06-05T15:46:59+02:00
---
# Bulk review modal: separator and two-step preview focus

## Goal
Fix the stacked PR bulk review modal so it reads as a clear master-detail layout and so selecting a PR row does not immediately enter text editing. The left PR list must show each PR title by wrapping it naturally instead of truncating it.

The modal must show a vertical separator between the left PR list and the right PR preview/form. Keyboard flow must become two-step: when the user chooses a PR from the left list with `Enter`, focus moves to the right preview/form for that same PR; a second `Enter` on a focused editable field starts editing that field.

## Context
The active rewrite is Linux-only and uses the Generate screen as the host for stacked PR generation. Read `docs/rewrite-plan.md` and `docs/stacked-pr-plan.md` before changing behavior.

The stacked PR review modal is rendered from `src/screens/generate.rs`:

- `render_bulk_review` currently splits the modal body directly into `list_area` and `form_area`, then calls `render_bulk_pr_list` and `render_bulk_pr_form`.
- `render_bulk_pr_list` handles natural scrolling through `GenerateState::bulk_list_scroll`; preserve the existing scroll behavior.
- `render_bulk_pr_list` currently truncates the title row with `truncate_ellipsis`. That makes long generated titles hard to review in the list; titles should wrap within the list pane instead.
- `render_bulk_pr_form` renders read-only head/base, status, and editable title/branch/description fields using `GenerateState::bulk_editor`.

The modal input path is in `src/screens/generate/input.rs`:

- `on_key` routes all keys to `on_bulk_modal_key` whenever `state.bulk != BulkPhase::Idle`.
- `on_bulk_review_key` currently uses `Up/Down/j/k` for row navigation and treats `Enter` the same as `i`, immediately setting `state.bulk_editor.editing = true` on the current `bulk_editor.field_focus`.
- `BulkItemEditor::default()` and `BulkItemEditor::from_plan_item` focus `BulkItemField::Title`, so pressing `Enter` after choosing a PR row jumps straight into title editing.

This is wrong for the desired UX: selecting a PR row should land the user in the right preview/form in field-selection mode, not active textbox-editing mode.

## Non-Goals
Do not change stacked PR generation, context collection, LLM parsing, push/precheck behavior, blocker detection, or the single-PR Generate flow.

Do not add a new screen. The bulk review remains a modal overlay inside the Generate screen.

Do not make the modal persist state to disk or change `StackPlan` domain shape unless required for UI focus state.

## Design Decisions
Add explicit focus state for the bulk review modal instead of overloading `BulkItemEditor::editing` and `field_focus`.

Use a small enum such as:

```rust
pub enum BulkReviewFocus {
    List,
    Preview,
}
```

Store it on `GenerateState` if that is the least invasive shape, or on a dedicated bulk-review UI state if the surrounding code already has a better home. Default to `List` when entering `BulkPhase::Review` and when seeding a new review plan.

Keyboard behavior in `BulkPhase::Review` with no push in flight:

- `Up/Down/j/k` while focus is `List`: move the PR-list cursor, flush any pending editor values first, seed the editor from the newly selected row, keep focus on `List`.
- `Enter` while focus is `List`: keep the current cursor, seed/sync the editor for that item, switch focus to `Preview`, do not start editing, and do not mutate the textbox buffer beyond normal seeding.
- `Tab` / `BackTab` while focus is `Preview`: move among title/branch/description field focus exactly as the current code does.
- `Up/Down/j/k` while focus is `Preview`: move among title/branch/description fields if this matches existing modal ergonomics better than Tab-only; otherwise leave field movement on Tab/BackTab. Whichever choice is made, the ticket must preserve an obvious way to select any preview field before editing.
- `Enter` or `i` while focus is `Preview`: begin editing the currently focused per-PR field.
- `Esc` while focus is `Preview` and not editing: move focus back to `List` rather than closing the modal. `Esc` while focus is `List` closes the modal after flushing edits, matching existing close behavior.
- `Esc` while actively editing: commit/cancel according to the existing bulk editor text semantics, then return to `Preview` field-selection mode, not to `List` and not out of the modal.
- `p` / `P` can still push from either focus when not editing, but they must flush any editor values before starting as today.

While `pushing: Some(_)`, preserve the current rule: navigation remains live but mutating and modal-closing actions are disabled. If focus is `Preview` during a push, either keep field-selection navigation read-only or route list navigation consistently; do not allow `Enter`, `i`, `p`, `P`, or `Esc` to mutate/close while a push is running.

Rendering behavior:

- Add a vertical separator column between the list and preview/form in `render_bulk_review`. Allocate layout as list, one-column separator, form. Use the shared theme color helpers; an ASCII `|` separator is acceptable if it matches existing ratatui styling.
- In the left PR list, do not truncate PR titles. Wrap titles across as many rows as needed within the row group, with the bookmark/status sub-row still associated with the same PR. Keep the cursor-visible natural scroll behavior correct when wrapped titles make rows variable-height.
- Visually distinguish `List` focus from `Preview` focus. The active list row should remain marked as selected in both cases, but the preview field marker/style should only look active when focus is `Preview` or editing. Avoid making both panes look equally active.
- Update footer/help text for the review modal if there is modal-specific hint text nearby. It should communicate list selection vs preview field editing without adding verbose in-app instructions.

## Implementation Plan
1. Inspect `BulkPhase::Review` construction sites in `src/app.rs`, `src/screens/generate.rs`, and tests. Add/initialize the new review focus state so opening a freshly generated plan starts with focus on the PR list.
2. Update `src/screens/generate/input.rs`:
   - Split `Enter` handling in review mode by list vs preview focus.
   - Keep `i` as edit activation only when preview/form focus is active.
   - Make non-editing `Esc` from preview focus return to list focus; keep list-focus `Esc` closing the modal.
   - Preserve push gating and list navigation during `pushing: Some(_)`.
   - Add focused unit tests that reproduce the bug: `Enter` on list focus must not set `bulk_editor.editing`; a second `Enter` after preview focus must set it.
3. Update `src/screens/generate.rs` rendering:
   - Insert the separator column in `render_bulk_review` without breaking small-terminal fallback.
   - Replace title truncation in `render_bulk_pr_list` with width-aware wrapping. Wrapped title lines must count toward that row's scroll span so the highlighted row stays visible.
   - Style list and preview focus distinctly.
   - Keep natural scrolling in `render_bulk_pr_list`; do not replace it with direct highlighted-index scrolling.
4. Update `tests/render_smoke.rs` for both focus states in the bulk review modal, including at least one multi-item plan and one long PR title that wraps in the list. Existing bulk review smoke tests should still pass.
5. Run `just verify`, then `just snapshots`; inspect the generated bulk review snapshot text/SVG or `target/ui-snapshots/index.html` to confirm the separator and focus styling are visible and text does not overlap.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "docs/stacked-pr-plan.md"
  ],
  "likely_files": [
    "src/screens/generate.rs",
    "src/screens/generate/input.rs",
    "src/app.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "Enter from the PR list moves focus to the right preview/form and never starts title editing on the same keypress.",
    "A second Enter or i from preview/form focus starts editing the focused field.",
    "The review modal has a visible vertical separator and no text overlap in snapshots, including small terminal coverage if affected.",
    "Long PR titles in the left list wrap instead of truncating, and the highlighted row remains visible with variable-height rows.",
    "Existing push-in-flight behavior remains non-mutating except allowed navigation."
  ],
  "jj_description_prefix": "fix"
}
```

## Acceptance Criteria
- The stacked PR review modal renders a visible vertical separator between the left PR list and the right PR preview/form.
- The left PR list wraps long PR titles instead of truncating them.
- Opening a review plan starts with the PR list focused.
- Pressing `Enter` while the PR list is focused moves focus to the right preview/form for the currently highlighted PR and does not set `bulk_editor.editing = true`.
- Once the preview/form is focused, the user can move/select among title, branch, and description fields, and `Enter` or `i` starts editing only the focused field.
- Non-editing `Esc` from preview/form focus returns to list focus; non-editing `Esc` from list focus closes the modal as before.
- `p` and `P` still push current/all from the review modal when allowed, flushing edited values first; they remain blocked during an in-flight push.
- The render smoke suite covers both list-focused and preview-focused review modal states.
- `just verify` and `just snapshots` pass.

## Verification Plan
Run `just verify` for formatting, compile, clippy, and tests.

Run `just snapshots` after the UI change. Inspect the bulk review modal artifacts under `target/ui-snapshots/` to confirm the separator appears between panes, long list titles wrap instead of truncating, focus styling is unambiguous, and the modal remains legible.

Add or update unit tests in `src/screens/generate/input.rs` for the reported Enter-navigation bug and for the second-Enter edit activation path.

## Files Likely Touched
- `src/screens/generate.rs`
- `src/screens/generate/input.rs`
- `src/app.rs`
- `tests/render_smoke.rs`

## Risks
The modal already uses `BulkItemEditor::field_focus` and `editing`; adding list-vs-preview focus can accidentally make both panes appear focused or can break existing Tab/BackTab field navigation. Keep the new state small and make the input tests describe the intended transitions.

The review modal supports push-in-flight navigation while blocking mutation. Preserve this behavior explicitly; do not let the new focus state re-enable editing, closing, or pushing during `pushing: Some(_)`.

The separator consumes one column and wrapped titles make PR list rows variable-height. Re-check small terminal rendering and cursor-visible scrolling so the list and preview do not collapse, overlap, or scroll the selected row out of view.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-06-05T15:46:59+02:00
- state: implemented

Completed:
- Added explicit BulkReviewFocus state for the stacked PR bulk review modal and initialized fresh review plans to list focus.
- Changed review input so Enter from the PR list moves to preview/form focus without starting text editing; Enter or i from preview starts editing the focused field; Esc from preview returns to list.
- Preserved push gating while keeping navigation live, with read-only field navigation from preview focus during push.
- Added the vertical list/preview separator, focus-aware styling, focus-aware footer hints, and natural wrapped PR titles in the left list while preserving row-span-aware scrolling.
- Added unit tests for the two-step Enter flow and Esc preview-to-list behavior.
- Added render smoke coverage for preview focus with a wrapped long PR title and updated snapshots to exercise the separator/wrapping.

Deviations:
- Kept Up/Down/j/k as field navigation while preview focus is active, matching the ticket's allowed ergonomics and preserving a direct way to choose any preview field.
- Kept BulkReviewFocus on GenerateState rather than changing domain StackPlan shape.

Verification:
- Ran cargo fmt.
- Ran just verify: passed, including 168 unit tests and 50 render smoke tests.
- Ran just snapshots: passed, wrote 20 snapshots to target/ui-snapshots.
- Inspected target/ui-snapshots/generate-bulk-review.txt and confirmed the separator, list-focused footer, and wrapped title render without overlap.

Important files changed:
- src/screens/generate.rs
- src/screens/generate/input.rs
- src/app.rs
- src/bin/ui-snapshots.rs
- tests/render_smoke.rs

Residual risks or follow-up:
- Visual inspection was from the deterministic text snapshot on the Windows dev host; interactive terminal feel still needs Linux runtime validation for the broader app constraint.
