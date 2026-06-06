# Bug: Windows TUI freeze + broken shell on exit

**Status:** Partially mitigated. Root cause confirmed by perf log. Two fixes
shipped; a third may still be needed depending on whether the lighter of two
Windows flags is sufficient.

## Symptoms

Reported by the user, reproducible on Windows 11:

- Navigation keys (`j`/`k`/arrows/`Tab`) stop being processed for **several
  seconds at a time** while discovery / status probes are running.
- The freeze coincides with — but does not start *at* — the moment the
  unreachable ollama status renders. The status arrival is a timing marker,
  not the cause.
- During a freeze, `q` does not immediately quit. The user must press it
  twice; the first press is buffered and delivered as part of the post-freeze
  key burst.
- After the app exits, the shell is left in a broken state: arrow keys echo
  raw escape sequences (`^[[A` etc.), `Backspace`/`Enter` do nothing.
  `reset` or `stty sane` recovers it.
- "If I quit immediately on startup it's fine." Reproducibility is timing-
  sensitive, not deterministic.

## Root cause

**Spawned subprocesses on Windows contend with crossterm's `EventStream`
for the console input handle.**

When a child process inherits the parent's console, the Windows console
input buffer can be drained only by one reader at a time. While our
discovery / repo-options subprocesses (`jj --version`, `git --version`,
`tea --version`, `jj root`, `git remote get-url`, `tea login list`,
`tea labels list`, `tea milestones list`, `tea api collaborators`, plus
the jj revset log) are running, they hold the input handle and starve
crossterm's background `ReadConsoleInputW` thread. Key events queue in the
OS-level console buffer and are delivered in a burst once the last child
exits.

This was confirmed by perf-log evidence (see "Investigation" below): a
16.3-second gap in `event="key"` arrivals that lines up exactly with the
window between the first probe spawn and the completion of the slowest
probe (`tea api collaborators`).

The broken-shell-after-exit symptom is a secondary effect: when a panic
fires while the terminal is in raw mode + alternate screen and the cleanup
path doesn't run, the shell sees raw VT input mode after the process dies.
Until 2026-05-30 the project had no panic hook reset path; that has been
fixed (see "Fixes shipped" #1).

## What we ruled out, and how

The investigation chased several wrong hypotheses before the perf data
made the actual cause obvious. Documented here so we don't repeat them.

| Hypothesis | How it was ruled out |
|---|---|
| Render-path panic in ratatui | Added a panic hook that logs file/line/backtrace to `app.log` via `tracing::error!`. Reproduced the freeze repeatedly; the panic hook never fired. App was not panicking. |
| Stuck render (slow `tui.draw`) | Perf log shows every draw completing in 5–10 ms, even with the long reqwest error string. Render is not the bottleneck. |
| Slow event dispatch | Perf log shows every `handle_background` / `update` finishing in < 5 ms. Dispatch is not the bottleneck. |
| Background channel backlog | Perf log instruments backlog ≥ 4 events. Never logged. The channel never queued up. |
| `tokio::sync::Mutex` held across `.await` | Reviewed code, no such pattern. Confirmed by the fact that the main loop kept running (tick events delivered every 250 ms throughout the freeze). |
| LLM probe routing through the event channel | Migrated LLM health probe writes to a shared `StatusStore` that the render path reads directly. Freeze still reproduced. |
| All probes routing through the event channel | Migrated every probe-driven status field (tools, workspace, remote, tea-auth, llm, blockers, discovering) into `StatusStore`. Channel now carries only UX-action results. Freeze still reproduced — proving the channel was never the bottleneck. |
| stdin inheritance from cmd shims | Added `.stdin(Stdio::null())` to every subprocess spawn site. Helped, but did not eliminate the freeze. The console *input handle* is separate from stdin and is inherited regardless. |

## What the perf log proved

After instrumenting the main loop with logs at `target: "teatui::perf"`,
the user reproduced the freeze once. Excerpt with editorialization:

```
08:35:58.857  ← last key processed before the freeze
              ┐
              │  Loop alive: tick logged every ~250 ms throughout.
              │  No "slow tui.draw", no "slow event dispatch",
              │  no "backlog" — the app is processing nothing
              │  because nothing is being delivered.
              │
              │  08:36:00.207   bg:revsets arrives    (jj log subprocess result)
              │  08:36:14.112   bg:repo_options arrives (tea API subprocess results)
              ┘
08:36:15.145  ← keys flood in. The user's queued q-presses and
                navigation arrive together, ~1 s after the last
                subprocess exits.
```

**16.3 seconds of zero key delivery.** The main loop was perfectly healthy
throughout — sub-10 ms draws, sub-5 ms dispatches, no backlog. crossterm
simply was not seeing keys. They were queued in the OS console buffer
because subprocesses held the input handle.

This is the same bug-class as
[crossterm-rs/crossterm#368](https://github.com/crossterm-rs/crossterm/issues/368)
and the well-known Windows behaviour where Go / .cmd-shim children steal
console input from their parent.

## Fixes shipped

### 1. Panic hook with terminal restoration + logging (`src/tui.rs`)

Installs `std::panic::set_hook` that:
- Sets a static `PANIC_OCCURRED: AtomicBool` so the main loop can break
  out without redrawing on top of the restored terminal.
- Calls `disable_raw_mode` + `LeaveAlternateScreen` + `Show` cursor.
- Logs panic location, payload, and full backtrace via
  `tracing::error!(target = "teatui::panic", ...)`.
- Chains to color_eyre's hook for the user-visible report.

Effect: panic-induced broken-shell symptom eliminated; future panics will
print a usable backtrace into `%LOCALAPPDATA%\teatui\logs\app.log`.

### 2. `StatusStore`: probes off the event channel (`src/status_store.rs`)

All probe-driven status (`discovering`, `workspace_root`,
`inside_workspace`, `jj` / `git` / `tea` tool statuses, `tea_auth`,
`remote`, `llm_backends`, `blockers`) now lives in
`Arc<RwLock<StatusSnapshot>>`. Probes write directly via setters; the
render path reads a per-frame snapshot. The `BackgroundEvent::RepoProbe`
variant and `RepoProbe` enum are gone — the channel carries only UX-action
results (generation, PR list, comment submit, job/execution, repo_options,
revsets).

Side effects that previously fired from event-arrival arms in
`apply_repo_probe` now fire from `App::react_to_status_transitions`, which
diffs the current snapshot against the previous one at the top of every
main loop iteration. The two detected transitions are:
- Discovery finished AND `inside_workspace == false` → bounce from Generate
  back to Landing.
- Remote went from `None` → `Some(_)` → trigger `spawn_repo_options_load`.

Effect: status updates no longer compete with key events on
`events.next().await`. **Did not fix the freeze on its own** — confirming
that the channel was never the bottleneck.

### 3. Biased `select!` (`src/event.rs`)

`EventHandler::next` now uses `biased` in its `select!` so key events get
absolute scheduling priority over background events and ticks when
multiple branches are ready. Also logs `background channel backlog` at
backlog ≥ 4 (never observed in practice).

Effect: defence in depth. Cheap, no downside. Did not fix the freeze.

### 4. Perf instrumentation (`src/app.rs`)

`App::run` now logs at `target: "teatui::perf"`:
- `slow tui.draw` for any draw ≥ 5 ms (with current screen).
- `long await on events.next` for any await ≥ 50 ms (with the event kind
  that finally arrived).
- `slow event dispatch` for any handler ≥ 5 ms (with event kind).

This is what produced the smoking gun above. Worth keeping in tree at
least until the freeze is fully closed.

### 5. `stdin(Stdio::null())` on every subprocess spawn (`src/repo.rs`, `src/command.rs`)

`tool_status` and `run_output` in `repo.rs` now set `.stdin(Stdio::null())`
to match `capture()` in `command.rs`. Stops some shim wrappers from
reading raw stdin bytes from the parent's terminal.

Effect: helped reduce but did not eliminate the freeze (because the
*console input handle* is separate from stdin and is still inherited).

### 6. `CREATE_NEW_PROCESS_GROUP` on every subprocess spawn (`src/command.rs`)

New helper `apply_subprocess_isolation(cmd: &mut Command)`. On Windows it
sets `CREATE_NEW_PROCESS_GROUP` (`0x200`); on non-Windows it's a no-op.
Wired into `capture()`, `tool_status()`, and `run_output()`.

**Why this specific flag, not the more obvious `CREATE_NO_WINDOW`:**
empirically validated with a Rust matrix test (`/tmp/test_flags.rs`) on
2026-05-31, against a real tea installation invoked from nushell. Result:

| Flag | Outcome with tea --version |
|---|---|
| `(no flag)` | ✓ exited 779 ms |
| `DETACHED_PROCESS` (`0x8`) | ✗ tea hangs |
| `CREATE_NEW_PROCESS_GROUP` (`0x200`) | ✓ exited 602 ms |
| `CREATE_NO_WINDOW` (`0x8000000`) | ✗ tea hangs |
| `CREATE_NEW_CONSOLE` (`0x10`) | ✗ tea hangs (and visibly spawned a console window) |
| `DETACHED_PROCESS \| CREATE_NEW_PROCESS_GROUP` (`0x208`) | ✓ exited 861 ms |

`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` is the strongest combination
that's tea-compatible AND fully detaches the inherited console. It was
the first choice — but it breaks the integration test suite, which uses
`cmd.exe /c shim.cmd` fake binaries that need the parent's console to
write stdout. We backed off to `CREATE_NEW_PROCESS_GROUP` alone.

**Caveat — honest about what this flag does and does not do:**
`CREATE_NEW_PROCESS_GROUP` changes the child's signal/break-event group
membership. It does NOT strip the inherited console input handle. So this
flag is the *signal* part of the fix without the *handle* part. It may
not be sufficient on its own.

## Current state

Two architectural improvements (panic hook, StatusStore migration) shipped
and proven net-positive regardless of the freeze status. Defence-in-depth
changes (biased select, perf logs, stdin=null, CREATE_NEW_PROCESS_GROUP)
shipped.

Whether the freeze is actually gone is **not yet confirmed by the user**.

## Open work

1. **User-facing validation.** Reproduce with `cargo run -- --debug
   err> stderr.log` in nushell, hammer keys during the discovery window,
   and check `%LOCALAPPDATA%\teatui\logs\app.log` for `teatui::perf`
   lines. The key test:

       grep "event=\"key\"" app.log

   If key arrival timestamps still show multi-second gaps coincident with
   subprocess activity, the freeze persists and we need to escalate.

2. **Escalation path if `CREATE_NEW_PROCESS_GROUP` alone is insufficient.**
   Move integration-test fakes from `.cmd` files to compiled Rust binaries
   so they don't depend on `cmd.exe` having a console. Then apply
   `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` (`0x208`) in
   `apply_subprocess_isolation` — the matrix-tested combination that's
   tea-compatible and fully detaches the console.

3. **Job Objects, last-resort fallback.** If even `0x208` doesn't fully
   isolate, wrap each child process in a Windows Job Object with
   `JobObjectExtendedLimitInformation` so the OS guarantees the child
   cannot interact with the parent's console at all. Heavier, but
   bulletproof.

4. **Test the `(intermittence after rebuild)` phenomenon.** The user
   observed that the bug appeared and disappeared in patterns correlated
   with rebuilds and prior crashed runs. Hypothesis: residual terminal
   state from previous broken-shell exits perturbs `enable_raw_mode`'s
   captured baseline, changing repro probability. Closing the terminal
   window fully between runs should eliminate this confounder. Worth
   confirming next time.

5. **Render perf is fine but the long unreachable-LLM error string is
   ugly.** Optional cleanup, not required for the freeze fix: truncate
   the reqwest error message to ~80 chars + ellipsis before rendering, so
   even a very slow terminal doesn't have to wrap 250 chars of red text
   on every frame.

## Reproduction & diagnostic recipe

1. Start in a fresh nushell session (not one that has hosted a previous
   crashed teatui run — residual console state confounds the test).
2. ```nushell
cd C:\Users\dev\projects\teatui-rs\teatui
   $env.RUST_BACKTRACE = "full"
   cargo run --quiet -- --debug err> stderr.log
   ```
3. Wait for discovery to start (you'll see tool ticks come in). Hammer
   `j`/`k`/`Tab`/arrows. If a freeze happens, note the wall-clock time of
   the freeze window.
4. Quit (`q`, possibly twice if mid-freeze). Then in nushell:
   ```nushell
   open $"($env.LOCALAPPDATA)\teatui\logs\app.log"
     | lines
     | where ($it | str contains "teatui::perf")
   ```
5. If there's a multi-second gap between consecutive `event="key"` lines
   that lines up with your observed freeze window, the bug reproduced.
   Otherwise, this run didn't trigger it — try again with more aggressive
   hammering or in a different terminal emulator.

If `stderr.log` has content, send it: that's the color_eyre panic report
target. If it's empty, no panic happened — diagnose from the perf log
alone.

## Files of interest

- `src/tui.rs` — panic hook, terminal raw-mode lifecycle.
- `src/status_store.rs` — the shared status state, side-effect diff
  helper.
- `src/event.rs` — `EventHandler::next` with `biased` select and
  backlog log.
- `src/app.rs` — `App::run` with perf instrumentation,
  `react_to_status_transitions`.
- `src/command.rs` — `apply_subprocess_isolation` helper, `capture`.
- `src/repo.rs` — discovery probes (`tool_status`, `run_output`,
  `run_discovery`) all writing to `StatusStore`.
- `tests/render_smoke.rs` — render-path smoke coverage. Notably
  `renders_under_navigation_and_probe_interleaving` is the reproducer
  attempt at the unit-test level (currently passes — the real freeze
  needs a real Windows console, which `TestBackend` doesn't simulate).
