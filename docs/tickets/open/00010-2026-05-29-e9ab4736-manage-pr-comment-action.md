---
id: 00010-2026-05-29-e9ab4736-manage-pr-comment-action
created_at: 2026-05-29T17:16:26+02:00
created_by_model: gpt-5.5/medium
state: open
---
# Manage PR Comment Action

## Goal
Add the minimal Manage PRs comment flow: open a short comment input for the selected PR, submit it through a typed noninteractive `tea comment` command, and surface success/failure without adding broader PR-management features.

## Context
This builds on the PR list/filter/detail UI ticket. The design includes one mutating action for Manage PRs: add a plain text comment. It explicitly excludes review approvals, line comments, merge operations, checks management, reviewer assignment, and the full `tea` TUI.

The installed `tea` 0.14.0 help reports:

```text
tea comment [options] <issue / pr index> [<comment body>]
```

For this app, the command must be constructed as argv, with the comment body supplied as a positional argument so `tea` does not open an editor or prompt. The body is user-authored text from the TUI, not model output.

## Non-Goals
- Do not implement issue comments.
- Do not show, fetch, or paginate existing PR comments.
- Do not implement review approvals, review comments, merges, close/reopen, checkout, checks, or reviewer assignment.
- Do not introduce a generic modal/window stack or `rat-dialog`.
- Do not add confirmation flows for branch/push/PR creation; this is unrelated to Generate PR execution.

## Design Decisions
- Add `TeaClient::pr_comment_command` or equivalently named builder in `src/tea.rs` that returns `tea comment <index> <body>` as an `ExternalCommand` argv array.
- The app must validate that a selected PR exists and the trimmed comment body is non-empty before spawning the command.
- Use the existing job registry/log pattern for the async command. If needed, expose a narrow command helper from `src/command.rs` that runs one command as a job and returns its `JobResult`, rather than duplicating queued/running/succeeded event code in `App`.
- Pressing `c` in Manage PRs normal mode opens a small centered comment input modal for the currently selected PR.
- First version comment input is single-line and short: printable characters insert, Backspace/Delete/Home/End/Left/Right edit, `Enter` submits, and `Esc` cancels.
- While the comment modal is open, global keybindings are inactive. Typing `q`, `j`, `k`, `g`, `r`, or `c` inserts text or is handled as an editing key; it must not quit, navigate, generate, refresh, or recursively open another comment modal.
- During submission, keep the selected PR visible and show a compact running state. On success, clear the buffer and close the modal. On failure, keep the buffer available for retry and surface the error in the modal/work pane plus logs.
- Do not advertise comment help in Manage PRs unless a PR is selected and the comment flow is wired.

## Implementation Plan
1. Add the `tea comment` command builder and argv unit test in `src/tea.rs`.
2. Add comment state to the explicit PR viewer state from the previous ticket: idle/editing/submitting/failed is sufficient.
3. Extend `Action`/key handling only as needed so `c` opens the modal in `Screen::PullRequests`, while Generate PR's existing `c` confirmation behavior remains unchanged.
4. Add edit-key handling for PR comment input that is separate from Generate form editing and PR filter editing.
5. Spawn the comment command asynchronously through the existing command/job boundary; add a `BackgroundEvent` result for completion if needed to update PR comment state.
6. Render a centered comment modal over the Manage PRs screen and update the help bar/status only for wired states.
7. Add focused tests for command argv, comment modal key capture, empty-comment blocking, and failure retaining the typed body.
8. Run formatting and verification.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/app.rs", "src/ui.rs", "src/event.rs", "src/tea.rs", "src/command.rs", "src/pull_requests.rs"],
  "likely_files": ["src/tea.rs", "src/command.rs", "src/event.rs", "src/app.rs", "src/ui.rs", "src/pull_requests.rs"],
  "verification_commands": ["just test"],
  "review_focus": [
    "Comment submission uses a typed argv command and cannot open an editor or interactive tea UI.",
    "Comment edit mode captures global keys instead of leaking navigation/quit/refresh/generate actions.",
    "Generate PR c/confirm behavior is not regressed.",
    "Failed comment submissions keep the user's typed body for retry."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- Manage PRs exposes a working `c` comment action only when a PR is selected.
- The comment body is user-entered, trimmed for validation, and submitted through `tea comment <index> <body>` as argv without shell construction.
- Empty comments do not spawn a command and show a concise validation error.
- While the comment input is active, global keybindings are inactive.
- Successful submissions close and clear the input; failed submissions keep the typed body and surface the failure.
- Existing Generate PR confirmation/retry key behavior remains intact.
- No issue-comment or review/merge functionality is added.

## Verification Plan
- Run `just test`.
- Include focused unit tests for command argv and app key-state transitions.
- If a configured Gitea repo is available, manually submit a short comment to a harmless PR; otherwise note that live comment submission was not run.

## Files Likely Touched
- `src/tea.rs`
- `src/command.rs`
- `src/event.rs`
- `src/app.rs`
- `src/ui.rs`
- `src/pull_requests.rs`

## Risks
- `tea comment` accepts both issue and PR indexes. The app should present this only from the PR viewer against the selected PR index.
- Comment submission is mutating. Keep the action explicit, user-typed, and visible; do not submit on mere text entry.
- Reusing global `InputMode::Editing` can regress Generate PR unless PR edit targets are handled explicitly.
