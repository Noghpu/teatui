---
id: 00011-2026-05-29-b73fe9f6-manage-pr-open-and-yank-url
created_at: 2026-05-29T21:55:31+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-29T22:06:57+02:00
---
# Manage PRs Open-In-Browser and Yank-URL Shortcuts

## Goal
Add two read-only shortcuts to the Manage PRs viewer: `o` opens the selected PR's URL in the system browser, and `y` copies that URL to the system clipboard. Both surface a short confirmation in the status bar and log so the user has visible feedback.

## Context
The Manage PRs screen (`Screen::PullRequests`) already exposes a list of `PullRequestSummary` records with a `url` field populated by the `tea` JSON parsers (`src/pull_requests.rs`). Selection state and a comment modal flow already exist in `PullRequestState` (`src/app.rs`). Help-bar text is rendered per-phase in `src/ui.rs`.

The keys are vim-flavored: `y` mirrors yank, `o` is a single-letter "open". Both must be inert outside the Manage PRs normal mode so they cannot conflict with form editing, filter editing, or the comment modal.

The repo has no clipboard or browser-launcher dependency yet. Add `arboard` (clipboard) and `opener` (browser launch) as new crate dependencies. Both are cross-platform and widely used.

## Non-Goals
- Do not add a `g`-prefix sequence (`gx` etc.) or any multi-key vim chord.
- Do not add open/copy actions for any field other than the PR URL (no copying titles, branches, bodies, etc.).
- Do not add a confirmation modal â€” feedback is a single status-bar/log line.
- Do not change the Generate screen or the comment modal key surface.
- Do not add tests that actually open a browser or touch the real clipboard.

## Design Decisions
- Add two new `Action` variants: `OpenPrInBrowser` and `CopyPrUrl`. Both are dispatched only when `screen == Screen::PullRequests`, `input_mode == InputMode::Normal`, the comment modal is `Idle`, and a PR is currently selected with a non-empty `url`.
- Keybindings live in `App::handle_key` next to the existing `c`/`r`/`g` PR-screen shortcuts. The matches use `KeyCode::Char('o')` and `KeyCode::Char('y')` with `KeyModifiers::empty()`. Uppercase `O`/`Y` are not bound.
- Browser launch goes through a thin wrapper around `opener::open(&url)`. Add a free function `pub fn open_in_browser(url: &str) -> Result<(), String>` in a new small module `src/external.rs`.
- Clipboard write uses `arboard::Clipboard::new()?.set_text(url.to_string())`. Wrap in `pub fn copy_to_clipboard(text: &str) -> Result<(), String>` in the same `src/external.rs` module. Both wrappers map underlying errors to short user-facing strings.
- Both actions run synchronously on the UI thread. `opener::open` returns immediately after spawning the platform handler; `arboard` writes are also effectively instant. No job runner integration is needed.
- Success path: log `opened <url> in browser` / `copied <url> to clipboard` and set a transient status-bar message field on `App`. If a transient status field does not yet exist, add `App::status_message: Option<String>` (cleared on the next key press, navigation, or background event that updates UI state â€” keep the clearing rule simple). Inspect the existing status-bar implementation in `src/ui.rs`; if it already supports an ephemeral message, reuse that path instead.
- Failure path: log the error and surface it in the same status-bar slot prefixed with `error:`.
- Help-bar additions: when on the Manage PRs screen with a PR selected and no modal open, include `o open` and `y yank url` hints alongside the existing `c comment` hint.

## Implementation Plan
1. Add `arboard` and `opener` to `[dependencies]` in `Cargo.toml`. Run `cargo check` to confirm they build on Windows.
2. Create `src/external.rs` exposing `open_in_browser(url: &str) -> Result<(), String>` and `copy_to_clipboard(text: &str) -> Result<(), String>`. Register the module in `src/lib.rs`.
3. Add `Action::OpenPrInBrowser` and `Action::CopyPrUrl` variants in `src/action.rs`.
4. In `src/app.rs::handle_key`, after the existing PR-screen `c` shortcut block, add `o` and `y` shortcuts gated by the same selection + idle-modal conditions. Return the new actions.
5. In `App::update`, route the new actions to small methods `open_selected_pr_in_browser` and `copy_selected_pr_url`. Each fetches the selected PR's URL, calls the `external` helper, and writes a status-bar/log message.
6. Inspect the current status-bar render path in `src/ui.rs`. If an ephemeral message slot exists, reuse it; otherwise add `App::status_message: Option<String>` and render it in the status bar. Clear on the next non-tick key action.
7. Extend the Manage PRs help-bar text to include `o open` and `y yank url` when a PR is selected and the comment modal is `Idle`.
8. Unit tests in `src/app.rs`:
   - `o_in_pr_view_emits_open_action_when_pr_selected`
   - `y_in_pr_view_emits_copy_action_when_pr_selected`
   - `o_and_y_are_inert_with_no_selection`
   - `o_and_y_are_inert_while_comment_modal_open`
   - `o_and_y_are_inert_in_filter_edit_mode`
   - `open_action_with_empty_url_logs_error_and_does_not_call_helper` (use the action handler, not the helper itself â€” verify the URL-empty guard via observable state like status_message/log)
   Do not call `open_in_browser` or `copy_to_clipboard` directly in tests; gate them behind action dispatch and assert on side effects (log entries / status message) without actually touching the OS.
9. Run `just verify`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/app.rs", "src/action.rs", "src/ui.rs", "src/pull_requests.rs", "Cargo.toml"],
  "likely_files": ["Cargo.toml", "src/lib.rs", "src/external.rs", "src/action.rs", "src/app.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "o and y are inert outside Manage PRs normal mode (no leakage into filter edit, comment modal, Generate screen, or Landing).",
    "Browser launch and clipboard write go through the external module wrappers; tests do not touch the real OS clipboard or spawn a browser.",
    "Status-bar/log feedback is visible on both success and failure paths.",
    "Empty PR URL is handled defensively without panic or shelling out an empty string.",
    "Help-bar hints appear only when a PR is selected and the comment modal is Idle."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- Pressing `o` on the Manage PRs screen with a PR selected opens that PR's URL in the system browser and logs/surfaces a confirmation message.
- Pressing `y` on the Manage PRs screen with a PR selected copies that PR's URL to the system clipboard and logs/surfaces a confirmation message.
- Both keys do nothing (and produce no log spam) when no PR is selected, when the comment modal is open, when the filter is being edited, or on any non-Manage-PRs screen.
- Failures from `opener` or `arboard` are caught and surfaced as a single status-bar/log error line, not propagated as panics.
- The Manage PRs help bar advertises `o` and `y` only when a PR is selected and no modal is open.
- `just verify` passes (fmt, check, clippy -D warnings, tests).

## Verification Plan
- `just verify`.
- Manual smoke on Windows: select a PR in Manage PRs, press `o` (browser opens to the URL), press `y` (paste into another app yields the URL). Confirm the status bar shows the confirmation line for both.
- Manual smoke with no PR selected: confirm `o` and `y` are no-ops.
- Manual smoke while the comment modal is open: confirm `o` and `y` insert into the comment buffer instead of triggering the new actions.

## Files Likely Touched
- `Cargo.toml`
- `src/lib.rs`
- `src/external.rs` (new)
- `src/action.rs`
- `src/app.rs`
- `src/ui.rs`

## Risks
- `arboard` on Linux requires an X11/Wayland display; the dev machine is Windows so this is acceptable, but document the caveat in the wrapper if it surfaces in CI.
- `opener::open` on Windows uses `ShellExecute`; URLs containing characters that need escaping should already be safe because `opener` does its own quoting. Avoid hand-building a `cmd /c start` string.
- Adding two new crates increases compile time; both are small but worth noting.
- If a status-bar ephemeral message slot does not yet exist, adding one touches `App` state shared with many flows â€” keep the addition minimal and clear the message on the next key event to avoid stale text bleeding across screens.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-29T22:04:11+02:00
- state: implemented

## What was completed

Implemented `o` (open in browser) and `y` (yank/copy URL) shortcuts for the Manage PRs screen.

- Added `arboard = "3"` and `opener = "0.7"` to `Cargo.toml`.
- Created `src/external.rs` with `open_in_browser(url: &str) -> Result<(), String>` and `copy_to_clipboard(text: &str) -> Result<(), String>` wrappers.
- Added `Action::OpenPrInBrowser` and `Action::CopyPrUrl` variants to `src/action.rs`.
- Registered `pub mod external` in `src/lib.rs` (alphabetical order).
- Added `status_message: Option<String>` field to `App` struct.
- In `App::handle_key`, added `o` and `y` shortcuts gated by `screen == PullRequests`, `input_mode == Normal`, `comment_phase == Idle`, and a selected PR.
- In `App::update`, cleared `status_message` on every action call, then dispatched new actions to `open_selected_pr_in_browser` and `copy_selected_pr_url` methods.
- Both action handlers guard against empty URLs and surface errors to `status_message` and log.
- Added `App::status_message()` public accessor.
- Updated `render_status` in `src/ui.rs` to show `status_message` in the PR screen status bar (green for success, red for errors).
- Updated `render_help` in `src/ui.rs` to include `o open` and `y yank url` hints when a PR is selected and no modal is active.
- Added 8 unit tests covering all required scenarios.

## Deviations from plan

None. The plan was followed exactly. The `status_message` field was added as specified since no existing ephemeral message slot existed.

## Verification

`just verify` passed: fmt, check, clippy -D warnings, 201 tests (all passing), integration tests.

## Important files changed

- `Cargo.toml`
- `src/lib.rs`
- `src/external.rs` (new)
- `src/action.rs`
- `src/app.rs`
- `src/ui.rs`

## Residual risks / follow-up

- `arboard` on Linux requires an X11/Wayland display; this is documented in the wrapper comment. CI is Windows so this is acceptable.
- The `status_message` field is currently only rendered on the PullRequests screen status bar; if other screens need transient messages in the future, `render_status` will need extending.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7/high
- reviewed_at: 2026-05-29T22:06:57+02:00
- state: reviewed

## Verdict
Accept as implemented. All acceptance criteria are satisfied and `just verify` passes (fmt, check, clippy `-D warnings`, 201 lib tests + 4 integration tests, all green). No code changes made during review.

## What works (facts)
- `src/external.rs` wraps `opener::open` and `arboard::Clipboard` behind `Result<(), String>` helpers, mapping errors to short user-facing strings â€” exactly as scoped.
- `Action::OpenPrInBrowser` and `Action::CopyPrUrl` are added to `src/action.rs`.
- Key dispatch (`App::handle_key`, `src/app.rs:536-554`) gates `o` and `y` on `screen == PullRequests`, `input_mode == Normal`, `comment_phase == Idle`, and `selected_item().is_some()`. The earlier short-circuit for `comment_phase != Idle` at the top of `handle_key` plus the `InputMode::Editing` branch above ensure leakage into the comment modal or the filter editor is impossible â€” both verified by `o_and_y_are_inert_while_comment_modal_open` and `o_and_y_are_inert_in_filter_edit_mode`.
- Both action handlers (`open_selected_pr_in_browser`, `copy_selected_pr_url`) trim the URL and short-circuit on empty with a logged + status-bar `error:` message before calling the external helper, so the OS is never invoked with an empty string. Whitespace-only URLs are covered by the trim and the `copy_action_with_empty_url_logs_error_and_does_not_call_helper` test.
- `App::status_message: Option<String>` is added and cleared at the top of every `App::update` invocation, so messages persist exactly until the next user key event â€” appropriate since `AppEvent::Tick` does not call `update`.
- `render_status` shows the message in green (`colors::GOOD`) for success and red (`colors::BAD`) for any `error:`-prefixed message, in the PR-screen status bar.
- Help bar (`src/ui.rs:1461-1478`) advertises `o open` / `y yank url` only when on `Screen::PullRequests` with a PR selected and no comment-modal phase active. The earlier match arms for `Editing`/`Failed`/`Submitting`/`InputMode::Editing` correctly take precedence, so the hint disappears in those states.
- Eight new focused unit tests cover the action dispatch matrix without touching the OS clipboard or spawning a browser.

## Issues fixed during review
None. No fixes required.

## Inferences / residual notes
- The inline comment above the `self.status_message = None` line in `App::update` says "on each non-Tick user action", but the clear runs unconditionally including for `Action::Tick`. This is harmless because `AppEvent::Tick` is consumed at the run-loop without dispatching `update`, so the only way `Action::Tick` reaches `update` is via an unhandled key event â€” clearing the message on an unhandled keypress is reasonable behavior. Comment is mildly inaccurate but does not warrant a code change for this slice.
- `status_message` is only rendered on the Manage PRs status bar today. That matches the slice scope; future screens that want transient feedback will need to extend `render_status` accordingly. Already flagged in the implementation note.
- `arboard` on Linux requires X11/Wayland; documented in the wrapper. CI/dev is Windows, so this is acceptable.
- Live smoke (real browser launch + real clipboard paste) was not run in the review environment; relying on `opener` and `arboard`'s upstream behavior. The narrow surface (a `Result` propagation) makes this low-risk.

## Verification
- `just verify` â€” clean: fmt OK, `cargo check` OK, `cargo clippy --all-targets --all-features -- -D warnings` OK, 201 lib tests pass, 4 integration tests pass.
