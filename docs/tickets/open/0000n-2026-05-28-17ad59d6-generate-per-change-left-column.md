---
id: 0000n-2026-05-28-17ad59d6-generate-per-change-left-column
created_at: 2026-05-28T10:00:37+02:00
created_by_model: claude-opus-4-7/high
state: open
---
# Generate Screen: Per-Change Left Column (`trunk()..@`)

## Goal

Replace the three-preset "Changes" list in the Generate screen with a per-change list of every change in `trunk()..@`. Each row identifies one change. Selecting a row sets the "tip" of the PR scope. No execution-plan changes â€” the scope is still expressed as a string revset (`trunk()..<change_id>`) consumed by the existing pipeline.

## Context

Today the left column on the Generate screen renders three preset revsets defined in `src/jj.rs:11`:

```rust
const CANDIDATE_REVSETS: &[&str] = &["@", "@-", "heads(trunk()..)"];
```

`spawn_revset_discovery` calls `JjClient::candidate_revsets`, which produces one `RevsetSummary` per preset. The UI lists those three in `render_menu` (Generate branch, `src/ui.rs:244-255`). Selection moves `GenerateState::selected_revset`. The selected `RevsetSummary` drives `selected_revset()` reads in `render_generate_preview` and `ExecutionPlan::from_draft` (`src/generate.rs:909`).

We want each row to represent one individual change in `trunk()..@`. Selection picks the "tip" â€” the PR scope becomes `trunk()..<selected change_id>`. The PR scope is still a single jj revset string used by the rest of the pipeline (prompt context, freshness check, execution plan). The data type that flows through is still `RevsetSummary`, but it now represents the range `trunk()..<change_id>` rather than a single preset expression.

Row display logic per change (single line, wraps to fit the 28-char column):

- Primary identifier:
  - If bookmarks non-empty: first bookmark name, **bold** (use `colors::ACCENT`-bold or a dedicated bookmark style).
  - Else if `description()` non-empty and not a jj placeholder (`(no description set)` etc., reuse `is_jj_default_description` from `src/ui.rs`): description first line.
  - Else: abbreviated change_id.
- Secondary tag (muted): if primary is bookmark and description is meaningful, append the description first line truncated; otherwise nothing.
- Wrap text inside the row (use `Wrap { trim: false }` or render each row as a multi-line block).
- Insert a thin separator (`â”€` line, dynamic width matching the inner column) between rows.

Bookmark highlight constraint from the user: space is tight, so do **not** add extra prefix/suffix chars â€” use bold styling on the bookmark text itself.

The PR-scope range is `trunk()..<change_id>`. The "head" form field continues to be the change_id (the selected change). Existing freshness, context collection, prompt building, and execution remain unchanged because they accept the revset string emitted from the new logic.

## Non-Goals

- Changing what "base" means or the execution plan. Base remains `main@origin` (or whatever `repo.base_branch.name` resolves to). Auto-bookmark generation for base lives in a separate ticket.
- Multi-select or range selection of changes.
- Caching/incremental refresh of the per-change list. A re-run of discovery on `r` is sufficient.
- Right-column redesign â€” that is a separate ticket.
- Gating landingâ†’Generate transition while discovery is loading â€” separate ticket.

## Design Decisions

- **Discovery query.** Replace `CANDIDATE_REVSETS` with a single discovery query: enumerate the changes in `trunk()..@` (oldest-first, i.e. closest to trunk at top â€” natural reading order; matches how `jj log -r 'trunk()..@'` reads). Empty result (`@` itself is on trunk or no changes above trunk): show a placeholder row "no changes above trunk()" and keep behavior degraded but stable (no panics; selecting it triggers a soft error in form validation).
- **One `RevsetSummary` per change.** Reuse the existing `RevsetSummary` struct unchanged. For each change, build a `RevsetSummary` where `label` = `trunk()..<change_id>` (this becomes the scope string), `description` / `bookmarks` / `change_ids` / `commit_ids` / `stats` reflect *just that one change*. `commit_count` = 1. `recent_log` = one entry.
  - Rationale: the rest of the codebase (`ExecutionPlan::from_draft`, freshness check, prompt) treats the selected `RevsetSummary` as "the scope of the PR". Setting `label = trunk()..<change_id>` means the selected scope automatically includes everything from trunk to the selected change.
  - The `stats` field for a row should be the diff stat for `trunk()..<change_id>` (the full range from trunk), since that is the PR scope, not the single-change diff. The single-change description is what we render in the row label.
- **JJ command shape.**
  - One `jj log -r 'trunk()..@' --no-graph -T <template>` invocation enumerates the changes (gives us change_id, commit_id, bookmarks, description first line per change). Cheap.
  - Per change, we still need a `jj diff -r 'trunk()..<change_id>' --stat` to populate `stats`. Run these concurrently (`tokio::try_join_all` or sequential â€” they're cheap and the list is small).
  - Description body (multi-line description beyond first line) is **not** needed for this ticket. The left column shows first-line only; the right column (separate ticket) will fetch description body lazily.
- **Selection semantics.** `move_revset_up/down` continues to move `selected_revset` index. `sync_head_from_selected_revset` on `GenerateState` currently sets `form.head` to `revset.label()` and `form.branch_name` to the first bookmark. Keep this â€” the `label` is now `trunk()..<change_id>` and `head` field is documented as "selected scope tip". This is slightly misleading semantically but avoids ripping up the form schema; that cleanup belongs in the base-as-change ticket.
- **Wrap and separators.** In `render_menu`, replace the `List` widget with a manual paragraph-style render that supports wrapping per item. Use `ratatui::widgets::Paragraph` per row inside a vertical layout, or a custom list construction that emits `ListItem`s with multi-line `Text` and inserts a separator `ListItem` between them. Pick whichever yields correct selection styling (the selected row keeps `ACCENT` styling and the cursor marker `â–¶`).
  - Recommended approach: build a single `Paragraph` with `Wrap { trim: false }` whose `Vec<Line>` is `marker + styled row label + wrapped continuation lines + separator`. Selection styling applied per-line.
  - The marker `â–¶` only renders on the first line of the selected row.
- **Placeholder handling.** When `revsets` is the placeholder (empty/no-revsets state from `GenerateState::with_placeholder`), render a single muted row "no changes above trunk()"; the existing placeholder constructor stays valid.
- **List title.** Keep `"Changes"` (already renamed in 0000m).

## Implementation Plan

1. `src/jj.rs`:
   - Add `JjClient::changes_above_trunk(cwd)` that runs `jj log -r 'trunk()..@' --no-graph -T <LOG_TEMPLATE>` and returns `Vec<ParsedLogEntry>`.
   - Add `JjClient::diff_stats_for(cwd, revset)` if not already covered by `revset_diff_stats_command`/`capture`.
   - Add `pub async fn per_change_revsets(&self, cwd: &Path) -> Vec<RevsetSummary>` that:
     - Fetches the change list once.
     - For each parsed entry, builds the per-change `RevsetSummary` with `label = format!("trunk()..{}", change_id)` and fetches the diff stat for that revset.
     - Returns the vector. If the list is empty, returns a single placeholder `RevsetSummary` indicating "no changes above trunk()".
   - Remove `CANDIDATE_REVSETS` and `candidate_revsets`. Keep `revset_log_command` and `revset_diff_stats_command` â€” they are still used.
   - Update `spawn_revset_discovery` to call `per_change_revsets`.
   - Adjust existing unit tests in `src/jj.rs` that touch `CANDIDATE_REVSETS` or `candidate_revsets`. Add a parse test for a multi-entry `trunk()..@` log output.
2. `src/ui.rs`:
   - Replace `render_menu` Generate branch.
   - Build a `Vec<Line>` for the menu area: for each `RevsetSummary` in `app.generate().revsets`, append:
     1. `marker + " " + primary` styled as: bookmark in bold if first bookmark exists, else description first line, else abbreviated change_id.
     2. Wrap continuation: if primary text is longer than `inner_width`, fold into additional lines (use `textwrap::wrap` if available, or hand-rolled char-based wrap; do not byte-slice).
     3. Optional secondary line: when primary is the bookmark and description is meaningful, append a muted line with the description first line (truncated).
     4. A separator `Line::from("â”€".repeat(inner_width)).fg(colors::BORDER)` after each row except the last.
   - Selection styling: the selected row's primary line uses `colors::ACCENT` bold; bookmark-as-primary stays bold either way (highlight the bookmark even when not selected).
   - Render via `Paragraph::new(lines).block(...).wrap(Wrap { trim: false })`.
   - Remove (or simplify) the existing `revset_display_label` truncation helper; the wrap approach replaces single-line truncation. Keep `truncate_chars` and `is_jj_default_description` for use elsewhere.
3. `src/generate.rs`:
   - No struct changes. Confirm `sync_head_from_selected_revset` still does the right thing (sets head to label and branch_name to first bookmark) â€” this is acceptable for now; the base-as-change ticket revisits it.
4. Manual smoke: launch the app inside a jj workspace with several changes above trunk, confirm one row per change, selection moves between them, bookmarks display bold, descriptions wrap, separators draw between rows.
5. `just verify` passes.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "docs/tickets/reviewed/0000m-2026-05-27-70bb2caf-generate-screen-polish.md"],
  "likely_files": ["src/jj.rs", "src/ui.rs", "src/generate.rs", "src/app.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "Left column shows one row per change in trunk()..@",
    "Bookmark name displayed bold when present; otherwise description first line; otherwise abbreviated change_id",
    "Rows wrap to multiple lines when text is long; no byte-slice panics on multibyte chars",
    "Separator line drawn between rows, not after the last row",
    "Selection styling (ACCENT) applies to first line of selected row only; marker â–¶ only on first line",
    "Empty trunk()..@ shows the placeholder row without panicking",
    "Selected RevsetSummary.label() is trunk()..<change_id> so downstream code (ExecutionPlan, freshness, prompt) keeps working",
    "Existing tests updated; per_change_revsets has a parse test; just verify passes"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria

- Discovery emits one `RevsetSummary` per change in `trunk()..@`, with `label = trunk()..<change_id>`.
- Left column renders one wrapped row per change with a separator between rows.
- A change with a bookmark shows that bookmark name in bold as the primary identifier.
- A change without a bookmark shows the description first line; if that's also a jj placeholder, the abbreviated change_id is shown.
- Selection cycles through individual changes; `selected_revset()` returns the per-change summary.
- Empty `trunk()..@` does not panic; a single placeholder row is shown.
- `just verify` passes.

## Verification Plan

- Unit tests: parse a multi-entry log output via the new `per_change_revsets` helper; assert one summary per entry and correct labels.
- Visual: in a workspace with â‰¥2 changes above trunk, one of them bookmarked: confirm the bookmarked row shows bold bookmark, non-bookmarked rows show description first line, separators appear between rows, long descriptions wrap.
- Visual: empty workspace (`@` on trunk): confirm placeholder row, no crash.
- `just verify`.

## Files Likely Touched

- `src/jj.rs` â€” new `per_change_revsets`, removal of `CANDIDATE_REVSETS`/`candidate_revsets`.
- `src/ui.rs` â€” `render_menu` Generate branch rewritten as wrapped `Paragraph`-based render.
- `src/generate.rs` â€” minor; confirm `sync_head_from_selected_revset` still sensible.
- Tests in `src/jj.rs` and `src/ui.rs` updated.

## Risks

- Diff-stat fetch per change is N round-trips to `jj`. For repos with many changes above trunk this could be slow. Acceptable for the first cut; revisit if it bites. (Could be parallelized with `futures::future::join_all` if needed.)
- Wrapping inside a `Paragraph` interacts with selection styling. If selection styling visually bleeds into the wrapped continuation line, render the row label as multiple `Line`s with explicit per-line style instead of letting `Wrap` do it.
- The `RevsetSummary.label` field doubles as both human label and jj scope string. Setting it to `trunk()..<change_id>` means callers that print `revset.label()` for human consumption (e.g. status line, right column) will see the scope string. The right-column ticket should account for this; for now, accept the slight UX regression there since the right column is being redesigned anyway.
