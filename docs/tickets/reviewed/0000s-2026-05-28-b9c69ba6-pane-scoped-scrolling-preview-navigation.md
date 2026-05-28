---
id: 0000s-2026-05-28-b9c69ba6-pane-scoped-scrolling-preview-navigation
created_at: 2026-05-28T11:40:59+02:00
created_by_model: gpt-5
state: reviewed
state_updated_at: 2026-05-28T12:38:48+02:00
---
# Add Pane-Scoped Scrolling And Preview Navigation

## Goal
Make vertical navigation act on the focused pane instead of leaking into another pane, and ensure left, center, and right panes handle overflowing content predictably.

## Context
The Generate PR screen has three panes: left revset list, center PR form, and right preview/details. In `src/app.rs::navigate`, any Generate PR focus that is not `Focus::Form` currently moves the selected revset on Up/Down. That means pressing `j`/`k` or arrows while the right pane is focused navigates the left pane. The user wants right-pane navigation disabled in that sense: vertical keys in the right pane should scroll the right pane only.

`src/ui.rs` currently renders pane contents as paragraphs/lists without persistent scroll offsets. Long revset lists, long forms, prompt text, drafts, command logs, issue/PR previews, and small terminal sizes can overflow without a focused scrolling model.

## Non-Goals
- Do not implement picker behavior.
- Do not fetch repo metadata.
- Do not add mouse scrolling unless the existing event model already supports it cheaply.
- Do not introduce a modal/window stack.

## Design Decisions
- In Generate PR, Up/Down semantics are pane-scoped:
  - `Focus::Menu`: move the selected revset and keep it visible in the left pane.
  - `Focus::Form`: move the selected form field in normal mode and keep it visible in the center pane.
  - `Focus::Preview`: scroll preview content only; do not change selected revset or selected form field.
- Horizontal focus movement remains `h`/`l`, arrows, tab/backtab as currently designed.
- Every pane with content taller than its visible area gets persistent scroll state and clamped offsets. At minimum cover Generate PR left, center, and right panes; preserve or extend the same pattern for Manage PRs and Manage Issues previews where they use the shared pane renderers.
- Rendering should use Ratatui scrolling (`Paragraph::scroll` or list state where appropriate) rather than dropping lines manually in multiple ad hoc places.
- Resize events must clamp scroll offsets to the new visible height.

## Implementation Plan
1. Add explicit scroll state to app/generate state for pane content, such as menu/form/preview offsets plus helpers for clamping and keeping selected rows visible.
2. Update `src/app.rs::navigate` so Generate PR `Focus::Preview` uses scroll actions and never calls `move_revset_up` or `move_revset_down`.
3. Update left-pane revset rendering to keep selected revsets visible when the list overflows.
4. Update center-pane form rendering to keep the selected field visible after field navigation, after entering/exiting edit mode, and after resize.
5. Update right-pane preview rendering to apply the preview scroll offset and clamp it against the current generated line count.
6. Apply the same overflow-safe rendering pattern to simple Manage PRs/Issues work and preview panes where their content is already paragraph-based.
7. Add focused tests for navigation isolation: with Generate PR focus on Preview, vertical navigation changes preview scroll and does not mutate `selected_revset`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/app.rs", "src/generate.rs", "src/ui.rs", "src/action.rs"],
  "likely_files": ["src/app.rs", "src/generate.rs", "src/ui.rs", "src/action.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["Preview-pane vertical keys do not change left-pane selection", "All Generate PR panes clamp and retain scroll offsets safely", "Resize handling keeps focused content visible without overlap"],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- In Generate PR with focus on the right pane, pressing Up/Down or `k`/`j` scrolls the right pane when it overflows.
- In Generate PR with focus on the right pane, vertical keys do not change the selected left-pane revset.
- The left pane keeps the selected revset visible when there are more revsets than rows.
- The center pane keeps the selected form field visible when the form overflows.
- The right pane can scroll long prompt text, generated drafts, logs, and execution previews.
- Resize events do not leave scroll offsets pointing past available content.

## Verification Plan
- Run `just verify`.
- Manually run the TUI with a small terminal height, focus each Generate PR pane, and confirm vertical keys affect only the focused pane.
- Manually inspect long prompt preview/draft content and confirm the right pane scrolls without changing the left selection.

## Files Likely Touched
- `src/app.rs`
- `src/generate.rs`
- `src/ui.rs`
- `src/action.rs`

## Risks
- Form row heights are dynamic because description and validation errors can add rows; keep row-height calculations centralized so scroll clamping matches rendering.
- Ratatui `List` and `Paragraph` scrolling behave differently; use one small local abstraction if needed instead of scattering offset math.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-05-28T12:34:49+02:00
- state: implemented

Completed the Generate PR pane-scoped scrolling work.

What changed:
- Added explicit scroll state for Generate PR menu, form, and preview panes, plus the same scroll path for the PR and issue preview placeholders.
- Updated Generate PR navigation so Preview focus only scrolls the preview pane and never mutates the selected revset.
- Kept the selected revset and form field visible by clamping scroll offsets from the measured rendered ranges.
- Added focused unit coverage for preview-scroll isolation and scroll clamping.

Deviations:
- None material; the implementation followed the ticket plan.

Verification:
- Ran `just verify`.

Files changed:
- `src/app.rs`
- `src/generate.rs`
- `src/ui.rs`

Residual risk:
- The preview panes for Manage PRs and Manage Issues still render placeholder content, so their scroll behavior is wired but not exercised by longer real data yet.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-05-28T12:38:48+02:00
- state: reviewed

## Review Postmortem

Facts:
- Reviewed ticket 0000s-2026-05-28-b9c69ba6-pane-scoped-scrolling-preview-navigation against docs/design.md and the implemented changes in src/app.rs, src/generate.rs, and src/ui.rs.
- Generate PR preview navigation no longer changes selected_revset; focused test coverage exists in app::tests::preview_navigation_scrolls_without_moving_the_selected_revset.
- Generate PR menu, form, and preview scroll offsets are persistent and clamped during rendering.
- Found that preview clamp height used raw Line count while Paragraph::wrap can render one long Line as multiple terminal rows, which could make the tail of long prompt/draft/log lines unreachable.
- Fixed preview clamping to estimate wrapped content height from Ratatui Line::width for Generate, PR, and Issue preview panes, and added ui::tests::wrapped_content_height_accounts_for_wrapped_preview_lines.

Inferences:
- The wrapped-height estimate may allow a little extra blank scroll for word-wrapped text, but it avoids under-clamping and is preferable for long prompt/draft/log review content.
- Manage PRs and Manage Issues still use placeholder preview content, so the shared preview clamp path is covered structurally rather than by real list/detail data.

Verification:
- Ran focused tests for wrapped content height, preview navigation isolation, and scroll range clamping.
- Ran just verify successfully.
