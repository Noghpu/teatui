---
id: 00016-2026-05-29-6abd1bd0-generate-press-feedback
created_at: 2026-05-29T22:38:43+02:00
created_by_model: claude-opus-4-7/high
state: open
---
# Inline blocker banner and auto-focus Preview on `g`

## Goal
Make pressing `g` on the Generate screen visibly do something: surface validation blockers as a red one-line banner in the Form pane and, on success, move focus to the Preview pane so the user sees the async progress that already renders there.

## Context
`generate_pr` in `src/app.rs` (around line 1022) runs `validate_form` and `blocking_errors`. When blockers are present it calls `fail_context_collection(message)` and logs â€” but the only UI surface that changes is the Preview's `Failed`/`work-title` text and a log line. The Form pane shows nothing. On success the Preview pane renders `"Collecting contextâ€¦"` / `"Generating draftâ€¦"` but focus stays where it was, so the user (typically in the Form pane) sees no change. This ticket closes both gaps.

## Non-Goals
- No spinner animation; that lives in `phase-aware-footer-spinner`.
- No changes to the actual generation pipeline or LLM calls.
- No new error categories â€” only surface the existing blockers.
- No banner outside the Generate screen.

## Design Decisions
- Store the blocker text on `GenerateState` as `pub last_blocker: Option<String>`. Set on validation failure inside `generate_pr`, cleared when:
  - The user begins editing any form field (`begin_editing_selected_field`).
  - A phase transition to `CollectingContext`, `Generating`, or `Complete` succeeds.
  - The Menu pane writes a new revset selection (force-sync from `enter-force-syncs-head`, if present).
- Render the banner in `src/ui.rs` above the form-field list when `last_blocker.is_some()`. Use `colors::BAD` (or the closest existing red); single line, truncated with ellipsis if narrow.
- After a successful path through `generate_pr` (either `start_context_collection` or `start_generation`), set `self.focus = Focus::Preview` to mirror `confirm_execution` at `src/app.rs:1088`. Do **not** move focus on the validation-blocker branch.
- Do not reuse `generation_error` for blockers â€” the two have different lifecycles. Keep them separate.

## Implementation Plan
1. In `src/generate.rs`, add `pub last_blocker: Option<String>` to `GenerateState` and initialize it to `None`. Add `set_blocker(&mut self, msg: impl Into<String>)` and `clear_blocker(&mut self)`. Wire `clear_blocker` into `begin_editing_selected_field`, `begin_context_collection`, `begin_generation`, and (if present after merge) the force-sync path.
2. In `src/app.rs::generate_pr` (around line 1022), on the blockers branch:
   - Call `self.generate.set_blocker(blockers.join("; "))`.
   - Keep `self.focus = Focus::Form` so the banner is visible.
   - Keep `fail_context_collection` (it sets `Failed` phase, which the Preview pane still benefits from).
3. In the success branches inside `generate_pr` (both the `start_generation` and `start_context_collection` paths), set `self.focus = Focus::Preview` after kicking off the async work.
4. In `src/ui.rs`, in the Form-pane render block, when `app.generate().last_blocker.is_some()`, render a one-line `Line::from(...)` styled with `colors::BAD` above the field list.
5. Add two tests in `src/app.rs`:
   - `pressing_g_with_blocker_records_inline_banner_and_keeps_focus_on_form`: induce a blocker (e.g. clear head), press `g`, assert `app.generate().last_blocker.is_some()` and `app.focus == Focus::Form`.
   - `pressing_g_with_valid_form_moves_focus_to_preview`: with a valid form and a stubbed context path, press `g`, assert `app.focus == Focus::Preview` and `last_blocker.is_none()`.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/app.rs", "src/generate.rs", "src/ui.rs"],
  "likely_files": ["src/app.rs", "src/generate.rs", "src/ui.rs"],
  "verification_commands": ["just fmt", "just check", "just clippy", "just test", "just verify"],
  "review_focus": [
    "Blocker banner visible above the Form when validation fails.",
    "Banner is cleared on edit, on successful phase transition, and on Menu revset re-selection.",
    "Focus auto-moves to Preview on successful g; stays on Form on the blocker branch.",
    "No banner state survives a full successful generation run."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- Pressing `g` with one or more validation blockers renders a red one-line banner above the Form fields and keeps focus on the Form pane.
- Pressing `g` with a valid form clears any prior banner and moves focus to the Preview pane.
- Starting to edit any form field clears the banner.
- The two new tests pass and existing tests are unaffected.

## Verification Plan
- `just verify`.
- Manual smoke: clear the head field, press `g`, confirm the banner; then fill head, press `g`, confirm focus is on Preview with the existing `Collecting contextâ€¦` text.

## Files Likely Touched
- `src/app.rs`
- `src/generate.rs`
- `src/ui.rs`

## Risks
- If `head-field-as-change-id` lands first, the blocker text will mention change_ids; if it has not landed, the same text path still works because blockers are field error strings, not value strings.
- Be careful not to clear the banner inside `validate_form` â€” clearing must be tied to user action (edit / press / select), not to a re-validation that happens to find no errors.
- Confirm `colors::BAD` (or equivalent) exists in `src/colors.rs`. If not, reuse the closest red.
