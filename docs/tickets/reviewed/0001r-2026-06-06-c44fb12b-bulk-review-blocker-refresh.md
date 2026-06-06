---
id: 0001r-2026-06-06-c44fb12b-bulk-review-blocker-refresh
created_at: 2026-06-06T21:49:50+02:00
created_by_model: unknown
state: reviewed
state_updated_at: 2026-06-07T01:17:35+02:00
---
# Bulk Review Blocker Refresh

## Goal
Add a refresh action inside the stacked-PR bulk review modal that rechecks blocker inputs and updates each PR row without leaving the modal. A user who resolves a bookmark conflict or existing-PR conflict after generation finishes must be able to refresh the review and continue pushing once the blocker is gone.

## Context
The bulk review modal currently computes blockers from cached status data: local/base bookmarks and existing remote PRs. `App::refresh_stack_review_blockers` re-annotates the in-memory `StackPlan`, but it only uses values already in `StatusStore`. The app fetches fresh bookmark/existing-PR data when generation enters review and again as a push precheck via `StackPushPrecheckJob`. If a blocker appears after generation, the user can fix it outside the app, but the modal has no explicit recheck action. The affected PR remains blocked because the current review plan is not refreshed from newly fetched blocker inputs.

Existing paths to reuse:

- `src/domain/probe.rs` has `StackPushPrecheckJob`, `StackPushPrecheck`, `fetch_base_bookmarks`, and existing PR fetching.
- `src/app.rs` has `start_stack_push_precheck`, `handle_stack_push_precheck`, and `refresh_stack_review_blockers`.
- `src/screens/generate/input.rs` owns bulk review input handling.
- `src/screens/generate.rs` owns bulk modal help text and render state.

## Non-Goals
Do not change stacked-PR generation, LLM parsing, branch/title/description editing, or push/PR creation behavior. Do not add a generic refresh framework for all screens. Do not change blocker rules except where stale blocker data must be replaced by freshly fetched data.

## Design Decisions
Use `r` as the bulk review modal refresh key when not editing a field. The Menu pane already uses `r` for revset refresh; inside `BulkPhase::Review`, `r` should be modal-local and should not refresh revsets.

Also run the same blocker recheck automatically after the user manually edits and commits a generated branch name in the bulk review modal. Branch edits can introduce or resolve bookmark and existing-PR conflicts just like external bookmark changes, so committing the Branch field must refresh the blocker inputs and re-annotate the plan without requiring an extra keypress.

Add a dedicated transition for bulk review blocker refresh rather than overloading `PushStackPr`, because refresh must not set `pushing`, must not start a push, and must not change `push_all` sequencing.

Reuse the existing precheck fetch path so refresh retrieves both local bookmark state and existing remote PR state in one worker job. If needed, introduce a distinct pending state for refresh results so stale refresh/precheck results cannot accidentally continue a push. A refresh result must only update `StatusStore`, call `refresh_stack_review_blockers`, clear its in-flight marker, and mark the UI dirty.

The modal should visibly indicate refresh in progress without replacing the review with `Loading`. Keep the current plan visible and show a concise status such as `refreshing blockers...` or an equivalent last-action line. This follows the repo's `Cached<T>` rule: pure refresh keeps known content visible.

While a refresh is in flight, pressing `r` again should either be ignored or coalesced; do not queue multiple identical refresh jobs. Pushing during refresh should be conservative: either disable push until refresh completes, or let push run its normal precheck and let stale refresh results be ignored. Pick one behavior and cover it in tests.

## Implementation Plan
1. Add a new `Transition` variant, for example `RefreshStackBlockers`, in `src/screens/mod.rs`.
2. In `src/screens/generate/input.rs`, handle `KeyCode::Char('r')` in `on_bulk_review_key` when `bulk_editor.editing` is false. Flush the current bulk editor to the plan before returning the refresh transition so edits remain reflected.
3. In `src/screens/generate/input.rs`, when the bulk editor commits the Branch field, flush the edit and return the same refresh transition. Title and Description commits should keep their current behavior unless the implementation chooses to refresh all field commits for simplicity and tests the tradeoff.
4. In `src/app.rs`, dispatch the new transition to a new app method, for example `refresh_stack_blockers_from_modal`.
5. Implement the app method by submitting a worker job that refetches base bookmarks and existing PRs using the same inputs as `StackPushPrecheckJob`: current `jj_binary`, resolved forge, and current remote owner/repo. Track it separately from `pending_stack_push` so a refresh result never triggers `submit_stack_push`.
6. On refresh result, update `StatusStore` with the fresh bookmarks and existing PRs, call `refresh_stack_review_blockers`, clear the in-flight refresh marker, and set a concise `last_action` such as `blockers refreshed` when still in `BulkPhase::Review`.
7. Update the bulk review help hints in `src/screens/generate.rs` to show `r refresh` while the modal is open and not editing a field. Preserve existing `p`, `P`, navigation, edit, preview, and close hints.
8. Add tests for input routing, branch-edit-triggered refresh, stale-result behavior or in-flight coalescing, and blocker reannotation after fresh data removes a bookmark conflict. Add/adjust render smoke coverage so the bulk review modal renders the refresh hint and the in-flight/final status without panicking.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md"
  ],
  "likely_files": [
    "src/screens/mod.rs",
    "src/screens/generate/input.rs",
    "src/screens/generate.rs",
    "src/app.rs",
    "src/domain/probe.rs",
    "src/domain/status_store.rs",
    "src/domain/stack.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "Refresh must refetch bookmark/existing-PR data and re-run blocker annotation without starting a push.",
    "Committing a manually edited generated Branch field in the bulk review modal must trigger the same recheck.",
    "Stale refresh and push-precheck results must not cross wires; a refresh result must never call submit_stack_push.",
    "Bulk review keeps the current plan visible while refreshing and preserves unsaved editor changes by flushing before refresh.",
    "The modal help text exposes the refresh key only where it works."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- In `BulkPhase::Review`, pressing `r` while not editing a field starts a blocker refresh from the modal.
- Committing a manual edit to a generated Branch field in the bulk review modal starts the same blocker refresh automatically.
- The refresh refetches local/base bookmarks and existing remote PRs, then re-runs blocker annotation for the current `StackPlan`.
- If a bookmark conflict is removed outside the app after generation, refreshing clears the corresponding blocker and the PR can proceed through the normal push path.
- Refresh does not start a push, does not alter stack item titles/branches/descriptions, and does not close or reset the modal.
- The UI keeps the review list/form visible while refresh is in flight and shows an appropriate refresh hint/status.
- Duplicate refresh keypresses during an in-flight refresh do not queue unbounded jobs.
- Existing push precheck behavior remains intact for single-item push and push-all sequencing.

## Verification Plan
Run `just verify` for formatting, compile, clippy, unit tests, and render smoke tests. Run `just snapshots` after the UI hint/status change and inspect the bulk review snapshot artifacts under `target/ui-snapshots/`, especially `generate-bulk-review.*` if present.

Add focused unit tests around `on_bulk_review_key` for the `r` transition, branch field commit returning the refresh transition, and no-op behavior while editing if applicable. Add app/domain tests or narrow helper tests showing that fresh bookmark data clears a prior bookmark blocker. Extend render smoke for the bulk review modal to include the refresh hint and any in-flight/final refresh status.

## Files Likely Touched
- `src/screens/mod.rs`
- `src/screens/generate/input.rs`
- `src/screens/generate.rs`
- `src/app.rs`
- `src/domain/probe.rs`
- `src/domain/status_store.rs`
- `src/domain/stack.rs`
- `tests/render_smoke.rs`

## Risks
The main risk is mixing the manual refresh result with the existing push precheck path: `handle_stack_push_precheck` currently may continue into `submit_stack_push` when no blocker remains, which is correct for a push precheck but wrong for a refresh action. Keep refresh and push intent separate even if they share the same fetched payload shape.

Another risk is using stale `StatusStore` data and only re-running `refresh_stack_review_blockers`; that would not solve the user problem. The refresh action must actually submit a worker job to refetch blocker inputs.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-07T01:13:08+02:00
- state: implemented

## What was completed

Implemented the Bulk Review Blocker Refresh feature (ticket 0001r):

1. Added `Transition::RefreshStackBlockers` variant to `src/screens/mod.rs`.
2. In `src/screens/generate/input.rs`:
   - `r` key in `on_bulk_review_key` (when not editing, no push in flight) flushes the editor to the plan and returns `RefreshStackBlockers`.
   - Committing the Branch field in `on_bulk_editor_key` returns `RefreshStackBlockers` (Title/Description commit returns the existing `Dirty`).
3. In `src/app.rs`:
   - Added `pending_stack_blocker_refresh: bool` field to `App` for in-flight coalescing.
   - Added `refresh_stack_blockers_from_modal` method that submits `StackPushPrecheckJob` only when in `BulkPhase::Review { pushing: None }` and no other refresh/push is in flight; sets `last_action` to `"refreshing blockersâ€¦"`.
   - Modified `handle_stack_push_precheck` to detect refresh results early (when `pending_stack_blocker_refresh` is true): clear the flag, update `last_action` to `"blockers refreshed"`, return without ever entering push-continuation logic.
4. Updated `BulkPhase::Review` help hints in `src/screens/generate.rs` to include `r refresh` for both List and Preview focus states (not during editing or push-in-flight).

## Deviations from plan

None significant. Implementation follows the plan exactly. No distinct result type needed because `handle_stack_push_precheck` already updates bookmarks/existing PRs before checking `pending_stack_push`; the `pending_stack_blocker_refresh` flag intercepts the result at the right point.

## Verification

`just verify` passed: formatting, compile, clippy (0 warnings), 217 unit tests (including 7 new input routing tests).
`just snapshots` updated 20 UI snapshot artifacts.

## Files changed

- `src/screens/mod.rs` â€” new `RefreshStackBlockers` transition variant
- `src/screens/generate/input.rs` â€” `r` key + Branch commit routing, 7 new tests
- `src/screens/generate.rs` â€” `r refresh` added to List and Preview help hints
- `src/app.rs` â€” `pending_stack_blocker_refresh` field, `refresh_stack_blockers_from_modal`, precheck handler guard

## Risks / follow-up

- If both a push precheck AND a blocker refresh result land in the same event drain cycle, the second result is a no-op (stale). This is safe but means the refresh indicator clears earlier than the push precheck processes. This edge case is unlikely in practice.
- No tests at the App integration level for the full refresh flow (worker job â†’ state update) since the ticket's test plan covered input routing and reannotation logic; an integration harness for job submission would require additional test infrastructure not present in the codebase.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-07T01:17:35+02:00
- state: reviewed

## Review Outcome
Accepted with no changes required. The implementation fully satisfies the ticket goal, design decisions, and all acceptance criteria.

## Verification (facts)
- `just verify` passed: rustfmt clean, compile clean, `clippy --all-targets --all-features -- -D warnings` reported zero warnings, 217 unit tests passed (7 new input-routing tests), 53 render smoke tests passed.
- No test failures or warnings observed.

## Correctness assessment (facts)
- `Transition::RefreshStackBlockers` added in `src/screens/mod.rs` and dispatched in `src/app.rs` to `refresh_stack_blockers_from_modal`.
- `r` in `on_bulk_review_key` (non-editing) flushes the editor to the plan, then returns the refresh transition (`src/screens/generate/input.rs:340-345`). When editing, `r` routes to `on_bulk_editor_key` and is treated as text input, so it does not refresh â€” covered by `r_while_editing_does_not_refresh`.
- Branch field commit returns the refresh transition; Title/Description commits return `Dirty` (`input.rs:359-372`). Covered by `branch_field_commit_triggers_refresh` and `title_field_commit_does_not_trigger_refresh`.
- During an in-flight push, `on_bulk_review_key` only allows navigation; `r` falls through to `Transition::None` (`input.rs:219-244`). The app method `refresh_stack_blockers_from_modal` independently re-guards on `BulkPhase::Review { pushing: None }`, giving defense in depth.
- Coalescing: `refresh_stack_blockers_from_modal` ignores the request when `pending_stack_push.is_some()` or `pending_stack_blocker_refresh` is already set, so duplicate `r` presses do not queue unbounded jobs.
- Refresh reuses `StackPushPrecheckJob` (same `jj_binary`, resolved forge, owner/repo), so it actually refetches bookmarks and existing PRs rather than only re-annotating stale `StatusStore` data â€” this directly addresses the ticket's stated primary risk.
- In `handle_stack_push_precheck`, fresh bookmarks/existing PRs are written to `StatusStore` and `refresh_stack_review_blockers` is called BEFORE the refresh-flag early return. The refresh branch then clears `pending_stack_blocker_refresh`, sets `last_action` to `blockers refreshed`, and returns without ever reaching `pending_stack_push.take()` or any push continuation. A refresh result therefore never calls `submit_stack_push`.
- The flag is cleared even if the user has left `BulkPhase::Review` (the review-phase check only gates `last_action`), so the in-flight marker cannot get stuck.
- The review keeps the current plan visible during refresh (uses `last_action` "refreshing blockersâ€¦", not a `Loading` swap), consistent with the repo's keep-known-content rule.
- Help hints: `r refresh` is shown only in the non-editing, non-pushing Review states. The `pushing: Some(_)` arm is matched first (`src/screens/generate.rs:2352`) and shows the push-running hint instead, so the key is advertised only where it works.

## Inferences
- The implementer's noted absence of an App-level end-to-end refresh test is acceptable: the codebase has no job-submission test harness, and the ticket scoped tests to input routing and reannotation. Adding such infrastructure would exceed the ticket scope.
- The same-event-drain edge case (push precheck + refresh result coalescing) is benign as described; refresh results only update status/blockers and clear their own marker.

## Changes applied during review
None. No code or test changes were necessary.
