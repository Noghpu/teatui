---
id: 0001j-2026-06-05-71d3e932-bulk-review-left-right-pane-nav
created_at: 2026-06-05T21:13:52+02:00
created_by_model: claude-opus-4-8/high
state: reviewed
state_updated_at: 2026-06-05T21:28:20+02:00
---
# Bulk review modal: left/right arrow navigation between the two panes

## Goal
Let the user move focus between the two boxes of the stacked-PR bulk review modal with the Left and Right arrow keys, in addition to the existing Enter and Esc. Left moves focus to the left `Stack` pane; Right moves focus to the right `Selected PR` pane. This makes the master-detail layout navigable the way its side-by-side shape implies.

## Context
The active rewrite is Linux-only and the Generate screen owns the stacked-PR review modal. Read `docs/rewrite-plan.md` and `AGENTS.md` before changing behavior.

The modal's two-pane focus is tracked by `BulkReviewFocus::{List, Preview}` on `GenerateState` (`src/screens/generate.rs`). Key handling lives in `on_bulk_review_key` in `src/screens/generate/input.rs` (around line 215).

Today, crossing between the two boxes only happens via:

- `Enter` while focus is `List` (`input.rs:291`): flushes the editor to the plan, seeds the editor from the cursor, and sets focus to `Preview`.
- `Esc` while focus is `Preview` (`input.rs:253-256`): sets focus back to `List` (no flush; Esc from `List` instead closes the modal).

Within `Preview`, `Up/Down/j/k` move the per-PR field focus; within `List` they move the selected PR. `Tab`/`BackTab` move field focus inside `Preview`. `Enter`/`i` in `Preview` begins editing the focused field, after which keys route to `on_bulk_editor_key` and the modal-level handler is bypassed (so Left/Right inside an active editor still reach the textarea — see Non-Goals).

While a push is in flight, `on_bulk_review_key` enters a restricted branch (`input.rs:220-246`) that only allows `Up/Down/j/k` navigation; all mutating and modal-closing actions are disabled.

The footer key hints for the modal are built in `generate.rs` around line 2403-2434 (the `BulkPhase::Review { .. }` arm of the help-hints function).

Relevant existing tests in `src/screens/generate/input.rs`:
`enter_from_bulk_review_list_focuses_preview_without_editing`,
`esc_from_bulk_review_preview_returns_to_list`,
`bulk_review_navigation_stays_live_while_push_is_running`,
`bulk_review_mutating_and_closing_keys_are_disabled_during_push`.

## Non-Goals
Do not change Enter/Esc behavior — they continue to work exactly as today. Left/Right are additive.

Do not change the in-editor behavior. When a field is actively being edited (`state.bulk_editor.editing == true`), `on_bulk_editor_key` already handles the key and Left/Right move the textarea cursor; do not intercept arrows there.

Do not change list scrolling, per-PR field focus movement, push (`p`/`P`), or any other Generate behavior. Do not touch the description-cursor / shared-renderer work (that is a separate ticket).

Do not add Left/Right box switching while a push is in flight: the push branch stays navigation-only (`Up/Down/j/k`) exactly as today, consistent with Enter/Esc being disabled during push.

## Design Decisions
Add two new key arms to the non-push, non-editing `match` in `on_bulk_review_key`:

- `KeyCode::Right`: if focus is `List`, move to `Preview`. This must mirror the existing `Enter`-from-`List` transition exactly: `state.flush_bulk_editor_to_plan(); state.seed_bulk_editor_from_cursor(); state.bulk_review_focus = BulkReviewFocus::Preview;` and return `Transition::Dirty`. If focus is already `Preview`, do nothing (`Transition::None`). Right must NOT begin field editing — it only moves box focus, matching Enter-from-List.
- `KeyCode::Left`: if focus is `Preview`, move to `List`. This must mirror the existing `Esc`-from-`Preview` transition: set `state.bulk_review_focus = BulkReviewFocus::List;` and return `Transition::Dirty`. (Esc-from-Preview does not flush, so Left does not flush either; consistency is intentional.) If focus is already `List`, do nothing — Left must NOT close the modal (only Esc-from-List closes).

Place these arms so they do not collide with the existing `Up/Down/j/k`, `Enter`, `Tab`/`BackTab`, `p`/`P`, and `Esc` arms. Both new arms are unconditional on `key.code` but branch internally on `state.bulk_review_focus`.

Because the active-editor case returns early via `if state.bulk_editor.editing { return on_bulk_editor_key(...) }` (`input.rs:248`), the new arms are naturally unreachable during editing, so the textarea keeps Left/Right for cursor movement. Do not add Left/Right to `on_bulk_editor_key`.

Update the modal footer hints (`generate.rs` ~2417-2434) so the navigable directions are discoverable:

- In the `List`-focus hint set, surface that Right (or Enter) opens the preview, e.g. change the primary hint to read `Enter/→` for `preview`.
- In the `Preview`-focus hint set, surface that Left (or Esc) returns to the list, e.g. show `Esc/←` for `list`.

Keep the hint list compact and within the existing style; do not add a separate standalone arrow hint that crowds the footer. Match the existing `theme::HelpHint` usage and the `Enter/i` style already used for combined keys.

## Implementation Plan
1. In `src/screens/generate/input.rs`, `on_bulk_review_key`, add `KeyCode::Right` and `KeyCode::Left` arms to the main (non-push, non-editing) `match key.code` block, implementing the focus transitions described in Design Decisions. Reuse the exact mutation sequence from the existing `Enter`-from-`List` and `Esc`-from-`Preview` paths so the seeding/flush behavior cannot drift.
2. Leave the push-in-flight restricted branch unchanged (Left/Right deliberately not added there).
3. In `src/screens/generate.rs`, update the `BulkPhase::Review { .. }` help-hint arms for `List` focus and `Preview` focus to mention the arrow keys alongside Enter/Esc.
4. Add unit tests in `src/screens/generate/input.rs` mirroring the existing focus tests:
   - Right from `List` focus moves to `Preview` without entering edit mode (assert `bulk_review_focus == Preview` and `bulk_editor.editing == false`).
   - Left from `Preview` focus returns to `List`.
   - Right while already `Preview` is a no-op for focus; Left while already `List` does NOT close the modal (assert `bulk` is still `Review`).
   - Left/Right are ignored (or routed to the editor, not focus changes) while a push is in flight: assert focus does not jump boxes in the push branch.
5. Run `just verify`. Run `just snapshots` only if you change anything that alters rendered output (the footer hint text does affect the Generate footer snapshot, so refresh and eyeball it).

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "docs/tickets/reviewed/0001i-2026-06-05-ccf51e1f-bulk-review-boxed-metadata.md"
  ],
  "likely_files": [
    "src/screens/generate/input.rs",
    "src/screens/generate.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "Left/Right move box focus exactly mirroring Esc-from-Preview and Enter-from-List, including the flush+seed on entering Preview.",
    "Right does not begin field editing; Left does not close the modal.",
    "Left/Right do not change box focus while a push is in flight (push branch stays navigation-only).",
    "When a field is actively being edited, Left/Right still move the textarea cursor and are not intercepted as box navigation.",
    "Footer hints mention the arrow keys without crowding the hint line."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- In the bulk review modal with focus on the `Stack` (list) pane, pressing Right moves focus to the `Selected PR` (preview) pane, seeding the editor from the selected PR, without starting field editing.
- With focus on the `Selected PR` pane (not editing), pressing Left moves focus back to the `Stack` pane.
- Right while already on `Preview`, and Left while already on `List`, do not change panes and do not close the modal.
- Enter and Esc continue to behave exactly as before.
- While editing a field, Left and Right move the text cursor and do not change pane focus.
- While a push is in flight, Left/Right do not switch panes.
- The modal footer hints communicate that the arrow keys navigate between panes.
- `just verify` passes; the Generate footer snapshot reflects the updated hints.

## Verification Plan
Run `just verify` for fmt, compile, clippy (`-D warnings`), unit tests, and render smoke tests. The new `input.rs` unit tests must pass.

Run `just snapshots` because the footer hint text changes; inspect `target/ui-snapshots/index.html` and the relevant Generate snapshot to confirm the hint line still fits and reads cleanly.

## Files Likely Touched
- `src/screens/generate/input.rs`
- `src/screens/generate.rs`

## Risks
The active-editor early return is what keeps Left/Right available to the textarea cursor; if the new arms were placed before that guard they would steal arrow keys from editing. Keep the new arms inside the post-guard match.

Footer hint width: adding `Enter/→` and `Esc/←` lengthens the hint line. Verify it still fits at the modal/footer width and degrades gracefully on narrow terminals via the existing `theme::help_line` width handling.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-05T21:24:03+02:00
- state: implemented

## What was completed

Added Left/Right arrow key navigation between the two panes of the bulk review modal in src/screens/generate/input.rs. Right from List focus mirrors the Enter-from-List transition exactly (flush + seed + set focus to Preview, no editing). Left from Preview mirrors Esc-from-Preview (set focus to List, no flush). Both are no-ops when focus is already on the target pane. Neither is active during push-in-flight (the restricted branch only handles Up/Down/j/k). The active-editor early return means Left/Right in edit mode reach the textarea cursor handler as before.

Updated footer hints in src/screens/generate.rs: List focus now shows "Enter/right-arrow preview" and Preview focus shows "Esc/left-arrow list".

## Deviations from plan

None. Implementation followed the plan precisely.

## Verification

just verify passed: fmt, compile, clippy (-D warnings), 179 unit tests plus 50 render smoke tests all green.

just snapshots ran successfully (20 snapshots written). The generate-bulk-review.txt snapshot footer shows the updated hint and fits cleanly within the existing footer width.

## Important files changed

- src/screens/generate/input.rs: Added KeyCode::Right and KeyCode::Left arms in on_bulk_review_key; added 5 new unit tests.
- src/screens/generate.rs: Updated List-focus hint to Enter/right-arrow and Preview-focus hint to Esc/left-arrow.

## Residual risks / follow-up

None. The arrow key guard structure is consistent with the existing early-return for editing and the push-in-flight restricted branch.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-05T21:28:20+02:00
- state: reviewed

Metadata:
- reviewer model: claude-opus-4-8
- verdict: approved, no fixes required

## Summary

Ticket 0001j adds Left/Right arrow navigation between the two panes of the
bulk review modal. The implementation matches the plan precisely and passes
all checks. No code changes were needed during review.

## What I verified (facts)

- `KeyCode::Right` arm (src/screens/generate/input.rs:297-307) mirrors the
  existing Enter-from-List transition byte-for-byte: flush_bulk_editor_to_plan
  + seed_bulk_editor_from_cursor + set focus Preview, returning Transition::Dirty.
  No-op (Transition::None) when already Preview. It does NOT begin field editing.
- `KeyCode::Left` arm (input.rs:308-316) mirrors Esc-from-Preview: set focus
  List, Transition::Dirty (no flush, intentionally consistent with Esc). No-op
  when already List; it does NOT close the modal.
- Both arms are inside the main match, after the push-in-flight restricted
  branch (input.rs:220-246, navigation-only) and after the active-editor early
  return (input.rs:248-250). So Left/Right are correctly unreachable during a
  push and during active field editing, where they fall through to the textarea
  cursor handler. This satisfies the key non-goals.
- Footer hints updated (src/screens/generate.rs:2419 and 2432): List focus shows
  "Enter/→ preview", Preview focus shows "Esc/← list". Compact, matching the
  existing "Enter/i" combined-key style.
- Five new unit tests added (input.rs:1000-1066) covering: Right->Preview without
  editing; Left->List; Right-no-op-on-Preview; Left-does-not-close-on-List;
  Left/Right ignored during push (both codes, asserts focus unchanged + None).
- `just verify` passes: fmt, clippy -D warnings (clean), 179 unit tests, 50
  render smoke tests, all green.

## Notes / clarification (inference)

- The implementation note says "the generate-bulk-review.txt snapshot footer
  shows the updated hint." In this repo `just snapshots` writes to
  target/ui-snapshots, which is gitignored (/target/), so it is a visual review
  artifact, not a tracked golden file. The absence of a snapshot file in the
  implementation diff is therefore correct, not a missing change. Footer text is
  covered by the render_smoke tests, which pass.

## Acceptance criteria

All criteria met: Right opens preview (seeded, no edit); Left returns to list;
no-ops on the already-focused pane without closing; Enter/Esc unchanged;
Left/Right are textarea cursor moves while editing; Left/Right do not switch
panes during push; footer hints communicate the arrows; `just verify` passes.

No fixes applied — implementation is correct, minimal, and idiomatic.
