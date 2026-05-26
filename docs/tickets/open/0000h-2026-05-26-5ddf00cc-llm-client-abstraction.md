---
id: 0000h-2026-05-26-5ddf00cc-llm-client-abstraction
created_at: 2026-05-26T18:02:10+02:00
created_by_model: claude-sonnet-4-6/high
state: open
---
# LLM Client Abstraction: Enum Dispatch for Ollama Native and OpenAI-Compat

## Goal

Introduce a `LlmClient` enum that dispatches `generate_draft` and `health_check` to either the existing Ollama native client or a new `OpenAiCompatClient` (covering llama-cpp and vllm). Wire the enum into `app.rs` using the active backend config. The caller never needs to know which backend is in use.

## Context

After the config schema ticket (0000g), `Config` exposes `llm: LlmConfig` with an `active` field and `backends: Vec<LlmBackendConfig>`. `LlmBackendConfig` has a `backend_type: String` field (`"ollama"`, `"llama-cpp"`, `"vllm"`). The existing `OllamaClient` in `src/ollama.rs` uses Ollama's native `/api/generate` endpoint and is kept as-is for Ollama backends.

This ticket adds:
- `OpenAiCompatClient` targeting `/v1/chat/completions` (OpenAI-compat) for llama-cpp and vllm.
- `LlmClient` enum wrapping both, with a shared `generate_draft` and `health_check` API.
- Construction via `LlmClient::from_config(backend: &LlmBackendConfig) -> Result<Self>`.
- Rename `OllamaError` to `LlmError` (shared across both implementations).
- Update `app.rs` to construct `LlmClient::from_config(active_backend)` instead of `OllamaClient::new(&self.config)`.

## Non-Goals

- Streaming responses.
- Per-request backend switching from the TUI (config-time selection only).
- Supporting OpenAI API authentication/tokens (local servers only, no Authorization header needed).
- Passing `context_size` or `min_p` to backends that do not support them -- silently omit unknown fields.

## Design Decisions

- **Enum not trait object**: avoids async_trait / dyn complexity. Two variants: `Ollama(OllamaClient)` and `OpenAiCompat(OpenAiCompatClient)`. Dispatch is a match in each method.
- **`backend_type` determines variant**: `"ollama"` maps to `Ollama`, `"llama-cpp"` and `"vllm"` map to `OpenAiCompat`. Unknown type returns an error from `from_config`.
- **OpenAI-compat request format**: `POST /v1/chat/completions` with model, messages array containing a single user message with the full prompt string, temperature, max_tokens, stream: false. Optionally include min_p if set in config.
- **OpenAI-compat response parsing**: extract `choices[0].message.content`, then call shared `parse_generated_draft`. Response shape: `{"choices": [{"message": {"content": "..."}}]}`.
- **Ollama-specific params**: `context_size` maps to `options.num_ctx`; `max_tokens` maps to `options.num_predict`. Both passed only if set in config.
- **Health check**: `OllamaClient::health_check` keeps its existing `GET /` probe. `OpenAiCompatClient::health_check` does `GET /v1/models` and returns `LlmStatus::Reachable` on 200, `Unreachable(msg)` otherwise.
- **`LlmError`** replaces `OllamaError` with the same fields (`message: String`, `raw_response: Option<String>`). The rename is mechanical.
- **Timeout**: reuse the existing `REQUEST_TIMEOUT` (60s) and `HEALTH_CHECK_TIMEOUT` (2s) constants, shared between both clients.
- **`src/repo.rs` health check**: the per-backend health checks called during `discover_repo_state` switch from `crate::ollama::health_check(&config)` to `LlmClient::health_check_for(backend: &LlmBackendConfig) -> LlmStatus`, a static method that builds a minimal client just for the probe.

## Implementation Plan

1. **`src/ollama.rs` renamed to `src/llm.rs`**: Rename the file. Inside:
   - Rename `OllamaError` to `LlmError`. Update all usages.
   - Keep `OllamaClient` struct and its `generate_draft` impl unchanged except it now takes `&LlmBackendConfig` instead of `&Config`.
   - Add `OpenAiCompatClient { base_url, model, client, config }`.
   - Implement `OpenAiCompatClient::new(backend: &LlmBackendConfig) -> Result<Self>`.
   - Implement `OpenAiCompatClient::generate_draft`: build messages request, POST, extract `choices[0].message.content`, call `parse_generated_draft`.
   - Implement `OpenAiCompatClient::health_check`: GET `/v1/models`.
   - Add `LlmClient` enum with `Ollama(OllamaClient)` and `OpenAiCompat(OpenAiCompatClient)`.
   - Implement `LlmClient::from_config(backend: &LlmBackendConfig) -> Result<Self>`.
   - Implement `LlmClient::generate_draft` and `LlmClient::health_check` via match dispatch.
   - Add `LlmClient::health_check_for(backend: &LlmBackendConfig) -> LlmStatus` static method.
   - Keep shared `parse_generated_draft`, `normalize_review_notes`, `DraftPayload` in `llm.rs`.
2. **`src/main.rs`**: Update `mod ollama` to `mod llm`.
3. **`src/app.rs`**: Replace `use crate::ollama::OllamaClient` with `use crate::llm::LlmClient`. Replace construction call. Update `OllamaError` references to `LlmError`.
4. **`src/repo.rs`**: Replace `crate::ollama::health_check(&config)` with `LlmClient::health_check_for(backend)` called per backend via `join_all`.
5. **Tests in `src/llm.rs`**: Keep existing Ollama parse tests. Add OpenAI-compat response parsing tests: happy path, malformed response (missing choices), non-JSON content.

## Agent Handoff
```json
{
  "read_next": ["CLAUDE.md", "src/ollama.rs", "src/config.rs", "src/repo.rs", "src/app.rs", "docs/tickets/reviewed/0000g-2026-05-26-d7cc7ec7-llm-config-schema.md"],
  "likely_files": ["src/ollama.rs", "src/llm.rs", "src/app.rs", "src/repo.rs", "src/main.rs"],
  "verification_commands": ["cargo test --all", "cargo clippy -- -D warnings"],
  "review_focus": ["OpenAI-compat response parsing handles missing fields gracefully", "LlmClient::from_config returns clear error for unknown backend_type", "health_check_for is a pure probe with no side effects", "no OllamaClient or OllamaError symbols remain in public API", "parse_generated_draft is shared between both paths"]
}
```

## Acceptance Criteria

- `cargo test --all` passes.
- `cargo clippy -- -D warnings` is clean.
- `LlmClient::from_config` constructs the right variant for each `backend_type`.
- `LlmClient::from_config` returns `Err` for unknown `backend_type` values.
- OpenAI-compat happy-path test: valid `choices[0].message.content` JSON produces a `GeneratedDraft`.
- OpenAI-compat error tests: malformed envelope, empty choices, non-JSON content all produce `LlmError`.
- No public symbols named `OllamaClient`, `OllamaError`, or `OllamaStatus` remain.
- `app.rs` constructs the client from the active backend config, not from `Config.ollama`.

## Verification Plan

- `cargo test --all` -- unit tests for both parse paths plus existing Ollama tests.
- `cargo clippy -- -D warnings` -- type safety and dead-code check.
- Manual: run with an Ollama backend in config and confirm generation still works end-to-end.

## Files Likely Touched

- `src/ollama.rs` (renamed to `src/llm.rs`)
- `src/llm.rs` (new file from rename)
- `src/app.rs`
- `src/repo.rs`
- `src/main.rs` (mod declaration update)
- `Cargo.toml` (no new deps needed -- reqwest and serde_json already present)

## Risks

- Renaming `ollama.rs` to `llm.rs` requires updating `mod ollama` in `main.rs`. Easy to miss.
- The OpenAI-compat response has multiple nesting levels (`choices[0].message.content`) -- serde struct must handle missing or empty `choices` without panicking.
- `context_size` mapped to `num_ctx` is Ollama-specific; if accidentally sent to a compat backend it is a no-op (JSON field ignored), which is acceptable.
- This ticket depends on 0000g being implemented first: `LlmBackendConfig` must exist before `LlmClient::from_config` can compile.
