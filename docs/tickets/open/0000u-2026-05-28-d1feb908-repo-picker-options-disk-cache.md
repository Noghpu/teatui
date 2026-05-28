---
id: 0000u-2026-05-28-d1feb908-repo-picker-options-disk-cache
created_at: 2026-05-28T11:42:07+02:00
created_by_model: gpt-5
state: open
---
# Load And Cache Repo Picker Options

## Goal
Fetch valid `labels`, `assignees`, and `milestone` picker options from the current Gitea repository, hydrate them from disk cache when possible, and refresh them asynchronously so users do not wait on every Generate PR entry.

## Context
The picker ticket makes `labels`, `assignees`, and `milestone` picker-only fields, but it intentionally does not load real repo options. The local `tea` CLI exposes noninteractive JSON output for labels and milestones:

- `tea labels list --output json --limit 100`
- `tea milestones list --state open --output json --limit 100`

For assignee candidates, `tea` 0.14.0 does not expose a top-level `users` command. It does expose `tea api`, and its help says placeholders such as `{owner}` and `{repo}` are replaced from the current repository context. Use a noninteractive API call for repository collaborators as the first source of valid assignees:

- `tea api '/repos/{owner}/{repo}/collaborators'`

All command execution must continue to use argv arrays through the existing command boundary; do not concatenate shell strings.

## Non-Goals
- Do not implement issue/PR browsing metadata editing.
- Do not add label, user, or milestone creation.
- Do not block rendering while metadata loads.
- Do not cache secrets or raw authorization material.

## Design Decisions
- Option loading is best-effort. Missing `tea`, missing auth, command failures, or parse failures should leave affected pickers disabled with a visible reason, not crash the app or block PR generation.
- Use stale-while-revalidate disk caching per repository. On Generate PR entry or refresh, load cache first if present, update picker options immediately, and start background refresh if the cache is older than 15 minutes.
- If live refresh fails, stale cached data up to 7 days old may remain usable with a warning in the picker/right pane. Older cache is ignored except for diagnostics.
- Cache location is `dirs::cache_dir()/teatui/repo-options/<repo-key>.json`, where `<repo-key>` is a sanitized stable key from Gitea host, owner, and repo name. Store only option metadata needed by the UI, such as names, ids, colors, states, and timestamps.
- Manual `r` refresh in Generate PR ignores the 15-minute TTL, starts a live refresh, and rewrites the cache on success.
- Labels and assignees are multi-select. Milestone is optional single-select and should list open milestones by default.

## Implementation Plan
1. Add typed `TeaClient` command builders for labels, milestones, and collaborators API calls. Use argv arrays and existing `ExternalCommand`/`capture` patterns.
2. Add parser types/functions for the JSON returned by `tea labels list`, `tea milestones list`, and the collaborators API. Keep parsers tolerant of extra fields but strict enough to reject missing display values.
3. Add a small repo-options module or explicit repo/app state to represent option loading status, option lists, cache freshness, and user-visible warnings.
4. Add async background loading that first reads the disk cache, sends cached options to the app, then runs live `tea` commands when needed.
5. Add a cache schema with a version number, fetched timestamp, repo identity, and option arrays. Write cache files atomically by writing a temporary file in the same directory and renaming it into place.
6. Wire loaded options into the picker state from the previous ticket without overwriting user selections that are still valid. If a selected value disappears from refreshed options, keep it selected but mark it invalid or stale until the user changes it.
7. Update `r` refresh in Generate PR to refresh revsets and repo picker options.
8. Add focused unit tests for JSON parsers, cache freshness decisions, cache key sanitization, and preserving valid selected picker values across refresh.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/tea.rs", "src/repo.rs", "src/app.rs", "src/event.rs", "src/generate.rs", "src/command.rs", "src/config.rs"],
  "likely_files": ["src/tea.rs", "src/repo.rs", "src/app.rs", "src/event.rs", "src/generate.rs", "src/command.rs", "src/config.rs"],
  "verification_commands": ["just verify"],
  "review_focus": ["Repo option loading is async and nonblocking", "Disk cache uses repo-scoped stale-while-revalidate behavior", "Only valid fetched options become selectable for labels, assignees, and milestone"],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- Generate PR can populate label picker options from `tea labels list --output json --limit 100`.
- Generate PR can populate milestone picker options from `tea milestones list --state open --output json --limit 100`.
- Generate PR can populate assignee picker options from `tea api '/repos/{owner}/{repo}/collaborators'`.
- Picker option loading never blocks rendering or keyboard input.
- Cached options are loaded from disk before live refresh when available.
- Cache entries younger than 15 minutes avoid redundant live fetches during normal entry.
- Manual refresh bypasses the freshness TTL and attempts a live fetch.
- Live fetch success updates picker options and writes the repo-scoped cache.
- Live fetch failure keeps usable stale cache up to 7 days and surfaces a warning.
- Missing or unauthenticated `tea` disables affected repo-backed pickers with a clear reason.

## Verification Plan
- Run `just verify`.
- Manually run with a configured Gitea repo and confirm labels, assignees, and milestone options appear without blocking the UI.
- Manually run once with `tea` unavailable or unauthenticated and confirm the app remains usable with disabled repo-backed pickers.
- Manually confirm a second Generate PR entry uses cache immediately rather than waiting for live commands.

## Files Likely Touched
- `src/tea.rs`
- `src/repo.rs`
- `src/app.rs`
- `src/event.rs`
- `src/generate.rs`
- `src/command.rs`
- `src/config.rs`
- Potential new module such as `src/repo_options.rs`

## Risks
- `tea` JSON shapes may vary by version. Parser tests should use representative fields but tolerate extra fields.
- The collaborators endpoint may require permissions on some Gitea instances. Treat failures as disabled assignee options and keep labels/milestones usable.
- Cache invalidation must avoid silently accepting very old stale metadata as fresh.
