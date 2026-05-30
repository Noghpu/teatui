---
id: 0000z-2026-05-29-fc60474d-manage-pr-list-filter-detail-ui
created_at: 2026-05-29T17:15:41+02:00
created_by_model: gpt-5.5/medium
state: reviewed
state_updated_at: 2026-05-29T21:18:30+02:00
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
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-05-29T17:37:28+02:00
- state: implemented

Completed:
- Wired Manage PRs to load open pull requests asynchronously through the typed tea command path.
- Replaced the placeholder PR list state with explicit PR viewer state, including filter edit buffer, load status/error, preview scroll, and stale request guarding.
- Rendered the PR list, work/filter pane, and detail preview from real PR data.
- Removed comment affordances from the PR screen and left issues without comment hints as well.

Deviations:
- Used the PR list payload as the detail source instead of adding a separate detail fetch, because the existing tea list fields already include the preview data needed for this slice.

Verification:
- Ran `just test`.
- Ran `just verify`.

Files changed:
- src/app.rs
- src/event.rs
- src/tea.rs
- src/ui.rs

Residual risks:
- PR preview rendering currently uses list payload fields only; if later tickets need comments or richer per-PR detail, the detail command path can be wired in then.
- The filter uses simple substring matching across summary fields, which is intentionally lightweight and may be broadened later.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-29T21:18:30+02:00
- state: reviewed

## Verdict
Acceptable. Implementation meets all acceptance criteria. Applied one small render-path fix and kept the slice intact.

## What works (facts)
- Explicit `PullRequestState` replaces the placeholder `ListState` and carries items, filter buffer, load status/error, preview scroll, and a stale-result guard via `next_request_id`/`active_request_id` (`src/app.rs`).
- `complete_load`/`fail_request` reject results whose `request_id` doesn't match the active request â€” confirmed by `stale_pr_list_result_is_ignored` test.
- Entering Manage PRs from Landing and pressing `r` while in Manage PRs both call `spawn_pull_requests_load`; in-flight loads are skipped unless `force_refresh=true` (`src/app.rs:925`).
- Filter editing routes through `InputMode::Editing` with PR-specific `apply_edit_key` and `finish_editing_pull_request_filter`; printable keys like `g`/`q`/`c` reach the field and do not fire global actions (`pr_filter_edit_mode_routes_printable_keys_into_the_filter` test).
- `visible_items` filters from the in-flight buffer so filtering is live; `clamp_selection` runs on every keystroke and on commit (`pr_filter_clamps_selection_when_filter_changes` test).
- PR list rows show `#index title` plus state/author/headâ†’base/updated/labels, not action placeholders. Work pane surfaces filter, status counters, selected metadata, and load errors. Preview shows full PR detail and body.
- Issues placeholder menu and help no longer hint at comments either.
- Background plumbing is via `BackgroundEvent::PullRequests(PullRequestsResult)` with a typed parser path (`src/tea.rs`, `src/pull_requests.rs`); no shell-string or table parsing.

## Issues fixed during review
- `render_preview` for `Screen::PullRequests` was reading `state.preview_scroll.offset` and clamping only into the local `scroll_offset`, never writing back. Combined with `ScrollState::scroll_down` using `saturating_add`, repeated scrolling could grow `offset` unbounded, and switching to a smaller PR detail would leave a stale large offset requiring many `k` presses to recover. Fixed to mirror the Issues pane: clamp through `app.pull_requests_mut().preview_scroll.clamp(...)` before rendering. Adjusted `render_pull_request_preview` to return `Vec<Line<'static>>` (all line strings were already owned via `format!`, with one `as_deref()` site converted via `to_string()`).

## Deviations from plan (acceptable)
- Implementer used the `tea pr list` payload as the detail source instead of issuing a separate `tea pr <index>` fetch. The ticket says the list fields already cover title/state/author/head/base/url/body/updated/labels â€” confirmed by `PR_FIELDS` in `src/tea.rs` â€” so this is in-scope for this slice. `pr_detail_command` is implemented for future use.

## Inferences / residual notes
- Filter input goes through `TextFieldState::input`, which uses a `TextArea` and accepts Tab/BackTab. For a single-line filter, Tab insertion is cosmetic only and harmless; Enter is intercepted by `apply_edit_key` before reaching the field, so commit semantics are correct.
- When a load fails after a previous success, prior items are retained via `fail_load`; the error message surfaces in the work pane and as a `load error` row in the preview.

## Verification
- `cargo fmt --check` clean.
- `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo test --all-targets --all-features` â€” 181 lib tests + 4 integration tests pass.
- Live `tea` not exercised; documented as not run.
