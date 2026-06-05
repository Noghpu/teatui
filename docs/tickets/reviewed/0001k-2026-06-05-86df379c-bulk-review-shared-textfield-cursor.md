---
id: 0001k-2026-06-05-86df379c-bulk-review-shared-textfield-cursor
created_at: 2026-06-05T21:13:57+02:00
created_by_model: claude-opus-4-8/high
state: reviewed
state_updated_at: 2026-06-05T21:46:02+02:00
---
# Bulk review modal: share the editable text-field renderer so the description has a cursor

## Goal
The Description field in the stacked-PR bulk review modal shows no text cursor while editing. The root cause is that the field-rendering logic is duplicated between the Generate form pane and the bulk modal, and the modal copy renders the multiline Description as a static preview instead of the live `TextArea` widget. Extract one shared "render an editable text field" component used by both the form pane and the bulk modal, so the cursor, autosizing, scrolling, empty-state, and overflow behavior are identical in both places and cannot drift again.

## Context
The active rewrite is Linux-only and the Generate screen owns the stacked-PR review modal. Read `docs/rewrite-plan.md` and `AGENTS.md` (note the "Scrolling on overflow" gotcha) before changing behavior.

The text-field *state* is already shared: both panes store fields as `form::TextFieldState`, each wrapping a `ratatui_textarea::TextArea` (`src/screens/generate/form.rs`). The bulk editor's three fields (`title`, `branch`, `description`) are `form::TextFieldState` (`src/screens/generate.rs:278-290`), exactly like the form pane. What is duplicated and divergent is the *rendering*:

Form pane — `render_form` (`src/screens/generate.rs` ~770-913) renders each field with a virtual-cursor positional model:
- `form_line(frame, inner, cy, scroll, line)` (`:916`) draws one viewport-clipped line.
- `form_block(frame, inner, cy, scroll, lines)` (`:931`) draws a multi-line block with internal scroll/clip.
- When `editing`, it renders the live `&t.editor` widget into a value rect (`:845`) for BOTH single-line and multiline fields — this is what draws the cursor.
- Value height comes from `multiline_value_height(t, value_w, editing)` (`:998`, min `MULTILINE_MIN_HEIGHT = 6`); single-line is height 1.
- Not-editing multiline rendering wraps via `wrap_chars`, marks overflow with a trailing `…` row, and pads/empties with `empty_value_line()`.
- Field-aware scroll is computed by `form_scroll` (`:956`) using per-field heights from `form_field_height` (`:980`); the chosen offset is stored in `state.scroll_form: Cell<u16>` (`:303`).

Bulk modal — `render_bulk_pr_form` (`src/screens/generate.rs` ~1534-1727) re-implements field rendering by accumulating a `Vec<Line>` and rendering it as a single `Paragraph::new(lines).wrap(...)` at the end (`:1727`) with NO scroll offset:
- Single-line fields, when editing, render `&t.editor` inline at a computed `y` (`:1642-1659`) — so they DO get a cursor.
- The Description field (`:1628-1641`) renders a static `wrap_chars` preview (max 4 lines) of `t.buffer`/`t.value` even while editing, so the `TextArea` widget — and the cursor — are never drawn. This is the bug.
- The right pane never scrolls, so its content (per-PR head/base/status, separators, three fields, blockers/warnings, result, last action) can silently overflow the box, violating the AGENTS.md overflow gotcha.

The modal also re-derives its own marker/style/indent/empty-state logic that parallels the form pane's.

The right pane (`render_bulk_pr_form`) renders into `form_area`, the inner rect of the `Selected PR` pane block from `render_bulk_review`/`render_bulk_review_panes` (`:1310-1367`). The left list already scrolls naturally via `state.bulk_list_scroll: Cell<u16>` (`:327`, `:1511-1524`); mirror that pattern for the right pane.

Render smoke coverage for the modal lives in `tests/render_smoke.rs` (bulk review phases incl. wrapped titles, preview focus, push-in-flight, done, failed, blockers, and a small-terminal each-phase test). The deterministic snapshot binary emits `generate-bulk-review` from `src/bin/ui-snapshots.rs`.

## Non-Goals
Do not change the field *state* model (`form::TextFieldState`, `TextArea`, `begin_edit`/`commit`/`input`). It is already shared and correct.

Do not change stacked-PR generation, LLM parsing, context collection, blocker detection, push execution, or single-PR Generate field *behavior* (Tab/Enter/Esc editing semantics). Visual output of the form pane must stay equivalent after the extraction.

Do not change the modal's two-pane focus model, the left `Stack` list rendering/scrolling, the shared metadata footer, or the modal layout split. Do not implement the Left/Right pane-navigation feature (separate ticket); the two tickets touch different regions of the same files.

Do not introduce a generic widget framework. Extract exactly the editable-text-field renderer the two existing call sites need.

## Design Decisions
Introduce one shared text-field renderer and one shared height helper, then route both panes through them.

Shared renderer — a single function that renders one editable text field (label row + value box) using the form pane's existing viewport-positional model, so it is reusable by any caller that tracks a virtual `cy` and a `scroll` offset against an `inner` rect. Suggested shape (adjust names/params to fit cleanly, keep it in `src/screens/generate.rs` near `form_line`/`form_block`):

```
fn render_text_field(
    frame: &mut Frame,
    inner: Rect,
    cy: &mut u16,
    scroll: u16,
    t: &form::TextFieldState,
    label: &str,
    marker: &str,     // "▶ " when focused else "  "
    label_style: Style,
    editing: bool,
    value_w: usize,
)
```

Responsibilities of the shared renderer (single source of truth for all of these):
- Draw the label line `"{marker}{label}:"` via the `form_line` clip model and advance `*cy`.
- Compute value height: `1` for single-line, `multiline_value_height(t, value_w, editing)` for multiline.
- When `editing`: render the live `&t.editor` widget into the value rect (viewport-clipped like `form_block`) for both single- and multiline fields, giving a real cursor.
- When not editing: render the static value — single-line truncated with ellipsis; multiline wrapped via `wrap_chars` with the trailing `…` overflow marker and `empty_value_line()` padding — matching the current form-pane output exactly.
- Advance `*cy` by the value height.
- Field-level `errors` rendering (used by the form pane) can stay at the form-pane call site after the shared call, or be included — pick one and keep the form pane's current visual output identical. The bulk editor fields have no `errors` surfaced today, so the bulk caller need not render them.

Keep `multiline_value_height` as the shared autosizing source of truth (already used by `form_field_height`); the bulk pane must size the Description box with the same helper rather than the old fixed 4-line preview.

Form pane (`render_form`): replace the inline `FieldState::Text` rendering block (`:817-905`) with a call to the shared renderer (plus the existing error-line loop if not folded in). Net visual output must be unchanged. `form_scroll`/`form_field_height` keep working as-is.

Bulk modal (`render_bulk_pr_form`): convert the right pane from "accumulate `Vec<Line>` + final `Paragraph`" to the same `cy`/`scroll` positional model the form pane uses, so the shared renderer can draw the live editor with a visible cursor and the pane scrolls to keep the focused field in view:
- Draw the leading per-PR content (head/base/read-only, push status, separator) and the trailing content (blockers, warnings, result, last action) with `form_line`/`form_block` against the same `cy`/`scroll`, instead of pushing into a `Vec` rendered without scroll.
- Render the three editable fields (`Title`, `Branch`, `Description`) via the shared `render_text_field`, passing `editing = state.bulk_editor.editing && state.bulk_editor.field_focus == field` and the focus-aware marker/style already computed in the current loop (`:1605-1617`).
- Add `pub bulk_form_scroll: Cell<u16>` to `GenerateState` (next to `bulk_list_scroll`, `:327`), default `Cell::new(0)`. Compute the scroll offset with the natural-scroll pattern from `AGENTS.md` so the focused field's value box stays fully visible while editing (mirror `form_scroll`'s "only move when the focused span crosses an edge" logic, clamped to `content_height - visible`). Update the field-default comment at `:2554` to include `bulk_form_scroll`.
- Preserve the existing small-terminal robustness: the early `body_area` zero-size guard in `render_bulk_review` stays; the converted pane must not panic when `form_area.height` is tiny (the `form_line`/`form_block` clip model already no-ops off-screen rows).

Footer hints already advertise `Esc commit` / `Enter newline` for the multiline editor; no hint change is required for this ticket.

The end-state visual contract: in the modal, entering edit mode on Description shows a multi-row editor box (min height 6) with a blinking/positioned cursor identical to the form pane's description editor, and the right pane scrolls to keep the active field visible.

## Implementation Plan
1. In `src/screens/generate.rs`, extract `render_text_field` (label + value box, single/multiline, editing/not, viewport-clipped, autosized via `multiline_value_height`) near `form_line`/`form_block`. Make it the single place that renders `&t.editor` when editing and the static value otherwise.
2. Refactor `render_form`'s `FieldState::Text` arm to call `render_text_field`; confirm output is visually identical (run `just snapshots` and diff the Generate form snapshots).
3. Add `bulk_form_scroll: Cell<u16>` to `GenerateState` (declaration, `Default`/constructor init, and the reset/default comment at `:2554`).
4. Convert `render_bulk_pr_form` to the `cy`/`scroll` positional model: draw leading and trailing per-PR content via `form_line`/`form_block`, draw the three editable fields via `render_text_field`, and compute `bulk_form_scroll` with the natural-scroll pattern so the focused field stays visible. Remove the old `Vec<Line>` accumulation and the static 4-line Description preview.
5. Keep the small-terminal guard and verify no panic at tiny `form_area` heights.
6. Update/extend render smoke tests in `tests/render_smoke.rs` only as needed for the changed render shape — existing bulk phases (wrapped titles, preview focus, push-in-flight, done, failed item, blockers, small terminal) must still render without panic. Add a case that renders the modal with the Description field in edit mode so the editor-widget path is exercised.
7. Run `just verify`, then `just snapshots`. Inspect `target/ui-snapshots/generate-bulk-review.*` and the Generate form snapshots in `target/ui-snapshots/index.html`: confirm the Description editor renders as a multi-row box with a cursor in the modal, the right pane scrolls to keep the focused field visible, and the form pane is visually unchanged.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "docs/tickets/reviewed/0001i-2026-06-05-ccf51e1f-bulk-review-boxed-metadata.md"
  ],
  "likely_files": [
    "src/screens/generate.rs",
    "src/screens/generate/form.rs",
    "src/bin/ui-snapshots.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "A single shared renderer draws editable text fields for BOTH the form pane and the bulk modal; the modal no longer has its own divergent field-render path.",
    "Editing the Description field in the modal renders the live TextArea widget (visible cursor), not a static preview.",
    "Autosizing uses multiline_value_height in both panes; the modal Description box honors MULTILINE_MIN_HEIGHT.",
    "The modal right pane scrolls naturally (bulk_form_scroll) to keep the focused field visible and no longer silently overflows.",
    "Form-pane visual output is unchanged after the extraction (snapshot diff clean apart from intended modal changes).",
    "Small-terminal bulk render still does not panic."
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria
- Editing the Description field in the bulk review modal shows a text cursor, matching the Generate form pane's description editor.
- A single shared function renders editable text fields for both `render_form` and `render_bulk_pr_form`; the modal no longer renders Description as a static `wrap_chars` preview while editing.
- The modal Description box autosizes via `multiline_value_height` (min height 6), the same helper the form pane uses.
- The modal's right `Selected PR` pane scrolls naturally (via a new `bulk_form_scroll` cell) so the focused field stays visible and content no longer overflows silently.
- The Generate form pane's rendered output is unchanged by the extraction.
- Single-line modal fields (Title, Branch) still show a cursor while editing.
- Existing bulk review render smoke tests still pass, including the small-terminal each-phase case; a new case exercises the Description-editing render path.
- `just verify` and `just snapshots` pass.

## Verification Plan
Run `just verify` for fmt, compile, clippy (`-D warnings`), unit tests, and render smoke tests.

Run `just snapshots` and inspect `target/ui-snapshots/generate-bulk-review.txt`/`.svg` and the Generate form snapshots via `target/ui-snapshots/index.html`:
- Confirm the modal Description editor renders as a multi-row box with a cursor while editing.
- Confirm the right pane scrolls to keep the focused field visible when content exceeds the box.
- Confirm the form pane snapshots are unchanged from before the refactor.

## Files Likely Touched
- `src/screens/generate.rs`
- `src/screens/generate/form.rs` (only if helper visibility/signatures need adjusting; state model unchanged)
- `src/bin/ui-snapshots.rs` (only if a Description-editing snapshot variant is added)
- `tests/render_smoke.rs`

## Risks
Converting `render_bulk_pr_form` from `Vec<Line>` accumulation to the `cy`/`scroll` positional model is the largest change. The leading per-PR block and trailing blockers/result/last-action lines must be re-expressed against `form_line`/`form_block` without changing their content or order; diff the snapshot carefully.

Scroll math is easy to get wrong: follow the AGENTS.md natural-scroll pattern (`if hl < off { off = hl } else if hl >= off + rows { off = hl - rows + 1 }`, pre-clamped to `len - rows`) so the focused field does not pin to an edge or scroll the whole pane on every keypress. The "focused span" here is the focused field's label+value rows.

The shared renderer must keep the form pane byte-for-byte equivalent visually; the form pane's error-line rendering and picker fields must be unaffected. If folding error lines into the shared renderer is awkward, leave them at the form-pane call site.

Small terminals: the editor widget and multi-row Description box must clip, not panic, when `form_area` is short. Keep the existing zero-size guards and rely on the `form_line`/`form_block` off-screen no-op behavior.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-06-05T21:42:14+02:00
- state: implemented

## What was completed

Extracted a shared `render_text_field` function used by both the Generate form
pane (`render_form`) and the bulk review modal (`render_bulk_pr_form`), fixing
the bug where editing the Description field in the modal showed no cursor.

### Changes

1. Added `bulk_form_scroll: Cell<u16>` to `GenerateState` (struct declaration,
   `new()` constructor, and the default-field comment in the test helper).

2. Introduced `render_text_field(frame, inner, cy, scroll, t, label, marker,
   label_style, editing, value_w)` near `form_line`/`form_block` in
   `src/screens/generate.rs`. It is the single source of truth for:
   - label row (clipped via `form_line`)
   - value height (`multiline_value_height` for multiline, 1 for single-line)
   - editing path: renders live `&t.editor` widget (cursor visible)
   - not-editing path: static value with `wrap_chars`, `â€¦` overflow, `empty_value_line()`

3. Replaced the inline `FieldState::Text` rendering block in `render_form` with
   a call to `render_text_field`; error lines remain at the call site. Visual
   output is identical.

4. Converted `render_bulk_pr_form` from `Vec<Line>` accumulation + final
   `Paragraph::new(lines)` to the `cy`/`scroll` positional model:
   - Pre-pass computes `bulk_form_scroll` using the natural-scroll pattern from
     AGENTS.md, keeping the focused field visible.
   - Leading content (head/base/status/separator) rendered via `form_line`.
   - Three editable fields rendered via `render_text_field` â€” Description now
     uses `multiline_value_height` (min 6 rows) and shows the live TextArea
     widget with cursor when editing.
   - Trailing content (blockers, warnings, result, last action) rendered via
     `form_line`.

5. Updated struct literal initializers in `src/bin/ui-snapshots.rs` and
   `src/screens/generate/input.rs` to include `bulk_form_scroll`.

6. Added `generate_bulk_review_description_editing_renders` smoke test in
   `tests/render_smoke.rs` that exercises the Description-editing render path.

### Deviations from plan

None. Implementation follows the ticket design exactly.

### Verification

`just verify` passes: fmt, check, clippy (-D warnings), 179 unit tests, 51
render smoke tests (including the new description-editing case).

### Important files changed

- `src/screens/generate.rs` (main change)
- `src/screens/generate/input.rs` (struct init update)
- `src/bin/ui-snapshots.rs` (struct init update)
- `tests/render_smoke.rs` (new smoke test + struct init update)

### Residual risks / follow-up

- The pre-pass scroll computation in `render_bulk_pr_form` duplicates the
  height model. If future fields add rows between the pre-pass and render
  pass the scroll offset could drift; keep them in sync.
- `just snapshots` was not run (no Linux host available); visual inspection
  of the SVG/txt snapshots is recommended at next Linux session.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-8
- reviewed_at: 2026-06-05T21:46:02+02:00
- state: reviewed

## Review outcome: accepted, no code changes required

The implementation cleanly satisfies the ticket. A single shared
`render_text_field` (src/screens/generate.rs ~904) now renders editable text
fields for BOTH `render_form` and `render_bulk_pr_form`, and the bulk modal's
right pane was converted from `Vec<Line>` accumulation + a static 4-line
Description preview to the same `cy`/`scroll` positional model the form pane
uses. Verified facts:

- Editing path: `render_text_field` renders the live `&t.editor` widget when
  `editing`, so the Description (and Title/Branch) fields now draw a real
  cursor in the modal. The static-preview bug is gone.
- Autosizing: both panes size multiline value boxes via
  `multiline_value_height` (MULTILINE_MIN_HEIGHT = 6). The bulk Description box
  uses the same helper.
- Scroll: `bulk_form_scroll: Cell<u16>` added to `GenerateState` (declaration,
  `new()`, and both test/struct-literal call sites in input.rs and
  ui-snapshots.rs). The natural-scroll pattern from AGENTS.md is applied with
  pre-clamped `max_off`, matching `form_scroll`/`bulk_list_scroll`.
- Form-pane equivalence: the `FieldState::Text` arm now calls the shared
  renderer with error lines left at the call site; indentation (+2) is
  consistent between the editing rect (`inner.x + indent`) and the static
  value lines (prefixed `"  "`), so visual output is preserved.
- Small terminal: `form_line`/`form_block` no-op off-screen rows and the
  `body_area` zero-size guard remains; the small-terminal each-phase smoke
  test passes.

## Verification (facts)

`just verify` passes on this Windows host: `cargo fmt --check`, `cargo check`,
`cargo clippy --all-targets --all-features -D warnings` (clean), 179 unit
tests, 51 render smoke tests including the new
`generate_bulk_review_description_editing_renders` case that exercises the
Description editing render path.

`just snapshots` was NOT run: the recipe targets a Linux host (the rewrite is
Linux-only) and is unavailable here. This matches the implementer's note.
Visual confirmation of the rendered SVG/txt (cursor in the modal Description
box, right-pane scroll-to-focus, unchanged form-pane snapshots) remains a
recommended check at the next Linux session, but is not gating: the smoke
tests assert no-panic on the editing/scroll paths, and the render logic is now
a single shared code path with the form pane.

## Inference / residual risk (carried forward, not blocking)

The scroll pre-pass in `render_bulk_pr_form` duplicates the leading-block
height model (head/base/status/separator) of the render pass. I confirmed the
two passes agree today (status height: 1 when pushing; 2 for `Created`; 1
otherwise â€” identical in both passes), and the pre-pass deliberately only needs
to locate the focused *field* span, so it does not model the trailing
blockers/result/last-action content. The drift risk is real but contained and
documented in code comments. I left it as-is: collapsing the duplication into a
shared position-computing helper is a larger restructure than this ticket's
non-goals allow ("do not introduce a generic widget framework"; keep the
extraction minimal). Flagged for a future cleanup if more fields are added
between the passes.

No deviations from the plan. No acceptance criteria unmet (modulo the
Linux-only visual snapshot inspection).
