---
id: 0001l-2026-06-05-fbbb946a-shared-render-util-scroll-modal-text
created_at: 2026-06-05T22:11:03+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-05T22:34:15+02:00
---
# Extract shared render utilities: natural-scroll, centered-modal, and truncate/wrap

## Goal
Three small rendering primitives are hand-duplicated across the `screens` layer:

1. The AGENTS.md "natural scroll" offset calculation â€” **6 copies**.
2. The centered-modal `backdrop` + `Clear` setup â€” **4 copies**.
3. Char-based truncate/wrap text helpers â€” **2 divergent copies**.

Extract all three into one new `screens::util` module so every screen shares a single, unit-tested implementation. The scroll math is a documented, thrice-broken gotcha (AGENTS.md "Scrolling on overflow"); centralizing it removes the most-duplicated and most-fragile rendering logic in the codebase and gives the upcoming PR/issue management screens a ready foundation instead of a seventh hand-rolled copy. This ticket is a pure refactor: rendered output must stay byte-for-byte identical.

## Context
The active rewrite is Linux-only. Read `docs/rewrite-plan.md` and `AGENTS.md` (the "Scrolling on overflow" gotcha describes the exact natural-scroll formula) before changing anything.

**Natural-scroll duplication.** The pattern `if hl < off { off = hl } else if hl >= off + rows { off = hl - rows + 1 }`, pre-clamped to `len - rows`, appears in:
- `render_menu` â€” `src/screens/generate.rs:674-685` (span-based: a selected row spans multiple wrapped lines; `state.scroll_menu: Cell<u16>`). Note: this site does NOT pre-clamp `cur` to `max_off`.
- `form_scroll` â€” `src/screens/generate.rs:991-1013` (span-based; `state.scroll_form: Cell<u16>`).
- `render_picker_modal` â€” `src/screens/generate.rs:1082-1092` (index-based; `picker.scroll: Cell<usize>`).
- `render_bulk_pr_list` â€” `src/screens/generate.rs:1546-1559` (span-based; `state.bulk_list_scroll: Cell<u16>`).
- `render_bulk_pr_form` â€” `src/screens/generate.rs:1628-1643` (span-based; `state.bulk_form_scroll: Cell<u16>`, computed in a pre-pass).
- `backend_picker::render` â€” `src/screens/backend_picker.rs:110-120` (index-based; `picker.scroll: Cell<usize>`).

The index-based variant is the span variant with `start == end == highlighted` and `visible == rows`: `end + 1 > cur + visible` âŸº `hl >= cur + rows`, and `(end + 1).saturating_sub(visible)` âŸº `hl - rows + 1`. One helper covers all six.

Typing differs: span sites use `u16` scalars (`Cell<u16>`, `inner.height`), index sites use `usize` (`Cell<usize>`, `highlighted`, `list_rows`, `len`). Values are terminal dimensions (well under `u16::MAX`).

**Centered-modal duplication.** Each modal paints `theme::backdrop()` over the area, computes `Rect::new(area.x + area.width.saturating_sub(w)/2, area.y + area.height.saturating_sub(h)/2, w, h)`, and renders `Clear` into it:
- `render_picker_modal` â€” `src/screens/generate.rs:1050-1059`.
- `render_jj_op_dialog` â€” `src/screens/generate.rs:1132-1141`.
- `render_bulk_modal` â€” `src/screens/generate.rs:1183-1193`.
- `backend_picker::render` â€” `src/screens/backend_picker.rs:90-102`.

All four paint the backdrop first, then center+`Clear`. Only the width/height *policy* differs (`clamp` vs `max`), and that stays at the call site.

**Truncate/wrap duplication.**
- `truncate_ellipsis(value, width) -> (String, bool)` â€” `src/screens/generate.rs:2416` (10 callers across the file).
- `wrap_chars(value, width) -> Vec<String>` â€” `src/screens/generate.rs:2477`.
- `backend_picker::truncate(value, width) -> String` â€” `src/screens/backend_picker.rs:211`, which is exactly `truncate_ellipsis(value, width).0`.

This ticket is a prerequisite for the field-renderer relocation ticket (T2): the relocated renderer and line-vocabulary will call `util::wrap_chars` / `util::truncate_ellipsis`.

Render-path coverage lives in `tests/render_smoke.rs`; deterministic snapshots are emitted by `src/bin/ui-snapshots.rs` (`just snapshots`).

## Non-Goals
- Do not change any rendered output. The snapshots and smoke tests must be visually equivalent after this refactor.
- Do not change the list-row *content* builders (`revset_row_lines`, the row construction in `render_bulk_pr_list`, `backend_row`) beyond swapping their trailing scroll-offset calculation for the shared helper.
- Do not move the editable text-field renderer (`render_text_field`, `form_line`, `form_block`, `multiline_value_height`) or the pane line-vocabulary (`kv_line`, `section_header`, `status_line`, `hint_line`, `separator_line`, â€¦) â€” that is ticket T2.
- Do not merge or touch `theme`'s existing `kv`/`hint`/`header`/`footer` primitives; they use a different (non-indented) convention and have separate callers.
- Do not introduce a generic list/scroll widget framework. Extract exactly these three primitives.
- Do not touch the `domain` subprocess helpers â€” that is ticket T3.

## Design Decisions
Create one new module `src/screens/util.rs`, registered in `src/screens/mod.rs`. It may depend on `super::theme` and on `ratatui` (`Frame`, `Rect`, `widgets::Clear`). Expose `pub(crate)` items so both `screens::generate` and `screens::backend_picker` can use them via `super::util::â€¦`.

Helpers:

```rust
/// Natural-scroll offset (see AGENTS.md "Scrolling on overflow"). Keep rows
/// `[start, end]` visible inside a `visible`-row window over `total` rows,
/// moving the prior `cur` offset only when the span crosses an edge. Pre-clamps
/// `cur` to `total - visible`. Index-based callers pass `start == end ==
/// highlighted` and `visible == rows`.
pub(crate) fn natural_scroll(cur: usize, start: usize, end: usize, visible: usize, total: usize) -> usize {
    if visible == 0 {
        return 0;
    }
    let max_off = total.saturating_sub(visible);
    let cur = cur.min(max_off);
    if start < cur {
        start
    } else if end + 1 > cur + visible {
        (end + 1).saturating_sub(visible).min(max_off)
    } else {
        cur
    }
}

/// Centered `width`Ã—`height` sub-rect of `area` (the modal-placement math).
pub(crate) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect;

/// Paint the dim backdrop over `area`, carve a centered `width`Ã—`height` rect,
/// `Clear` it, and return that rect. Callers then draw their modal block into
/// the returned rect. The width/height policy stays at the call site.
pub(crate) fn open_modal(frame: &mut Frame, area: Rect, width: u16, height: u16) -> Rect;

/// Truncate to at most `width` display chars, suffixing "â€¦" if cut.
pub(crate) fn truncate_ellipsis(value: &str, width: usize) -> (String, bool); // moved verbatim

/// `truncate_ellipsis(value, width).0` â€” when only the string is needed.
pub(crate) fn truncate(value: &str, width: usize) -> String;

/// Greedy char-wrap to `width` columns. (moved verbatim)
pub(crate) fn wrap_chars(value: &str, width: usize) -> Vec<String>;
```

`natural_scroll` operates on `usize`; the `u16` span call sites convert with `as usize` on the way in and `as u16` on the way out (values are terminal-sized). The span call sites keep their existing pre-pass that computes `start`/`end`/`total`/`visible`; only the trailing `if/else if/else` block is replaced by the helper call, then stored back into the `Cell`.

`open_modal` replaces the `backdrop` + center + `Clear` triple at all four sites. Each caller keeps its own `width`/`height` computation, passes them in, receives the outer rect, and proceeds with `theme::modal_block(...)` + `block.inner(rect)` exactly as today.

`backend_picker::truncate` is deleted and its call replaced with `util::truncate(...)`.

## Implementation Plan
1. Create `src/screens/util.rs` with `natural_scroll`, `centered_rect`, `open_modal`, `truncate_ellipsis`, `truncate`, `wrap_chars`. Move `truncate_ellipsis` and `wrap_chars` verbatim out of `generate.rs`. Add `mod util;` to `src/screens/mod.rs`.
2. Add unit tests in `util.rs` for `natural_scroll` (highlight above window â†’ scrolls up to `start`; below window â†’ scrolls so `end` is last visible; inside window â†’ unchanged; `visible == 0` â†’ 0; `total <= visible` â†’ 0; pre-clamp when `cur` is stale/too large) and `centered_rect` (centering + odd remainder).
3. Replace the four modal `backdrop`+center+`Clear` blocks (`generate.rs` picker/jj/bulk, `backend_picker.rs`) with `util::open_modal(frame, area, w, h)`; keep each call site's width/height policy unchanged.
4. Replace the six natural-scroll tails with `util::natural_scroll(...)`, converting `u16`â†”`usize` at the four span sites. Keep each pre-pass that computes the focused span.
5. Update all `generate.rs` callers of `truncate_ellipsis`/`wrap_chars` to `util::â€¦`. Delete `backend_picker::truncate` and call `util::truncate`.
6. `just verify`, then `just snapshots`; diff `target/ui-snapshots/*` against the prior render to confirm nothing changed.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "src/screens/generate.rs",
    "src/screens/backend_picker.rs",
    "src/screens/mod.rs"
  ],
  "likely_files": [
    "src/screens/util.rs",
    "src/screens/mod.rs",
    "src/screens/generate.rs",
    "src/screens/backend_picker.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "One `natural_scroll` helper replaces all six hand-rolled scroll tails (render_menu, form_scroll, picker modal, bulk list, bulk form pre-pass, backend picker); each span pre-pass is preserved.",
    "`open_modal` replaces the backdrop+center+Clear triple at all four modal sites; width/height policy stays at the call site.",
    "`truncate_ellipsis`/`wrap_chars` now live in screens::util; backend_picker's private `truncate` is gone.",
    "Rendered output is unchanged: snapshots and render smoke tests are visually equivalent.",
    "natural_scroll has unit tests covering above/below/inside-window and the empty/clamp edge cases."
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria
- A new `screens::util` module exposes `natural_scroll`, `centered_rect`, `open_modal`, `truncate_ellipsis`, `truncate`, `wrap_chars`.
- All six previous natural-scroll implementations call `natural_scroll`; no screen hand-rolls the offset formula anymore.
- All four modal setups call `open_modal`; the backdrop+center+`Clear` triple exists in exactly one place.
- `backend_picker::truncate` is removed; `generate.rs` no longer defines `truncate_ellipsis`/`wrap_chars`.
- `natural_scroll` has unit tests for above/below/inside-window plus `visible == 0`, `total <= visible`, and stale-`cur` pre-clamp.
- `just verify` passes (fmt, check, clippy `-D warnings`, unit + render smoke tests).
- `just snapshots` output is visually equivalent to before the refactor (no intended visual change).

## Verification Plan
Run `just verify` for fmt, compile, clippy (`-D warnings`), unit tests, and render smoke tests. Run `just snapshots` and inspect `target/ui-snapshots/index.html`: confirm the Changes pane, picker modal, jj dialog, backend picker, and bulk review modal all render and scroll exactly as before. Because this is a pure extraction, any snapshot diff is a regression to investigate.

## Files Likely Touched
- `src/screens/util.rs` (new)
- `src/screens/mod.rs`
- `src/screens/generate.rs`
- `src/screens/backend_picker.rs`
- `tests/render_smoke.rs` (only if a focused unit test belongs there rather than in `util.rs`)

## Risks
- **`render_menu` pre-clamp change.** Today `render_menu` uses `cur` without clamping to `max_off`; `natural_scroll` pre-clamps. This is strictly more correct (prevents stale over-scroll after the list shrinks) and only differs in an edge case, but verify the Changes-pane snapshot is unchanged in the common case.
- **`usize`/`u16` conversions** at the four span sites must round-trip the same values; keep the `Cell<u16>` store/load semantics identical.
- **`open_modal` area argument.** Each site currently passes the same area it backdrops over (`area`/`main`); pass that same rect to `open_modal` so the backdrop covers identical cells.
- **Equivalence is the whole contract.** The pre-passes that compute the focused span are not part of the duplication being removed â€” leave them in place; only the final offset formula is shared.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: codex-gpt5
- completed_at: 2026-06-05T22:34:02+02:00
- state: implemented

Completed:
- Added `screens::util` with shared natural scrolling, centered modal setup, and truncate/wrap helpers.
- Replaced all six duplicated natural-scroll tails in Generate and backend picker rendering.
- Replaced the four modal backdrop/center/Clear setup blocks with `util::open_modal`.
- Removed `backend_picker::truncate` and the local `generate.rs` truncate/wrap helpers.

Deviations:
- `form_scroll` now computes total rendered form height before applying the shared helper so the helper can pre-clamp stale offsets consistently.

Verification:
- `just verify` passed.
- `just snapshots` generated 20 snapshots in `target/ui-snapshots`.

Files changed:
- `src/screens/util.rs`
- `src/screens/mod.rs`
- `src/screens/generate.rs`
- `src/screens/backend_picker.rs`

Residual risks:
- Snapshot equivalence was verified by render smoke and regenerated deterministic snapshots; there was no pre-run visual baseline preserved for an automated diff.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: codex-gpt5-self-reviewed
- reviewed_at: 2026-06-05T22:34:15+02:00
- state: reviewed

Reviewed immediately per user instruction.

Findings:
- No functional issue found in the extracted helpers or call-site replacements.
- The natural-scroll helper covers the documented above/below/inside and clamp edge cases with unit tests.
- Modal call sites preserve their width/height policy and only share backdrop, centering, and Clear.
- Text truncate/wrap helpers are centralized and existing render smoke tests still pass.

Verification:
- `just verify` passed before review finalization.
- `just snapshots` generated deterministic UI snapshots.

Residual risk:
- No separate reviewer agent was run; this ticket was treated as reviewed immediately because the user requested it.
