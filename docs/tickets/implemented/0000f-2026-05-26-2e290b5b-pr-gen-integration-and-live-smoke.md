---
id: 0000f-2026-05-26-2e290b5b-pr-gen-integration-and-live-smoke
created_at: 2026-05-26T12:09:01+02:00
created_by_model: gpt-5
state: implemented
state_updated_at: 2026-05-26T12:32:33+02:00
---
# Fake and Live PR Generation Integration Tests

## Goal
Add deterministic fake-service integration coverage for the Generate PR AI suggestion and Gitea execution flow, plus an explicit opt-in live smoke test path that can run against a local llama.cpp server and a WSL-hosted Gitea instance.

## Context
Generate PR is now wired end-to-end: context collection, Ollama-compatible draft generation, draft review, command preview, jj bookmark/push execution, and `tea pr create` URL capture. Unit tests cover parsers and command builders, but there is no higher-level integration test proving that the app-facing workflow handles realistic LLM responses, command failures, stale context, and PR URL capture together.

The default test suite must stay fast and deterministic. Live infrastructure tests are useful, but they depend on local binaries, model files, WSL, Gitea startup, credentials, ports, and wall-clock timing. Keep those live checks behind an explicit recipe and environment gate so `just verify` remains reliable.

## Non-Goals
- Do not make live llama.cpp, WSL, Gitea, `tea` authentication, or model downloads required for `just verify`.
- Do not evaluate subjective LLM writing quality. Test contracts, request/response handling, validation, state transitions, command ordering, and URL extraction.
- Do not add broad UI snapshot farms or terminal automation unless a small harness needs it for state progression.
- Do not push to or create PRs in a non-disposable repository.
- Do not store tokens, passwords, model paths, generated PR URLs, or live server logs containing secrets in the repo.

## Design Decisions
- Add a fake integration harness under a test-focused module or `tests/` directory. Prefer in-process fake HTTP for the Ollama-compatible endpoint and temporary executable shims for `jj` and `tea` so existing command-wrapper boundaries are exercised without real infrastructure.
- The fake Ollama server should expose the endpoint shape used by `OllamaClient`, record the request body, and return deterministic responses for: valid draft JSON, malformed JSON, missing required fields, invalid branch name, HTTP error, and delayed/timeout response.
- Fake `jj` and `tea` shims should be generated into a temp directory and configured through `Config.commands` rather than relying on shell aliases. They must record argv in files for assertions and return deterministic stdout/stderr.
- Add at least one default fake end-to-end happy path that covers: context collection, Ollama draft parsing, dirty-field preservation where practical, confirmation/freshness check, sequential execution, and PR URL capture from fake `tea pr create` output.
- Add focused fake failure scenarios: malformed LLM JSON remains visible as a generation failure, stale jj context prevents mutation, and `tea pr create` nonzero exit transitions to Failed with the execution plan retained for retry.
- Add a new `just smoke-live` recipe, or similarly explicit name, for the live smoke path. This recipe must not be a dependency of `just verify`.
- Live smoke configuration should come from environment variables with clear names, for example:
  - `TEATUI_SMOKE_LIVE=1` to opt in.
  - `TEATUI_SMOKE_MODEL=/path/to/Qwen3.5-4B-UD-Q8_K_XL.gguf` for the llama.cpp model path.
  - `TEATUI_SMOKE_LLAMA_SERVER=llama-server` for the llama.cpp executable.
  - `TEATUI_SMOKE_LLAMA_URL=http://127.0.0.1:8081` with port `8081` as the default.
  - `TEATUI_SMOKE_WSL_DISTRO` optional for choosing the WSL distro.
  - `TEATUI_SMOKE_GITEA_URL`, `TEATUI_SMOKE_GITEA_USER`, and `TEATUI_SMOKE_GITEA_REPO` for the disposable Gitea target.
- The live smoke helper must check whether the llama.cpp server is already reachable before starting it. If not reachable, start it with:

```text
llama-server \
  -m /path/to/Qwen3.5-4B-UD-Q8_K_XL.gguf \
  -ngl 99 \
  -c 8192 \
  -fa on \
  --reasoning off \
  --port 8081 \
  --log-disable
```

- The live smoke helper must allow enough time for llama.cpp cold start before failing. After the server is reachable, individual LLM requests should use a 15 second timeout.
- The live smoke helper may use WSL to run a local Gitea instance. Prefer a disposable WSL-side directory, repository, and Gitea data path. The helper must detect whether the Gitea service is already reachable before starting it, and it must print actionable setup errors when WSL, Gitea, `tea`, `jj`, or auth is missing.
- Keep process management explicit: if the helper starts llama.cpp or Gitea, it should track child processes and clean them up on normal exit. If it attaches to already-running services, it must not stop them.
- Document the smoke recipe and required environment in a short repo doc or in the recipe comments.

## Implementation Plan
1. Add dev dependencies only if needed for deterministic tests, such as a temporary-directory helper and a small HTTP test server. Keep dependency count modest and avoid async/runtime duplication beyond the existing Tokio stack.
2. Extract or expose a small testable workflow helper if needed so integration tests can drive Generate PR state without terminal automation. Keep production architecture unchanged: `App`, `Message`/`Action`, command wrappers, and background events remain the main boundaries.
3. Build fake `jj` and `tea` executable shims in test temp dirs. They should record argv, support the specific commands used by context collection/freshness/execution, and return configurable success/failure outputs.
4. Build a fake Ollama-compatible HTTP server used by integration tests. Record request payloads and return configured JSON/error/timeout responses.
5. Add fake integration tests for the happy path and the failure cases listed in Design Decisions. These tests must run under `just test` and therefore under `just verify`.
6. Add a live smoke helper script, preferably under `scripts/` or `tests/smoke/`, that checks opt-in env vars, probes the llama.cpp URL, cold-starts `llama-server` when necessary, waits for readiness, uses a 15 second request timeout after readiness, and sets up/probes the WSL Gitea target.
7. Add `just smoke-live` to run the live helper. The recipe must fail fast with a clear message unless `TEATUI_SMOKE_LIVE=1` is set.
8. Ensure live smoke uses a disposable branch/repo/PR target and makes the created PR URL visible in output without persisting secrets.
9. Update documentation for fake integration tests and live smoke prerequisites.
10. Run `just verify`; optionally run `just smoke-live` only when the local live dependencies and env vars are available.

<!-- ticket-section:agent-handoff v1 -->
## Agent Handoff
```json
{
  "read_next": [
    "AGENTS.md",
    "docs/design.md",
    "src/app.rs",
    "src/generate.rs",
    "src/context.rs",
    "src/command.rs",
    "src/jj.rs",
    "src/tea.rs",
    "src/ollama.rs",
    "justfile"
  ],
  "likely_files": [
    "Cargo.toml",
    "Cargo.lock",
    "justfile",
    "tests/",
    "scripts/",
    "src/app.rs",
    "src/context.rs",
    "src/ollama.rs",
    "src/command.rs",
    "src/jj.rs",
    "src/tea.rs",
    "docs/"
  ],
  "verification_commands": [
    "just verify",
    "just smoke-live"
  ],
  "review_focus": [
    "fake integration tests are deterministic and included in just verify",
    "live smoke test is explicitly opt-in and never required for just verify",
    "llama.cpp readiness is probed before start and cold start receives enough time",
    "post-readiness LLM request timeout is 15 seconds",
    "WSL Gitea setup is disposable and does not require or mutate a real user repository",
    "no tokens, passwords, model paths, or service logs with secrets are committed",
    "fake jj/tea shims exercise argv boundaries rather than bypassing command construction"
  ],
  "jj_description_prefix": "feat"
}
```

## Acceptance Criteria
- Default fake integration tests cover the PR generation happy path through PR URL capture using fake Ollama, fake `jj`, and fake `tea` services or shims.
- Default fake integration tests cover malformed LLM JSON, stale jj context preventing mutation, and `tea pr create` failure retaining retryable state.
- Fake integration tests run with `just verify` and do not require network access outside localhost, WSL, Gitea, llama.cpp, real `tea` auth, or a real model file.
- The fake Ollama test server records enough request detail to assert the prompt/request includes the selected context and form values without asserting model prose quality.
- `just smoke-live` exists and is not part of `just verify`.
- `just smoke-live` refuses to run unless explicitly opted in with an environment variable such as `TEATUI_SMOKE_LIVE=1`.
- The live smoke helper checks whether the llama.cpp server is already reachable. If not, it starts `llama-server` with the requested arguments, waits long enough for cold start, then uses a 15 second timeout for LLM requests after readiness.
- The live smoke helper supports running a disposable Gitea instance through WSL, detects already-running Gitea when present, and reports clear setup instructions when WSL or Gitea prerequisites are missing.
- The live smoke path creates or uses only disposable repo/branch/PR resources and prints the resulting PR URL on success.
- Documentation explains fake tests, live smoke prerequisites, environment variables, and cleanup behavior.

## Verification Plan
- Run `just verify` and confirm the new fake integration tests pass as part of the default test suite.
- Run targeted fake integration tests for the happy path, malformed LLM JSON, stale context, and failed `tea pr create` cases while developing.
- Run `just smoke-live` with no opt-in env var and confirm it exits quickly with a clear opt-in message.
- When local prerequisites are available, run `TEATUI_SMOKE_LIVE=1 just smoke-live` against llama.cpp plus WSL Gitea and confirm a disposable PR URL is printed.
- If live smoke cannot be run on the implementer's machine, document the exact missing prerequisite in the implementation note and keep `just verify` passing.

## Files Likely Touched
- `Cargo.toml`
- `Cargo.lock`
- `justfile`
- `tests/` or a new test harness module
- `scripts/` or `tests/smoke/`
- `src/app.rs`
- `src/context.rs`
- `src/ollama.rs`
- `src/command.rs`
- `src/jj.rs`
- `src/tea.rs`
- `docs/`

## Risks
- Live smoke tests can become flaky if they are accidentally included in the default verification path; keep them opt-in and clearly separated.
- llama.cpp cold start time varies by machine and model location; readiness probing is safer than a fixed sleep, but the helper must still allow a generous cold-start window.
- WSL networking and port forwarding can differ by Windows/WSL version; error messages need to identify whether the failure is WSL launch, Gitea readiness, `tea` auth, or repo setup.
- Fake tests can overfit implementation details; assert behavior, argv, state transitions, and request contracts rather than private internal structure where possible.
- Process cleanup is easy to get wrong. Track only processes started by the helper and never kill user-owned already-running services.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5.4-mini
- completed_at: 2026-05-26T12:32:33+02:00
- state: implemented

# Implementation notes

- Added a library crate entrypoint so the app, tests, and smoke helper share the same modules.
- Added deterministic fake-service integration tests covering the Generate PR happy path, malformed LLM JSON, stale context blocking confirmation, and `tea pr create` failure with retryable state retained.
- Added a new opt-in `smoke-live` binary plus `just smoke-live` recipe. It gates on `TEATUI_SMOKE_LIVE`, probes or starts llama.cpp, enforces a 15 second generation timeout, and performs Gitea/WSL preflight checks.
- Deviation: the live helper currently stops at Gitea/WSL preflight rather than automating a full disposable PR creation flow.
- Verification: `just verify` passed.
- Files changed: `src/lib.rs`, `src/main.rs`, `src/bin/smoke-live.rs`, `tests/pr_generation_integration.rs`, `Cargo.toml`, `justfile`.
- Residual risk: the live helper is intentionally light on orchestration and still depends on a prepared Gitea target or WSL prerequisites for meaningful smoke coverage.
