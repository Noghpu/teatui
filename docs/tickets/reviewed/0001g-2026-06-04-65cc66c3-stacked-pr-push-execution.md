---
id: 0001g-2026-06-04-65cc66c3-stacked-pr-push-execution
created_at: 2026-06-04T20:48:32+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-05T10:53:49+02:00
---
# Stacked PR: per-PR and whole-stack push with in-session resume

## Goal
Make the review modal executable: `p` pushes the highlighted PR and `P` pushes
the whole stack oldest-to-newest, reusing `ExecutePrJob`'s jj/tea command
builders. Track per-PR status, enforce earlier-PRs-first ordering, stop on the
first failure, keep completed PR URLs visible, and support in-session resume by
re-checking state before re-pushing.

## Context
Slice 6 (final) of `docs/stacked-pr-plan.md` (read "Execute Stack" / "Review +
push"). Prereqs: the review modal + `StackPlan`/`StackPlanItem { status: PrStatus,
blockers, .. }` (slice 4) and per-item collision/existing-PR checks (slice 5).

Current single-PR execution (`src/domain/execute.rs`):
- `ExecutePrJob { jj_binary, tea_binary, change_id, bookmark, base, title,
  description, labels, assignees, milestone }` runs three steps in `run_execute`:
  `jj bookmark set --allow-backwards <bookmark> -r <change>`,
  `jj git push --bookmark <bookmark>`, `tea pr create ...` (+ conditional
  `--labels/--assignees/--milestone`), returning
  `ExecuteResult::Ready { url }` / `Errored { step: ExecuteStep, message }`.
- `App::start_execution`/`handle_execute_result` drive the single-PR phase.
- `PrStatus` (slice 1) mirrors the steps: `Pending`/`Bookmarked`/`Pushed`/
  `Created { url }`/`Failed { step, message }`.

## Non-Goals
- No on-disk persistence â€” resume is in-session only.
- No auto-retry/backoff loop â€” resume is the user pressing `p`/`P` again
  (re-check before re-push). (Transport-level retry inside a single jj/tea call,
  if any, stays as in `ExecutePrJob`.)
- No new PR-shaping; the plan is fixed once in `Review`.

## Design Decisions
- **Reuse the builders**: factor the three argv builders + `run_capture` out of
  `run_execute` (`src/domain/execute.rs`) so both the single-PR job and a new
  per-PR stack push share them (the constraint "reuse existing command builders,
  don't duplicate shell behavior" is non-negotiable). Add a job that pushes **one**
  `StackPlanItem` (bookmark -> push -> create) and reports a `StackPushResult {
  index, status: PrStatus }`, advancing through `Bookmarked`/`Pushed`/
  `Created{url}` or stopping at `Failed{step,message}`.
- **`p`** (Review): push the highlighted item. **Blocked** (no-op with a visible
  reason) if the item has a blocker, or if any earlier item is not yet
  `Created` (its base bookmark would not exist) â€” this is the ordering rule.
- **`P`**: push the whole stack oldest-to-newest. Implement as: push the first
  not-`Created` item; on `Created`, if in "push-all" mode, submit the next;
  stop on the first `Failed`. Track push-all mode on the screen (or a small
  flag) so `absorb_payload` knows whether to chain.
- **`BulkPhase::Review { pushing: Some(index), .. }`** while a push job runs;
  `has_busy_job()` returns busy then, blocking further input/mutation. On result,
  set the item's `status` and clear/advance `pushing`.
- **In-session resume**: before pushing (or re-pushing) an item, re-run the
  per-item collision/existing-PR check from slice 5 so a partially-completed
  earlier attempt is not duplicated (e.g. bookmark already pushed / PR already
  created -> treat as done/blocked rather than re-creating). Completed items are
  fixed anchors for later bases.
- **Bases at push time**: PR 1 uses the snapshotted form base; PR k uses PR k-1's
  bookmark (already on the plan item). Do not recompute from live state.

## Implementation Plan
1. `src/domain/execute.rs`: extract `bookmark_args`/`push_args`/`tea_create_args`
   (+ shared `jj`/`tea`/`run_capture`) and add a single-item stack push job
   returning `StackPushResult`. Keep `ExecutePrJob` working via the same
   builders. Unit tests for argv shape (mirroring existing `extract_url` tests).
2. `src/screens/mod.rs`: add `Transition::PushStackPr(usize)` and
   `Transition::PushStackAll` (or one variant carrying a mode).
3. `src/screens/generate/input.rs`: in the bulk-modal key handler, bind `p`/`P`
   in `Review` (gated on no `pushing`, no blocker, ordering satisfied).
4. `src/app.rs`: handle the push transitions â€” re-check the target item (slice
   5), submit the per-item push job, set `pushing`; in `absorb_payload`, fold
   `StackPushResult` into the plan item's `status`, and if in push-all mode and
   the item is `Created`, submit the next not-`Created` item; stop on failure.
5. `src/screens/generate.rs`: render per-row push status badges, the active PR's
   in-flight step, completed URLs, and a done/failed summary; footer shows
   `p push current` / `P push all`.
6. Tests: unit â€” `p` refused when an earlier item is not `Created`; `P` stops on
   the first failed step and keeps completed URLs; status progression; ordering.
   Render smoke (`tests/render_smoke.rs`) â€” review with mixed statuses, a
   push-in-flight item, a done state (all URLs), and a failed state. Add/extend
   snapshot specs in `src/bin/ui-snapshots.rs`; run `just snapshots`.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/stacked-pr-plan.md",
    "src/domain/execute.rs",
    "src/domain/stack.rs",
    "src/app.rs",
    "src/screens/generate.rs",
    "src/screens/generate/input.rs",
    "src/screens/mod.rs",
    "tests/render_smoke.rs",
    "src/bin/ui-snapshots.rs"
  ],
  "likely_files": [
    "src/domain/execute.rs",
    "src/app.rs",
    "src/screens/generate.rs",
    "src/screens/generate/input.rs",
    "src/screens/mod.rs",
    "tests/render_smoke.rs",
    "src/bin/ui-snapshots.rs"
  ],
  "verification_commands": ["just verify", "just snapshots"],
  "review_focus": [
    "ExecutePrJob's command builders are factored and shared with the stack push job; no duplicated shell behavior.",
    "p is refused when an earlier PR is not yet Created (base would not exist); P walks oldest-to-newest and stops on first failure.",
    "Per-PR status progresses Pending->Bookmarked->Pushed->Created{url}/Failed{step}; completed URLs stay visible after a failure.",
    "Before (re-)pushing, the per-item check from slice 5 runs so resume does not duplicate a bookmark/PR.",
    "has_busy_job covers Review{pushing:Some(_)}; input is blocked during a push job.",
    "Bases use the snapshotted form base / previous bookmark, not recomputed live state."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- `p` pushes the highlighted PR (bookmark -> push -> create) and is refused with
  a visible reason when the item is blocked or an earlier PR is not yet
  `Created`.
- `P` pushes the stack oldest-to-newest and stops on the first failed step,
  leaving completed PR URLs visible.
- Per-PR status progresses through the documented states; a failed step shows
  the step and message.
- Re-pressing `p`/`P` after a failure re-checks item state and does not
  duplicate an already-created bookmark/PR.
- The single-PR `ExecutePrJob` path still works via the shared builders.
- `just verify` and `just snapshots` are green.

## Verification Plan
- `just verify`; `just snapshots` and review the push/done/failed modal states
  in `target/ui-snapshots/index.html`.
- Unit tests for ordering refusal, stop-on-first-failure, and status
  progression; render smoke for mixed/push/done/failed states.

## Files Likely Touched
- `src/domain/execute.rs`, `src/app.rs`, `src/screens/generate.rs`,
  `src/screens/generate/input.rs`, `src/screens/mod.rs`,
  `tests/render_smoke.rs`, `src/bin/ui-snapshots.rs`

## Risks
- Ordering: pushing a later PR before its base bookmark exists fails
  server-side; the earlier-first gate must hold for both `p` and `P`.
- Chaining `P` across async job results must stop cleanly on failure and not
  race ahead; key the "next" decision on the just-returned item's `Created`
  status.
- Resume correctness hinges on the slice-5 re-check; a missing re-check risks
  duplicate bookmarks/PRs.
- Keep the refactor of `run_execute` behavior-preserving for single PRs.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-06-05T10:43:09+02:00
- state: implemented

Implemented the stacked PR push flow for the review modal.

Completed:
- Added shared execute command builders and a new `StackPushJob` / `StackPushResult` in `src/domain/execute.rs`.
- Wired `p` / `P` into the bulk review modal, including push-all chaining, blocking on earlier unfinished PRs, and in-session resume behavior.
- Extended the review modal rendering to show push status, in-flight pushes, completed URLs, and completion/failure summaries.
- Added order-blocker handling for later PRs until earlier PRs are created, while avoiding self-blocking on completed rows.
- Added/updated unit tests, render smoke tests, and snapshot coverage for push-in-flight, done, and failed review states.

Deviations:
- No functional deviations from the ticket plan.
- I kept the existing review modal structure and added the push state to it rather than introducing a separate screen.

Verification:
- `just fmt` / `cargo fmt --all`
- `just check`
- `just test`
- `just snapshots`
- `just verify`

Files changed:
- `src/domain/execute.rs`
- `src/domain/stack.rs`
- `src/domain/mod.rs`
- `src/app.rs`
- `src/screens/mod.rs`
- `src/screens/generate.rs`
- `src/screens/generate/input.rs`
- `src/bin/ui-snapshots.rs`
- `tests/render_smoke.rs`

Residual risks:
- The bulk review right pane still uses paragraph rendering rather than a dedicated scroll state, so extremely dense blocker/result payloads can clip in very small terminals.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-06-05T10:53:49+02:00
- state: reviewed

Facts:
- Reviewed the implemented stacked PR push execution ticket against the persisted acceptance criteria and the active rewrite plan.
- The implementation already shared the single-PR execute command builders with `StackPushJob`, wired `p` and `P`, blocked later rows while earlier rows were unfinished, rendered mixed push states, and covered the modal with unit/render tests.
- The initial implementation did not run a fresh state check immediately before each push. It only used cached blocker data and, during push-all chaining, submitted the next `StackPushJob` directly after the prior result.

Fixes applied during review:
- Added `StackPushPrecheckJob` / `StackPushPrecheck`, reusing the existing base-bookmark and existing-PR parsers, so every `p` and every `P` chain step refreshes live bookmark and PR state before submitting `StackPushJob`.
- Added `mark_created_from_existing_prs` so in-session resume treats an already-created PR with the generated head and URL as `PrStatus::Created` instead of re-running `tea pr create`.
- Wired precheck handling in `App`, including busy-state input blocking, visible "checking" render copy, push-all continuation after already-created rows, and stop-on-fresh-blocker behavior.
- Added unit coverage for existing-PR resume status marking.

Verification:
- `cargo fmt --all`
- `just test` passed: 159 unit tests and 49 render smoke tests; the 2 llama integration tests were ignored as configured.
- `just verify` passed: fmt check, cargo check, clippy `-D warnings`, and tests.
- `just snapshots` passed and wrote 20 snapshots to `target/ui-snapshots`; reviewed push-state snapshot text for layout/wording sanity.

Residual risk:
- The fresh precheck treats an existing PR as completed only when the live `tea pr list --output json` payload includes a non-empty URL for the generated head branch. If a server or tea version omits URL fields, the row remains blocked rather than duplicated, which is conservative.
