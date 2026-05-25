---
id: 00002-2026-05-25-polished-pr-gen-command-runner
created_at: 2026-05-25T21:55:30+02:00
created_by_model: migration-placeholder
state: reviewed
state_updated_at: 2026-05-25T21:55:31+02:00
---
# Command Runner

## Goal
Add a small async command runner for `jj`, `git`, and `tea` that enforces the external command boundaries from the design doc.

## Context
This ticket was migrated from `docs/tasks/reviewed/0002-2026-05-25-polished-pr-gen-command-runner.md`. The design source of truth is `docs/design.md`, especially External Command Boundaries, Command Policy, Logs, and Architecture.

## Non-Goals
- Do not launch commands through PowerShell, `cmd`, or another shell.
- Do not implement interactive command categories.
- Do not put command policy in UI rendering code.

## Design Decisions
- Use `tokio::process::Command`.
- Construct every command as an argv array.
- Always set `current_dir`.
- Use configured command paths from `Config`.
- Redact tokens, authorization headers, and obvious secrets from display/log forms.

## Implementation Plan
- Add a `command` module with `ExternalCommand`, `CommandKind`, `CommandResult`, and `CommandRunner` or equivalent functions.
- Capture stdout, stderr, exit status, duration, cwd, and a redacted display form.
- Add timeout support where practical.
- Add job IDs and a minimal `JobRegistry` for queued, running, succeeded, failed, and cancelled jobs.
- Send job results back to the app through a Tokio channel.
- Store raw output in logs for later inspection.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/command.rs", "src/app.rs", "src/config.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["commands are argv arrays", "redaction covers obvious secrets", "UI remains decoupled from command policy"]
}
```

## Acceptance Criteria
- UI rendering remains responsive while a command runs.
- Command results arrive as typed app actions.
- Failed commands keep stdout and stderr in logs.
- No command is launched through PowerShell, `cmd`, or another shell.
- `just verify` passes unless this slice only needs one focused check.

## Verification Plan
Run `just verify`; include focused tests for argv display/redaction and at least one wrapper once wrappers exist.

## Files Likely Touched
- `src/command.rs`
- `src/app.rs`
- `src/config.rs`
- `Cargo.toml`

## Risks
- Async command plumbing can spread into UI code if boundaries are not kept tight.
- Redaction can miss new sensitive display forms.
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
- reviewed_at: 2026-05-25T21:55:31+02:00
- state: reviewed

Findings:
- Legacy reviewed task migrated into the new ticket lifecycle.
- Original review postmortem was not present in the old note, so this placeholder records the migration only.

Verification:
- Historical review verification was not available in the source note.

Residual risks:
- Placeholder metadata may not reflect the original reviewer run.
