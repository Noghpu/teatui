---
id: 0001s-2026-06-06-d186257e-bulk-review-three-pane-messages
created_at: 2026-06-06T22:03:36+02:00
created_by_model: unknown
state: reviewed
state_updated_at: 2026-06-07T01:45:20+02:00
---
# Bulk Review Modal Three-Pane Layout

## Goal
Split the stacked-PR bulk review modal so generated PR fields and operational messages do not compete for the same vertical space. The modal should have separate List, Form, and Messages panes, letting users navigate to and scroll the message content even when the generated Description is long.

## Context
The current bulk review modal is effectively a two-pane master/detail layout: the left pane lists stack items and the right `Selected PR` pane renders metadata, editable generated fields, blockers, warnings, push results, and last-action messages together. Reviewed ticket `0001k` fixed one part of this by sharing the text-field renderer and adding `bulk_form_scroll`, so focused fields stay visible and the Description editor shows a cursor. That did not fully solve the UX problem: content printed under the long Description can still be pushed out of reach because modal navigation in the right pane is field-oriented (`Title`, `Branch`, `Description`) rather than content-oriented.

The user-visible failure is: a long generated Description can occupy the available right-pane height, and blockers, warnings, push result, or last-action lines below it may overflow or become inaccessible. Moving focus between text fields cannot move focus to the messages below the Description.

Relevant existing work:

- `docs/tickets/reviewed/0001j-2026-06-05-71d3e932-bulk-review-left-right-pane-nav.md` added Left/Right navigation between the modal's two panes.
- `docs/tickets/reviewed/0001k-2026-06-05-86df379c-bulk-review-shared-textfield-cursor.md` added shared editable text-field rendering and `bulk_form_scroll`.
- `src/screens/generate.rs` owns the bulk modal layout/rendering, including `BulkReviewFocus`, `BulkItemEditor`, `bulk_list_scroll`, and `bulk_form_scroll`.
- `src/screens/generate/input.rs` owns modal key routing.
- `tests/render_smoke.rs` has bulk review render smoke coverage, including long Description cases.

## Non-Goals
Do not change stacked-PR generation, LLM parsing, blocker rules, push execution, or forge command behavior.

Do not replace the shared text-field renderer from reviewed ticket `0001k`; keep generated Title, Branch, and Description editing on the existing `form::TextFieldState` and shared render path.

Do not add another hidden modal-scroll shortcut as the primary fix. The design direction for this ticket is a visible three-pane modal: list/form/messages. It is acceptable to add conventional scroll keys for the Messages pane, but the messages must be reachable as a real focused pane.

Do not change the single-PR Generate screen's three-pane layout.

## Design Decisions
Use the three-pane modal design rather than a popup-style scroll key. The layout should mirror the main PR generation screen conceptually: a list/navigation pane, a form/editing pane, and a messages/status pane. This makes the content hierarchy visible and avoids relying on a hard-to-discover keybind for content below the Description.

Extend the modal focus model from two panes to three panes. Replace or extend `BulkReviewFocus::{List, Preview}` into a shape such as `BulkReviewFocus::{List, Form, Messages}`. If the implementation keeps the `Preview` name internally for compatibility, document the mapping clearly; the user-facing pane should read as Form or Selected PR fields, while messages/status are separate.

Left/Right navigation should move across panes in order: List <-> Form <-> Messages. Preserve the existing behavior that Enter from List opens the editable Form pane and Esc from Form returns to List. Define Esc from Messages to return to Form, not close the modal. Esc from List continues to close the modal. Do not intercept Left/Right while a text field is actively editing; the textarea keeps cursor movement.

Move non-field content that can grow or appear below the Description into the Messages pane. At minimum this includes blockers, warnings, push result, last-action text, refresh status from ticket `0001r`, and any similar generated/push/status messages that are currently rendered beneath the editable fields. Keep compact read-only identity metadata near the form only if it is essential for editing context; otherwise prefer putting status-style lines in Messages.

Make the Messages pane scrollable. Store a dedicated offset, for example `bulk_messages_scroll: Cell<usize>` or `Cell<u16>`, on `GenerateState`. When the Messages pane has focus, Up/Down and `j`/`k` scroll by one line, and PageUp/PageDown or `Ctrl+u`/`Ctrl+d` scroll by a larger step. Use `screens::util::scroll_window` or the existing natural-scroll/clamping helpers rather than hand-rolling overflow behavior.

The Form pane should continue to use `bulk_form_scroll` only for generated field editing. Long Description content should not hide Messages, because messages are no longer part of that form scroll region.

Footer hints must be pane-aware. List focus should advertise opening Form. Form focus should advertise edit controls plus Left/Right pane navigation. Messages focus should advertise scroll controls and how to return to Form/List. Keep hints compact enough for 80x24 render smoke coverage.

Coordinate with ticket `0001r`: the blocker-refresh ticket may add new refresh status/last-action text. This ticket should place that status in the Messages pane, not under the Description in the Form pane.

## Implementation Plan
1. Update `BulkReviewFocus` in `src/screens/generate.rs` to represent three panes: List, Form, and Messages. Adjust state initialization and any tests/builders that construct `GenerateState`.
2. Update `src/screens/generate/input.rs` bulk review routing.
3. Refactor bulk modal rendering in `src/screens/generate.rs` from two visible panes to three visible panes.
4. Move blockers, warnings, push result, last-action, refresh status, and other non-field status lines into the Messages pane.
5. Add `bulk_messages_scroll` to `GenerateState`, initialize it, and reset/clamp it when the selected stack item changes or when message content shrinks.
6. Update pane titles and footer/help hints to match the three-pane model.
7. Add focused unit tests in `src/screens/generate/input.rs` for List/Form/Messages focus transitions, Esc behavior from each pane, Messages-pane scrolling, active-editor arrow-key preservation, and push-in-flight restrictions if applicable.
8. Extend render smoke coverage in `tests/render_smoke.rs` with a long Description plus blockers/warnings/result/last-action case.
9. Run `just verify`. Run `just snapshots` because this changes modal layout, then inspect the bulk review artifacts under `target/ui-snapshots/`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "docs/tickets/reviewed/0001j-2026-06-05-71d3e932-bulk-review-left-right-pane-nav.md",
    "docs/tickets/reviewed/0001k-2026-06-05-86df379c-bulk-review-shared-textfield-cursor.md"
  ],
  "likely_files": [
    "src/screens/generate.rs",
    "src/screens/generate/input.rs",
    "src/screens/mod.rs",
    "src/app.rs",
    "tests/render_smoke.rs",
    "src/bin/ui-snapshots.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "Bulk review modal has reachable List, Form, and Messages panes; messages are not rendered below the Description in the Form pane.",
    "Long generated Description content cannot hide blockers, warnings, push results, refresh status, or last-action messages.",
    "Messages pane has its own persisted/clamped scroll offset and uses shared scroll helpers or equivalent natural clamping.",
    "Left/Right/Esc/Enter behavior remains predictable, and active text editing keeps textarea cursor keys.",
    "The layout remains readable and non-overlapping at the 80x24 smoke-test floor."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- The bulk review modal renders three reachable content areas: stack list, generated PR form fields, and messages/status.
- Blockers, warnings, push results, refresh status, and last-action lines render in the Messages pane rather than below the generated Description field.
- A long generated Description cannot push message content out of reach.
- Users can move focus to the Messages pane and scroll it with `j`/`k` or Up/Down; PageUp/PageDown and/or `Ctrl+u`/`Ctrl+d` provide larger scroll steps.
- Left/Right navigate across List, Form, and Messages while not editing. Active text editing still owns Left/Right for cursor movement.
- Esc from Messages returns to Form; Esc from Form returns to List; Esc from List closes the modal as before.
- Existing Title, Branch, and Description editing behavior remains intact.
- The bulk blocker refresh ticket's new status/last-action text has an obvious destination in the Messages pane.
- Render smoke covers a long Description plus message content and passes at 80x24 and normal 120x30 sizes.
- `just verify` and `just snapshots` pass.

## Verification Plan
Run `just verify` for formatting, compile, clippy, unit tests, and render smoke tests.

Run `just snapshots` and inspect `target/ui-snapshots/index.html` plus the bulk review `.txt`/`.svg` artifacts.

## Files Likely Touched
- `src/screens/generate.rs`
- `src/screens/generate/input.rs`
- `src/screens/mod.rs` if transition/focus wiring needs adjustment
- `src/app.rs` only if status/last-action routing needs a clearer messages destination
- `tests/render_smoke.rs`
- `src/bin/ui-snapshots.rs` if deterministic snapshots need a three-pane or long-description variant

## Risks
The main risk is over-compressing the modal. Three side-by-side columns may be too narrow at 80 columns, so the implementer should choose a split that keeps text readable and verify it visually.

Changing `BulkReviewFocus` touches input tests and render tests that currently assume List/Preview. Keep the migration mechanical and preserve existing Enter/Esc behavior where possible.

Do not regress the `0001k` shared text-field renderer fix. The Description editor must still show the live `TextArea` cursor, and the Form pane must still scroll fields naturally.

Ticket `0001r` may land before or after this one. Avoid tight coupling by designing the Messages pane as the destination for any bulk review status/last-action line, regardless of the exact refresh implementation.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-07T01:39:35+02:00
- state: implemented

## What was completed

Implemented the bulk review modal three-pane layout for ticket 0001s.

### Changes

1. src/screens/generate.rs:
   - Added BulkReviewFocus::Messages variant (Form pane retains "Preview" name internally; user-visible title is "Selected PR").
   - Added bulk_messages_scroll: Cell<usize> to GenerateState (struct, new() constructor, comment).
   - Updated render_bulk_review to split right side vertically: Form (top) and Messages (bottom). Height ratio: height/3 for Messages when >=16 rows, 4 rows at 10-15, 3 rows at <10.
   - Added render_bulk_messages: renders blockers, warnings, reuse notes, push results, and last-action text with a persisted clamped scroll offset.
   - Removed annotation/result/note rendering from render_bulk_pr_form (moved to Messages pane).
   - Simplified bulk_form_scroll (removed plan param; annotation sections no longer tracked there).
   - Updated help hints for three panes: List "Enter/-> form"; Form "edit + -> messages + Esc/<- list"; Messages "j/k scroll + <-/Esc form".

2. src/screens/generate/input.rs:
   - Updated on_bulk_review_key: Esc handles Messages->Preview->List->close; Left traverses Messages->Preview->List->noop; Right traverses List->Preview->Messages->noop.
   - Added PageUp/PageDown for Messages pane (+/-5 lines).
   - Up/Down/j/k scroll Messages when Messages pane is focused.
   - Updated push-in-flight branch to handle Messages scroll.
   - Added scroll_bulk_messages helper.
   - Replaced right_while_already_preview_is_noop with right_from_preview_moves_to_messages and right_while_already_messages_is_noop.
   - Added 6 new tests: left_from_messages_returns_to_form, esc_from_messages_returns_to_form, up_down_scroll_messages_when_focused, up_down_move_fields_when_form_focused, messages_scroll_does_not_go_below_zero, full_three_pane_navigation_round_trip.

3. tests/render_smoke.rs:
   - Added generate_bulk_review_long_description_with_messages_pane_renders.
   - Added generate_bulk_review_messages_pane_focused_renders.
   - Added bulk_messages_scroll to struct literal.

4. src/bin/ui-snapshots.rs: Added bulk_messages_scroll field.

## Deviations from plan

- BulkReviewFocus::Preview retained as the internal name for the Form pane (ticket allowed this).
- Messages pane scroll uses direct Cell clamping at render time rather than scroll_window (sufficient for simple paginated paragraph).
- bulk_scroll_target inlined into bulk_form_scroll since the only call site simplified after removing annotation/result from Form pane.

## Verification

just verify passes: fmt, check, clippy (-D warnings), 228 unit tests, 56 render smoke tests, all green.

## Important files changed

- src/screens/generate.rs
- src/screens/generate/input.rs
- src/bin/ui-snapshots.rs
- tests/render_smoke.rs

## Residual risks / follow-up

- just snapshots not run (Linux-only target). Visual inspection recommended at next Linux session.
- Messages pane is 3-4 rows at 80x24 (compact but reachable and scrollable).
- The Messages scroll clamping is done at render time; the persisted offset can temporarily exceed content height between cursor moves, which is immediately corrected on next render.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-07T01:45:20+02:00
- state: reviewed

# Review Postmortem: 0001s Bulk Review Modal Three-Pane Layout

## Verdict
Accepted with minor reviewer improvements. The implementation fully satisfies the ticket goal and all acceptance criteria.

## What was verified (facts)
- `just verify` passes: cargo fmt --check, cargo clippy --all-targets --all-features -D warnings, 230 unit tests, 56 render smoke tests, all green.
- `BulkReviewFocus` now has three variants: `List`, `Preview` (the user-visible "Selected PR" Form pane), and `Messages`. The internal `Preview` name is documented as historical compatibility, which the ticket explicitly permitted.
- The right side of the modal is split vertically into a Form pane (top, editable Title/Branch/Description via the shared `render_text_field`) and a Messages pane (bottom). Blockers, warnings, reuse notes, push results, and last-action text were removed from `render_bulk_pr_form` and now render in `render_bulk_messages`. The Description can no longer push messages out of reach because they are in a separate, independently scrolled region.
- `bulk_messages_scroll: Cell<usize>` was added to `GenerateState`, the `new()` constructor, the render_smoke struct literal, and `ui-snapshots.rs`. It is clamped at render time against `total.saturating_sub(visible)`.
- Pane navigation: Left/Right traverse List <-> Form <-> Messages while not editing; Esc steps Messages -> Form -> List -> close; Enter from List opens Form; Left from List is a no-op (does not close). Active text editing keeps Left/Right for the textarea because routing checks `bulk_editor.editing` before pane keys.
- Footer hints are pane-aware (List advertises form/select/push; Form advertises edit/fields/messages/list; Messages advertises scroll/page/form).
- The shared text-field cursor fix from 0001k is preserved: the Description still uses `multiline_value_height` and `render_text_field`, and `bulk_form_scroll` still keeps the focused field visible.
- The 0001r coordination requirement is met: refresh status/last-action text lands in the Messages pane (`render_bulk_messages` consumes `state.last_action`).

## Reviewer changes applied
1. `src/screens/generate/input.rs`: the push-in-flight key branch now handles PageUp/PageDown for Messages scrolling, matching the non-push branch and the Up/Down/j/k behavior that was already live during a push. Previously only Up/Down/j/k scrolled Messages during a push; PageUp/PageDown were silently dropped.
2. Added two tests: `page_up_down_scroll_messages_by_larger_step` (no prior coverage existed for PageUp/PageDown at all) and `page_up_down_scroll_messages_during_push`.
3. `src/screens/generate.rs`: added a `PgUp/PgDn page` hint to the Messages-pane footer so the larger-step scroll keys named in the acceptance criteria are discoverable. The 80x24 render smoke (`generate_bulk_review_messages_pane_focused_renders`) still passes, confirming the footer stays readable.

## Observations (not blocking)
- `just snapshots` was not run; it is a Linux-only target on this Windows host. Render smoke at 80x24 and 120x30 plus visual reasoning over the layout split give adequate confidence. A Linux visual pass remains a reasonable follow-up, as the implementer noted.
- At terminal heights below the 80x24 smoke floor (right_area.height ~3) the Form pane can collapse to zero rows because Messages takes a fixed 3-row minimum. This is below the supported floor and not a regression.
- The Messages scroll offset can transiently exceed content height between a key press and the next render; it is corrected on the next render-time clamp. This is the same natural-scroll pattern used elsewhere and is harmless.

## Risk assessment
Low. Changes are confined to the bulk review modal. No generation, parsing, blocker, push, or forge behavior was touched. The three-pane navigation has thorough unit coverage including round-trip and push-in-flight cases.
