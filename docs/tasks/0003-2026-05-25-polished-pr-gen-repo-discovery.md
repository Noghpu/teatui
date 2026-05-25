# Repo Discovery

## Goal

Replace fake landing and Generate PR repository state with real read-only
workspace, tool, remote, and base-branch discovery.

## Outcome

The Landing view and Generate PR entry state show useful local setup status:
current workspace path, `jj` availability, whether the cwd is inside a jj
workspace, remote metadata when available, `tea` availability, and configured
Ollama endpoint/model.

## Scope

- Add a `repo` module with `RepoState`, `ToolStatus`, `RemoteInfo`, and base
  branch metadata.
- Detect `jj`, `git`, and `tea` command availability through noninteractive
  commands.
- Detect the jj workspace root.
- Detect Git remote URL and parse owner/repo for common Gitea-style remotes.
- Use config `pr.default_base` as the conservative base branch default.
- Surface setup blockers in the Landing preview pane.
- Add refresh behavior for discovery.

## Implementation Notes

- Treat discovery failures as displayable status, not fatal startup errors.
- Do not run expensive commands every tick.
- Run discovery on startup and on explicit refresh.
- Keep remote parsing local and conservative. If parsing is uncertain, show the
  raw remote and a warning rather than guessing.

## Acceptance Criteria

- Landing no longer shows hard-coded setup statuses.
- Discovery failures are visible and actionable.
- Generate PR can refuse entry or show a clear blocker when no jj workspace is
  detected.
- `Esc` and `q` behavior remains consistent with the design doc.
- `just verify` passes unless this slice only needs one focused check.

## Tests

- Unit test remote URL parsing for SSH and HTTPS forms.
- Unit test discovery status formatting if logic becomes more than trivial.
