---
id: 00004-2026-05-25-polished-pr-gen-revset-selector
created_at: 2026-05-25T21:55:32+02:00
created_by_model: migration-placeholder
state: reviewed
state_updated_at: 2026-05-25T21:55:32+02:00
---
# Revset Selector

## Goal
Populate the Generate PR left pane with real candidate jj revsets and useful preview details.

## Context
This ticket was migrated from `docs/tasks/reviewed/0004-2026-05-25-polished-pr-gen-revset-selector.md`. The design source of truth is `docs/design.md`, especially Generate PR and jj Context Commands.

## Non-Goals
- Do not solve every revset default question.
- Do not implement mutating jj operations.
- Do not parse jj output outside the `jj` module.

## Design Decisions
- Add read-only jj wrappers for candidate revset summaries.
- Query initial candidate revsets: `@`, `@-`, and `heads(trunk()..)`. 
- Prefer machine-readable jj output when practical.
- Preserve raw command output in logs when text parsing is required.

## Implementation Plan
- Add a `jj` module with read-only wrappers for candidate revset summaries.
- Gather short descriptions, bookmarks, change counts, commit/change IDs, diff stats, and warnings where feasible.
- Replace placeholder revsets in `App::new`.
- Render revset list rows compactly in the left pane.
- Render selected revset detail in the preview pane.
- Reject or warn on conflicted, ambiguous, empty, or multi-head states when unsupported.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/jj.rs", "src/repo.rs", "src/app.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["jj wrappers are read-only", "parsing stays inside jj module", "selected revset updates the form head default"]
}
```

## Acceptance Criteria
- Generate PR shows real revsets from the current jj workspace.
- Selecting a revset updates the form `head` default.
- Preview includes description, bookmarks, stats, recent log context, and warnings.
- Refresh reloads revset data.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; add focused parser and argv construction tests if parsing or wrappers are introduced.

## Files Likely Touched
- `src/jj.rs`
- `src/repo.rs`
- `src/app.rs`
- `src/ui.rs`

## Risks
- jj output shape can vary by installed version.
- Ambiguous revsets may need clearer warnings than the first implementation provides.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: legacy-migration
- completed_at: 2026-05-25T21:55:32+02:00
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
- reviewed_at: 2026-05-25T21:55:32+02:00
- state: reviewed

Findings:
- Legacy reviewed task migrated into the new ticket lifecycle.
- Original review postmortem was not present in the old note, so this placeholder records the migration only.

Verification:
- Historical review verification was not available in the source note.

Residual risks:
- Placeholder metadata may not reflect the original reviewer run.
