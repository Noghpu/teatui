---
id: 00018-2026-05-30-ec767766-landing-quit-selectable
created_at: 2026-05-30T11:11:10+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-30T11:22:26+02:00
---
# Make Quit a selectable landing entry

## Goal
Allow the user to navigate to a "Quit" entry on the Landing screen with `j`/`k`/arrow keys and confirm it with `Enter`. The existing `q` global shortcut continues to work unchanged; this ticket just makes Quit reachable through the same selection model as the three modes.

## Context
On the Landing screen the user can move a `â–¶` cursor up and down through `Generate PR`, `Manage PRs`, `Manage Issues` â€” but the `Quit` row immediately below them is rendered as a static line, so the cursor visibly stops at `Manage Issues`. This is an obvious UX papercut: a discoverable selection list that silently excludes its last visible entry.

The current cap lives in `src/app.rs`:
- `(Screen::Landing, _, Direction::Down) => self.landing.selected_entry = (self.landing.selected_entry + 1).min(2);`
- `fn open_selected_landing_entry(&mut self)` only matches indices `0`, `1`, and `_` (treating `_` as `Issues`).

The static Quit row is rendered in `src/ui.rs` by `render_landing_actions` â€” it builds three selectable rows from an `actions` array and appends a fourth row with `landing_action_line("â—†", "Quit", "q", false, â€¦)` outside the loop.

## Non-Goals
- No change to the `q` global shortcut. It must continue to quit from anywhere the existing keymap honors it.
- No change to the cap on the secondary Landing menu list rendered by `render_menu` (the `selectable_list(&["Generate PR", "Manage PRs", "Manage Issues"], â€¦)` arm). That menu mirrors the modes only â€” Quit is a hero-screen affordance.
- No reorganization of Landing layout, spacing, or visual style beyond what is required to highlight the Quit row when selected.

## Design Decisions
- Treat Quit as a fourth Landing entry. Bump the down-cap in `navigate` from `.min(2)` to `.min(3)` and add an explicit `3 => â€¦` arm to `open_selected_landing_entry`.
- `open_selected_landing_entry` cannot directly set `self.should_quit` because it returns a `Screen`. Refactor it so the Quit case sets `self.should_quit = true` (and leaves `screen` unchanged) rather than mapping to a screen. The cleanest shape is to convert `open_selected_landing_entry` from "compute a Screen and assign" into an early-return match: `0 => self.enter_generate(); 1 => self.enter_pull_requests(); 2 => self.enter_issues(); 3 => self.should_quit = true; _ => {}`. Extracting the three existing screen-entry side effects into small helpers keeps each arm a single intent.
- In `src/ui.rs`, fold the Quit row into the same `actions` array so the loop renders it (and selection highlight) uniformly. Drop the `lines.push(landing_action_line("â—†", "Quit", "q", false, â€¦))` tail.
- Keep the test bias of the existing suite: small, behavior-focused unit tests on `App`.

## Implementation Plan
1. In `src/ui.rs::render_landing_actions`, add a fourth entry to the `actions` array (`label: "Quit"`, `key: "q"`). Remove the now-unused trailing `lines.push(landing_action_line("â—†", "Quit", ...))` call. Keep the blank spacer behavior â€” the loop already pushes `Line::from("")` between items; the extra blank line after the loop is no longer required and should be removed if it leaves trailing whitespace, otherwise leave it.
2. In `src/app.rs::navigate`, change `(Screen::Landing, _, Direction::Down)` to cap at `.min(3)`.
3. Refactor `src/app.rs::open_selected_landing_entry`:
   - Match on `self.landing.selected_entry`.
   - Index `0`: keep the existing Generate PR entry logic (workspace gating stays exactly as it is â€” that is the next ticket's concern).
   - Index `1`: enter `Screen::PullRequests` and `spawn_pull_requests_load(false)`.
   - Index `2`: enter `Screen::Issues`.
   - Index `3`: set `self.should_quit = true`; do not touch `screen`/`focus`/`input_mode`.
   - Default (`_`): no-op.
4. Add `#[test]` cases in `src/app.rs::tests` mirroring existing landing tests:
   - `landing_cursor_down_can_reach_quit`: starting at 0, press Down three times, expect `landing.selected_entry == 3`.
   - `landing_cursor_down_does_not_overflow_past_quit`: starting at 3, press Down once, expect still `3`.
   - `landing_enter_on_quit_sets_should_quit`: with `selected_entry = 3`, call `open_selected_landing_entry()`, expect `app.should_quit() == true` (use the existing pattern; expose a getter only if necessary â€” the existing tests already touch private state, so direct access is fine).
   - `landing_enter_on_quit_does_not_change_screen`: with `selected_entry = 3`, call `open_selected_landing_entry()`, expect `app.screen() == Screen::Landing`.
5. Update the help/keybinding line if it references "three modes" â€” search `src/ui.rs` for the Landing help text and adjust copy only if it is incorrect after the change. Do not invent new help text.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/app.rs", "src/ui.rs"],
  "likely_files": ["src/app.rs", "src/ui.rs"],
  "verification_commands": ["just fmt", "just check", "just clippy", "just test", "just verify"],
  "review_focus": [
    "Quit is reachable with Down arrow and selectable with Enter on Landing.",
    "Global `q` keybind still quits from Landing and other screens.",
    "Selection cap is exactly 3; pressing Down at Quit does not overflow.",
    "open_selected_landing_entry no longer relies on the `_` fallthrough to map Issues; explicit indices keep the intent obvious.",
    "Landing render_actions loop emits one selectable row per entry â€” no orphaned static Quit row."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- From Landing with `selected_entry = 0`, pressing Down three times moves the highlight to the Quit row.
- Pressing Enter while Quit is highlighted sets `should_quit` and exits the app on the next loop iteration.
- Pressing Up on Quit returns to Manage Issues.
- The `q` global shortcut still quits from Landing.
- No regression to entering Generate PR, Manage PRs, Manage Issues from their respective rows.
- The Quit row visibly shows the same `â–¶` highlight + ACCENT styling as the other rows when selected.

## Verification Plan
- `just verify`.
- Manual smoke: launch the app, press Down past Manage Issues, confirm Quit highlights, press Enter, confirm the app exits cleanly without a panic and the terminal is restored.

## Files Likely Touched
- `src/app.rs`
- `src/ui.rs`

## Risks
- The existing `_` fallthrough in `open_selected_landing_entry` masked any future entries. Switching to explicit indices is safer but could surface a missed call site â€” grep for `landing.selected_entry` to confirm no other code assumes the cap is 2.
- The Quit row currently appears with an empty blank-line spacer after it; folding it into the loop will add a spacer after Quit too. That is visually fine but worth a quick eyeball during manual smoke.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-30T11:18:43+02:00
- state: implemented

## What was completed

Ticket 00018 implemented in full: Quit is now a fourth selectable entry on the Landing screen, reachable with Down arrow and confirmable with Enter.

## Changes

**src/app.rs**
- `navigate`: changed `.min(2)` to `.min(3)` so Down can reach index 3 (Quit).
- `open_selected_landing_entry`: refactored from a `self.screen = match ...` assignment into early-return style with explicit indices 0/1/2/3. The `_` fallthrough is gone. Index 3 sets `self.should_quit = true` without touching screen/focus/input_mode.
- Extracted three helpers `enter_generate()`, `enter_pull_requests()`, `enter_issues()` so each match arm is a single intent call.
- Added four new tests: `landing_cursor_down_can_reach_quit`, `landing_cursor_down_does_not_overflow_past_quit`, `landing_enter_on_quit_sets_should_quit`, `landing_enter_on_quit_does_not_change_screen`.

**src/ui.rs**
- `render_landing_actions`: added Quit as a fourth entry in the `actions` array. Removed the separate `lines.push(landing_action_line("â—†", "Quit", "q", false, ...))` call below the loop. The loop now renders all four rows uniformly with spacer lines between them.

## Deviations from plan

None. The plan was followed exactly.

## Verification

`just verify` ran clean: 223 tests passed, 0 failures, no clippy warnings, format check passed.

## Risks / follow-up

The Quit row now gets a blank spacer after it (consistent with the other rows). Visually this is fine. No residual risks identified.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-30T11:22:26+02:00
- state: reviewed

# Review Postmortem â€” 00018 landing-quit-selectable

## Outcome
Accepted as-is. No code fixes applied. All acceptance criteria are met and `just verify` is clean (223 tests, 0 failures, clippy clean, fmt clean).

## What was verified
- `navigate`: `(Screen::Landing, _, Direction::Down)` caps at `.min(3)`. Up uses `saturating_sub`, so Quitâ†’Manage Issues works without extra changes.
- `open_selected_landing_entry`: refactored to an explicit `match` on indices 0/1/2/3 with `_ => {}`. The `_` fallthrough that previously mapped Issues is gone, matching the plan.
- Index 3 sets `self.should_quit = true` and does not touch `screen`/`focus`/`input_mode` â€” confirmed by `landing_enter_on_quit_does_not_change_screen`.
- Helpers `enter_generate`, `enter_pull_requests`, `enter_issues` keep each match arm single-intent. `enter_generate` preserves the workspace gate, the discovering-no-op, and the `spawn_repo_options_load(false)` call. `enter_pull_requests` keeps `spawn_pull_requests_load(false)`.
- `render_landing_actions`: Quit is the fourth `ActionItem`; the trailing static `landing_action_line(...)` call is removed. The loop emits selectable rows with blank spacers between them; the Quit row participates in the same selection highlight as the others.
- Global `q` keybind (`app.rs:293` â†’ `Action::Quit` â†’ `should_quit = true`) is untouched.
- Tests added match the plan and use the existing `test_app()` and `Action::Navigate` patterns rather than introducing new test scaffolding.

## Inferences (not facts) worth flagging
- Behavior delta on Landing entry index 0 when not in a workspace or while discovering: previously the code unconditionally reset `focus = Focus::Menu` and `input_mode = InputMode::Normal` before logging; the refactor only sets those when actually entering Generate. On Landing neither field is observable (the Landing hero only reads `landing.selected_entry`), so this is benign in practice, but it is a subtle semantic narrowing the implementation note does not call out.
- `render_menu` still has a dead `Screen::Landing` arm that builds a `selectable_list(&["Generate PR", "Manage PRs", "Manage Issues"], app.landing().selected_entry)`. It is never reached because `render` short-circuits to the Landing hero before invoking `render_menu`. The ticket's non-goals explicitly excluded touching that cap, so it correctly stays as-is; flagging as pre-existing dead code for a future cleanup ticket.
- Ticket non-goal honored: `ui.rs:1173` "Generate PR, Manage PRs, and Manage Issues are separate modes." is still factually accurate (Quit is not a mode), so the optional help-text adjustment from the plan was correctly skipped.

## Code quality
- Refactor improves locality and intent clarity; each helper has one job.
- No overengineering â€” Quit is a one-line side effect, not a new screen.
- Tests are small, behavior-focused, and follow the existing suite's idiom (direct access to private state via `app.landing.selected_entry`).

## Verification run
`just verify` â†’ fmt check ok, `cargo check` ok, `cargo clippy --all-targets --all-features -- -D warnings` ok, library tests 223 passed / 0 failed, integration tests 4 passed.
