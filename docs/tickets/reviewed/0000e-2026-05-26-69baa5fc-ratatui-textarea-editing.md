---
id: 0000e-2026-05-26-69baa5fc-ratatui-textarea-editing
created_at: 2026-05-26T08:41:29+02:00
created_by_model: unknown
state: reviewed
state_updated_at: 2026-05-26T11:37:00+02:00
---
# Adopt ratatui-textarea for PR Form Editing

## Goal
Replace the current append-only PR form editing behavior with `ratatui-textarea`-backed editing for text fields and text areas, while keeping the existing explicit TEA-style app state and navigation model. Add a design note to `docs/design.md` documenting `rat-dialog` as the preferred deferred candidate for future modal/window stacks, but do not implement `rat-dialog` in this ticket.

## Context
The Generate PR flow currently stores editable form values in `FieldState` and handles editing by appending printable characters or popping the last character. That is enough for early wiring, but it is too weak for the app's primary review surface: users need to edit generated branch names, titles, PR descriptions, and later comments without fighting cursor and multi-line behavior.

The design already requires mode-specific text input handling: printable keys must insert text while editing, and global keybindings such as `g`, `q`, `j`, and `k` must not fire from text input. The current `InputMode::Editing` split in `src/app.rs` should remain the boundary for that behavior.

`ratatui-textarea` is the narrowest useful dependency because it provides cursor-aware single-line and multi-line editing without requiring a broader framework rewrite. `rat-dialog` may become useful when comment inputs, command confirmation, logs, and other overlays need stacked modal behavior, but adopting it now would be premature.

## Non-Goals
- Do not rewrite the app around `rat-salsa`, `rat-widget`, `ratatui-interact`, `ratatui-form`, `tui-realm`, or another broad framework.
- Do not implement `rat-dialog`, modal stacks, popup comment boxes, or execution confirmation windows in this ticket.
- Do not change the prompt contract, Ollama request flow, jj context collection, branch validation rules, or PR execution behavior.
- Do not add a large test farm or snapshot suite; keep tests focused on risky state/input behavior.

## Design Decisions
- Add `ratatui-textarea` as the editing primitive for text values instead of building custom cursor movement and multi-line logic.
- Keep `PrForm`, `GenerateState`, and field-specific validation explicit so prompt assembly and dirty-field behavior remain domain-owned.
- Preserve the existing navigation/editing split: navigation mode owns pane/field movement, editing mode routes keys only to the active field editor except for explicit commit/cancel keys.
- Use `ratatui-textarea` for the PR description as a true multi-line editor.
- Use either `ratatui-textarea` single-line mode or a small shared wrapper around it for branch name, base/head text entry, title, labels, assignees, and milestone. Single-line fields must ignore newline insertion.
- Continue treating user-edited fields as dirty so generated draft sync does not overwrite explicit user intent.
- Add a short `docs/design.md` note under deferred implementation or architecture notes stating that `rat-dialog` is the preferred candidate for future modal/window stacks, especially comment inputs, command confirmation, and logs, but it should be introduced only when a modal feature needs it.

## Implementation Plan
1. Add `ratatui-textarea` to `Cargo.toml` and update `Cargo.lock`.
2. Introduce a local field-editor abstraction in `src/generate.rs` that owns the displayed value, committed value, dirty flag, validation errors, and a `ratatui_textarea::TextArea` editing buffer.
3. Preserve the existing public domain shape where practical: `PrForm` should still expose field values for prompt assembly, validation, draft sync, and UI rendering without leaking `ratatui-textarea` details through unrelated modules.
4. Update edit actions in `src/app.rs` so `KeyEvent`s in `InputMode::Editing` are converted to `ratatui_textarea::Input` for the selected field. `Esc` must cancel editing and restore the previous committed value. `Enter` should commit single-line fields and insert or be explicitly handled for the multi-line description according to the chosen UX below.
5. Define the description editor UX explicitly in code and help text: `Enter` inserts a newline in `description`; another deterministic key such as `Ctrl+S` commits the multi-line field, and `Esc` cancels. For single-line fields, `Enter` commits and must not insert a newline.
6. Update `Action` only as needed to carry raw edit key events or editor-specific actions. Keep global navigation keys inactive while editing.
7. Update `src/ui.rs` rendering so focused/editing fields render through the textarea widget, including visible cursor behavior where supported by Ratatui. Non-editing fields may continue to render as styled `Line`s if that keeps the display compact.
8. Keep branch-name and required-field validation behavior equivalent to the current behavior, including allowing an empty branch name before generation and blocking only invalid non-empty branch names.
9. Update help/status text to explain single-line commit, multi-line description newline behavior, commit key, and cancel key.
10. Update `docs/design.md` with the `rat-dialog` deferred note described above.
11. Add focused unit tests for dirty tracking, commit/cancel behavior, single-line newline suppression, description multi-line behavior, and global key suppression while editing.
12. Run `just verify`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/design.md"
  ],
  "likely_files": [
    "Cargo.toml",
    "Cargo.lock",
    "src/action.rs",
    "src/app.rs",
    "src/generate.rs",
    "src/ui.rs",
    "docs/design.md"
  ],
  "verification_commands": [
    "just verify"
  ],
  "review_focus": [
    "Editing mode must not trigger global keybindings for printable keys or navigation letters.",
    "Single-line fields must not accidentally accept newline content.",
    "Description editing must support multi-line PR bodies without making commit/cancel ambiguous.",
    "Generated draft sync must still preserve fields that the user has dirtied.",
    "The design note must document rat-dialog as deferred, not as an immediate dependency."
  ]
}
```

## Acceptance Criteria
- `Cargo.toml` includes `ratatui-textarea`, and the lockfile is updated.
- Branch name, base/head, title, labels, assignees, and milestone support cursor-aware single-line editing.
- Description supports multi-line editing suitable for PR body review.
- Editing mode continues to suppress global keybindings; typing `g`, `c`, `q`, `j`, or `k` into a field changes the field instead of generating, commenting, quitting, or navigating.
- `Esc` cancels an active edit and restores the committed field value.
- Commit behavior is deterministic for both single-line and description fields and is reflected in help text.
- Current validation semantics are preserved for required fields and optional branch-name validation.
- Generated draft sync continues to avoid overwriting dirty user-edited fields.
- `docs/design.md` contains a note that `rat-dialog` is the deferred candidate for future modal/window stacks and is not part of the current textarea implementation.
- `just verify` passes.

## Verification Plan
- Run focused unit tests covering field commit/cancel, dirty tracking, single-line newline suppression, description multi-line editing, and editing-mode key routing.
- Run `just verify` for formatting, compile check, linting, and tests.
- If a manual TUI probe is practical, run the app and verify that editing a generated PR body allows multi-line changes and that global keys insert text while editing.

## Files Likely Touched
- `Cargo.toml`
- `Cargo.lock`
- `src/action.rs`
- `src/app.rs`
- `src/generate.rs`
- `src/ui.rs`
- `docs/design.md`

## Risks
- `ratatui-textarea` owns cursor/rendering state, so the implementation must avoid cloning or rebuilding editors in a way that loses cursor position on each render.
- Multi-line commit behavior can be confusing if `Enter` both commits and inserts newlines; make this explicit and test it.
- The existing prompt/draft code expects simple string field values; keep a clear committed-value accessor so prompt assembly does not consume transient edit buffers by accident.
- Adding a widget dependency may require adapting imports or feature defaults to match the repo's Ratatui version.

---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5
- reviewed_at: 2026-05-26T11:37:00+02:00
- state: reviewed

Facts:
- Fixed the review findings for the textarea implementation.
- Routed editor navigation keys to the active `ratatui-textarea` field.
- Rendered the active editor through the textarea widget so cursor state is visible.
- Updated `ratatui-textarea` to a version compatible with the repository's Ratatui version, removing the duplicate older Ratatui dependency chain.

Verification:
- `just verify`
