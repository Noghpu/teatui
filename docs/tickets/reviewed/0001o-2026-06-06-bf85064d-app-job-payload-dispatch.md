---
id: 0001o-2026-06-06-bf85064d-app-job-payload-dispatch
created_at: 2026-06-06T10:31:48+02:00
created_by_model: gpt-5
state: reviewed
state_updated_at: 2026-06-06T11:20:19+02:00
---
# Refactor App Job Payload Dispatch

## Goal

Replace the repetitive `Box<dyn Any + Send>` downcast boilerplate in `App::absorb_payload` with a small centralized dispatch helper so adding a job result type does not require copy-pasting the full downcast/reassign/dirty/return pattern.

## Context

The worker runtime still delivers `JobOutcome::Done(Box<dyn Any + Send>)`, and `src/app.rs::absorb_payload` is the single consumer that downcasts job payloads into app state changes. That chain has grown well past the threshold noted in the Phase 1 post-mortem. Future PR and issue management modes will add more jobs, so this should be cleaned up before those modes start.

The runtime shape is otherwise working: workers catch panics, emit `JobEvent`, and keep the owner thread in charge of all app mutation. This ticket is about making the app-side payload dispatch less error-prone, not changing the owner-thread model.

## Non-Goals

- Do not change background job scheduling, worker count, panic recovery, or channel wiring.
- Do not move screen or domain mutation out of `App` handlers.
- Do not rewrite all job result types into one domain-wide enum.
- Do not implement PR or issue management jobs in this ticket.

## Design Decisions

Keep the runtime API unchanged for this ticket: `Job::run` continues to return `JobOutcome::Done(Box<dyn Any + Send>)`. The low-risk extraction is a private helper or macro near `App::absorb_payload` that handles the repeated typed downcast pattern and calls a typed closure.

The helper must preserve the current behavior:

- the first matching payload type invokes the same handler body as today
- successful handling sets `self.dirty = true`
- stale phase guards and follow-up job submission behavior remain in the typed handlers
- an unknown payload still logs a warning with the job name

After this lands, adding another payload type should require only a one-line helper invocation plus the typed handler logic, not the full `match any.downcast::<T>() { Ok(..), Err(..) }` scaffold.

## Implementation Plan

1. Add a private `try_payload!` macro or equivalent helper in `src/app.rs` close to `absorb_payload`.
2. Convert every existing arm in `App::absorb_payload` to use the helper.
3. Keep all existing typed handler bodies and ordering intact unless the ordering is provably irrelevant.
4. Update the background-job note in `AGENTS.md` if the implementation changes how new payload handlers should be added.
5. Run the existing app/runtime tests to confirm job event behavior and app state handling still work.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": ["AGENTS.md", "docs/rewrite-plan.md"],
  "likely_files": [
    "src/app.rs",
    "src/runtime/jobs.rs",
    "src/domain/probe.rs",
    "src/domain/context.rs",
    "src/domain/llm.rs",
    "src/domain/execute.rs",
    "src/domain/jj_mutate.rs"
  ],
  "verification_commands": ["just verify"],
  "review_focus": [
    "No behavior changes in typed payload handlers",
    "Unknown payloads still log instead of panicking",
    "Dirty flag behavior matches the old successful-handler behavior",
    "Runtime worker API remains app-agnostic"
  ],
  "jj_description_prefix": "refactor"
}
```

## Acceptance Criteria

- `App::absorb_payload` no longer repeats the full downcast/reassign scaffold for each payload type.
- Existing job result types are still handled with the same side effects as before.
- Unknown payload handling remains non-panicking and logged.
- The worker runtime remains independent of `App` and screens.
- `just verify` passes.

## Verification Plan

Run `just verify`. If the helper shape changes app/job boundaries beyond `app.rs`, also inspect the runtime job tests and add or update focused tests only if an existing behavior is no longer covered.

## Files Likely Touched

- `src/app.rs`
- `AGENTS.md` only if the documented background-job workflow changes

## Risks

- Accidentally moving `self.dirty = true` outside the successful payload path could cause unnecessary redraws or missed redraws.
- Reordering handlers could affect payloads that share wrapper types or stale-result guards.
- A trait-based design could couple the generic runtime to `App`; keep this ticket local unless there is a clearly smaller design.
---

<!-- ticket-section:review-postmortem v1 -->
## Review Postmortem

Metadata:
- model: gpt-5 medium
- reviewed_at: 2026-06-06T11:20:19+02:00
- state: reviewed

# Review Postmortem

## Findings

- Fact: `src/app.rs` adds a private `try_payload!` macro near `App::absorb_payload` and converts the existing downcast chain to one invocation per payload type.
- Fact: The converted arms preserve the previous payload ordering and the previous typed handler bodies for the job result types in this ticket's scope.
- Fact: Inline status/screen mutation arms still set `self.dirty = true`; delegating arms still let their typed handlers own dirty-flag changes and stale-result guards.
- Fact: Unknown payloads still fall through to the existing non-panicking `tracing::warn!` path.
- Fact: The runtime job API remains type-erased and app-agnostic; the implementation diff is limited to `src/app.rs`, `AGENTS.md`, and the ticket state move.
- Inference: No extra focused tests are needed for this mechanical refactor because the existing compile, lint, runtime, app, and render coverage exercises the unchanged behavior surface.

## Verification

- `just verify` passed: fmt, check, clippy with `-D warnings`, 189 unit tests, and 51 render smoke tests.
