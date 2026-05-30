# Freeze repro: arrow-key hammering during discovery

**Captured:** 2026-05-31 14:26:22–14:26:36 UTC (16:26 CEST), teatui run with `--debug`

## User narrative

> I was hammering the arrow keys and the UI seemed to freeze. I kept on hammering
> the arrow keys for several seconds after and then switched to hammering q until
> a few 'q's were printed in the broken shell, then switched to arrow keys again
> pretty much immediately, saw '[A' etc. then killed the app.

## Headline finding

The runtime, main loop, and rendering pipeline are **healthy the entire time**:

- Heartbeat fires every ~1 s, on schedule, ticks 1 → 14 land cleanly.
- The `pre_draw` / `pre_await` trace pairs alternate every ~250 ms (the tick interval) without interruption.
- No panic. `PANIC_OCCURRED` never flips.
- Final exit is *clean*: `should_quit` → `main loop exiting` → `tui.exit succeeded`.

But there is **a single key event in the entire 14-second run**:

- `14:26:36.158` — one `event="key"` (the `q` that triggered `should_quit`).
- Nothing else. No arrow keys, no earlier `q` presses, nothing — despite the user reporting heavy hammering throughout.

So crossterm's console reader delivered **zero** keys for 11+ seconds while the
TUI loop spun normally. The keys the user pressed were either lost or sat
buffered in the console input queue until something released the contention,
at which point one key surfaced.

## Subprocess correlation

Only one background event surfaced before the final keystroke:

| Time          | Event             |
|---------------|-------------------|
| 14:26:25.125  | `bg:revsets`      |
| 14:26:36.158  | first key (the q) |

`bg:repo_options` **never arrived** — that probe was still running when the
user killed the app. The single key arrived **right after** the probe window
opened up, consistent with the still-running subprocess holding the Windows
console input handle and blocking crossterm's reader.

This is the same root pattern documented in `bug-windows-tui-freeze.md`:
console input handle contention from subprocesses. `CREATE_NEW_PROCESS_GROUP`
alone has not fully isolated them.

## "Broken shell" explanation

The user's "broken shell" + visible `[A` after killing the terminal:
1. `tui.exit` ran cleanly at `14:26:36.160` and *should* have disabled raw mode.
2. But child processes (probably scoop-shimmed `tea`, or whatever was still
   running for `repo_options`) may still have been holding console handles after
   the parent exited — keeping raw mode partially asserted or leaking input
   focus.
3. The shell received the user's subsequent arrow-key escape sequences directly
   in raw form and echoed them as `[A`.

## Raw log

See `app-2026-05-31T1426.log` next to this file for the full trace.

## Conclusions for the fix track

This run confirms:

- **The bug is not a runtime hang.** The tokio runtime, main loop, and ratatui
  rendering remain fully alive.
- **The bug is not a panic.** `PANIC_OCCURRED` stays false.
- **The bug is console input starvation** caused by subprocess contention on
  the shared Windows console input handle.
- **`CREATE_NEW_PROCESS_GROUP` alone is insufficient** — it changes signal
  routing, not handle inheritance. Children still inherit the console input
  handle.

Next mitigation (per the bug-doc escalation path):

1. Production spawns should set non-inheritable stdin/stdout/stderr handles
   via `SetHandleInformation`, or equivalently add `DETACHED_PROCESS` only when
   not in test mode (where `.cmd` shims need an attached console).
2. Migrate test fakes from `.cmd` to compiled Rust binaries so `0x208`
   (`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`) can be applied
   unconditionally.
