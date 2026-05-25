# Form Editing

## Goal

Implement the polished keyboard-first PR form interaction that the product
depends on.

## Outcome

The center pane behaves like a real form: users can navigate fields, edit text,
commit or cancel edits, and type characters that would otherwise be global
keybindings.

## Scope

- Implement `Normal`, `Input`, and `Review` mode handling for Generate PR.
- Add form fields:
  - `head`
  - `branch name`
  - `base`
  - `title`
  - `description`
  - optional `labels`
  - optional `assignees`
  - optional `milestone`
  - optional user instructions if not already represented by description
- Support `j/k` or arrows for field navigation in normal mode.
- Support `Enter` or `i` to edit the focused field.
- In input mode, printable keys modify the focused field buffer and never
  trigger global keybindings.
- Support `Esc` to leave input mode without leaving Generate PR.
- Decide and implement commit/cancel semantics for text edits.
- Add validation display for required fields and branch-name shape.

## Implementation Notes

- Keep a single simple field-editing implementation shared by form and draft
  review fields.
- Do not introduce a generic form engine.
- Use stable field enum ordering rather than stringly typed field selection.
- For pickers, use editable text input for the first version. Suggestions can be
  displayed without a dropdown component.

## Acceptance Criteria

- Typing `g`, `q`, `j`, or `k` inside a text field inserts text.
- `Esc` in input mode returns to form navigation.
- `Esc` in Generate normal/review mode returns to Landing.
- Dirty fields are tracked.
- Validation errors are visible without blocking normal navigation.
- `just verify` passes unless this slice only needs one focused check.

## Tests

- Unit test key-to-action behavior for normal vs input mode.
- Unit test field commit/cancel behavior.
- Unit test branch-name validation.
