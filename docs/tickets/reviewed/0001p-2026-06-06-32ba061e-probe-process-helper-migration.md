---
id: 0001p-2026-06-06-32ba061e-probe-process-helper-migration
created_at: 2026-06-06T10:32:16+02:00
created_by_model: gpt-5
state: reviewed
state_updated_at: 2026-06-06T11:38:35+02:00
---
# Migrate Probe Subprocesses To Domain Process Helpers

## Goal

Move the remaining ad-hoc subprocess handling in `src/domain/probe.rs` onto `src/domain/process.rs` so probe jobs share one implementation for nulled stdin, captured stdout/stderr, `jj --no-pager`, and formatted command errors.

## Context

`domain::process` now provides `capture`, `jj`, and `tea` helpers, but `domain::probe` still contains several direct `Command::new(...).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output()` call sites. The old AGENTS note deferred this while the helper did not exist. The helper now exists, and the mechanical probe call sites should be folded into it before future management jobs add more `tea` and `jj` shell-outs.

Some probe behavior is intentionally heterogeneous and must be preserved. `VersionProbe` distinguishes a missing binary from an errored command. `WorkspaceProbe` treats nonzero `jj workspace root` as `WorkspaceInfo::Outside`. `RevsetStatsProbe` deliberately falls back to empty stats on failure. Those are semantic decisions, not boilerplate to erase.

## Non-Goals

- Do not change the public shape of probe result types.
- Do not change parsing templates, revsets, repository-option endpoints, or cache semantics.
- Do not migrate unrelated non-probe shell-outs unless required to keep `domain::process` coherent.
- Do not implement PR or issue management commands in this ticket.

## Design Decisions

Extend `domain::process` with the smallest low-level helper needed to preserve probe-specific status handling, for example `output(binary, args) -> io::Result<Output>` that applies the standard stdin/stdout/stderr setup. Keep `capture`, `jj`, and `tea` as the convenient success-or-error-string layer, building on that low-level helper if practical.

Use `process::jj` or `process::tea` for call sites that only need successful stdout or a formatted error. Use the low-level output helper for call sites that must inspect `io::ErrorKind`, exit status, stderr, or stdout separately.

## Implementation Plan

1. Add a low-level `domain::process` helper that returns `std::process::Output` while standardizing stdin nulling and output capture.
2. Keep or update the existing `capture`, `jj`, and `tea` tests so their formatted error behavior remains covered.
3. Migrate `VersionProbe`, `WorkspaceProbe`, `origin_remote_url`, `TeaAuthProbe`, `RevsetProbe`, `RevsetStatsProbe`, base bookmarks, existing PR precheck, and repo-options API calls in `src/domain/probe.rs` to use `domain::process` helpers.
4. Preserve each probe's current parsing and error/fallback semantics exactly unless a test exposes a real bug.
5. Remove now-unused direct imports of `Command` or `Stdio` from `probe.rs`.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/rewrite-plan.md"],
  "likely_files": [
    "src/domain/process.rs",
    "src/domain/probe.rs",
    "src/domain/mod.rs"
  ],
  "verification_commands": ["just verify"],
  "review_focus": [
    "VersionProbe still reports missing binaries as ToolStatus::Missing",
    "WorkspaceProbe still maps nonzero workspace-root output to WorkspaceInfo::Outside",
    "RevsetStatsProbe still falls back rather than surfacing an app-blocking error",
    "All jj calls still include --no-pager exactly once",
    "No parser or endpoint behavior changed while removing Command boilerplate"
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria

- `src/domain/probe.rs` no longer directly repeats the standard `Command` setup for stdin/stdout/stderr capture.
- `domain::process` exposes both the existing convenience helpers and any low-level status-preserving helper needed by probes.
- Existing probe result semantics are preserved.
- Unused imports are removed.
- `just verify` passes.

## Verification Plan

Run `just verify`. Pay particular attention to unit tests around process helpers, probe parsing, and render smoke tests that exercise probe failure states.

## Files Likely Touched

- `src/domain/process.rs`
- `src/domain/probe.rs`

## Risks

- Flattening `io::ErrorKind::NotFound` into a formatted command error would regress missing-tool status display.
- Treating `jj workspace root` nonzero output as a generic error would regress outside-workspace behavior.
- Accidentally adding `--no-pager` twice would break jj invocations.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5 medium
- reviewed_at: 2026-06-06T11:38:35+02:00
- state: reviewed

# Review Postmortem

## Findings

- Fact: The implemented ticket removed direct `Command`/`Stdio` setup from `src/domain/probe.rs` and routed probe subprocess execution through `domain::process` helpers.
- Fact: `process::output` correctly preserves `io::Result<Output>`, allowing missing binaries, exit status, stdout, and stderr to remain distinguishable for probes with heterogeneous semantics.
- Fact: The original implementation still left raw jj probe calls split between manual `--no-pager`, no `--no-pager`, and `process::jj`; this met much of the mechanical migration goal but did not centralize jj invocation policy as cleanly as the ticket and repo conventions call for.
- Fix: Added `process::jj_output`, backed by the same `jj_args` builder as `process::jj`, then migrated raw-output jj probes (`workspace root`, `git remote list`, and `RevsetProbe`) to it. This preserves missing-binary/status handling while ensuring jj probes receive `--no-pager` exactly once from the process layer.
- Fix: Added focused tests for the risky semantic cases: raw `jj_output` not-found propagation, `VersionProbe` missing-tool classification, `WorkspaceProbe` missing-jj classification, and `RevsetStatsProbe` fallback on missing jj.
- Fact: No ratatui rendering or input code changed in this ticket. The refactor stays in worker-side domain jobs, which matches the repo's TUI architecture: subprocess work remains off the owner/render path.
- Inference: The final shape is closer to the Rust implementation I would want to maintain: subprocess boilerplate is centralized, jj-specific policy is not repeated at call sites, probe-specific semantics remain explicit at the typed boundary, and tests pin the non-obvious behavior.

## Verification

- `cargo test domain::process` passed.
- `cargo test domain::probe` passed.
- `just verify` passed: fmt, check, clippy with `-D warnings`, 194 unit tests, and 51 render smoke tests.
