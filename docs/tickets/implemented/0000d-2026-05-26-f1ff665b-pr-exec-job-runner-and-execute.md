---
id: 0000d-2026-05-26-f1ff665b-pr-exec-job-runner-and-execute
created_at: 2026-05-26T08:01:34+02:00
created_by_model: claude-opus-4-7/high
state: implemented
state_updated_at: 2026-05-26T09:49:50+02:00
---
# Job Runner and PR Execution

## Goal
Wire the actual PR execution: run the `ExecutionPlan` through a small
job runner, capture the PR URL from `tea pr create`, and transition to a
new `Complete` phase with per-step failure handling.

## Context
The previous ticket lands `ExecutionPlan`, the `Confirming` phase, the
freshness check, and `InputMode::Confirm`. `Enter` in `Confirming`
currently lands on a deliberate dead-end that logs
"execution not yet wired" and stays in `Confirming`. **This ticket
replaces that dead-end.** It re-introduces job tracking (deferred when
Generate PR drafted shipped) and wires the three mutating commands:
`jj bookmark create|move`, `jj git push --bookmark`, and
`tea pr create`. On full success it parses the PR URL and lands in
`Complete`. On any step failure it retains state and lets the user retry.

Pure jj has been chosen for branch operations. `git` is never invoked
directly during execution.

## Non-Goals
- Do not add a Logs screen. Job status surfaces in the existing status
  bar and preview pane only.
- Do not add streaming/incremental output for long-running pushes.
- Do not implement PR update/edit, draft conversion, reviewer assignment,
  or any other `tea pr` operation beyond `create`.
- Do not change Landing or Manage PRs/Issues surfaces.
- Do not add retries that re-issue mutations automatically; user retries
  by pressing `Enter` again from `Failed` (which re-runs validation and
  the freshness check via the existing `c` path).

## Design Decisions
- Re-introduce `JobRecord`, `JobStatus`, and `JobRegistry` (smaller than
  the pre-refactor versions). Fields kept: `id`, `name`, `command` (the
  redacted display), `status`, `duration`, `stdout`, `stderr`,
  `timed_out`. Drop anything not displayed.
- Add `BackgroundEvent::Job(JobResult)` plus a `JobResult` struct
  matching `JobRecord` fields. Add to `src/event.rs`.
- New `pub async fn spawn_job(command: ExternalCommand, name: String, tx:
  UnboundedSender<BackgroundEvent>) -> u64` in `src/command.rs`. Emits a
  Queued event immediately, transitions to Running, runs `capture`,
  then emits Succeeded/Failed/TimedOut. Returns the job id synchronously.
- Add a `tea pr create` builder to `src/tea.rs`. Argv:
  `tea pr create --title <title> --description <body> --base <base>
  --head <branch_name>` plus repeated `--label`, `--assignee`,
  `--milestone` for each non-empty value (split labels/assignees by
  comma, trim, drop empties).
- Add `jj bookmark create`, `jj bookmark move`, and `jj git push
  --bookmark` builders to `src/jj.rs`. The execution code chooses
  create vs move using the same logic as `ExecutionPlan::from_draft`.
- Execution flow: `InputMode::Confirm` + `Enter` triggers
  `Action::ExecuteConfirmed`. App transitions `Confirming â†’ Executing`,
  spawns an executor task that runs the plan **sequentially**: step n+1
  only spawns after step n's `Job` event is Succeeded. The executor task
  holds a clone of `bg_tx`, sends a small `BackgroundEvent::ExecutionStep`
  message between each step so `App` can update progress, then sends a
  final `BackgroundEvent::ExecutionDone(Result<String, ExecutionError>)`
  where `Ok(String)` is the PR URL.
- PR URL parsing: scan `tea pr create` stdout line-by-line, return the
  first match of `https?://\S+`. On no match, return `Complete` with
  `pr_url: None` and a log line "URL not parsed; see job stdout in logs".
- Add `GeneratePhase::Complete` already exists in the enum; ensure its
  `label()` is wired and Confirming â†’ Executing â†’ Complete transitions
  exist.
- New `GenerateState` field `completion: Option<Completion>` with
  `Completion { pr_url: Option<String>, plan: ExecutionPlan }`. Cleared
  on `back()` / Esc from Complete.
- Per-step failure: on first failing job, transition to `Failed` with a
  log entry describing the failed step. The `ExecutionPlan` is retained
  and rendered in the preview pane along with which step failed. Pressing
  `c` re-runs validation + freshness check and starts over.
- Status bar gains a compact job indicator: "job:<status>" when a job is
  running, "job:idle" otherwise. Preview pane in `Executing` shows the
  job list with status markers.
- The dead-end behavior in the previous ticket is removed: the body of
  `Action::ExecuteConfirmed` is rewritten to spawn the executor.

## Implementation Plan
- Add `JobStatus`, `JobResult`, `JobRecord`, `JobRegistry` to
  `src/event.rs` and `src/app.rs` respectively. Hook `JobRegistry` into
  `App`.
- Add `BackgroundEvent::Job(JobResult)`, `BackgroundEvent::ExecutionStep
  { index: usize, total: usize }`, and `BackgroundEvent::ExecutionDone
  (ExecutionOutcome)`. `ExecutionOutcome` carries
  `{ pr_url: Option<String>, failed_step: Option<usize>, message: Option
  <String> }`.
- Implement `command::spawn_job` and a small helper `run_plan_sequentially
  (plan: ExecutionPlan, tx: UnboundedSender<BackgroundEvent>)` in
  `src/command.rs` (or `src/generate.rs` if it reads better).
- Add the `tea pr create` builder + a small `parse_pr_url(stdout: &str)
  -> Option<String>` helper in `src/tea.rs`.
- Add `jj bookmark create`, `jj bookmark move`, and `jj git push
  --bookmark` builders in `src/jj.rs`.
- Replace `Action::ExecuteConfirmed`'s dead-end body in `src/app.rs`
  with: transition to `Executing`, take ownership of the
  `execution_plan`, spawn the executor task. Remove the
  "execution not yet wired" log line.
- Wire `BackgroundEvent::Job`, `ExecutionStep`, and `ExecutionDone`
  handlers in `App`. `ExecutionDone` transitions to `Complete` or
  `Failed` depending on the outcome.
- Update `src/ui.rs`: render `Executing` preview with job list, render
  `Complete` preview with PR URL and a hint to press `Esc` to return.
- Update help bar branches: `Complete` shows `Esc back`.
- Tests covering: `parse_pr_url` extracts the URL from a sample tea
  output and returns `None` for plain text; `jj bookmark create` /
  `move` / `git push --bookmark` argv; `tea pr create` argv with and
  without optional fields; comma-split label/assignee normalization.
- Run `just verify`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/app.rs", "src/generate.rs", "src/command.rs", "src/event.rs", "src/jj.rs", "src/tea.rs", "src/ui.rs"],
  "likely_files": ["src/app.rs", "src/event.rs", "src/command.rs", "src/generate.rs", "src/jj.rs", "src/tea.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "the previous ticket's dead-end log line in Action::ExecuteConfirmed is removed",
    "steps run sequentially: step n+1 only after step n succeeds",
    "per-step failure preserves ExecutionPlan and lets user retry via c",
    "PR URL parsing is best-effort and never panics on weird tea output",
    "redacted display is used in all UI surfaces; no token/secret can leak",
    "no direct git invocation; jj is the only branch/push tool"
  ]
}
```

## Acceptance Criteria
- Pressing `Enter` in `Confirming` spawns the first job (`jj bookmark
  create|move`) and transitions to `Executing`.
- Each step's job appears in the registry with Queued â†’ Running â†’
  Succeeded/Failed transitions visible in the status bar and preview.
- All steps succeed â†’ `Complete` with `pr_url` populated (or
  `pr_url: None` with a log line if the URL could not be parsed).
- Any step fails â†’ `Failed` with a clear log line naming the step;
  pressing `c` re-runs validation/freshness and tries again.
- The dead-end log line from the previous ticket is gone.
- `tea pr create` argv includes `--label`/`--assignee`/`--milestone`
  only when the corresponding form fields are non-empty.
- `just verify` passes.

## Verification Plan
Run `just verify`. Unit tests for `parse_pr_url`, the new jj/tea argv
builders, and label/assignee splitting. Manual: against a real jj
workspace + Gitea + Ollama, generate a draft, press `c`, press `Enter`,
verify the three jobs run in order and a PR URL is captured. Then force a
failure (e.g. push without auth) and verify state is preserved.

## Files Likely Touched
- `src/app.rs`
- `src/event.rs`
- `src/command.rs`
- `src/generate.rs`
- `src/jj.rs`
- `src/tea.rs`
- `src/ui.rs`

## Risks
- `tea pr create` exit codes are inconsistent on certain failure modes;
  the executor must treat any non-zero exit as failure and surface stderr.
- Parsing PR URLs from stdout is fragile; the test must use a realistic
  sample and the helper must never panic.
- Sequential job orchestration via channel messages must not deadlock if
  a step's `Job` event is dropped. The executor task should observe
  child completion directly via `capture` rather than relying on
  re-receiving its own `BackgroundEvent::Job` messages.
- The Failed â†’ retry path must not double-spawn jobs if the user presses
  `c` while `Executing`; gate `c` on non-Executing phases.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5.4-mini
- completed_at: 2026-05-26T09:49:50+02:00
- state: implemented

Completed the PR execution wiring for ticket 0000d-2026-05-26-f1ff665b-pr-exec-job-runner-and-execute.

What changed:
- Reintroduced job tracking with `JobStatus`, `JobResult`, `JobRecord`, and `JobRegistry`.
- Added `BackgroundEvent::Job`, `ExecutionStep`, and `ExecutionDone` handling.
- Implemented async job spawning plus sequential execution of the bookmark, push, and `tea pr create` steps.
- Added typed `jj` bookmark/push builders and a typed `tea pr create` builder plus PR URL parsing.
- Replaced the Confirming dead-end with the real execution path, completion state, and retry handling from Failed.
- Updated the UI for job status, execution progress, completion, and failure previews.

Deviations:
- Used a small `PrCreateArgs` request struct to keep the `tea` builder clippy-clean instead of a wide argument list.
- Kept execution output visible through the existing preview/status/log surfaces rather than adding a new logs screen.

Verification:
- `just verify`

Files changed:
- `src/app.rs`
- `src/command.rs`
- `src/event.rs`
- `src/generate.rs`
- `src/jj.rs`
- `src/tea.rs`
- `src/ui.rs`

Residual risk:
- PR URL parsing remains best-effort against `tea` stdout format drift.
- The job registry keeps history across attempts, which is useful for inspection but not a hard reset between retries.
