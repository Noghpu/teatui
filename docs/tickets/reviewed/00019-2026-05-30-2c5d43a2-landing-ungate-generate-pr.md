---
id: 00019-2026-05-30-2c5d43a2-landing-ungate-generate-pr
created_at: 2026-05-30T11:11:14+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-30T11:37:32+02:00
---
# Allow entering Generate PR before discovery completes

## Goal
Make the Landing â†’ Generate PR entry feel instantaneous: pressing Enter on the Generate PR row immediately switches to the Generate screen, even while background workspace/tool/LLM discovery is still in flight. Show a small "discovering workspaceâ€¦" banner inside the Generate screen so the user understands why interactive fields are not yet populated, and bounce them back to Landing only when discovery completes with `inside_workspace == false`.

## Context
Today `App::open_selected_landing_entry` in `src/app.rs` matches Generate PR with:

```rust
0 if self.repo.inside_workspace && !self.repo.discovering => Screen::Generate,
0 if self.repo.discovering => Screen::Landing,
0 => { self.log("Generate PR requires a jj workspace"); Screen::Landing }
```

The middle arm silently swallows Enter for the entire ~50ms-2s window that `repo.discovering` is true. From the user's perspective the keypress does nothing â€” there is no log, no visual change, no banner. The user calls this "sluggish" even though the actual cause is "intentionally ignored." Discovery is dispatched from `App::refresh` (via `repo::spawn_discovery`) which runs `jj --version`, `git --version`, `tea --version`, `jj root`, `git remote get-url origin`, `tea login list`, and an LLM `health_check` per backend concurrently with a 5 s outer timeout (2 s for LLM).

Generate PR's left pane and form already cope with an empty revset list (the `GenerateState::with_placeholder` path covers this on startup). The screen renders fine before the first `Repo` event arrives. The blocker is only the gate in `open_selected_landing_entry` and the lack of an in-mode indication that work is still happening.

`apply_repo` (`src/app.rs` around line 979) already contains a bounce-back: if the user is on Screen::Generate and `inside_workspace` is false on the incoming RepoState, it logs and returns to Landing. That behavior is exactly what we want to preserve â€” it is the safety net for the optimistic entry.

## Non-Goals
- No change to how discovery itself runs or what events it emits. (Splitting discovery into per-probe events is a separate ticket.)
- No new caching or memoization of discovery results.
- No "skeleton" placeholders inside the form fields beyond what already renders today. Only the screen-level banner is new.
- No change to the Generate PR keybinds while discovery is pending â€” the user can navigate the revset list (which may be empty) and the form. They simply cannot run `g` (generate) until context is collected, which is enforced elsewhere already.

## Design Decisions
- Drop the `0 if self.repo.discovering` arm. Replace the three arms for index 0 with a simpler shape: enter Generate PR optimistically unless we already know we are not in a workspace.
  - If `repo.discovering` is true â†’ enter Generate PR (the bounce in `apply_repo` will eject us if discovery finishes with no workspace).
  - If `repo.discovering` is false and `inside_workspace` is true â†’ enter Generate PR.
  - If `repo.discovering` is false and `inside_workspace` is false â†’ log and stay on Landing (this is the "blocked: not a jj workspace" state, and we have a definitive answer).
- Add a small banner row to the Generate screen that renders when `repo.discovering` is true. Reuse the existing `spinner_frame` helper from `src/ui.rs` so we get the same Braille spinner used in the footer for async phases. The banner should sit at the top of the Generate work area or as a short line above the three-pane layout â€” pick whichever location does not push form fields off-screen at common terminal sizes. A single-row banner inside the left menu pane (above the revset list) is acceptable and keeps the three-pane geometry stable.
- The banner text: `â ‹ discovering workspaceâ€¦` (spinner glyph from `spinner_frame(Instant::now())`). When `repo.discovering == false`, the banner disappears.
- Keep the bounce-back in `apply_repo` exactly as-is. Add no new logging spam; the existing `"Generate PR blocked: cwd is not inside a jj workspace"` log line already explains the bounce.
- Add a focused integration test in `tests/windows_landing_async.rs` (new file) that exercises the optimistic entry through the real `App` + fake-shim harness:
  - Build an `App` using a fake `jj` shim that sleeps before printing root.
  - Call `App::handle_key(Enter)` on the Generate PR landing entry *before* draining the `BackgroundEvent::Repo` event.
  - Assert `app.screen() == Screen::Generate` immediately after.
  - Drain the discovery event; assert that with `inside_workspace == true` the screen stays on Generate, and with `inside_workspace == false` the screen bounces back to Landing.
  - Follow the `windows_` prefix + `#![cfg(windows)]` convention from `AGENTS.md`.
- Use the existing fake-shim machinery in `tests/windows_pr_generation_integration.rs` as the model: extract the `.cmd` fakes and `set_fake_env` pattern; do not refactor those into a shared helper crate as part of this ticket (that is a larger cleanup).

## Implementation Plan
1. `src/app.rs::open_selected_landing_entry`:
   - Replace the three index-0 arms with two: enter `Screen::Generate` when `repo.discovering || repo.inside_workspace`, otherwise log + stay on Landing.
   - Preserve the existing `spawn_repo_options_load(false)` call when entering Generate.
   - The bounce-back in `apply_repo` already handles the "discovery resolved as no-workspace while user is on Generate" path; do not duplicate it here.
2. `src/ui.rs`:
   - Add a `render_generate_discovering_banner(frame, app, area)` helper (or fold into the existing left-menu render path) that draws a single-row spinner-led line when `app.repo().discovering` is true. Place it above the existing revset list inside the menu pane so the three-pane horizontal geometry is unchanged.
   - Use the existing `spinner_frame` function.
   - When `repo.discovering` is false, render nothing â€” the existing menu content fills the space.
3. Add `tests/windows_landing_async.rs`:
   - Mirror the prologue of `tests/windows_pr_generation_integration.rs` (`#![cfg(windows)]`, `TEST_LOCK`, `FakeCommandTree`).
   - Add a slow `jj.cmd` variant: insert `powershell -NoLogo -NonInteractive -Command "Start-Sleep -Milliseconds 300"` before printing root. A 300 ms sleep is long enough to win the race against the immediate `Enter` keypress while keeping the test fast.
   - Build a real `App` with `App::new`, call `app.refresh()`, then synchronously call the public key handler with an Enter `KeyEvent` while the discovery task is still sleeping.
   - Assert `app.screen() == Screen::Generate` *before* awaiting any background event.
   - Drive the event loop one step at a time (drain `bg_rx` with `timeout(Duration::from_millis(500), ...)`) and assert the screen stays on Generate when the fake reports `inside_workspace = true`.
   - Add a sibling test where the fake `jj root` returns a non-zero exit (so `workspace_root` ends up `None`): assert that after the `Repo` event is applied, the screen bounces back to Landing.
4. Verify existing tests still pass: `landing_entry_stays_on_landing_while_discovering` and `landing_entry_stays_on_landing_when_not_in_workspace` in `src/app.rs::tests` will change behavior. The first test must be updated to reflect the new optimistic policy (now the screen *does* transition to Generate while discovering); rename it to `landing_entry_transitions_to_generate_while_discovering` and flip the assertion. The second test (`landing_entry_stays_on_landing_when_not_in_workspace`) is still correct as-is and must keep passing.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/design.md",
    "src/app.rs",
    "src/ui.rs",
    "src/repo.rs",
    "tests/windows_pr_generation_integration.rs"
  ],
  "likely_files": [
    "src/app.rs",
    "src/ui.rs",
    "tests/windows_landing_async.rs"
  ],
  "verification_commands": ["just fmt", "just check", "just clippy", "just test", "just verify"],
  "review_focus": [
    "Pressing Enter on Generate PR while discovery is in flight transitions to Screen::Generate immediately.",
    "If discovery later reports no workspace, apply_repo bounces back to Landing â€” confirm the existing bounce log fires once and not twice.",
    "A spinner-led 'discovering workspaceâ€¦' banner appears on the Generate screen while repo.discovering is true and disappears once it is false.",
    "The updated landing_entry_* tests in src/app.rs cover both the optimistic-transition and the post-discovery-bounce paths.",
    "The new windows_landing_async.rs integration test races a key press against a sleeping fake `jj` and asserts the optimistic entry.",
    "spawn_repo_options_load is still called on Generate entry (cache-first behavior preserved)."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- Enter on the Generate PR row from Landing transitions to `Screen::Generate` immediately, regardless of whether `repo.discovering` is true or false (as long as the screen has not already been determined to be not-a-workspace).
- While `repo.discovering` is true on the Generate screen, a single-row spinner-led banner reads `â ‹ discovering workspaceâ€¦` (spinner glyph from `spinner_frame`).
- When discovery completes with `inside_workspace = false`, the user is bounced back to Landing with the existing log message; the banner is no longer rendered.
- When discovery completes with `inside_workspace = true`, the banner disappears and the existing revset list / form rendering takes over with no visible flicker beyond the banner removal.
- The new integration test in `tests/windows_landing_async.rs` passes on Windows. It compiles to an empty crate on other platforms via `#![cfg(windows)]`.
- Existing `landing_entry_*` tests in `src/app.rs` pass after being updated to the new policy.

## Verification Plan
- `just verify` on Windows.
- Manual smoke: launch the app inside a jj repo, press Down â†’ Down â†’ Up to land on Generate PR, then press Enter rapidly. Confirm the screen transitions immediately and the banner is visible for a short blink.
- Manual smoke (negative): launch the app outside a jj workspace, repeat the keypress. Confirm Generate PR enters briefly and then bounces back to Landing within ~5 s with the existing "blocked" log line.

## Files Likely Touched
- `src/app.rs`
- `src/ui.rs`
- `tests/windows_landing_async.rs` (new)

## Risks
- The optimistic-entry change means the Generate screen can render with an empty revset list and an empty form. The screen already supports this state, but visual polish at very small terminal sizes was not validated â€” the banner row must not push form fields off-screen at the smallest reasonable terminal height. Use a single line, not a multi-line widget.
- Bouncing back to Landing during discovery could surprise a user who had already started typing in the form. The current code does not start in `Editing` mode automatically, so this is theoretical; document in `Risks` and do not pre-emptively guard against it.
- The fake `jj` sleep in the new integration test must be long enough to lose the race deliberately, but short enough not to slow CI. 300 ms is the proposed default; the implementer may tune it.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-30T11:33:06+02:00
- state: implemented

## What was completed

Implemented optimistic entry to the Generate PR screen before workspace discovery completes.

### Changes

**`src/app.rs`**
- `enter_generate`: Replaced the three-arm match with a two-arm form. Entering Generate PR is now allowed when `repo.discovering || repo.inside_workspace` (optimistic entry). Only blocked when discovery has definitively resolved to `inside_workspace == false`.
- `update`: Made `pub` so integration tests can drive the app's state machine.
- `handle_background`: Made `pub` so integration tests can inject background events.
- `landing_mut`: New `pub fn` returning `&mut LandingState` so integration tests can set `selected_entry`.
- Unit test `landing_entry_stays_on_landing_while_discovering` renamed to `landing_entry_transitions_to_generate_while_discovering` with assertion flipped to `Screen::Generate` to reflect the new optimistic policy.

**`src/ui.rs`**
- `render_generate_menu_lines`: Added a single-row spinner-led banner (`â ‹ discovering workspaceâ€¦`) when `app.repo().discovering == true`. Banner uses the existing `spinner_frame` helper with `ACCENT` color for spinner and `MUTED` for text. When `discovering == false`, renders nothing. A separator below the banner is rendered only when the revset list is non-empty to keep the geometry stable.

**`tests/windows_landing_async.rs`** (new)
- `optimistic_entry_to_generate_while_discovering`: Verifies that pressing Enter on Generate PR while `discovering == true` immediately transitions to `Screen::Generate`.
- `optimistic_entry_stays_on_generate_when_workspace_confirmed`: After optimistic entry, injecting a `Repo` event with `inside_workspace == true` keeps the screen on `Screen::Generate`.
- `optimistic_entry_bounces_to_landing_when_not_in_workspace`: After optimistic entry, injecting a `Repo` event with `inside_workspace == false` bounces back to `Screen::Landing` via the existing `apply_repo` bounce.
- `racing_keypress_beats_slow_discovery`: Real fake-jj shim that sleeps 300 ms before returning root. `refresh()` spawns discovery, keypress immediately wins the race, then the background event confirms `inside_workspace == true` and the screen stays on Generate.

### Deviations from plan

- Made `update` and `handle_background` public (plan said "public key handler" but `handle_key` is private and returns `Action` â€“ exposing `update` and `handle_background` directly is cleaner for integration tests and sufficient for the test assertions). 
- Added `landing_mut()` as a minimal public accessor rather than exposing the private field.
- The bounce-back in `apply_repo` was left exactly as-is per the ticket's Non-Goals.

### Verification

`just verify` â€” all 4 new integration tests pass, all 223 unit tests pass, existing 4 integration tests pass. Format and clippy clean.

### Risks

- The optimistic entry means Generate PR can render with an empty revset list. The screen already handles this via the `with_placeholder` path.
- The banner adds one row above the revset list, not displacing form fields (it only affects the left menu pane). Tested at the default terminal height; very small terminals were not validated but the banner is a single row.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-30T11:37:32+02:00
- state: reviewed

## Review Postmortem

Metadata:
- model: claude-opus-4-7
- review_completed_at: 2026-05-30

### Outcome

Accepted as-is. No source-level fixes were applied.

### What I verified

- `enter_generate` in `src/app.rs` (lines 515-531) now uses the two-arm form specified by the plan: enter Generate when `repo.discovering || repo.inside_workspace`, otherwise log and stay on Landing. `spawn_repo_options_load(false)` is preserved.
- `apply_repo` (lines 1000-1011) retains the bounce-back: when on `Screen::Generate` and the incoming RepoState has `inside_workspace == false`, it logs `"Generate PR blocked: cwd is not inside a jj workspace"` and returns to Landing. The bounce log fires exactly once (no duplication from `enter_generate`).
- `render_generate_menu_lines` in `src/ui.rs` (lines 392-405) renders a single-row spinner banner (` â ‹ discovering workspaceâ€¦`) when `app.repo().discovering` is true, with a separator line below only when revsets exist (preserving the three-pane geometry when the list is empty). Uses the existing `spinner_frame` and `colors::ACCENT`/`colors::MUTED`. Renders nothing when `discovering == false`.
- The renamed unit test `landing_entry_transitions_to_generate_while_discovering` in `src/app.rs::tests` flips its assertion correctly. `landing_entry_stays_on_landing_when_not_in_workspace` continues to pass under the new policy.
- `tests/windows_landing_async.rs` (new, 338 lines) contains 4 tests:
  - `optimistic_entry_to_generate_while_discovering` â€” direct state-machine assertion.
  - `optimistic_entry_stays_on_generate_when_workspace_confirmed` â€” direct injection of `BackgroundEvent::Repo` with `inside_workspace == true`.
  - `optimistic_entry_bounces_to_landing_when_not_in_workspace` â€” direct injection of `BackgroundEvent::Repo` with `inside_workspace == false`.
  - `racing_keypress_beats_slow_discovery` â€” real fake-jj shim that sleeps 300 ms before returning root; verifies the keypress wins the race and the screen stays on Generate once the Repo event arrives.
- `just verify` passes locally: 223 unit tests, 4 new integration tests in `windows_landing_async`, 4 existing in `windows_pr_generation_integration`. `just clippy` is clean.

### Quality observations (not blocking)

- **Public API expansion for tests.** `App::update`, `App::handle_background`, and the new `App::landing_mut` were promoted to `pub` solely so integration tests in `tests/` can drive the state machine. This is a real encapsulation cost â€” `update` and `handle_background` are now exposed to every consumer, not just tests. A `#[cfg(any(test, feature = "test-api"))]` gate or moving these three integration tests into `#[cfg(test)] mod tests` inside `src/app.rs` would have kept the public surface narrower. Given the design's "tests minimal" stance and the convenience of integration tests for race scenarios, the trade-off is acceptable; flag for future refactor if `App`'s public API needs to be re-tightened.
- **Test fixture overhead in non-race tests.** Three of the four tests in `windows_landing_async.rs` (`optimistic_entry_to_generate_while_discovering`, `optimistic_entry_stays_on_generate_when_workspace_confirmed`, `optimistic_entry_bounces_to_landing_when_not_in_workspace`) construct a `FakeCommandTree` and call `set_fake_env` but never trigger any subprocess â€” they only manipulate `App` state directly via `update`/`handle_background`. Only `racing_keypress_beats_slow_discovery` actually needs the fake shim and the `#![cfg(windows)]` gate. Consolidating the three pure-state tests into a regular `#[test]` block in `src/app.rs::tests` (where the rest of the policy is tested) would have removed ~80 lines of fixture boilerplate and made them portable across platforms. Not worth churning now since the tests are correct and the file is already self-contained, but the bar for similar future work should be lower.
- **Banner spacing.** The banner renders as ` â ‹ discovering workspaceâ€¦` (leading space before spinner). This is a minor cosmetic deviation from the ticket's stated `â ‹ discovering workspaceâ€¦` but reads cleanly and matches the indentation feel of selected/unselected revset rows (which prefix with a one-char marker + space). No fix applied.
- **`inner_width == 0` panic safety.** The new separator `"â”€".repeat(inner_width)` shares the same potential zero-width concern as the existing per-row separator. Both rely on a non-degenerate menu pane width and would render an empty line if width were 0. Not a new bug.

### Risks confirmed

- The Generate screen renders cleanly with an empty revset list during the discovery window â€” covered by the existing `GenerateState::with_placeholder("Revsets pending discovery")` path called from `App::new`.
- The banner is a single row; at very small terminal heights the form fields would already be off-screen for other reasons. Not validated against pathologically small terminals.

### Verification

- `just verify` â€” pass (227 tests total, fmt/check/clippy/test all green).
- `just clippy` â€” pass (no warnings, `-D warnings` enforced).
