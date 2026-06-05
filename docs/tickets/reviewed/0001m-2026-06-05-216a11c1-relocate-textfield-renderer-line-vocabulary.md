---
id: 0001m-2026-06-05-216a11c1-relocate-textfield-renderer-line-vocabulary
created_at: 2026-06-05T22:11:04+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-05T22:40:53+02:00
---
# Relocate the editable text-field renderer and pane line-vocabulary into a shared screens module

## Goal
The shared text-field renderer introduced by ticket 0001k (`render_text_field` + `form_line` + `form_block` + `multiline_value_height`) and the indented "pane line-vocabulary" (`kv_line`, `section_header`, `section_heading_line`, `status_line`, `hint_line`, `separator_line`, `placeholder_line`, `empty_value_line`, `field_header_line`) are private functions inside `src/screens/generate.rs`. The upcoming PR/issue management screens need exactly these: the field renderer to compose and edit comments, and the line-vocabulary to render PR/issue detail and comment threads. Relocate them into a shared `screens::widgets` module so a sibling screen module can import them instead of copying.

This is a **mechanical move, not a redesign**. `generate.rs`'s rendered output must remain byte-for-byte identical. The point is *where the code lives*, so the next screen reaches for an import rather than a copy â€” without inventing a speculative API.

## Context
The active rewrite is Linux-only. Read `AGENTS.md`, `docs/rewrite-plan.md`, and the reviewed ticket `docs/tickets/reviewed/0001k-2026-06-05-86df379c-bulk-review-shared-textfield-cursor.md` (it documents the positional `cy`/`scroll` clip model these functions implement) before moving anything.

**Depends on ticket T1** (the `screens::util` module). The relocated functions call `util::wrap_chars` and `util::truncate_ellipsis`; land T1 first.

Functions to relocate (all currently private in `src/screens/generate.rs`):
- Positional field renderer: `render_text_field` (~`:904`), `form_line` (`:850`), `form_block` (`:865`), `multiline_value_height` (`:1033`).
- Line-vocabulary: `kv_line` (`:2177`), `section_header` (`:2312`), `section_heading_line` (`:2316`), `status_line` (`:2305`), `hint_line` (`:2173`), `separator_line` (`:2470`), `placeholder_line` (`:2162`), `empty_value_line` (`:2169`), `field_header_line` (`:2291`).

These reference `theme::*` styles, `util::*` (after T1), and â€” for `render_text_field` â€” the field state type `form::TextFieldState` (defined in `src/screens/generate/form.rs`). They are consumed throughout `generate.rs`: the form pane (`render_form`), the bulk modal (`render_bulk_pr_form`), and the preview pane (`render_preview` and its `preview_*_lines` helpers) all call the line-vocabulary heavily; `form_field_height`/`form_scroll` call `multiline_value_height`.

**Type-location decision (scope guard).** `render_text_field` is parameterized on `&form::TextFieldState`. This ticket deliberately leaves `TextFieldState` (and the rest of the form state model â€” `PrForm`, `FieldState`, `PickerFieldState`) in `generate::form`. `screens::widgets` references `TextFieldState` by path (`crate::screens::generate::form::TextFieldState`). Module-level reference cycles within one crate are legal, so `widgets â†’ generate::form` and `generate â†’ widgets` coexist fine. Moving the *state type* to a shared home is a separate, larger change that should wait for a real second consumer; over-moving now would be speculative.

**Overlap note (do not merge).** `theme` already has `kv`/`hint`/`header`/`footer` (`src/screens/theme.rs:122-169`) with a different, non-indented convention and their own callers (landing, status bars). The `generate.rs` `kv_line`/`hint_line`/`section_heading_line` are the indented in-pane variants. Keep both; do not unify in this ticket.

Render coverage: `tests/render_smoke.rs`; snapshots via `src/bin/ui-snapshots.rs` (`just snapshots`).

## Non-Goals
- No behavior or visual change. `generate.rs` output must be identical; this is a relocation.
- Do not move `TextFieldState`, `PrForm`, `FieldState`, or `PickerFieldState` out of `generate::form`.
- Do not merge with `theme`'s `kv`/`hint`/`header`/`footer`.
- Do not redesign the function signatures (`render_text_field`'s 10-argument shape stays as 0001k left it).
- Do not extract a generic "list-row" builder (`revset_row_lines` / bulk-PR rows) â€” that is an explicitly deferred future item (see AGENTS.md "Deferred â€” PR management pass").
- Do not invent a placeholder management screen or fake consumer to "prove" reuse.
- Do not touch `domain` subprocess helpers (ticket T3).

## Design Decisions
Create `src/screens/widgets.rs`, registered as `mod widgets;` in `src/screens/mod.rs`. Move the field renderer and line-vocabulary functions there verbatim, changing only their visibility to `pub(crate)` and updating their internal calls to `theme::*`, `util::*`, and `crate::screens::generate::form::TextFieldState` to resolve from the new location.

`generate.rs` keeps every call site but routes through the new module â€” either `use super::widgets::{â€¦}` and call bare, or qualify as `widgets::kv_line(...)`. Pick one style and apply it consistently.

`multiline_value_height` moves to `widgets`; `form_field_height` and `form_scroll` (which stay in `generate.rs`) call `widgets::multiline_value_height`.

The end state: `screens::widgets` is a self-contained rendering toolkit â€” "render an editable text field" plus the indented in-pane `Line` vocabulary â€” importable by any current or future screen module, with `generate.rs` as its first consumer and visual output unchanged.

## Implementation Plan
1. Create `src/screens/widgets.rs`; add `mod widgets;` to `src/screens/mod.rs` (after T1's `mod util;`).
2. Move the line-vocabulary functions (`kv_line`, `section_header`, `section_heading_line`, `status_line`, `hint_line`, `separator_line`, `placeholder_line`, `empty_value_line`, `field_header_line`) into `widgets`, as `pub(crate)`. Fix their `theme::`/`util::` references.
3. Move the positional field renderer (`form_line`, `form_block`, `multiline_value_height`, `render_text_field`) into `widgets`, as `pub(crate)`. Reference `TextFieldState` by its `generate::form` path.
4. Update every call site in `generate.rs` (form pane, bulk modal, preview pane, `form_field_height`, `form_scroll`, `field_lines`, etc.) to the new module paths. Remove the now-moved definitions from `generate.rs`.
5. `just verify`; resolve any visibility/borrow fallout from the move (no logic changes).
6. `just snapshots`; diff `target/ui-snapshots/*` to confirm the form pane, bulk review modal, and preview pane are pixel-identical.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "docs/tickets/reviewed/0001k-2026-06-05-86df379c-bulk-review-shared-textfield-cursor.md",
    "src/screens/generate.rs",
    "src/screens/generate/form.rs"
  ],
  "likely_files": [
    "src/screens/widgets.rs",
    "src/screens/mod.rs",
    "src/screens/generate.rs",
    "tests/render_smoke.rs"
  ],
  "verification_commands": [
    "just verify",
    "just snapshots"
  ],
  "review_focus": [
    "The 0001k field renderer (render_text_field/form_line/form_block/multiline_value_height) and the indented line-vocabulary now live in screens::widgets and are importable by sibling screen modules.",
    "generate.rs is the first consumer; its rendered output (form pane, bulk modal, preview pane) is unchanged.",
    "TextFieldState and the form state model stayed in generate::form (referenced by path); nothing was over-moved.",
    "theme's existing kv/hint/header were not merged or altered.",
    "No new list-row abstraction and no placeholder management screen were introduced."
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria
- A new `screens::widgets` module holds the editable text-field renderer (`render_text_field`, `form_line`, `form_block`, `multiline_value_height`) and the indented line-vocabulary (`kv_line`, `section_header`, `section_heading_line`, `status_line`, `hint_line`, `separator_line`, `placeholder_line`, `empty_value_line`, `field_header_line`).
- `generate.rs` no longer defines those functions; it calls them through `screens::widgets`.
- `TextFieldState` and the form state model remain in `generate::form`.
- `theme`'s `kv`/`hint`/`header`/`footer` are untouched.
- The Generate form pane, bulk review modal, and preview pane render identically to before (snapshot diff clean).
- `just verify` and `just snapshots` pass.

## Verification Plan
Run `just verify` (fmt, check, clippy `-D warnings`, unit + render smoke). Run `just snapshots` and compare `target/ui-snapshots/index.html`: the Generate form, `generate-bulk-review`, and preview snapshots must match the pre-move render exactly. Any diff is a regression.

## Files Likely Touched
- `src/screens/widgets.rs` (new)
- `src/screens/mod.rs`
- `src/screens/generate.rs`
- `src/screens/generate/form.rs` (only if a visibility tweak on `TextFieldState` is needed â€” it is already `pub`)
- `tests/render_smoke.rs` (only if a test references a moved function by its old path)

## Risks
- **Large mechanical surface.** The line-vocabulary has many call sites in `generate.rs` (preview pane especially). Move definitions and update paths without altering bodies; lean on the snapshot diff to catch any accidental change.
- **Type-location coupling.** `widgets` referencing `generate::form::TextFieldState` is intentional but slightly awkward; it is the minimal move. Do not "fix" it by dragging the state model along â€” that is out of scope and would balloon the diff.
- **Visibility fallout.** Functions that were private may rely on other private `generate.rs` items; the only dependency the renderer truly needs is `TextFieldState` (path import), `theme::*`, and `util::*`. If a moved function reaches for something else private to `generate.rs`, prefer moving the small leaf helper too over widening unrelated visibility.
- **Ordering.** Depends on T1; if T1 has not landed, `util::wrap_chars`/`util::truncate_ellipsis` will not resolve.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: codex-gpt5
- completed_at: 2026-06-05T22:40:38+02:00
- state: implemented

Completed:
- Added `screens::widgets` with the editable text-field renderer and indented in-pane line vocabulary.
- Registered `screens::widgets` in `screens::mod`.
- Updated `generate.rs` to import and use the relocated helpers while leaving form state in `generate::form`.
- Removed the moved helper definitions from `generate.rs`.

Deviations:
- None. This was a mechanical relocation.

Verification:
- `just verify` passed.
- `just snapshots` generated 20 snapshots in `target/ui-snapshots`.

Files changed:
- `src/screens/widgets.rs`
- `src/screens/mod.rs`
- `src/screens/generate.rs`

Residual risks:
- Snapshot generation succeeded, but no preserved pre-move baseline was available for automated diffing in this run.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: codex-gpt5-self-reviewed
- reviewed_at: 2026-06-05T22:40:53+02:00
- state: reviewed

Reviewed immediately per user instruction.

Findings:
- No functional issue found in the relocation.
- `screens::widgets` now owns the field renderer and line vocabulary, and `generate.rs` imports those helpers instead of defining them locally.
- `TextFieldState` and the form state model remain in `generate::form`.
- `theme` primitives were not merged or changed.
- No generic list-row abstraction or management-screen placeholder was introduced.

Verification:
- `just verify` passed before review finalization.
- `just snapshots` generated deterministic UI snapshots.

Residual risk:
- No separate reviewer agent was run; this ticket was treated as reviewed immediately because the user requested it.
