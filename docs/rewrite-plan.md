# teatui Rewrite Plan

Source of truth for the in-place Linux-only rewrite. Update this doc as
decisions evolve. Each finished phase gets a **Post-mortem** entry at the
bottom; resume work by reading the post-mortems then jumping to the first
phase whose status is not `done`.

## How to resume

1. Read **Status** below to see the current phase and any deviations.
2. Skim the latest post-mortem(s) for context on what just changed.
3. Pick up the first phase whose status is `pending` or `in progress`.
4. The agreed design rules and architecture (sections below) are
   non-negotiable unless the post-mortems explicitly retract them.

## Status

- **Current phase:** 7 (Form editing) — done.
- **Branch / working tree:** uncommitted changes on `HEAD`. Use `jj
  --no-pager st` to inspect.

## Direction

Linux-only TUI rewrite focused on **snappy, never-swallowed input** and
**rendering the right thing the first time**. We drop all Windows-specific
code, Tokio, and Reqwest. PR management features are deferred to a later
pass; PR generation is the rewrite scope.

## Stack — locked

- **`ratatui` + `crossterm` (Unix paths only).** Kitty keyboard enhancement
  flags are pushed on entry → instant `Esc`, real key release events,
  unambiguous modifiers on supporting terminals (kitty, wezterm, foot,
  ghostty, recent alacritty). Graceful fallback elsewhere.
- **`crossbeam-channel` + small typed worker pool** for background jobs.
  No Tokio. Cancellation = drop the result; jobs are short and idempotent.
- **`ureq` for HTTP/LLM** (sync, pure Rust TLS). `reqwest::blocking`
  transitively pulls Tokio in, so it's out.
- Kept: `ratatui-textarea`, `serde`, `serde_json`, `clap`, `tracing`,
  `tracing-subscriber`, `color-eyre`, `humantime`, `config`, `dirs`,
  `arboard`, `opener`.
- Removed: `tokio`, `futures`, `reqwest`, `windows-sys`, the
  `crossterm_winapi` debug-assertion tweak.

## Design rules

### `Cached<T>` semantics

```text
Unknown                  — never fetched. Should rarely be visible; if it is,
                           prefetching wasn't wired up for this value.
Loading                  — in-flight fetch with no prior value. The ONLY
                           state where a loading indicator is appropriate.
Ready(T)                 — value available. Render as normal.
Stale { value, refreshing } — previous value available; show it. Indicate
                           refresh ONLY if `refreshing` is true.
```

**Rendering rule:** every keypress must render the *right* thing, not just
"something". Pure-navigation transitions render the new view from whatever
is already in the store — never with a fake `Loading`. `Loading` is only
shown when there is genuinely in-flight work the user is waiting on.

**Corollary:** predictive prefetch is load-bearing, not an optimization.
Discovery + revsets fire at app boot, before the landing screen's first
render. Any screen the user can navigate to must have its data at least
`Loading` (and usually `Stale`/`Ready`) before the first paint.

### Input is sacred

- Dedicated input thread does a blocking `crossterm::event::read()` loop
  and forwards every relevant event over a `crossbeam` channel.
  No polling overhead.
- Owner thread drains the input channel on each `Select` hit so bursts
  never get reordered behind background work.
- Anything that can take >1ms must move to a worker job.

### Render strategy

- Dirty-flag driven, not tick-driven. We redraw only when the app reports
  `is_dirty()` after dispatching an event.
- Animations (spinners) will require a tick channel — added when the first
  animated screen lands, not before.

## Architecture

```text
┌───────────────────┐    ┌──────────────────────────────────────────────┐
│ input thread      │ →  │ owner thread (runtime)                       │
│ crossterm::read   │    │   Select over (input_rx, jobs_events_rx)     │
└───────────────────┘    │   drain bursts, dispatch to App              │
                         │   render iff dirty                            │
┌───────────────────┐    │                                              │
│ worker pool (N=4) │ →  │                                              │
│ run Box<dyn Job>  │    └──────────────────────────────────────────────┘
└───────────────────┘
```

- `App` owns all state. The runtime never holds locks.
- `Jobs` is type-erased: `Job::run` returns `Box<dyn Any + Send>`; the
  consumer downcasts. Worker panics are caught and reported as
  `JobOutcomeEvent::Failed`.

## Module layout (target)

```
src/
├── main.rs              CLI parse, config load, logging, run
├── lib.rs               module list
├── config.rs            TOML config (XDG only)
├── logging.rs           tracing-subscriber → $XDG_STATE_HOME/teatui/logs/app.log
├── terminal.rs          raw mode, alt screen, kitty flags, panic teardown
├── input.rs             input thread → crossbeam channel
├── app.rs               AppState, on_input, on_job, render
└── runtime/
    ├── mod.rs           owner loop, Select, dirty-flag render
    ├── jobs.rs          Job trait, Jobs pool, JobEvent
    └── cache.rs         Cached<T>
```

Domain modules (jj, tea, llm, prompt, context, generate, …) land in their
own folder structure in later phases — exact shape TBD when phase 3 begins.

## Phased plan

| #  | Phase                       | Status      | Goal |
|----|-----------------------------|-------------|------|
| 0  | Strip + scaffold            | in progress | Compiles. Empty TUI launches, `q` quits. Tokio + Windows gone. |
| 1  | Job runtime + StatusStore   | pending     | `Cached<T>` store; discovery probes as jobs; debug dump of store. |
| 2  | Landing screen              | pending     | Action list + status pane. Prefetch on boot. `Loading` only when unavoidable. |
| 3  | Generate scaffolding        | pending     | Screen state machine, revset picker, form fields. |
| 4  | Generate domain             | pending     | Context collection, prompt build, LLM call (`ureq`), as Jobs. Stale-check on draft ready. |
| 5  | Execution                   | pending     | tea pr create + jj bookmark + push as a Job sequence. Completion screen. |
| 6  | Tests                       | pending     | Render smoke (`TestBackend`, 120×30). Job runtime tests already exist; expand. |

## Deferred — PR management pass (notes to carry forward)

These features exist in the old code and will be rebuilt later. Findings
worth preserving:

- `pull_requests.rs` had list / filter / detail / comment against
  `tea pr list` JSON. Keybindings `i` edit comment, `c` cycle phase.
- `external.rs` did browser-open (`opener`) + clipboard copy (`arboard`) —
  both Linux-clean.
- Per-PR write state machine `Idle → Editing → Submitting → Failed` was a
  clean template for any write action against tea.
- `repo_options.rs` had a disk cache (labels / milestones / assignees)
  keyed by `host/owner/repo`. Useful pattern for snappy field pickers.
- Old code in git history; restore with `jj op log` / `jj diff -r ...`.

## Post-mortems

Append-only. Newest at the bottom. Each entry: what was done, what worked,
what surprised, anything to remember.

### Phase 0, step 1 — Delete legacy files (task #4)

- Deleted: `src/{action,app,bookmark_naming,colors,command,context,event,external,generate,hidden_console,jj,llm,prompt,pull_requests,repo,repo_options,status_store,tea,tui,ui}.rs`, `src/bin/` (smoke-live, smoke-no-window), `tests/render_smoke.rs`, `tests/windows_landing_async.rs`, `tests/windows_pr_generation_integration.rs`, `stderr.log`.
- Kept: `src/{lib,main,config,logging}.rs` (to be overwritten), `docs/` (including freeze-logs and design.md), `AGENTS.md` (will revise tests section in phase 6).
- Surprise: none. Clean slate is cleaner than partial migration.
- Note: `docs/freeze-logs/app-*.log` raw logs were intentionally kept as research artifacts.

### Phase 0, step 2 — Rewrite `Cargo.toml` (task #1)

- Dropped: `tokio`, `futures`, `reqwest`, `windows-sys` (entire `[target.'cfg(windows)']` block), the `[profile.dev.package.crossterm_winapi]` debug-assertion workaround.
- Added: `crossbeam-channel = "0.5"`, `ureq = "2" + json + tls`.
- Changed: `crossterm = "0.29"` (removed `event-stream` feature — we use blocking `event::read` now).
- Kept release profile (`lto = "thin"`, `codegen-units = 4`, `panic = "abort"`, `strip = true`).
- Surprise: none. Will revisit `ureq` vs `ureq` 3.x when phase 4 lands.

### Phase 0, step 3 — Scaffold new src tree (task #2)

- Wrote: `src/lib.rs`, `src/main.rs`, `src/config.rs`, `src/logging.rs`, `src/terminal.rs`, `src/input.rs`, `src/app.rs`, `src/runtime/{mod,cache,jobs}.rs`.
- Design choices made during scaffold:
  - **No render tick.** The original plan mentioned a 16ms tick; we dropped it because there are no animations yet. Owner loop is purely event-driven: `Select` over input + job events. Tick channel re-introduced when the first animated screen lands.
  - **Burst draining.** After each `Select` hit, drain `try_recv` on the same channel before continuing. Prevents reordering input behind background work, and lets a flurry of job events coalesce into one redraw.
  - **`AppDecision` removed.** Not needed yet; `on_input` returns `()`. Add a return type when actions actually need to spawn jobs from the app side.
  - **Heartbeat thread.** Kept the original "alive" tick (1Hz) on a plain `std::thread` — cheap, useful for diagnosing future freezes.
  - **Kitty flags negotiated in `Terminal::enter`.** Failure is silently ignored (terminal didn't support it). Logged at `debug` so we can confirm in field reports.
  - **Panic hook in `terminal.rs`.** Tears down terminal then chains to the original hook. Simpler than the old version: no `PANIC_OCCURRED` flag exposed, because the runtime doesn't need it — the panic propagates normally after teardown.
- Surprise: `ratatui-textarea` is not yet imported anywhere — that's fine, it'll surface in phase 3 (form fields). Left it in `Cargo.toml` so phase 3 doesn't have to add it.

### Phase 0, step 4 — Verify (task #3)

- `just verify` passes: `cargo fmt --check` clean, `cargo check` clean,
  `cargo clippy --all-targets --all-features -- -D warnings` clean,
  `cargo test --all-targets --all-features` passes 7 tests
  (3 `Cached<T>` transitions, 2 worker-pool behaviours including
  panic-recovery, 2 config loaders).
- Fix required: rustfmt wanted a wrapped `matches!(...)` in
  `runtime/cache.rs::is_refreshing`. Applied with `cargo fmt`.
- Surprise — `windows-sys` still appears in `cargo test` output as a
  transitive dep. That's from `arboard` and `opener` for
  clipboard/browser-open APIs on Windows. We didn't add it ourselves and
  it's gated on `cfg(windows)`; on Linux it won't appear. Acceptable.
- Caveat for the dev workflow: the primary dev machine is currently
  Windows. The code compiles and tests pass there, but the *snappiness
  benefits* (instant Esc via kitty protocol, no `crossterm_winapi`
  freezes) only manifest when actually run on Linux. Recommend doing
  the next interactive smoke-test on Linux (WSL is enough for kitty
  protocol if running under a kitty/wezterm/foot terminal).

### Phase 1 — Job runtime + StatusStore

- **Status:** done. `just verify` green (15 unit tests).
- Restored `LlmConfig { base_url, model, temperature, max_tokens, timeout_secs }` and `PrConfig { default_base }` in `config.rs`. Single-backend schema; multi-backend deferred until we actually need it.
- Split `Jobs` into `Jobs` (owns workers + receiver) + `JobSubmitter` (cloneable Sender + shared `Arc<AtomicU64>` id counter). Apps and downstream code submit jobs via a `JobSubmitter`; `Jobs::events()` stays on the runtime.
- `Runtime::new` now takes a factory `FnOnce(JobSubmitter) -> App`. Removes the chicken-and-egg between owning Jobs and constructing App.
- New `src/domain/{probe,status_store}.rs`:
  - `VersionProbe { kind: Jj|Git|Tea, binary }` → `VersionResult` (`ToolStatus::Available { version } | Missing | Errored`).
  - `WorkspaceProbe { jj_binary }` → `WorkspaceInfo::Inside { root } | Outside | Errored`.
  - `TeaAuthProbe { tea_binary }` → `TeaAuthStatus::Configured { logins } | None | Errored`. Parses `tea login list` whitespace-aligned output, skipping the header row.
  - `LlmHealthProbe { base_url, timeout }` → `LlmHealth::Available { models } | Unreachable`. Uses `ureq::AgentBuilder::new().timeout(...).build()` + `into_json::<TagsResponse>()`.
  - `StatusStore` holds one `Cached<T>` per probe + `mark_all_loading()` and `set_*` setters.
- `App::new` now: builds `StatusStore`, calls `mark_all_loading()`, submits all six probes via `JobSubmitter`. Discovery is running before the first render.
- `App::on_job` does a downcast chain (`VersionResult` → `WorkspaceInfo` → `TeaAuthStatus` → `LlmHealth`) and writes into the store. Unknown payload types log a warning rather than panic — phase 3+ payloads will extend the chain.
- Render is still a debug-style status dump, but laid out with `tools` / `environment` headers and a footer hint. Phase 2 replaces it with the proper landing UI.

Surprises / things to remember:

- ureq 2.12's `AgentBuilder::timeout` covers both connect and read. If a probe hangs the call returns `Unreachable { message: "connection timed out" }` rather than blocking the worker — verified behaviour, no test needed yet.
- The downcast chain pattern (`match any.downcast::<T>() { Ok(b) => …, Err(a) => a }`) gets verbose fast. If it crosses ~6 types, refactor to a `Box<dyn AppPayload>` trait or a macro. For now, fine.
- We did NOT bring back the old multi-backend `LlmConfig` schema. If the user has a config.toml from before with `[[llm.backends]]`, it will be ignored. We'll prompt for migration on encounter, not preemptively.

### Phase 2 — Landing screen

- **Status:** done. `just verify` green.
- Added `Action` enum (`GeneratePr`, `Quit`) with a label method; landing renders both with a `▶` marker on the selected row, cyan + bold highlight.
- Layout uses ratatui `Layout::vertical` with fixed-size action block + separator + min-sized status block + 1-line footer.
- Status pane rendered from `StatusStore` via per-variant formatters (`render_tool`, `render_workspace`, `render_auth`, `render_llm`). All go through a `render_cached` helper that emits `loading…` for `Loading`, the value for `Ready`, and `value (refreshing…)` for `Stale { refreshing: true }`.
- Input rule honoured: `selected` only changes (and `dirty` only flips) when navigation actually moved. Pressing Down at the last action is a no-op — no redraw, no log spam.
- `Generate PR` activation currently logs `Generate PR selected (phase 3 pending)`. Phase 3 wires it to a screen transition.

Surprises:
- Clippy required `if-guard` form for the up/down arms (`(KeyCode::Up, _) if self.selected > 0 => …`) instead of an `if` inside the arm. Cleaner anyway.
- Layout `Constraint::Length(ACTIONS.len() as u16)` works because `ACTIONS: &[Action]` and `usize → u16` cast is safe for ≤ 65535 items. Trivial but worth noting if action count ever exceeds 1 row per item.

### Phase 3 — Generate scaffolding

- **Status:** done. `just verify` green (18 unit tests).
- `RevsetProbe { jj_binary, revset }` (default revset: `mutable()`) runs `jj --no-pager log -r <revset> --no-graph -T <template>`; the template emits tab-separated rows `change_id<TAB>commit_id<TAB>bookmarks_joined<TAB>description.first_line<TAB>author_name`. `parse_revsets` splits each non-blank line with `splitn(5, '\t')` and returns `RevsetSummary`.
- Output type `Revsets::Loaded(Vec<RevsetSummary>) | Errored { message }` lives as `Cached<Revsets>` on `StatusStore`. `mark_all_loading()` now also touches `revsets`.
- App submits `RevsetProbe` at boot alongside the discovery probes — by the time the user navigates to Generate, the list is there (or visibly Loading).
- `src/screens/{mod,landing,generate,status}.rs` introduced. `Screen::Landing(LandingState) | Generate(GenerateState)`. `Transition::{None, Dirty, Quit, Navigate(NewScreen)}` returned from each screen's `on_key`; `App::apply_transition` is the one place that mutates `screen / quit / dirty`.
- Generate screen: 3 columns (Revsets 34 / Form min 30 / Preview 34) inside an outer `Min(0) + Length(1)` split, with a footer line showing focused pane label, revset count, and key hints.
- Selecting a revset (Up/Down/k/j in Menu pane) syncs `form.head = item.change_id` so the form already reflects what would be PR'd. Entering Generate primes `form.head` with the first revset if any are loaded.
- No form editing yet. Tab/BackTab cycles pane; Esc → Landing; q/Ctrl+C quits.

Surprises / lessons:
- Ratatui `Line<'a>` keeps its lifetime tied to any `&str` you feed into `Span::styled(..., style)`. Several functions had to be widened to return `Line<'static>` (taking owned `String` instead of `&str`) once values came from `format!` temporaries. Easy fix but trips you up if you try to write a "borrowing" Line constructor.
- The `KeyboardEnhancementFlags::REPORT_EVENT_TYPES` flag means we now see `KeyEventKind::Repeat` too. The input thread filter `if key.kind == KeyEventKind::Press` already excludes those; if we ever want key-repeat navigation we can broaden the filter.
- Considered passing `&JobSubmitter` to screen `on_key` so screens can spawn their own jobs. Decided against it for now: keeps screens pure (only mutate own state + emit Transition) and centralises the "what jobs follow from which user actions" logic in App. We'll revisit if it gets unwieldy.

### Phase 4 — Generate domain

- **Status:** done. `just verify` green (28 unit tests).
- `domain/context.rs`: `ContextJob { jj_binary, revset, diff_byte_budget }` shells `jj status / log -r ... --no-graph / diff -r ... --stat / diff -r ...` sequentially on a worker. Diff truncated to budget with a `[... truncated ...]` marker, UTF-8-aware (walks `char_indices`).
- `domain/prompt.rs`: pure `build_prompt(&ContextBundle) -> PromptBuild { prompt, manifest }`. Four sections (Status / Log / Diff Stats / Diff); empty sections render `(empty)` placeholder so the LLM gets unambiguous structure. Manifest tracks per-section bytes for the preview pane.
- `domain/llm.rs`: `LlmGenerateJob` POSTs `/api/generate` to Ollama with `stream: false`. `parse_draft` tries JSON first, strips `\`\`\`json … \`\`\`` fences, falls back to first-non-blank-line / rest split.
- `GeneratePhase`: `Idle | Collecting | Generating { context, prompt } | DraftReady { draft, prompt } | Failed { message }`. `#[derive(Default)]` with `#[default]` on `Idle`.
- Wiring: `Transition::Generate` added. App's `start_generation` submits ContextJob; `handle_context_result` builds prompt + submits LlmGenerateJob; `handle_llm_result` populates `form.title / form.description` and transitions to DraftReady. Stale results from prior runs are ignored (phase guard).
- Preview pane renders by phase: Idle (head + base), Collecting ("collecting context…"), Generating (prompt manifest with byte sizes), DraftReady (title + wrapped description + total bytes), Failed (red error + retry hint).
- Footer shows pane label, revset count, phase summary, and `g generate • tab cycle • esc landing • q quit`.

Surprises:
- Clippy flagged `Screen::Generate(GenerateState)` as a large-variant enum (392 bytes vs 8 for Landing). Boxed: `Generate(Box<GenerateState>)`. Worth keeping in mind when introducing future screen variants.
- ureq 2.x's `AgentBuilder::timeout` + `agent.post(...).send_json(value)` is the natural sync POST-JSON shape; `response.into_json::<T>()` does the deserialize. No tokio anywhere in the LLM path.
- `std::mem::replace(&mut state.phase, GeneratePhase::Idle)` is the cleanest way to take ownership of the `prompt: PromptBuild` out of `Generating` without cloning. Avoid `mem::take` because reusing the unchanged variant requires putting it back — `replace` makes that explicit.

### Phase 5 — Execution

- **Status:** done. `just verify` green (36 unit tests).
- `domain/bookmark.rs`: `slugify` — ascii alnum lowercased, runs of other chars collapse to one dash, trimmed, capped at 64 chars (with a second trim in case truncation left a trailing dash).
- `domain/execute.rs`: `ExecutePrJob` runs the three-step pipeline sync on a worker — `jj bookmark set --allow-backwards <branch> -r <change>`, `jj git push --bookmark <branch>`, `tea pr create --base <base> --head <branch> --title <title> --description <description>`. Per-step failures return `ExecuteResult::Errored { step, message }`. URL parsing scans `split_whitespace` for the first `http://`/`https://` token, trimming trailing `. , ; )`.
- `GeneratePhase` grew `Executing { draft }` + `Done { url }`. `is_in_progress` now includes Executing.
- LLM-ready handler derives `form.branch = slugify(draft.title)` when the form's branch is empty.
- `Transition::Execute` (from `x`), `Transition::CopyUrl` (from `c`), `Transition::OpenUrl` (from `o`). Copy uses `arboard::Clipboard`; open uses `opener::open`. Both set `state.last_action: Option<&'static str>` for a one-line green hint in the Preview pane.
- Footer hint string adapts to phase: `DraftReady` shows `g regenerate • x execute`, `Done` shows `c copy • o open`.

Surprises:
- Clippy `manual_pattern_char_comparison` flagged the closure form `trim_end_matches(|c| matches!(c, ...))`. Array literal `['.', ',', ';', ')']` is the modern idiom — same semantics, simpler.
- `arboard::Clipboard::new().and_then(|mut c| c.set_text(url))` — the clipboard handle must outlive the set on Linux. Holding it for the duration of the call is enough; we drop after.

### Phase 6 — Tests

- **Status:** done. `just verify` green (36 unit + 18 render smoke = **54 tests**).
- `tests/render_smoke.rs` covers a 120×30 `TestBackend` render for: landing (default / all-loading / populated / missing-tools / quit-selected), generate in every `GeneratePhase` (Idle, Collecting, Generating, DraftReady, Executing, Done, Done-with-action-hint, Failed), and each Revsets state (Loaded-empty, Loading, Errored) and each pane focus.
- `build_prompt_then_render_does_not_panic` stitches the pure prompt builder into a DraftReady render so any divergence between the manifest and the preview surface is caught.
- `AGENTS.md` rewritten for the Linux-only rewrite: dropped the `windows_*` test-file convention, added an architecture diagram, codified the "every screen + every phase has a render test" rule, pointed at `docs/rewrite-plan.md` as source of truth.

Surprises:
- Building `GenerateState` literals from tests required `screens::generate::PrForm` to be public-accessible — it already was. No additional API exposure needed; the screen module's public surface is exactly what tests need.
- `TestBackend::new(120, 30)` + `Terminal::new(backend)` + `terminal.draw(|frame| ...)` is the same shape ratatui uses in its own examples. No special test fixtures needed.

## Status — rewrite complete through Phase 7

The original six planned phases plus Phase 7 are done. Final state:

- `src/{main,lib,config,logging,terminal,input,app}.rs` + `src/runtime/{mod,cache,jobs}.rs` + `src/domain/{mod,probe,status_store,context,prompt,llm,execute,bookmark}.rs` + `src/screens/{mod,landing,generate,status}.rs`.
- 41 unit tests + 22 render smoke tests.
- `just verify` clean: fmt, check, clippy `-D warnings`, tests.
- No Tokio, no Reqwest, no `windows-sys`, no `cfg(windows)` paths.
- Kitty keyboard enhancement flags negotiated on entry → instant Esc on supporting terminals.

### Known gaps / next moves

1. **Stale-check before execute.** If the user takes >some-threshold between LLM draft and `x`, the underlying revset may have changed. The old code re-ran the revset probe just before execution. Worth adding next.
2. **Multi-revset selection.** Currently we pick a single revset to PR. Multi-select (the old space/comma UX) would let a user PR a stacked range.
3. **PR management pass.** See the "Deferred" section above. List / detail / comment / browser-open flow against `tea pr list`.
4. **Linux interactive verification.** The dev host is Windows; smoke tests pass there but the snappiness wins only materialise on Linux. Recommend running the binary on Linux (kitty / wezterm / foot terminal) and confirming instant-Esc + no input swallowing.

## Phase 7 — Form editing

Goal: bring back the form editing UX from the pre-rewrite code (read out
of jj revision `mslxvzxn` for reference). Every field on the Generate
form becomes either an editable text area (single- or multi-line) or a
filterable picker with options sourced from background probes.

### UX (matches prior iterations)

- **Input modes** on the Generate screen: `Normal | Editing`.
  Confirm-before-execute is a separate concern, stays out of phase 7.
- **Field focus** in Form pane: `Up/Down` (or `j/k`) moves between
  fields, clamp-bounded. `Tab/BackTab` still cycle *panes*, not fields —
  the old code used this split and it was right.
- **Begin edit**: `i` or `Enter` when Form pane focused on a field.
- **Commit / cancel**:
  - single-line text (title, branch): `Enter` commits, `Esc` cancels.
  - multiline text (description): `Ctrl+S` (or `Alt+Enter`) commits,
    `Esc` cancels. Enter inserts a newline.
  - picker: `Enter` commits (selecting the highlighted option for
    single-select), `Esc` cancels. `Space` toggles in multi-select.
- **Picker filter**: typing in editing mode filters the option list;
  `Up/Down` moves highlight; `Backspace` shrinks the filter.

### Field shape

Eight fields (preserve old ordering):

| FieldId       | Kind                                 | Source                                          |
|---------------|--------------------------------------|-------------------------------------------------|
| `Head`        | single-select picker, required       | `StatusStore::revsets` (already prefetched)     |
| `BranchName`  | single-line text, required           | derived from title via `slugify` until user edits |
| `Base`        | single-select picker, required       | new `BaseBookmarksProbe` (jj bookmark list)     |
| `Title`       | single-line text, required           | populated by LLM `DraftReady`                   |
| `Description` | multiline text, required             | populated by LLM `DraftReady`                   |
| `Labels`      | multi-select picker, optional        | new `RepoOptionsProbe` (tea api `/repos/{}/{}/labels`) |
| `Assignees`   | multi-select picker, optional        | `RepoOptionsProbe` (tea api `/repos/{}/{}/collaborators`) |
| `Milestone`   | single-select picker, optional       | `RepoOptionsProbe` (tea api `/repos/{}/{}/milestones`) |

### Types — keep close to the prior shapes (they earned their nuance)

```rust
// screens/generate/form.rs
pub enum FieldId { Head, BranchName, Base, Title, Description, Labels, Assignees, Milestone }

pub enum FieldKind { Text { multiline: bool }, Picker { multi_select: bool, optional: bool } }

pub enum FieldState {
    Text(Box<TextFieldState>),
    Picker(PickerFieldState),
}

pub struct TextFieldState {
    initial: String,
    pub value: String,
    pub buffer: String,
    pub editor: ratatui_textarea::TextArea<'static>,
    pub dirty: bool,
    pub errors: Vec<String>,
}

pub struct PickerOption { pub label: String, pub value: String, pub enabled: bool }

pub struct PickerFieldState {
    initial: Vec<String>,
    committed: Vec<String>,
    draft: Vec<String>,
    pub value: String,        // joined "a, b"
    pub options: Vec<PickerOption>,
    pub filter: String,
    pub highlighted: usize,
    pub multi_select: bool,
    pub optional: bool,
    pub editing: bool,
    pub errors: Vec<String>,
    pub dirty: bool,
}
```

Why the draft/committed/initial triple matters: a picker has three
states the UI needs to distinguish — what the user originally had
(`initial`, used to compute `dirty`), what they last committed
(`committed`, what we'd actually use), and what they're currently
toggling (`draft`, what the rendered popup shows). The prior code's
`begin_edit`/`commit`/`cancel` triad lifts cleanly into our new
arch — preserve it.

For text fields, `buffer` mirrors the `TextArea` content live;
`value` only updates on commit. `dirty = (value != initial)`.

### State changes

- `GenerateState` grows: `input_mode: InputMode`, `field_focus: FieldId`, replace the plain `PrForm` (currently `String`/`Vec<String>`) with `PrForm { head: FieldState, ... }`.
- `is_in_progress` unchanged.
- New helper `GenerateState::ensure_field_options_synced(&StatusStore)` called whenever picker-source data lands, so picker option lists reflect current revsets / base bookmarks / repo options.

### New probes / jobs

1. `BaseBookmarksProbe { jj_binary }` runs `jj --no-pager bookmark list --all-remotes -T '<template>'`. Returns `Vec<BaseBookmark { name, remote: Option<String>, is_remote: bool }>`. Cached on `StatusStore::base_bookmarks`. Submitted at boot.
2. `RepoOptionsProbe { tea_binary, host, owner, repo }` runs `tea api repos/{owner}/{repo}/labels`, `…/collaborators`, `…/milestones` (three tea calls, can be three jobs or one combo). Returns `RepoOptions { labels: Vec<String>, assignees: Vec<String>, milestones: Vec<String> }`. Requires knowing the remote owner/repo, which comes from `WorkspaceProbe` → `jj git remote list` or parsing `git remote get-url origin`. Submitted *after* WorkspaceProbe completes (dependency).

   For phase 7 simplicity, infer owner/repo from the remote URL in `WorkspaceProbe`. If not derivable, repo-options probe is skipped and the three pickers show "no options".

3. (Optional) Disk cache for `RepoOptions` keyed by `host/owner/repo`, mirroring the prior pattern. Defer to a follow-up if the API is fast enough.

### Input dispatch changes

In `screens/generate/on_key`:
- When `input_mode == Normal` and Form pane focused:
  - `Up/Down/j/k`: move `field_focus` among `FieldId::ALL`.
  - `i` or `Enter`: `field.begin_edit()`, set `input_mode = Editing`.
  - all other current bindings (`g`, `x`, `Esc`, `Tab`, etc.) keep working as today.
- When `input_mode == Editing`:
  - `Esc`: `field.cancel()`, `input_mode = Normal`.
  - text single-line + `Enter`: `field.commit()`, `input_mode = Normal`.
  - text multiline + `Ctrl+S` (and `Alt+Enter` as alias): commit.
  - text + anything else: forward to `field.editor.input(key)`.
  - picker + `Enter`: commit (single-select selects highlighted).
  - picker + `Space` (multi-select): toggle highlighted.
  - picker + `Up/Down`: move highlighted.
  - picker + `Char` / `Backspace`: filter.

The dispatch grows substantially; keep it in a helper file
`screens/generate/input.rs` so `generate.rs` proper stays scannable.

### Rendering changes

Form pane:
- Render each field as `label  value` with the focused field marked
  (`▶`, cyan) and dirty fields suffixed with `•`.
- When editing a text field: render the `TextArea` widget inline in the
  row (single-line) or as an inline expansion (multiline).
- When editing a picker: render a centered modal overlay (`Clear` +
  bordered block) listing visible options with selection markers
  (`[x] foo`, `[ ] bar` for multi; `▶ foo` for single).

Preview pane stays unchanged.

Footer hints become mode-aware:
- Normal: `i edit • g generate • x execute • tab cycle • esc landing`
- Editing text: `enter commit • esc cancel` (or `ctrl-s commit` for multi)
- Editing picker: `enter commit • space toggle • esc cancel`

### Validation hook-up

- `Title`, `BranchName`, `Head`, `Base` errors block `g` and `x`.
- `Description` empty → soft warning (allow but show ⚠).
- Picker `optional=false` empty → error.
- Validation errors stored on `field.errors`, rendered next to the field
  in red.

`x` precondition becomes: `field_focus is fine + form.validate()` succeeds.

### DraftReady wiring

When LLM returns a draft, call `form.title.set_value(draft.title)` and
`form.description.set_value(draft.description)`. `set_value` resets
`initial=value=buffer`, recreates the `TextArea`, clears `dirty`. If
`form.branch_name.value` is empty, also `set_value(slugify(title))`.

`set_value` must NOT clobber a field the user has edited (dirty=true).
Preserve that — old code allowed re-running `g` to overwrite even dirty
fields, but if we want a kinder UX we can only fill empty/clean fields.
Match the old code's behaviour (clobber) for parity; revisit later.

### Execute wiring

`ExecutePrJob` already takes plain strings. After phase 7, build it
from `form.field(FieldId::*).value` rather than the current `form.title`
etc. Also pass `labels`, `assignees`, `milestone` to tea via additional
flags (`--label`, `--assignee`, `--milestone`). Verify tea's actual flag
names against `tea pr create --help`; phase 7 needs to adjust
`execute.rs` accordingly.

### Test coverage to add

Unit tests in `screens/generate/form.rs`:
- `TextFieldState::input` happy paths (chars, backspace, Enter for
  multiline) and `commit/cancel` semantics.
- `PickerFieldState::input` filtering, highlight movement, multi-select
  toggle, single-select Enter-commit.
- `PrForm::validate` flags missing required fields.

Render smoke tests in `tests/render_smoke.rs`:
- form pane with each field focused, both Normal and Editing modes.
- picker modal in multi-select and single-select shapes.
- TextArea inline in form (single-line + multiline).
- dirty-field marker shown for edited fields.

### Estimated effort

Sizing: roughly the same as phase 3+4 combined. Three sub-steps:

- **7a**: `FieldState` types + new `PrForm`, with empty option lists
  (no probes wired yet). Dispatch in/out of Editing mode for text
  fields only. Verify.
- **7b**: Picker rendering + dispatch (modal overlay, filter, multi-select
  toggle). Wire `Head` picker options from existing `StatusStore::revsets`.
  Verify.
- **7c**: `BaseBookmarksProbe` + `RepoOptionsProbe` + remote-info parsing
  out of WorkspaceProbe. Hook up the remaining pickers. Validation.
  Tea-flag extension for labels/assignees/milestone in `ExecutePrJob`.
  Verify.

Each sub-step ends green on `just verify` and gets a post-mortem
entry. Phase 8 (stale-check) becomes feasible immediately after 7c.

### Phase 7 — Form editing

- **Status:** done. `just verify` green (41 unit + 22 render smoke = **63 tests**).
- Added `screens::generate::form` with `InputMode`, `FieldId`, text fields backed by `ratatui-textarea`, picker fields with `initial / committed / draft`, and `PrForm::validate`.
- Added `screens::generate::input` so Generate screen input dispatch stays separate from rendering. Normal mode keeps pane navigation on Tab/BackTab and field navigation on Up/Down or j/k in the Form pane. Editing mode handles text commit/cancel, multiline Ctrl+S or Alt+Enter, picker filter/highlight/toggle/commit.
- Generate form now renders all eight fields, dirty markers, validation messages, inline text editing, and centered picker modals. Footer hints are mode-aware.
- Boot probes now include `BaseBookmarksProbe`. `WorkspaceProbe` also derives origin remote owner/repo when possible and schedules `RepoOptionsProbe` for labels, collaborators, and milestones. Picker options sync into an open Generate screen whenever source data lands.
- `ExecutePrJob` builds from field values and passes optional fields to tea using verified current flags: `--labels`, `--assignees`, and `--milestone`.
- User-requested dependency bump completed in the same slice: direct deps are pinned to current published versions, including `ureq 3.3.0`, `opener 0.8.4`, and `ratatui-textarea 0.9.1`. HTTP call sites were migrated to ureq 3's `Agent::config_builder`, `send_json(&body)`, and `body_mut().read_json()` APIs.

Surprises / things to remember:

- `tea pr create --help` uses plural `--labels` / `--assignees`, not the singular names in the Phase 7 sketch.
- `cargo update --dry-run --verbose` still reports `generic-array 0.14.7` behind latest, but it is transitive and constrained upstream.
- The repo is still Linux-only by design, but Windows-target transitive crates can appear while compiling on the Windows dev host through cross-platform dependencies.
