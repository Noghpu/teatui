---
id: 0000o-2026-05-28-9b216151-generate-right-column-identifier-block
created_at: 2026-05-28T10:00:45+02:00
created_by_model: claude-opus-4-7/high
state: open
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
