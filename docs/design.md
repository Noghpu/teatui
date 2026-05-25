# teatui Design

## Purpose

`teatui` is a Ratatui-based Rust TUI for creating pull request branches and
pull requests from jj-managed repositories targeting Gitea. It wraps the
existing `tea` CLI/TUI where possible instead of replacing Gitea behavior.

The primary workflow is:

1. Inspect the current jj repo and selected change stack.
2. Gather all context needed to describe the work.
3. Send one complete prompt to a local Ollama-compatible LLM.
4. Review the generated branch name, PR title, and PR body in the TUI.
5. Create the branch and PR through jj/git plus `tea`.

Secondary workflows are deliberately small: list repo issues, list repo PRs,
view details, and optionally add a short comment.

## Product Principles

- PR generation is the main product surface. Browsing issues and existing PRs is
  supporting context, not a full issue tracker.
- The app should feel like a focused terminal code-review tool, borrowing from
  `octo.nvim` navigation and layout: list panes, preview panes, compact status
  bars, keyboard-first movement, and modal actions.
- AI output must be reviewable and editable before any branch, push, or PR
  operation runs.
- Local-first is the default. Development targets local Ollama, production uses
  an on-prem Ollama-compatible endpoint.
- External tools remain explicit dependencies: jj manages repo state, `tea`
  handles Gitea API/auth behavior, and the LLM only drafts PR metadata.

## Non-Goals

- Reimplementing the full `tea` TUI.
- Replacing jj workflows, interactive conflict resolution, or stack management.
- Multi-provider AI abstraction before Ollama-compatible endpoints are proven.
- Complex issue triage, labels, milestones, assignments, or project boards.
- Autonomous PR creation without a user review step.

## Target Users

Developers working in jj-managed repositories hosted on Gitea who want a
fast terminal flow for converting a jj change stack into a high-quality PR.

## First Release Scope

### Primary Flow: Generate PR

Inputs:

- Current jj workspace root.
- Current change or selected revset.
- Base branch or trunk inference.
- Repository remote metadata.
- User-provided optional instruction text.

Collected context:

- `jj status`.
- `jj log` for the selected stack and nearby trunk context.
- `jj diff` for the selected change stack.
- Existing change descriptions.
- Git remote and current branch information needed by `tea`.
- Optional recent issue/PR references selected in the UI.

Generated output:

- Branch name.
- PR title.
- PR body in Markdown.
- Optional reviewer notes explaining what the model inferred.

Execution:

- Create or update a PR branch from the selected jj work.
- Push the branch.
- Create the PR with `tea`.

Every mutating step is shown in a confirmation view before execution.

### Secondary Flow: Issues

- List open issues for the current repo.
- Filter by simple text search.
- Open a detail preview.
- Add a plain text comment.

### Secondary Flow: Pull Requests

- List open PRs for the current repo.
- Filter by simple text search.
- Open a detail preview.
- Add a plain text comment.

## UI Model

The app uses the multi-view Ratatui component template. The UI follows a
compact, keyboard-first layout inspired by `octo.nvim`:

- Views: `Landing`, `Generate PR`, `Issues`, `PRs`, `Logs`.
- Left navigation rail: one entry per view.
- Center list/work area: selected workflow items or form fields.
- Right preview pane: diff summary, generated PR body, issue body, or PR body.
- Bottom command/status bar: mode, current repo, background job state, help.

The landing view is intentionally useful, not a marketing screen. It should show
current repository detection, auth/tool availability, and the next likely action.

Primary modes:

- `Normal`: navigate panes and lists.
- `Input`: edit prompt notes, comments, branch name, title, and body.
- `Review`: inspect generated PR output and proposed commands.
- `Running`: background command or LLM request in progress.

Initial keybindings:

- `j`/`k` or arrows: move selection.
- `h`/`l` or tab/backtab: move focus between panes.
- `Enter`: open item or activate command.
- `g`: generate PR proposal.
- `e`: edit current generated field or comment.
- `r`: refresh current view.
- `c`: comment on selected issue or PR.
- `q`: quit or close modal.

## Architecture

The app starts from the Ratatui async template and uses an Elm-style loop:

- `App`: in-memory state, selected view, focused pane, forms, job status.
- `Message`: user input, timer ticks, background command results, LLM results.
- `update`: pure state transitions plus commands to spawn async work.
- `view`: renders lists, previews, modals, and status bars.

Planned modules:

- `main`: startup, terminal lifecycle, panic restoration.
- `app`: app model and update logic.
- `ui`: Ratatui rendering.
- `event`: keyboard/event stream handling.
- `command`: async process runner for `jj`, `git`, and `tea`.
- `repo`: workspace detection and repo metadata.
- `jj`: typed wrappers around jj commands.
- `tea`: typed wrappers around tea commands.
- `ollama`: local/on-prem LLM client.
- `prompt`: context assembly and PR generation prompt.
- `config`: endpoint, model, defaults, and command paths.

## External Command Boundaries

`teatui` should use structured command wrappers rather than shell strings.

Command wrappers return:

- Command name and args.
- Exit status.
- stdout/stderr.
- Parsed data when the command supports machine-readable output.
- A redacted display form for review screens.

The app should avoid interactive external commands. Any operation that can open
an editor, pager, merge tool, or TUI must be rejected or called with flags that
force noninteractive behavior.

## AI Prompt Strategy

PR generation uses exactly one LLM request per generation attempt. The request
contains all gathered repo context plus detailed instructions.

The model is asked to return strict JSON:

```json
{
  "branch_name": "feature/example-branch",
  "title": "Short PR title",
  "body": "Markdown PR body",
  "review_notes": [
    "Important inference or uncertainty"
  ]
}
```

Prompt sections:

- Role: expert maintainer writing clear Gitea PRs from jj context.
- Output contract: strict JSON, no Markdown fence, no extra commentary.
- Repository summary: root, remotes, trunk/base, selected revset.
- Change metadata: jj status, log, descriptions.
- Diff context: complete selected diff when size allows, otherwise summarized
  file-level context plus explicit truncation notes.
- User intent: optional notes typed in the UI.
- Writing rules: concise title, conventional branch name, PR body with summary,
  rationale, testing, risks, and review notes.
- Safety rules: do not invent tests, reviewers, issue links, or behavior not
  supported by provided context.

Large context handling:

- Start with hard byte/token budgets.
- Prefer full diffs for small changes.
- For large changes, include file lists, stats, selected hunks, and explicit
  truncation markers.
- Surface truncation in the review pane before generation.

## Configuration

Initial config file location:

- Linux/macOS: `$XDG_CONFIG_HOME/teatui/config.toml`.
- Windows: platform config directory via `dirs`.

Initial fields:

```toml
[ollama]
base_url = "http://localhost:11434"
model = "qwen2.5-coder:latest"

[commands]
jj = "jj"
git = "git"
tea = "tea"

[pr]
default_base = "main"
```

Environment overrides:

- `TEATUI_OLLAMA_BASE_URL`
- `TEATUI_OLLAMA_MODEL`
- `TEATUI_JJ`
- `TEATUI_GIT`
- `TEATUI_TEA`

## Error Handling

- Restore terminal state on panic.
- Show command failures in a logs pane with stdout/stderr.
- Keep failed generated PR drafts in memory for editing or retry.
- Never run branch, push, or PR creation if context gathering failed.
- Treat malformed LLM JSON as a recoverable generation error.

## Testing Strategy

- Unit tests for prompt assembly, JSON parsing, branch-name validation, and
  command argument construction.
- Snapshot-style tests for rendered UI states where useful.
- Integration tests using fake `jj`, `git`, `tea`, and Ollama command/server
  shims.
- Manual terminal checks for resize behavior and keyboard navigation.

## Milestones

1. Project scaffold: async Ratatui app, docs, config skeleton.
2. Repo detection: show current jj repo status and log in the TUI.
3. Prompt assembly: collect context and render prompt preview.
4. Ollama generation: call local endpoint and parse strict JSON.
5. PR review screen: edit branch, title, and body before execution.
6. PR execution: create branch, push, and call `tea` to open PR.
7. Simple issue/PR listing and comment commands.
