---
id: 0000a-2026-05-25-polished-pr-gen-handoff-polish
created_at: 2026-05-25T21:55:35+02:00
created_by_model: migration-placeholder
state: implemented
state_updated_at: 2026-05-26T06:49:58+02:00
---
# Handoff Polish

## Goal
Bring the completed Generate PR draft workflow to handoff quality with focused tests, documentation updates, and verification.

## Context
This ticket was recreated by the planner from `docs/tasks/open/000a-2026-05-25-polished-pr-gen-handoff-polish.md` after reading all legacy notes. It is the final polish pass for the non-mutating Generate PR draft workflow after app state, command runner, repo discovery, revset selection, form editing, context collection, prompt manifest, Ollama generation, and draft review/edit are in place. The design source of truth remains `docs/design.md`.

## Non-Goals
- Do not expand into Manage PRs or Manage Issues.
- Do not implement push or PR creation.
- Do not add broad architecture contract tests or a regression test farm.
- Do not refactor unrelated code for style alone.

## Design Decisions
- Prefer fixing real rough edges over adding new features.
- Keep refactors limited to cleanup needed for readability and correctness.
- Update `docs/design.md` only if implementation decisions clarify open questions.
- Use focused tests for risky logic and run full handoff checks.

## Implementation Plan
- Review UI copy for clarity and compactness.
- Ensure status/help bars reflect the current mode and phase.
- Ensure all recoverable errors keep useful user state.
- Confirm terminal cleanup on panic and normal exit.
- Add focused tests for prompt assembly, JSON parsing, branch validation, command argv construction, remote parsing, and input mode key behavior.
- Add a short manual test checklist if useful.
- Run the full handoff checks.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/app.rs", "src/ui.rs", "src/generate.rs", "src/prompt.rs", "src/ollama.rs", "src/command.rs", "src/repo.rs", "docs/design.md"],
  "verification_commands": ["just verify"],
  "review_focus": ["Generate PR draft workflow works end to end without mutation", "recoverable errors preserve useful state", "tests remain focused", "docs changes are limited to clarified decisions"]
}
```

## Acceptance Criteria
- The full Generate PR draft workflow works end to end against a jj workspace.
- Errors from missing tools, failed commands, bad Ollama responses, and timeouts are understandable.
- Existing command logs are enough to debug failures.
- `just verify` passes.

## Verification Plan
Run `just verify`. Perform a manual terminal check of the Generate PR draft workflow if practical and record any skipped manual coverage in implementation notes.

## Files Likely Touched
- `src/app.rs`
- `src/ui.rs`
- `src/generate.rs`
- `src/prompt.rs`
- `src/ollama.rs`
- `src/command.rs`
- `src/repo.rs`
- `docs/design.md`

## Risks
- Polish can sprawl into unrelated features.
- Tests can become too broad for the repo's stated testing strategy.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-05-26T06:49:58+02:00
- state: implemented

Completed:
- Tightened Generate PR draft polish by keeping the last generated draft visible across context refresh retries, instead of clearing it up front.
- Reworked terminal cleanup in `src/tui.rs` to be best-effort on normal exit, panic, and partial startup failure.
- Made the status/help bars in `src/ui.rs` reflect the active screen, focus, prompt view, and editing state, and removed stale "not implemented yet" wording.

Deviations:
- No design-doc update was needed; the implementation stayed within the existing Generate PR draft workflow.

Verification:
- `just test`
- `just verify`

Files changed:
- `src/generate.rs`
- `src/tui.rs`
- `src/ui.rs`

Residual risk:
- Manual terminal resize/navigation coverage was not exercised here.
