# Handoff Polish

## Goal

Bring the completed Generate PR draft workflow to handoff quality with focused
tests, documentation updates, and verification.

## Outcome

The first target is coherent and shippable as a polished PR generation draft
experience: repository discovery, revset selection, form editing, context
manifest, Ollama generation, draft review/edit, logs, and recoverable errors.

## Scope

- Review UI copy for clarity and compactness.
- Ensure status/help bars reflect the current mode and phase.
- Ensure all recoverable errors keep useful user state.
- Confirm terminal cleanup on panic and normal exit.
- Add focused tests for risky logic:
  - prompt assembly
  - JSON parsing
  - branch validation
  - command argv construction
  - remote parsing
  - input mode key behavior
- Update `docs/design.md` only if implementation decisions clarify open
  questions.
- Add a short manual test checklist if useful.
- Run the full handoff checks.

## Implementation Notes

- Do not add broad architecture contract tests.
- Do not expand into Manage PRs, Manage Issues, push, or PR creation.
- Keep refactors limited to cleanup needed for readability and correctness.
- Prefer fixing real rough edges over adding new features.

## Acceptance Criteria

- The full Generate PR draft workflow works end to end against a jj workspace.
- Errors from missing tools, failed commands, bad Ollama responses, and timeouts
  are understandable.
- Existing command logs are enough to debug failures.
- `just verify` passes.

## Tests

- Run `just fmt`.
- Run `just check`.
- Run `just clippy`.
- Run `just test`.
- Run `just verify`.
