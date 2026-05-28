---
id: 0000r-2026-05-28-4758b9ae-stabilize-generate-form-edit-layout
created_at: 2026-05-28T11:40:33+02:00
created_by_model: gpt-5
state: reviewed
state_updated_at: 2026-05-28T11:55:59+02:00
---
# Stabilize Generate PR Form Editing Layout

## Goal
Fix the center pane layout problems when entering Generate PR form edit mode: separators must remain visible, the form must not collapse, and single-line text fields must stay visually single-line with the label and input on the same row.

## Context
`docs/design.md` defines the Generate PR center pane as a compact PR form with field navigation and separate edit behavior for text inputs, text areas, and pickers. The current implementation stores every form field in `FieldState` backed by `ratatui_textarea::TextArea`, and `src/ui.rs::render_generate_editor` swaps the normal form paragraph for a custom vertical layout while editing. That edit layout drops the normal separators and renders the selected single-line editor below a separate header row, which produces the reported `head` on one line and input on the next line.

The first pass should fix rendering for the current text-backed fields without implementing pickers yet. The later picker ticket will change the field model for non-freeform fields.

## Non-Goals
- Do not implement picker behavior in this ticket.
- Do not fetch labels, assignees, or milestones from Gitea.
- Do not redesign the whole Generate PR screen or landing page.
- Do not add broad snapshot or architecture tests.

## Design Decisions
- Only `branch name`, `title`, and `description` are true text editors long term. For this ticket, preserve current text-backed behavior for all fields, but render non-description fields as single-line editors while they are still text-backed.
- The selected field must use the same surrounding row/separator structure in normal mode and edit mode so entering edit mode does not remove separators or change the overall form shape.
- Single-line fields render as `label: <editor>` on one visual row. `Enter` commits them and must not insert a newline.
- `description` remains a bounded multiline editor with a label row and a fixed or clamped editor area inside the center pane.
- Rendering must never allocate field/editor areas outside the center pane inner block. If the pane is too short, content is clipped or scrolled by the later overflow ticket; it must not overlap the status/help bars.

## Implementation Plan
1. In `src/generate.rs`, add a small field-kind helper for the current form fields, at minimum distinguishing single-line text fields from the multiline description. Keep it compatible with the later picker ticket.
2. Refactor `src/ui.rs` form rendering so normal mode and edit mode share one field-row model, including separators between fields.
3. Update `render_generate_editor` so non-description fields render the selected editor inline after the label instead of using a separate `header_area` plus `editor_area` stack.
4. Keep the description editor bounded to a predictable multiline height, and clamp that height when the available center pane is smaller.
5. Preserve validation error rendering under the owning field without consuming unrelated rows.
6. Add focused unit tests only for risky state/rendering helpers, such as field-kind classification and single-line commit/newline behavior if existing coverage does not already cover it.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/ui.rs", "src/generate.rs", "src/app.rs"],
  "likely_files": ["src/ui.rs", "src/generate.rs", "src/app.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["Center pane edit mode keeps separators and stable row structure", "Single-line fields render and commit as one-line inputs", "Description remains bounded and does not overlap adjacent panes or bars"],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- Entering edit mode for any Generate PR center-pane field does not make separators disappear.
- Entering edit mode does not collapse the center pane or move content outside its block.
- Single-line fields display label and input on the same row while editing.
- `Enter` on single-line fields commits without inserting a newline.
- `Enter` in `description` still inserts a newline, and `Ctrl-S` still commits it.
- Existing PR generation form values, validation messages, and prompt/execution behavior continue to work.

## Verification Plan
- Run `just verify`.
- Manually run the TUI, open Generate PR, enter edit mode for `head`, `branch name`, `title`, and `description`, and resize the terminal to confirm the center pane remains coherent.

## Files Likely Touched
- `src/ui.rs`
- `src/generate.rs`
- `src/app.rs`

## Risks
- `ratatui_textarea::TextArea` may not fit naturally into an inline single-row layout; if so, isolate the workaround so the later picker/text-input split can remove it cleanly.
- Small terminal heights can still require scrolling; avoid solving all scrolling here beyond clamping and non-overlap, because pane overflow is covered by the next ticket.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5.4-mini
- completed_at: 2026-05-28T11:53:22+02:00
- state: implemented

Completed:
- Added `FieldKind` and `FieldId::kind()` so the form can distinguish single-line fields from the multiline description explicitly.
- Removed the separate Generate edit-mode stack and render the form through one row/separator path in both normal and edit modes, which keeps separators visible and the layout stable.
- Switched Enter/Ctrl-S handling to the field-kind helper.

Deviation:
- None beyond the narrow renderer refactor needed to keep edit mode in the same form shape.

Verification:
- `just verify`
- Passed formatting, check, clippy, unit tests, and the integration test suite.

Files changed:
- `src/generate.rs`
- `src/ui.rs`
- `src/app.rs`

Residual risk:
- Very small terminal heights can still clip content; the next overflow/scrolling slice is still the right place to solve that.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-05-28T11:55:59+02:00
- state: reviewed

Facts:
- Reviewed ticket 0000r-2026-05-28-4758b9ae-stabilize-generate-form-edit-layout against docs/design.md, src/generate.rs, src/app.rs, and src/ui.rs.
- The implementation adds FieldKind, uses it for Enter/Ctrl-S edit behavior, removes the separate Generate edit-mode renderer, and keeps separators in the shared field list.
- Added a review fix in src/ui.rs to bound description field display to six lines with overflow indication, plus focused helper tests.
- Ran `just verify`; formatting, check, clippy, unit tests, and Windows integration tests passed.

Inferences:
- The shared row path now satisfies the ticket's stable separator/row-shape requirements for current text-backed fields.
- Bounded description rendering reduces small-pane layout risk while leaving full scrolling/overflow behavior for the later planned ticket.

Residual risk:
- Manual TUI resize checks were not run in an interactive terminal during this review.
