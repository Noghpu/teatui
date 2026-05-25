---
id: 00006-2026-05-25-polished-pr-gen-context-collection
created_at: 2026-05-25T21:55:33+02:00
created_by_model: migration-placeholder
state: reviewed
state_updated_at: 2026-05-25T21:55:34+02:00
---
# Context Collection

## Goal
Collect all read-only repository context needed to build a PR generation prompt for the selected revset and current form values.

## Context
This ticket was migrated from `docs/tasks/reviewed/0006-2026-05-25-polished-pr-gen-context-collection.md`. The design source of truth is `docs/design.md`, especially Generate PR, jj Context Commands, Prompt Strategy, and Logs.

## Non-Goals
- Do not build the final prompt in command wrapper modules.
- Do not silently overwrite user-entered form values with inferred defaults.
- Do not add mutating repository commands.

## Design Decisions
- Add `ContextBundle` with raw and parsed data needed by prompt assembly.
- Collect context using sequential commands first unless parallelism is clearly valuable and does not complicate errors.
- Keep context gathering read-only.
- Preserve raw command output in logs.

## Implementation Plan
- Capture selected revset, base branch, `jj status`, selected revset log, selected descriptions, diff stats, selected diff, remote metadata, and form values.
- Capture collection start time and repo identity for later stale-context checks.
- Surface collection progress in the status bar and preview pane.
- Show recoverable failure state with retained form values.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/generate.rs", "src/jj.rs", "src/repo.rs", "src/command.rs", "src/app.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["context gathering is read-only", "raw command output is logged", "failure preserves selected revset and form edits"]
}
```

## Acceptance Criteria
- `g` starts context collection only when required inputs are valid enough.
- UI remains responsive during collection.
- Success stores a complete `ContextBundle`.
- Failure keeps the selected revset and form edits intact.
- Logs show every external command that ran.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; add focused tests for context bundle assembly or failure mapping if parsing logic is introduced.

## Files Likely Touched
- `src/generate.rs`
- `src/jj.rs`
- `src/repo.rs`
- `src/command.rs`
- `src/app.rs`

## Risks
- Long diffs can make logs or future prompts too large.
- Failure mapping can hide useful command stderr if it is over-summarized.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: legacy-migration
- completed_at: 2026-05-25T21:55:34+02:00
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
- reviewed_at: 2026-05-25T21:55:34+02:00
- state: reviewed

Findings:
- Legacy reviewed task migrated into the new ticket lifecycle.
- Original review postmortem was not present in the old note, so this placeholder records the migration only.

Verification:
- Historical review verification was not available in the source note.

Residual risks:
- Placeholder metadata may not reflect the original reviewer run.
