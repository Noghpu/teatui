# Context Collection

## Goal

Collect all read-only repository context needed to build a PR generation prompt
for the selected revset and current form values.

## Outcome

Pressing `g` from Generate PR normal/review mode starts a context collection job
and moves through `CollectingContext` to `ContextReady` or `Failed` without
blocking the UI.

## Scope

- Add `ContextBundle` with raw and parsed data for:
  - selected revset
  - base branch
  - current `jj status`
  - selected revset log
  - selected descriptions
  - diff stats
  - selected diff
  - remote metadata
  - form values
- Capture collection start time and repo identity for stale-context checks
  later.
- Preserve raw command output in logs.
- Surface collection progress in the status bar and preview pane.
- Show recoverable failure state with retained form values.

## Implementation Notes

- Collect context using sequential commands first unless parallelism is clearly
  valuable and does not complicate error reporting.
- Keep context gathering read-only.
- Avoid prompt construction in command wrapper modules.
- Do not silently overwrite user-entered form values with inferred defaults.

## Acceptance Criteria

- `g` starts context collection only when required inputs are valid enough.
- UI remains responsive during collection.
- Success stores a complete `ContextBundle`.
- Failure keeps the selected revset and form edits intact.
- Logs show every external command that ran.
- `just fmt`, `just check`, and `just clippy` pass.

## Tests

- Unit test context bundle assembly from fake command outputs if parsing logic is
  introduced.
- Unit test failure mapping from command result to user-visible error.
