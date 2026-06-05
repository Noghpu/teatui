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
- `just snapshots`: render deterministic visual UI artifacts to
  `target/ui-snapshots/` (`.svg`, `.txt`, and `index.html`). Use this after
  UI changes so agents can inspect actual rendered screens and catch layout
  bugs visually, not only through smoke tests.

## UI gotchas

**Scrolling on overflow.** Any list/pane that renders a variable number
of lines into a fixed `Rect` MUST clamp to the available height and scroll
to keep the focused/highlighted row visible. Pushing every item into a
`Paragraph` silently truncates once content exceeds the area — the focused
row can scroll off-screen with no indication. This has bitten us three
times (Changes pane, Form pane, picker modal). When you add or edit a
rendered list, compute visible rows from `inner.height` and clamp to it.

Scroll *naturally*: persist the window's top offset (a `Cell<usize>` works
in render) and move it only when the highlight crosses the top or bottom
edge — never recompute the offset purely from the highlighted index, which
pins the highlight to one edge and scrolls the whole list on every keypress.
The pattern: `if hl < off { off = hl } else if hl >= off + rows { off = hl -
rows + 1 }`, with `off` pre-clamped to `len - rows`.

Don't re-derive this by hand. `screens::util::scroll_window(cur, start, end,
visible, total)` wraps the formula and returns a `ScrollWindow { offset,
range }`: store `offset` in your `Cell`, then either pass it to
`Paragraph::scroll` (whole-list rendering) or iterate `range` (sliced
rendering). Pass `start == end` for single-row highlights, or the first/last
row of a grouped multi-line item to keep its whole span visible.

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
2. Add a `try_payload!(any, T, payload => { … })` arm in `App::absorb_payload`.
   The macro handles the type-erased downcast and the early return; the body is
   the only thing you write. Either mutate `StatusStore`/screen state inline and
   set `self.dirty = true` at the end of the body, or delegate to a typed
   `handle_*` method that owns its own dirty flag and stale-phase guards (do
   *not* set `self.dirty` in the arm for the delegating case, or stale results
   will force needless redraws).
3. Mutate `StatusStore` or screen state from the handler.

## Deferred — PR management pass

Not in scope for the current rewrite. See the "Deferred — PR management
pass" section in [docs/rewrite-plan.md](docs/rewrite-plan.md) for the
findings carried forward from the pre-rewrite code (`pull_requests.rs`
list/filter/comment flow, `repo_options.rs` disk cache pattern, etc.).

### Pre-management groundwork (land before the management pass)

The PR and issue management modes are both scrollable lists with a
comment view/compose flow, so they should build on shared rendering and
process primitives rather than re-deriving them. The first shared
helpers have landed (`screens::util`, `screens::widgets`, and
`domain::process`). Three open tickets track the remaining concrete
groundwork; do them before starting the management screens so the new
code imports instead of copies:

- `0001o … app-job-payload-dispatch` — centralize the repetitive
  `App::absorb_payload` downcast boilerplate before more background job
  result types are added.
- `0001p … probe-process-helper-migration` — move the remaining
  mechanical `probe.rs` subprocess call sites onto `domain::process`,
  preserving each probe's current missing-tool/outside-workspace/fallback
  semantics.
- `0001q … scrollable-list-window-helper` — wrap the repeated
  visible-row/natural-scroll/offset/range pattern without extracting row
  rendering yet.

### Deferred refactors (extract when the management screens exist, not before)

These are intentionally NOT pre-built — doing so now would abstract on
guesses. Apply the rule of three: the first management screen is the
third concrete example for list rows and comments. If a future feature
adds a different third example, extract then, with that feature in hand.

- **Shared list-row builder.** The "focus marker + status badge +
  wrapped title + muted sub-row" shape is hand-built in
  `revset_row_lines` (Changes pane) and the row loop in
  `render_bulk_pr_list` (bulk PRs). PR/issue rows would be the third
  copy. Extract a shared row builder when writing the first management
  list; generalize against the second. Don't unify on two examples.
- **List/comment data layer.** Paginated remote lists + filter +
  refresh + comment read/write is a data-fetch/cache decision
  (`Cached<T>` + a per-fetch `Job` + filter state), not a rendering
  dedup — it has no existing pattern to share. It is closest to the
  pre-rewrite `pull_requests.rs`/`repo_options.rs` disk-cache flow noted
  above. Design it when the management feature is scoped.
- **Comment composer / generic form framework.** Reuse
  `screens::widgets` primitives when comments arrive, but do not
  generalize the Generate form and bulk PR editor into a framework until
  a real comment composer exists.
- **LLM request builders/parsers.** The single-PR and stacked-PR LLM
  paths have similar Ollama/OpenAI body construction and response
  parsing. Extract shared request/parse helpers only when another
  LLM-backed flow is added or provider behavior needs to change.
- **Render/dev fixtures.** `tests/render_smoke.rs` and
  `src/bin/ui-snapshots.rs` duplicate sample status, revsets, stack
  plans, prompts, drafts, and command previews. Extract shared
  cfg-gated fixtures when adding the first new management render-smoke
  cases.
- **Modal body helper.** `screens::util::open_modal` and themed blocks
  are enough for now. Add a simple modal-lines/body helper
  opportunistically when touching modals again, but do not schedule a
  standalone abstraction pass.
