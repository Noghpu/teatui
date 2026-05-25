---
id: 00001-2026-05-25-polished-pr-gen-app-loop-and-state
created_at: 2026-05-25T21:55:30+02:00
created_by_model: migration-placeholder
state: reviewed
state_updated_at: 2026-05-25T21:55:30+02:00
---
# App Loop And State

## Goal
Replace the demo-oriented app state with explicit state for the polished Generate PR workflow, while keeping the existing Elm-style update loop simple.

## Context
This ticket was migrated from `docs/tasks/reviewed/0001-2026-05-25-polished-pr-gen-app-loop-and-state.md`. The design source of truth is `docs/design.md`, especially UI Model, Architecture, Core State, Action Flow, and Implementation Order.

## Non-Goals
- Do not introduce async traits or dynamic dispatch in app state.
- Do not create a generic widget map or component framework.
- Do not implement external command execution beyond a typed future path.

## Design Decisions
- Preserve the existing `Action -> update -> render` shape.
- Keep `update` responsible for state transitions only.
- Keep rendering synchronous and side-effect free.
- Keep Generate PR state concrete in a dedicated module.

## Implementation Plan
- Introduce explicit `InputMode`, `Focus`, and `GeneratePhase` enums.
- Move Generate PR-specific state into a dedicated `generate` module.
- Add `GenerateState`, `PrForm`, `FieldState`, `GeneratedDraft`, and draft review placeholders.
- Fix the event tick loop so the interval is owned by `EventHandler` instead of recreated for each event poll.
- Add a typed path for future job results into the app loop, even if no jobs are spawned yet.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/app.rs", "src/event.rs", "src/main.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["state remains concrete", "input mode and navigation mode are distinct", "event loop tick interval is not recreated per poll"]
}
```

## Acceptance Criteria
- `GenerateState` owns all Generate PR-specific fields and selected indices.
- Text input mode and normal navigation mode are distinct in state.
- The event loop supports terminal events, ticks, and future job results.
- Existing navigation still works with placeholder data.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; use a narrower compile check only if the migrated historical implementation already justified that limitation.

## Files Likely Touched
- `src/app.rs`
- `src/event.rs`
- `src/main.rs`
- `src/generate.rs`

## Risks
- State may drift into generic abstractions too early.
- Placeholder job plumbing may overfit future work.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: legacy-migration
- completed_at: 2026-05-25T21:55:30+02:00
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
- reviewed_at: 2026-05-25T21:55:30+02:00
- state: reviewed

Findings:
- Legacy reviewed task migrated into the new ticket lifecycle.
- Original review postmortem was not present in the old note, so this placeholder records the migration only.

Verification:
- Historical review verification was not available in the source note.

Residual risks:
- Placeholder metadata may not reflect the original reviewer run.
