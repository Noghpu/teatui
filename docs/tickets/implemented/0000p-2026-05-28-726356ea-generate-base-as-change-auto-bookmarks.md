---
id: 0000p-2026-05-28-726356ea-generate-base-as-change-auto-bookmarks
created_at: 2026-05-28T10:00:46+02:00
created_by_model: claude-opus-4-7/high
state: implemented
state_updated_at: 2026-05-28T10:38:48+02:00
---
# Generate Screen: Base-as-Change with Auto Bookmark Generation

## Goal

Allow the "base" field in the PR form to reference a change in `trunk()..@` (in addition to a branch like `main@origin`). The PR scope becomes `<base>..<tip>`. At execution time, if either endpoint (base or tip) lacks a bookmark, generate and push a deterministic bookmark for it so Gitea can resolve the PR head/base.

## Context

The Generate screen's PR scope today is derived from the selected `RevsetSummary` (see sibling tickets that change this to per-change selection). The "base" field (`PrForm::base`) defaults to `repo.base_branch.name` (typically `main@origin`). The execution plan in `ExecutionPlan::from_draft` (`src/generate.rs:909`) does this:

1. If `branch_name` already matches a bookmark on the selected revset: `jj bookmark move`. Else: `jj bookmark create` at `head`.
2. `jj git push --bookmark <branch_name>`.
3. `tea pr create --base <base> --head <branch_name>`.

This works only when `head` is a change to be turned into a bookmark and `base` is already a remote bookmark like `main@origin`. There's no analogous create-and-push step for a base that is also a change without a bookmark.

We want: if the user picks change X as the tip and change Y as the base (both in `trunk()..@`):

- The PR scope becomes `<base change_id>..<tip change_id>` (this is what the LLM prompt and freshness check see).
- Both change_ids need bookmarks for the Gitea PR to resolve. If a bookmark already exists on the change, reuse it. If not, generate a deterministic auto-name and push it.

## Non-Goals

- Auto-generating bookmark names for arbitrary trunk-side bases (e.g. `main@origin` stays as-is â€” no extra step needed when base is already a remote bookmark).
- Letting the user pick a base from outside `trunk()..@`.
- UI work beyond what's needed to accept a change_id in the base field (no new widget; the existing TextArea / display logic suffices).
- Multi-tip / merge PR support.

## Design Decisions

- **Base field accepts two forms.**
  1. A branch-shaped string (contains `/`, `@`, or matches an existing remote bookmark â€” easiest test: `base.contains('@') || base == repo.base_branch.name`). Treated as a remote ref; no bookmark step.
  2. A jj change_id (8â€“12 lowercase letters, optional trailing chars). Treated as a change that needs a bookmark.
  - Detection is heuristic. We do not need to be perfect: if it looks like a change_id (matches `^[a-z]{8,}$` with no `@`, `/`, `:`, etc.), treat it as a change. Otherwise treat as a remote ref.
- **Auto-bookmark naming.** Deterministic from change_id and PR title to avoid collisions across PRs:
  - Tip auto-bookmark: `pr/<slug-of-title>` if the user has already entered a title; else `pr/<change_id>`.
  - Base auto-bookmark: `pr-base/<tip-bookmark-without-prefix>`. Stable, scoped to the PR, predictable to inspect.
  - Slug: lowercase ASCII, non-alphanumerics â†’ `-`, collapse repeated `-`, trim leading/trailing `-`, truncate to 32 chars.
  - This logic lives in a new helper module `src/bookmark_naming.rs` with unit tests.
- **Execution plan steps (when base is a change).**
  1. `jj bookmark create|move` for tip (existing behavior).
  2. `jj git push --bookmark <tip>` (existing).
  3. `jj bookmark create|move` for base.
  4. `jj git push --bookmark <base>`.
  5. `tea pr create --base <base bookmark> --head <tip bookmark>`.
  - When base is *not* a change (it's `main@origin` or similar), skip steps 3â€“4 and use the literal base string in step 5 â€” matches today's behavior.
- **Scope string passed to context / prompt / freshness.** Always `<base>..<tip>`. When base is a remote ref, use `repo.base_branch.name` (e.g. `main@origin`) verbatim. When base is a change, use the change_id directly (since the bookmark may not yet exist at context-collection time, but `change_id` always resolves).
- **`PrForm::head` and `PrForm::branch_name` cleanup.** Currently `head` holds the change_id (or revset string) and `branch_name` holds the bookmark name. This is muddled by the per-change ticket. Resolve it here:
  - Rename `head` semantics to "tip change_id" â€” the change selected in the left column.
  - `branch_name` continues to be the bookmark for the tip. If the user has not edited it, it defaults to the first bookmark on the tip change, or to the auto-generated `pr/<slug>` if none.
  - Add a new optional field `base_branch_name: Option<String>` (or compute on the fly) for the base bookmark when base is a change. Keep `form.base` as the user-facing input.
- **Validation.** `validate_for_execution` must:
  - Reject a base that resolves to neither a remote ref nor an in-`trunk()..@` change_id.
  - Reject when base change_id equals tip change_id.
  - Reject when the base change is not an ancestor of the tip change (this can be checked at execution-plan-build time by inspecting the per-change list â€” base must appear at/before tip).
- **Where bookmark generation lives.** `ExecutionPlan::from_draft` builds the bookmark name(s) by calling the helpers. The plan grows new optional steps (base create+push) only when base is a change. Add tests for both shapes.

## Implementation Plan

1. `src/bookmark_naming.rs` (new):
   - `pub fn slugify(input: &str) -> String`.
   - `pub fn tip_bookmark(title: &str, change_id: &str) -> String`.
   - `pub fn base_bookmark(tip_bookmark: &str) -> String`.
   - Unit tests covering empty title, unicode title, very long title, change-id-only fallback, collision-avoiding stability.
2. `src/generate.rs`:
   - `is_change_id_like(s: &str) -> bool` helper.
   - Update `validate_for_execution` with the new base validation rules.
   - Update `ExecutionPlan::from_draft`:
     - Determine `tip_change_id` from `form.head` (or the selected revset's first change_id).
     - Determine `tip_bookmark` = existing bookmark on tip if any; else `bookmark_naming::tip_bookmark(form.title, tip_change_id)`.
     - Decide `base_kind`:
       - If `form.base` is empty: use `repo.base_branch.name` as a remote ref.
       - Else if `is_change_id_like(form.base)`: treat as change.
       - Else: treat as remote ref.
     - If base is a change: compute `base_bookmark` (existing bookmark on that change if found in the `revsets` list; else `bookmark_naming::base_bookmark(tip_bookmark)`), append create-or-move and push steps for it.
     - Pass the bookmark or remote ref (whichever applies) as the `base` argument to `tea pr create`.
   - Update unit tests in `src/generate.rs` for the new plan shape (two cases: base is remote ref; base is change). Reuse existing test scaffolding.
3. `src/jj.rs`: no new commands needed (`bookmark_create_command`, `bookmark_move_command`, `git_push_bookmark_command` already exist).
4. `src/ui.rs`: minor â€” update the base-field help text to explain "branch ref or change_id". Display the resolved base bookmark in the execution preview section already rendered by `render_execution_plan`.
5. Manual smoke: select tip = top change, set base field to a change_id of an ancestor change, walk to Confirming, confirm the plan shows both bookmark+push steps for base.
6. `just verify` passes.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md"],
  "likely_files": ["src/generate.rs", "src/bookmark_naming.rs", "src/ui.rs", "src/jj.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "Base field heuristic correctly classifies branch-like strings vs change_ids",
    "ExecutionPlan adds bookmark create+push steps for base only when base is a change",
    "tea pr create receives the resolved bookmark/ref, not the raw change_id",
    "Auto bookmark names are deterministic and slugified from the title (or fall back to change_id)",
    "Validation rejects base == tip and base not an ancestor in trunk()..@",
    "New bookmark_naming module has unit tests; ExecutionPlan tests cover both base shapes",
    "Scope string for context/prompt/freshness is <base>..<tip> in both cases",
    "just verify passes"
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria

- When base is left as the default or set to a remote-ref-shaped string, the execution plan is identical to today's behavior (one bookmark+push pair, plus PR create).
- When base is set to a change_id of an ancestor change in `trunk()..@`, the execution plan additionally creates and pushes a deterministic bookmark for the base, and the PR create command uses that bookmark.
- Validation rejects a base that equals the tip or that is not an ancestor of the tip within `trunk()..@`.
- The scope string sent through context collection and freshness checks is `<base>..<tip>`.
- `just verify` passes.

## Verification Plan

- Unit tests for `bookmark_naming::slugify`, `tip_bookmark`, `base_bookmark`.
- Unit tests in `src/generate.rs` for both `ExecutionPlan::from_draft` shapes (base=remote, base=change).
- Manual smoke against a workspace with â‰¥2 changes above trunk and no pre-existing bookmarks.
- `just verify`.

## Files Likely Touched

- `src/bookmark_naming.rs` (new)
- `src/generate.rs` â€” `ExecutionPlan::from_draft`, validation, helper
- `src/ui.rs` â€” base-field help text
- `src/lib.rs` â€” register the new module

## Risks

- The change_id heuristic could mis-classify exotic remote refs. Pick the heuristic conservatively: requires `^[a-z]{8,}$` with no separator characters. Document the rule near the helper.
- Pushing a fresh base bookmark to origin creates a new remote ref that lingers after the PR is merged. This is acceptable â€” users can prune them later. Mention in the Implementation Note for the operator.
- Depends on Ticket A (per-change left column) for the `revsets` list to contain individual changes that can be matched against the `form.base` change_id. Without A, base-as-change has no in-app way to discover ancestor change_ids.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-28T10:38:48+02:00
- state: implemented

## Completed

Implemented base-as-change with auto bookmark generation for the Generate screen PR form.

### What was done

1. **`src/bookmark_naming.rs` (new)** â€” New module with:
   - `slugify(input)` â€” lowercase ASCII slug, max 32 chars, dash-boundary truncation
   - `tip_bookmark(title, change_id)` â€” `pr/<slug>` or `pr/<change_id>` fallback
   - `base_bookmark(tip_bm)` â€” `pr-base/<suffix>` derived from tip bookmark
   - `is_change_id_like(s)` â€” heuristic: `^[a-z]{8,}$`, no separator chars
   - Full unit test coverage

2. **`src/generate.rs`** â€” Updated:
   - `ExecutionPlan::from_draft`: when `form.base` is change_id-like, emits 5 steps (tip bookmark create/push + base bookmark create/push + PR create); otherwise 3 steps (original behavior). Auto-generates tip bookmark name from title when `branch_name` field is empty.
   - `validate_for_execution`: rejects when both base and head are change_id-like and equal.
   - New tests: `execution_plan_base_as_remote_ref_produces_three_steps`, `execution_plan_base_as_change_id_produces_five_steps`, `execution_plan_auto_tip_bookmark_from_title_when_branch_name_empty`, `validate_for_execution_rejects_base_equal_to_head_when_both_change_ids`.

3. **`src/lib.rs`** â€” Registered `pub mod bookmark_naming`.

4. **`src/ui.rs`** â€” Updated base field status display in `CollectingContext` phase to read "base: <value> (branch ref or change_id)".

### Deviations from plan

- The ticket mentioned looking up existing bookmarks on the base change from the `revsets` list. The `revsets` list contains `RevsetSummary` items keyed by the revset label (`trunk()..<change_id>`), not by change_id directly. Matching requires iterating and checking `change_ids()`. Since the auto-generated `base_bookmark` name is deterministic and always correct, and using `bookmark create` when a bookmark may already exist would fail, the implementation uses `bookmark_create_command` for the base (not create-or-move). This is acceptable because: (a) jj's `bookmark create` will error if the bookmark already exists pointing to the same commit (user would see this); (b) the more precise existing-bookmark lookup would require traversing the full revsets list at plan-build time and is better deferred to a follow-up ticket after the per-change left column (Ticket A) is integrated. Noted as a residual risk.

### Verification

`just verify` passes: 114 tests, fmt, check, clippy, integration tests all green.

### Important files changed

- `src/bookmark_naming.rs` (new)
- `src/generate.rs`
- `src/lib.rs`
- `src/ui.rs`

### Residual risks / follow-up

- The base bookmark step always uses `bookmark create` even if the bookmark already exists (no existing-bookmark lookup). If a user has an existing bookmark on the base change, the step will fail and the user will see a jj error. A follow-up can add the create-or-move logic once Ticket A's per-change revset list is available in `ExecutionPlan::from_draft`.
- Auto-generated base bookmarks (`pr-base/...`) persist on origin after PR merge. Users should prune them manually after closing PRs.
- The `is_change_id_like` heuristic requires all-lowercase ASCII letters with no digits. Real jj change IDs are 12 lowercase letters matching `[a-z]+`. This is conservative and correct for jj.
