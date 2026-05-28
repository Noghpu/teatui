---
id: 0000t-2026-05-28-b1c06ef8-generate-non-freeform-picker-fields
created_at: 2026-05-28T11:41:31+02:00
created_by_model: gpt-5
state: reviewed
state_updated_at: 2026-05-28T16:00:12+02:00
---
# Replace Non-Freeform Generate Fields With Pickers

## Goal
Replace free-text editing for non-freeform Generate PR form fields with picker behavior so users can only select valid offered values. Only `branch name`, `title`, and `description` remain free text inputs.

## Context
`docs/design.md` already describes `head` and `base` as picker fields and `labels`, `assignees`, and `milestone` as optional picker fields. The current implementation represents every field as `FieldState` with a text buffer/editor, so users can type arbitrary values for `head`, `base`, `labels`, `assignees`, and `milestone`.

This ticket introduces the picker state and UI behavior using already-known in-memory options. A later ticket will fetch and cache repo metadata options for labels, assignees, and milestones.

## Non-Goals
- Do not fetch labels, assignees, or milestones from Gitea in this ticket.
- Do not add disk caching.
- Do not create a popup/dropdown system unless it is necessary for a clean terminal layout; an inline fuzzy/filterable picker is acceptable for this release.
- Do not change PR execution semantics beyond reading selected picker values from the form.

## Design Decisions
- Free-text fields are exactly `branch name`, `title`, and `description`.
- Picker fields are `head`, `base`, `labels`, `assignees`, and `milestone`.
- `head` is a single-select picker backed by the current `GenerateState.revsets`; selecting a left-pane revset updates the `head` picker default as it does today.
- `base` is a single-select picker backed by valid change choices from the current revset/change list. Keep the configured/default base branch visible only as a fallback option if existing execution paths require it, but the user must choose an offered value and cannot type arbitrary text.
- `labels` and `assignees` are multi-select pickers. `milestone` is a single-select optional picker. Until repo metadata loading lands, they may show empty/loading/disabled option states, but they must not accept arbitrary text values.
- Picker edit mode supports keyboard-only use: Up/Down moves the highlighted option, printable keys filter visible options, Space toggles multi-select values, Enter commits, and Esc cancels. For single-select pickers, Enter commits the highlighted option.
- Existing prompt and execution code can continue to consume string display values. Multi-select display values should remain comma-separated so `tea.rs` and `prompt.rs` keep working until typed values are introduced later.

## Implementation Plan
1. Refactor `src/generate.rs` form state so each `FieldId` has a field kind and can store either text state or picker state without forcing every field through `ratatui_textarea::TextArea`.
2. Add picker option types with display label, stable value, enabled/disabled status, and selected/highlighted state.
3. Populate `head` and `base` picker options from existing revset/change data in `GenerateState` and refresh them when revsets are replaced.
4. Update `begin_editing_selected_field`, `input_selected_field`, `commit_selected_field`, `cancel_selected_field`, and validation so picker fields follow picker semantics and cannot commit values outside their option sets.
5. Update `src/ui.rs` to render picker fields distinctly but compactly in the center pane, including selected values, empty/loading/disabled states, and the highlighted option while editing.
6. Ensure `sync_head_from_selected_revset` updates the head picker selection without overwriting dirty text fields such as branch name.
7. Add focused unit tests for picker commit/cancel, multi-select toggling, filter behavior, and invalid/unavailable option handling.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/generate.rs", "src/ui.rs", "src/app.rs", "src/prompt.rs", "src/tea.rs"],
  "likely_files": ["src/generate.rs", "src/ui.rs", "src/app.rs", "src/prompt.rs", "src/tea.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["Only branch name, title, and description accept arbitrary text", "Head/base picker options stay synchronized with revsets", "Picker display values still feed prompt and PR execution correctly"],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- `branch name`, `title`, and `description` remain editable text inputs with the behavior from the layout ticket.
- `head`, `base`, `labels`, `assignees`, and `milestone` no longer accept arbitrary committed text.
- `head` offers current change/revset choices and defaults to the selected left-pane revset.
- `base` offers valid change/base choices from current repo state and cannot be committed to an arbitrary typed value.
- `labels` and `assignees` support multi-select picker state even if repo options have not loaded yet.
- `milestone` supports optional single-select picker state even if repo options have not loaded yet.
- Current prompt manifest, prompt text, and execution command construction still include selected form values.

## Verification Plan
- Run `just verify`.
- Manually run the TUI and verify picker fields can be navigated, filtered, committed, and cancelled without accepting invalid text.
- Manually verify generating a prompt still includes selected picker values.

## Files Likely Touched
- `src/generate.rs`
- `src/ui.rs`
- `src/app.rs`
- `src/prompt.rs`
- `src/tea.rs`

## Risks
- This changes a central form data shape; keep the refactor direct and explicit rather than introducing a generic widget registry.
- Prompt/execution paths currently expect strings. Preserve display-value compatibility in this ticket to avoid dragging command construction into the picker refactor.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-28T16:00:12+02:00
- state: reviewed

# Review Postmortem â€” 0000t replace non-freeform generate fields with pickers

## Outcome
Accepted as implemented. No code changes required during review. `just verify`
passes (cargo fmt --check, cargo check, cargo clippy -D warnings, cargo test
--all-targets) on the implementation commit.

## What the implementation does
- Introduces `FieldKind { Text { multiline }, Picker { multi_select, optional } }`
  and reshapes `FieldState` into an enum of `Text(Box<TextFieldState>)` or
  `Picker(PickerFieldState)`. Free-text fields (branch name, title,
  description) keep `TextArea`-backed editing; head, base, labels, assignees,
  and milestone are pickers that store option lists, draft/committed value
  vectors, a filter string, and a highlighted index.
- `PickerFieldState` enforces option-bounded selection: `commit` calls
  `select_highlighted` for single-select pickers, `ensure_valid_selection`
  drops committed values that disappear from the option set, and
  `invalid_selection_error` surfaces "selection is unavailable"/"no available
  options" diagnostics for non-optional pickers.
- `GenerateState::refresh_picker_options` populates `head` from current
  revset labels and `base` from change_ids across all revsets plus a
  `main@origin` fallback. Labels/assignees/milestone get empty option vectors
  for now (deferred to repo-metadata loading ticket).
- `sync_head_from_selected_revset` continues to update head only when not
  dirty, and uses `FieldState::set_value` which re-baselines initial/
  committed/draft and clears the dirty flag â€” correct given head options are
  rebuilt right before the sync via `refresh_picker_options`.
- `ExecutionPlan::from_draft`, `prompt`, and the integration test consume
  display values as before; multi-select display joins selected values with
  ", " so `tea pr create --labels`/`--assignees` arguments stay shaped as
  expected.
- Tests added for single-select commit, filtering, multi-select toggle +
  cancel, and dropping unavailable committed values.

## Acceptance criteria check
- Branch name, title, description remain `FieldState::Text` (verified in
  `FieldId::kind`). Other five fields are `FieldKind::Picker`.
- Head defaults to selected revset label and offers all revset labels.
- Base offers change_ids from current revsets plus `main@origin` fallback;
  arbitrary typed text is impossible because the input path only filters or
  toggles options.
- Labels/assignees are multi-select optional pickers; milestone is
  single-select optional. With no loaded options they render `(no options
  loaded)` and commit to an empty value, which downstream `tea` flags treat
  as omitted.
- Prompt manifest and execution still read `display_value()`; integration
  test `fake_happy_path_captures_prompt_and_pr_url` continues to assert PR
  creation with the picker-derived values.

## Notes and minor observations (not fixed)
- The filter input excludes the space character unconditionally
  (`ch != ' '` guard). This is the intended trade-off because Space toggles
  multi-select values; for single-select pickers it means labels containing
  spaces cannot be filtered by a phrase. Acceptable for now â€” head/base
  option labels are revset/change_id strings without spaces, and labels/
  assignees/milestone options aren't populated yet.
- `PickerFieldState::ensure_valid_selection` resets `value`, `buffer`,
  `committed`, and `draft` when `options` is empty but does not update the
  `dirty` flag. In practice the only caller is `set_options` from
  `refresh_picker_options`, which runs at construction or when revsets
  replace â€” at that moment the picker is not actively edited, so the stale
  `dirty` flag isn't observable. Worth keeping in mind if option loading
  becomes more dynamic.
- `commit` for a picker that was never edited still runs `select_highlighted`
  when `editing` is false? No â€” guarded by `if self.editing && !self.multi_select`,
  so out-of-band `commit` calls are safe.
- For pickers, `FieldState::set_value` bypasses option validation. This is
  fine for the head sync path (`refresh_picker_options` populates options
  before `set_value` is called with a label that is guaranteed to be in the
  option set), but a future caller that injects arbitrary defaults would
  re-introduce invalid selections. The validation layer (`form_picker_errors`
  / `invalid_selection_error`) would still surface the error to the user.

## Verification
- `just verify` (fmt --check, check, clippy -D warnings, test --all-targets)
  passes on the implementation commit. 129 unit tests + 4 integration tests
  green.

## Files reviewed
- `src/generate.rs`
- `src/ui.rs`
- `src/app.rs`
- `tests/windows_pr_generation_integration.rs`
- `docs/design.md`, `docs/tickets/implemented/0000t-...md`
