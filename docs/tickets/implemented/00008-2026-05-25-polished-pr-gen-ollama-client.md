---
id: 00008-2026-05-25-polished-pr-gen-ollama-client
created_at: 2026-05-25T21:55:34+02:00
created_by_model: migration-placeholder
state: implemented
state_updated_at: 2026-05-25T22:18:41+02:00
---
# Ollama Client

## Goal
Send the assembled prompt to the configured Ollama-compatible endpoint and parse the model response into a validated generated draft.

## Context
This ticket was recreated by the planner from `docs/tasks/open/0008-2026-05-25-polished-pr-gen-ollama-client.md` after reading all legacy notes. It follows the completed sequence: app state, command runner, repo discovery, revset selector, form editing, context collection, and prompt manifest. The design source of truth is `docs/design.md`, especially Ollama Contract, Prompt Contract, Generate PR, Logs, and Error Handling.

## Non-Goals
- Do not add a provider abstraction.
- Do not execute model-generated commands.
- Do not trust model branch names until locally validated.
- Do not implement branch push or PR creation in this slice.

## Design Decisions
- Add an `ollama` module for configured Ollama-compatible HTTP requests.
- Use non-streaming generation for the first version.
- Use low temperature for stable PR metadata.
- Treat malformed model output as recoverable workflow failure.
- Store raw model responses in logs for inspection and retry.

## Implementation Plan
- Add minimal HTTP and JSON dependencies, likely `reqwest`, `serde`, and `serde_json`.
- Support configured base URL and model from config.
- Add request timeout and visible connection/timeout failures.
- Wire `g` after `ContextReady` so generation moves through `Generating` to `DraftReady` or `Failed`.
- Parse strict JSON response into `GeneratedDraft`.
- Validate branch name, title, body, and review notes.
- Keep malformed JSON visible and retryable without losing context.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["Cargo.toml", "src/ollama.rs", "src/prompt.rs", "src/generate.rs", "src/app.rs", "src/config.rs", "src/command.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["malformed JSON is recoverable", "branch names are locally validated", "raw model responses are logged", "no provider abstraction is introduced"]
}
```

## Acceptance Criteria
- `g` can trigger generation after context and prompt are ready.
- Generation progress is visible.
- Valid model output creates a `GeneratedDraft`.
- Invalid output leaves raw response in logs and keeps current context.
- Timeout and connection failures are user-visible.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; include unit tests for generated JSON parsing, missing required fields, invalid branch names, and review notes normalization.

## Files Likely Touched
- `Cargo.toml`
- `src/ollama.rs`
- `src/prompt.rs`
- `src/generate.rs`
- `src/app.rs`
- `src/config.rs`
- `src/command.rs`

## Risks
- Ollama-compatible endpoints may differ slightly in response shape.
- Timeouts and malformed output need enough raw context for debugging without exposing secrets.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-05-25T22:18:41+02:00
- state: implemented

Completed:
- Added `src/ollama.rs` with a reqwest-based Ollama client targeting `/api/generate` using non-streaming requests and low temperature.
- Parsed strict JSON model output into `GeneratedDraft`, with local branch/title/body validation and review-note normalization.
- Wired Generate PR to move from context-ready into generating, then into draft-ready or failed via a new generation event channel.
- Logged raw model responses in the app log on both success and failure.
- Added generation-failure state to keep context visible after malformed output or request errors.

Deviations:
- Used a dedicated generation event channel instead of reusing job or context plumbing.
- Logged raw responses as line-by-line log entries rather than a single blob.

Verification:
- `just verify` passed.

Files changed:
- `Cargo.toml`
- `Cargo.lock`
- `src/action.rs`
- `src/app.rs`
- `src/event.rs`
- `src/generate.rs`
- `src/main.rs`
- `src/ollama.rs`
- `src/ui.rs`

Residual risks:
- Ollama-compatible servers may vary in wrapper response shape or endpoint behavior.
- Full end-to-end generation still depends on a live configured Ollama endpoint at runtime.
