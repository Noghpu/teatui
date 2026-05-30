---
id: 00013-2026-05-29-a9575b31-pane-aware-footer-hints
created_at: 2026-05-29T22:38:28+02:00
created_by_model: claude-opus-4-7/high
state: open
---
# Pane-aware footer hints and `p manifest/raw` rename

## Goal
Make the Generate-screen footer hint line reflect the focused pane and the current phase, so every visible shortcut actually fires in the current `(focus, phase)`. Rename the misleading `p prompt` hint to `p manifest/raw`.

## Context
Today the Generate-screen default footer arm (`src/ui.rs` around line 1417) renders a single fixed line: `h/l Enter i g c p r Esc`. That line is correct in no single pane: `i` only works in Form, `p` only does anything once a prompt exists, `r` only refreshes from Menu, etc. The user reported the labels confuse the flow. This ticket aligns the hints with the pane-local dispatch landed in `pane-local-keymap`.

## Non-Goals
- No behavioral changes to key handling itself; that is owned by `pane-local-keymap`.
- No spinner / async status â€” that lives in `phase-aware-footer-spinner`.
- No inline blocker banner â€” that lives in `generate-press-feedback`.
- No changes to other screens (Landing, PullRequests, Issues) footers.

## Design Decisions
- Replace the single Generate-default arm with a match on `(focus, phase, draft_present)`. Use a small helper, e.g. `generate_footer_hints(focus, phase, draft: Option<&GeneratedDraft>) -> Line<'static>`.
- Hint sets:
  - `Focus::Menu`: `â†‘/â†“ select`, `Enter pick revset`, `r refresh`, `Tab â†’ Form`, `Esc back`.
  - `Focus::Form`: `â†‘/â†“ field`, `Enter/i edit`, `g generate`, `Tab â†’ Preview`, `Shift+Tab â†’ Menu`, `Esc back`.
  - `Focus::Preview` with no draft: `â†‘/â†“ scroll`, `Tab â†’ Menu`, `Esc back`.
  - `Focus::Preview` with draft: `â†‘/â†“ scroll`, `p manifest/raw`, `g regenerate`, `c confirm`, `Tab â†’ Menu`, `Esc back`.
- Rename the footer hint string `prompt` â†’ `manifest/raw` everywhere it appears in `src/ui.rs`.
- Keep existing dedicated footer arms for `InputMode::Editing`, `InputMode::Confirm`, `Failed`, `Complete` untouched in this ticket.
- ASCII arrows (`â†‘`/`â†“`/`â†’`) match the rest of the UI hint style; keep them.

## Implementation Plan
1. In `src/ui.rs`, locate the Generate-default footer arm (current `Screen::Generate => Line::from(...)` around line 1417).
2. Replace it with a match on `(app.focus(), app.generate().phase, app.generate().draft.is_some())`.
3. Extract the body into `fn generate_footer_hints(focus: Focus, phase: GeneratePhase, has_draft: bool) -> Line<'static>` to keep the match arms readable.
4. Rename every occurrence of the `prompt` hint label tied to `p` in the Generate-screen footers to `manifest/raw`. Confirm there is exactly one place (the Preview arm) and not elsewhere.
5. Add a focused unit test in `src/ui.rs` (or wherever existing ui helpers are tested) for `generate_footer_hints`, asserting the rendered string contains:
   - `r refresh` only when `focus == Menu`.
   - `g generate` only when `focus == Form`.
   - `p manifest/raw`, `c confirm`, `g regenerate` only when `focus == Preview && has_draft`.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/ui.rs"],
  "likely_files": ["src/ui.rs"],
  "verification_commands": ["just fmt", "just check", "just clippy", "just test", "just verify"],
  "review_focus": [
    "Footer hint per pane matches the pane-local dispatch contract.",
    "`p prompt` no longer appears anywhere; replaced by `p manifest/raw`.",
    "Hints only show keys that actually fire in the current (focus, phase, draft) tuple.",
    "Existing Editing/Confirm/Failed/Complete arms unchanged."
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- The footer line on the Generate screen changes when focus moves between Menu, Form, and Preview.
- Preview pane hides `p manifest/raw`, `g regenerate`, and `c confirm` until a draft is present.
- The literal string `p prompt` does not appear in the footer; `p manifest/raw` does (Preview + draft only).
- `Tab â†’ Form` / `Tab â†’ Preview` / `Tab â†’ Menu` directions are pane-correct.
- Existing footer arms for editing / confirming / executing / failed / complete continue to render unchanged.

## Verification Plan
- `just verify`.
- Manual smoke: navigate panes on the Generate screen with and without a generated draft and confirm hints change.

## Files Likely Touched
- `src/ui.rs`

## Risks
- The `Focus` enum may need to be in scope where it is not today â€” confirm imports.
- Whitespace and color styling in the existing hint line is delicate; preserve `colors::ACCENT` / `colors::MUTED` pattern when refactoring.
- Make sure the new helper does not allocate a fresh `Line<'static>` per frame in a way that surprises clippy lints (current code uses `Line::from(vec![...])` per frame, so equivalent allocation is fine).
