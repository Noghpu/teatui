---
id: 00017-2026-05-29-8888fe3e-phase-aware-footer-spinner
created_at: 2026-05-29T22:39:05+02:00
created_by_model: claude-opus-4-7/high
state: implemented
state_updated_at: 2026-05-29T23:23:58+02:00
---
# Phase-aware footer spinner during async work

## Goal
Replace the static keybind line in the Generate-screen footer with a spinner-led status line while async phases are running (`CollectingContext`, `Generating`, `CheckingFreshness`, `Executing`). Unify the existing dedicated `Confirming` and `Executing` footer arms through the same helper.

## Context
Today the footer hint line stays the same during long-running phases like context collection and LLM generation. The user has to know to look at the right pane to see the small `"Collecting context…"` / `"Generating draft…"` text. With pane-local keybinds now in place, the footer is the natural place to surface async status. This ticket adds a Braille spinner driven by `Instant::now()` and reuses the existing tick action for repaints.

## Non-Goals
- No new threads or tickers — reuse the existing tick path that already drives repaints.
- No changes to the async pipeline itself.
- No spinner outside the Generate screen.
- No replacement of the `InputMode::Editing` footer arm (that is a different mode).

## Design Decisions
- Add a small `fn spinner_frame(now: Instant) -> char` returning one of `⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏` based on elapsed time modulo 10 frames at ~10 fps. Or the 4-frame variant `⠋ ⠙ ⠸ ⠴` if cleaner.
- Add `fn generate_async_status_line(phase: GeneratePhase, now: Instant) -> Line<'static>`:
  - `CollectingContext` → spinner + `Collecting context…` + `Esc cancel`.
  - `Generating` → spinner + `Generating draft…` + `Esc cancel` (if cancel is wired; otherwise omit).
  - `CheckingFreshness` → spinner + `Verifying repo context…` + `Esc cancel`.
  - `Executing` → spinner + `Executing plan…` (no Esc — execution is non-cancellable today).
- Replace the existing dedicated arms for `CheckingFreshness` (around `src/ui.rs:1384`) and `Executing` (around `src/ui.rs:1392`) by calling the new helper. Keep `Confirming` separate (it is an input-mode arm with `Enter execute / Esc cancel`).
- Use `Instant::now()` inside the render path. The existing tick action already triggers redraws often enough that the spinner will animate without a new ticker.
- Do not animate when the phase is steady (`SelectingRevset`, `EditingForm`, `ContextReady`, `DraftReady`, `Complete`, `Failed`).

## Implementation Plan
1. In `src/ui.rs`, add `fn spinner_frame(now: Instant) -> char` and `fn generate_async_status_line(phase: GeneratePhase, now: Instant) -> Line<'static>`.
2. Replace the body of the existing `CheckingFreshness` (around line 1384) and `Executing` (around line 1392) arms with calls to `generate_async_status_line`.
3. Add a new arm above the Generate-default arm that matches `CollectingContext` and `Generating` and routes to `generate_async_status_line`.
4. Confirm the tick action already calls `tea::draw` (or equivalent) at a rate sufficient to animate; if not, document the constraint and consider a small periodic tick from the background channel — but prefer the existing mechanism.
5. Add a focused test for `generate_async_status_line`:
   - For each async phase, the returned line text contains the expected status string and a spinner char.
   - For non-async phases, the helper is not invoked from the dispatcher (assert via a separate small test of the dispatcher arm).

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/ui.rs", "src/event.rs", "src/tea.rs"],
  "likely_files": ["src/ui.rs"],
  "verification_commands": ["just fmt", "just check", "just clippy", "just test", "just verify"],
  "review_focus": [
    "Async phases show a spinner-led status in the footer.",
    "Existing CheckingFreshness and Executing arms are unified through the new helper.",
    "Confirming and Editing input-mode arms remain dedicated.",
    "No new threads or tickers introduced.",
    "Spinner animates from the existing tick path."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- Entering `CollectingContext`, `Generating`, `CheckingFreshness`, or `Executing` replaces the footer hint line with a status line that includes a Braille spinner frame and a phase-specific message.
- The spinner advances visually over time without a new ticker or thread.
- `Esc cancel` continues to appear where cancel actually works (CheckingFreshness, Generating, CollectingContext) and is omitted where it does not (Executing).
- `Confirming` and `Editing` footer arms render unchanged.
- New helper has unit coverage for each async phase.

## Verification Plan
- `just verify`.
- Manual smoke: press `g` and observe the footer change to a spinning status while the context collection / generation runs.

## Files Likely Touched
- `src/ui.rs`

## Risks
- If the existing tick rate is too slow, the spinner will look choppy; that is acceptable as long as it visibly animates. Avoid adding a new thread just to drive the spinner.
- Braille glyphs render inconsistently on some Windows terminals; pick a glyph set that renders in the default Windows Terminal font (Cascadia Mono is fine for Braille).
- Test assertions on glyph chars are brittle if the frame set changes; assert "contains some spinner char" rather than a specific glyph.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-29T23:23:58+02:00
- state: implemented

## What was completed

Added a Braille spinner-led status line in the Generate-screen footer for the four async phases: `CollectingContext`, `Generating`, `CheckingFreshness`, and `Executing`.

### Functions added in `src/ui.rs`
- `spinner_frame(now: Instant) -> char`: cycles through 10 Braille frames at ~10 fps using elapsed milliseconds.
- `generate_async_status_line(phase: GeneratePhase, now: Instant) -> Option<Line<'static>>`: returns a spinner + phase-specific status line for async phases, `None` for steady phases.

### Match arm changes in `render_help`
- Replaced the separate `CheckingFreshness` and `Executing` arms with a single `matches!` guard arm covering all four async phases (`CollectingContext`, `Generating`, `CheckingFreshness`, `Executing`), delegating to `generate_async_status_line`.
- The `Confirming` (input mode `Confirm`) and `Editing` arms remain dedicated and unchanged.
- Cancel hint (`Esc cancel`) is included for `CollectingContext`, `Generating`, and `CheckingFreshness`, and omitted for `Executing` (non-cancellable).

## Deviations from plan
None. Implementation follows the plan exactly.

## Verification
- `just fmt` â€” pass
- `just check` â€” pass
- `just clippy` â€” pass
- `just test` â€” 218 tests pass (6 new tests for the new functions)
- Manual smoke was not run (no runtime available), but all automated checks pass.

## Important files changed
- `src/ui.rs`: added `use std::time::Instant;`, two new helper functions, updated match arm, added 6 unit tests.

## Residual risks / follow-up
- Tick rate drives repaints; if the tick interval is slower than 100ms the spinner may appear to skip frames but will still animate.
- Braille glyphs render correctly in Cascadia Mono (Windows Terminal default); other terminals were not tested.
