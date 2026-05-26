---
id: 0000c-2026-05-26-2a94c3dc-pr-exec-execution-plan-and-confirming
created_at: 2026-05-26T08:01:29+02:00
created_by_model: claude-opus-4-7/high
state: open
---
# ExecutionPlan and Confirming Phase

## Goal
Compute and preview the exact commands that would create the PR â€” without
running anything â€” so the user can validate the plan and detect stale
context before the execution slice lands.

## Context
Generate PR currently ends at `DraftReady`. The execution sequence
described in `docs/design.md` requires a confirmation step that previews
the planned commands and detects stale repo state. This ticket builds the
plan, the new phase, and the input mode that gates execution. A separate
ticket wires the actual mutating commands.

`ExternalCommand::redacted_display` already exists for this purpose
(see Deferred Implementation Notes in `docs/design.md`).

## Non-Goals
- Do not spawn any mutating jj/git/tea command.
- Do not re-introduce `JobRecord` / `JobRegistry` yet; that lands with the
  execution ticket.
- Do not parse PR URLs or implement a `Complete` phase.
- Do not change Landing or Manage PRs/Issues surfaces.
- Do not change the `tea` wrapper from the previous ticket; if the
  `tea pr create` builder is missing it is intentionally added in the
  execution ticket alongside its caller.

## Design Decisions
- New `ExecutionPlan { steps: Vec<ExecutionStep> }`, where
  `ExecutionStep { command: ExternalCommand, label: String }`. The label is
  the short human-readable description (e.g. "create or move bookmark",
  "push bookmark to origin", "create gitea PR").
- The plan is built from the current `DraftReady` state: it consumes the
  selected revset, the committed `PrForm` values, and the
  `RepoState.base_branch`. It is not stored on `GenerateState` long-term;
  it is rebuilt when the user requests Confirming.
- For this slice the plan contains placeholder commands for the steps that
  will be wired in the execution ticket. The actual command argv must match
  what the execution ticket will use so the preview is honest:
  - `jj bookmark set <name> -r <head>` (use `--allow-backwards` is rejected
    by policy; instead, prefer `jj bookmark create <name> -r <head>` when
    the bookmark does not exist on `head`, and `jj bookmark move <name> -r
    <head>` when it does â€” choose based on whether the selected revset's
    bookmarks already include the form's branch name).
  - `jj git push --bookmark <name>`.
  - `tea pr create --title <title> --description <body> --base <base>
    --head <name>` plus `--label`/`--assignee`/`--milestone` repeats when
    those form fields are non-empty.
- A `tea pr create` builder is **not** added in this ticket. Instead, the
  plan stores a synthetic `ExternalCommand` constructed inline that
  matches the future builder's argv. This keeps `src/tea.rs` free of
  unused exports until the execution ticket needs them.
- New `Action::ConfirmExecution` bound to `c` from `DraftReady`. On press:
  1. Re-validate the form: head non-empty, base non-empty, branch name
     passes shape rules, title non-empty, body non-empty, label/assignee
     strings contain no shell metacharacters
     (`;` `&` `|` `` ` `` `$` `<` `>` newline).
  2. Spawn an async stale-context check task that runs the existing
     `jj log -r <selected_revset>` and compares the parsed commit ID list
     against the `ContextBundle.selected_revset.commit_ids`. Result flows
     in via a new `BackgroundEvent::StaleCheck(StaleCheckResult)`.
  3. While the stale check is in flight, phase = `CheckingFreshness`
     (a new transient phase) and the preview pane shows "verifying repo
     context...". `CheckingFreshness` is a Generate-internal phase only.
  4. On `Fresh`: transition to `Confirming` and build the `ExecutionPlan`.
  5. On `Stale { reason }`: transition to `Failed`, log the reason, and
     instruct user to press `r` to refresh revsets/context.
- New `InputMode::Confirm`. `Enter` from Confirming triggers
  `Action::ExecuteConfirmed`, which **for this ticket** logs
  "execution not yet wired (see ticket
  0000d-2026-05-26-pr-exec-job-runner-and-execute)" and stays in
  `Confirming`. The execution ticket replaces this body with the actual
  execute call.
- `Esc` from Confirming returns to `DraftReady` and clears the plan.
- Validation failures during step 1 stay in `DraftReady` and append a
  human-readable line to the log per failing rule.
- Confirming preview pane renders the plan as a numbered list using
  `ExternalCommand::redacted_display`, plus the validation summary and
  the freshness result line.
- Help bar gets a new branch for `InputMode::Confirm` listing
  `Enter execute`, `Esc cancel`.

## Implementation Plan
- Add `GeneratePhase::CheckingFreshness` to `src/generate.rs` and update
  the `label()` match.
- Add `InputMode::Confirm` and its `label()` arm.
- Add `Action::ConfirmExecution` and `Action::ExecuteConfirmed` to
  `src/action.rs`. Map `KeyCode::Char('c')` in normal mode to
  `ConfirmExecution` *only* on Generate PR + DraftReady; otherwise no-op.
  Map `KeyCode::Enter` to `ExecuteConfirmed` when `InputMode::Confirm`.
- Add `ExecutionPlan` and `ExecutionStep` types to `src/generate.rs`. Add
  `GenerateState::execution_plan: Option<ExecutionPlan>` cleared on
  cancel/back and rebuilt on entry to Confirming.
- Add `BackgroundEvent::StaleCheck(StaleCheckResult)` and a small
  `StaleCheckResult` enum.
- Wire the freshness check: a `tokio::spawn` task using
  `JjClient::revset_log_command` + `command::capture`, parsed by the
  existing log parser, compared by sorted commit-id sets against the
  stored `ContextBundle`. Result sent on the background channel.
- Add `validate_for_execution(&PrForm, &RepoState) -> Result<(), Vec<String>>`
  in `src/generate.rs`. Cover: head, base, branch_name shape, title,
  body, label/assignee/milestone metacharacters.
- Compose `ExecutionPlan::from_draft(form, repo, revset)` returning the
  three steps with correct argv. Choose `jj bookmark create` vs
  `jj bookmark move` based on whether the form's branch name already
  appears in the selected revset's bookmarks.
- Render Confirming preview lines in `src/ui.rs`. Reuse
  `render_recent_logs` style for compactness.
- Update help bar branch for `InputMode::Confirm`.
- Tests covering: validate_for_execution rejects bad inputs;
  `ExecutionPlan::from_draft` produces expected argv for both bookmark
  create and bookmark move; freshness comparison is order-independent.
- Run `just verify`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/generate.rs", "src/app.rs", "src/jj.rs", "src/ui.rs", "src/event.rs"],
  "likely_files": ["src/generate.rs", "src/app.rs", "src/action.rs", "src/event.rs", "src/jj.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "no mutating command is spawned in this slice",
    "ExecutionPlan argv matches what the execution ticket will run",
    "stale-context detection is order-independent and explicit",
    "validate_for_execution rejects shell metacharacters in optional fields",
    "Enter in Confirming logs the dead-end message and stays in Confirming",
    "Esc in Confirming returns cleanly to DraftReady and clears the plan"
  ]
}
```

## Acceptance Criteria
- Pressing `c` in `DraftReady` runs validation, runs the freshness check,
  then either: transitions to `Confirming` with a fully constructed plan,
  or returns to `Failed`/`DraftReady` with a clear log message.
- Preview pane in `Confirming` renders three steps in order using
  redacted display.
- Pressing `Enter` in `Confirming` does **not** mutate anything; it logs
  the "execution not yet wired" message and stays in `Confirming`.
- Pressing `Esc` in `Confirming` returns to `DraftReady` and clears
  `execution_plan`.
- Stale-context detection works: editing `jj` history between context
  collection and confirmation triggers `Failed` with a refresh hint.
- Validation catches: empty head/base/title/body; invalid branch name
  shape; shell metacharacters in label/assignee/milestone.
- `just verify` passes.

## Verification Plan
Run `just verify`. Add focused unit tests for `validate_for_execution`,
`ExecutionPlan::from_draft` (both create and move branches of bookmark
logic), and the freshness comparison. Manual: in a jj workspace, generate
a draft, press `c`, confirm the preview lists the expected three commands,
press `Esc` to return to DraftReady; then re-enter Confirming, press
`Enter`, confirm the log shows the dead-end message and phase stays
`Confirming`.

## Files Likely Touched
- `src/generate.rs`
- `src/app.rs`
- `src/action.rs`
- `src/event.rs`
- `src/jj.rs`
- `src/ui.rs`

## Risks
- The dead-end `Enter` behavior must be obvious to the user so they do not
  think the app is broken. The log line + retained phase together should
  make it clear.
- Stale-context detection must not run a slow command synchronously; the
  async hop via `BackgroundEvent::StaleCheck` is mandatory.
- `ExecutionPlan` argv must match the execution ticket exactly; any drift
  produces a misleading preview. Reviewer must cross-check.
- Validation must not block on a transient empty title during draft
  retries; the check runs only on `c` press, not continuously.
