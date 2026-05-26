---
id: 00009-2026-05-25-polished-pr-gen-draft-review
created_at: 2026-05-25T21:55:35+02:00
created_by_model: migration-placeholder
state: implemented
state_updated_at: 2026-05-26T06:44:11+02:00
---
# Draft Review

## Goal
Turn generated PR metadata into a polished review and edit experience before any mutation is implemented.

## Context
This ticket was recreated by the planner from `docs/tasks/open/0009-2026-05-25-polished-pr-gen-draft-review.md` after reading all legacy notes. It should build on generated drafts from the Ollama client and the existing form/input machinery. The design source of truth is `docs/design.md`, especially Generate PR states, UI Model, Logs, and Safety and Review.

## Non-Goals
- Do not implement push or `tea pr create` in this slice.
- Do not persist draft state outside memory.
- Do not add modal complexity unless the current layout cannot show the data cleanly.

## Design Decisions
- Reuse the form editing machinery for draft fields.
- Keep generated draft state in memory for now.
- Use Ratatui `Paragraph`, wrapping, scrolling, and state objects before custom rendering.
- Show a non-mutating execution preview placeholder for future work.

## Implementation Plan
- Add `DraftReady` review rendering in the right and center panes.
- Show generated branch name, PR title, PR body, review notes, and manifest warnings.
- Reuse existing field editing for draft fields.
- Add clear state for retrying generation without losing context.
- Add logs access or a log preview sufficient to inspect command and model failures.
- Make resize behavior reasonable for narrow terminals.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/generate.rs", "src/ui.rs", "src/app.rs", "src/ollama.rs", "src/prompt.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["draft fields are editable and preserve user edits", "retry does not lose context", "UI clearly avoids mutation", "ordinary terminal sizes remain readable"]
}
```

## Acceptance Criteria
- Draft fields are reviewable and editable.
- Failed generation can be retried.
- User edits to generated draft are preserved while navigating.
- The UI clearly says execution is not implemented yet.
- Text does not overlap or become unreadable in ordinary terminal sizes.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; add focused unit tests for draft field editing if it diverges from form editing. Add snapshot-style render tests only if they catch real layout risk without becoming broad regression coverage.

## Files Likely Touched
- `src/generate.rs`
- `src/ui.rs`
- `src/app.rs`
- `src/ollama.rs`
- `src/prompt.rs`

## Risks
- Draft review can accidentally imply that execution is implemented.
- Narrow terminal rendering can become unreadable if wrapping and scrolling are not explicit.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5.4-mini
- completed_at: 2026-05-26T06:44:11+02:00
- state: implemented

Metadata:
- model: gpt-5.4-mini
- completed_at: 2026-05-26T00:00:00+02:00
- state: implemented

Completed:
- Added draft-ready review rendering in the Generate PR center and preview panes.
- Surfaced generated branch name, PR title, PR body, review notes, manifest warnings, and recent logs in the draft review view.
- Synced successful generated drafts back into the editable form fields so the generated draft is the review surface.
- Preserved the last draft across retry attempts so generation can be retried without losing context.
- Added readable multiline rendering for the PR body and wrapped the work/preview panes for narrow terminals.

Deviations:
- Kept the existing prompt-manifest toggle for context-ready review, but the draft-ready pane now defaults to the generated draft instead of the prompt view.
- Preserved the previous draft during generation retries instead of blanking the screen, which better matches the retry-with-context requirement.

Verification:
- `just verify` passed.

Files changed:
- `src/generate.rs`
- `src/ui.rs`

Residual risks:
- The preview still depends on line wrapping and clipped terminal height for very large drafts.
- A retry can still overwrite any in-progress edits if generation finishes while the user is editing the same fields.
