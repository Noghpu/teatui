---
id: 0001t-2026-06-07-be230561-forge-autodetection-ux
created_at: 2026-06-07T10:13:53+02:00
created_by_model: claude-sonnet-4-6/medium
state: reviewed
state_updated_at: 2026-06-07T11:06:42+02:00
---
# Forge Autodetection UX: Show Which Forge Is Active on the Landing Screen

## Goal

Make it immediately obvious which forge was detected and is being used, including the detection source (auto vs. configured), the remote host, and the auth status (logged-in accounts).

## Context

The landing footer currently shows a single chip like `âœ“ tea` or `âœ“ gh` for the forge. This comes from `status.forge_label` (the CLI binary label) and `status.forge` (the tool availability check). The user cannot tell:

1. Whether the forge was autodetected or manually configured (`pr.forge = "auto"` vs. `"github"` vs. `"gitea"`).
2. Which host the forge is pointing at (e.g., `gitea.example.com` vs. `github.com`).
3. Who is authenticated (the logins from `ForgeAuthStatus::Configured { logins }`).

The relevant data already exists in the app:
- `config.pr.forge` â€” `ForgeSelection` enum (`Auto`, `Gitea`, `Github`).
- `status.workspace` â€” contains `RemoteInfo { host, owner, repo }` when inside a jj workspace.
- `status.forge_auth` â€” `Cached<ForgeAuthStatus>` with `Configured { logins }`, `None`, or `Errored`.
- `status.forge_label` â€” the CLI label ("tea" or "gh").
- `status.forge` â€” `Cached<ToolStatus>` (available/missing/errored).

`render_landing_footer` in `src/screens/landing.rs` calls `push_tool` for the forge chip, which only shows the binary label + health symbol.

## Non-Goals

- No changes to detection logic â€” only the display layer.
- No changes to the `StatusStore` data model; all needed data is already there.
- No changes to the generate screen (separate ticket).

## Design Decisions

**What to show in the forge chip:**

Replace the single muted `âœ“ tea` chip with a richer inline display:

```
âœ“ tea Â· gitea.example.com Â· @alice
```

- Binary label ("tea" or "gh") stays as the anchor.
- If workspace has a remote host AND the host is non-trivial (not "github.com" when using gh), show `Â· host`.
- If `ForgeAuthStatus::Configured { logins }` and logins is non-empty, show `Â· @first_login`. Truncate to one login to keep it compact.
- If `ForgeSelection::Auto`, prefix with `auto:` when the terminal is wide enough, e.g. `auto: tea Â· gitea.example.com Â· @alice`.  When narrow, omit the prefix.
- Auth `None` â†’ no login suffix. Auth `Errored` â†’ show `Â· auth error` in error style.
- If `ForgeSelection` is `Gitea` or `Github` (manual), no `auto:` prefix â€” it's implicit.
- Host is shown as-is from `RemoteInfo.host`. If workspace is Outside or no remote, omit host.

**Width budget:** The landing footer already lays out chips inline with spaces. The existing `push_tool` call is replaced with a new `push_forge` function in `landing.rs` that builds the richer span sequence. No layout changes needed.

**Implementation approach:** Extract a `push_forge` function in `landing.rs`. It takes `status: &StatusStore` and `forge_selection: ForgeSelection` (passed from app state via `render` â†’ `render_landing_footer`). The `render` function for landing already receives `status: &StatusStore` but does not currently receive the config. Two options:
1. Add `ForgeSelection` as a parameter to `render`.
2. Store `ForgeSelection` in `StatusStore`.

Option 1 is cleaner â€” the landing render is called from `App::render` which has access to `self.config.pr.forge`. Pass it alongside `status`.

**Signature change:** `landing::render(state, status, frame, area)` â†’ `landing::render(state, status, forge_selection, frame, area)`. Update the call site in `src/screens/mod.rs` or wherever `landing::render` is dispatched from.

## Implementation Plan

1. In `src/screens/landing.rs`:
   - Add `ForgeSelection` import from `crate::config`.
   - Change `render` signature to accept `forge_selection: ForgeSelection`.
   - Replace the `push_tool(...forge_label..., &status.forge)` call inside `render_landing_footer` with a call to a new `push_forge` function.
   - Implement `push_forge(spans, status, forge_selection)`:
     - Determine health symbol/style from `status.forge` (same as `tool_health`).
     - Build label: `{binary_label}` + optional ` Â· {host}` + optional ` Â· @{login}` or ` Â· auth error`.
     - If `forge_selection == ForgeSelection::Auto`, prepend `auto: ` to the label.
     - Auth error uses `theme::error()` for that segment only; host uses `theme::muted()`.

2. In `src/app.rs` (or `src/screens/mod.rs`):
   - Update the call to `landing::render` to pass `self.config.pr.forge`.

3. Snapshot tests in `src/bin/ui-snapshots.rs`:
   - Update any existing landing snapshot that checks the forge chip text.
   - Add new snapshot variants: `auto` with host+login, `manual` without prefix, `auth error` state.

## Acceptance Criteria

- Landing footer shows `auto: tea Â· gitea.example.com Â· @alice` when forge=auto, workspace has a gitea remote, and auth is configured.
- Landing footer shows `gh Â· github.com Â· @bob` (no `auto:` prefix and host shown for github.com) when forge=auto and remote is github.com.  Actually: for github.com remotes, always show the host since it confirms which org/host is active â€” no special-casing.
- Landing footer shows `tea` (no host, no login) when workspace has no remote or is outside a jj workspace.
- Landing footer shows `gh` with `Â· auth error` in error style when forge auth probe returned `ForgeAuthStatus::Errored`.
- When forge selection is `Gitea` or `Github` (manual), no `auto:` prefix is shown.
- `cargo test` passes.
- UI snapshot tests updated.

## Verification Plan

1. `cargo test` â€” unit + snapshot tests green.
2. Run the TUI in a gitea-backed repo and confirm the footer shows forge name, host, and login.
3. Run the TUI in a github.com repo and confirm it shows `gh Â· github.com Â· @login`.
4. Run the TUI outside any jj workspace and confirm only the binary label appears.

## Files Likely Touched

- `src/screens/landing.rs` â€” main change: `push_forge` function + render signature.
- `src/app.rs` or `src/screens/mod.rs` â€” update call site to pass `forge_selection`.
- `src/bin/ui-snapshots.rs` â€” update/add snapshots.

## Risks

- Snapshot test churn if the landing footer snapshots are pixel-perfect string matches.
- Terminal width sensitivity: the chip can grow long; if the footer overflows, spans wrap or clip. Keep a max-length guard on the login suffix (truncate at ~20 chars).

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "src/screens/landing.rs", "src/app.rs", "src/config.rs", "src/domain/status_store.rs"],
  "likely_files": [
    "src/screens/landing.rs",
    "src/app.rs",
    "src/screens/mod.rs",
    "src/bin/ui-snapshots.rs"
  ],
  "verification_commands": ["cargo test"],
  "review_focus": ["push_forge correctness for all ForgeAuthStatus variants", "call site update completeness", "snapshot test coverage"],
  "jj_description_prefix": "ui"
}
```
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5.4-mini-medium
- completed_at: 2026-06-07T11:00:45+02:00
- state: implemented

Completed:
- Threaded ForgeSelection into Landing rendering so the footer can distinguish auto-detected vs manually configured forge selection.
- Replaced the flat forge tool chip with a richer chip showing health, optional auto prefix, remote host, first authenticated login, and auth-error state.
- Added render smoke assertions for auto/manual/auth-error/no-remote cases.
- Updated UI snapshot fixtures and added landing manual-forge and auth-error variants.

Deviations from plan:
- Kept the auto prefix width gate local to the footer render using the actual footer width; no StatusStore model changes were needed.

Verification:
- cargo test passed.
- just snapshots passed and regenerated target/ui-snapshots.
- just verify passed, including fmt, check, clippy -D warnings, and tests.

Files changed:
- src/screens/landing.rs
- src/app.rs
- src/bin/ui-snapshots.rs
- tests/render_smoke.rs

Residual risks or follow-up:
- The footer is still a single centered line, so very small terminals may clip long host/login text after the existing inline status chips.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5.5-medium
- reviewed_at: 2026-06-07T11:06:42+02:00
- state: reviewed

# Review Postmortem

Facts:
- Reviewed ticket 0001t-2026-06-07-be230561-forge-autodetection-ux against the implemented changes in src/screens/landing.rs, src/app.rs, src/bin/ui-snapshots.rs, and tests/render_smoke.rs.
- The implementation threads ForgeSelection into landing rendering and shows the forge CLI label, remote host, first configured login, and auth error state on the landing footer.
- Render smoke assertions cover auto, manual, auth-error, and no-remote/no-auth footer states.
- Review changed the auto-prefix width gate from a fixed footer-width threshold to a measured full-footer fit check, so `auto:` is omitted only when the assembled line would overflow.
- Added a render smoke assertion for an 80-column footer to pin the narrow-width omission behavior.
- Ran `cargo test landing_forge_chip --test render_smoke`; all 5 focused tests passed.
- Ran `just verify`; fmt, check, clippy -D warnings, and all tests passed.

Inferences:
- The measured fit check better matches the ticket's intent than the previous `footer_width >= 100` heuristic because the available space depends on the current LLM label, workspace chip, host, and login text.
- The remaining risk is normal footer clipping on extremely small terminals if even the no-prefix line is too long, which is consistent with the existing single-line footer behavior and outside this ticket's scope.
