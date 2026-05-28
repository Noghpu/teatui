---
id: 0000l-2026-05-27-d6313f6e-landing-ux-polish
created_at: 2026-05-27T20:53:46+02:00
created_by_model: claude-sonnet-4-6/normal
state: reviewed
state_updated_at: 2026-05-28T08:02:27+02:00
---
# Landing UX Polish: Key Hint, Async Navigation, Status Bar Pruning

## Goal

Three small quality-of-life fixes on the landing screen and status bar:

1. Change the Generate PR key hint from `"Enter/g"` to `"g"`.
2. Allow navigating into Generate PR before repo discovery finishes â€” currently `inside_workspace` starts as `false` and blocks the entry until the background event lands.
3. Prune status bar keybind hints to non-obvious bindings only.

## Context

`src/ui.rs` drives all rendering. `src/app.rs` handles navigation guards. `src/repo.rs` owns `RepoState`.

**Key hint change**: `render_landing_actions` in `ui.rs:98` hardcodes `key: "Enter/g"` for the Generate PR action. The desired key is just `"g"` because `Enter` navigates to the selected item generically and doesn't need a separate callout.

**Async navigation block**: `RepoState::new()` (`repo.rs`) initialises `inside_workspace = false`. The background task `spawn_discovery` / `discover` runs async and eventually fires `BackgroundEvent::Repo`. In `app.rs`, `open_selected_landing_entry()` guards entry to Generate with:
```rust
0 if self.repo.inside_workspace => Screen::Generate,
0 => { self.log("Generate PR requires a jj workspace"); Screen::Landing }
```
This blocks navigation until the background event arrives. During that window the user sees "requires a jj workspace" even if they are in a workspace. Fix: add a `pub discovering: bool` field to `RepoState`, set to `true` in `RepoState::new()`, and cleared to `false` in `repo::discover()` before it returns the result. Then change the guard to:
```rust
0 if self.repo.inside_workspace || self.repo.discovering => Screen::Generate,
0 => { self.log("Generate PR requires a jj workspace"); Screen::Landing }
```
The existing `apply_repo` handler already kicks the user back to Landing if the confirmed result has `!inside_workspace`, so there is no need for additional logic.

**Status bar hint pruning**: `render_help` in `ui.rs` shows hints for every screen. The goal is to keep only bindings a user would not guess from standard TUI conventions.

Proposed pruning:
- **Landing**: Remove `â†‘/k up`, `â†“/j down` (standard vim nav), `Esc back` (standard). Keep only `q quit` and nothing else â€” `Enter` to open is also standard but may be kept for discoverability. Judgment call: keep `q quit`, remove the rest.
- **Generate (normal mode)**: Remove `â†‘/k up`, `â†“/j down`. Keep `h/l move focus`, `i edit`, `g generate`, `c confirm`, `p prompt`, `r refresh`, `Esc back`. The `Enter select/edit` hint is borderline â€” keep it since Enter's role varies by context.
- **Generate (editing mode)**: `Ctrl+S commit description` is non-obvious, keep. `Enter` and `Esc` behaviour for single-line vs description differs, keep those. Remove nothing additional.
- Other Generate sub-states (confirming, executing, etc.) are already minimal; leave unchanged.
- PullRequests/Issues: `â†‘/k up`, `â†“/j down` can go; keep `Enter select`, `c comment`, `Esc back`.

## Non-Goals

- Redesigning the status bar layout or adding new segments.
- Changes to keybinding logic â€” only the *hints* change.
- Changing how the actual navigation works, only the discovery guard.

## Design Decisions

- `RepoState.discovering` is `true` from construction until `discover()` returns. There is no intermediate "still discovering" state in the UI â€” the existing "pending" labels in `LlmStatus::Unknown` and `ToolStatus::Unknown` already communicate that.
- The entry guard lets the user into Generate optimistically. `apply_repo` will redirect back if workspace is absent â€” that's the correct single source of truth.
- Exact set of removed hints: arrow row removed from Landing; arrow row removed from Generate normal; arrow row removed from PullRequests/Issues. All others stay.

## Implementation Plan

1. `src/repo.rs`: Add `pub discovering: bool` to `RepoState`. Set it to `true` in `RepoState::new()`. In `repo::discover()`, before constructing the returned `RepoState`, set the field to `false` (i.e., it's `false` in the discovered state, `true` in the bootstrap state).
2. `src/app.rs` `open_selected_landing_entry`: Change the entry guard to `0 if self.repo.inside_workspace || self.repo.discovering`.
3. `src/ui.rs` `render_landing_actions`: Change `key: "Enter/g"` â†’ `key: "g"` for the Generate PR item.
4. `src/ui.rs` `render_help`:
   - Landing: remove `â†‘/k up`, `â†“/j down`, `Esc back` spans; keep `Enter open`, `q quit`.
   - Generate normal mode: remove `â†‘/k up`, `â†“/j down` spans.
   - PullRequests/Issues: remove `â†‘/k up`, `â†“/j down` spans.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/repo.rs", "src/app.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "RepoState.discovering field is true in new() and false in discover() return value",
    "open_selected_landing_entry guard uses || discovering",
    "apply_repo does NOT need changes â€” existing logic already redirects back to Landing",
    "Landing help bar: only Enter open and q quit remain",
    "Generate normal help bar: arrow hints removed, all others present",
    "PullRequests/Issues help bar: arrow hints removed"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria

- Launching the app and immediately pressing `g` navigates to Generate PR without "requires a jj workspace" error, even before discovery completes.
- After discovery completes and `inside_workspace = false`, pressing `g` / `Enter` on Generate PR shows the error message and stays on Landing.
- The Generate PR action row shows `g` as the key, not `Enter/g`.
- Landing help bar shows only `Enter open  q quit`.
- Generate normal mode help bar has no arrow/vim-nav row; all other hints are present.

## Verification Plan

- `just verify` passes.
- Visual check: run the app; the landing action rows show `g`.
- Visual check: help bar on Landing shows `Enter open  q quit` only.
- Visual check: help bar on Generate normal mode has no `â†‘/k` or `â†“/j` entries.

## Files Likely Touched

- `src/repo.rs` â€” add `discovering` field
- `src/app.rs` â€” update navigation guard
- `src/ui.rs` â€” key hint and help bar changes

## Risks

- Adding `discovering` to `RepoState` may require updating any pattern matches or struct literals in tests or other call sites. `RepoState` is constructed in `repo.rs`; search for other `RepoState { ... }` usages and add the field.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-28T07:57:08+02:00
- state: implemented

## Completed

All three items from the ticket were implemented:

1. **Key hint change**: Changed `key: "Enter/g"` to `key: "g"` for the Generate PR action in `render_landing_actions` in `src/ui.rs`.

2. **Async navigation guard**: Added `pub discovering: bool` field to `RepoState` in `src/repo.rs`. Set to `true` in `RepoState::new()` and `false` in the struct literal returned by `discover()`. Updated the guard in `open_selected_landing_entry` in `src/app.rs` to `0 if self.repo.inside_workspace || self.repo.discovering`.

3. **Status bar hint pruning**:
   - Landing: removed `â†‘/k up`, `â†“/j down`, `Esc back`; kept `Enter open`, `q quit`.
   - Generate normal mode: removed `â†‘/k up`, `â†“/j down`; all other hints kept.
   - PullRequests/Issues: removed `â†‘/k up`, `â†“/j down`; kept `Enter select`, `c comment`, `Esc back`.

## Deviations

None. Implementation follows the plan exactly. The `discovering: false` field was also added to all other `RepoState` struct literals across the codebase (in `src/bin/smoke-live.rs`, `src/generate.rs`, `src/prompt.rs`, and `tests/windows_pr_generation_integration.rs`).

## Verification

`just verify` passes: 66 unit tests + 4 integration tests all pass.

## Files Changed

- `src/repo.rs` â€” added `discovering` field to `RepoState`, set `true` in `new()`, `false` in `discover()` return value
- `src/app.rs` â€” updated navigation guard to `|| self.repo.discovering`
- `src/ui.rs` â€” key hint and help bar changes
- `src/bin/smoke-live.rs` â€” added `discovering: false` to two `RepoState` struct literals
- `src/generate.rs` â€” added `discovering: false` to test `RepoState` struct literal
- `src/prompt.rs` â€” added `discovering: false` to test `RepoState` struct literal
- `tests/windows_pr_generation_integration.rs` â€” added `discovering: false` to test `RepoState` struct literal

## Residual Risks

None identified. The `apply_repo` handler already redirects back to Landing if discovery completes with `!inside_workspace`, so the optimistic entry is safe.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-28T08:02:27+02:00
- state: reviewed

## Reviewer Metadata
- model: claude-opus-4-7
- reviewed_at: 2026-05-28

## Summary

Implementation is correct and minimal; matches plan exactly. All acceptance criteria met. `just verify` passes (66 unit tests + 4 integration tests, fmt clean, clippy clean with -D warnings). No code changes applied during review.

## Findings

- Verified `RepoState.discovering` is `true` in `RepoState::new()` (src/repo.rs:174) and `false` in the struct literal returned by `discover()` (src/repo.rs:262). Field is `pub` as required.
- Verified guard in `open_selected_landing_entry` (src/app.rs:370) now reads `0 if self.repo.inside_workspace || self.repo.discovering => Screen::Generate`.
- Verified `apply_repo` (src/app.rs:625) still redirects from Generate back to Landing when the resolved repo state has `!inside_workspace`, so optimistic entry remains safe.
- Verified Generate PR landing key hint is `"g"` (src/ui.rs:101).
- Verified `render_help` (src/ui.rs:650):
  - Landing: only `Enter open` and `q quit` remain.
  - Generate normal mode: arrow/vim-nav row gone; `h/l`, `Enter select/edit`, `i`, `g`, `c`, `p`, `r`, `Esc` all preserved.
  - PullRequests/Issues: arrow row gone; `Enter select`, `c comment`, `Esc back` preserved.
  - Editing, Confirm, Executing, Complete, Failed, Preview Generate sub-states untouched.
- Verified all other `RepoState { ... }` struct literals (src/bin/smoke-live.rs, src/generate.rs, src/prompt.rs, tests/windows_pr_generation_integration.rs) were updated to include `discovering: false`. No leftover construction sites missed.

## Notes / Observations

- `src/ui.rs` shows up in the diff as a near-full-file rewrite (2746 lines touched). On inspection this is a line-ending normalization: parent commit stored ui.rs as CRLF while every other Rust source in the tree is LF. The implementation rewrote it as LF, bringing the file in line with repository convention. There is no `.gitattributes` enforcing either style, so this normalization is incidental but improves consistency. Not flagged as a defect; logged for orchestrator visibility.
- No deviation from the plan. No simpler alternative identified â€” the optimistic-entry-plus-redirect pattern is the right shape given the existing `apply_repo` redirect.
- No hidden dependencies or sequencing issues.

## Verification

- `just verify` (fmt + clippy + tests): pass.
