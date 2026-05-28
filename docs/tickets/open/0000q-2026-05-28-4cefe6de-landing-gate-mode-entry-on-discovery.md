---
id: 0000q-2026-05-28-4cefe6de-landing-gate-mode-entry-on-discovery
created_at: 2026-05-28T10:00:46+02:00
created_by_model: claude-opus-4-7/high
state: open
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
