---
id: 00005-2026-05-25-polished-pr-gen-form-editing
created_at: 2026-05-25T21:55:33+02:00
created_by_model: migration-placeholder
state: reviewed
state_updated_at: 2026-05-25T21:55:33+02:00
---
# Form Editing

## Goal
Implement the polished keyboard-first PR form interaction that the product depends on.

## Context
This ticket was migrated from `docs/tasks/reviewed/0005-2026-05-25-polished-pr-gen-form-editing.md`. The design source of truth is `docs/design.md`, especially UI Model, Generate PR, and Action Flow.

## Non-Goals
- Do not introduce a generic form engine.
- Do not implement a dropdown component unless the current layout requires it.
- Do not let input mode trigger global keybindings.

## Design Decisions
- Keep a single simple field-editing implementation shared by form and draft review fields.
- Use stable field enum ordering rather than stringly typed field selection.
- For pickers, use editable text input for the first version with optional suggestions.

## Implementation Plan
- Implement `Normal`, `Input`, and `Review` mode handling for Generate PR.
- Add fields for head, branch name, base, title, description, labels, assignees, milestone, and user instructions if not already represented.
- Support `j`/`k` or arrows for field navigation in normal mode.
- Support `Enter` or `i` to edit the focused field.
- Make printable keys update the focused field buffer in input mode.
- Support `Esc` to leave input mode without leaving Generate PR.
- Decide and implement commit/cancel semantics for text edits.
- Add validation display for required fields and branch-name shape.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/generate.rs", "src/app.rs", "src/event.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["text input mode does not trigger global keybinds", "dirty fields are tracked", "field ordering is typed and stable"]
}
```

## Acceptance Criteria
- Typing `g`, `q`, `j`, or `k` inside a text field inserts text.
- `Esc` in input mode returns to form navigation.
- `Esc` in Generate normal/review mode returns to Landing.
- Dirty fields are tracked.
- Validation errors are visible without blocking normal navigation.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; add unit tests for key-to-action behavior, field commit/cancel behavior, and branch-name validation.

## Files Likely Touched
- `src/generate.rs`
- `src/app.rs`
- `src/event.rs`
- `src/ui.rs`

## Risks
- Input mode can accidentally leak key events to global actions.
- Validation state can become too blocking for normal navigation.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: legacy-migration
- completed_at: 2026-05-25T21:55:33+02:00
- state: implemented

Completed:
- Legacy completed task migrated into the new ticket lifecycle.
- Original implementation details were not present in the old note, so this placeholder records the migration only.

Deviations:
- Placeholder lifecycle note used because historical implementation output was unavailable.

Verification:
- Historical verification was not available in the source note.

Files changed:
- Placeholder: see implementation revision history for actual historical files.

Residual risks:
- Placeholder metadata may not reflect the original implementer run.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: legacy-migration
- reviewed_at: 2026-05-25T21:55:33+02:00
- state: reviewed

Findings:
- Legacy reviewed task migrated into the new ticket lifecycle.
- Original review postmortem was not present in the old note, so this placeholder records the migration only.

Verification:
- Historical review verification was not available in the source note.

Residual risks:
- Placeholder metadata may not reflect the original reviewer run.
