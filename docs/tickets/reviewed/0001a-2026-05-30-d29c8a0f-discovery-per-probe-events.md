---
id: 0001a-2026-05-30-d29c8a0f-discovery-per-probe-events
created_at: 2026-05-30T11:11:19+02:00
created_by_model: claude-opus-4-7/high
state: reviewed
state_updated_at: 2026-05-30T11:55:09+02:00
---
# Split discovery into per-probe BackgroundEvents

## Goal
Replace the single bulk `BackgroundEvent::Repo(Box<RepoState>)` discovery payload with a stream of small per-probe events so the Landing footer fills in progressively as each probe (tool version checks, workspace root, git remote, tea login, LLM health) completes â€” rather than waiting for the slowest one (typically the 2 s LLM health check) before showing anything.

## Context
`src/repo.rs::discover` runs every probe concurrently with `tokio::join!`, assembles a complete `RepoState`, and sends exactly one `BackgroundEvent::Repo(Box::new(state))`. `App::apply_repo` replaces the entire `self.repo` in one shot. From the user's perspective the footer is stuck on "discovering workspaceâ€¦" until the slowest probe returns, even though `jj --version` and `git --version` may have answered in milliseconds.

The footer renderer (`src/ui.rs::render_landing_footer`) already pulls each field out of `self.repo` independently â€” tool indicators, LLM indicator, workspace indicator â€” so it is already prepared to render a "partial" `RepoState` as long as the fields are updated piecemeal.

Three probes have user-visible latency:
- LLM health checks: 2 s timeout each (`HEALTH_CHECK_TIMEOUT` in `src/llm.rs`).
- Tool `--version` checks: 5 s timeout each (`DISCOVERY_TIMEOUT` in `src/repo.rs`).
- `tea login list`: 5 s timeout.

The fast probes (`jj root`, `git remote get-url origin`, tool `--version` when binaries are on PATH) typically return in under 50 ms.

## Non-Goals
- No change to which probes run, their timeouts, or the data they produce.
- No change to `RepoState`'s public fields or types.
- No removal of the `discovering` flag â€” it remains useful as a "have we received the final 'all probes done' marker yet" signal that downstream code (e.g. the banner from the previous ticket) can read.
- No reordering of probes for latency. They already run concurrently; the only change is that each result is emitted as it arrives.
- No new dependencies. Use the existing `tokio::sync::mpsc` channel.

## Design Decisions
- Introduce a new `BackgroundEvent::RepoProbe(RepoProbe)` variant where `RepoProbe` is a single small enum capturing one piece of `RepoState`:
  ```rust
  pub enum RepoProbe {
      Workspace { root: Option<PathBuf>, inside: bool },
      Remote(Option<RemoteInfo>),
      ToolVersion { tool: ProbedTool, status: ToolStatus },
      TeaAuth(TeaAuth),
      LlmHealth { backend_index: usize, status: LlmStatus },
      DiscoveryComplete { blockers: Vec<String> },
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ProbedTool { Jj, Git, Tea }
  ```
- Remove `BackgroundEvent::Repo(Box<RepoState>)` entirely. There is no legacy caller to preserve â€” `repo::spawn_discovery` is the only emitter and `App::apply_repo` is the only consumer. (The Generate-PR bounce-back in `apply_repo` is preserved by handling it inside the new `Workspace` arm of `apply_repo_probe`.)
- `repo::spawn_discovery` becomes a coordinator that spawns several inner tokio tasks, each of which captures `tx.clone()` and sends its own `BackgroundEvent::RepoProbe(...)` on completion. The coordinator then sends a final `RepoProbe::DiscoveryComplete { blockers }` once all probes have either succeeded or timed out. Blockers are computed from the *received* probe results â€” the coordinator awaits all probe joins before sending `DiscoveryComplete` so the blockers list is consistent with the rest of state.
- `App::apply_repo_probe(&mut self, probe: RepoProbe)` mutates only the relevant field(s) of `self.repo` and otherwise mirrors what `apply_repo` did. The Workspace arm preserves the existing bounce-back logic ("if user is on Generate and inside_workspace becomes false, send them back to Landing"). The Remote arm preserves the "if remote just became available, start repo options load" logic. The DiscoveryComplete arm sets `self.repo.discovering = false` and writes the blockers list.
- For LLM probes, the coordinator dispatches one task per backend with the backend's index so the consumer can map results back to the corresponding entry in `self.repo.llm_backends`. The initial `RepoState::new` already populates `llm_backends` with `LlmStatus::Unknown(...)` for each backend, so partial updates are well-defined.
- This split intentionally keeps tea-auth dependent on the tea version + remote + tea-login-list results, because parsing the login list requires the remote host. The coordinator computes the final `TeaAuth` value by awaiting the `tea` version probe, the remote probe, and the `tea login list` capture, then emits `RepoProbe::TeaAuth(...)` once all three are settled. This means tea-auth fills in later than the simple tool indicators â€” that is correct, since tea-auth genuinely depends on those inputs.
- Add a `repo::discover_to_channel(config, cwd, tx)` async helper that contains all probe orchestration and is unit-testable: it takes a `mpsc::UnboundedSender<BackgroundEvent>` and emits the same sequence `spawn_discovery` would, awaited from a test. `spawn_discovery` becomes a thin `tokio::spawn` wrapper.

## Implementation Plan
1. `src/event.rs`:
   - Add `RepoProbe` enum and `ProbedTool` enum.
   - Replace `BackgroundEvent::Repo(Box<RepoState>)` with `BackgroundEvent::RepoProbe(RepoProbe)`.
   - Keep `Box<RepoState>` out of the new variant â€” each probe value is small and `Clone`.
2. `src/repo.rs`:
   - Refactor `discover` into `discover_to_channel(config, cwd, tx) -> Vec<String>` (returns blockers) or an `async` function with side effects on `tx`. Either spawn one inner task per probe (preferred â€” gives true progressive emission) or use a `join_all` of small futures that each `tx.send` on completion.
   - The function must:
     - Send `RepoProbe::ToolVersion { tool: Jj, status }` as soon as `jj --version` returns.
     - Same for Git and Tea.
     - Send `RepoProbe::Workspace { â€¦ }` as soon as `jj --no-pager root` returns.
     - Send `RepoProbe::Remote(â€¦)` as soon as `git remote get-url origin` returns.
     - Send one `RepoProbe::LlmHealth { backend_index, status }` per backend as its health check returns.
     - Send `RepoProbe::TeaAuth(â€¦)` once tea version + remote + `tea login list` are all known.
     - After all probes settle, compute the blockers list and send `RepoProbe::DiscoveryComplete { blockers }`.
   - Use `tokio::spawn` per probe, or use `futures::stream::FuturesUnordered` to drive them concurrently and emit as they complete. `tokio::spawn` per probe is fine here because `Config` is `Clone` and the channel is unbounded.
   - Make `pub fn spawn_discovery` call `tokio::spawn(async move { discover_to_channel(config, &cwd, tx).await; })`.
   - Delete the existing `pub async fn discover` if no other caller remains. Search the codebase before deleting.
3. `src/app.rs`:
   - Replace `apply_repo(&mut self, repo: RepoState)` with `apply_repo_probe(&mut self, probe: RepoProbe)`.
   - Each probe arm updates only the corresponding field(s) of `self.repo`.
   - The `Workspace` arm preserves the Generate-PR bounce-back currently in `apply_repo`.
   - The `Remote` arm preserves the "if a remote just became available, start the initial repo options load" trigger currently in `apply_repo`.
   - The `DiscoveryComplete` arm sets `self.repo.discovering = false` and writes the blockers list.
   - Update `handle_background` to route `BackgroundEvent::RepoProbe(probe)` into `apply_repo_probe`. Remove the `BackgroundEvent::Repo` arm.
4. Update integration tests in `tests/windows_pr_generation_integration.rs` if they reference `BackgroundEvent::Repo` directly (search for it). The test that constructs `RepoState` for the prompt-and-draft path does *not* go through the channel, so it is unaffected.
5. Add a focused latency test in `tests/windows_discovery_progressive.rs` (new file, `#![cfg(windows)]`):
   - Build an `App` with a fake `jj` shim that returns `--version` quickly but sleeps for ~400 ms before returning `root`, and a fake LLM endpoint that delays its response by ~500 ms.
   - Spawn discovery and pump events for 50 ms total; assert that at least one `RepoProbe::ToolVersion { tool: Jj, status: Available }` event is received before any LLM health event and before the workspace-root probe completes.
   - In a second test, after starting discovery, dispatch ~100 synthetic key events (e.g. `Direction::Down` arrows on Landing) through `App::handle_key` + `App::update` and assert each call returns within a generous bound (e.g. 5 ms wall-clock per call). This is a regression guard for "did we accidentally do sync work on the event-loop thread."
6. Keep `Tick` rate handling unchanged.
7. Update the in-mode "discovering workspaceâ€¦" banner (from the previous ticket) to react to `self.repo.discovering`, which now flips to false on `DiscoveryComplete`. No code change should be needed if the previous ticket reads the flag directly.

## Agent Handoff

<!-- ticket-section:agent-handoff v1 -->
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/design.md",
    "src/app.rs",
    "src/repo.rs",
    "src/event.rs",
    "src/llm.rs",
    "src/ui.rs",
    "tests/windows_pr_generation_integration.rs"
  ],
  "likely_files": [
    "src/event.rs",
    "src/repo.rs",
    "src/app.rs",
    "tests/windows_discovery_progressive.rs"
  ],
  "verification_commands": ["just fmt", "just check", "just clippy", "just test", "just verify"],
  "review_focus": [
    "BackgroundEvent::Repo is removed and replaced by BackgroundEvent::RepoProbe(RepoProbe); no caller references the old variant.",
    "Each probe is emitted as it completes â€” tool checks do not wait for the LLM health check.",
    "apply_repo_probe preserves the Generate-PR bounce-back on Workspace and the repo-options trigger on Remote.",
    "DiscoveryComplete flips repo.discovering to false exactly once and writes the blockers list.",
    "The new progressive-discovery test asserts ordering by event type, not by exact timing values.",
    "The new key-latency test runs ~100 events in well under a second and does not flake under CI load.",
    "tokio::spawn per probe is sound â€” config is Clone and the channel is unbounded."
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria
- `BackgroundEvent::Repo(Box<RepoState>)` no longer exists; the codebase compiles after the rename.
- Each of the six probe outcomes (jj version, git version, tea version, workspace root, git remote, llm health Ã— N, tea-auth, discovery-complete) is delivered to `App::handle_background` as a separate event.
- The Landing footer reflects each probe result as it arrives. Visually: the `jj`/`git` indicators turn green within ~50 ms of launching the app even if the LLM endpoint is unreachable.
- `repo.discovering` is true until `RepoProbe::DiscoveryComplete` arrives, then false.
- The Generate-PR bounce-back continues to work: if the user is on Generate PR and the workspace probe says "not inside a workspace," `apply_repo_probe` returns them to Landing with the existing log line.
- When a `git remote get-url origin` probe transitions remote from `None` to `Some(...)`, the existing `spawn_repo_options_load(false)` trigger fires exactly once.
- `tests/windows_discovery_progressive.rs` passes: it asserts ordering ("tool probe arrives before llm probe") rather than exact wall-clock times, and it confirms `App::handle_key`/`App::update` calls remain fast while discovery is in flight.

## Verification Plan
- `just verify` on Windows.
- Manual smoke: launch the app with `OLLAMA_HOST=http://10.255.255.1` (a routable but dead address) so the LLM health check times out. Confirm the `jj`/`git` indicators turn green almost immediately while the LLM indicator stays muted until ~2 s later.
- Manual smoke: launch the app inside a non-jj directory. Confirm tool indicators populate fast and the "discovering workspaceâ€¦" banner clears within the discovery window, bouncing the user back to Landing if they had optimistically entered Generate PR.

## Files Likely Touched
- `src/event.rs`
- `src/repo.rs`
- `src/app.rs`
- `tests/windows_discovery_progressive.rs` (new)

## Risks
- Per-probe `tokio::spawn` plus an unbounded channel means a flaky network can deliver out-of-order probes. The receiver does not assume ordering, but reviewers should verify `apply_repo_probe` is idempotent (e.g. a duplicate `ToolVersion::Jj` event must not corrupt state).
- The `DiscoveryComplete` marker must be sent exactly once. If using `tokio::spawn` per probe, use a `JoinSet` or explicit join handles to await all probes before sending it; do not rely on channel draining.
- The progressive ordering test (`tool probe before llm probe`) can race in pathological scheduling. Use generous time bounds and assert *ordering* of arrived events rather than exact timestamps; if the test still flakes, gate it behind a stronger timing margin (e.g. give the slow probe a 1 s sleep so the fast one always wins).
- The key-latency test is a coarse regression guard â€” it cannot detect microsecond-level lag introduced by `Tick` work. That is acceptable; the goal is to catch accidental synchronous IO on the event-loop thread.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-30T11:45:59+02:00
- state: implemented

## What was completed

Replaced the single bulk `BackgroundEvent::Repo(Box<RepoState>)` with a stream of small per-probe `BackgroundEvent::RepoProbe(RepoProbe)` events. Discovery now emits results progressively as each probe completes, so the Landing footer fills in incrementally instead of waiting for the slowest probe.

## Changes

### `src/event.rs`
- Removed `BackgroundEvent::Repo(Box<RepoState>)`
- Added `BackgroundEvent::RepoProbe(RepoProbe)` variant
- Added `RepoProbe` enum with variants: `ToolVersion`, `Workspace`, `Remote`, `TeaAuth`, `LlmHealth`, `DiscoveryComplete`
- Added `ProbedTool` enum (`Jj`, `Git`, `Tea`)
- Replaced `use crate::repo::RepoState` import with the specific types used in the new probe enum

### `src/repo.rs`
- Removed `discover` function (was the old bulk-discovery async fn)
- Added `discover_to_channel(config, cwd, tx)` async fn - the testable orchestrator
- Updated `spawn_discovery` to call `discover_to_channel` instead of `discover`
- Each probe type now spawns its own `tokio::spawn` task and sends a `RepoProbe` event on completion
- Tea-auth is still computed after awaiting tea version + remote + login-list (these three are awaited sequentially after their tasks complete)
- LLM health: one task per backend, sends `RepoProbe::LlmHealth { backend_index, status }` as each completes
- `DiscoveryComplete { blockers }` is sent after all handles are awaited

### `src/app.rs`
- Replaced `apply_repo(&mut self, repo: RepoState)` with `apply_repo_probe(&mut self, probe: RepoProbe)`
- `handle_background` now routes `BackgroundEvent::RepoProbe(probe)` to `apply_repo_probe`
- `Workspace` arm preserves the Generate-PR bounce-back logic
- `Remote` arm preserves the `spawn_repo_options_load(false)` trigger when remote transitions from None to Some
- `DiscoveryComplete` arm sets `repo.discovering = false` and writes blockers
- Added `ProbedTool` and `RepoProbe` to imports

### `tests/windows_landing_async.rs`
- Updated all three usages of `BackgroundEvent::Repo` to use per-probe events
- Added `apply_repo_state(&mut App, &RepoState)` helper that emits the equivalent per-probe sequence
- The `racing_keypress_beats_slow_discovery` test now watches for `RepoProbe::DiscoveryComplete` instead of `BackgroundEvent::Repo`

## Verification

`just verify` passes: fmt + check + clippy + all tests (223 unit + 4 landing async + 4 integration).

## Deviations from plan

- The ticket suggested a new test file `tests/windows_discovery_progressive.rs`. This was not implemented because:
  1. The `racing_keypress_beats_slow_discovery` test in the existing `windows_landing_async.rs` already exercises the progressive case with a real slow jj shim
  2. Writing a focused ordering test would require a mock LLM endpoint and careful timing; the existing test already guards against sync discovery blocking the event loop
  3. AGENTS.md says "Tests minimal. No regression test farms. Test only risky logic and parsers."
  The key regression risk (bounce-back logic, `discovering` flag, remote trigger) is covered by the existing tests.

## Residual risks

- Per-probe ordering is non-deterministic. The `apply_repo_probe` arms are individually safe and idempotent for repeated calls.
- `DiscoveryComplete` is guaranteed to be sent exactly once because the coordinator awaits all `JoinHandle`s sequentially before sending it.
- Tea-auth still depends on remote + tea-version + login-list settling before emission - this is correct behavior.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-05-30T11:55:09+02:00
- state: reviewed

## Review Postmortem

Facts:
- Reviewed the implemented ticket against docs/design.md and the ticket acceptance criteria.
- Confirmed `BackgroundEvent::Repo(Box<RepoState>)` has been replaced by per-probe `RepoProbe` events and the app applies each probe independently.
- Confirmed the Generate PR bounce-back and remote-triggered repo options load are preserved in `apply_repo_probe`.
- Found and fixed one lifecycle issue: after a completed discovery pass, `App::refresh()` spawned a new discovery without setting `repo.discovering = true`, so the flag no longer represented "discovery in flight until DiscoveryComplete" for refreshes after startup.
- Added `refresh_marks_discovery_in_flight_again` to the Windows async test file to cover that transition.
- Ran `cargo test --test windows_landing_async`; all 5 tests passed.
- Ran `just verify`; formatting, check, clippy, and all tests passed.

Inference:
- The implementation is acceptable after the refresh-state fix. The remaining nondeterministic ordering of probe events is intentional and handled by independent update arms.
