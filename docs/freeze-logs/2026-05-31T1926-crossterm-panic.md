# Freeze repro: crossterm_winapi panic captured

**Captured:** 2026-05-31 19:26:29–19:26:31 UTC (21:26 CEST), teatui run with `--debug`

## User narrative

> Another crash. I was hammering arrow keys, the app crashed and we ended up
> in the shell but it did not print any of my keypresses, **until** I pressed
> Ctrl+C, then they showed up. Then I pressed q a few times and killed the
> terminal.

## Root cause identified

The panic is in **`crossterm_winapi-0.9.1/src/console.rs:169:13`** with payload
`assertion failed: num_read == 1`. From the captured backtrace:

```
11: crossterm_winapi::console::Console::read_single_input_event
12: crossterm::event::source::windows::impl$1::try_read
13: crossterm::event::read::InternalEventReader::poll
14: crossterm::event::poll_internal
15: crossterm::event::stream::default::closure   (EventStream worker thread)
```

This means the EventStream background thread called `ReadConsoleInputW(handle,
&mut buf, 1, &mut num_read)`, the call succeeded, but `num_read` was `0`, not
`1`. The assertion fires, the thread panics, and `EventStream` never produces
another event.

This is a known-fragile spot in `crossterm_winapi` 0.9.x. `ReadConsoleInputW`
can legitimately return `num_read == 0` if the console input buffer is reset
or filtered mid-read (which can happen under console-state contention or
during rapid key event delivery). The assertion is too strict — it should
retry.

## Why this fully explains everything we've seen

- **"Freeze" without a hang.** Heartbeats kept firing, the main loop kept
  iterating draws and ticks, but the reader thread was dead so no key events
  ever reached the select!. The main loop ran healthily forever, never
  receiving input.
- **"Broken shell" with delayed echo until Ctrl+C.** When the reader thread
  panicked mid-`ReadConsoleInputW`, it left the Windows console in an
  inconsistent state. After teatui exited, the parent shell's keystrokes were
  buffered but not delivered. Ctrl+C reset the console subsystem state, after
  which the buffered keystrokes flushed.
- **Why subprocess isolation helped without fixing.** Console-handle
  contention from subprocesses raises the probability of triggering the
  underlying race. Reducing contention reduces hit-rate but doesn't fix the
  assertion bug.
- **Why CREATE_NEW_PROCESS_GROUP alone happened to look "good enough" some
  runs.** The crossterm panic is probabilistic — depends on timing of arrow
  key bursts vs console-state changes. Some runs win, some lose.

## Timeline

| Time         | Event                                       |
|--------------|---------------------------------------------|
| 29.872       | App starts                                  |
| 29.878       | ollama probe begins                         |
| 30.111       | First (and only) key event delivered        |
| 30.226–31.136| Normal ticks, no key events                 |
| 31.270       | **PANIC** in crossterm_winapi                |
| 31.270–31.387| Panic hook completes, backtrace logged       |
| 31.387       | Main loop exits via panic_occurred           |
| 31.388       | tui.exit succeeds                            |

Total run: 1.5 seconds. Crossterm panicked ~1.4 seconds in.

## Raw log

See `app-2026-05-31T1926-crossterm-panic.log` next to this file.

## Fix options

1. **Patch `crossterm_winapi`** via `[patch.crates-io]` pointing at a
   vendored fix that swaps the `assert!` for a retry loop when
   `num_read != 1`. Small code surface (~10 lines).
2. **Bump crossterm** to whatever current version is; the upstream may have
   already fixed it.
3. **Drop `EventStream`** and roll a small custom Windows console reader on
   a dedicated OS thread that handles the `num_read != 1` case directly and
   forwards events into a tokio channel.

Path (1) is the smallest deviation from existing code. Path (2) is worth
checking first — if upstream already fixed this, no patching needed.
