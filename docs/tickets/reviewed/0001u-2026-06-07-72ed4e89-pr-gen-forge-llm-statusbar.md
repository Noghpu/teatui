---
id: 0001u-2026-06-07-72ed4e89-pr-gen-forge-llm-statusbar
created_at: 2026-06-07T10:15:00+02:00
created_by_model: claude-sonnet-4-6/medium
state: reviewed
state_updated_at: 2026-06-07T11:20:59+02:00
---
# PR Gen Screen: Forge and LLM Indicator in the Status Bar

## Goal

Show which forge and LLM backend are active in the PR generation screen's bottom status bar, so the user always knows the active context without returning to the landing screen.

## Context

The landing screen footer already shows forge and LLM health via `push_tool`/`push_llm` in `render_landing_footer`. The PR generation screen (`src/screens/generate.rs`) has only a `render_help` line at the bottom and no status bar chip row.

The generate screen `render` function already receives `status: &StatusStore`. The `StatusStore` has `forge_label`, `forge`, `forge_auth`, and `llm`. The config's active LLM backend model is in `config.llm.active_backend().model` and forge selection in `config.pr.forge`.

Two surfaces are relevant: the single-PR generation flow (3-pane/2-pane layout) and the bulk review modal. The bulk modal already has its own `bulk_review_footer_line`. Only the single-PR flow needs the new indicator; the bulk modal draws on top and is unaffected.

## Non-Goals

- No changes to detection/selection logic.
- No changes to the `StatusStore` data model.
- Do not add forge/LLM to the bulk modal footer (already information-rich and tight on space).

## Design Decisions

**Where to add the status bar:**

Add a new status bar row between the main content area and the help line in `render`. The layout currently is:

```
[main (Fill)]
[help_area (Length 1)]
```

Change it to:

```
[main (Fill)]
[status_area (Length 1)]   <- new
[help_area (Length 1)]
```

**What to show:**

A compact chip row using `theme::StatusChip` and `theme::status_line` (same pattern as `landing.rs` and the bulk modal footer):

```
[ Normal ] [ Generate ] [ gh . github.com ] [ LLM: qwen2.5-coder:latest ]
```

- Mode chip: `Normal` or `Editing` depending on `state.input_mode`.
- Screen chip: `Generate`.
- Forge chip: binary label + optional host (compact, no login -- login is a landing concern). Use `theme::success()` when forge tool is available, `theme::error()` when missing/errored, `theme::muted()` when pending.
- LLM chip: `LLM: {model_name}` or `LLM: pending`. Use `theme::success()` when available, `theme::muted()` when pending/unreachable.

**Data access:** Pass `config: &Config` to `render` (rather than storing model name in `StatusStore`, which would be duplication since it comes from config, not a probe). Signature change: `generate::render(state, status, frame, area)` becomes `generate::render(state, status, config, frame, area)`. Update call site in `src/app.rs` or `src/screens/mod.rs`.

**Forge host:** Extract from `status.workspace` value if `WorkspaceInfo::Inside { remote: Some(r), .. }` -- use `r.host`.

## Implementation Plan

1. In `src/screens/generate.rs`:
   - Add `Config` import from `crate::config`.
   - Add `WorkspaceInfo` to the domain imports (already imported elsewhere in the file -- verify).
   - Change `render` signature to add `config: &Config` parameter.
   - Change vertical layout from 2 areas to 3: `[Fill, Length(1), Length(1)]` for `[main, status_area, help_area]`.
   - Implement `fn render_generate_status_bar(state: &GenerateState, status: &StatusStore, config: &Config, frame: &mut Frame, area: Rect)`:
     - Mode chip: `theme::StatusChip::mode(if editing { "Editing" } else { "Normal" })`.
     - Screen chip: `theme::StatusChip::plain("Generate", N)`.
     - Forge chip: build label string from `status.forge_label` + optional ` . {host}`, then push as a styled span based on forge tool health.
     - LLM chip: `format!("LLM: {}", config.llm.active_backend().model)`.
     - Render via `theme::status_line(chips, area.width)`.
   - Call `render_generate_status_bar` in `render` with the new `status_area`.

2. In `src/app.rs` or `src/screens/mod.rs`:
   - Update the call to `generate::render` to pass `&self.config`.

3. Snapshot tests in `src/bin/ui-snapshots.rs`:
   - Add/update generate-screen snapshots to include the new status bar row.

## Acceptance Criteria

- The generate screen shows a one-line status bar above the help line.
- The status bar includes mode (Normal/Editing), screen name (Generate), forge info (label + host if available), and LLM info (active backend model name).
- The forge chip uses success/error/muted styling consistent with `landing.rs`.
- The LLM chip shows `config.llm.active_backend().model`.
- When `state.input_mode == InputMode::Editing`, the mode chip shows `Editing`.
- `cargo test` passes.
- Snapshot tests updated.

## Verification Plan

1. `cargo test` -- unit + snapshot tests green.
2. Run the TUI, navigate to generate screen, confirm status bar appears with correct forge and LLM values.
3. Enter edit mode on a form field, confirm mode chip updates to `Editing`.
4. Test with missing forge tool to confirm error styling.

## Files Likely Touched

- `src/screens/generate.rs` -- main change: layout, `render_generate_status_bar`.
- `src/app.rs` or `src/screens/mod.rs` -- update call site.
- `src/bin/ui-snapshots.rs` -- snapshot updates.

## Risks

- Adding a row reduces vertical space for the main content pane by 1 line on short terminals. Accept the trade-off; minimum terminal height is not enforced elsewhere.
- Snapshot test churn: all generate-screen snapshots that include the full height will have bottom rows shift by 1.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "src/screens/generate.rs", "src/screens/landing.rs", "src/app.rs", "src/config.rs", "src/domain/status_store.rs"],
  "likely_files": [
    "src/screens/generate.rs",
    "src/app.rs",
    "src/screens/mod.rs",
    "src/bin/ui-snapshots.rs"
  ],
  "verification_commands": ["cargo test"],
  "review_focus": ["status bar layout does not break narrow-terminal paths", "forge chip styling matches landing conventions", "call site update completeness", "snapshot test coverage"],
  "jj_description_prefix": "ui"
}
```
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5.4-mini-medium
- completed_at: 2026-06-07T11:15:37+02:00
- state: implemented

Completed:
- Added a one-line Generate screen status bar between the main content and help line.
- Threaded Config into generate rendering so the status bar shows the active LLM backend model.
- Added styled status chips so forge and LLM health can use success/error/muted styling.
- Rendered mode, Generate screen name, forge label plus remote host, and active LLM model for the single-PR flow.
- Updated app, snapshot renderer, and render smoke fixtures for the new render signature.
- Added render-smoke assertions for Normal/Editing mode plus forge host and LLM model content.

Deviations:
- The current codebase already has the forge abstraction from a later phase, so the implementation uses the existing StatusStore forge label and current Config rather than older tea-only assumptions.
- Snapshot artifacts were regenerated under target/ui-snapshots; tracked snapshot fixture files were not present to update.

Verification:
- cargo check passed.
- cargo test passed: 231 unit tests and 62 render smoke tests, with 2 ignored llama integration tests.
- just snapshots passed and wrote 22 snapshots to target/ui-snapshots.
- just verify passed: fmt check, cargo check, clippy -D warnings, and all tests.

Files changed:
- src/screens/generate.rs
- src/screens/theme.rs
- src/app.rs
- src/bin/ui-snapshots.rs
- tests/render_smoke.rs

Residual risks:
- The added status row reduces main content height by one line as expected by the ticket.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-06-07T11:20:59+02:00
- state: reviewed

Facts:
- Reviewed ticket 0001u against the implemented diff and current Phase 9 codebase.
- The Generate render signature is threaded through app rendering, render smoke helpers, and UI snapshot rendering.
- The single-PR Generate screen now has a status row above the help line; bulk modal rendering remains layered over the main screen.
- The status row includes mode, Generate, forge label plus remote host when known, and the configured active LLM model once LLM health is available.
- Review changes adjusted the forge host separator to match the established middle-dot UI convention and changed pending/unreachable LLM health to render as `LLM: pending` / `LLM: unreachable` instead of implying the configured model is reachable before health lands.
- Added render-smoke coverage for pending LLM status in the Generate status bar.
- Verification passed with `cargo test generate_status_bar --test render_smoke` and `just verify`.

Inferences:
- The implementation satisfies the acceptance criteria after review edits.
- The one-line vertical space tradeoff is acceptable and already covered by small-terminal render smoke tests.
- No additional queue blocking or replanning is needed.
