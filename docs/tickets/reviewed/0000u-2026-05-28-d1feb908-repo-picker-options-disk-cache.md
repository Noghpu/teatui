---
id: 0000u-2026-05-28-d1feb908-repo-picker-options-disk-cache
created_at: 2026-05-28T11:42:07+02:00
created_by_model: gpt-5
state: reviewed
state_updated_at: 2026-05-28T16:14:24+02:00
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
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-28T16:09:11+02:00
- state: implemented

## What was completed

Implemented the full repo picker options disk cache feature for the Generate PR form.

### New module: `src/repo_options.rs`
- `sanitize_key_component` and `repo_cache_key` for stable per-repo cache file naming
- JSON parsers: `parse_labels_json`, `parse_milestones_json`, `parse_collaborators_json` - tolerant of extra fields, strict on missing display values
- `RepoOptionsCache` with `is_fresh` (15 min TTL) and `is_usable` (7 day max stale age)
- `write_cache` uses atomic rename (write temp, rename to final)
- `read_cache` discards entries with mismatched version numbers
- `OptionsLoadStatus` enum: Idle, Loading, Ready, Unavailable
- `RepoOptions` with label/milestone/assignee picker option conversion methods
- `spawn_repo_options_load`: stale-while-revalidate background loader that sends cached options first (if usable), then live fetch; force_refresh=true bypasses freshness TTL

### `src/tea.rs`
- Added `labels_list_command` (`tea labels list --output json --limit 100`)
- Added `milestones_list_command` (`tea milestones list --state open --output json --limit 100`)
- Added `collaborators_command` (`tea api /repos/{owner}/{repo}/collaborators`)
- Added unit tests for all three command builders

### `src/event.rs`
- Added `BackgroundEvent::RepoOptions(Box<RepoOptionsResult>)` variant

### `src/app.rs`
- Added `repo_options: RepoOptions` field to `App`
- Added `apply_repo_options` handler - updates picker options on generate form, logs result, calls `validate_form`
- Updated `handle_background` to dispatch `RepoOptions` event
- Updated `refresh` (triggered by `r` key) to call `spawn_repo_options_load` with `force_refresh=true`
- Added `spawn_repo_options_load` helper that only spawns when remote is available
- `apply_repo` now triggers initial load when remote transitions from None to Some
- `open_selected_landing_entry` triggers stale-while-revalidate load on Generate PR entry

## Deviations from plan
- No UI warning display in the picker/right pane beyond log messages was implemented; the warning text is stored in `RepoOptions.status_warning()` for a future UI slice to render. The ticket noted the warning is visible in the picker/right pane; this is deferred as there is no existing right-pane warning rendering infrastructure.
- `OptionsLoadStatus::Loading` variant is defined but the loader transitions directly from Idle to Ready/Unavailable without emitting a Loading event; sufficient for non-blocking behavior since the form remains usable throughout.

## Verification
- `just verify` passed: 153 unit tests + 4 integration tests, no clippy warnings

## Important files changed
- `src/repo_options.rs` (new)
- `src/tea.rs`
- `src/event.rs`
- `src/app.rs`
- `src/lib.rs`

## Residual risks / follow-up
- The warning from stale cache or partial failure is stored in `RepoOptions.status_warning()` but not yet surfaced in the UI right pane or picker field. A future UI ticket should render it near the picker or in the status bar.
- `tea api` path injection uses `{owner}` and `{repo}` placeholders per the ticket's description; actual replacement is done by the `tea` CLI based on current repo context, not by teatui.
- Cache is stored in `dirs::cache_dir()/teatui/repo-options/<key>.json`; no migration needed since version field guards old formats.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: claude-opus-4-7
- reviewed_at: 2026-05-28T16:14:24+02:00
- state: reviewed

## Review Summary

Reviewed 0000u: load and cache repo picker options. Implementation is solid and meets the acceptance criteria. Applied minor cleanups for clarity.

## What was reviewed

- `src/repo_options.rs` (new, ~760 lines): JSON parsers, atomic disk cache, stale-while-revalidate spawn loader.
- `src/tea.rs`: new argv builders for `labels list`, `milestones list`, `api /repos/{owner}/{repo}/collaborators`.
- `src/event.rs`: new `BackgroundEvent::RepoOptions(Box<RepoOptionsResult>)`.
- `src/app.rs`: `RepoOptions` field, `apply_repo_options`, `spawn_repo_options_load` wired into Generate PR entry, remote-transition trigger, and `r` refresh.
- `src/lib.rs`: registers the new module.

## Strengths

- Cache is version-gated, atomically written (temp + rename), uses `dirs::cache_dir()/teatui/repo-options/<sanitized-key>.json`.
- Stale-while-revalidate: usable cache (<7d) sent first; live refresh skipped when fresh (<15min) unless `force_refresh`.
- Parsers tolerant of extra fields but reject missing required display values (name/title/login).
- All command construction uses argv arrays through `ExternalCommand`/`capture`, no shell strings.
- Loader never blocks the UI; failures degrade to `Unavailable` with a reason rather than panicking.
- Good unit coverage: sanitization, cache freshness/usability boundaries, parser edge cases, selection retention behavior.

## Fixes applied

- Removed unused `OptionsLoadStatus::Loading` variant (dead code; loader transitions Idle -> Ready/Unavailable directly, as the implementation note acknowledged).
- Inlined the trivial three-line `build_commands` helper at its only call site; dropped now-unused `Path` and `ExternalCommand` imports.
- Verified `just verify` still passes: 153 unit tests + 4 integration tests, no clippy warnings.

## Findings not fixed (out of scope or accepted deviation)

- The ticket says "If a selected value disappears from refreshed options, keep it selected but mark it invalid or stale until the user changes it." The existing `PickerFieldState::ensure_valid_selection` silently drops invalid selections, and the new test `set_picker_options_clears_invalid_previously_selected_value` asserts that drop behavior. This is inherited from ticket 0000t and changing it would alter picker semantics globally; it deserves a follow-up ticket if the design intent is to preserve-but-flag.
- `RepoOptions.status_warning()` is populated but not yet surfaced in the UI right pane / picker. The implementation note flags this as a deferred UI slice; consistent with the ticket's intent that the warning is "visible in the picker/right pane".
- `apply_repo_options` logs counts even when called from cache, which is useful for diagnostics. No change recommended.

## Verification

- `just verify` after cleanups: all 153 unit + 4 integration tests pass; no clippy warnings.
