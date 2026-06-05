---
id: 0001q-2026-06-06-b5900201-scrollable-list-window-helper
created_at: 2026-06-06T10:32:45+02:00
created_by_model: gpt-5
state: reviewed
state_updated_at: 2026-06-06T12:10:27+02:00
---
# Add Shared Scrollable List Window Helper

## Goal

Generalize the repeated "visible rows + natural scroll + offset cell + visible range" logic used by rendered lists and fixed-height panes, without extracting a shared row builder yet.

## Context

`screens::util::natural_scroll` already centralizes the edge-crossing formula from AGENTS.md, but every caller still repeats the surrounding work: derive visible row count from `Rect::height`, compute highlighted row spans, call `natural_scroll`, persist the offset, and slice the rendered rows. This pattern appears in the backend picker, Generate changes pane, form pane, picker modal, bulk PR list, and bulk editor pre-pass.

The future PR and issue management modes will add more scrollable lists. They should import a small window/range helper instead of re-deriving the same overflow behavior.

## Non-Goals

- Do not extract a shared list-row builder in this ticket.
- Do not redesign row content, status badges, wrapping, selection markers, or pane layout.
- Do not add PR or issue management screens.
- Do not change the natural-scroll behavior documented in AGENTS.md.

## Design Decisions

Add a small helper in `screens::util` or `screens::widgets` that wraps `natural_scroll` and returns the offset and visible range for a fixed-height list. A concrete shape like this is sufficient:

```rust
pub(crate) struct ScrollWindow {
    pub offset: usize,
    pub visible: usize,
    pub range: std::ops::Range<usize>,
}
```

The helper should accept the prior offset, highlighted start/end row indices, total row count, and available height. Simple one-row lists pass the same highlighted index for start and end; grouped row renderers can pass the first and last row occupied by the selected item. Callers remain responsible for building `Line`s and for storing the returned offset in their existing `Cell<usize>` state.

## Implementation Plan

1. Add a `ScrollWindow` helper and focused unit tests next to `natural_scroll`.
2. Convert existing simple call sites first: backend picker and picker modal.
3. Convert grouped-row call sites in Generate: changes pane, form pane, bulk PR list, and bulk editor pre-pass.
4. Keep row construction and styling unchanged.
5. Verify that render smoke output still fits 80x24 and 120x30 scenarios.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/rewrite-plan.md"],
  "likely_files": [
    "src/screens/util.rs",
    "src/screens/backend_picker.rs",
    "src/screens/generate.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": ["just verify", "just snapshots"],
  "review_focus": [
    "Helper preserves natural-scroll behavior instead of pinning highlights to an edge",
    "Every converted caller persists the returned offset in its existing Cell",
    "Grouped rows keep the whole selected span visible when possible",
    "No shared row-builder abstraction is introduced prematurely",
    "Small-terminal render smoke remains non-panicking"
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria

- A shared scroll-window/range helper exists and is covered by unit tests.
- Existing natural-scroll callers use the helper where it reduces repeated window/range code.
- The helper supports both single-row highlights and multi-row selected spans.
- Visual layout and row content remain unchanged except for any intentional bug fix discovered during conversion.
- `just verify` passes, and `just snapshots` completes for visual inspection.

## Verification Plan

Run `just verify`. Because this touches render paths, also run `just snapshots` and inspect the generated snapshot index or text output for obvious clipping, overlap, or lost highlighted rows.

## Files Likely Touched

- `src/screens/util.rs`
- `src/screens/backend_picker.rs`
- `src/screens/generate.rs`
- `tests/render_smoke.rs` only if additional coverage is needed

## Risks

- A helper that recomputes offset directly from highlighted index would reintroduce edge-pinned scrolling.
- Treating item indices as row indices in grouped lists would let selected multi-line rows scroll partially off-screen.
- Over-generalizing into row rendering would collide with the deferred list-row-builder decision.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5 medium
- reviewed_at: 2026-06-06T12:10:27+02:00
- state: reviewed

Facts:
- The implemented ticket added `screens::util::ScrollWindow` / `scroll_window` around `natural_scroll` and converted the expected backend picker, Generate changes/form/picker modal, bulk PR list, and bulk editor pre-pass call sites without introducing a shared row builder.
- The helper preserves the documented natural-scroll edge-crossing behavior by delegating offset calculation to `natural_scroll` and returning a clamped visible range.
- Review found one render-path bug in `screens::widgets::render_text_field`: when a tall focused multiline field is scrolled so its top is above the viewport, the value box was skipped entirely.
- Review fixed `render_text_field` to render any value box that intersects the viewport, clipping the rect and applying `Paragraph::scroll` for static multiline previews while leaving `ratatui-textarea` to manage the active editor viewport.
- Review added `generate_bulk_review_long_description_stays_visible_when_scrolled`, which asserts long selected descriptions remain visible on an 80x24 `TestBackend` in both preview and editing modes.
- Verification passed with `cargo test generate_bulk_review_long_description_stays_visible_when_scrolled --test render_smoke`, `just verify`, and `just snapshots`.
- Snapshot text artifacts inspected: `backend-picker.txt`, `generate-form-focused.txt`, `generate-picker-modal.txt`, `generate-bulk-review.txt`, and `generate-bulk-small.txt`; no obvious clipping, overlap, or lost focused rows were observed in the reviewed surfaces.

Inferences:
- The implementation matches the ticket's intended level of abstraction: it centralizes scroll window calculation without prematurely extracting row rendering.
- The reviewer fix makes the shared scroll helper safer for future management screens with tall editable text/comment panes.
