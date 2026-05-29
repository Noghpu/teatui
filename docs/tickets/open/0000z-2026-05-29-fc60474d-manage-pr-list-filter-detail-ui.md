---
id: 0000z-2026-05-29-fc60474d-manage-pr-list-filter-detail-ui
created_at: 2026-05-29T17:15:41+02:00
created_by_model: gpt-5.5/medium
state: open
---
# Manage PR List Filter And Detail Preview

## Goal
Wire the Manage PRs screen to load open pull requests through the typed `tea` PR data path, show a real filtered PR list, and render the selected PR detail preview.

## Context
This builds on the Manage PR data command/parser ticket. The current app already has `Screen::PullRequests`, a placeholder `ListState`, placeholder menu/work/preview rendering in `src/ui.rs`, and global input handling in `src/app.rs`. The design says Manage PRs is shallow supporting context: list open PRs, filter by simple text search, open a detail preview, and add a short comment later.

The PR-generation cleanup work established patterns this ticket must preserve:

- Treat the left pane as an actual list pane, not a placeholder command menu.
- Keep scroll state pane-scoped and clamp it from render/update paths.
- Keep text input handling mode-specific: while editing a filter, printable keys such as `g`, `c`, `q`, `j`, and `k` must edit the filter and must not trigger global actions.
- Use typed command wrappers and `BackgroundEvent` plumbing instead of blocking UI rendering.
- Avoid visible dead affordances. Do not advertise comments until the comment ticket implements them.

## Non-Goals
- Do not implement PR comments in this ticket.
- Do not implement issue viewer behavior.
- Do not add approvals, reviews, merge operations, checks, checkout, or line comments.
- Do not introduce a full modal/window stack or `rat-dialog`.
- Do not use shell command strings or table-output parsing.

## Design Decisions
- Replace or specialize the generic `ListState` for PRs with explicit PR viewer state, expected name `PullRequestState`, containing at minimum: loaded PR items, selected item index, filter text/edit buffer, load status/error, preview scroll, and a request id or equivalent stale-result guard.
- `Screen::PullRequests` left pane should render the filtered PR list directly. Rows should be compact and scannable: PR index/title primary, plus state/author/head/base or updated metadata when available.
- The center work pane should show the filter field, loading/error/empty state, selected count, and enough selected-row metadata to orient the user. It should not be a marketing or placeholder description pane.
- The right preview pane should show selected PR title, author, state, source branch, target branch, URL when present, labels when present, updated value when present, and body text. Empty/missing body should be explicit but compact.
- Entering Manage PRs from Landing should trigger an async open-PR load unless one is already in flight for the current request. Pressing `r` while in Manage PRs should refresh PRs. Existing global refresh for Landing/Generate should keep working.
- `j`/`k` or arrows in the PR list focus move the selected filtered PR. `h`/`l`/Tab move between list, filter/work, and preview using existing `Focus` values.
- `i` or `Enter` on the center/work pane starts filter editing. `Esc` cancels filter editing back to the previous committed filter; `Enter` commits it. Backspace/delete/cursor movement should behave predictably for a simple single-line field.
- While editing the filter, global keybindings are inactive.
- If a load fails, keep the previous successful list if present and surface the failure in the work pane plus logs.
- If filtering removes the current selected item, clamp selection to the filtered list.

## Implementation Plan
1. Add PR load result/event plumbing using `BackgroundEvent`, an async spawn helper, and the parser/command builder from the PR data ticket.
2. Replace `pull_requests: ListState` with explicit PR viewer state and update accessors/tests accordingly.
3. Trigger PR loading from `open_selected_landing_entry` when opening `Screen::PullRequests`, and from `refresh` when the active screen is `PullRequests`.
4. Extend key handling/update logic so PR filter editing uses `InputMode::Editing` without leaking global keybindings.
5. Update PR navigation to move through the filtered PR list when focus is on the left pane, and to scroll preview only when focus is `Focus::Preview`.
6. Replace placeholder `Screen::PullRequests` render branches in `render_menu`, `render_work`, `render_preview`, `render_status`/`render_help` as needed.
7. Add focused tests for filter behavior, stale load result handling if request ids are used, and navigation clamping. Avoid broad snapshot farms.
8. Run formatting and verification.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/app.rs", "src/ui.rs", "src/event.rs", "src/tea.rs", "src/pull_requests.rs", "src/command.rs"],
  "likely_files": ["src/app.rs", "src/ui.rs", "src/event.rs", "src/pull_requests.rs"],
  "verification_commands": ["just test"],
  "review_focus": [
    "PR viewer uses real loaded data rather than placeholder Open items/Filter/Comment rows.",
    "Filter edit mode does not leak global keybindings.",
    "Background PR loads cannot block rendering and stale/failed results are handled predictably.",
    "Comments are not advertised or half-wired in this slice."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- Opening Manage PRs from Landing starts an async open-PR load using typed `tea` commands.
- The left pane shows actual filtered PR rows or clear loading/empty/error rows, not placeholder action names.
- The center pane exposes a usable filter field and compact selected/list status.
- The preview pane renders selected PR details and body from loaded data.
- `r` refreshes the PR list while in Manage PRs without breaking existing repo/revset refresh behavior elsewhere.
- `j`/`k` navigation, focus movement, and preview scrolling are pane-scoped for Manage PRs.
- Editing the PR filter captures printable/navigation keys as filter input/editing keys and does not trigger global app actions.
- No comment UI or command is presented until the later comment ticket.

## Verification Plan
- Run `just test`.
- Add focused unit tests in `src/app.rs` or adjacent modules for PR filter editing and selection clamping.
- Manually run the TUI if a configured Gitea repo is available; otherwise note that live `tea` verification was not run.

## Files Likely Touched
- `src/app.rs`
- `src/event.rs`
- `src/ui.rs`
- `src/pull_requests.rs`

## Risks
- Key handling can regress Generate PR editing if `InputMode::Editing` is generalized carelessly. Keep Generate and PR edit paths explicit.
- PR list loads can race with refresh; use a small request id or equivalent guard if results can arrive out of order.
- `tea` auth or repo discovery failures are normal. Surface them without clearing usable previous data.
