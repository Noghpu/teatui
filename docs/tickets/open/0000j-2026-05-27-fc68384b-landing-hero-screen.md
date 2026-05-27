---
id: 0000j-2026-05-27-fc68384b-landing-hero-screen
created_at: 2026-05-27T15:28:43+02:00
created_by_model: claude-sonnet-4-6
state: open
---
# Landing Hero Screen (LazyVim-Style Full-Width Dashboard)

## Goal
Replace the 3-pane landing screen layout with a full-width hero dashboard inspired by the LazyVim/lazy.nvim startup screen. The landing screen gets its own layout: a centered header area (app name + tagline), a centered action list (icon + label on left, key hint on right), and a compact status footer. Other screens (Generate, PRs, Issues) keep the 3-pane layout unchanged.

## Context
The current landing screen reuses the same 3-pane layout as Generate/PRs/Issues, making it feel like just another form screen. The design doc (`docs/design.md`) explicitly calls for a "LazyVim-style" landing and an "operational dashboard." The reference screenshot shows the LazyVim dashboard: large header, full-width centered action list with icons and key hints, and a footer line.

This ticket depends on ticket `0000i` (Catppuccin Mocha colors) being implemented first, as it uses the `colors::*` constants.

## Non-Goals
- No changes to Generate, PullRequests, or Issues screens
- No 3-pane layout for the landing screen
- No ASCII art logo (simple styled text header)
- No nerd-font icons (use plain Unicode that works in any terminal)

## Design Decisions
- **Conditional layout**: In `render()`, branch on `Screen::Landing` before the horizontal split. The landing uses its own vertical layout; all other screens use the existing 3-pane horizontal split.
- **Three vertical areas**: `header_area` (fixed height ~5 lines), `actions_area` (fills), `footer_area` (1 line).
- **Header**: Centered block with no border. Two lines: `teatui` in bold TEXT color, dim tagline `jj Â· Gitea Â· LLM`. Vertically padded with blank lines above.
- **Actions list**: No border. Each action is a full-width row with: left side = `icon  Label` (icon in MUTED, label in ACCENT when selected, TEXT when not), right side = key hint in MUTED. Selected row gets `â–¶` prefix, others get `  ` prefix. Spacing: one blank line between items (or use `List` with spacing if ratatui 0.30 supports it; otherwise render manually as `Paragraph`).
- **Action items** (in order):
  1. `â—†  Generate PR` â†’ key `g` (or Enter)
  2. `â—†  Manage PRs` â†’ key `p`
  3. `â—†  Manage Issues` â†’ key `i`
  4. (blank spacer)
  5. `â—†  Quit` â†’ key `q`
- **Footer**: Single `Line` showing compact status: each tool/service as `âœ“ name` (GREEN) or `âœ— name` (RED) or `Â· name` (MUTED). Items separated by `  `. Example: `âœ“ jj  âœ“ git  âœ“ tea  âœ“ LLM: ollama/qwen2.5  Â· workspace`.
- **Centering**: Use `Layout::horizontal` with `Constraint::Percentage(20)`, `Constraint::Percentage(60)`, `Constraint::Percentage(20)` to center the content horizontally. The outer 20% columns are empty.
- **Keyboard**: The existing `LandingState.selected_entry` drives which action is highlighted. The existing key handling (`j`/`k` to navigate, `Enter` to select, `q` to quit) must still work â€” only the rendering changes.
- **`render_menu` and `render_work`**: These functions are currently called for all screens including Landing. After this ticket, `render()` should skip calling them for `Screen::Landing` and instead call a new `render_landing_hero(frame, app, area)` function that handles the full-width layout internally.

## Implementation Plan
1. In `src/ui.rs`, modify `render()`:
   - Before the horizontal split, add: `if app.screen() == Screen::Landing { render_landing_hero(frame, app, main_area); render_status(...); render_help(...); return; }`
   - The status and help bars still render the same way for Landing.

2. Add `fn render_landing_hero(frame: &mut Frame, app: &App, area: Rect)`:
   - Split `area` into `[header_area, actions_area, footer_area]` using `Layout::vertical` with `[Constraint::Length(5), Constraint::Fill(1), Constraint::Length(1)]`
   - Split each sub-area horizontally into `[_, center, _]` with `[Constraint::Percentage(20), Constraint::Percentage(60), Constraint::Percentage(20)]`
   - Render header, actions, footer into the center columns

3. **Header rendering**: `Paragraph::new(vec![Line::from(""), Line::from("teatui".bold().fg(colors::TEXT)), Line::from("jj Â· Gitea Â· LLM".fg(colors::MUTED)), Line::from("")])` with `.alignment(Alignment::Center)`

4. **Actions rendering**: Build a `Vec<Line>` with one line per action plus blank spacers. For each action, use `Line::from(vec![Span, Span, Span])` â€” left spans for icon+label, right span for key. To push the key to the right edge, use `Line::from(...)` with the key as a right-aligned span. Since ratatui `Line` doesn't support mixed alignment, pad with spaces: compute padding width from `center_width - left_text_len - key_len`. Render as `Paragraph::new(lines)`.

   Selected action: prefix `â–¶ ` in ACCENT, label in ACCENT.
   Unselected action: prefix `  `, label in TEXT, icon in MUTED.
   Key hint: MUTED.

5. **Footer rendering**: Build a single `Line` from the `RepoState`. For each tool (jj, git, tea), emit `âœ“ name` in GOOD or `âœ— name` in BAD or `Â· name` in MUTED. Add LLM backend name. Render with `.alignment(Alignment::Center)`.

6. Remove the now-unused Landing branches from `render_menu` and `render_work` â€” or leave them as dead code with a comment; the implementer can clean up if it's safe.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "src/ui.rs", "src/app.rs", "src/repo.rs", "docs/design.md"],
  "likely_files": ["src/ui.rs"],
  "verification_commands": ["cargo build", "cargo check"],
  "review_focus": [
    "Screen::Landing gets full-width layout, not 3-pane",
    "Other screens (Generate, PullRequests, Issues) are unaffected",
    "Horizontal centering via percentage split",
    "Action list selection works with existing LandingState.selected_entry",
    "Footer shows real tool status from RepoState",
    "colors::* constants used throughout (no raw .cyan()/.dim() etc.)"
  ],
  "jj_description_prefix": "ui"
}
```

## Acceptance Criteria
- Landing screen shows full-width hero layout (no left menu pane, no right preview pane)
- Header shows `teatui` centered with dim tagline
- Three mode actions + quit rendered as a centered list with key hints on the right
- Selected action uses `â–¶` prefix and ACCENT color
- Footer shows live tool status from `RepoState`
- Pressing `j`/`k` moves selection through actions correctly
- Pressing `Enter` on an action opens the correct screen
- `q` still quits from landing
- All other screens unchanged

## Verification Plan
- `cargo build` clean
- Manual: launch app, verify hero layout renders
- Manual: press `j`/`k` to navigate actions, verify selection highlight moves
- Manual: press `Enter` to open Generate PR, then `Esc` back to Landing
- Manual: verify other screens still use 3-pane layout

## Files Likely Touched
- `src/ui.rs`

## Risks
- Key padding arithmetic for right-aligning key hints may be off if the center column width is dynamic; test at multiple terminal widths
- If `LandingState.selected_entry` only handles 3 items (0-2) but we add a Quit row, ensure `j`/`k` bounds are correct â€” the Quit action may need to be special-cased as a key press rather than a selectable entry
