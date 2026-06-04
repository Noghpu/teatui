---
id: 0001e-2026-06-04-a3665ad2-stacked-pr-review-modal
created_at: 2026-06-04T20:48:32+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-04T22:24:36+02:00
---
# Stacked PR: review modal, BulkPhase, Form bulk semantics, and `G` wiring

## Goal
Wire slices 1â€“3 into a working (push-less) bulk flow: pressing `G` in the
Changes pane assembles a `StackSelection`, runs context collection + the batched
LLM, and opens a **modal** that shows loading then a two-pane review (PR list +
per-PR form) where the user edits each PR's title/branch/description. Also apply
the Form's bulk semantics (derived read-only `head`). No push yet (slice 6).

## Context
Slice 4 of `docs/stacked-pr-plan.md` (read it â€” UX Flow Â§2â€“6, State Model,
Rendering). Prereqs from slices 1â€“3: `selected_heads`/`BulkPhase`/
`derive_stack_ranges` and the `Stack*` types (`src/domain/stack.rs`),
`StackContextJob` (`src/domain/context.rs`), `build_stack_prompt` +
`parse_stack_drafts` (`src/domain/prompt.rs`, `src/domain/llm.rs`).

Patterns to mirror:
- **Modal overlay**: `render_jj_op_dialog` in `src/screens/generate.rs` draws a
  centered `Clear`ed box over the panes; `render` calls it last. Input routes to
  it first in `src/screens/generate/input.rs` (`if state.jj_op_dialog.is_some()
  { return on_jj_dialog_key(..) }`). The picker modal is similar.
- **Job + result + absorb**: `App::start_generation` sets a phase, submits a job,
  and `App::absorb_payload` downcasts the result and advances the phase
  (`handle_context_result` then submits `LlmGenerateJob`; `handle_llm_result`).
  `Transition` (`src/screens/mod.rs`) carries user intents into
  `App::apply_transition`. `CancelHandle`/`gen_cancel` aborts an in-flight LLM.
- **Form widgets**: `src/screens/generate/form.rs` `TextFieldState` + the
  editing grammar in `on_editing_key` (Enter/Esc/`i`).
- `branch_from_draft` (`src/app.rs`) builds `pr/{type}/{slug}`.

## Non-Goals
- **No push / `p` / `P`** and no `tea`/`jj` mutation (slice 6).
- No collision/existing-PR blockers yet (slice 5 populates `blockers`); render
  the (empty) blocker area so slice 5 only fills it.
- Single-PR scalar flow (`GeneratePhase`) is untouched.

## Design Decisions
- **Finalize `BulkPhase`** (`src/screens/generate.rs`): `Idle`, `Collecting`,
  `Generating`, `Review { plan: StackPlan, cursor: usize, pushing: Option<usize> }`,
  `Failed { message: String }`. `pushing` is always `None` this slice. The bulk
  modal is open whenever `bulk != Idle`.
- **`G` (Changes pane, >=1 head selected, no busy job)** -> new
  `Transition::GenerateStack`. `App` handles it: `derive_stack_ranges` from the
  selection + form `base`; snapshot the Form's shared metadata
  (`labels`/`assignees`/`milestone`) and stack intent (`title`/`description`/
  `branch`) into a `StackSelection`; set `bulk = Collecting`; submit
  `StackContextJob`. Reuse the per-backend diff budget logic in
  `start_generation` (`CONTEXT_DIFF_BUDGET_BYTES` / `diff_budget_bytes`).
- **Batched LLM job**: factor the HTTP transport in `src/domain/llm.rs` so a new
  `StackLlmJob` reuses `call_ollama`/`call_openai`'s request/response handling
  but parses with `parse_stack_drafts`, returning a `StackLlmResult`
  (`Ready(Vec<StackDraft>)` / `Errored { message }` / `Cancelled`). Reuse the
  existing `CancelHandle` so `Esc` aborts it.
- **Plan assembly**: on `StackLlmResult::Ready`, build `StackPlan` â€” one
  `StackPlanItem` per input: `bookmark = pr/{type}/{slug}` (reuse the
  `branch_from_draft` logic, extracted to a shared helper), `title`/`description`
  from the draft, `status = Pending`, empty `warnings`/`blockers`. Carry shared
  metadata + intent on `StackPlan`. Set `bulk = Review { plan, cursor: 0,
  pushing: None }`.
- **`Esc` while `Collecting`/`Generating`** cancels generation (abort the LLM,
  drop to `bulk = Idle`, keep the selection). `Esc` in `Review` closes the modal
  to `Idle` (keep selection). Mirror `cancel_generation`.
- **Form bulk semantics** (selected_heads non-empty): force `head` to the oldest
  selected change and make it read-only (refuse `begin_edit`, show a
  `(from selection)` hint). `base` stays editable. Do not change other fields'
  widgets; the visible selection is the mode signal.
- **Modal master-detail**: left = PR list (oldest->newest; per row head
  id/subject, range, base, bookmark, status, blocker/warning marker). Right =
  per-PR editor for the highlighted item's `title`/`branch`/`description`,
  reusing `TextFieldState` (+ the existing editing grammar). `head`/`base` for
  that PR are read-only; shared metadata shows in the modal header. Header: base,
  PR count, shared-metadata summary, blocker count. Footer: navigate / edit /
  `Esc` (and reserve `p`/`P` labels for slice 6).
- `has_busy_job()` must report busy for `Collecting`/`Generating`/`Review {
  pushing: Some(_), .. }` so input gates and Tier D keys stay consistent.

## Implementation Plan
1. `src/screens/generate.rs`: finalize `BulkPhase`; add `has_busy_job()`; add a
   small per-PR editor state (three `TextFieldState`, or reuse a `PrForm`
   subset) seeded from the highlighted `StackPlanItem`; add `render_bulk_modal`
   (loading/generating/review) drawn last in `render`, after the jj dialog.
   Apply the derived read-only `head` in `render_form`/Form input.
2. `src/screens/generate/input.rs`: route keys to a `on_bulk_modal_key` first
   when `bulk != Idle` (navigate list up/down/`j`/`k`; `Enter`/`i` edit the
   focused per-PR field via the existing editing path; `Esc` cancel/close).
   Bind `G` in the `Pane::Menu` arm (>=1 selected, not busy) -> `GenerateStack`.
   Refuse `begin_edit` on `head` when in bulk mode.
3. `src/screens/mod.rs`: add `Transition::GenerateStack` and
   `Transition::CancelStack` (or reuse `CancelGeneration` for both).
4. `src/app.rs`: handle `GenerateStack` (assemble selection, submit
   `StackContextJob`); add `absorb_payload` arms for `StackContextResult`
   (-> submit `StackLlmJob`) and `StackLlmResult` (-> assemble `StackPlan`, enter
   `Review`); cancel path drops to `Idle`. Extract the `pr/{type}/{slug}` helper
   for reuse.
5. `src/domain/llm.rs`: add `StackLlmJob`/`StackLlmResult` reusing factored
   transport + `parse_stack_drafts`.
6. Tests: render smoke (`tests/render_smoke.rs`) for the modal in `Collecting`,
   `Generating`, `Review` (0/1/many items), and a small-terminal floor; unit
   tests for plan assembly (bookmarks, base chaining, statuses) and the
   derived-head Form rule. Add snapshot specs in `src/bin/ui-snapshots.rs` for
   loading and review. Run `just snapshots`.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/stacked-pr-plan.md",
    "src/screens/generate.rs",
    "src/screens/generate/input.rs",
    "src/screens/generate/form.rs",
    "src/app.rs",
    "src/domain/llm.rs",
    "src/domain/stack.rs",
    "src/screens/mod.rs",
    "tests/render_smoke.rs",
    "src/bin/ui-snapshots.rs"
  ],
  "likely_files": [
    "src/screens/generate.rs",
    "src/screens/generate/input.rs",
    "src/screens/generate/form.rs",
    "src/screens/mod.rs",
    "src/app.rs",
    "src/domain/llm.rs",
    "tests/render_smoke.rs",
    "src/bin/ui-snapshots.rs"
  ],
  "verification_commands": ["just verify", "just snapshots"],
  "review_focus": [
    "The bulk modal captures keys first while open and never leaks them to the panes; Esc cancels generation / closes review and keeps the selection.",
    "G assembles the StackSelection from the live Form (base + shared metadata + intent) and is gated to >=1 selected head with no busy job.",
    "StackLlmJob reuses the existing transport + CancelHandle and parses via parse_stack_drafts; the single-PR LLM path is unchanged.",
    "Plan assembly builds pr/{type}/{slug} bookmarks (shared helper, not duplicated), chains bases, and sets Pending status.",
    "head is read-only in bulk mode; the rest of the Form is unchanged; has_busy_job covers all bulk busy states.",
    "Render smoke + snapshots cover loading/generating/review incl. small terminal."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- `G` (Changes pane, >=1 selected head, no busy job) opens the modal, runs
  context + batched LLM, and lands in a two-pane `Review`.
- The modal shows a loading state during collection/generation; `Esc` cancels
  and keeps the selection.
- Review lists PRs oldest-to-newest with head/range/base/bookmark/status; the
  highlighted PR's title/branch/description are editable via the existing
  editing grammar; shared metadata shows in the header.
- In bulk mode the Form's `head` is derived (oldest selected) and read-only;
  other fields behave as before; the single-PR flow is unchanged.
- Render smoke + snapshots cover loading, generating, and review (incl. small
  terminal). `just verify` and `just snapshots` are green.

## Verification Plan
- `just verify`; `just snapshots` then eyeball `target/ui-snapshots/index.html`
  for the new modal states.
- Unit tests for plan assembly and the derived-head rule; render smoke for each
  modal state.

## Files Likely Touched
- `src/screens/generate.rs`, `src/screens/generate/input.rs`,
  `src/screens/generate/form.rs`, `src/screens/mod.rs`, `src/app.rs`,
  `src/domain/llm.rs`, `tests/render_smoke.rs`, `src/bin/ui-snapshots.rs`

## Risks
- This is the largest slice. Keep per-PR editing scoped to three reused
  `TextFieldState`s; do not rebuild the whole `PrForm` for N PRs.
- The modal must capture keys first (like the jj/picker modals) or navigation
  will leak to the panes underneath.
- Factoring the LLM transport must not change single-PR `parse_draft`/result
  behavior.
- `GenerateState` struct literals (render_smoke, ui-snapshots, in-module tests)
  must stay in sync with any new fields.
- Cancellation races: a late `StackContextResult`/`StackLlmResult` after `Esc`
  must be ignored by a stale-phase guard, mirroring the single-PR handlers.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5
- completed_at: 2026-06-04T22:15:43+02:00
- state: implemented

what was completed:
- Finished the stacked PR review-modal slice for ticket 0001e.
- Added BulkPhase Collecting/Generating/Review/Failed wiring, G from the Changes pane, stack context + batched LLM job handling, and modal-first input routing.
- Added per-PR review modal rendering and editable title/branch/description state, with generated StackPlan items chained through pr/{type}/{slug} bookmarks.
- Applied bulk Form semantics: selected heads drive the derived read-only head field while base and shared metadata remain editable.
- Added render smoke and snapshot scenarios for bulk collecting, generating, review, failed, and small-terminal states.

meaningful deviations from the plan:
- No push behavior was added, matching the ticket non-goal.
- Existing textarea edit semantics are preserved for the per-PR editor instead of introducing a new cancel model.

verification run:
- just verify: passed.
- just snapshots: passed; wrote 17 snapshots to target/ui-snapshots.

important files changed:
- src/app.rs
- src/domain/llm.rs
- src/domain/mod.rs
- src/screens/mod.rs
- src/screens/generate.rs
- src/screens/generate/input.rs
- src/bin/ui-snapshots.rs
- tests/render_smoke.rs

residual risks or follow-up work:
- Collision/existing-PR blockers and push actions remain deferred to later stacked-PR slices, as planned.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: unknown
- reviewed_at: 2026-06-04T22:24:36+02:00
- state: reviewed

## Review Postmortem

Metadata:
- reviewer_model: gpt-5
- reviewed_at: 2026-06-04T22:00:00+02:00
- state: reviewed

facts:
- The implementation satisfied the ticket's main surface: `G` starts the bulk flow, the modal captures input while open, loading/generating/review/failed states render, and the Form head is derived/read-only while selected heads exist.
- `just verify` passed after review changes.
- `just snapshots` passed and wrote 17 artifacts to `target/ui-snapshots`; direct artifact inspection confirmed the bulk collecting, generating, review, failed, and small-modal text snapshots are nonblank and contain the expected modal content.
- The in-app Browser `iab` backend was unavailable, so snapshot visual QA used the generated `.txt`/`.svg` artifacts directly.

review fixes applied:
- Preserved the exact submitted `StackPrInput` values through `StackContextResult::Ready` and `StackLlmResult::Ready`, then used those carried inputs for prompt construction, LLM fallback parsing, and final plan assembly.
- This removes a race where a background revset refresh could change live `StatusStore::revsets` between context collection and result handling, causing context bundles to be zipped with recomputed range metadata.
- Pre-clamped `bulk_list_scroll` to the rendered list height before applying natural scrolling, so reopening or re-rendering a shorter stack cannot leave the PR list scrolled beyond its content.
- Updated affected unit tests for the new carried-input result shape.

assessment:
- The reviewed slice is acceptable for the planned push-less modal review phase.
- Push behavior, collision checks, and existing-PR blockers remain deferred to later stacked-PR slices, matching the ticket non-goals.

verification:
- just verify: passed.
- just snapshots: passed.
