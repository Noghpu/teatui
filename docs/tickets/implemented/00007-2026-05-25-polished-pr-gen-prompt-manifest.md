---
id: 00007-2026-05-25-polished-pr-gen-prompt-manifest
created_at: 2026-05-25T21:55:34+02:00
created_by_model: migration-placeholder
state: implemented
state_updated_at: 2026-05-25T21:55:34+02:00
---
# Prompt Manifest

## Goal
Build the one-shot PR generation prompt and a reviewable manifest before any LLM request is made.

## Context
This ticket was migrated from `docs/tasks/implemented/0007-2026-05-25-polished-pr-gen-prompt-manifest.md`. The design source of truth is `docs/design.md`, especially AI Prompt Strategy, Prompt Contract, Prompt Outline, Safety and Review, and Testing Strategy.

## Non-Goals
- Do not include config secrets, environment variables, tokens, or auth headers.
- Do not add multi-provider prompt abstractions.
- Do not put model-specific behavior into prompt assembly unless required by Ollama-compatible APIs.

## Design Decisions
- Add deterministic prompt assembly in a `prompt` module.
- Return both the final prompt string and a structured manifest.
- Treat user-entered branch, title, and body values as stronger intent than inferred values.
- Use byte-budget based truncation with explicit omitted sections and warnings.

## Implementation Plan
- Add `PromptBuild`, `PromptManifest`, `PromptSection`, and `OmittedSection`.
- Build the strict JSON output contract prompt from the design doc.
- Include repository summary, selected jj changes, status, log, descriptions, diff stats, diff context, user instructions, and PR form values.
- Prefer full diffs for small changes.
- For large changes, include file-level summaries, selected hunks if available, and explicit truncation markers.
- Render manifest by default in the right pane, with a way to view prompt text later if practical.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/prompt.rs", "src/generate.rs", "src/ui.rs", "src/app.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["prompt requests strict JSON without a Markdown fence", "manual form values are explicit user intent", "truncation and omitted sections are visible before generation", "secrets are not included"]
}
```

## Acceptance Criteria
- `ContextReady` displays a useful prompt manifest.
- The generated prompt asks for strict JSON and no Markdown fence.
- Truncation is visible before generation.
- Manual form values are included as explicit user intent.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; include unit tests for dirty form values, strict JSON schema, truncation warnings/omitted sections, and excluded secrets/config values.

## Files Likely Touched
- `src/prompt.rs`
- `src/generate.rs`
- `src/ui.rs`
- `src/app.rs`

## Risks
- Prompt assembly can accidentally include sensitive config or environment data.
- Truncation can hide important context unless warnings are clear.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: legacy-migration
- completed_at: 2026-05-25T21:55:34+02:00
- state: implemented

Completed:
- Legacy implemented task migrated into the new ticket lifecycle.
- Original implementation details were not present in the old note, so this placeholder records the migration only.

Deviations:
- Placeholder lifecycle note used because historical implementation output was unavailable.

Verification:
- Historical verification was not available in the source note.

Files changed:
- Placeholder: see implementation revision history for actual historical files.

Residual risks:
- Placeholder metadata may not reflect the original implementer run.
