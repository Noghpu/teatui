# Revset Selector

## Goal

Populate the Generate PR left pane with real candidate jj revsets and useful
preview details.

## Outcome

The user can enter Generate PR, see candidate revsets, move through them, and
inspect enough detail to choose the stack or change that should become a PR.

## Scope

- Add a `jj` module with read-only wrappers for candidate revset summaries.
- Query initial candidate revsets:
  - `@`
  - `@-`
  - `heads(trunk()..)`
- Gather short descriptions, bookmarks, change counts, commit/change IDs, diff
  stats, and warnings where feasible.
- Replace placeholder revsets in `App::new`.
- Render revset list rows compactly in the left pane.
- Render selected revset detail in the preview pane.
- Reject or warn on conflicted, ambiguous, empty, or multi-head states when the
  first version cannot handle them safely.

## Implementation Notes

- Prefer machine-readable jj output if it is practical for the installed jj
  version.
- If text parsing is required, keep parsing isolated inside the `jj` module and
  preserve raw output in logs.
- Avoid solving every revset default question now. Pick a conservative default
  and make it visible.

## Acceptance Criteria

- Generate PR shows real revsets from the current jj workspace.
- Selecting a revset updates the form `head` default.
- Preview includes description, bookmarks, stats, recent log context, and
  warnings.
- Refresh reloads revset data.
- `just fmt`, `just check`, and `just clippy` pass.

## Tests

- Unit test parsers introduced for jj output.
- Unit test candidate revset command argv construction.
