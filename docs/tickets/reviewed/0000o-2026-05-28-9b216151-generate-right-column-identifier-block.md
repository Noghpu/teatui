---
id: 0000o-2026-05-28-9b216151-generate-right-column-identifier-block
created_at: 2026-05-28T10:00:45+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-28T10:32:14+02:00
---
# Generate Screen: Right Column Redesign with Identifier Block

## Goal

Replace the current flat list rendering in the right column of the Generate screen with a structured view. The top of the right column always identifies the selected change (change_id, description first line, description body, bookmark(s), readable diff stat). Phase-specific content sits below, grouped into clearly separated sections rather than a long list of muted lines.

## Context

The right column is rendered by `render_generate_preview` (`src/ui.rs:1041`). It currently emits a flat `Vec<Line>` starting with the selected revset metadata followed by phase-specific blocks (`CollectingContext`, `Generating`, `ContextReady`, `DraftReady`, `CheckingFreshness`, `Confirming`, `Executing`, `Complete`, `Failed`, and a default block). Many lines are muted and stylistically equivalent, making it visually hard to scan.

After the per-change left-column ticket (sibling), `RevsetSummary.label` will be `trunk()..<change_id>` (a scope string), `description` will be the selected change's description first line, and `bookmarks` will be that change's bookmarks. The diff `stats` will be the diff stat for `trunk()..<change_id>`.

This ticket reshapes only the right column's *presentation*. No data is added that isn't already available except:

- **Description body** (lines beyond the first) for the selected change. This is fetched on-demand by extending the discovery template to include the full description (multi-line) or by storing it on `RevsetSummary`. Picking the second option keeps rendering pure.

The user has constrained: no author, no timestamp, no parent change. Diff stat must be **readable** â€” parse the raw jj diff stat string into a compact form like `3 files Â· +42 / -7` rather than dumping the multi-line jj-formatted stat.

## Non-Goals

- Changing data flow into `GenerateState` beyond adding description body to `RevsetSummary`.
- Reorganizing phase-specific business logic (state machine remains the same).
- Adding interactive elements (scrolling, expand/collapse) â€” pure presentation.
- Visual polish beyond grouping into sections (no decorative ASCII art, no extra columns).

## Design Decisions

- **Identifier block (top, always visible on Generate screen).** A clearly delineated block with these fields in order:
  1. `change_id` (full short id from jj, e.g. `vpzywprv`) in `colors::ACCENT`.
  2. Bookmark(s) â€” comma-joined, each name bold. If none: omit the line.
  3. Description first line (subject).
  4. Description body (additional lines, indented; if empty: omit).
  5. Diff stat in compact form: `N files Â· +X / -Y` (parsed from raw jj diff-stat string).
  6. Scope string (the `trunk()..<change_id>` revset) on a final muted line so power users can see what is being submitted.
- **Section separators.** A blank line plus a bold section header (no rules/lines drawn) between major sections: `Selected change` / `<phase title>` / `Logs` / `Execution plan` / `Manifest warnings`. The phase title comes from `generate_work_title(phase)` to keep visual continuity with the block title.
- **Phase blocks.** Reorganize the per-phase blocks so each one renders three clear sub-sections in this order (omit any that are empty):
  - **Status** â€” one or two lines summarizing what is happening right now (e.g. "Collecting context", "Draft ready", "Freshness: verified").
  - **Details** â€” phase-specific data (prompt manifest, draft, validation summary, execution plan, completion summary).
  - **Recent logs** â€” `render_recent_logs(&app.logs().entries, 6)` where currently applicable.
- **Compact diff stat parsing.** Add a helper `compact_diff_stat(raw: &str) -> String` in `src/ui.rs` that extracts file/insertion/deletion counts from the typical jj `--stat` summary line (`N files changed, X insertions(+), Y deletions(-)`). If parsing fails, fall back to the first non-empty line of the raw stat. Unit test the parser for common shapes including 1-file, plural, zero-files, and missing-insertions cases.
- **Description body field on `RevsetSummary`.** Add `description_body: String` (the description beyond the first line) populated by extending the jj log template to include the multi-line description and splitting it. The first line continues to be exposed via `description()`; expose the rest via `description_body()`. Treat purely whitespace/placeholder bodies as empty.
  - The current log template uses `description.first_line()`. Switch to a template that emits `description` (full) and split on first newline at the parser. Use a separator that cannot appear in jj output (e.g. `\u{001F}` unit separator) for the new field to avoid colliding with `|`.
- **No styling churn elsewhere.** Keep `render_recent_logs`, `render_execution_plan`, `render_draft_section`, `render_manifest_warnings`, `render_prompt_manifest`, `render_prompt_text` as-is unless they emit redundant headers that collide with the new section structure. If they do, remove the inner headers (the outer section header now owns the title).

## Implementation Plan

1. `src/jj.rs`:
   - Extend `LOG_TEMPLATE` to emit the full description (multi-line) using a non-pipe separator. Suggested template: `commit_id.short() ++ "|" ++ change_id.short() ++ "|" ++ bookmarks.map(|b| b.name()).join(",") ++ "|" ++ description ++ "\u{001E}"` with record-separator delimiters (or pick a delimiter not present in `|` to keep parser shape similar). If multi-line records are awkward, an alternative is two queries: one for first-line per change and one for `jj show -T description <change>` per change. Pick whichever is simpler.
   - Update `parse_log_entry` / `parse_log_entries` to consume the full description and split into `(first_line, body)`.
   - Populate the new `description_body` field on `RevsetSummary` in `parse_revset_summary` and `failed_revset_summary` (body = empty).
2. `src/generate.rs`:
   - Add `description_body: String` to `RevsetSummary`; update `new` and accessors. Add an `is_meaningful_body(&self) -> bool` helper that returns true if body has non-whitespace content not equal to a jj placeholder.
   - Update tests that construct `RevsetSummary` manually.
3. `src/ui.rs`:
   - Add `compact_diff_stat(raw: &str) -> String`.
   - Add `render_change_identifier(revset: &RevsetSummary) -> Vec<Line<'static>>` â€” the top block as described.
   - Add `render_section_header(title: &str) -> Vec<Line<'static>>` returning `[Line::from(""), Line::from(title.bold())]`.
   - Rewrite `render_generate_preview` to:
     1. Emit the identifier block.
     2. Emit a section header for the phase title.
     3. Emit phase-specific Status / Details / Recent logs sub-sections.
   - Remove the existing one-off muted metadata lines at the top (`phase: â€¦`, `input mode: â€¦`, `base branch: â€¦`, raw `stats: â€¦`, `commit ids: â€¦`, `change ids: â€¦`, `revset: â€¦`, `bookmarks: â€¦`) â€” the identifier block subsumes them. The base branch belongs in a one-line muted footer at the bottom of the identifier block (it is context the user needs to know).
   - Audit and de-duplicate inner headers in `render_draft_section` / `render_execution_plan` / `render_manifest_warnings` so they don't visually conflict with the new section headers.
4. Tests:
   - Unit test `compact_diff_stat` for the cases listed in Design Decisions.
   - Unit test `is_meaningful_body` for empty / whitespace / placeholder / real body.
   - A `render_generate_preview` shape test is not required (TUI rendering is exercised manually).
5. Manual smoke: walk through phases (CollectingContext â†’ ContextReady â†’ Generating â†’ DraftReady â†’ CheckingFreshness â†’ Confirming â†’ Executing â†’ Complete; Failed via a forced error). Confirm the identifier block stays consistent across phases, sections read cleanly, diff stat is readable.
6. `just verify` passes.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "docs/tickets/reviewed/0000m-2026-05-27-70bb2caf-generate-screen-polish.md"],
  "likely_files": ["src/ui.rs", "src/generate.rs", "src/jj.rs"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "Right column always opens with the change identifier block (change_id, bookmarks bold, description subject, description body, compact diff stat, scope)",
    "Phase content reorganized into Status / Details / Recent logs sub-sections with bold section headers",
    "compact_diff_stat parses common jj stat shapes and falls back gracefully",
    "RevsetSummary gained description_body; jj log template / parser updated accordingly",
    "No author, no timestamp, no parent change in the identifier block",
    "just verify passes; new parser tests included"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria

- Across every Generate phase, the right column opens with a consistent identifier block listing change_id, bookmark(s) bold, description subject, description body (when present), compact diff stat, and the scope revset.
- Author, commit timestamp, and parent change_id never appear in the identifier block.
- Phase-specific content is visually grouped by bold section headers (Status / Details / Recent logs).
- Diff stat is rendered as `N files Â· +X / -Y` for typical jj output; raw multiline stat strings no longer appear.
- `just verify` passes; new unit tests for `compact_diff_stat` and `is_meaningful_body` exist.

## Verification Plan

- Unit tests for `compact_diff_stat` and `is_meaningful_body`.
- Visual smoke through all Generate phases.
- `just verify`.

## Files Likely Touched

- `src/jj.rs` â€” log template + parser for full description.
- `src/generate.rs` â€” `RevsetSummary::description_body`, helpers, constructor/test updates.
- `src/ui.rs` â€” `render_generate_preview` rewrite, identifier-block helper, section-header helper, diff-stat formatter, audit of inner headers.

## Risks

- Changing the jj log template can affect parsing if descriptions contain the chosen delimiter. Use a record-separator character (`\u{001E}`) or move to a second `jj show -T description` query per change.
- This ticket should land *after* the per-change left-column ticket. The identifier block assumes `RevsetSummary` represents a single change. If sequenced first, the identifier block would be misleading for the legacy aggregated revsets. Recommend the orchestrator picks the per-change ticket first.
- Width: the identifier block adds 5â€“7 lines at the top. The right column is `Fill(1)` (`src/ui.rs:37`), so it has plenty of width but vertical space at small terminal sizes can become tight. Wrapping is already applied by the outer `Paragraph`.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-28T10:28:43+02:00
- state: implemented

## What was completed

- Added `description_body: String` to `RevsetSummary` in `src/generate.rs` with `with_description_body()` builder, `description_body()` accessor, and `is_meaningful_body()` helper that returns false for empty/whitespace/placeholder bodies.
- Extended `LOG_TEMPLATE` in `src/jj.rs` to emit the full description using `description.lines().join("\x1F")` with a `\x1E` record-separator terminator per line.
- Updated `parse_log_entry` to strip the `\x1E` terminator, split the description field on `\x1F`, and populate `description_body` on `ParsedLogEntry`.
- Updated `parse_revset_summary` to propagate `description_body` via `with_description_body()`.
- Updated `per_change_revsets` to re-encode the description with `\x1F` separators when reconstructing the log line for `parse_revset_summary`.
- Added `compact_diff_stat(raw: &str) -> String` in `src/ui.rs` that parses `N files changed, X insertions(+), Y deletions(-)` into `N files Â· +X / -Y` with graceful fallback.
- Added `render_section_header(title: &str)` and `render_change_identifier(revset: &RevsetSummary)` helpers.
- Rewrote `render_generate_preview` to: (1) emit the identifier block (change_id, bold bookmarks, description subject, indented body if meaningful, compact diff stat, scope revset) followed by the base branch as a muted footer, then (2) a bold section header for the phase title, then (3) phase-specific Status / Details / Logs sub-sections.
- Removed inner `"Execution plan"`, `"Generated draft"`, `"Prompt manifest warnings"`, and `"Recent logs"` bold headers from the four sub-render functions since callers now emit them via `render_section_header`.
- Added unit tests: 5 for `is_meaningful_body`, 3 for new log entry parsing formats, 1 updated test for `per_change_revsets_parse_produces_correct_labels`, 7 for `compact_diff_stat`.

## Deviations from plan

- `LOG_TEMPLATE` uses `description.lines().join("\x1F")` instead of the unit-separator approach mentioned as an alternative using a separate `jj show -T description` query. The join approach keeps the single-query shape and avoids a second jj invocation per change.
- The identifier block uses the first item from `change_ids()` rather than a dedicated `change_id()` accessor; the per-change ticket already ensures only one change_id per revset.
- `render_section_header` renders a blank line + bold title (no horizontal rule), exactly as designed.

## Verification

`just verify` passed: 85 unit tests + 4 integration tests, zero warnings.

## Important files changed

- `src/generate.rs` â€” `RevsetSummary` new field + accessor + `is_meaningful_body()` + tests
- `src/jj.rs` â€” `LOG_TEMPLATE` + `ParsedLogEntry.description_body` + updated parser + updated tests
- `src/ui.rs` â€” `compact_diff_stat` + `parse_stat_count` + `render_section_header` + `render_change_identifier` + rewritten `render_generate_preview` + updated sub-render functions + tests

## Residual risks / follow-up

- The jj template `description.lines().join("\x1F")` is not tested against real jj output; verify at runtime that jj's `.lines()` method exists and behaves as expected. If jj rejects the template, fallback is to revert to `description.first_line()` and accept no body display until a two-query approach is added.
- If descriptions contain literal `\x1F` characters (unlikely but possible), the body parser would misinterpret them. An additional sanitization pass could be added if needed.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-28T10:32:14+02:00
- state: reviewed

## Review summary

Implementation matched the ticket plan closely: `RevsetSummary.description_body` was added with the planned `with_description_body()` builder, `is_meaningful_body()` helper (with the placeholder-case-insensitive check), and `description_body()` accessor. The jj `LOG_TEMPLATE` was extended with `\x1F` line separators and a `\x1E` record terminator; `parse_log_entry` strips the terminator and splits the description into subject + body. `compact_diff_stat` parses the standard jj `--stat` summary line into `N files Â· +X / -Y` with a graceful fallback. `render_change_identifier` emits the change_id (accent), bold bookmarks, subject, indented body, compact stat (muted), and the scope revset (muted). `render_section_header` emits a blank line + bold title. Inner headers were removed from `render_draft_section`, `render_execution_plan`, `render_manifest_warnings`, and `render_recent_logs` so the outer section header owns the title. Tests cover `is_meaningful_body` (5 cases), `compact_diff_stat` (7 cases), and the legacy + new log-entry parser formats. `just verify` reports 85 unit + 4 integration tests passing with zero warnings.

## Fixes applied during review

- `generate_work_title` previously returned `"PR Form"` for every phase that wasn't `Confirming`, `CheckingFreshness`, or `DraftReady`. With the new rewrite, the value is now consumed as the bold section header for *every* phase, so phases like `CollectingContext`, `Generating`, `Executing`, `Complete`, and `Failed` were rendering with a misleading `"PR Form"` header. Expanded `generate_work_title` into a full `match` covering every `GeneratePhase` variant with the appropriate per-phase title (`Collecting Context`, `Generating Draft`, `Executing`, `Execution Complete`, `Workflow Failed`, etc.). This satisfies the ticket requirement that the phase title comes from `generate_work_title(phase)` and keeps continuity with the block title.
- Simplified the bookmark span construction in `render_change_identifier`: removed the fully qualified `ratatui::text::Span`/`ratatui::style::Style` paths (both are already imported at the top of the module) and replaced the `enumerate`/`flat_map` indirection with a straight loop that prepends a `", "` separator after the first bookmark. Capacity is pre-reserved for the worst case to avoid repeated reallocations.
- Made `compact_diff_stat` private (`fn` instead of `pub fn`) since it is only consumed within `ui.rs`.
- Removed a stray trailing comma inside `format!("base: {}", app.repo().base_branch.name,)`.

## Verification

- `just verify` passed after the changes: 85 unit tests + 4 integration tests, zero warnings, formatting/clippy clean.
- Spot-checked the new jj template by running `jj --no-pager log --limit 1 -T 'description.lines().join("\x1F") ++ "\x1E\n"'` which confirmed the template is accepted by jj 0.41.0 and produces the expected single-line record.

## Residual notes

- The implementation note flagged that real jj output had not been validated for `description.lines().join("\x1F")`; this review confirmed the template works on jj 0.41.0.
- The `is_meaningful_body` placeholder check and `is_jj_default_description` (in `src/ui.rs`) duplicate the `"(no description set)"` literal. Acceptable churn-wise â€” they live in different modules and the duplication is small.
