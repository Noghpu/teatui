---
id: 0001r-2026-06-06-9bd47108-bulk-review-blocker-refresh
created_at: 2026-06-06T21:41:50+02:00
created_by_model: unknown
state: reviewed
state_updated_at: 2026-06-07T10:52:57+02:00
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

Add a dedicated transition for bulk review blocker refresh rather than overloading `PushStackPr`, because refresh must not set `pushing`, must not start a push, and must not change `push_all` sequencing.

Reuse the existing precheck fetch path so refresh retrieves both local bookmark state and existing remote PR state in one worker job. If needed, introduce a distinct pending state for refresh results so stale refresh/precheck results cannot accidentally continue a push. A refresh result must only update `StatusStore`, call `refresh_stack_review_blockers`, clear its in-flight marker, and mark the UI dirty.

The modal should visibly indicate refresh in progress without replacing the review with `Loading`. Keep the current plan visible and show a concise status such as `refreshing blockers...` or an equivalent last-action line. This follows the repo's `Cached<T>` rule: pure refresh keeps known content visible.

While a refresh is in flight, pressing `r` again should either be ignored or coalesced; do not queue multiple identical refresh jobs. Pushing during refresh should be conservative: either disable push until refresh completes, or let push run its normal precheck and let stale refresh results be ignored. Pick one behavior and cover it in tests.

## Implementation Plan
1. Add a new `Transition` variant, for example `RefreshStackBlockers`, in `src/screens/mod.rs`.
2. In `src/screens/generate/input.rs`, handle `KeyCode::Char('r')` in `on_bulk_review_key` when `bulk_editor.editing` is false. Flush the current bulk editor to the plan before returning the refresh transition so edits remain reflected.
3. In `src/app.rs`, dispatch the new transition to a new app method, for example `refresh_stack_blockers_from_modal`.
4. Implement the app method by submitting a worker job that refetches base bookmarks and existing PRs using the same inputs as `StackPushPrecheckJob`: current `jj_binary`, resolved forge, and current remote owner/repo. Track it separately from `pending_stack_push` so a refresh result never triggers `submit_stack_push`.
5. On refresh result, update `StatusStore` with the fresh bookmarks and existing PRs, call `refresh_stack_review_blockers`, clear the in-flight refresh marker, and set a concise `last_action` such as `blockers refreshed` when still in `BulkPhase::Review`.
6. Update the bulk review help hints in `src/screens/generate.rs` to show `r refresh` while the modal is open and not editing a field. Preserve existing `p`, `P`, navigation, edit, preview, and close hints.
7. Add tests for input routing, stale-result behavior or in-flight coalescing, and blocker reannotation after fresh data removes a bookmark conflict. Add/adjust render smoke coverage so the bulk review modal renders the refresh hint and the in-flight/final status without panicking.

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
    "Stale refresh and push-precheck results must not cross wires; a refresh result must never call submit_stack_push.",
    "Bulk review keeps the current plan visible while refreshing and preserves unsaved editor changes by flushing before refresh.",
    "The modal help text exposes the refresh key only where it works."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- In `BulkPhase::Review`, pressing `r` while not editing a field starts a blocker refresh from the modal.
- The refresh refetches local/base bookmarks and existing remote PRs, then re-runs blocker annotation for the current `StackPlan`.
- If a bookmark conflict is removed outside the app after generation, refreshing clears the corresponding blocker and the PR can proceed through the normal push path.
- Refresh does not start a push, does not alter stack item titles/branches/descriptions, and does not close or reset the modal.
- The UI keeps the review list/form visible while refresh is in flight and shows an appropriate refresh hint/status.
- Duplicate refresh keypresses during an in-flight refresh do not queue unbounded jobs.
- Existing push precheck behavior remains intact for single-item push and push-all sequencing.

## Verification Plan
Run `just verify` for formatting, compile, clippy, unit tests, and render smoke tests. Run `just snapshots` after the UI hint/status change and inspect the bulk review snapshot artifacts under `target/ui-snapshots/`, especially `generate-bulk-review.*` if present.

Add focused unit tests around `on_bulk_review_key` for the `r` transition and no-op behavior while editing if applicable. Add app/domain tests or narrow helper tests showing that fresh bookmark data clears a prior bookmark blocker. Extend render smoke for the bulk review modal to include the refresh hint and any in-flight/final refresh status.

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
- model: gpt-5.4-mini:medium
- completed_at: 2026-06-07T10:44:35+02:00
- state: implemented

Completed:
- Verified the bulk review blocker refresh implementation is present: `r` in review flushes editor changes and routes to `RefreshStackBlockers`, branch field commits also refresh, and the app refetches bookmark and existing-PR blocker inputs without starting a push.
- Confirmed duplicate refreshes are coalesced with `pending_stack_blocker_refresh`, and refresh results return before push continuation logic.
- Confirmed the review modal keeps the current plan visible and exposes `r refresh` in the rendered help hints.

Deviations:
- No code changes were needed in this pass because the implementation was already present in the selected repository revision; this pass validated it and completed the ticket queue move.

Verification:
- Ran `just verify` successfully: fmt, check, clippy, unit tests, and render smoke tests passed.
- Ran `just snapshots` successfully and inspected `target/ui-snapshots/generate-bulk-review.txt` for the `r refresh` hint.

Important files changed:
- docs/tickets/open/0001r-2026-06-06-9bd47108-bulk-review-blocker-refresh.md moved to docs/tickets/implemented by the ticket helper.

Residual risks or follow-up:
- None identified in this pass.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-06-07T10:52:57+02:00
- state: reviewed

Facts:
- Reviewed the bulk review blocker refresh implementation against the ticket acceptance criteria and current app flow.
- The refresh key path in `src/screens/generate/input.rs` flushes the bulk editor before returning `Transition::RefreshStackBlockers`, and ignores `r` while editing or while a push is visibly running.
- The app refresh path in `src/app.rs` submits the shared `StackPushPrecheckJob`, updates bookmark and existing-PR status, re-runs `refresh_stack_review_blockers`, and returns before push continuation while the refresh flag is set.
- Found and fixed one correctness gap: `p`/`P` could start a push while a blocker refresh job was already in flight, allowing the shared precheck payload type to cross refresh and push intents depending on result ordering. `start_stack_push` now refuses push initiation while `pending_stack_blocker_refresh` is true and leaves a visible wait message.
- Added `app::tests::stack_push_waits_for_in_flight_blocker_refresh` to cover the app-level coordination that the screen input tests cannot observe.

Verification:
- Ran focused tests: `cargo test stack_push_waits_for_in_flight_blocker_refresh --all-targets --all-features`, `cargo test r_in_bulk_review_list_triggers_refresh --all-targets --all-features`, and `cargo test r_during_push_is_ignored --all-targets --all-features`.
- Ran `just verify` successfully: formatting check, compile check, clippy with `-D warnings`, unit tests, integration test harness, and render smoke tests passed.
- Ran `just snapshots` successfully and inspected `target/ui-snapshots/generate-bulk-review.txt` for the `r refresh` hint.

Inference:
- With push initiation disabled during blocker refresh, refresh and push precheck results can no longer be confused through the shared `StackPushPrecheck` payload for the user-visible modal actions covered by this ticket.

Residual risk:
- No unresolved ticket acceptance risk found after the fix and verification pass.
