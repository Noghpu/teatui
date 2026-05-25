# Draft Review

## Goal

Turn generated PR metadata into a polished review and edit experience before
any mutation is implemented.

## Outcome

After generation, the user can inspect and edit branch name, title, body, and
review notes context in the TUI. The workflow feels complete through draft
approval, even though branch push and PR creation remain out of scope.

## Scope

- Add `DraftReady` review rendering in the right and center panes.
- Show generated:
  - branch name
  - PR title
  - PR body
  - review notes
  - manifest warnings
- Reuse the form editing machinery for draft fields.
- Add clear state for retry generation without losing context.
- Add a non-mutating command preview placeholder for future execution.
- Add logs access or log preview sufficient to inspect command and model
  failures.
- Make resize behavior reasonable for narrow terminals.

## Implementation Notes

- Keep draft state in memory only for now.
- Do not implement push or `tea pr create` in this slice.
- Avoid modal complexity unless the current layout cannot show the data
  cleanly.
- Use Ratatui `Paragraph`, wrapping, scrolling, and state objects rather than
  custom rendering where built-ins work.

## Acceptance Criteria

- Draft fields are reviewable and editable.
- Failed generation can be retried.
- User edits to generated draft are preserved while navigating.
- The UI clearly says execution is not implemented yet.
- Text does not overlap or become unreadable in ordinary terminal sizes.
- `just verify` passes unless this slice only needs one focused check.

## Tests

- Unit test draft field edit behavior if separate from form fields.
- Add snapshot-style render tests only if they catch real layout risk without
  creating a brittle regression farm.
