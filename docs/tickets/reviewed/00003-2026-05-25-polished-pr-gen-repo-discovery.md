---
id: 00003-2026-05-25-polished-pr-gen-repo-discovery
created_at: 2026-05-25T21:55:31+02:00
created_by_model: migration-placeholder
state: reviewed
state_updated_at: 2026-05-25T21:55:32+02:00
---
# Repo Discovery

## Goal
Replace fake landing and Generate PR repository state with real read-only workspace, tool, remote, and base-branch discovery.

## Context
This ticket was migrated from `docs/tasks/reviewed/0003-2026-05-25-polished-pr-gen-repo-discovery.md`. The design source of truth is `docs/design.md`, especially Landing, Generate PR, Configuration, and External Command Boundaries.

## Non-Goals
- Do not make discovery failures fatal startup errors.
- Do not run expensive discovery commands every tick.
- Do not guess owner/repo when remote parsing is uncertain.

## Design Decisions
- Add conservative repo discovery in a `repo` module.
- Use config `pr.default_base` as the conservative base branch default.
- Surface setup blockers in the Landing preview pane.
- Treat discovery failures as displayable status.

## Implementation Plan
- Add `RepoState`, `ToolStatus`, `RemoteInfo`, and base branch metadata.
- Detect `jj`, `git`, and `tea` availability with noninteractive commands.
- Detect the jj workspace root.
- Detect Git remote URL and parse owner/repo for common Gitea-style remotes.
- Add refresh behavior for discovery.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/repo.rs", "src/command.rs", "src/app.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["discovery failures are visible", "remote parsing is conservative", "Generate PR handles missing jj workspace clearly"]
}
```

## Acceptance Criteria
- Landing no longer shows hard-coded setup statuses.
- Discovery failures are visible and actionable.
- Generate PR can refuse entry or show a clear blocker when no jj workspace is detected.
- `Esc` and `q` behavior remains consistent with the design doc.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; include unit tests for SSH and HTTPS remote URL parsing.

## Files Likely Touched
- `src/repo.rs`
- `src/command.rs`
- `src/app.rs`
- `src/ui.rs`

## Risks
- Remote parsing may over-accept unsupported URLs.
- Discovery can become too eager and slow startup or refresh.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: legacy-migration
- completed_at: 2026-05-25T21:55:31+02:00
- state: implemented

Completed:
- Legacy completed task migrated into the new ticket lifecycle.
- Original implementation details were not present in the old note, so this placeholder records the migration only.

Deviations:
- Placeholder lifecycle note used because historical implementation output was unavailable.

Verification:
- Historical verification was not available in the source note.

Files changed:
- Placeholder: see implementation revision history for actual historical files.

Residual risks:
- Placeholder metadata may not reflect the original implementer run.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: legacy-migration
- reviewed_at: 2026-05-25T21:55:32+02:00
- state: reviewed

Findings:
- Legacy reviewed task migrated into the new ticket lifecycle.
- Original review postmortem was not present in the old note, so this placeholder records the migration only.

Verification:
- Historical review verification was not available in the source note.

Residual risks:
- Placeholder metadata may not reflect the original reviewer run.
