---
id: 0001f-2026-06-04-5a1b7c73-stacked-pr-collision-existing-pr-checks
created_at: 2026-06-04T20:48:32+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-04T22:39:35+02:00
---
# Stacked PR: bookmark-collision and existing-PR checks

## Goal
Detect bookmark collisions and pre-existing PRs for a built stack plan and
surface them as per-item **blockers** in the review modal, so the user cannot
push a PR that would duplicate work or clash with an existing local/remote
bookmark. Fast-fail only â€” the app never renames, deletes, or updates anything.

## Context
Slice 5 of `docs/stacked-pr-plan.md` (read "Collision And Existing PR Checks").
Slice 4 built `StackPlan`/`StackPlanItem { bookmark, blockers, warnings, .. }`
and renders a (currently empty) blocker area in the modal. This slice fills
`blockers`.

Available signals:
- Local bookmarks: `RevsetSummary.bookmarks` (`src/domain/probe.rs`) on the
  Changes list in `StatusStore::revsets`.
- All local/remote bookmarks: `BaseBookmarksProbe` already runs at startup and
  yields `BaseBookmarks = Vec<BaseBookmark { name, remote, is_remote }>`
  (`src/domain/probe.rs`), stored in `StatusStore`.
- Existing PRs: `tea pr list --output json` (the `tea` binary and command
  patterns live in `src/domain/execute.rs`; `RepoOptionsProbe` shows the
  `tea api`/argv style). PR JSON exposes the head branch and state.
- The "green path only" constraint: any out-of-ordinary state is a blocker, not
  something to repair.

## Non-Goals
- No push (slice 6) â€” this slice only annotates the plan with blockers.
- No automatic suffixes, renames, bookmark deletion, or PR updates.
- Do not detect "same change pushed under a different bookmark" (explicitly out
  of scope for v1 per the plan).

## Design Decisions
- **Pure check functions** in `src/domain/stack.rs` (unit-tested without IO):
  - duplicate generated bookmarks within the plan -> blocker on the later item;
  - a plan bookmark equal to an existing local/remote bookmark
    (`BaseBookmark`/`RevsetSummary.bookmarks`) -> collision blocker naming the
    bookmark (a remote bookmark with no matching PR is treated as a collision,
    not an existing PR);
  - a plan bookmark that matches an existing PR's head -> existing-PR blocker
    (open, closed, or merged all block) carrying the PR URL/identifier if known.
- **Existing-PR probe**: add a `StackExistingPrsProbe` (or fold into a checks
  job) that runs `tea pr list --output json` and parses a tolerant list of
  `{ head_branch, state, url }` (tolerate string/object/missing fields and extra
  keys; malformed JSON -> empty list, never panic â€” mirror the tolerant parsing
  style of `RepoOptionsProbe`/`probe.rs`). Match by head bookmark name.
- **When checks run**: build the plan first (slice 4), then run the checks pass
  as the plan enters `Review` (submit the probe; fold results into
  `plan.items[*].blockers` in `absorb_payload`). Re-running the checks for a
  single item just before its push is slice 6's concern; expose the per-item
  check so slice 6 can reuse it.
- A blocked item renders its blocker first (before warnings) in the modal row
  and detail; the header blocker count reflects the total.

## Implementation Plan
1. `src/domain/stack.rs`: add `annotate_blockers(plan: &mut StackPlan,
   local_bookmarks: &[..], existing_prs: &[ExistingPr])` (or return blockers per
   item) implementing the three checks above; unit tests for each case
   (duplicate within plan, local/remote bookmark collision, open/closed/merged
   existing PR, clean plan -> no blockers).
2. `src/domain/probe.rs` (or `src/domain/stack.rs`): add the existing-PR probe +
   tolerant parser for `tea pr list --output json`; unit tests for the parser
   shape tolerance.
3. `src/app.rs`: when entering `Review`, submit the existing-PR probe; in
   `absorb_payload`, combine it with the already-probed bookmarks and call
   `annotate_blockers`, then update the plan in `bulk = Review { .. }`.
4. `src/screens/generate.rs`: ensure the modal renders blockers (first) and the
   header blocker count (the area exists from slice 4).
5. Render smoke (`tests/render_smoke.rs`): a review with a bookmark-collision
   blocker and one with an existing-PR blocker.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/stacked-pr-plan.md",
    "src/domain/stack.rs",
    "src/domain/probe.rs",
    "src/domain/execute.rs",
    "src/app.rs",
    "src/screens/generate.rs",
    "tests/render_smoke.rs"
  ],
  "likely_files": [
    "src/domain/stack.rs",
    "src/domain/probe.rs",
    "src/app.rs",
    "src/screens/generate.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": ["just verify"],
  "review_focus": [
    "Every collision/existing-PR case is a fast-fail blocker; nothing is renamed, deleted, or updated.",
    "tea pr list JSON parsing is tolerant and never panics on missing/odd fields; matching is by head bookmark.",
    "Duplicate-in-plan, local/remote bookmark collision, and open/closed/merged existing-PR cases each produce the right blocker.",
    "Checks run when entering Review and the per-item check is reusable by slice 6.",
    "Blockers render first with an accurate header count."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- Duplicate bookmarks within the plan, collisions with existing local/remote
  bookmarks, and existing PRs (open/closed/merged) for a plan bookmark each
  produce a clear, item-scoped blocker (with the bookmark name / PR identifier
  where known).
- A remote bookmark without a matching PR is treated as a collision blocker.
- The existing-PR parser tolerates malformed/odd `tea` JSON without panicking.
- Blockers render first in the modal with an accurate header count; clean plans
  show none.
- `just verify` is green.

## Verification Plan
- `just verify`.
- Unit tests for `annotate_blockers` and the `tea pr list` parser; render smoke
  for the two blocker states.

## Files Likely Touched
- `src/domain/stack.rs`, `src/domain/probe.rs`, `src/app.rs`,
  `src/screens/generate.rs`, `tests/render_smoke.rs`

## Risks
- `tea` JSON varies across versions; keep the parser tolerant and fail closed.
- Bookmark matching must consider both local (`RevsetSummary.bookmarks`) and
  remote (`BaseBookmark`) names.
- Keep the per-item check function reusable so slice 6's pre-push re-check does
  not duplicate logic.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-06-04T22:33:42+02:00
- state: implemented

- Completed bookmark-collision and existing-PR blocker checks for stacked PR review plans.
- Added a tolerant `tea pr list --output json` probe and pure blocker annotation logic.
- Wired review-time blocker refresh into the app when revsets, base bookmarks, or existing PR results land; the review modal now shows computed blockers before warnings.
- Added render smoke coverage for bookmark-collision and existing-PR blocker states.
- Verification: `just verify`.
- Files changed: `src/domain/probe.rs`, `src/domain/stack.rs`, `src/domain/mod.rs`, `src/domain/status_store.rs`, `src/app.rs`, `tests/render_smoke.rs`.
- Residual risk: `tea` JSON shape may vary across versions, but parsing is intentionally tolerant and fails closed.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-06-04T22:39:35+02:00
- state: reviewed

## Review Postmortem

Facts:
- Reviewed the implemented stacked-PR collision and existing-PR checks against the ticket and `docs/stacked-pr-plan.md`.
- The implementation adds pure blocker annotation, boot/review-time bookmark checks, a `tea pr list --output json` probe, and render smoke coverage for bookmark-collision and existing-PR blocker states.
- Tightened the existing-PR parser so nested head branch shapes such as `head.ref` are accepted, while PR title-only entries are not treated as branches.
- Added parser coverage for the nested head shape and title-only guard.
- Ran `just verify`; it passed.

Inference:
- The remaining slice-6 pre-push re-check can reuse the landed pure blocker logic, as intended by the ticket.
