# Stacked-PR Generation via Shared-Prefix Caching

Proposed redesign of the **LLM request strategy** for stacked-PR generation.
Scope is narrow: how the batched draft request is issued, not the surrounding
flow (selection, Form semantics, review modal, push) — those stay as designed in
[stacked-pr-plan.md](stacked-pr-plan.md), which remains the feature source of
truth. If adopted, this replaces the single batched array request on the LLM
path only.

## Status

- **Phase:** accepted 2026-06-05; implementation in progress.
- **Motivation:** the current batched path sends one ~17K-token prompt. On a
  local llama.cpp box that prefills at ~140 tok/s, prefill alone is ~120s, which
  meets or exceeds the client read timeout. Because the request is non-streaming,
  the client gets zero bytes during prefill and trips its read timeout before the
  first token, then retries with a full re-prefill (SWA defeats cache reuse) and
  fails again. See the originating discussion for the log.
- **Decision:** implement this as the default stacked-generation path, but treat
  cache reuse as an observed optimization rather than a hidden assumption. A cold
  cache warning is surfaced per PR when telemetry proves reuse is not happening;
  the flow still produces fallback drafts so review remains resumable.

## Core idea

Today `build_stack_prompt` (`src/domain/prompt.rs`) packs the whole stack into
one prompt and asks for a **JSON array** back — one big request that prefills for
~120s and times out. Instead:

1. Build **one cacheable prefix** containing the entire stack context (the model
   still sees everything → cross-PR coherence preserved), ending with the
   *single-PR* output schema.
2. Prefill that prefix **once**.
3. Fire **N sequential, tiny requests** — `prefix + "now draft change_index = k
   only"` — each of which reuses the cached prefix and prefills only its short
   suffix.

The big prefill happens once; every per-PR request after it is fast and safely
under the timeout.

## Hard prerequisite (load-bearing)

This only works if the shared prefix's KV cache **survives between requests**. On
an SWA model that means the server must run with **`--swa-full`** (or reliable
`--ctx-checkpoints`). Without it, each of the N requests re-prefills the full
prefix → **N × ~110s, strictly worse than today**. The design must detect when
reuse isn't happening and say so (see Validation).

This is a property of *whichever server actually serves the request*. Single-user
local `llama-server` is fine; a shared remote backend (e.g. an Ollama box) may
evict the slot under other load, or may not expose the flag at all.

**Verified (2026-06-05):** the local `llama-server` Qwen3.5-4B reuses the prefix
*without* `--swa-full` — an identical second request reported
`cached_tokens = 2014 / 2018` (`timings.cache_n = 2014`, `prompt_n = 4`). So the
prerequisite only bites on genuinely-SWA models (e.g. Gemma) or a contended
remote slot, not this model. Don't assume — measure it per backend (see
[Cache-health validation](#cache-health-validation-backend-aware)).

## Prompt restructuring (`src/domain/prompt.rs`)

Split `build_stack_prompt` into a prefix builder + a per-PR suffix:

```rust
pub struct StackPrefix {
    pub prefix: String,          // byte-identical across all PRs — the cacheable part
    pub manifest: PromptManifest,
}

/// Everything except the "which PR now" ask: system + instructions + input schema
/// + SINGLE-OBJECT output schema + stack intent + full ranges context (all ranges).
pub fn build_stack_prefix(
    contexts: &[ContextBundle], inputs: &[StackPrInput],
    intent: &StackIntent, shared_labels: &[String], milestone: &str,
) -> StackPrefix;

/// Tiny, varies per PR. Appended AFTER the prefix so the cached prefix stays maximal.
pub fn stack_pr_suffix(input: &StackPrInput) -> String;
```

Two prompt-text changes:

- **Instructions** become "you'll be asked to draft each PR one at a time; the
  full stack is provided so you keep them consistent and non-overlapping." (Adapt
  `STACK_INSTRUCTIONS`.)
- **Output schema** switches from the array (`STACK_OUTPUT_SCHEMA`) back to the
  **single object** — reuse the single-PR `OUTPUT_SCHEMA`. We assign
  `change_index` ourselves from `k`, so the model needn't echo it.

Suffix sketch:

```
## Your Task Now
From the full stack context above, draft the pull request for change_index = {k} ONLY
(head `{head}` against base `{base}`). Output a single JSON object per the Output JSON
Schema — no array, no other PRs, no prose.
```

**Cache-correctness rule:** the prefix must be byte-identical across all `k` and
come first; the suffix is pure tail. For the OpenAI chat path, put the per-PR ask
as the *trailing portion of the final user message* so the rendered token prefix
is shared.

## Request/job model (`src/domain/llm.rs`)

The job runner is one-event-per-job (`src/runtime/jobs.rs` — `run()` returns a
single `JobOutcome`; jobs can't emit mid-run or submit follow-ups). So the
sequence is **app-driven**: each per-PR result triggers the next submission. That
is also what gives incremental UI for free.

```rust
pub struct StackPrLlmJob {
    pub base_url: String, pub model: String, pub api: LlmApi, pub api_key: Option<String>,
    pub prefix: Arc<str>,        // shared; Arc avoids cloning ~60 KB into every job
    pub suffix: String,          // per-PR (tiny)
    pub index: usize,
    pub temperature: Option<f32>, pub max_tokens: Option<u32>,
    pub timeout: Duration,
    pub cancel: CancelHandle,
    pub fallback_subject: String, // to build a fallback draft on a bad row
}

pub enum StackPrLlmResult {
    Ready    { index: usize, draft: StackDraft },
    Errored  { index: usize, message: String, fallback: StackDraft }, // non-fatal: record + continue
    Cancelled{ index: usize },                                        // stop chain, keep prior
}
```

`run()` builds the body as `format!("{prefix}{suffix}")`, sends through the
**existing** `transport_send` (so cancellation + the retry-as-cache-warmer
already apply), and parses the reply with the **existing** `parse_draft` → wrap
into `StackDraft { index, .. }`. A bad/empty row or transport error becomes a
local fallback draft and a non-fatal warning on that row; cancellation is the
only result that stops the chain. This reuses the whole single-PR parse path;
`parse_stack_drafts` retires from the production path. `run()` also reads the
response's cache telemetry (`cached_tokens` / `timings` for the openai path;
`prompt_eval_*` for Ollama) and returns it on each result, so the orchestrator
can flag a cold cache after PR 0 — see
[Cache-health validation](#cache-health-validation-backend-aware).

## App orchestration & state (`src/app.rs`)

Build the prefix once when context lands, then drive the chain:

```rust
enum BulkPhase {
    Idle, Collecting,
    Generating {
        prefix: Arc<str>,
        inputs: Vec<StackPrInput>,
        intent: StackIntent,
        labels: Vec<String>,
        assignees: Vec<String>,
        milestone: String,
        drafts: Vec<Option<StackDraft>>, // by index; fills in as results land
        warnings: Vec<Vec<String>>,      // parse/cache/transport warnings by index
        next: usize,                     // next index to submit; one active at a time
        total: usize,
    },
    Review { plan: StackPlan, .. },
    Failed { message: String },
}
```

- `StackContextResult::Ready` handler: call `build_stack_prefix`, store
  `Arc<str>`, set `Generating { next: 0, .. }`, submit `StackPrLlmJob` for index
  0 with `self.gen_cancel`.
- new `handle_stack_pr_result(index, ..)`: write `drafts[index]` and any row
  warnings, then if `next < total` submit the job for `next` (**only one in
  flight at a time** → maximizes same-slot reuse), else build `StackPlan` and
  enter the existing `Review` modal.
- `Cancelled` → stop submitting and close the modal with the normal
  cancellation acknowledgement. The app keeps in-memory drafts only while the
  modal is open; no disk persistence is introduced in this pass.

Because requests are strictly sequential and nothing else uses the server,
llama.cpp's LCP slot match (`-sps`, threshold 0.1) lands them on the same warm
slot. To be bulletproof, pin `id_slot` in the request body (a llama.cpp
extension) — optional.

## Warm-up (optional but clean)

The **first** request still pays the full ~110s prefill and must fit the timeout.
Two ways:

- **No warm-up:** PR 0 carries the big prefill; PRs 1…N-1 are fast. Simplest.
- **Explicit warm-up:** a `max_tokens: 1` request with just the prefix, shown as a
  distinct `Warming` phase, then all N PRs are uniformly fast. Same total work;
  nicer UX and uniform per-PR latency.

**Timeout strategy:** give the warm-up / PR-0 request the full `timeout_secs`
(≥110s); PRs 1…N-1 are fast once the prefix is cached, so a shorter timeout is
fine. But don't rely on the timeout *alone* to detect a cold cache — read the
cache telemetry off each response instead (see
[Cache-health validation](#cache-health-validation-backend-aware)). Keep a short
PR-`k` timeout only as the backstop for Ollama, where that telemetry is
unreliable.

## Incremental UI

Each landed result flips one row, so the bulk/review surface (the `0001e` review
modal) renders per-PR status: `pending → generating → done/failed`, filling in
live instead of one long freeze. This is the real UX payoff and matches the
repo's incremental-results philosophy (cf. discovery-per-probe-events, `0001a`).

## What stays the same

- Transport, cancellation (`src/runtime/http.rs`), and the timeout/retry plumbing
  — unchanged; reused.
- Context collection (`src/domain/context.rs`) and `diff_budget_bytes = 0`
  behavior — unchanged.
- `parse_draft` / `fallback_draft` — reused; `parse_stack_drafts` retires from
  this path.

## Cache-health validation (backend-aware)

The prefix-cache prerequisite is *observable*: teatui can read whether the prefix
was actually reused off the same responses it already parses, and warn when it
wasn't — instead of inferring from a timeout. The right signal differs by backend.

| Backend | Per-request signal (in the completion response) | Reliable? |
|---|---|---|
| llama.cpp | `usage.prompt_tokens_details.cached_tokens`, plus `timings.cache_n` / `timings.prompt_n` | yes — exact, on by default |
| vLLM | `usage.prompt_tokens_details.cached_tokens` | yes, but block-rounded; needs `--enable-prefix-caching` **and** `--enable-prompt-tokens-details` |
| Ollama | `prompt_eval_count` | no — reports total prompt size, and drops out on repeat requests |

The check, per backend:

- **openai-type (llama.cpp, vLLM):** read `usage.prompt_tokens_details.cached_tokens`
  on each per-PR response. `cached_tokens ≈ prefix length` → reuse working;
  `cached_tokens ≈ 0` on PR `k>0` → cold cache, surface a hint:
  - llama.cpp SWA model → "start `llama-server` with `--swa-full`";
  - vLLM → "launch with `--enable-prefix-caching` (and `--enable-prompt-tokens-details`)".

  vLLM counts at block granularity (~16 tokens), so compare with a tolerance, not `==`.
- **Ollama:** `prompt_eval_count` is unreliable (reports total size; can vanish on
  repeats), so fall back to wall-clock / `prompt_eval_duration` collapsing on PR
  `k>0` as the reuse signal. Also require `keep_alive` (default unloads after 5
  min, wiping the cache), a byte-identical prefix, and a consistent `num_ctx`.

If you'd rather scrape aggregates than read per-response, metrics endpoints exist:
llama.cpp `/metrics` (needs `--metrics`); vLLM `/metrics` (Prometheus, on by
default — `vllm:prefix_cache_queries` + `vllm:prefix_cache_hits`, combine with
`rate()`). Ollama exposes no cache-state endpoint (`/api/ps` lists loaded models
only).

Implementation note: the deserialized response structs in `llm.rs` (`ChatResponse`,
`GenerateResponse`) gain optional telemetry fields, and `StackPrLlmResult::Ready`
carries a small `CacheHealth { cached_tokens, prompt_tokens }` (or a timing for
Ollama) the orchestrator inspects after PR 0.

Sources: [llama.cpp server](https://github.com/ggml-org/llama.cpp/tree/master/tools/server),
[vLLM OpenAI server](https://docs.vllm.ai/en/stable/serving/openai_compatible_server.html) /
[prefix caching](https://docs.vllm.ai/en/stable/design/prefix_caching/) /
[metrics](https://docs.vllm.ai/en/latest/design/metrics/),
[Ollama API usage](https://docs.ollama.com/api/usage) (and issues
[#3427](https://github.com/ollama/ollama/issues/3427),
[#2068](https://github.com/ollama/ollama/issues/2068) on `prompt_eval_count`).

## Risks & open questions

1. **Cache-reuse dependency** is load-bearing. It works out of the box on the
   local Qwen3.5-4B (verified), but a genuinely-SWA model needs `--swa-full` and a
   contended/remote slot can evict mid-run. If reuse can't be guaranteed, the
   independent-small-prompts alternative is safer. Decide first.
2. **Coherence in isolation-of-output:** the model sees the whole stack (good) but
   emits each PR without seeing its own siblings' *generated* output. Usually fine
   since the stack intent anchors them; validate on a real stack.
3. **Slot contention:** if the server serves other clients, the warm slot can be
   evicted. Single-user local is fine; note it for shared/remote backends.
4. **Chat-template prefix stability:** confirm the chosen model's template keeps
   the shared text as an exact token prefix (check with `llama-tokenize` or by
   observing reuse in server logs).

## Suggested ticket split

1. `prompt.rs`: `build_stack_prefix` + `stack_pr_suffix` + single-object schema
   (+ tests).
2. `llm.rs`: `StackPrLlmJob` / `StackPrLlmResult`, reuse
   `parse_draft`/`fallback_draft`, and capture cache telemetry from the response
   (`usage.prompt_tokens_details.cached_tokens`; `prompt_eval_*` for Ollama)
   (+ tests).
3. `app.rs`: prefix-once + sequential chain orchestration, `BulkPhase` streaming
   variant, optional warm-up, dual-timeout cache-health probe.
4. UI: per-row incremental status in the bulk/review surface.
5. Validation: backend-aware cache-health check reading `cached_tokens` per
   response (timing fallback for Ollama), warning when reuse is cold; live smoke
   against local `llama-server`.
