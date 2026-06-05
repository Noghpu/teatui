---
id: 0001n-2026-06-05-142fb95d-domain-process-subprocess-capture
created_at: 2026-06-05T22:11:04+02:00
created_by_model: claude-opus-4-8/xhigh
state: reviewed
state_updated_at: 2026-06-05T22:47:26+02:00
---
# Consolidate jj/tea subprocess capture into a shared domain::process module

## Goal
Three byte-for-byte-equivalent subprocess capture helpers exist in the domain layer: `run_jj` in `context.rs`, `jj` + `tea` + `run_capture` in `execute.rs`, and `jj` in `jj_mutate.rs`. Each builds a `Command`, nulls stdin, pipes stdout/stderr, runs `output()`, and formats `"{binary} {args:?}: {body}"` on failure. They have already drifted (the `context.rs` copy reports only stderr; the others fall back to stdout when stderr is empty). Consolidate them into one `domain::process` module so every shell-out shares one capture implementation and one error format â€” and so the upcoming PR/issue comment fetch/post jobs inherit it for free.

## Context
The active rewrite is Linux-only and shells out to `jj` and `tea`. Read `AGENTS.md` and `docs/rewrite-plan.md` first. Every `jj` invocation must pass `--no-pager` (enforced today by each helper prepending it).

The three duplicates:
- `context.rs:333` â€” `fn run_jj(jj: &str, args: &[&str]) -> Result<String, String>`. Prepends `--no-pager`. On failure formats `"{jj} {args:?}: {stderr}"` (stderr only).
- `execute.rs:146-157,215-229` â€” `fn jj(&str, &[String])` and `fn tea(&str, &[String])` delegating to `fn run_capture(Command, &str, &[String])`. On failure uses `body = if !stderr.is_empty() { stderr } else { stdout }`.
- `jj_mutate.rs:133-150` â€” `fn jj(&str, &[String])`, identical to `execute.rs`'s `jj` + `run_capture` (stderr-or-stdout fallback).

The stderr-or-stdout fallback (execute/jj_mutate) is the more robust behavior; adopting it everywhere strictly improves `context.rs`'s error messages without changing success-path output.

`tea` differs from `jj` only in not prepending `--no-pager`.

Call sites: `execute.rs` calls `jj(...)`/`tea(...)` with `Vec<String>` argument lists built by `bookmark_args`/`push_args`/`tea_create_args`; `jj_mutate.rs` calls its `jj(...)` with `Vec<String>` from `JjOp::command_args` and an inline conflict-probe arg list; `context.rs` calls `run_jj(jj, &["...","..."])` with `&str` literals.

Domain jobs and their `Command` usage live under `src/domain/` (see `src/domain/mod.rs` for the module list).

## Non-Goals
- Do not change any command's arguments, success output, or externally-visible behavior â€” only the shared plumbing and (for `context.rs`) the failure-message body, which becomes stderr-or-stdout.
- **Do not refactor `probe.rs`'s ~10 ad-hoc `Command` sites in this ticket.** Their success checks and output parsing are heterogeneous (some ignore exit status, some parse JSON, some return `Option`); folding them in risks behavior drift. A low-level `process::output(...) -> io::Result<Output>` helper can absorb the purely-mechanical ones in a later pass.
- Do not add new dependencies. Use `std::process`.
- Do not touch the `screens` layer (tickets T1/T2).
- Do not change the `Job` trait, `JobOutcome`, or `App::absorb_payload` wiring.

## Design Decisions
Create `src/domain/process.rs`, registered in `src/domain/mod.rs` (e.g. `pub(crate) mod process;`). Make the capture generic over `AsRef<str>` so both `&[String]` and `&[&str]` call sites pass their existing argument slices with no conversion:

```rust
use std::process::{Command, Stdio};

/// Run `binary` with `args` â€” stdin nulled, stdout/stderr captured. On success
/// returns stdout. On failure returns `"{binary} {args:?}: {body}"` where `body`
/// is the trimmed stderr, or trimmed stdout when stderr is empty.
pub(crate) fn capture<S: AsRef<str>>(binary: &str, args: &[S]) -> Result<String, String> {
    let shown: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let out = Command::new(binary)
        .args(&shown)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{binary} {shown:?}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let body = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("{binary} {shown:?}: {body}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `capture` with `--no-pager` prepended â€” the standard jj invocation.
pub(crate) fn jj<S: AsRef<str>>(binary: &str, args: &[S]) -> Result<String, String> {
    let mut all: Vec<&str> = Vec::with_capacity(args.len() + 1);
    all.push("--no-pager");
    all.extend(args.iter().map(AsRef::as_ref));
    capture(binary, &all)
}

/// `capture` for `tea` (no `--no-pager`).
pub(crate) fn tea<S: AsRef<str>>(binary: &str, args: &[S]) -> Result<String, String> {
    capture(binary, args)
}
```

`{shown:?}` formats as `["--no-pager", "squash", â€¦]` â€” equivalent to today's `{args:?}` on `&[&str]`/`&[String]`, so error strings are unchanged in shape. (For `jj`, the message now includes the leading `--no-pager`, matching the actual argv; this is a minor, acceptable refinement to the error text â€” call it out in the description.)

Migration:
- `context.rs`: delete `run_jj`; call `process::jj(jj_binary, &["â€¦", "â€¦"])` at its sites (the `&str` literal slices satisfy `AsRef<str>`).
- `execute.rs`: delete `jj`, `tea`, `run_capture`; replace calls with `process::jj` / `process::tea`. The `Vec<String>` arg lists pass directly.
- `jj_mutate.rs`: delete `jj`; replace calls with `process::jj`.

Keep `bookmark_args`/`push_args`/`tea_create_args`/`JjOp::command_args` exactly as they are â€” they build the argument vectors and are independently unit-tested.

## Implementation Plan
1. Create `src/domain/process.rs` with `capture`, `jj`, `tea` as above; register the module in `src/domain/mod.rs`.
2. Add unit tests in `process.rs` using a cross-platform always-present binary or a tiny success/exit-1 invocation (e.g. run the repo's own `jj`/`tea` is not guaranteed in CI â€” prefer a portable check such as capturing a known-success command, or assert the error-format string shape via a guaranteed-missing binary path so `map_err` fires). Keep tests hermetic and Linux-friendly.
3. Migrate `context.rs` (`run_jj` â†’ `process::jj`), confirming the diff-range builder still returns the same strings.
4. Migrate `execute.rs` (`jj`/`tea`/`run_capture` â†’ `process::jj`/`process::tea`), keeping all callers and the arg-builder functions unchanged.
5. Migrate `jj_mutate.rs` (`jj` â†’ `process::jj`), including the conflict-probe call.
6. `just verify`. Confirm `context.rs`, `execute.rs`, and `jj_mutate.rs` unit tests still pass and no `Command` capture boilerplate remains in those three files.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/rewrite-plan.md",
    "src/domain/execute.rs",
    "src/domain/context.rs",
    "src/domain/jj_mutate.rs",
    "src/domain/mod.rs"
  ],
  "likely_files": [
    "src/domain/process.rs",
    "src/domain/mod.rs",
    "src/domain/execute.rs",
    "src/domain/context.rs",
    "src/domain/jj_mutate.rs"
  ],
  "verification_commands": [
    "just verify"
  ],
  "review_focus": [
    "A single domain::process::capture (plus jj/tea wrappers) replaces run_jj (context.rs), jj/tea/run_capture (execute.rs), and jj (jj_mutate.rs).",
    "Error format is preserved as `{binary} {args:?}: {stderr-or-stdout}`; context.rs failures now use the stderr-or-stdout fallback (improvement, no success-path change).",
    "AsRef<str> generic lets &[String] and &[&str] call sites pass their existing slices with no conversion.",
    "Arg-builder functions (bookmark_args/push_args/tea_create_args/command_args) are unchanged and their tests still pass.",
    "probe.rs was intentionally left alone; no parsing/success semantics changed there."
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria
- A new `domain::process` module exposes `capture`, `jj`, and `tea`; `jj` prepends `--no-pager`, `tea` does not.
- `context.rs::run_jj`, `execute.rs::{jj,tea,run_capture}`, and `jj_mutate.rs::jj` are deleted; those files contain no `Command`/`Stdio` capture boilerplate.
- The shared helper accepts both `&[String]` and `&[&str]` argument slices (`AsRef<str>`).
- Failure messages keep the `"{binary} {args:?}: {body}"` shape with the stderr-or-stdout body everywhere (including the former `context.rs` site).
- Argument-builder functions and their existing unit tests are unchanged and pass.
- `probe.rs` is untouched.
- `just verify` passes.

## Verification Plan
Run `just verify` (fmt, check, clippy `-D warnings`, unit + render smoke). Confirm the existing `jj_mutate.rs` `command_args` tests, `execute.rs`/`context.rs`/`stack.rs` tests, and the live integration coverage in `tests/llama_integration.rs` (if exercised) still pass. Grep `src/domain/{context,execute,jj_mutate}.rs` to confirm no `Stdio::piped()` capture boilerplate remains.

## Files Likely Touched
- `src/domain/process.rs` (new)
- `src/domain/mod.rs`
- `src/domain/context.rs`
- `src/domain/execute.rs`
- `src/domain/jj_mutate.rs`

## Risks
- **Error-text shift for `jj`.** Including the prepended `--no-pager` in the `{args:?}` portion of failure messages is a minor, intended change. If any test asserts on the exact failure string, update it to match the shared format.
- **`context.rs` body change.** Switching `context.rs` failures from stderr-only to stderr-or-stdout can surface stdout text that was previously dropped; this is an improvement but verify no test pins the old stderr-only message.
- **Generic plumbing.** `AsRef<str>` keeps call sites clean, but double-check the `--no-pager` prepend builds the argv in the right order (flag first, then the caller's args) for both `jj` wrappers.
- **Scope discipline.** Resist migrating `probe.rs` here; its heterogeneous success/parse handling is the reason it is explicitly deferred.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: codex-gpt5
- completed_at: 2026-06-05T22:47:10+02:00
- state: implemented

Completed:
- Added `domain::process` with `capture`, `jj`, and `tea` helpers.
- Registered `domain::process` in `domain::mod`.
- Replaced local subprocess capture wrappers in `context.rs`, `execute.rs`, and `jj_mutate.rs`.
- Left `probe.rs` untouched.
- Added hermetic tests for error-format shape, `jj` `--no-pager` prefixing, and `tea` accepting `Vec<String>` args.

Deviations:
- None. `jj` failure messages now include the prepended `--no-pager`, as specified by the ticket.

Verification:
- `just verify` passed.
- Grep confirmed no `Command`/`Stdio` capture boilerplate remains in `context.rs`, `execute.rs`, or `jj_mutate.rs`.

Files changed:
- `src/domain/process.rs`
- `src/domain/mod.rs`
- `src/domain/context.rs`
- `src/domain/execute.rs`
- `src/domain/jj_mutate.rs`

Residual risks:
- Failure text for former `context.rs` sites now uses stderr-or-stdout fallback and includes the actual `--no-pager` argv prefix; this is intentional and limited to error messages.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: codex-gpt5-self-reviewed
- reviewed_at: 2026-06-05T22:47:26+02:00
- state: reviewed

Reviewed immediately per user instruction.

Findings:
- No functional issue found in the process helper consolidation.
- `domain::process::capture` centralizes stdin/stdout/stderr handling and the stderr-or-stdout failure body.
- `domain::process::jj` prepends `--no-pager`; `tea` does not.
- `context.rs`, `execute.rs`, and `jj_mutate.rs` no longer define local capture wrappers.
- Existing argument-builder functions were not changed, and `probe.rs` was left alone.

Verification:
- `just verify` passed before review finalization.
- Grep confirmed capture boilerplate remains only in `domain::process` among the targeted files.

Residual risk:
- No separate reviewer agent was run; this ticket was treated as reviewed immediately because the user requested it.
