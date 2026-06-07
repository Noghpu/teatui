# Agent Notes

Greenfield. No backwards compat. Break shape if design says so.

Read [docs/rewrite-plan.md](docs/rewrite-plan.md) first. It is the active
source of truth for this rewrite — direction, architecture, phased plan,
and per-phase post-mortems. The older `docs/design.md` describes the
pre-rewrite design and is kept for reference only.

**Linux-only.** Windows support was intentionally dropped during the
2026-06 rewrite. There is no `cfg(windows)` code, no `windows-sys`
dependency, and no `windows_*`-prefixed test files. The development host
may still be Windows, but the snappiness benefits (instant Esc via kitty
keyboard protocol, no `crossterm_winapi` freezes) only manifest on Linux.

Tests lean, not absent. No regression test farms. Cover risky logic,
parsers, and a small floor of essential end-to-end coverage: the app
must be tested to start, and each implemented screen must render against
a `TestBackend` without panicking — including with representative
failure-state payloads (unreachable LLM, missing tools, errored
revsets). Render-path smoke tests live in
[`tests/render_smoke.rs`](tests/render_smoke.rs); when you add a new
screen or phase, add a corresponding render test in the same file.

Repo uses jj. Use `jj --no-pager …`. Do not use git history/status.

Command runner is `just`:

- Prefer `just verify` for handoff because it bundles formatting,
  compile check, linting, and tests. Use a single focused recipe only
  when exactly one check is relevant.
- `just fmt`: format code.
- `just check`: compile check.
- `just clippy`: lint Rust (`-D warnings`).
- `just test`: run all tests (unit + render smoke).
- `just verify`: run all handoff checks.

## Architecture at a glance

```
input thread (blocking crossterm::event::read)
        │ → InputEvent
        ▼
owner thread (runtime::Runtime)
        Select { input_rx, jobs_events_rx }
        - drains bursts
        - dispatches to App.on_input / on_job
        - renders iff App.is_dirty
                                ▲
                                │ JobEvent
worker pool (N=4)               │
        run Box<dyn Job>  ──────┘
        result delivered as Box<dyn Any + Send>
        downcast in App.absorb_payload
```

- `Cached<T>` from `runtime::cache` is the only state model views read
  from. `Loading` is shown ONLY when a fetch the user is waiting on is
  in flight; pure navigation never shows fake `Loading`.
- Predictive prefetch is load-bearing. Boot probes (tools, workspace,
  auth, LLM, revsets) start in `App::new` before the first paint.

## Code style

The repo follows the standard Rust style — `cargo fmt` is authoritative.
Clippy runs with `-D warnings`, so all lints must be resolved (not
silenced) unless there's a documented reason.

When adding a screen, also:
1. Add a `Screen::<Name>(Box<State>)` variant (box anything >32 bytes
   to keep `Screen` small).
2. Implement `pub fn on_key(state, status, key) -> Transition` and
   `pub fn render(state, status, frame, area)`.
3. Wire dispatch in `app.rs` (`dispatch_key`, `render`).
4. Add render tests covering every phase / failure state.

When adding a background job:
1. Implement `Job` in `domain::<topic>`. Return `JobOutcome::Done(Box<T>)`.
2. Add `match any.downcast::<T>()` arm to `App::absorb_payload`.
3. Mutate `StatusStore` or screen state from the handler.

## Deferred — PR management pass

Not in scope for the current rewrite. See the "Deferred — PR management
pass" section in [docs/rewrite-plan.md](docs/rewrite-plan.md) for the
findings carried forward from the pre-rewrite code (`pull_requests.rs`
list/filter/comment flow, `repo_options.rs` disk cache pattern, etc.).
