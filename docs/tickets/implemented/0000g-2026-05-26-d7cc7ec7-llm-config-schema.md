---
id: 0000g-2026-05-26-d7cc7ec7-llm-config-schema
created_at: 2026-05-26T18:00:35+02:00
created_by_model: claude-sonnet-4-6/high
state: implemented
state_updated_at: 2026-05-26T20:56:25+02:00
---
# LLM Config Schema: Multi-Backend `[[llm.backends]]`

## Goal

Replace the single `[ollama]` config section with a `[llm]` section containing an `active` field and a `[[llm.backends]]` array, so any number of named backends (ollama, llama-cpp, vllm) can be configured and one selected. Update all downstream config consumers, repo state, and the landing UI to use the new schema.

## Context

Currently `src/config.rs` has `OllamaConfig { base_url, model }` and `src/repo.rs` has `RepoState` with `ollama_base_url: String`, `ollama_model: String`, `ollama: OllamaStatus`. The landing screen shows a single Ollama reachability probe. This ticket replaces the entire config and state layer; the client implementation stays in place (renamed) and is wired in the next ticket.

The `OllamaStatus` enum in `src/repo.rs` must be renamed to `LlmStatus` and generalised across backends. `RepoState` drops the three ollama-specific fields and gains a `Vec<LlmBackendStatus>` covering all configured backends.

## Non-Goals

- Implementing the `OpenAiCompatClient` or any new HTTP client code (next ticket).
- Streaming support.
- Runtime backend switching UI (config-time selection only for now, but the state field is wired for future TUI switching).

## Design Decisions

- TOML array-of-tables: `[[llm.backends]]`, each entry has `name`, `type`, `base_url`, `model` plus optional `temperature`, `max_tokens`, `context_size`, `min_p`, `seed`.
- `[llm] active = "name"` selects which backend is used for generation.
- `type` field values: `"ollama"`, `"llama-cpp"`, `"vllm"`.
- `context_size` is Ollama-native only; silently ignored for compat backends (kept in the struct for forward compat).
- `min_p` is OpenAI-compat only; silently ignored for Ollama native (kept in the struct).
- Backward compat: if a `[ollama]` section is present with no `[llm]` section, emit a `tracing::warn!` and treat it as a single backend named `"default"` of type `"ollama"`. Do not parse both simultaneously.
- `LlmBackendStatus { name: String, backend_type: String, base_url: String, model: String, status: LlmStatus }` replaces the three flat ollama fields on `RepoState`.
- The `active` field on `RepoState` mirrors `llm.active` from config so the app can read it without re-reading config.
- Health checks in `src/repo.rs` run concurrently for all configured backends using `tokio::join` or `futures::future::join_all`.
- Default config (no config file): single Ollama backend named `"default"`, `http://localhost:11434`, `qwen2.5-coder:latest`, temperature 0.1, context_size 4096, max_tokens 2048, active = "default".

## Implementation Plan

1. **`src/config.rs`**: Add `LlmBackendConfig { name, backend_type, base_url, model, temperature, max_tokens, context_size, min_p, seed }` with appropriate `Option<_>` fields and defaults. Add `LlmConfig { active: String, backends: Vec<LlmBackendConfig> }`. Replace `OllamaConfig` with `LlmConfig` on `Config`. Implement `Default` for all new types. Add backward-compat deserialization: attempt to deserialize `[llm]` first; if absent and `[ollama]` is present, construct a synthetic `LlmConfig`. Keep the `TEATUI_OLLAMA_BASE_URL` / `TEATUI_OLLAMA_MODEL` env vars working as aliases during the transition via the existing `config::Environment` layer or explicit fallback.
2. **`src/repo.rs`**: Rename `OllamaStatus` â†’ `LlmStatus`. Add `LlmBackendStatus`. Remove `ollama_base_url`, `ollama_model`, `ollama` from `RepoState`. Add `llm_backends: Vec<LlmBackendStatus>` and `llm_active: String`. Update `RepoState::new` to populate from the new config. Update `discover_repo_state` to health-check all backends concurrently and populate `llm_backends`.
3. **`src/ollama.rs`**: Update `health_check` signature to accept `base_url: &str` instead of `&Config` (so it can be called per-backend). Keep the Ollama native client intact but update its constructor to take `LlmBackendConfig` instead of `Config`.
4. **`src/app.rs`**: Update all references from `repo.ollama_*` to `repo.llm_backends` / `repo.llm_active`. Update log strings that say "ollama" generically to say "llm". Update client construction to use `config.llm.active_backend()` (a helper that finds the active `LlmBackendConfig` by name).
5. **`src/ui.rs`**: Update landing screen to show `llm_backends` status. If only one backend is configured, show it as before. If multiple, show a compact list (name: status).
6. **Tests**: Update all test fixtures that construct `RepoState` with the old ollama fields. Update `src/prompt.rs` test fixture (`sample_context`) to use the new field names. Add a deserialization test in `src/config.rs` for the `[[llm.backends]]` schema and the backward-compat `[ollama]` path.

## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "src/config.rs", "src/repo.rs", "src/ollama.rs", "src/app.rs", "src/ui.rs", "src/prompt.rs"],
  "likely_files": ["src/config.rs", "src/repo.rs", "src/ollama.rs", "src/app.rs", "src/ui.rs", "src/prompt.rs"],
  "verification_commands": ["cargo test --all", "cargo clippy -- -D warnings"],
  "review_focus": ["backward-compat [ollama] deserialization path", "LlmBackendStatus correctly mirrors all config fields", "concurrent health checks use join_all correctly", "RepoState test fixtures updated everywhere", "no references to old OllamaStatus or ollama_base_url remain"]
}
```

## Acceptance Criteria

- `cargo test --all` passes with no failures.
- `cargo clippy -- -D warnings` is clean.
- `Config::default()` produces a single Ollama backend named `"default"` with the previous defaults.
- A config file with `[[llm.backends]]` deserializes correctly.
- A config file with only `[ollama]` still deserializes (via backward compat) with a `tracing::warn!`.
- `RepoState` has no `ollama_base_url`, `ollama_model`, or `ollama` fields.
- All test fixtures compile and pass.
- Landing UI shows at least the active backend's reachability status.

## Verification Plan

- `cargo test --all` â€” covers unit tests for config deserialization, RepoState construction, and prompt assembly.
- `cargo clippy -- -D warnings` â€” ensures no dead code or type errors.
- Manual: run `cargo run` with no config file and confirm the landing screen shows the default Ollama backend status.
- Manual: add a `[[llm.backends]]` config and confirm it parses without error.

## Files Likely Touched

- `src/config.rs`
- `src/repo.rs`
- `src/ollama.rs`
- `src/app.rs`
- `src/ui.rs`
- `src/prompt.rs` (test fixture only)
- `src/generate.rs` (test fixture only)
- `docs/design.md` (update config section)

## Risks

- Backward-compat deserialization: the `config` crate merges sources â€” `[ollama]` and `[llm]` keys could conflict. Test explicitly that only one path wins.
- `tokio::join!` for N backends requires `futures::future::join_all` or manual tuple expansion; the current code uses a fixed-arity `tokio::join!`. Switch to `join_all` for the backend health checks.
- Test fixtures in `src/generate.rs` and `src/prompt.rs` construct `RepoState` directly â€” all must be updated or the build will fail.
---

<!-- ticket-section:implementation-note v1 -->
## Implementation Note

Metadata:
- model: gpt-5.4-mini
- completed_at: 2026-05-26T20:56:25+02:00
- state: implemented

Completed:
- Replaced the single `[ollama]` config with `[llm]` plus `[[llm.backends]]`, including default values and legacy `[ollama]` fallback with a warning.
- Added `LlmBackendConfig`, `LlmConfig`, `LlmStatus`, and `LlmBackendStatus`, and updated repo discovery to track all backends concurrently.
- Switched the Ollama client and discovery paths to consume the active backend config.
- Updated landing UI status rendering to show the active backend and additional backends when present.
- Updated test fixtures and the live smoke binary to the new repo/config shape.
- Updated `docs/design.md` to match the new config schema.

Deviations:
- I kept the existing `OllamaClient` type name for this ticket, since the next ticket is the client implementation rewrite.
- Legacy env/table compatibility is handled during config load rather than by adding a separate deserialization layer.

Verification:
- `just verify`

Important files changed:
- `src/config.rs`
- `src/repo.rs`
- `src/ollama.rs`
- `src/app.rs`
- `src/ui.rs`
- `src/generate.rs`
- `src/prompt.rs`
- `src/bin/smoke-live.rs`
- `tests/windows_pr_generation_integration.rs`
- `docs/design.md`

Residual risks:
- The active backend must still be an Ollama-compatible endpoint for the current client implementation.
- Legacy `[ollama]` compatibility is intentionally transitional and may be removed in a later ticket.
