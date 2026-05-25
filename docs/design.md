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

### View Responsibilities

#### Landing

The landing view is the operational dashboard. It should answer "can I generate
a PR from here?" without requiring a command.

Work pane:

- Current workspace path.
- jj availability and whether the current directory is inside a jj workspace.
- Gitea remote detection.
- `tea` authentication status when cheaply available.
- Ollama endpoint/model reachability.
- Detected base branch and selected revset.

Preview pane:

- Recent stack summary.
- Suggested next action.
- Blocking setup errors with exact command names.

#### Generate PR

The generate view is the primary workflow. It is a small state machine rather
than one screen.

States:

- `Idle`: no context loaded yet.
- `ContextReady`: jj/git/tea context collected and prompt can be inspected.
- `Generating`: Ollama request is running.
- `DraftReady`: branch, title, body, and notes are available for review.
- `Confirming`: commands are shown before mutation.
- `Executing`: branch, push, or `tea` command is running.
- `Complete`: PR URL and final command log are available.
- `Failed`: recoverable failure with retained context and draft.

Work pane:

- Revset selector, initially current stack.
- Optional user instruction input.
- Action list: collect context, preview prompt, generate, edit draft, create PR.
- Confirmation checklist for branch/push/PR creation.

Preview pane:

- Context summary before generation.
- Prompt preview when requested.
- Generated branch/title/body after generation.
- Proposed external commands before execution.

#### Issues

The issues view is intentionally shallow.

Work pane:

- Open issue list.
- Text filter.
- Comment command for the selected issue.

Preview pane:

- Selected issue title, author, state, labels if already returned by `tea`.
- Issue body.
- Recent comments only if the command is cheap and noninteractive.

Out of scope: editing metadata, closing issues, assignment, project boards,
milestones, bulk actions.

#### Pull Requests

The PRs view mirrors Issues with PR-specific fields.

Work pane:

- Open PR list.
- Text filter.
- Comment command for the selected PR.

Preview pane:

- Selected PR title, author, source branch, target branch, state.
- PR body.
- Recent comments only if cheap and noninteractive.

Out of scope: review approvals, line comments, merge operations, checks
management, reviewer assignment.

#### Logs

The logs view is a structured command and job history.

Work pane:

- Chronological jobs.
- Status: queued, running, succeeded, failed, cancelled.
- External tool: `jj`, `git`, `tea`, `ollama`.

Preview pane:

- Command display form.
- stdout/stderr.
- Parsed error summary.
- Redaction notice when secrets or tokens are hidden.

## Architecture

The app starts from the Ratatui component template and uses an Elm-style loop:

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

### Core State

The first implementation should keep state explicit rather than hiding it behind
generic widget maps.

```rust
struct App {
    config: Config,
    repo: RepoState,
    active_view: View,
    focus: Focus,
    generate: GenerateState,
    issues: IssueState,
    prs: PullRequestState,
    logs: LogState,
    jobs: JobRegistry,
}
```

Important domain types:

- `RepoState`: workspace root, remote URL, owner/repo, base branch, selected
  revset, tool availability.
- `GenerateState`: current phase, context bundle, generated draft, editable
  fields, confirmation state.
- `ContextBundle`: the exact data sent to the model plus size/truncation
  metadata.
- `GeneratedDraft`: branch name, title, body, review notes, raw model response.
- `ExternalCommand`: program, args, cwd, env redactions, display string.
- `JobResult`: status, timing, stdout/stderr, parsed payload, error summary.

### Action Flow

User input should produce high-level actions, and actions should produce either
state changes or async jobs.

```text
KeyEvent -> Action -> update(App) -> CommandRequest? -> JobResult -> Action
```

Examples:

- `g` in Generate/Idle starts context collection.
- `g` in Generate/ContextReady starts Ollama generation.
- `Enter` in Generate/DraftReady opens the confirmation state.
- `Enter` in Generate/Confirming starts the PR execution job.
- `r` refreshes the current view only.
- `c` in Issues/PRs opens a comment input modal.

Async jobs should report progress through channels back into the event loop.
Rendering should never block on process output or network IO.

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

### Command Policy

All external commands must be constructed as argv arrays and run without a
shell. Wrappers should own command policy so UI code never concatenates command
strings.

Allowed command categories:

- Read-only discovery: repo root, status, logs, diffs, remote metadata.
- Draft preparation: branch name validation, command preview construction.
- Mutating PR execution: branch creation/update, push, `tea` PR creation.
- Simple browsing: list/view issues and PRs.
- Simple comments: add comment text to a selected issue or PR.

Rejected command categories:

- Commands that can open editors or pagers.
- Interactive merge/conflict resolution.
- `tea` TUI launches.
- Destructive jj operations not required for PR generation.
- Hidden pushes or PR creation before confirmation.

### jj Context Commands

The exact commands will be refined against jj's stable output formats, but the
wrapper should expose these operations:

- Detect workspace root.
- Read current status.
- Read selected revset log.
- Read selected revset descriptions.
- Read selected revset diff and diff stats.
- Identify trunk/base when possible.

Prefer machine-readable formats where jj provides them. If a wrapper must parse
text, keep parsing local to the wrapper and preserve raw command output for the
logs view.

### PR Execution Sequence

PR creation must be split into previewable steps:

1. Validate generated branch name, title, and body.
2. Confirm selected revset and base branch have not changed since context
   collection, or show a stale-context warning.
3. Create or move the local PR branch to the selected jj commit/stack tip.
4. Push the branch to the configured remote.
5. Create the PR with `tea`.
6. Capture the PR URL and show it in the Complete state.

The first implementation may require a simple linear stack and can reject
ambiguous multi-head or conflicted states with a clear error.

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

### Prompt Contract

The prompt builder should return both the final prompt string and a structured
manifest describing what was included:

```rust
struct PromptManifest {
    selected_revset: String,
    base_branch: String,
    included_sections: Vec<PromptSection>,
    omitted_sections: Vec<OmittedSection>,
    byte_count: usize,
    truncation_warnings: Vec<String>,
}
```

The review UI should show the manifest by default and the full prompt on
request. That keeps the normal flow compact while preserving auditability.

### Prompt Outline

The initial prompt should use this shape:

```text
You are helping write a Gitea pull request for a jj-managed repository.

Return strict JSON matching this schema:
{ ... }

Rules:
- Use only the context below.
- Do not invent tests, issue links, reviewers, or behavior.
- If context is missing, mention the uncertainty in review_notes.
- Prefer a short branch name with lowercase words separated by hyphens.
- Write a concise title.
- Write a PR body with Summary, Testing, Risks, and Notes sections.

Repository:
...

Selected jj changes:
...

Status:
...

Log:
...

Diff stats:
...

Diff:
...

User instructions:
...
```

The app should validate the response by parsing JSON first, then validating
fields. Invalid responses remain visible in Logs and can be retried.

### Ollama Contract

The Ollama client should target the generate/chat endpoint exposed by local and
on-prem deployments through config. The app should avoid provider-specific
prompt features unless Ollama supports them directly.

Expected request settings:

- Model from config.
- Non-streaming response for the first version.
- Low temperature for stable PR metadata.
- Timeout with visible cancellation.

Streaming can be added later if the review pane needs progressive output.

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

## Safety and Review

The tool is allowed to automate tedious command sequences, but it should make
state changes boring and inspectable.

Safety rules:

- No push or PR creation before a confirmation screen.
- No generated text is trusted until parsed and displayed.
- No model-generated command is ever executed.
- Branch names are validated locally, not trusted from the model.
- Stale repo context is detected before mutation when possible.
- Command logs preserve enough raw output to debug failures.

Sensitive data:

- Redact tokens and authorization headers from logs.
- Do not include local config secrets in prompts.
- Do not include environment variables in prompts unless explicitly allowlisted.
- Treat remotes and issue/PR text as prompt-visible project data.

## Testing Strategy

- Unit tests for prompt assembly, JSON parsing, branch-name validation, and
  command argument construction.
- Snapshot-style tests for rendered UI states where useful.
- Integration tests using fake `jj`, `git`, `tea`, and Ollama command/server
  shims.
- Manual terminal checks for resize behavior and keyboard navigation.

## Implementation Order

The next slices should keep the app usable at each step:

1. Repo/tool detection in Landing with log output.
2. Shared async command runner and job registry.
3. jj context collection for the Generate view.
4. Prompt manifest and prompt preview.
5. Ollama client and strict JSON parsing.
6. Draft review/edit state.
7. Branch/push/`tea` execution preview and confirmation.
8. Minimal Issues list/detail/comment.
9. Minimal PRs list/detail/comment.

## Open Questions

- Which jj revset should be default for PR generation: `@`, `@-`, or
  `heads(trunk()..)`?
- Should the app require an explicit base branch in config, or infer it from
  remotes/trunk and only fall back to config?
- Which `tea` commands provide the most stable noninteractive output for issue
  and PR lists?
- Should PR branch creation use `git branch` directly or jj's git export model
  depending on repo configuration?
- How large should the initial prompt byte budget be for local Ollama versus
  the on-prem model?

## Milestones

1. Project scaffold: component-template Ratatui app, docs, config skeleton.
2. Repo detection: show current jj repo status and log in the TUI.
3. Prompt assembly: collect context and render prompt preview.
4. Ollama generation: call local endpoint and parse strict JSON.
5. PR review screen: edit branch, title, and body before execution.
6. PR execution: create branch, push, and call `tea` to open PR.
7. Simple issue/PR listing and comment commands.
