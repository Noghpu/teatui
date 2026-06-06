---
id: 0001s-2026-06-06-0d331a39-bulk-bookmark-conflict-same-change-reuse
created_at: 2026-06-06T22:05:43+02:00
created_by_model: claude-opus-4-8/medium
state: reviewed
state_updated_at: 2026-06-07T01:29:03+02:00
---
# Bulk PR gen: reuse a bookmark already on its own change instead of blocking

## Goal
In the stacked / bulk PR generation flow, a suggested bookmark that already
exists **on the same change it would be created on** must NOT count as a
blocker â€” the existing bookmark should simply be reused. A bookmark conflict
should only block when the suggested name already exists pointing at a
**different** change (or exists with an unknown target we cannot prove is this
change).

This matches jj's model: multiple distinct bookmarks on one commit is legal and
common, and bookmarks are deterministically re-derived from the draft
(`bookmark_from_draft_fields`), so re-running gen/review after the changes were
already bookmarked reproduces the same name on the same change. Today that is
wrongly flagged as `bookmark <name> already exists`, blocking push.

## Context
- `src/domain/stack.rs` `annotate_blockers` is the pure shared check run both
  when the plan enters Review and again before each push
  (`App::refresh_stack_review_blockers`, `src/app.rs:1450`). It currently takes
  a flat `local_bookmarks: &[String]` (names only) and at `src/domain/stack.rs:121`
  blocks whenever the item's bookmark name appears in that set â€” with no
  knowledge of which change the existing bookmark sits on.
- `collect_stack_bookmarks` (`src/app.rs:1489`) flattens away the
  bookmarkâ†’change association even though it has it: `status.revsets`
  (`Revsets::Loaded(items)`) yields `RevsetSummary` entries with
  `change_id` / `change_ids` and `bookmarks` (`src/domain/probe.rs:296`), so we
  know which change each in-revset bookmark is on. `status.base_bookmarks`
  (`BaseBookmark`, `src/domain/probe.rs:521`) carries only `name` + `remote` â€”
  no target change.
- Each plan item targets a head change via `item.input.head` (a change id; see
  reviewed ticket `00015-head-field-as-change-id`). The bookmark for an item is
  placed on that head change.
- The existing-PR blocker (`format_existing_pr_blocker`), the
  duplicate-within-plan blocker (two plan items deriving the same name), the
  precedence/order blocker (`annotate_order_blockers`), and the
  completed-item self-block guard all stay exactly as they are. This ticket only
  changes the **local-bookmark-exists** check.
- `item.warnings` is owned by plan-build time (LLM fallback / cache messages,
  `src/app.rs:874-877`) and is rendered with a `~` prefix. `annotate_blockers`
  resets only `blockers`, not `warnings`, and runs repeatedly â€” so the reuse
  note must NOT be appended to `warnings` (it would duplicate on every re-check
  and mix with LLM warnings). Use a dedicated note vector that
  `annotate_blockers` owns and clears, alongside `blockers`.

## Non-Goals
- Do not change how bookmarks are *derived* (`bookmark_from_draft_fields`).
- Do not change existing-PR, order, or duplicate-within-plan blocker logic.
- Do not add new probes or change what `jj` commands are run; reuse the
  already-probed `status.revsets` and `status.base_bookmarks`.
- Do not attempt to fetch targets for base/remote bookmarks (no target probe).

## Design Decisions
- **Reuse rule.** For a `Pending` item with bookmark `name` and head change
  `head`:
  1. If `name` is on `head` per the revset mapping â†’ **reuse**: add a
     non-blocking note `reusing existing bookmark <name>`, no blocker.
  2. Else if `name` exists anywhere (revset bookmarks âˆª base bookmark names)
     but is not confirmed on `head` â†’ **conflict** blocker:
     `bookmark <name> already exists on another change`.
  3. Else â†’ no blocker (clean).
- **Unknown target = conflict (block).** Decided with the user: a bookmark on
  `head` always shows up in the revset entry for that change, so an
  existing-but-unconfirmed name (e.g. a base/remote bookmark, or a bookmark
  outside the selected revset) is almost certainly elsewhere â€” keep it blocking.
  This is the safe default and preserves current behavior for the
  "names only, no target" callers/tests.
- **Reuse shown as a non-blocking note.** Decided with the user. Render it like
  a warning (not red, not counted as a blocker) so the user sees why no new
  bookmark is created. Put it in a new `StackPlanItem` field
  (e.g. `reuse_notes: Vec<String>`) that `annotate_blockers` clears and
  repopulates each call, mirroring its existing ownership of `blockers`. Do not
  reuse the LLM-owned `warnings` field.
- **Signature.** Change `annotate_blockers` to also receive the bookmarkâ†’change
  mapping. Suggested shape: keep an "exists" name set and add
  `bookmark_targets: &HashMap<String, Vec<String>>` (name â†’ change ids it is on,
  built from revset entries). The implementer may choose the exact parameter
  types but must keep the function pure and side-effect-free beyond rewriting
  `blockers` + the new note vector.
- **Change-id matching.** Compare `item.input.head` against the revset entry
  change ids. Be tolerant of short vs full change-id form (prefix match if a
  direct equality is not guaranteed); the revset template emits
  `change_id.short()`. Document the chosen comparison in a comment.
- **Build the mapping in `collect_stack_bookmarks` (or a sibling).** Walk
  `status.revsets` to build name â†’ change-ids; build the "exists" set from
  revset bookmarks plus `status.base_bookmarks` names. Update
  `refresh_stack_review_blockers` to pass both into `annotate_blockers`.

## Implementation Plan
1. `src/domain/stack.rs`:
   - Add `reuse_notes: Vec<String>` (or similarly named) to `StackPlanItem`.
   - Change `annotate_blockers` signature to accept the bookmarkâ†’change mapping
     plus the existing-names set. Clear both `blockers` and `reuse_notes` at the
     top of each item loop. Replace the `local_bookmarks.contains(...)` branch
     (current `src/domain/stack.rs:121-124`) with the reuse rule above.
   - Update the existing unit tests to the new signature and rename/adjust
     expectations: `local_and_remote_bookmarks_block` (names with no target â†’
     still block as "another change"), and add new tests:
     `bookmark_on_same_change_is_reused_not_blocked` (mapping says name is on the
     item's head â†’ no blocker, one reuse note) and
     `bookmark_on_different_change_blocks` (mapping says name is on another change
     â†’ blocker). Keep `existing_prs_take_precedence_over_bookmark_collisions`,
     `clean_plan_has_no_blockers`, `completed_items_do_not_self_block_on_live_bookmarks`,
     and the duplicate / order tests passing.
2. `src/app.rs`:
   - Build the bookmarkâ†’change mapping and exists-set (extend or add a sibling to
     `collect_stack_bookmarks`, `src/app.rs:1489`) from `status.revsets` and
     `status.base_bookmarks`.
   - Update `refresh_stack_review_blockers` (`src/app.rs:1450`) and the
     `build_plan_items` call so new `StackPlanItem`s initialize the new note
     field.
3. `src/screens/generate.rs`: render the reuse note in the bulk review modal as a
   non-blocking line (style like the existing `~`/warning rows at
   `src/screens/generate.rs:1719` and `:1885`), distinct from the red blocker
   rows; make sure it does not increment the header blocker count.
4. `src/bin/ui-snapshots.rs` and `tests/render_smoke.rs`: update `StackPlanItem`
   literals for the new field; fix the `annotate_blockers` call in
   `generate_bulk_review_with_bookmark_collision_blocker_renders`
   (`tests/render_smoke.rs:842`) to the new signature (names-only â†’ still a
   blocker). Add a render-smoke case for an item showing a reuse note so the
   non-blocking note path is exercised.
5. Run `just verify` and `just snapshots` (UI changed).

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "AGENTS.md", "docs/rewrite-plan.md", "docs/tickets/reviewed/0001f-2026-06-04-5a1b7c73-stacked-pr-collision-existing-pr-checks.md"],
  "likely_files": ["src/domain/stack.rs", "src/app.rs", "src/screens/generate.rs", "src/domain/probe.rs", "tests/render_smoke.rs", "src/bin/ui-snapshots.rs"],
  "verification_commands": ["just verify", "just snapshots"],
  "review_focus": ["annotate_blockers reuse-vs-conflict logic and the unknown-target=conflict default", "change-id short/full matching between item.input.head and RevsetSummary change ids", "reuse note uses a dedicated field, not the LLM-owned warnings vec, and is idempotent across re-checks", "reuse note renders as non-blocking and is excluded from the header blocker count"],
  "jj_description_prefix": "fix"
}
```

## Acceptance Criteria
- A suggested bookmark already present on the item's own head change produces no
  blocker and a single non-blocking `reusing existing bookmark <name>` note,
  stable across repeated `annotate_blockers` calls.
- A suggested bookmark present on a different change produces a
  `... already exists on another change` blocker.
- A bookmark that exists only with an unknown target (base/remote bookmark, or
  outside the selected revset) still blocks.
- Existing-PR, duplicate-within-plan, order, and completed-item behaviors are
  unchanged.
- The reuse note is not counted in the header blocker count and renders
  distinctly from blockers.

## Verification Plan
- `just verify` (fmt, check, clippy `-D warnings`, tests).
- Unit tests in `src/domain/stack.rs` for: same-change reuse (no blocker + one
  note), different-change conflict, unknown-target conflict, idempotent
  re-invocation, and unchanged existing-PR/duplicate/order cases.
- Render-smoke in `tests/render_smoke.rs` for both a blocker row and a reuse-note
  row.
- `just snapshots` to visually confirm the reuse note styling.

## Files Likely Touched
- `src/domain/stack.rs`
- `src/app.rs`
- `src/screens/generate.rs`
- `tests/render_smoke.rs`
- `src/bin/ui-snapshots.rs`

## Risks
- **Change-id form mismatch.** `item.input.head` vs `RevsetSummary.change_id`
  (short) could differ in length; a naive equality may misclassify a real reuse
  as a conflict. Use prefix-tolerant matching and cover it with a test.
- **Head change outside revset.** If an item's head is not represented in
  `status.revsets`, the mapping won't confirm reuse and it falls through to the
  conflict branch (acceptable per the unknown-target decision, but note it).
- **Re-check duplication.** `annotate_blockers` runs repeatedly; the new note
  field must be cleared each call like `blockers`, or notes accumulate.
- **Snapshot/literal churn.** Adding a `StackPlanItem` field touches several test
  and fixture literals; missing one breaks compilation (caught by `just check`).
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-07T01:24:30+02:00
- state: implemented

## What was completed

Implemented the bookmark-reuse logic for the bulk PR generation flow.
A suggested bookmark that already exists on the same change as the plan item's head is now treated as a non-blocking reuse rather than a conflict blocker.

## Key changes

- `src/domain/stack.rs`:
  - Added `reuse_notes: Vec<String>` field to `StackPlanItem` (cleared each `annotate_blockers` call like `blockers`).
  - Changed `annotate_blockers` signature to accept `bookmark_targets: &HashMap<String, Vec<String>>` (bookmark name â†’ change ids from revset entries).
  - Reuse rule: if bookmark name is in `bookmark_targets` and a target change id prefix-matches `item.input.head` â†’ reuse note, no blocker. Otherwise â†’ "already exists on another change" blocker. Names only known via `local_bookmarks` (no target) continue to block as unknown-target (safe default).
  - Added unit tests: `bookmark_on_same_change_is_reused_not_blocked`, `bookmark_on_same_change_is_idempotent`, `bookmark_on_different_change_blocks`, `bookmark_prefix_match_triggers_reuse`.
  - Updated existing tests for new signature and updated blocker message text.

- `src/app.rs`:
  - Added `collect_bookmark_targets` function that builds name â†’ change_ids map from `status.revsets`.
  - Updated `refresh_stack_review_blockers` to call `collect_bookmark_targets` and pass the result to `annotate_blockers`.
  - Updated `build_plan_items` to initialize `reuse_notes: Vec::new()`.

- `src/screens/generate.rs`:
  - Updated `bulk_annotation_lines` to render `reuse_notes` with `"  ~ "` prefix using `theme::muted()` style.
  - Updated the row flag logic to show `~` when `reuse_notes` is non-empty (just like `warnings`).
  - Header blocker count (`blocker_count`) only sums `item.blockers` â€” no change needed.

- `tests/render_smoke.rs`: Added `reuse_notes` field to `StackPlanItem` literals; updated `annotate_blockers` calls to new signature; added `generate_bulk_review_with_bookmark_reuse_note_renders` smoke test.

- `src/bin/ui-snapshots.rs`: Added `reuse_notes` field to `StackPlanItem` literals.

- `src/screens/generate/input.rs`: Added `reuse_notes` field to `StackPlanItem` literals in tests.

## Verification

`just verify` passed (fmt, check, clippy -D warnings, 221 unit tests, 54 render-smoke tests).
`just snapshots` ran cleanly (20 snapshots written).

## Deviations from plan

None material. Used `is_some_and` instead of `map_or(false, ...)` per clippy lint.

## Risks

- If `item.input.head` is a full-length change id and the revset emits a short id, the prefix match handles it correctly. The reverse (revset emits full, item has short) is also handled.
- If an item's head is outside the loaded revsets, `bookmark_targets` will have no entry for that change, so the name-only path (conflict/block) applies â€” acceptable per the unknown-target decision.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-07T01:29:03+02:00
- state: reviewed

## Review outcome

Accepted as implemented. No code changes were required during review; the implementation matches the plan and all acceptance criteria.

## Verification (facts)

- `just verify` passed: fmt, check, clippy `-D warnings`, 221 unit tests, 54 render-smoke tests, all green.
- `just snapshots` wrote 20 snapshots cleanly (target dir is gitignored, no repo churn).
- Working copy clean throughout; the review change `@` is empty above the implementation.

## What was checked against the standard

- Reuse-vs-conflict logic (`annotate_blockers`, `src/domain/stack.rs`): correct. A `Pending` item whose bookmark sits on its own `item.input.head` gets a non-blocking `reusing existing bookmark <name>` note and no blocker. A name on a different change, or a name known only via `local_bookmarks` with no target, produces `bookmark <name> already exists on another change`. The unknown-target=conflict safe default is preserved.
- Idempotence: `blockers` and the new `reuse_notes` are both cleared at the top of each item loop; the `bookmark_on_same_change_is_idempotent` test exercises a double call and asserts a single note. Confirmed correct.
- Change-id matching: prefix-tolerant in both directions (`tid.starts_with(head) || head.starts_with(tid)`), covered by `bookmark_prefix_match_triggers_reuse`. Matches the revset `change_id.short()` (12-char) form noted in the plan.
- Dedicated note field: `reuse_notes` is a new `StackPlanItem` field owned and cleared by `annotate_blockers`, not the LLM-owned `warnings` vec, exactly as the plan required.
- Render (`src/screens/generate.rs`): reuse notes render with a `  ~ ` prefix in `theme::muted()` (distinct from the red blocker rows and from `theme::warning()`), and set the row `~` flag like warnings. `blocker_count` still sums only `item.blockers`, so reuse notes are excluded from the header blocker count â€” acceptance criterion met.
- Untouched logic: existing-PR, duplicate-within-plan, order, and completed-item self-block paths are unchanged. The existing tests for those remain and pass.
- Field churn: `reuse_notes: Vec::new()` added to every `StackPlanItem` literal (`src/app.rs` build_plan_items, `src/bin/ui-snapshots.rs`, `src/screens/generate/input.rs`, `tests/render_smoke.rs`, stack.rs test helper). Compilation confirms none were missed.

## Tests

- New unit tests: same-change reuse, idempotent re-invocation, different-change conflict, prefix-match reuse.
- Updated `local_and_remote_bookmarks_block` to the new "another change" message for the names-only path.
- New render-smoke `generate_bulk_review_with_bookmark_reuse_note_renders` exercises the non-blocking note row; the collision-blocker smoke test was updated to the new signature.

## Inferences / minor notes (not blocking)

- An item whose head is outside the loaded revsets has no `bookmark_targets` entry and falls through to the conflict path. This is the documented and accepted unknown-target behavior, not a defect.
- `collect_bookmark_targets` ignores `included_change_ids` and keys only on each revset entry's own `change_id`; this is correct because the bookmark is placed on the head change itself.
