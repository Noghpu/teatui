# App Loop And State

## Goal

Replace the demo-oriented app state with explicit state for the polished
Generate PR workflow, while keeping the existing Elm-style update loop simple.

## Outcome

The application has clear top-level state for screens, focus, input mode, job
status, logs, repository detection, and Generate PR workflow phases. The UI may
still show placeholder data, but the state model should match the intended
product flow.

## Scope

- Introduce explicit `InputMode`, `Focus`, and `GeneratePhase` enums.
- Move Generate PR-specific state into a dedicated `generate` module.
- Add `GenerateState`, `PrForm`, `FieldState`, `GeneratedDraft`, and draft
  review placeholders.
- Keep state concrete rather than creating generic widget maps or a component
  framework.
- Fix the event tick loop so the interval is owned by `EventHandler` instead
  of recreated for each event poll.
- Add a typed path for future job results into the app loop, even if no jobs are
  spawned yet.

## Implementation Notes

- Preserve the current `Action -> update -> render` shape.
- Keep `update` responsible for state transitions only.
- Keep rendering synchronous and side-effect free.
- Avoid introducing async traits or dynamic dispatch in the app state.

## Acceptance Criteria

- `GenerateState` owns all Generate PR-specific fields and selected indices.
- Text input mode and normal navigation mode are distinct in state.
- The event loop supports terminal events, ticks, and future job results.
- Existing navigation still works with placeholder data.
- `just fmt` and `just check` pass.

## Tests

No broad test suite needed. Add narrow unit tests only if helper methods for
field focus or phase transitions become nontrivial.
