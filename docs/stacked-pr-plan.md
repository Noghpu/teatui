# Stacked PR LLM Generation Plan

Source of truth for the **full stacked-PR generation** feature. Tier D
(in-pane jj stack shaping) is already implemented and is treated as foundation,
not future scope.

Update this doc as decisions evolve. Tickets carved from it live under
`docs/tickets/open`.

## Status

- **Phase:** design locked 2026-06-04; carving implementation tickets.
- **Implemented foundation:** in-pane jj stack shaping from the Changes pane:
  squash with below, move above, move below, confirmation/error modals,
  conflict probing, `jj undo` on introduced conflicts, and a visible
  `JjMutating` busy phase.
- **Remaining scope:** the full stacked PR flow: multi-select PR heads ->
  describe the stack in the existing Form -> batched LLM drafts -> review in a
  dedicated modal -> bookmark, push, and create PRs as a stacked chain.
- **Screen decision:** bulk generation is integrated into the existing Generate
  screen/mode. The default three-pane screen (Changes / Form / Preview) is
  **unchanged**; bulk is additive. Review + push happen in a **modal overlay**,
  not by restructuring the main panes.

### Resolved Decisions (2026-06-04)

- **Default view unchanged.** Opening PR-gen still shows Changes / Form /
  Preview exactly as today. Bulk capability is layered on; no fields are removed
  and no panes are restructured.
- **Selection.** In the Changes pane, **`space`** toggles the current row as a
  selected PR head (multi-select). `space` follows the Gmail/lazygit convention
  and is free in Changes today. Selected heads render with a distinct marker and
  a live count; a `G review stack` footer hint appears only when >=1 is selected.
- **The Form describes the whole stack in bulk.** When >=1 head is selected, the
  *same* Form fields take a stack-level meaning (the visible multi-selection is
  the mode signal, so nothing is hidden or greyed):
  - **title / description / branch** = the overall goal of the stack *as if it
    were a single PR*. These are **soft guidance** fed to the LLM, not literal
    per-PR output (see prompt below).
  - **labels / assignees / milestone** = **shared**, applied to every PR. Labels
    and milestone are also sent to the LLM as context.
  - **head** = **derived, read-only** = the oldest selected change (foot of the
    stack / head of PR 1). Editing is intentionally disabled — `base` plus the
    selection carry all the flexibility, and an editable head would require
    invalid-state validation and disabled list rows.
  - **base** = **editable** (default `main`). This is the lever for manual
    resume / append: point `base` at an already-pushed bookmark to generate a
    new batch *on top of* an existing remote stack. `base` + `head` describe the
    *first PR of this batch*, not necessarily the bottom of the whole remote
    stack.
- **LLM call: single batched prompt.** One prompt carries the whole stack (the
  per-range contexts plus the soft stack-intent block) and the model returns a
  JSON array, one entry per PR. Needs an array parser with per-row fallback and
  a diff budget divided across ranges. (Not N looped single-PR calls — the batch
  is what lets the model keep the per-PR drafts coherent.)
- **Review / push: a dedicated modal.** `G` opens a modal that shows *loading*
  while context + LLM run (`Esc` cancels generation). When ready it becomes a
  two-pane review — **PR list on the left, a per-PR form on the right**
  (master-detail, mirroring the Changes+Form grammar) — for editing each PR's
  **title / branch / description**. Push from the modal: **`p`** pushes the
  highlighted PR, **`P`** pushes the whole stack.
- **Resume: in-session, manual.** Per-PR push *is* the resume mechanism: push
  stops on the first failure; fix the cause and press `p` again, or `P` to walk
  the rest. Earlier PRs must be pushed before later ones (a later PR's base is
  the earlier PR's bookmark). Nothing is persisted to disk; relaunch starts
  fresh and re-validates against current repo/server state.
- **Diff budget (default):** reuse the existing total `CONTEXT_DIFF_BUDGET_BYTES`
  as a whole-prompt budget divided across ranges, falling back to stat-only for
  a range whose share drops below a floor.

## Goal

The current Generate flow creates one PR from one selected change. The new flow
lets the user select multiple **PR heads** in the Changes pane and generate a
stacked chain of PRs. Selected heads do not have to be contiguous. Any
unselected changes between two selected heads become part of the later PR's
range.

Example, oldest to newest:

```text
main, 1, 2, 3, 4, 5
selected heads: 1, 4

PR 1: base=main, head=1
PR 2: base=<bookmark for 1>, head=4, includes changes 2..4

change 5 remains outside this app's responsibility for now
```

The app does not try to rebase leftover changes after merge. The user shapes
the stack first with the already-implemented Tier D jj operations, then runs
the stacked PR generation flow.

## Non-Negotiable Constraints

- **Review before mutate.** No bookmark, push, or PR creation runs until the
  user is in the review modal and explicitly presses `p` / `P`.
- **One PR per selected head.** Gaps are included in the next selected head's
  PR range.
- **Full LLM drafts.** The batched LLM call returns branch slug, title, and
  description for each PR.
- **Refuse existing PRs.** If a selected head/bookmark already appears to have
  a PR, do not update it. Show a blocking error on that plan item and refuse to
  push it.
- **Refuse bookmark collisions.** Detect collisions with proposed bookmarks and
  existing local/remote bookmarks before push. Fast-fail with a clear error;
  do not offer automatic suffixes or cleanup.
- **Green path only.** This tool is convenience automation for ordinary clean
  stacks. If bookmark/PR state is out of the ordinary, stop and require cleanup
  through the traditional jj/tea/Gitea paths.
- **Partial failure is explicit and manually resumable.** Push is sequential and
  stops on the first failed step, showing which PR/step failed and which
  previous PRs succeeded (with URLs). The user fixes the cause and re-pushes;
  the app re-checks state before re-pushing so it does not duplicate work.
- **Reuse existing command builders.** Bulk push composes the same jj/tea
  command construction as `ExecutePrJob`, not duplicated shell behavior in
  screen code.
- **Responsive while busy.** While context, LLM, or push jobs run, navigation
  can remain responsive, but mutating actions are blocked and the UI must
  visibly explain what is running.

## Existing Foundation

| Building block | Location | Reuse |
|---|---|---|
| Changes list, cursor, scrolling | `screens/generate.rs` | add selected-head markers |
| PR Form fields + widgets | `screens/generate/form.rs` | stack-level inputs in bulk; reuse `TextFieldState` in the modal |
| Tier D stack shaping | `domain/jj_mutate.rs`, Generate screen | user prepares stack before generation |
| `slugify()` | `domain/bookmark.rs` | fallback / normalized branch slugs |
| Single PR command flow | `domain/execute.rs` | reuse `ExecutePrJob` step builders for per-PR push |
| Per-change context collection | `domain/context.rs` | extend for per-range stack context |
| Prompt building | `domain/prompt.rs` | add batched stacked-PR prompt + stack-intent block |
| LLM parsing | `domain/llm.rs` | add array parser with per-row fallback |
| Revset metadata | `domain/probe.rs` | selected head ordering, bookmarks; existing-PR / collision probes |
| jj op modal | `screens/generate.rs` (`render_jj_op_dialog`) | pattern for the bulk modal overlay |
| Render smoke contract | `tests/render_smoke.rs` | add every new bulk phase/state |

## UX Flow

### 1. Select PR heads (Changes pane)

- `space` toggles the current row as a selected PR head.
- The cursor (`revset_selected`) and the selected-head set are separate concepts.
- Selections are stored by `change_id`, not row index, so refreshes/reorders do
  not corrupt the set. Stale ids (no longer in `StatusStore::revsets`) are
  dropped on read.
- The Changes pane is newest-first. The bulk flow derives oldest-to-newest order
  by reversing the selected heads in display order.
- Render: a selected-head marker in addition to the cursor marker, plus a live
  selected count, plus a `G review stack` footer hint shown only when >=1 head
  is selected.

### 2. Describe the stack (Form pane)

With >=1 head selected the Form's fields are read as stack-level inputs (no
layout change; the selection markers are the mode signal):

- **head** is forced to the oldest selected change and made read-only, with a
  `(from selection)` hint.
- **base** stays editable (default `main`); it is PR 1's base and the
  resume/append lever.
- **title / description / branch** are the overall stack intent (soft guidance).
- **labels / assignees / milestone** are shared across all PRs.

### 3. `G` opens the review modal (loading)

- `G` from the Changes pane (>=1 head selected, no other job/modal active) opens
  the bulk modal and starts generation. It first derives PR ranges:
  - PR 1: `base..oldest_selected`
  - PR k: `prev_selected..this_selected` (unselected gap changes fold in)
- Validate before LLM: selected heads still exist; ordered on the stack; the
  range is non-empty. (Collision / existing-PR checks land before push, step 6.)
- The modal shows a *loading* state (collecting context, then generating).
  `Esc` cancels: the modal closes, generation is aborted, the selection is kept.

### 4. Collect stack context

One context bundle per PR range, oldest-to-newest. Each includes:

- base expression and head change id
- selected head subject/body
- all changes in that range, including unselected gap changes
- per-range diff stat
- aggregate diff for the range, budgeted (total budget divided across ranges;
  stat-only when a range's share is below a floor)

Extend `domain/context.rs` rather than forking a parallel command style. Keep
the snapshot rule: one working-copy snapshot where needed, then read-only
commands with `--ignore-working-copy`.

### 5. Generate batched LLM drafts

Add a stacked prompt builder alongside the single-PR prompt. It carries every
range context plus a **soft stack-intent block** built from the Form's overall
title / description / branch, instructed roughly as:

> **Stack intent (guidance only):** title / description / branch describe the
> overall goal of the whole stack as if it were a single PR. Use them only to
> keep each PR's title and description consistent and pointing the same
> direction. Do **not** copy them verbatim into any PR; each PR must describe
> its own slice.

Labels and milestone are included as additional context. The model output is an
array, one item per PR range:

```json
[
  {
    "change_index": 0,
    "type": "feat",
    "branch_slug": "add-parser-cache",
    "title": "Add parser cache",
    "description": "..."
  }
]
```

Parser requirements:

- Match rows by `change_index`.
- If a row is missing or malformed, fill it with local fallbacks:
  `type = "chore"`, `branch_slug = slugify(subject)`, title from subject,
  description from the range summary.
- Normalize branch slugs through `slugify()`; build bookmarks `pr/{type}/{slug}`
  the same way `branch_from_draft` does for single PRs.
- One malformed row must not discard valid rows.

There is no fixed maximum number of selected heads; rely on context budgets,
truncation, and clear progress/error reporting, not a hard UI cap.

### 6. Review + push (modal, two-pane)

When generation completes the modal becomes a two-pane review that mirrors the
Changes+Form grammar:

- **Left — PR list** (oldest-to-newest): per row the head id/subject, included
  range, base, generated bookmark, push status, and any blocker/warning marker.
- **Right — per-PR form**: edit the highlighted PR's **title / branch /
  description** (reuse `TextFieldState` + undo/redo). `head` and `base` for that
  PR are read-only; the shared labels/assignees/milestone show in the modal
  header (read-only here — they were set in the main Form).
- **Header**: base, PR count, shared-metadata summary, blocker count.
- **Footer**: contextual keymap (navigate, edit, `p` push current, `P` push all,
  `Esc` close).

Push (reuses `ExecutePrJob`'s builders per PR):

1. `jj bookmark set --allow-backwards <bookmark> -r <head>`
2. `jj git push --bookmark <bookmark>`
3. `tea pr create --base <base> --head <bookmark> --title --description` plus the
   shared `--labels` / `--assignees` / `--milestone` flags when non-empty.

For PR 1 `<base>` is the Form base; for PR k `<base>` is PR k-1's bookmark. Per
PR the modal tracks status: `pending` -> `bookmarked` -> `pushed` ->
`created{url}` -> or `failed{step, message}`.

- `p` pushes the highlighted PR. It is refused (blocker) if an earlier PR in the
  chain is not yet `created` (its base bookmark would not exist).
- `P` walks the stack oldest-to-newest, stopping on the first failure.
- On failure, completed URLs stay visible; the user fixes the cause and presses
  `p`/`P` again. Before re-pushing an item the app re-checks its bookmark/PR
  state so it does not duplicate work. This is the in-session resume.

## State Model

Keep the existing single-PR flow intact. Add parallel bulk state inside the
Generate screen rather than a separate screen.

Recommended shape (planning anchor, not mandatory API):

```rust
pub struct GenerateState {
    // existing fields ...
    pub selected_heads: Vec<String>, // change_id; stable set, newest-first display order
    pub bulk: BulkPhase,
}

pub enum BulkPhase {
    Idle,
    Collecting,                 // modal: "Collecting context..."
    Generating,                 // modal: "Generating drafts..."
    Review {                    // modal: two-pane review + push
        plan: StackPlan,
        cursor: usize,          // highlighted PR
        pushing: Option<usize>, // index of the PR whose push job is in flight
    },
    Failed { message: String }, // collection/generation failure (modal shows error)
}
```

Use `GeneratePhase` for the scalar PR path and `BulkPhase` for the stack path. A
helper such as `GenerateState::has_busy_job()` must cover scalar jobs, bulk jobs
(`Collecting` / `Generating` / `Review { pushing: Some(_), .. }`), and Tier D
`JjMutating`, so input gates stay consistent. The bulk modal is open whenever
`bulk` is not `Idle`; while open it captures keys like the existing jj/picker
modals.

## Domain Types

Expected additions (keep the landed shape close to this):

```rust
pub struct StackIntent {        // overall guidance from the Form, bulk mode
    pub title: String,
    pub description: String,
    pub branch: String,
}

pub struct StackPrInput {
    pub index: usize,
    pub base: String,           // PR 1 = form base; PR k = prev bookmark (filled at plan build)
    pub head: String,
    pub included_change_ids: Vec<String>,
    pub subject: String,
}

pub struct StackSelection {     // assembled at `G`, drives context + prompt
    pub items: Vec<StackPrInput>,
    pub intent: StackIntent,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
}

pub struct StackDraft {         // one parsed LLM row
    pub index: usize,
    pub pr_type: String,
    pub branch_slug: String,
    pub title: String,
    pub description: String,
}

pub enum PrStatus {
    Pending,
    Bookmarked,
    Pushed,
    Created { url: String },
    Failed { step: ExecuteStep, message: String },
}

pub struct StackPlanItem {
    pub input: StackPrInput,
    pub bookmark: String,
    pub title: String,
    pub description: String,
    pub status: PrStatus,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
}

pub struct StackPlan {
    pub items: Vec<StackPlanItem>,
    pub labels: Vec<String>,    // shared, applied to every PR's tea create
    pub assignees: Vec<String>,
    pub milestone: String,
    pub intent: StackIntent,
}
```

## Collision And Existing PR Checks

Checks run when the plan is built (entering Review) and again per item just
before its push. Every bookmark/PR conflict is a fast-fail condition surfaced as
a `blocker` on the plan item; the app never invents alternative names, updates
existing PRs, deletes bookmarks, or repairs unusual state.

Check against:

- duplicate generated bookmarks within the plan
- local bookmarks on `RevsetSummary`
- remote bookmarks from bookmark probes / `jj bookmark list`
- existing PRs found by tea (`tea pr list --output json`)

On bookmark collision: refuse the item's push, show a blocker naming the
colliding bookmark, require manual cleanup/renaming outside this flow.

Existing-PR scenarios:

- **Open PR for the same head bookmark** (main case): refuse, show the existing
  PR.
- **Closed/merged PR for the same head bookmark**: out-of-ordinary; refuse.
- **Remote bookmark without an obvious PR**: treat as a bookmark collision;
  refuse.
- **Same change pushed under a different bookmark**: detect only if tea/jj
  exposes it cheaply; otherwise out of scope for v1.

Blocked items keep their blocker visible and cannot be pushed until the user
resolves the state and regenerates / re-checks.

## Input Summary

Changes pane:

- `space` — toggle selected head
- `G` — open the review modal and generate from selected heads (>=1 selected)
- existing `Enter` (focus Form), `r` (refresh), `s` / `J` / `K` (Tier D) keep
  their meaning; Tier D keys stay active only when no bulk/scalar job is running

Review modal:

- `up` / `down` / `j` / `k` — move the PR-list cursor (swaps the right form)
- `Enter` / `i` — edit the focused per-PR field (same as the main Form)
- `p` — push the highlighted PR; `P` — push the whole stack
- `Esc` — cancel generation (while loading) or close the modal (keeps selection)

Pane navigation on the main screen stays on the existing arrow / `h` / `l`
bindings.

## Rendering Summary

Changes pane:

- cursor marker + selected-head marker + selected count
- `G review stack` footer hint when >=1 head selected
- busy/disabled visual when a job blocks mutation

Form pane (bulk mode): `head` shown read-only with `(from selection)`; other
fields unchanged.

Bulk modal:

- loading / generating: status + selected count, `Esc cancel`
- review: header (base, count, shared metadata, blocker count); left PR list
  (blockers/warnings first per row, push status badges); right per-PR form
- push in flight: the active PR's step; completed PRs show URLs
- done: all URLs in stack order; failed: failed PR/step plus completed URLs

## Tests

Unit:

- selection toggles by change id and survives reorder/refresh; stale ids dropped
- ranges derive oldest-to-newest with gaps folded into the later PR
- derived head = oldest selected; base passes through from the Form
- batched parser accepts valid arrays and falls back per malformed row; one bad
  row does not discard valid rows
- stacked prompt includes the soft stack-intent block and shared labels/milestone
- bookmark/existing-PR collision detection maps to blockers
- `p` is refused when an earlier PR is not yet created (ordering blocker)
- `P` stops on the first failed step; completed items keep their URLs
- input gates block Tier D / scalar / bulk mutations while any job is in flight

Render smoke:

- Changes pane with zero, one, and multiple selected heads
- bulk modal: loading, generating, review, pushing, done, failed
- review with a bookmark-collision blocker and with an existing-PR blocker
- small terminal floor for every bulk modal state

Verification: `just verify` for handoff; `just snapshots` after UI changes.

## Implementation Order

One feature, landed as reviewable slices. Data-only slices (context, prompt,
parser, checks) are independently testable without UI.

1. **Selection + ranges + types.** `selected_heads` set, `space` toggle, Changes
   markers/count/footer hint, the `Stack*` domain types, and pure stack-range
   derivation with tests. No generation yet.
2. **Stack context collection.** Per-range bundles in `domain/context.rs` with
   budget division; data-only + tests.
3. **Batched prompt + array parser.** Stacked prompt builder with the
   stack-intent block (`domain/prompt.rs`) and the per-row-fallback array parser
   (`domain/llm.rs`); data-only + tests.
4. **Review modal + Form bulk semantics + `G` wiring.** `BulkPhase`, the modal
   (loading/generating/review with master-detail per-PR editing), derived
   read-only head, and `G` assembling the `StackSelection` and running slices
   2-3. Read-only review (no push yet). Render smoke + snapshots.
5. **Collision / existing-PR checks.** Bookmark + `tea pr list` probes/parsers
   and plan-item blockers shown in the modal.
6. **Push.** Per-PR (`p`) and whole-stack (`P`) push reusing `ExecutePrJob`
   builders, per-item status, ordering blocker, stop-on-first-failure, in-session
   re-push resume; push UI + done/failed states; render smoke + snapshots.

Each slice keeps `just verify` green.

## Open Questions For User

_None outstanding._

## Completed Foundation Post-Mortem

### Tier D - In-pane jj management

- Added conflict-safe jj stack shaping from the Changes pane:
  `s` squashes the selected row into the visual row below, `J` / `Ctrl+Up` /
  `Ctrl+k` moves the row above, and `K` / `Ctrl+Down` / `Ctrl+j` moves the row
  below. The menu is newest-first, so move-up uses
  `jj rebase -r <change> --insert-after <above>` and move-down uses
  `--insert-before <below>`.
- Added a generic jj operation modal for confirmation and errors. Boundary
  failures show a popup instead of silently doing nothing.
- `JjMutateJob` blocks on pre-existing conflicts, runs the jj mutation, probes
  conflicts again with `self.conflict()`, and runs `jj undo` on introduced
  conflicts. Squash passes `--use-destination-message` to avoid opening an
  editor.
- Generate now has an explicit `JjMutating` phase. Navigation remains
  responsive, but mutating actions are blocked and the Preview pane shows the
  running operation. Successful jj mutations reset PR generation state to idle
  and refresh revsets.
- Tests added for command construction, input fallback keys, boundary/error
  behavior, render smoke for `JjMutating`, and confirm/error dialogs. Snapshot
  generation includes the jj surfaces.
- Verification: `just verify` green; `just snapshots` writes 12 artifacts.
