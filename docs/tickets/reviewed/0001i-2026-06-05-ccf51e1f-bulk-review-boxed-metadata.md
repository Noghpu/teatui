---
id: 0001i-2026-06-05-ccf51e1f-bulk-review-boxed-metadata
created_at: 2026-06-05T17:24:08+02:00
created_by_model: gpt-5/medium
state: reviewed
state_updated_at: 2026-06-05T20:53:10+02:00
---
# Bulk review modal: boxed panes with shared metadata footer

## Goal
Make the boxed-pane bulk review modal layout the production UI and move the shared stack metadata out of the top header into a footer-style row below the two pane boxes.

The review modal should read as a master-detail surface with two framed panes: the left pane is the PR stack list, and the right pane is the selected PR form. Shared information such as base branch, PR count, labels, milestone, and blocker count belongs below those pane boxes, inside the modal border.

## Context
The active rewrite is Linux-only and the Generate screen owns the stacked PR review modal. Read `docs/rewrite-plan.md` before changing behavior.

The stacked PR review modal is rendered from `src/screens/generate.rs`:

- `render_bulk_review` currently builds `header_parts` from shared stack metadata and renders that line above the body.
- A recent snapshot-comparison change added `BulkReviewSeparation`, `render_with_bulk_review_separation`, and `render_bulk_review_body` variants for `Bar`, `Gutter`, `SectionHeaders`, and `BoxedPanes`.
- The user chose the boxed-pane option. The production `render` path must use boxed panes, not the old one-column separator.
- `render_bulk_pr_list` owns natural scrolling through `GenerateState::bulk_list_scroll`; preserve that behavior.
- `render_bulk_pr_form` renders per-PR head/base/status and editable fields; keep per-PR details inside the right pane.

The deterministic snapshot binary in `src/bin/ui-snapshots.rs` currently emits comparison snapshots for the bulk review separator variants. Once boxed panes are chosen, update snapshot output so the canonical `generate-bulk-review` artifact shows the production boxed layout with shared metadata below the boxes.

The previous implementation ticket for the modal was `docs/tickets/implemented/0001h-2026-06-05-c7c65713-bulk-review-modal-focus-separator.md`. It is useful context for focus, scrolling, and push-state constraints.

## Non-Goals
Do not change stacked PR generation, LLM parsing, context collection, blocker detection, push execution, or single-PR Generate behavior.

Do not change the two-step review focus behavior: list focus, preview/form focus, and edit activation should continue to work as already implemented.

Do not add a new screen or move the bulk review modal out of the Generate screen.

Do not introduce persistent UI settings for the separator style. Boxed panes are the selected design.

## Design Decisions
Use boxed panes as the only production bulk review modal layout.

The top of the modal should contain only the modal title/border from `theme::modal_block("Review Stacked PRs")`; remove the shared metadata line from above the pane body.

Inside the modal border, split the review content vertically into:

1. A fill area containing the two boxed panes side by side.
2. A one-line shared metadata footer below the boxes.

The metadata footer should include the same shared information currently assembled in `header_parts`: base, PR count, labels when present, milestone when present, and blocker count when nonzero. It should be visually quiet, using existing theme helpers such as `theme::muted()` or `theme::status_line` style patterns. If the line is too wide, truncate gracefully rather than wrapping over the pane boxes or modal border.

Keep per-PR `head`, `base`, and `push status` inside the right `Selected PR` pane. The shared footer is for stack-level context only.

Use existing pane/block styling, preferably `theme::pane_block`, for the `Stack` and `Selected PR` boxes. The active pane border should still reflect `BulkReviewFocus`.

Collapse the temporary comparison plumbing unless it remains clearly useful for another snapshot purpose:

- Production `screens::generate::render` must no longer default to `BulkReviewSeparation::Bar`.
- If `BulkReviewSeparation` and `render_with_bulk_review_separation` are kept, they must not obscure the production path, and the canonical snapshot must render `BoxedPanes`.
- Prefer removing obsolete `Bar`, `Gutter`, and `SectionHeaders` snapshot specs after the choice is made, so the snapshot index does not keep presenting rejected designs as first-class UI states.

Small terminal handling must stay robust. If there is not enough height to render both pane boxes and the footer row, keep a compact fallback that shows the shared metadata without panicking or overlapping text.

## Implementation Plan
1. In `src/screens/generate.rs`, update `render_bulk_review` so it no longer renders the shared metadata above the body.
2. Split the modal inner area into a boxed-pane body and a bottom metadata row. Preserve the existing small-height fallback for tiny terminals.
3. Make boxed panes the production layout:
   - Render the left `Stack` pane and right `Selected PR` pane using existing theme block helpers.
   - Preserve focus-aware borders for `BulkReviewFocus::List` and `BulkReviewFocus::Preview`.
   - Keep `render_bulk_pr_list` and `render_bulk_pr_form` behavior intact except for area plumbing.
4. Render the shared metadata footer below the boxes. Reuse the existing metadata content, but make it width-aware and quiet. Do not let it wrap over or push text outside the modal.
5. Clean up the snapshot-only variant code from the comparison pass where appropriate:
   - Remove obsolete separator variants from `src/bin/ui-snapshots.rs`, or at minimum make `generate-bulk-review` the boxed production layout and stop emphasizing rejected variants in the default snapshot list.
   - Remove dead helpers in `src/screens/generate.rs` if the alternative layouts are no longer referenced.
6. Update or add focused render smoke coverage only as needed by the changed layout shape. Existing bulk review smoke tests should continue to render without panic, including wrapped titles, preview focus, push-in-flight, done, failed item, and blocker states.
7. Run `just verify`, then `just snapshots`. Inspect `target/ui-snapshots/generate-bulk-review.txt` or `.svg` and `target/ui-snapshots/index.html` to confirm boxed panes are the canonical review modal and shared metadata appears below the boxes.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "docs/tickets/implemented/0001h-2026-06-05-c7c65713-bulk-review-modal-focus-separator.md"
  ],
  "likely_files": [
    "src/screens/generate.rs",
    "src/bin/ui-snapshots.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "The production bulk review modal uses boxed Stack and Selected PR panes, not the old single-column vertical separator.",
    "Shared stack metadata is rendered below the pane boxes and no shared metadata line remains above them.",
    "The metadata footer is width-aware and does not overlap the pane boxes, modal border, or app footer.",
    "Existing bulk review focus and editing behavior remains unchanged.",
    "The canonical generate-bulk-review snapshot shows the chosen boxed layout with metadata below the boxes."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- The stacked PR bulk review modal renders two boxed panes labeled for the stack list and selected PR details.
- The production render path uses the boxed-pane layout by default.
- The old shared metadata row above the panes is removed.
- Shared stack metadata appears below the boxed panes, inside the modal border.
- The metadata footer includes base, PR count, labels when present, milestone when present, and blocker count when nonzero.
- Metadata footer rendering is width-aware and does not overlap or wrap into surrounding UI.
- Existing list/preview focus transitions, field editing, list scrolling, and push-in-flight behavior are preserved.
- The canonical `generate-bulk-review` snapshot reflects the boxed production layout with metadata below the boxes.
- `just verify` and `just snapshots` pass.

## Verification Plan
Run `just verify` for formatting, compile checks, clippy, unit tests, and render smoke tests.

Run `just snapshots` after the UI change. Inspect `target/ui-snapshots/generate-bulk-review.txt`, `target/ui-snapshots/generate-bulk-review.svg`, and `target/ui-snapshots/index.html` to confirm the boxed panes are canonical and the shared metadata row appears below the boxes.

If the implementation changes render layout helpers or removes snapshot variants, update affected render smoke expectations by rendering the existing bulk review phases and focus states against `TestBackend` without adding broad snapshot assertions.

## Files Likely Touched
- `src/screens/generate.rs`
- `src/bin/ui-snapshots.rs`
- `tests/render_smoke.rs`

## Risks
The bottom metadata row consumes vertical space from the pane boxes. Keep small-terminal behavior explicit so the modal does not hide the focused list row or truncate the selected PR form into incoherence.

The snapshot-comparison code may have been added only to support design selection. Removing it is fine, but do not accidentally remove the canonical snapshot fixture or the normal render smoke coverage.

The boxed panes add borders around content that already uses leading spaces. Check the deterministic text snapshot so the left list marker, wrapped titles, right form fields, and footer line still align cleanly.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-06-05T17:44:10+02:00
- state: implemented

Completed the bulk review modal rewrite so the production Generate screen now renders the boxed Stack and Selected PR panes with a quiet shared metadata footer below them. Removed the temporary separator/comparison plumbing and moved the shared labels metadata out of the form pane.

Deviations: I kept the existing per-PR status/result lines inside the right pane; the ticket only required relocating the stack-wide metadata footer. I also removed the obsolete snapshot comparison variants rather than keeping them around as hidden helpers.

Verification: `cargo fmt`, `just verify`, and `just snapshots` all passed.

Files changed: `src/screens/generate.rs`, `src/bin/ui-snapshots.rs`, `tests/render_smoke.rs` was unchanged.

Residual risk: the footer is width-aware and the small-terminal fallback is intentionally compact, but the modal still needs a visual sanity check on a very narrow terminal if the layout constraints change later.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-05T20:53:10+02:00
- state: reviewed

# Review: 0001i bulk review modal boxed panes with shared metadata footer

## Verdict
Approved. No changes required.

## What was verified
- Production `screens::generate::render` no longer threads a `BulkReviewSeparation`; the temporary `render_with_bulk_review_separation`, `render_bulk_review_body`, `render_bulk_separator`, `render_bulk_section_header`, and the `BulkReviewSeparation` enum are all removed. No dangling references remain (grep clean).
- `render_bulk_review` splits the modal inner area into `Fill(1)` body + `Length(1)` footer. The body renders two framed panes via `theme::pane_block("Stack", ...)` and `theme::pane_block("Selected PR", ...)`, with focus-aware borders driven by `BulkReviewFocus`. Matches the chosen boxed-pane design.
- Shared stack metadata (base, PR count, labels when present, milestone when present, blocker count when nonzero) now renders below the boxes via `bulk_review_footer_line` using `theme::StatusChip::plain` + `theme::status_line`, which is width-aware and sheds chips under pressure instead of wrapping. The old `header_parts` row above the body and the shared `labels` block inside the form pane are both gone.
- Small-terminal handling is robust: the footer always renders, and a zero-size body returns early before laying out panes. `generate_bulk_small_terminal_each_phase_renders` passes without panic.
- Snapshot binary cleanup: rejected `gutter`, `section-headers`, and `boxed-panes` comparison specs/kinds removed; canonical `generate-bulk-review` now drives the production `render` path. Confirmed no `*gutter*`/`*section-headers*`/`*boxed-panes*` snapshot artifacts are emitted.

## Verification run
- `just verify`: fmt --check clean, clippy --all-targets --all-features -D warnings clean, all unit + render-smoke tests pass (174 + 50).
- `just snapshots`: 20 snapshots written. Inspected `target/ui-snapshots/generate-bulk-review.txt` â€” boxed Stack and Selected PR panes side by side with the quiet `base | PRs | labels | milestone` footer below the boxes, inside the modal border, no metadata row above. Alignment of list markers, wrapped titles, and form fields is clean.

## Observations (non-blocking, not fixed)
- `bulk_review_footer_line` assigns priorities base=0..blocker=4, and `theme::status_line` drops the *lowest* priority first. So under extreme width pressure the "base"/"PRs" anchor chips are shed before "labels"/"milestone"/"blockers". This is defensible (keeps blocker warnings visible longest) and only triggers at very narrow widths inside an already-wide modal; left as-is.

## Facts vs. inferences
- Fact: all removed helpers are unreferenced and verification/snapshots pass.
- Inference: the footer truncation priority is intentional design rather than oversight; not changed since it is graceful either way.
