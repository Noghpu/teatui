---
id: 0000y-2026-05-29-d54a969a-manage-pr-data-commands-parsers
created_at: 2026-05-29T17:14:55+02:00
created_by_model: gpt-5.5/medium
state: open
---
# Manage PR Data Commands And Parsers

## Goal
Add the typed, noninteractive `tea` command builders and tolerant JSON parsers needed by the Manage PRs viewer, without wiring them into the TUI yet.

## Context
`docs/design.md` defines Manage PRs as a shallow support mode: list open PRs, filter them, preview one, and add a plain text comment. The app already has `Screen::PullRequests`, placeholder UI, a typed `TeaClient` in `src/tea.rs`, and a shared `ExternalCommand` runner. PR generation work established the boundary to preserve: UI code should not concatenate shell strings, should not invoke interactive `tea` commands, and should keep parsing local to the wrapper/domain layer.

The installed `tea` help on the planning machine is version 0.14.0. Relevant noninteractive commands are:

- `tea pr list --state open --fields index,title,state,author,url,head,base,body,updated,labels --output json --limit 50`
- `tea pr --comments=false --fields index,title,state,author,url,head,base,body,updated,labels --output json <index>`
- `tea comment <issue / pr index> <comment body>`

This ticket should establish the first two commands and parse their output. The comment command is intentionally left to the later comment ticket so the mutating path is reviewed with its UI state.

## Non-Goals
- Do not change `App`, `BackgroundEvent`, keyboard handling, or `ui.rs` in this ticket.
- Do not implement the PR list UI, filtering, preview rendering, refresh behavior, or comments.
- Do not call `tea` through a shell or parse table/simple output.
- Do not add issue viewer support.

## Design Decisions
- Add PR viewer domain types in a small dedicated module, expected name `src/pull_requests.rs`, and export it from `src/lib.rs`.
- Keep command construction on `TeaClient` in `src/tea.rs`, following existing `labels_list_command`, `milestones_list_command`, and `pr_create_command` patterns.
- Use argv arrays only. The PR list command should include `--state open`, explicit `--fields`, `--output json`, and `--limit 50`.
- The PR detail command should use `tea pr --comments=false ... <index>` so it does not prompt for comments when run interactively-capable terminals are attached.
- Represent PR rows with a type that includes at minimum: `index`, `title`, `state`, `author`, `url`, `head`, `base`, `body`, `updated`, and `labels`.
- Parsers must be tolerant of `tea` JSON variation: numeric or string `index`, string or object `author`, string fields missing/null, labels as strings or objects with `name`, and extra fields.
- Invalid or malformed JSON should return an empty list or `None`/error-like result suitable for the caller to surface later; it must not panic.
- Keep tests focused on command argv shape and parser tolerance.

## Implementation Plan
1. Add `src/pull_requests.rs` with `PullRequestSummary` or equivalent, parser functions for list JSON and a single detail JSON payload, and small helpers for extracting author/label strings from mixed JSON shapes.
2. Export the new module from `src/lib.rs`.
3. Add `TeaClient::pr_list_command` and `TeaClient::pr_detail_command` to `src/tea.rs`.
4. Add unit tests in `src/tea.rs` for exact argv/cwd shape of the new commands.
5. Add unit tests in `src/pull_requests.rs` for representative JSON arrays/objects, missing fields, object authors, object labels, numeric/string indexes, and malformed JSON.
6. Run formatting and the relevant tests.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/tea.rs", "src/command.rs", "src/lib.rs"],
  "likely_files": ["src/tea.rs", "src/pull_requests.rs", "src/lib.rs"],
  "verification_commands": ["just test"],
  "review_focus": [
    "All tea commands are argv arrays and cannot open editor/pager/TUI behavior.",
    "PR JSON parsing is tolerant without swallowing panics through unwrap/expect in production code.",
    "No TUI/app wiring is introduced in this data-only slice."
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- `TeaClient` can build noninteractive PR list and PR detail commands with the fields and flags specified above.
- A PR JSON parser returns usable PR summary/detail values for representative `tea --output json` payloads.
- Parser helpers tolerate missing, null, string/object, and extra JSON fields without panicking.
- The new module is available to later app/UI tickets through `src/lib.rs`.
- Existing PR generation command builders and tests still pass.

## Verification Plan
- Run `just test`.
- If `just test` is too broad during implementation, first run targeted `cargo test` for `tea` and `pull_requests` tests, then run `just test` before finishing.

## Files Likely Touched
- `src/tea.rs`
- `src/pull_requests.rs`
- `src/lib.rs`

## Risks
- `tea` JSON shapes can vary across versions. Keep parser tests based on representative fields but tolerant of additional shape variation.
- The detail command syntax must not trigger comment prompts; preserve `--comments=false`.
- Avoid adding app state too early; this ticket should be easy to review as a command/parser slice.
