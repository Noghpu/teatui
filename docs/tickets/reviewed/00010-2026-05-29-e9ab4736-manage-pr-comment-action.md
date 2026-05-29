---
id: 00010-2026-05-29-e9ab4736-manage-pr-comment-action
created_at: 2026-05-29T17:16:26+02:00
created_by_model: gpt-5.5/medium
state: reviewed
state_updated_at: 2026-05-29T21:41:00+02:00
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
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-29T21:35:51+02:00
- state: implemented

## What was completed

Implemented the Manage PR comment flow as specified in ticket 00010.

### Changes

**src/tea.rs**
- Added `TeaClient::pr_comment_command` that builds `tea comment <index> <body>` as an argv array
- Added `spawn_pr_comment` async helper that runs the command and sends `PrCommentResult` via the background channel
- Added unit test `builds_pr_comment_command_argv`

**src/event.rs**
- Added `PrCommentResult` enum with `Succeeded` and `Failed` variants
- Added `BackgroundEvent::PrComment(PrCommentResult)` variant

**src/action.rs**
- Added `OpenCommentModal`, `SubmitComment`, and `CancelComment` action variants

**src/app.rs**
- Added `PrCommentPhase` enum (Idle/Editing/Submitting/Failed)
- Added comment state fields to `PullRequestState`: `comment_phase`, `comment_buffer`, `comment_cursor`, `comment_error`
- Added editing methods: `open_comment_modal`, `close_comment_modal`, `comment_input_key` (handles Char/Backspace/Delete/Left/Right/Home/End)
- In `handle_key`: comment modal captures all keys when phase is Editing or Failed (global keys like q/j/k/g/r/c are blocked)
- Added `handle_comment_modal_key` routing Enterâ†’SubmitComment, Escâ†’CancelComment
- In `apply_edit_key`: routes EditKey to comment buffer when modal is active
- Added `open_comment_modal`, `cancel_comment`, `submit_comment`, `apply_pr_comment` methods
- `submit_comment` validates non-empty body before spawning command
- Failed submissions keep the buffer; successful submissions clear and close the modal
- `back()` closes comment modal (except while Submitting)
- Added 10 focused unit tests covering: key blocking, empty-body validation, failed-keeps-buffer, success-clears, cursor editing, Generate PR `c` regression check

**src/ui.rs**
- Added `render_pr_comment_modal` that shows a centered modal over the PR screen
- Modal displays PR title, single-line input with block-cursor rendering, status/error, and key hints
- `render_pull_request_work` shows compact submitting/failed state beneath selected PR info
- Help bar updated: shows `c comment` hint when PR is selected; comment editing and submitting hints for modal phases

### Deviations from plan
None significant. The comment input is manual (not ratatui-textarea) since the ticket calls for a "first version" single-line input with basic editing keys.

### Verification
- `just test` passes: 192 tests (10 new)
- Live comment submission was not run (no configured Gitea repo available in this environment)

### Residual risks
- `tea comment` accepts both issue and PR indexes. The app only exposes this from the PR viewer.
- Comment submission is mutating and explicit; no accidental submission on text entry.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-29T21:41:00+02:00
- state: reviewed

## Verdict
Acceptable with fixes. Implementation meets the acceptance criteria; review tightened modal key capture during submission and resolved clippy lints the implementer's `just test` run did not catch.

## What works (facts)
- `TeaClient::pr_comment_command` builds `["comment", "<index>", "<body>"]` as argv with no shell construction; `spawn_pr_comment` runs it via `capture` and emits a typed `PrCommentResult` on the background channel (`src/tea.rs`).
- `c` opens the modal only when `Screen::PullRequests`, a PR is selected, and `comment_phase == Idle`. The Generate `c â†’ ConfirmExecution` mapping is preserved because the Generate phase check returns earlier in `handle_key` (`generate_pr_c_confirm_behavior_not_regressed` test).
- `submit_comment` trims and rejects empty bodies before spawning, populating `comment_error`.
- `PrCommentResult::Failed` leaves the buffer untouched and flips phase to `Failed`; `PrCommentResult::Succeeded` calls `close_comment_modal` which clears buffer/cursor/error and returns to `Idle`.
- `render_pr_comment_modal` is centered over the PR screen with a block-cursor input, status/error line, and Enter/Esc hint.
- Help bar swaps between editing/submitting/normal variants per phase.
- No issue-comment or review/merge plumbing added.

## Issues fixed during review
- `handle_key` only routed keys to the modal during `Editing | Failed`. While `Submitting`, all global keys leaked: pressing `Esc` reached `back()`, which is gated against closing the modal during submit and so fell through and dumped the user back to Landing; `q` would quit, `j/k/h/l` would still move the PR list. Fixed by capturing every non-`Idle` phase and short-circuiting all keys during `Submitting` to `Action::Tick`. Added `comment_modal_swallows_keys_while_submitting` test.
- `cargo clippy --all-targets --all-features -- -D warnings` failed on the implementation:
  - `collapsible_match` on the `KeyCode::Backspace` and `KeyCode::Delete` arms of `comment_input_key` (the inner `if cursor > 0` / `if cursor < len` blocks). Collapsed into match guards.
  - `field_reassign_with_default` on the new `comment_buffer_editing_inserts_and_moves_cursor` test. Converted to struct-update syntax.
  - The implementer ran `just test` per the ticket's verification list and never ran `just verify`, so these were not caught upstream.

## Inferences / residual notes
- `comment_input_key` walks `char_indices().rev()` and `nth(1)` on each key for cursor movement / boundary lookups. That is O(n) per keystroke; acceptable for short comments and matches the ticket's "first version" scope.
- `tea comment` accepts both issue and PR indexes. The app only ever wires it from the PR viewer against the currently selected PR, matching the ticket's risk note.
- Submission relies on `tea` picking up the right login from cwd. No `--login` or repo-spec is passed, which matches the project's existing `tea` invocations.
- Live submission against a Gitea instance was not exercised â€” flagged here as in the implementation note.

## Verification
- `cargo fmt --check` clean.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo test --all-targets --all-features` â€” 193 lib tests (11 new for this slice) + 4 integration tests pass.
