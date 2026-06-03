# Stacked PR Bulk Bookmark Plan

Source of truth for the **bulk stacked-PR** feature: select several mutable
changes in the Changes pane, generate slugified bookmarks for each (LLM-drafted),
then push and open a stacked chain of PRs where each PR targets the previous
change's bookmark.

Update this doc as decisions evolve. Tickets carved from it live under
`docs/tickets/open`.

## Status

- **Phase:** Tier D implemented; Tier A not started.
- **Scope decision:** full A+B+C (multi-select → LLM slugs → stacked push + PR
  creation), plus **Tier D** (in-pane jj management). Tiers A, B, and D are
  shippable checkpoints, not throwaway steps. D is independent of A–C and can
  land first (it needs no LLM and improves the everyday flow on its own).

## Goal

Today the generate flow is single-change: one `PrForm`, one head/base, one draft,
one `tea pr create`. The state machine (`GenerateState` / `GeneratePhase`) is
entirely scalar. This feature adds a parallel bulk flow that operates on N
selected changes at once and wires them into a **stacked** PR chain:

```
trunk() ─< c1 ─< c2 ─< c3        (selected, oldest→newest)
            │     │     │
   bookmark slug1 slug2 slug3
       PR  base=trunk  base=slug1  base=slug2
```

## What we reuse (already built)

| Building block | Location | Reuse |
|---|---|---|
| `slugify()` — branch-safe, collapse, 64-char truncate | `domain/bookmark.rs` | direct |
| `jj bookmark set --allow-backwards <name> -r <change>` | `domain/execute.rs:53` | loop it |
| `jj git push --bookmark <name>` | `domain/execute.rs:71` | loop it |
| `tea pr create --base … --head … --title … --description …` | `domain/execute.rs:85` | per change, base chained |
| Per-change context (subject/body/diff_stat, oldest→newest) | `domain/context.rs` | feeds the slug prompt |
| Single-draft LLM `{type, branch_slug, title}` + schema | `domain/prompt.rs`, `domain/llm.rs` | extend to an array schema |
| Changes pane listing + selection + bookmark prefill | `screens/generate.rs` | extend to multi-select |
| `RevsetSummary` rows (change_id, bookmarks, description, stats) | `domain/probe.rs` | per-change source data |

## Non-negotiable constraints

- **Review before mutate.** No bookmark set, push, or PR create runs until the
  user confirms the full plan. Matches the existing design principle.
- **Reuse, don't fork.** The bulk job composes the same jj/tea command builders
  used by `ExecutePrJob`; do not duplicate command construction.
- **Partial failure is reported, not hidden.** N changes → up to 3N steps. A
  failure stops the chain (a later PR's base may not exist yet) and the UI shows
  which step of which change failed and which changes already succeeded.

---

## Phases

### Tier A — Multi-select + bulk bookmark-only (shippable)

Select N mutable changes, slugify each from its commit subject, set N bookmarks.
The selected changes do **not** have to be contiguous. Each selected change is a
PR head, and any unselected changes between two selected heads are included in
the later PR's range. Example, oldest→newest stack `main, 1, 2, 3, 4, 5` with
heads `1` and `4` selected:

- PR 1: base `main`, head `1`.
- PR 2: base bookmark from PR 1, head `4`, including changes `2..4`.
- Change `5` is left alone; rebasing it after merges is outside the app for now.

No LLM, no push, no PR.

- **State:** add a selection set to `GenerateState` (e.g. `selected: Vec<usize>`
  or a `HashSet<change_id>`). Keep the existing `revset_selected` cursor for
  navigation; selection is layered on top.
- **Input:** a toggle key (Space) in the Changes pane; "select range" optional.
  Update `input.rs` and the footer help hints.
- **Render:** per-row selection markers in `render_menu`; a selection count.
- **Job:** new `SetBookmarksJob` looping `jj bookmark set` over the selected
  heads in oldest→newest order. Collision detection across the proposed slugs
  and against existing local/remote bookmarks happens before running. On
  collision, refuse, show a warning, offer a suffixed replacement, and let the
  user retry.
- **Confirm UI:** a per-change list (change → proposed slug), editable slug,
  collision flags.

**Risk:** low. Slug source and jj command both already exist.

### Tier B — LLM-generated slugs (batched)

Replace local subject-slugs with one LLM call returning an array.

- **Prompt:** new template alongside the single-PR one in `domain/prompt.rs`.
  Output schema becomes an array:
  `[{ change_index, type, branch_slug, title, description }]`. Reuse the
  per-change context already collected by `context.rs`.
- **Parse:** new response parser in `domain/llm.rs` for the array shape, with a
  **fallback to local slugify** per change if the model omits or malforms a row.
- **Wiring:** a `GenerateSlugsJob` (LLM) feeding the Tier A confirm list.

**Risk:** medium — array prompt design + robust parsing/fallback.

### Tier C — Stacked push + PR creation (full feature)

Extend execution from "set bookmarks" to the full stacked chain.

- **Ordering / base chain:** sort the selection oldest→newest; the first PR's
  base is the chosen trunk/base, each subsequent PR's base is the previous
  change's bookmark.
- **Execution:** extend the bulk job to, per change in order: `bookmark set` →
  `git push --bookmark` → `tea pr create --base <prev-bookmark>`. Reuse
  `ExecutePrJob`'s command builders.
- **Per-PR titles/descriptions:** use the Tier B batched LLM drafts: branch name,
  title, and description per selected head / PR range.
- **Existing PRs:** if the stack appears to already have PRs for the target
  heads/bookmarks, refuse and show an error/warning. Updating existing PRs is
  out of scope.
- **Status UI:** a `BulkPhase` (parallel to `GeneratePhase`) tracking per-change
  step status (pending / bookmarked / pushed / PR-created / failed) and the
  resulting PR URLs.

**Risk:** medium-high — base-chain correctness and partial-failure recovery
across N PRs are the fiddly parts.

### Tier D — In-pane jj management (independent, can land first) — done

Because the feature commits to **one PR per change**, the stack the user picks
must be the stack they want. Tier D adds minimal, conflict-safe stack editing
directly in the Changes pane so they can shape it without leaving the app.

#### Operations

| Action | Key(s) | jj command (on the cursor's change, by `change_id`) |
|---|---|---|
| Squash with below | `s` | `jj squash --from <change> --into <below>` |
| Move change up | `J` / `Ctrl+Up` / `Ctrl+k` | `jj rebase -r <change> --insert-after <above>` |
| Move change down | `K` / `Ctrl+Down` / `Ctrl+j` | `jj rebase -r <change> --insert-before <below>` |

- These operate on the **cursor's single change** (`revset_selected`), *not* the
  bulk multi-select set from Tier A. Keep the two concepts separate.
- The Changes menu is **newest-first**. "Above" means the visually previous row
  toward `@`; "below" means the visually next row toward `trunk()`. The confirm
  dialog must use those words literally.
- Operations are inert at the boundaries (squash/move-down on the last row,
  move-up on the first) and while any job is in flight. Show a popup error for
  trunk/immutable rows rather than letting an operation target them.
- Tier D operations are active only in Normal mode, only in the Changes pane,
  and only when no modal / generation / execution / jj mutation is running.

#### Confirmation dialog (new, generic)

Each op opens a confirmation modal **before** running. The existing `Confirming`
phase is PR-execution-specific, so add a generic dialog:

- New `Option<JjOpDialog>` (or a small modal enum) on `GenerateState`, holding
  the pending op + a human summary ("Squash `<id> subject` into `<id> subject`?
  This rewrites both changes."). `Enter` confirms, `Esc` cancels.
- Render as an overlay; reuse `render_picker_modal`'s centered-box styling
  (`generate.rs:624`) rather than inventing new modal chrome.

#### Conflict-safe execution + auto-revert

**Key jj fact:** rebase/squash *never fail on conflict* — jj records the conflict
as a first-class object inside the commit and the command **succeeds (exit 0)**.
So exit code tells us nothing; we must probe the result. jj's own docs recommend
exactly this run-then-`jj undo` pattern, so it's idiomatic, not a workaround.
([conflicts docs](https://docs.jj-vcs.dev/latest/conflicts/))

1. **Block on pre-existing conflicts** in the relevant stack range. If conflicts
   already exist, show an error dialog and do not mutate.
2. **Run** the jj command (squash / rebase). jj rebases descendants
   automatically and marks any that don't apply cleanly as conflicted. This
   command snapshots the working copy first (capturing pending edits before the
   rewrite) — desired here.
3. **Probe for conflicts** with `--ignore-working-copy` (the snapshot was just
   taken in step 1; skip the redundant rescan — see the snapshot-cost note
   below):
   `jj log --ignore-working-copy -r '<range>' --no-graph -T 'if(self.conflict(), "C", "")'`
   — any `C` means the op introduced a conflict. For Tier D, derive the smallest
   useful range from the operation: the moved/squashed change plus affected
   descendants, at least through the following change. `trunk()..@` remains an
   acceptable conservative fallback because it matches the displayed list.
   Note `self.conflict()` is the jj 0.41 template *method*; the bare `conflict`
   keyword is gone.
4. **On conflict, revert with `jj undo`** — it reverses exactly the last
   operation (our rebase/squash) and nothing else, leaving any pre-mutation
   auto-snapshot intact. Then pop an **error dialog**: the op was reverted
   because it would conflict. We deliberately do *not* capture an op id up front
   (`jj op restore`'s job); `jj undo` is the leaner, more idiomatic primitive and
   needs no snapshot bookkeeping.
5. On success, dismiss the confirm dialog.
6. Either way the repo changed, so re-emit `RefreshRevsets` to reload the
   Changes pane (and invalidate revset stats).

This runs as a new background job `JjMutateJob` in `domain/` (sibling of
`execute.rs`), returning `Applied | Reverted { reason } | Errored { message }`.
Keep command construction in the job; the screen only dispatches a `Transition`.
`jj undo` is considered safe enough for this workflow; keep it simple.

#### Wiring

- New `Transition` variants (e.g. `JjOp(JjOp)` carrying squash / move-up /
  move-down + the target `change_id`), dispatched from `input.rs` in the Menu
  pane only.
- App layer submits `JjMutateJob` and maps its outcome to the success-dismiss or
  error-dialog state.
- Add a general, obvious in-progress state for jj mutation. While it is active,
  navigation remains responsive but mutating actions are blocked, and the
  Preview pane shows what operation is running.
- Any successful jj operation resets PR generation state back to idle; stack
  shaping happens before generation/review.
- Footer help hints + new tests for key handling and boundary inertness.

**Risk:** medium. `jj undo` revert is reliable. The remaining sharp edge is
choosing the smallest conflict probe range; `trunk()..@` is conservative and
matches the current displayed list.

---

## State-model note (cross-cutting, the real cost)

The jj/slug mechanics are cheap; the cost is breaking the **scalar single-draft
state model**. `GenerateState` assumes one `PrForm` / one draft / one head-base.
Two viable shapes:

1. **Parallel `BulkPhase`** inside the generate screen, with a per-change result
   list rendered in the preview pane. Smaller blast radius; reuses panes.
2. **Dedicated bulk screen.** Cleaner separation; more new render/input code.

Recommendation: option 1 (parallel phase) to maximize reuse of the existing
three-pane layout, multi-select living in the Changes pane.

Most touched files: `screens/generate.rs` (~1.5k lines), `screens/generate/input.rs`,
`domain/prompt.rs`, `domain/llm.rs`, `domain/execute.rs` (new bulk job).

## Rough effort

- Tier A: ~1 day (low risk).
- Tier A+B: ~2–2.5 days.
- Tier A+B+C (full): ~4–5 days (medium-high risk).
- Tier D (in-pane jj management): ~1.5–2 days (medium risk); independent of A–C.

## Open questions

- Tier D conflict probe range can start conservative (`trunk()..@`) but should
  be tightened if the UX starts surfacing unrelated conflicts too often.

## Post-mortems

### Tier D — In-pane jj management

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
  generation now includes the new jj surfaces.
- Verification: `just verify` green; `just snapshots` writes 12 artifacts.
