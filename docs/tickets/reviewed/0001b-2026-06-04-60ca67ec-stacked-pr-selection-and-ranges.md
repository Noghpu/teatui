---
id: 0001b-2026-06-04-60ca67ec-stacked-pr-selection-and-ranges
created_at: 2026-06-04T20:48:31+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-04T21:17:02+02:00
---
# Stacked PR: head multi-selection, domain types, and range derivation

Implemented in this revision. See implementation notes for details.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-04T21:09:50+02:00
- state: implemented

## What was completed

Implemented slice 1 of the stacked-PR feature: head multi-selection, domain types, and range derivation.

### Changes

1. src/domain/stack.rs (new): All Stack* domain types (StackIntent, StackPrInput, StackSelection, StackDraft, PrStatus, StackPlanItem, StackPlan), BulkPhase enum (#[default] Idle with stub variants for later slices), and the pure derive_stack_ranges function with 8 unit tests covering gap folding, base chaining, stale-id filtering, and order stability.

2. src/domain/mod.rs: Added pub mod stack declaration and re-exports for all new public types.

3. src/screens/generate.rs: Added selected_heads: Vec<String> and bulk: BulkPhase fields to GenerateState; initialized in new(); added toggle_selected_head, is_head_selected, and selected_heads_present helpers. Updated render_menu to show a Count selected title suffix. Updated revset_row_lines to accept and render both cursor and selected-head markers. Added space toggle head hint in normal_help_hints.

4. src/screens/generate/input.rs: Bound KeyCode::Char space in the Pane::Menu arm to toggle the cursor row change_id, gated behind !state.is_in_progress().

5. tests/render_smoke.rs: Added 5 render smoke tests for selected-heads states (zero, one, multiple, cursor+selected on same row, small terminal).

6. Updated all full GenerateState struct literals to include the two new fields.

### Deviations

None meaningful.

### Verification

- just verify green: fmt, check, clippy (-D warnings), 121 unit tests pass.
- just snapshots writes 12 artifacts without error.
- Render smoke tests expanded to 37 tests, all passing.

### Residual risks

- BulkPhase stub variants compile but are not wired - slice 4 will wire them.
- selected_heads_present filters stale ids at read time; callers must use it when stale-safety matters.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-04T21:17:02+02:00
- state: reviewed

## Review summary

Reviewed slice 1 of the stacked-PR feature (head multi-selection, `Stack*` domain
types, and pure range derivation). The implementation is correct, matches the
locked design in `docs/stacked-pr-plan.md`, and satisfies every acceptance
criterion in the ticket. I applied focused quality fixes and added one missing
test rather than filing complaints.

## Facts

- `just verify` is green: fmt, check, clippy (`-D warnings`), 122 unit tests, 37
  render smoke tests. `just snapshots` writes 12 artifacts cleanly.
- `derive_stack_ranges` is correct: stale ids are dropped, selection order does
  not affect output, gap changes fold into the later PR, bases chain
  `base -> prev-head`, and PR 0 reaches the oldest change. Verified by tracing
  the newest-first index math and by the 8 existing unit tests.
- `space` toggles the cursor row's `change_id` in the Changes pane only, gated
  behind `!state.is_in_progress()`. The Changes pane renders an orthogonal
  cursor + selected-head gutter and a `(N selected)` title suffix. Scroll
  clamping is untouched.
- All full `GenerateState` struct literals (render_smoke, ui-snapshots, two test
  modules) were updated for the new `selected_heads` / `bulk` fields.
- The `G review stack` footer hint is correctly deferred to slice 4; only the
  `space toggle head` hint was added, matching the plan's preference.

## Changes I made (applied directly)

1. `src/domain/stack.rs` â€” Rewrote the stream-of-consciousness comment block
   inside `derive_stack_ranges` (it contained a literal "Wait â€”" mid-thought
   rewrite and explained the same range three times) into a concise, accurate
   comment. No behavior change.
2. `src/domain/stack.rs` â€” Collapsed the redundant sort-ascending-then-reverse
   (two passes plus a needless `mut` rebind) into a single
   `sort_unstable_by(|a, b| b.cmp(a))` descending sort. Behavior identical;
   the order-independence and gap-folding tests still pass.
3. `src/screens/generate.rs` â€” Added a unit test
   `selected_heads_present_follows_display_order_and_drops_stale_ids` covering
   the acceptance criterion "selection is unaffected by reordering/refreshing
   the revset list; stale ids are dropped." This path (`selected_heads_present`)
   had no direct test and the implementer flagged it as a residual risk.

## Inferences (not verified end-to-end)

- `is_head_selected` is an O(n) linear scan called per row in `render_menu` and
  per revset in `selected_heads_present`, making selection rendering O(n*m). For
  the Changes pane (changes above trunk) n and m are small, so this is not worth
  a HashSet rewrite now; I left it as-is to avoid churn. Flagging in case the
  later slices grow the selection set.
- The `BulkPhase` stub variants (`Collecting`, `Generating`, `Review`, `Failed`)
  compile and default to `Idle` but are unwired, as intended for slice 1. They
  carry `#[derive(Default)]` only (no `PartialEq`) unlike the sibling
  `GeneratePhase`; that is fine for now since nothing compares them yet, and
  slice 4 will finalize the shape.

## Residual risks

- None blocking. The slice is data/selection-only; no LLM/modal/push paths are
  reachable yet, so the unwired `BulkPhase` variants cannot misfire.
