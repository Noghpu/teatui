# Ollama Client

## Goal

Send the assembled prompt to the configured Ollama-compatible endpoint and parse
the model response into a validated generated draft.

## Outcome

Generate PR can move from `ContextReady` to `Generating` to `DraftReady` or
`Failed` using a real local or on-prem Ollama-compatible model.

## Scope

- Add an `ollama` module.
- Add minimal HTTP dependencies, likely `reqwest`, `serde`, and `serde_json`.
- Support configured base URL and model.
- Use non-streaming generation for the first version.
- Use low temperature for stable PR metadata.
- Add request timeout and visible failure.
- Parse strict JSON response into `GeneratedDraft`.
- Validate branch name, title, body, and review notes.
- Store raw model response in logs.
- Keep malformed JSON visible and retryable.

## Implementation Notes

- Do not add a provider abstraction.
- Keep endpoint shape configurable enough for Ollama-compatible deployments.
- Treat malformed model output as a recoverable workflow error.
- Never execute model-generated commands.
- Do not trust model branch names until locally validated.

## Acceptance Criteria

- `g` can trigger generation after context and prompt are ready.
- Generation progress is visible.
- Valid model output creates a `GeneratedDraft`.
- Invalid output leaves raw response in logs and keeps current context.
- Timeout and connection failures are user-visible.
- `just verify` passes unless this slice only needs one focused check.

## Tests

- Unit test generated JSON parsing.
- Unit test missing required fields.
- Unit test invalid branch names.
- Unit test review notes normalization.
