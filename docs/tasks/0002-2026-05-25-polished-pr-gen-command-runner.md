# Command Runner

## Goal

Add a small async command runner for `jj`, `git`, and `tea` that enforces the
external command boundaries from the design doc.

## Outcome

The app can run read-only external commands asynchronously without blocking
rendering, preserve command logs, and return typed results to the update loop.

## Scope

- Add a `command` module with `ExternalCommand`, `CommandKind`, `CommandResult`,
  and `CommandRunner` or equivalent functions.
- Construct all commands as argv arrays. Do not use shell strings.
- Capture stdout, stderr, exit status, duration, cwd, and a redacted display
  form.
- Add timeout support where practical.
- Add job IDs and a minimal `JobRegistry` for queued, running, succeeded,
  failed, and cancelled jobs.
- Send job results back to the app through a Tokio channel.
- Store raw output in the logs state for later inspection.

## Implementation Notes

- Use `tokio::process::Command`.
- Always set `current_dir`.
- Use configured command paths from `Config`.
- Keep command policy in wrappers and runner code, not in UI code.
- Use `kill_on_drop(true)` when a dropped future should terminate the child
  process.
- Redact tokens, authorization headers, and obvious secrets from display/log
  forms.

## Acceptance Criteria

- UI rendering remains responsive while a command runs.
- Command results arrive as typed app actions.
- Failed commands keep stdout and stderr in logs.
- No command is launched through PowerShell, `cmd`, or another shell.
- `just fmt`, `just check`, and `just clippy` pass.

## Tests

- Unit test argv display/redaction.
- Unit test command construction for at least one `jj` wrapper call once wrappers
  exist.
