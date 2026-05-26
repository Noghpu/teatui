---
id: 0000b-2026-05-26-add09352-pr-exec-tea-and-landing-probes
created_at: 2026-05-26T08:01:24+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-26T08:27:17+02:00
---
# tea Wrapper and Async Landing Probes

## Goal
Add a `tea` command wrapper module and surface every interesting Landing
status field (`tea` auth, Ollama reachability, Gitea host) through async
probes that do not block the render loop.

## Context
Generate PR currently ends at `DraftReady`. The next slices implement
confirmation and execution, but neither has a typed `tea` wrapper to lean on,
and Landing today only shows tool presence (`tea --version` succeeded) and a
parsed remote URL. To make Landing answer "what's reachable and configured
right now?" without slowing it down, every probe must run asynchronously
through the existing `BackgroundEvent::Repo` envelope.

Pure jj has been chosen for the eventual branch/push step, so the `tea`
wrapper in this slice only needs the read-only commands required for status
detection. The actual `tea pr create` builder is intentionally out of scope
here and will land with the execution ticket so this file does not gain
unused code paths.

## Non-Goals
- Do not add `tea pr create`, `tea pr list`, or any other `tea` command not
  needed by Landing in this slice.
- Do not add a Logs screen or job tracking.
- Do not change `RepoDiscovery` into a streaming probe per field; one
  `discover` call returning a fully populated `RepoState` is fine.
- Do not introduce a new HTTP client; reuse `reqwest` already in `Cargo.toml`
  for the Ollama health check.
- Do not change Generate PR phases or flow.

## Design Decisions
- New `src/tea.rs` mirrors `src/jj.rs`: a `TeaClient` (or free functions)
  exposing `ExternalCommand` builders for `tea --version` and
  `tea login list`. No execution helpers; callers use `command::capture`.
- `tea login list` is parsed by line. Each non-empty data line is matched
  against the detected remote host (case-insensitive). Output format is
  treated as best-effort; unparseable output yields `TeaAuth::Unknown` with a
  short reason string rather than a hard error.
- `RepoState` grows two new fields:
  - `tea_auth: TeaAuth` with variants `Unknown`, `NotConfigured`,
    `Configured { host: String, user: Option<String> }`, `Error(String)`.
  - `ollama: OllamaStatus` with variants `Unknown`, `Reachable`,
    `Unreachable(String)`.
- `repo::discover` performs all probes concurrently using `tokio::join!` so
  the slowest probe sets the overall latency floor rather than the sum.
- Ollama reachability is a single `GET {base_url}` with a small timeout
  (e.g. 2 seconds). Any 2xx, 3xx, or 4xx response counts as reachable
  (server is up); only connect/timeout failures count as unreachable.
- Landing UI is the only UI surface that changes. Status bar stays the same.
- Existing helpers in `repo.rs` are reused; nothing in `app.rs` needs new
  actions because `App::refresh()` already triggers `repo::spawn_discovery`.

## Implementation Plan
- Create `src/tea.rs`. Add a `TeaClient::new(config)` constructor and two
  command builders: `version_command(cwd)` and `login_list_command(cwd)`.
- Add `#[cfg(test)]` argv tests confirming the program and args.
- Add `TeaAuth` and `OllamaStatus` enums to `src/repo.rs` (or a small new
  helper module if it keeps `repo.rs` readable; planner preference is to
  keep them in `repo.rs`). Add `label()` methods used by Landing rendering.
- Extend `RepoState` with `tea_auth` and `ollama` fields. Update
  `RepoState::bootstrap` to default both to `Unknown`.
- Add `parse_tea_login_list(stdout: &str, host: &str) -> TeaAuth`. Use a
  small, lenient parser: split by whitespace per line, look for a token
  equal to the host (or ending with the host), capture an adjacent user
  token when present. Reject obviously empty input.
- Add an Ollama reachability probe in `src/ollama.rs`: `pub async fn
  health_check(config: &Config) -> OllamaStatus` with a short timeout.
  Reuse the existing `reqwest::Client` configuration shape but with the
  short timeout local to this call.
- Update `repo::discover` to also run the tea-login-list capture and the
  Ollama probe concurrently. Wire `tea_auth` only when a remote host was
  detected; otherwise leave it as `Unknown` with a reason.
- Update `src/ui.rs` Landing pane to render the two new fields with the
  existing `status_line` helper or an equivalent style. Use `.green()` only
  for fully OK states; muted/`.red()` for missing/unreachable.
- Update `docs/design.md` Deferred Implementation Notes only if the chosen
  shape closes an open question; otherwise leave it.
- Run `just verify`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/repo.rs", "src/jj.rs", "src/ollama.rs", "src/ui.rs"],
  "likely_files": ["src/tea.rs", "src/repo.rs", "src/ollama.rs", "src/ui.rs", "src/main.rs", "Cargo.toml"],
  "verification_commands": ["just verify"],
  "review_focus": [
    "all probes run concurrently and do not block render",
    "tea_auth parsing is lenient and never panics on weird tea output",
    "Ollama reachability check uses a short timeout and does not regress startup",
    "Landing UI shows new fields with sensible empty/error states",
    "no unused tea command builders are added"
  ]
}
```

## Acceptance Criteria
- `repo::discover` returns a fully populated `RepoState` including
  `tea_auth` and `ollama` fields.
- Concurrent probes: total discovery latency is bounded by the slowest
  probe, not the sum of probes.
- `parse_tea_login_list` returns `Configured` when output contains the
  detected host, `NotConfigured` when output is non-empty but missing the
  host, and a sensible `Unknown`/`Error` variant for unparseable output.
- Ollama health check timeout is short and surfaces a readable error string
  on connect/timeout failure.
- Landing pane renders both new status lines. With no tea binary or no
  remote, the lines render as muted "(unknown)" or "(not configured)".
- `just verify` passes. Unit tests cover `parse_tea_login_list` and the
  argv of `tea version_command` and `login_list_command`.

## Verification Plan
Run `just verify`. Add unit tests for:
- `parse_tea_login_list` with: empty input, a typical `tea login list`
  block matching the host, a block missing the host, malformed lines.
- `tea version_command` and `login_list_command` argv and cwd.
Manual: open the app inside a jj workspace and confirm Landing shows the
new lines without a noticeable startup hitch.

## Files Likely Touched
- `src/tea.rs` (new)
- `src/main.rs`
- `src/repo.rs`
- `src/ollama.rs`
- `src/ui.rs`
- `Cargo.toml`
- `docs/design.md` (optional, only if a deferred note closes out)

## Risks
- `tea login list` output format may vary across `tea` versions; lenient
  parsing must avoid false `NotConfigured` results on valid but unfamiliar
  output.
- Ollama instances behind auth or proxies may return 401/403; treat any
  HTTP response as "reachable" so we do not flap on auth-only setups.
- Adding probes to discovery can extend Landing time-to-first-paint if any
  probe accidentally serializes; the concurrent-join requirement protects
  this but must be exercised by review.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: unknown
- completed_at: 2026-05-26T08:23:25+02:00
- state: implemented

Completed:
- Added `src/tea.rs` with a typed `TeaClient` wrapper and argv tests for `tea --version` and `tea login list`.
- Extended repo discovery with concurrent probes for jj, git, tea auth, Ollama reachability, workspace root, and Gitea remote data.
- Added `TeaAuth` and `OllamaStatus` to `RepoState`, plus lenient `parse_tea_login_list` tests.
- Updated Landing rendering to show the new status lines and surface unknown/not configured/error states.

Deviations:
- Kept `TeaAuth`/`OllamaStatus` in `repo.rs` and returned the repo-owned Ollama status from `ollama::health_check`.
- Used concise landing details rather than adding extra UI screens or job tracking.

Verification:
- `just verify`

Files changed:
- `src/tea.rs`
- `src/main.rs`
- `src/repo.rs`
- `src/ollama.rs`
- `src/ui.rs`
- `src/prompt.rs`

Residual risk:
- `tea login list` output remains best-effort parsed and may need adjustment if tea changes its table layout.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5 medium
- reviewed_at: 2026-05-26T08:27:17+02:00
- state: reviewed

Facts:
- Reviewed the implemented tea wrapper, repo discovery, Ollama health check, Landing rendering, and related tests against the ticket and docs/design.md.
- Confirmed discovery uses tokio::join! for tool, workspace, remote, tea auth, and Ollama probes so discovery latency is bounded by the slowest probe.
- Confirmed the tea wrapper only exposes read-only version and login list command builders.
- Ran `just verify` successfully before and after review changes.

Improvements made:
- Tightened tea login host matching to normalize URLs, ports, case, and user prefixes consistently.
- Added parser coverage for URL-form tea login rows, portless tea hosts against ported remotes, and partial-host false positives.

Residual risk:
- `tea login list` remains best-effort parsed because the command output format may vary by tea version.
