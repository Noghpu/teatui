---
id: 0001d-2026-06-04-9cdbe3e8-stacked-pr-batched-prompt-parser
created_at: 2026-06-04T20:48:31+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-04T21:42:02+02:00
---
# Stacked PR: batched prompt builder and array parser

## Goal
Add the batched stacked-PR prompt builder (with a soft "stack intent" guidance
block) alongside the existing single-PR prompt, and an array parser that turns
one LLM response into one `StackDraft` per PR with per-row local fallbacks.
Data-only slice: pure builder + parser + tests. No job wiring or UI.

## Context
Slice 3 of `docs/stacked-pr-plan.md` (read it). The locked LLM decision is a
**single batched prompt** returning a JSON array, one entry per PR â€” not N
single-PR calls â€” so the model keeps per-PR drafts coherent.

Current single-PR code:
- `src/domain/prompt.rs` â€” `build_prompt(ctx: &ContextBundle, form: &PromptForm) -> PromptBuild`
  assembles SYSTEM + Instructions + Incoming Data Schema + Output JSON Schema +
  Context JSON sections via `push_section`, tracking a `PromptManifest`.
- `src/domain/llm.rs` â€” `parse_draft(raw) -> LlmResult` parses one object into a
  `GeneratedDraft { pr_type, branch_slug, title, description }`, strips ```json
  fences, normalizes the type and slug (`normalize_pr_type`,
  `normalize_branch_slug` via `slugify`), and falls back to first-line/rest for
  non-JSON.
- `slugify` lives in `src/domain/bookmark.rs`; `branch_from_draft` in
  `src/app.rs` builds `pr/{type}/{slug}`.
- Slice 1 added `StackIntent`, `StackDraft`, `StackPrInput` in
  `src/domain/stack.rs`. Slice 2 produces per-range `ContextBundle`s.

## Non-Goals
- No LLM job, no `App` wiring, no modal/UI (slice 4).
- Do not change the single-PR `build_prompt`/`parse_draft` behavior or output
  schema.

## Design Decisions
- **Stacked prompt builder** `build_stack_prompt(contexts: &[ContextBundle], inputs: &[StackPrInput], intent: &StackIntent, shared_labels: &[String], milestone: &str) -> PromptBuild`
  (final signature can group these into a small struct). It carries:
  - each PR range's context, tagged with its `change_index` (0-based, oldest
    first), reusing the existing per-range context rendering;
  - a **soft stack-intent block** built from the Form's overall title /
    description / branch, instructed as *guidance only*: "describe the overall
    goal of the whole stack as if it were a single PR; use them only to keep
    each PR's title and description consistent and pointing the same direction;
    do NOT copy them verbatim; each PR must describe its own slice";
  - the shared `labels` and `milestone` as additional context (assignees are
    not useful model context â€” omit them from the prompt);
  - an output schema specifying a JSON **array**, each item:
    `{ "change_index", "type", "branch_slug", "title", "description" }`.
  Track a `PromptManifest` like `build_prompt`.
- **Array parser** `parse_stack_drafts(raw: &str, inputs: &[StackPrInput]) -> Vec<StackDraft>`:
  - strip ```json fences (reuse `strip_code_fences`);
  - parse a top-level JSON array; **match rows to `inputs` by `change_index`**;
  - for any missing or malformed row, fill from local fallbacks:
    `type = "chore"`, `title` from the input `subject`, `description` from a
    short range summary, `branch_slug = slugify(subject)`;
  - normalize each `type`/`branch_slug` through the same helpers `parse_draft`
    uses (reuse `normalize_pr_type` / `normalize_branch_slug`, refactoring them
    to be shared rather than duplicated);
  - **one malformed row must not discard valid rows** â€” always return exactly
    `inputs.len()` drafts, in index order.
- Keep `StackDraft` -> bookmark naming consistent with single PRs: bookmarks are
  `pr/{type}/{slug}` built the same way `branch_from_draft` does (the bookmark
  itself is assembled when the plan is built in slice 4; this slice just yields
  normalized `pr_type` + `branch_slug`).

## Implementation Plan
1. `src/domain/llm.rs`:
   - Refactor `normalize_pr_type`/`normalize_branch_slug`/`strip_code_fences` to
     be reusable by both parsers (they already are module-private fns; expose to
     the stack parser within the crate).
   - Add `parse_stack_drafts` (+ a `StackDraft` row struct deserialize) with the
     per-row fallback behavior above.
   - Tests: a valid full array; an array with one malformed row (falls back,
     others intact); a missing index (filled by fallback); a non-array/garbage
     payload (every row falls back); slug normalization per row.
2. `src/domain/prompt.rs`:
   - Add `build_stack_prompt` reusing `push_section` and the per-range context
     rendering. Add the stack-intent + array output-schema sections.
   - Tests: the prompt contains the de-emphasized stack-intent guidance and the
     shared labels/milestone; the output schema requests an array keyed by
     `change_index`; assignees are not present.
3. Re-export any new public items from `src/domain/mod.rs`.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/stacked-pr-plan.md",
    "src/domain/prompt.rs",
    "src/domain/llm.rs",
    "src/domain/bookmark.rs",
    "src/domain/stack.rs",
    "src/app.rs"
  ],
  "likely_files": [
    "src/domain/prompt.rs",
    "src/domain/llm.rs",
    "src/domain/mod.rs"
  ],
  "verification_commands": ["just verify"],
  "review_focus": [
    "parse_stack_drafts always returns inputs.len() drafts in index order; one bad row never discards valid rows.",
    "Per-row fallback uses chore + slugify(subject) + subject title, matching the single-PR fallback spirit.",
    "Stack-intent block is clearly guidance-only and not copied verbatim; assignees are excluded from the prompt.",
    "Type/slug normalization is shared with parse_draft, not duplicated.",
    "Single-PR build_prompt/parse_draft behavior is unchanged."
  ],
  "jj_description_prefix": "prompt"
}
```

## Acceptance Criteria
- `build_stack_prompt` produces one prompt covering all ranges plus a soft
  stack-intent block and shared labels/milestone, requesting a JSON array keyed
  by `change_index`.
- `parse_stack_drafts` returns exactly one normalized `StackDraft` per input, in
  index order, filling missing/malformed rows from local fallbacks.
- Single-PR prompt/parse paths and tests are unchanged.
- `just verify` is green.

## Verification Plan
- `just verify`.
- Unit tests as listed for both the parser and the prompt builder.

## Files Likely Touched
- `src/domain/prompt.rs`
- `src/domain/llm.rs`
- `src/domain/mod.rs`

## Risks
- Models may return fewer/extra rows or wrong indices; matching strictly by
  `change_index` with fallback fill is the contract â€” test it.
- Sharing `normalize_*` helpers must not alter single-PR results.
- The stack-intent wording must discourage verbatim copying; weak wording will
  make every PR title identical.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-04T21:35:23+02:00
- state: implemented

## What was completed

Implemented slice 3 of the stacked-PR plan: batched prompt builder and array parser.

### `src/domain/llm.rs`

- Added `parse_stack_drafts(raw: &str, inputs: &[StackPrInput]) -> Vec<StackDraft>`.
  - Strips code fences, then parses as `Vec<serde_json::Value>` first so that a single malformed element does not discard valid ones.
  - Each element is individually re-parsed into `StackDraftJson`; non-objects are silently skipped.
  - Matches rows to `inputs` by `change_index`; fills missing/malformed rows from local fallbacks: `type = "chore"`, `title` from `input.subject`, `branch_slug = slugify(subject)`, empty description.
  - Normalizes each row's `type` and `branch_slug` through `normalize_pr_type` / `normalize_branch_slug` (the same helpers `parse_draft` uses â€” no duplication).
  - Always returns exactly `inputs.len()` drafts in index order.
- Added `StackDraftJson` private deserialize struct for per-row parsing.
- Added `fallback_draft` helper.
- Added 6 unit tests covering: valid full array, one malformed row with others intact, missing index filled by fallback, garbage payload (all fallback), slug normalization, code-fenced array.

### `src/domain/prompt.rs`

- Added `build_stack_prompt(contexts, inputs, intent, shared_labels, milestone) -> PromptBuild`.
  - Uses `STACK_SYSTEM`, `STACK_INSTRUCTIONS`, `STACK_INPUT_SCHEMA`, `STACK_OUTPUT_SCHEMA` constants.
  - Includes a "Stack Intent (Guidance Only)" section built from `StackIntent`, instructing the model not to copy the intent verbatim.
  - Shared labels and milestone are included; assignees are explicitly excluded.
  - Per-range context is rendered as a JSON array keyed by `change_index`, reusing `PromptChange` / `PromptDiff` serialization structs.
  - Tracks `PromptManifest` exactly like `build_prompt`.
- Added `render_stack_intent` and `render_ranges_json` helpers.
- Added 5 unit tests: guidance-only block present, shared labels/milestone present, output schema requests array with `change_index`, assignees excluded, manifest total_bytes matches prompt length.

### `src/domain/mod.rs`

- Re-exported `parse_stack_drafts` from `llm`.
- Re-exported `build_stack_prompt` from `prompt`.

## Deviations from plan

None. The ticket implementation plan was followed exactly. The `normalize_pr_type` / `normalize_branch_slug` / `strip_code_fences` helpers were already module-private in `llm.rs` and are reused directly by `parse_stack_drafts` in the same module â€” no refactoring needed.

## Verification

`just verify` is green (142 unit tests + 37 render smoke tests, 0 failures).

## Important files changed

- `src/domain/llm.rs`
- `src/domain/prompt.rs`
- `src/domain/mod.rs`

## Residual risks / follow-up

- Slice 4 will wire `build_stack_prompt` and `parse_stack_drafts` into the job/modal flow; type signatures are final for that handoff.
- The stack-intent wording says "guidance only" and "do NOT copy verbatim" â€” downstream review should confirm the wording discourages literal title copying.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-04T21:42:02+02:00
- state: reviewed

## Review Postmortem

Reviewed slice 3 of the stacked-PR plan: the batched stacked-PR prompt builder
(`build_stack_prompt`) and the per-row-fallback array parser
(`parse_stack_drafts`). Data-only slice; no job/UI wiring.

### Verdict

Accepted with a small test hardening. The implementation is correct against the
ticket contract, clean, idiomatic, and passes `just verify` (fmt + clippy
`-D warnings` + 142 unit + 37 render tests). No production-code changes were
needed.

### Verification (facts)

- `just verify` green before and after my change.
- After adding two tests: 144 unit tests (was 142) + 37 render tests pass; fmt
  and clippy clean.

### What I confirmed against the contract (facts)

- `parse_stack_drafts` always returns exactly `inputs.len()` drafts in index
  order. It parses the payload as `Vec<serde_json::Value>` first, then re-parses
  each element individually, so a single malformed element cannot discard valid
  ones. Non-array / garbage payloads `unwrap_or_default()` to empty and every row
  falls back. Matching is strictly by `change_index`.
- Per-row and full fallbacks both yield `type = "chore"`, title from
  `input.subject`, and `branch_slug = slugify(subject)`. I confirmed the inline
  empty-title path's `normalize_branch_slug("", title, type)` collapses to
  `slugify(title)`, so it matches `fallback_draft`'s direct `slugify(&title)`.
- `normalize_pr_type` / `normalize_branch_slug` / `strip_code_fences` are reused
  directly from the single-PR path in the same module â€” no duplication. The
  implementation note's claim that no refactor was needed (they were already
  module-private) is accurate.
- The stack-intent block is guidance-only: `STACK_INSTRUCTIONS` says "Do NOT copy
  the stack-intent title or description verbatim into any PR; each PR must
  describe its own slice," and `STACK_INPUT_SCHEMA` repeats "guidance only".
  Assignees are excluded from the prompt (verified by test and by reading the
  rendered JSON structs â€” only `shared_labels` / `shared_milestone` are sent).
- Single-PR `build_prompt` / `parse_draft` behavior is unchanged. The diff to
  `mod.rs` only adds re-exports; the existing functions and their tests are
  untouched. Diff is confined to `llm.rs`, `prompt.rs`, `mod.rs` plus the ticket
  move.

### Change I made

Added two focused unit tests in `src/domain/llm.rs` that pin down risks the
ticket explicitly names ("Models may return fewer/extra rows or wrong indices")
but that were previously only covered indirectly:

- `stack_drafts_extra_and_indexless_rows_do_not_displace_valid_ones`: a valid
  row, a structurally-valid object with no `change_index` (must be dropped, not
  mis-assigned), and an out-of-range `change_index: 9` (must be ignored). The
  unmatched input still falls back correctly.
- `stack_drafts_duplicate_index_is_deterministic_last_wins`: two rows claiming
  the same `change_index`; locks in the deterministic last-wins behavior of the
  `by_index` HashMap so a future refactor can't silently change it.

These are test-only; no production behavior changed.

### Deviation noted, intentionally not "fixed" (inference)

The ticket lists the per-row fallback `description` as coming "from a short range
summary." The implementation uses an empty description, which the implementation
note documents. I left this as-is: the parser's only input is `StackPrInput`
(`index`, `base`, `head`, `included_change_ids`, `subject`) â€” it has no diff
stat or range text, so any "summary" manufactured here would be lower quality
than what slice 4 can build from the real `ContextBundle`/`StackPlan`. Forcing a
synthetic description into the parser now would risk shipping fake prose to
`tea pr create`. Recommend slice 4 fill a meaningful fallback description from
the per-range context when assembling the plan.

### Minor observations (not blocking, left unchanged)

- When a matched row has an empty title but a valid `branch_slug`, the code
  discards the LLM slug and re-derives it from `subject`. Defensible ("title is
  the anchor"; an empty title implies a low-quality row), and within the
  contract. Not worth churn.
- Duplicate `change_index` rows resolve last-wins via HashMap insertion order;
  now locked by a test.
