---
id: 0000i-2026-05-27-41c04fd3-catppuccin-mocha-colors-border-polish
created_at: 2026-05-27T15:27:55+02:00
created_by_model: claude-sonnet-4-6
state: reviewed
state_updated_at: 2026-05-27T15:50:01+02:00
---
# Catppuccin Mocha Color Module and Border Polish

## Goal
Create a `src/colors.rs` module with the full Catppuccin Mocha palette as `Color::Rgb` constants, add semantic aliases used throughout the UI, and apply them to `src/ui.rs` while also switching to rounded borders, focus-colored full borders, `â–¶` selection markers, and inner padding on all panes.

## Context
The current `ui.rs` uses ratatui's convenience methods (`.cyan()`, `.dim()`, `.green()`, `.red()`, `.bold()`) directly, with plain square `Borders::ALL` blocks and a `>` marker for selection. The result looks flat and generic. This ticket lays the visual foundation the subsequent landing-hero and form-polish tickets build on.

The Catppuccin Mocha palette (dark variant) is being adopted as the app's color scheme. ratatui 0.30 supports `Color::Rgb(r, g, b)` for true color output.

## Non-Goals
- No new layouts or structural changes to any screen
- No Catppuccin flavor switching (Mocha only)
- No changes to `app.rs`, event handling, or business logic

## Design Decisions
- **Mocha only, hardcoded**: No `Theme` enum, no config switch. One flavor, one file.
- **Palette constants in `src/colors.rs`**: Raw palette entries (`BASE`, `MANTLE`, `SURFACE0`, `TEXT`, `BLUE`, `LAVENDER`, `GREEN`, `RED`, `PEACH`, `OVERLAY0`, `OVERLAY1`, etc.) plus semantic aliases (`ACCENT = BLUE`, `MUTED = OVERLAY0`, `GOOD = GREEN`, `BAD = RED`, `WARN = PEACH`, `BORDER = SURFACE0`, `FOCUSED_BORDER = BLUE`).
- **Focused pane border**: When a pane has focus, draw the entire border in `ACCENT` (blue), not just the title. Use `Block::border_style(Style::new().fg(FOCUSED_BORDER))`.
- **`â–¶` selection marker** replaces `>` in `selectable_list` and `render_generate_field`.
- **`BorderType::Rounded`** on every `Block::default()` call.
- **`Padding::horizontal(1)`** inside all panes via `Block::padding()`.
- **Color replacements in `ui.rs`**:
  - `.cyan()` â†’ `.fg(ACCENT)` (focused/active items)
  - `.dim()` â†’ `.fg(MUTED)` (inactive/secondary content)
  - `.green()` â†’ `.fg(GOOD)`
  - `.red()` â†’ `.fg(BAD)`
  - `.yellow()` â†’ `.fg(WARN)`
  - `.bold()` â†’ keep bold but pair with `TEXT` fg where appropriate
  - Status bar mode badge `.on_cyan()` â†’ `.on(ACCENT).fg(BASE)`

## Implementation Plan
1. Create `src/colors.rs`:
   - `pub const BASE: Color = Color::Rgb(30, 30, 46);`
   - `pub const MANTLE: Color = Color::Rgb(24, 24, 37);`
   - `pub const CRUST: Color = Color::Rgb(17, 17, 27);`
   - `pub const SURFACE0: Color = Color::Rgb(49, 50, 68);`
   - `pub const SURFACE1: Color = Color::Rgb(69, 71, 90);`
   - `pub const SURFACE2: Color = Color::Rgb(88, 91, 112);`
   - `pub const OVERLAY0: Color = Color::Rgb(108, 112, 134);`
   - `pub const OVERLAY1: Color = Color::Rgb(127, 132, 156);`
   - `pub const OVERLAY2: Color = Color::Rgb(147, 153, 178);`
   - `pub const TEXT: Color = Color::Rgb(205, 214, 244);`
   - `pub const SUBTEXT0: Color = Color::Rgb(166, 173, 200);`
   - `pub const SUBTEXT1: Color = Color::Rgb(186, 194, 222);`
   - `pub const ROSEWATER: Color = Color::Rgb(245, 224, 220);`
   - `pub const FLAMINGO: Color = Color::Rgb(242, 205, 205);`
   - `pub const PINK: Color = Color::Rgb(245, 194, 231);`
   - `pub const MAUVE: Color = Color::Rgb(203, 166, 247);`
   - `pub const RED: Color = Color::Rgb(243, 139, 168);`
   - `pub const MAROON: Color = Color::Rgb(235, 160, 172);`
   - `pub const PEACH: Color = Color::Rgb(250, 179, 135);`
   - `pub const YELLOW: Color = Color::Rgb(249, 226, 175);`
   - `pub const GREEN: Color = Color::Rgb(166, 227, 161);`
   - `pub const TEAL: Color = Color::Rgb(148, 226, 213);`
   - `pub const SKY: Color = Color::Rgb(137, 220, 235);`
   - `pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236);`
   - `pub const BLUE: Color = Color::Rgb(137, 180, 250);`
   - `pub const LAVENDER: Color = Color::Rgb(180, 190, 254);`
   - Semantic aliases: `pub const ACCENT: Color = BLUE;`, `pub const MUTED: Color = OVERLAY0;`, `pub const GOOD: Color = GREEN;`, `pub const BAD: Color = RED;`, `pub const WARN: Color = PEACH;`, `pub const BORDER: Color = SURFACE0;`, `pub const FOCUSED_BORDER: Color = BLUE;`

2. Add `pub mod colors;` to `src/lib.rs` (or `src/main.rs` â€” follow where existing modules are declared).

3. In `src/ui.rs`:
   - Add `use crate::colors;`
   - Add `use ratatui::widgets::BorderType;` and `use ratatui::layout::Padding;`
   - Replace `Block::default().borders(Borders::ALL).title(...)` with a helper `fn themed_block(title: Line<'static>, focused: bool) -> Block<'static>` that returns a block with `BorderType::Rounded`, `border_style` set to `FOCUSED_BORDER` when focused or `BORDER` when not, and `Padding::horizontal(1)`.
   - Replace all `.cyan()` with `.fg(colors::ACCENT)`
   - Replace all `.dim()` with `.fg(colors::MUTED)`
   - Replace all `.green()` with `.fg(colors::GOOD)`
   - Replace all `.red()` with `.fg(colors::BAD)`
   - Replace all `.yellow()` with `.fg(colors::WARN)`
   - Replace `">"` marker string with `"â–¶"` in `selectable_list` and non-selected marker with `" "` unchanged
   - In `render_status`: change `.bold().on_cyan()` to `.bold().fg(colors::BASE).bg(colors::ACCENT)`

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "src/ui.rs", "src/lib.rs", "src/main.rs"],
  "likely_files": ["src/colors.rs", "src/ui.rs", "src/lib.rs"],
  "verification_commands": ["cargo build", "cargo check"],
  "review_focus": [
    "All Color::Rgb values match the Catppuccin Mocha spec exactly",
    "No .cyan()/.dim()/.green()/.red()/.yellow() remain in ui.rs",
    "BorderType::Rounded applied everywhere",
    "focused pane border uses FOCUSED_BORDER not just title",
    "â–¶ marker present in selectable_list and generate field renderer"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- `src/colors.rs` exists with all 26 palette entries + 7 semantic aliases
- `cargo build` passes with no warnings about unused imports
- All three panes have rounded borders
- Focused pane border is blue, unfocused border is surface0
- Selection marker is `â–¶` throughout
- No raw `.cyan()`, `.dim()`, `.green()`, `.red()`, `.yellow()` in `ui.rs` (use `colors::*` constants instead)
- Status bar mode badge uses `BASE` fg on `ACCENT` bg

## Verification Plan
- `cargo build` clean
- `cargo check` clean
- Manual: run the app, navigate all screens, verify colors render correctly in a true-color terminal

## Files Likely Touched
- `src/colors.rs` (new)
- `src/ui.rs`
- `src/lib.rs` or `src/main.rs` (module declaration)

## Risks
- Terminals without true-color support will show incorrect colors; this is an accepted tradeoff for the aesthetic gain
- Some `.bold()` calls are on `Line` values created from `String` â€” the `Stylize` trait method chains may need refactoring to use `Style::new().fg(...).bold()` applied via `.style()` instead
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-27T15:46:16+02:00
- state: implemented

## What was completed

Created `src/colors.rs` with the full Catppuccin Mocha palette (26 raw color constants + 7 semantic aliases). Added `pub mod colors;` to `src/lib.rs`. Updated `src/ui.rs` with:

- `use crate::colors;` import
- `Padding` and `BorderType` added to widget imports
- New `themed_block()` helper returning `Block` with `BorderType::Rounded`, `FOCUSED_BORDER`/`BORDER` style, and `Padding::horizontal(1)`
- All 3 panes (menu, work, preview) switched to `themed_block()` with focus-dependent border color
- `render_generate_editor` block also uses `themed_block()`
- `â–¶` replaces `>` in `selectable_list`, `render_generate_field`, and `render_generate_editor` header
- All `.cyan()` â†’ `.fg(colors::ACCENT)`, `.dim()` â†’ `.fg(colors::MUTED)`, `.green()` â†’ `.fg(colors::GOOD)`, `.red()` â†’ `.fg(colors::BAD)`, `.yellow()` â†’ `.fg(colors::WARN)` throughout
- Status bar mode badge uses `.fg(colors::BASE).bg(colors::ACCENT)` instead of `.on_cyan()`

## Deviations from plan

- `Padding` is in `ratatui::widgets` not `ratatui::layout` â€” import corrected accordingly.
- `title.dim().fg(colors::MUTED)` in `focused_title` was simplified to just `title.fg(colors::MUTED)` (no double-apply needed).

## Verification

- `cargo build` â€” clean, no warnings
- `cargo check` â€” clean, no warnings

## Important files changed

- `src/colors.rs` (new) â€” 26 palette entries + 7 semantic aliases
- `src/ui.rs` â€” full color migration + rounded borders + â–¶ markers + padding
- `src/lib.rs` â€” added `pub mod colors;`

## Residual risks / follow-up

- Terminals without true-color (24-bit) support will show nearest-palette colors; accepted tradeoff as noted in the ticket.
- The `Padding::horizontal(1)` on all panes may slightly affect text layout in very narrow terminals; no structural changes were needed but worth watching in testing.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-27T15:50:01+02:00
- state: reviewed

# Review Postmortem: 0000i Catppuccin Mocha Color Module and Border Polish

## Outcome
Accepted as implemented. No code changes required during review.

## Verification performed
- `cargo build` â€” clean, no warnings.
- `cargo check` â€” clean.
- `cargo clippy --no-deps --all-targets` â€” clean.
- `cargo test --lib` â€” 66 passed, 0 failed.
- Grep audit confirmed no residual `.cyan()`/`.dim()`/`.green()`/`.red()`/`.yellow()`/`.on_cyan()` style calls in `src/ui.rs`.
- Spot-checked Catppuccin Mocha RGB values against the published spec (BASE=#1e1e2e, MANTLE=#181825, TEXT=#cdd6f4, BLUE=#89b4fa, GREEN=#a6e3a1, RED=#f38ba8) â€” all match exactly.

## Acceptance criteria check
- `src/colors.rs` exists with all 26 palette entries + 7 semantic aliases. Verified.
- `cargo build` clean with no warnings. Verified.
- Rounded borders on all panes via `themed_block` helper. Verified.
- Focused border uses `FOCUSED_BORDER` (blue), unfocused uses `BORDER` (surface0). Verified.
- `â–¶` marker used in `selectable_list`, `render_generate_field`, and `render_generate_editor` header. Verified.
- No raw `.cyan()`/`.dim()`/`.green()`/`.red()`/`.yellow()` in `ui.rs`. Verified.
- Status bar mode badge uses `BASE` fg on `ACCENT` bg. Verified at lines 403-406.

## Observations
- The `themed_block` helper is a clean abstraction; it centralizes border type, color, padding, and title styling so future polish tickets (0000j, 0000k) only need to call it.
- `Padding::horizontal(1)` on every pane is consistent. The implementation-note residual risk about very narrow terminals is acknowledged but no fix is warranted at this stage.
- The implementation correctly used `ratatui::widgets::Padding` (not `ratatui::layout::Padding` as the original plan suggested). The deviation is documented in the implementation note.
- The `focused_title` simplification (dropping `.dim()` in favor of pure `.fg(MUTED)`) is correct â€” `.dim()` would have been a no-op once an explicit fg color is applied via Stylize chain re-application.
- All 26 palette constants are declared `pub`; only a subset are referenced by name today (BASE, ACCENT, MUTED, GOOD, BAD, WARN, FOCUSED_BORDER, BORDER, plus a few raw ones via aliases). The unused-but-defined raw palette entries are intentional foundation for the follow-up tickets and produce no warnings since they are `pub const` items in a library crate.

## Risks / follow-up
- True-color terminal dependency is an accepted tradeoff (already documented).
- No tests were added â€” appropriate, since this is pure visual styling with no logic surface that could regress meaningfully via unit test.
