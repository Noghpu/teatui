---
id: 0000v-2026-05-28-e70c02b1-windows-config-xdg-fallback
created_at: 2026-05-28T16:08:28+02:00
created_by_model: gpt-5/medium
state: implemented
state_updated_at: 2026-05-28T16:23:57+02:00
---
# Windows Config Should Prefer XDG Config Home

## Goal
Change default config discovery so Windows honors `XDG_CONFIG_HOME` when it is set and non-empty, using `$XDG_CONFIG_HOME/teatui/config.toml` before falling back to the existing Roaming AppData location from `dirs::config_dir()`.

## Context
The design currently says Linux/macOS use `$XDG_CONFIG_HOME/teatui/config.toml` and Windows uses the platform config directory via `dirs`. The requested behavior is to make Windows check the XDG environment variable as well, while preserving the current AppData/Roaming fallback.

`src/config.rs` currently builds configuration in `Config::load` by checking `dirs::config_dir().join("teatui").join("config.toml")`, then an explicit CLI path, then `TEATUI_*` environment overrides. Keep that precedence for explicit paths and environment overrides: default config first, explicit `--config` path second, environment overrides last.

## Non-Goals
Do not change the config schema or LLM backend fields. Do not introduce a new CLI flag. Do not migrate, copy, or delete existing user config files. Do not change Linux/macOS behavior beyond any small helper extraction needed to keep path discovery readable.

## Design Decisions
On Windows, the default config candidate order is:

1. If `XDG_CONFIG_HOME` is set and not only whitespace, `$XDG_CONFIG_HOME/teatui/config.toml`.
2. Otherwise, `dirs::config_dir()/teatui/config.toml`, which is the current Roaming AppData behavior.

Only the first applicable default candidate should be loaded. If `XDG_CONFIG_HOME` is set but the XDG config file does not exist, do not silently also load AppData; the environment variable is an explicit location choice. The existing explicit `--config` path should still layer on top of whichever default candidate was loaded, and `TEATUI_*` environment overrides should still win over file values.

Extracting a small path helper in `src/config.rs` is acceptable so tests can cover candidate resolution without mutating the real user environment or depending on the developer machine.

## Implementation Plan
Update `src/config.rs` to route default config path selection through a helper that can inspect an optional XDG value and an optional platform config directory. Use `std::env::var_os("XDG_CONFIG_HOME")` for the runtime Windows check and treat empty or whitespace-only values as unset.

Gate Windows-only behavior with `#[cfg(windows)]` or a helper that has platform-specific internals. Keep non-Windows behavior aligned with the existing design and `dirs::config_dir()` unless the code already handles XDG through `dirs` on those platforms.

Add focused unit tests in `src/config.rs` for the helper logic:

- On Windows, non-empty `XDG_CONFIG_HOME` produces `<xdg>/teatui/config.toml`.
- On Windows, missing or blank `XDG_CONFIG_HOME` falls back to `<platform-config>/teatui/config.toml`.
- When no platform config dir is available and no XDG is set, there is no default path.

If testing actual `Config::load` with environment variables is feasible without global-env races, keep it narrow; otherwise prefer pure helper tests and existing config deserialization tests.

Update `docs/design.md` Configuration to state that Windows first checks `XDG_CONFIG_HOME` and falls back to the platform config directory via `dirs`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/design.md", "src/config.rs"],
  "likely_files": ["src/config.rs", "docs/design.md"],
  "verification_commands": ["cargo test config::tests", "just verify"],
  "review_focus": ["Windows XDG_CONFIG_HOME precedence is explicit and AppData remains fallback", "Explicit --config and TEATUI_* override precedence are preserved", "Tests avoid relying on the developer machine's real config directory"],
  "jj_description_prefix": "fix"
}
```

## Acceptance Criteria
- Windows default config discovery uses `$XDG_CONFIG_HOME/teatui/config.toml` when `XDG_CONFIG_HOME` is set to a non-blank value.
- Windows default config discovery falls back to `dirs::config_dir()/teatui/config.toml` when `XDG_CONFIG_HOME` is unset or blank.
- A set `XDG_CONFIG_HOME` is treated as the chosen default location even if that file does not exist; AppData is not additionally checked in that case.
- Explicit config paths and `TEATUI_*` environment overrides keep their existing precedence over default file values.
- `docs/design.md` describes the new Windows lookup order.

## Verification Plan
Run `cargo test config::tests` for focused config coverage. Run `just verify` for handoff unless unrelated in-flight work in the default workspace makes full verification inappropriate; if that happens, report the narrower checks that were run.

## Files Likely Touched
- `src/config.rs`
- `docs/design.md`

## Risks
Environment-variable tests can become flaky if they mutate process-global state while tests run in parallel. Prefer pure helper tests that pass synthetic XDG and platform config inputs, or serialize any environment-mutating tests carefully.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: claude-sonnet-4-6
- completed_at: 2026-05-28T16:23:57+02:00
- state: implemented

## What was completed
- Added `default_config_candidate` helper in `src/config.rs` that accepts an optional XDG_CONFIG_HOME OsString and an optional platform config PathBuf, returning the appropriate `teatui/config.toml` path.
- On Windows (`#[cfg(windows)]`), the helper checks XDG_CONFIG_HOME first; if set and non-blank/whitespace, uses `$XDG_CONFIG_HOME/teatui/config.toml`. Otherwise falls back to the platform config dir (AppData/Roaming).
- On non-Windows, the XDG parameter is ignored (dirs already handles XDG on those platforms).
- Updated `Config::load` to route default path selection through the helper, calling `std::env::var_os("XDG_CONFIG_HOME")` and `dirs::config_dir()` and passing both to the helper.
- Added 4 focused unit tests under `#[cfg(windows)]` in `config::tests` covering: XDG set uses XDG path, XDG empty falls back to platform, XDG missing falls back to platform, neither set returns None.
- Updated `docs/design.md` Configuration section to describe Windows now first checks `XDG_CONFIG_HOME` before falling back to Roaming AppData.

## Deviations from plan
None. Implementation matches the plan exactly. Used `let Some(...) && condition` syntax to satisfy clippy's `collapsible_if` lint.

## Verification
Ran `cargo test config::tests` â€” 8 tests pass (4 new + 4 existing).
Ran `just verify` â€” all 157 tests pass, fmt, clippy, and check all green.

## Important files changed
- `src/config.rs` â€” new `default_config_candidate` helper and updated `Config::load`
- `docs/design.md` â€” Configuration section updated

## Residual risks / follow-up
- Tests are pure helper tests (no environment mutation), so no parallel-test flakiness risk.
- If XDG_CONFIG_HOME contains a path with non-UTF-8 characters on Windows, `to_string_lossy` will replace them with the replacement character for the whitespace check, but the actual PathBuf construction uses `as_os_str()` directly so the path itself is preserved correctly.
