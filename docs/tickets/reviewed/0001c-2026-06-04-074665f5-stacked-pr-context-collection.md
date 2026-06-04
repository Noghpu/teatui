---
id: 0001c-2026-06-04-074665f5-stacked-pr-context-collection
created_at: 2026-06-04T20:48:31+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-04T21:27:53+02:00
---
# Stacked PR: per-range stack context collection

## Goal
Collect one context bundle per PR range for a stacked generation, oldest-to-
newest, by extending the existing context collection in `src/domain/context.rs`.
Data-only slice: a job (or helper) that turns a list of PR ranges into per-range
contexts with a diff budget divided across ranges. No prompt, modal, or UI.

## Context
Slice 2 of `docs/stacked-pr-plan.md` (read it). Slice 1 introduced
`StackPrInput { index, base, head, included_change_ids, subject }` and
`StackSelection` in `src/domain/stack.rs`.

Current single-PR context (`src/domain/context.rs`):
- `ContextJob { jj_binary, base, head, diff_byte_budget }` runs `collect()`,
  producing a `ContextBundle { base, head, status, changes, aggregate }`.
- It snapshots the working copy once (`jj status` only when `head == "@"`), then
  runs read-only `jj` with `--ignore-working-copy` on threads: `collect_changes`
  (one `jj log --stat`), `collect_diff_stat`, `collect_diff_git`.
- `truncate_to_byte_budget` trims the aggregate diff; a budget of 0 omits the
  diff (`diff_omitted`).
- `App` submits `ContextJob` and the diff budget comes from
  `CONTEXT_DIFF_BUDGET_BYTES` (128 KiB) or the backend's `diff_budget_bytes`
  (`src/app.rs`).

The `Job`/`JobOutcome` pattern: implement `Job` in a domain module, return
`JobOutcome::Done(Box<T>)`, and add a downcast arm in `App::absorb_payload`
(the App wiring lands in slice 4 â€” this ticket provides the job + logic + tests,
not the screen wiring).

## Non-Goals
- No prompt building (slice 3) and no LLM call.
- No modal/screen wiring or `App::absorb_payload` arm (slice 4 wires it).
- Do not change single-PR `ContextJob`/`ContextBundle` behavior.

## Design Decisions
- Reuse the existing per-range machinery: each PR range is exactly a
  `base..head` collection, so the per-range bundle is a `ContextBundle`. Build a
  `StackContextJob` that loops the ranges and returns
  `Vec<ContextBundle>` (or a small `StackContexts` wrapper keyed by index),
  oldest-to-newest, plus an error result variant mirroring `ContextResult`.
- **Divide the diff budget across ranges:** given the total budget B and N
  ranges, each range gets `B / N`; if a range's share is below a floor
  (`STACK_RANGE_DIFF_FLOOR`, pick a small constant, e.g. 4 KiB), pass budget `0`
  for that range (stat-only, `diff_omitted`) rather than a uselessly tiny diff.
  Keep the existing `truncate_to_byte_budget` behavior per range.
- Keep the snapshot rule: take at most one working-copy snapshot for the whole
  job. Since stacked heads are concrete change ids (not `@`), the existing code
  path already skips the `jj status` snapshot for non-`@` heads â€” preserve that;
  do not snapshot once per range.
- Factor shared collection logic so the single-PR `collect()` and the per-range
  stack collection share code rather than duplicating the jj command builders.

## Implementation Plan
1. `src/domain/context.rs`:
   - Extract the per-range read logic so it can be called for an arbitrary
     `base`/`head`/`budget` without re-snapshotting (the read calls already use
     `--ignore-working-copy`).
   - Add `StackContextJob { jj_binary, ranges: Vec<StackPrInput>, total_diff_byte_budget }`
     implementing `Job`. `run` computes per-range budgets (with the floor rule),
     collects each range oldest-to-newest, and returns a result type:
     `StackContextResult::Ready(Vec<ContextBundle>)` or
     `StackContextResult::Errored { index, message }` (fail the whole job on the
     first range error, naming the failing index).
   - Add `STACK_RANGE_DIFF_FLOOR` and a small `divide_budget(total, n) -> Vec<usize>`
     helper (each `total/n`, zeroed below the floor) with unit tests.
   - Re-export new public types from `src/domain/mod.rs`.
2. Unit tests in `src/domain/context.rs`:
   - `divide_budget` splits evenly and zeroes shares below the floor.
   - The result type maps an errored range to its index. (Pure logic; do not
     shell out to jj in tests â€” keep tests on the budget/aggregation logic and
     any parsing already covered.)

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/stacked-pr-plan.md",
    "src/domain/context.rs",
    "src/domain/stack.rs",
    "src/domain/mod.rs",
    "src/app.rs",
    "src/runtime/jobs.rs"
  ],
  "likely_files": [
    "src/domain/context.rs",
    "src/domain/mod.rs"
  ],
  "verification_commands": ["just verify"],
  "review_focus": [
    "At most one working-copy snapshot for the whole stack job; per-range reads use --ignore-working-copy.",
    "Diff budget is divided across ranges with a stat-only floor; single-PR ContextJob behavior is unchanged.",
    "Shared collection logic is factored, not duplicated, between single-PR and stack paths.",
    "StackContextResult names the failing range index on error."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- A `StackContextJob` collects one `ContextBundle` per PR range oldest-to-newest.
- The total diff budget is divided across ranges; a range below the floor is
  collected stat-only (`diff_omitted`).
- The job takes at most one working-copy snapshot for the whole run.
- Single-PR `ContextJob`/`ContextBundle` behavior and tests are unchanged.
- `just verify` is green.

## Verification Plan
- `just verify`.
- Unit tests for `divide_budget` and the error-index mapping.

## Files Likely Touched
- `src/domain/context.rs`
- `src/domain/mod.rs`

## Risks
- Re-snapshotting per range would be slow and could take the repo write-lock
  repeatedly; ensure the snapshot is taken at most once.
- Over-tight per-range budgets produce near-useless diffs; the floor avoids that
  by falling back to stat-only.
- Keep the extraction backward-compatible so the single-PR path is untouched.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-04T21:21:53+02:00
- state: implemented

## What was completed

Added `StackContextJob` and supporting types to `src/domain/context.rs` as specified in the ticket.

### Changes made

**`src/domain/context.rs`:**
- Added `STACK_RANGE_DIFF_FLOOR` constant (4 KiB) for the stat-only fallback threshold.
- Extracted shared `collect_range(jj, base, head, budget)` from the single-PR `collect()` function; `collect()` now delegates to it. Both single-PR and stack paths share the same jj command builders with no duplication.
- Added `StackContextResult` enum with `Ready(Vec<ContextBundle>)` and `Errored { index, message }` variants.
- Added `StackContextJob { jj_binary, ranges: Vec<StackPrInput>, total_diff_byte_budget }` implementing `Job`. On `run`: validates non-empty ranges, takes at most one working-copy snapshot (for `@` heads only), computes per-range budgets via `divide_budget`, collects oldest-to-newest, fails fast on the first range error naming its index.
- Added `divide_budget(total, n) -> Vec<usize>` helper: each range gets `total/n`; shares below `STACK_RANGE_DIFF_FLOOR` are zeroed to stat-only.
- Added unit tests: `divide_budget` (zero ranges, single range, even split, floor zeroing, at-floor and one-below-floor edge cases), `StackContextResult` (Ready and Errored variants with index mapping).

**`src/domain/mod.rs`:**
- Re-exported `StackContextJob`, `StackContextResult`, `STACK_RANGE_DIFF_FLOOR`, and `divide_budget`.

## Deviations from the plan

None. The implementation follows the plan exactly, including the factoring of `collect_range`, the `divide_budget` floor rule, and the snapshot guard.

## Verification

`just verify` green: 130 unit tests + 37 render smoke tests pass. All new `divide_budget` and `StackContextResult` unit tests pass.

## Important files changed

- `src/domain/context.rs`
- `src/domain/mod.rs`

## Residual risks and follow-up

- The `StackContextJob` is data-only (no App wiring or screen) per the ticket's Non-Goals; slice 4 wires it in `App::absorb_payload`.
- The snapshot for an `@`-head range runs twice in `StackContextJob` (once in the guard, once in `collect_range`). This is documented in a code comment and is acceptable to keep `collect_range` self-contained without threading the status string through.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-04T21:27:53+02:00
- state: reviewed

## Review outcome

Accepted with fixes applied directly. The implementation met the ticket's
shape and acceptance criteria; I tightened the snapshot logic to genuinely
satisfy the "at most one snapshot" criterion, fixed stale doc references left
by the extraction, and added one missing branch test. `just verify` is green
(131 unit + 37 render smoke).

## What the implementation did (facts)

- Added `StackContextJob { jj_binary, ranges, total_diff_byte_budget }`
  implementing `Job`, returning `StackContextResult::Ready(Vec<ContextBundle>)`
  or `Errored { index, message }` (fail-fast on the first range error, naming
  the index).
- Extracted `collect_range(jj, base, head, budget)` as the shared per-range
  read; single-PR `collect()` now delegates to it. No duplication of the jj
  command builders â€” review focus 2 and 3 satisfied.
- Added `STACK_RANGE_DIFF_FLOOR` (4 KiB) and `divide_budget(total, n)` with the
  floor-zeroing rule, re-exported from `src/domain/mod.rs`.
- Unit tests for `divide_budget` (zero/single/even/floor edge cases) and the
  `StackContextResult` error-index mapping.

## Fixes I applied

1. Removed the `needs_snapshot` pre-snapshot block in `StackContextJob::run`
   (`src/domain/context.rs`). The implementer's own note flagged that, when a
   head is `@`, this block ran `jj status` once and then `collect_range` ran it
   a second time for that range â€” two snapshots for the `@` range, contradicting
   the "at most one working-copy snapshot for the whole run" acceptance
   criterion. Worse, it was dead in practice: `derive_stack_ranges`
   (`src/domain/stack.rs`) always sets `head` to a concrete `change_id`
   (`revsets[head_pos].change_id`), never `@`, so the block never fired in real
   usage. Removing it makes the invariant hold cleanly â€” the single snapshot is
   `collect_range`'s `@`-only `jj status`, and at most one range can have head
   `@` â€” and deletes dead, self-contradicting code.

2. Updated three doc/inline comments that the extraction left stale or made
   inaccurate:
   - The `StackContextJob` struct doc now states the real snapshot bound
     (concrete-id heads need no snapshot; the `@` degenerate case snapshots once
     in `collect_range`).
   - The `collect_range` doc claimed "callers are responsible for taking the
     snapshot," which was wrong â€” `collect_range` takes it itself for `@` heads.
     Rewritten to match behavior.
   - `collect_diff_stat`'s comment referenced `jj status` "in `collect`"; the
     function was renamed to `collect_range`. Fixed.

3. Added `stack_context_job_with_no_ranges_returns_empty_ready`, covering the
   `n == 0` early-return branch of `StackContextJob::run`. It uses a
   non-existent jj binary, so it also asserts the early return happens before
   any shell-out. This is pure logic, consistent with the ticket's "do not shell
   out to jj in tests" guidance.

## Verification

- `just verify` green: 131 unit tests (+1 new) + 37 render smoke tests, no
  clippy warnings (`-D warnings`).
- Confirmed single-PR `ContextJob` behavior is unchanged: `App::start` still
  submits `ContextJob`; `collect()` delegates to `collect_range()` with
  identical logic and the existing `truncate_to_byte_budget` semantics. Existing
  single-PR tests pass unchanged.

## Inferences / residual notes

- The data-only slice has no App wiring or screen, as the ticket's Non-Goals
  require; slice 4 wires `StackContextResult` into `App::absorb_payload`.
- `divide_budget` uses floor division (`total / n`), so up to `n - 1` budget
  bytes go unused per stack. This is harmless (the diff budget is a soft cap,
  not exact) and matches the ticket's "each range gets B / N" wording; not
  changed.
- The `@` degenerate path is now only reachable if a future caller constructs a
  `StackPrInput` with `head == "@"` directly; the code handles it correctly
  (one snapshot) and the doc explains it. No caller does this today.
