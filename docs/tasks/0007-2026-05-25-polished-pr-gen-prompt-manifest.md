# Prompt Manifest

## Goal

Build the one-shot PR generation prompt and a reviewable manifest before any
LLM request is made.

## Outcome

The user can inspect what context will be sent to Ollama, including included
sections, omitted sections, byte counts, and truncation warnings.

## Scope

- Add a `prompt` module.
- Implement `PromptBuild`, `PromptManifest`, `PromptSection`, and
  `OmittedSection`.
- Build the strict JSON output contract prompt from the design doc.
- Include repository summary, selected jj changes, status, log, descriptions,
  diff stats, diff context, user instructions, and PR form values.
- Treat user-entered branch/title/body values as stronger intent than inferred
  values.
- Add byte-budget based truncation.
- Prefer full diffs for small changes.
- For large changes, include file-level summaries, selected hunks if available,
  and explicit truncation markers.
- Render manifest by default in the right pane, with a way to view prompt text
  later if practical.

## Implementation Notes

- Keep prompt assembly deterministic and easy to test.
- Do not include config secrets, environment variables, tokens, or auth headers.
- Do not add multi-provider prompt abstractions.
- Keep model-specific behavior out of prompt assembly unless required by
  Ollama-compatible APIs.

## Acceptance Criteria

- `ContextReady` displays a useful prompt manifest.
- The generated prompt asks for strict JSON and no Markdown fence.
- Truncation is visible before generation.
- Manual form values are included as explicit user intent.
- `just verify` passes unless this slice only needs one focused check.

## Tests

- Unit test prompt includes dirty form values.
- Unit test prompt includes strict JSON schema.
- Unit test truncation warnings and omitted sections.
- Unit test secrets/config values are not included.
