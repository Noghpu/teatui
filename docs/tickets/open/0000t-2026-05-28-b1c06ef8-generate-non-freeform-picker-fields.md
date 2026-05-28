---
id: 0000t-2026-05-28-b1c06ef8-generate-non-freeform-picker-fields
created_at: 2026-05-28T11:41:31+02:00
created_by_model: gpt-5
state: open
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
