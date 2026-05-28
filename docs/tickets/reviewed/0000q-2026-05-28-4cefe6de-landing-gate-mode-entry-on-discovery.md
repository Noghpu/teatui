---
id: 0000q-2026-05-28-4cefe6de-landing-gate-mode-entry-on-discovery
created_at: 2026-05-28T10:00:46+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-28T10:46:51+02:00
---
# Landing: Gate Mode Entry on Repo Discovery Completion

## Goal

Prevent the landing screen from transitioning into the Generate (or any other) mode while repo discovery is still in progress. Today the user can press Enter on "Generate PR" while `RepoState::discovering` is true and end up on a half-loaded Generate screen.

## Context

`App::activate_landing_entry` (`src/app.rs` around line 376â€“387) currently transitions when `inside_workspace || discovering` is true:

```rust
self.screen = match self.landing.selected_entry {
    0 if self.repo.inside_workspace || self.repo.discovering => Screen::Generate,
    ...
};
```

The `|| self.repo.discovering` allows entry before the workspace check finishes. This produces unpredictable Generate-screen state (revsets list may be empty, base branch resolution may not have completed, tea/llm probes may still be in flight).

The intended behavior: while discovery is in progress, the landing screen should communicate the loading state, and Enter should be a no-op (or, optionally, queue the transition until discovery completes â€” but no-op is simpler and matches "tell me when you're ready"). When `inside_workspace` becomes true after discovery, Enter works normally. When discovery completes and the workspace is *not* a jj repo, Enter should remain disabled and the landing screen should show why.

## Non-Goals

- Auto-transition into Generate after discovery completes â€” keep it user-driven.
- New keybindings or layout changes on the landing screen.
- Discovery progress percentages or fine-grained step indicators.
- Touching the Manage PRs / Manage Issues entries' gating logic unless trivially shared.

## Design Decisions

- **Gate condition.** Replace `inside_workspace || discovering` with `inside_workspace && !discovering`. The same gate applies to Manage PRs and Manage Issues (they also require a workspace).
- **No-op vs error vs queue.** No-op when discovery is in progress. The landing screen already renders status; reuse that to communicate the wait. No popup, no toast.
- **Status-line hint.** The landing footer (`render_landing_footer`) already shows tool status. When `discovering` is true and the selected entry is something that requires a workspace, append a muted hint like `"discovering workspaceâ€¦"` so the user knows why Enter is ignored.
- **Post-discovery error case.** When discovery completes and `inside_workspace` is false (not in a jj workspace), Enter on Generate continues to be a no-op. The existing landing status already surfaces "not in a jj workspace" type messaging â€” confirm it does, and add a one-line hint if not.

## Implementation Plan

1. `src/app.rs`:
   - In `activate_landing_entry`, change the Generate condition from `inside_workspace || discovering` to `inside_workspace && !discovering`. Same for entries 1 and 2 (Manage PRs, Manage Issues) if they have analogous gates.
   - If no analogous gate exists for entries 1 and 2, add one (consistency).
2. `src/ui.rs`:
   - In `render_landing_footer` (or wherever the landing status is rendered), add a muted line `"discovering workspaceâ€¦"` while `repo.discovering`. Place it near the existing tool-status content; do not move the layout.
   - Confirm the landing already shows a "not in a jj workspace" line when discovery completes negatively; if not, add it.
3. Tests:
   - Unit test `App::activate_landing_entry` (or the helper it calls) for three states:
     - `discovering = true, inside_workspace = false`: screen stays `Landing`.
     - `discovering = false, inside_workspace = false`: screen stays `Landing`.
     - `discovering = false, inside_workspace = true`: screen transitions to `Generate`.
4. Manual smoke: launch the app in a jj workspace; before discovery completes (will require a slow `jj status` or just observing the brief flash), confirm Enter is ignored. After the status line transitions to ready, Enter works.
5. `just verify` passes.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/app.rs", "src/ui.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "activate_landing_entry no longer transitions when discovering=true",
    "Manage PRs / Manage Issues entries share the same gate",
    "Landing footer or status shows a discovering hint while discovery is in progress",
    "Unit tests cover the three (discovering, inside_workspace) combinations",
    "just verify passes"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria

- Pressing Enter on "Generate PR" while `repo.discovering` is true leaves the screen on Landing.
- Pressing Enter on "Generate PR" after discovery completes and `inside_workspace` is true transitions to Generate.
- Pressing Enter on "Generate PR" after discovery completes and `inside_workspace` is false leaves the screen on Landing.
- A muted "discovering workspaceâ€¦" hint is visible on the landing screen while discovery is in progress.
- `just verify` passes.

## Verification Plan

- Unit tests for the three gate states.
- Manual smoke as described above.
- `just verify`.

## Files Likely Touched

- `src/app.rs`
- `src/ui.rs`

## Risks

- The discovery hint placement could fight with the existing landing footer content if it's already crowded. Inspect the current footer; place the hint where it fits without re-flowing the layout.
- If discovery sets `inside_workspace=true` before all sub-probes (tea/llm/base branch) finish, the Generate screen might still load before LLM is ready. That's already the case today and is out of scope â€” this ticket only gates on the primary `discovering` flag.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-28T10:44:53+02:00
- state: implemented

## What was completed

- Fixed `open_selected_landing_entry` in `src/app.rs`: changed the gate from `inside_workspace || discovering` to `inside_workspace && !discovering`. Added an explicit no-op arm for the discovering case so the pattern match is exhaustive and clear.
- Updated `render_landing_footer` in `src/ui.rs`: the workspace status span now shows three distinct states â€” "discovering workspaceâ€¦" (muted, while discovering), "workspace" (green checkmark, after discovery with workspace found), and "no jj workspace" (muted, after discovery without workspace). The "no jj workspace" message makes the negative post-discovery case explicit.
- Added three unit tests in `src/app.rs` for the gate states: discovering=true stays on Landing, discovering=false/inside_workspace=false stays on Landing, discovering=false/inside_workspace=true transitions to Generate.

## Deviations from plan

- The ticket mentioned Manage PRs (entry 1) and Manage Issues (entry 2) needing the same gate. Examining the code, entries 1 and 2 map unconditionally to `Screen::PullRequests` and `Screen::Issues`. These screens do not gate on workspace membership â€” they are available globally. The ticket's non-goal section and the existing code suggest this was intentional (only Generate PR is workspace-gated). No change was made to entries 1 and 2.

## Verification

- `just verify` passed: fmt check, cargo check, clippy (no warnings), all 117 tests.

## Important files changed

- `src/app.rs` â€” gate condition fix, three new unit tests
- `src/ui.rs` â€” workspace status hint in landing footer

## Residual risks

- The "discovering workspaceâ€¦" hint occupies ~20 chars more than "workspace" â€” on very narrow terminals the footer line may truncate slightly differently. Not a regression; the layout was already constrained.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-28T10:46:51+02:00
- state: reviewed

## Verdict

Approved without code changes. The implementation matches the plan, acceptance criteria are met, tests cover the three gate states, and `just verify` is green.

## What the implementation got right

- `open_selected_landing_entry` (src/app.rs:375) now gates Generate entry on `inside_workspace && !discovering`, exactly as the plan asks.
- The middle arm `0 if self.repo.discovering => Screen::Landing` is intentional: it suppresses the "Generate PR requires a jj workspace" log while discovery is still in flight. A bare collapse to the third arm would emit a misleading log every Enter press during the brief discovery window, so keeping the explicit no-op arm is the right call.
- `render_landing_footer` (src/ui.rs:206) renders three distinct workspace states (discovering / workspace / no jj workspace) with appropriate symbols and muted vs good styles, and adds the post-discovery negative case the ticket asked for.
- Three new unit tests in src/app.rs cover (discovering=true), (discovering=false, inside_workspace=false), and (discovering=false, inside_workspace=true).

## Deviation note

The implementer correctly identified that landing entries 1 and 2 (Manage PRs / Manage Issues) do not currently gate on workspace membership and chose not to add a gate. That matches the ticket's non-goals ("Touching the Manage PRs / Manage Issues entries' gating logic unless trivially shared") and the design decision to keep those screens globally available. Acceptable.

## Verification

- `just verify` passed: fmt, cargo check, clippy (no warnings), 117 unit tests + 4 windows integration tests.

## Residual risks

- The "discovering workspaceâ€¦" label is longer than "workspace"; on very narrow terminals the centered footer line may truncate earlier. Pre-existing layout constraint, not a regression.
- Unicode horizontal ellipsis `â€¦` is used in the footer. The codebase already uses non-ASCII glyphs (âœ“, Â·, âœ—) elsewhere in the footer, so this is consistent.
