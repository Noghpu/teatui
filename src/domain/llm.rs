use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::bookmark::slugify;
use super::stack::{StackDraft, StackPrInput};
use crate::config::LlmApi;
use crate::runtime::http::{self, CancelHandle, HttpError};
use crate::runtime::{Job, JobOutcome};

// ========================== Stack LLM job ===================================

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheHealth {
    /// OpenAI-compatible `usage.prompt_tokens_details.cached_tokens` or
    /// llama.cpp `timings.cache_n`.
    pub cached_tokens: Option<u64>,
    /// OpenAI-compatible `usage.prompt_tokens` or llama.cpp `timings.prompt_n`.
    pub prompt_tokens: Option<u64>,
    /// Ollama `prompt_eval_count`, when present.
    pub prompt_eval_count: Option<u64>,
    /// Ollama `prompt_eval_duration`, converted from ns to ms.
    pub prompt_eval_duration_ms: Option<u64>,
}

/// Result type for [`StackPrLlmJob`]. Each job drafts exactly one stack row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackLlmResult {
    Ready {
        index: usize,
        draft: StackDraft,
        cache: CacheHealth,
    },
    Errored {
        index: usize,
        message: String,
        fallback: StackDraft,
        cache: CacheHealth,
    },
    /// The user aborted the request mid-flight.
    Cancelled { index: usize },
}

/// LLM job for one row of stacked-PR generation. The large shared prefix is an
/// `Arc<str>` so sequential jobs do not clone the whole stack context.
pub struct StackPrLlmJob {
    pub base_url: String,
    pub model: String,
    pub api: LlmApi,
    pub api_key: Option<String>,
    pub prefix: Arc<str>,
    pub suffix: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: Duration,
    /// Shared cancel handle so `Esc` can abort the request.
    pub cancel: CancelHandle,
    /// Input row used for fallback and result indexing.
    pub input: StackPrInput,
}

impl Job for StackPrLlmJob {
    fn name(&self) -> &'static str {
        "domain.llm.stack_generate_pr"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = match self.api {
            LlmApi::Ollama => call_ollama_stack_pr(*self),
            LlmApi::Openai => call_openai_stack_pr(*self),
        };
        JobOutcome::Done(Box::new(result))
    }
}

fn call_ollama_stack_pr(job: StackPrLlmJob) -> StackLlmResult {
    let index = job.input.index;
    let url = format!("{}/api/generate", job.base_url.trim_end_matches('/'));
    let mut options = serde_json::Map::new();
    if let Some(t) = job.temperature {
        options.insert("temperature".into(), serde_json::json!(t));
    }
    if let Some(n) = job.max_tokens {
        options.insert("num_predict".into(), serde_json::json!(n));
    }
    let prompt = format!("{}{}", job.prefix, job.suffix);
    let body = serde_json::json!({
        "model": job.model,
        "prompt": prompt,
        "stream": false,
        "options": options,
    });

    let raw = match transport_send(&url, None, &body, job.timeout, &job.cancel) {
        Ok(raw) => raw,
        Err(result) => return stack_from_transport_error(index, result, &job.input),
    };
    if let Some(message) = http_error_message(raw.status, &raw.body) {
        return stack_row_error(index, message, &job.input, CacheHealth::default());
    }
    let parsed: GenerateResponse = match serde_json::from_str(&raw.body) {
        Ok(p) => p,
        Err(e) => {
            return stack_row_error(
                index,
                format!("invalid response shape: {e}"),
                &job.input,
                CacheHealth::default(),
            );
        }
    };
    let cache = CacheHealth::from_ollama(&parsed);
    stack_row_from_content(index, &parsed.response, &job.input, cache)
}

fn call_openai_stack_pr(job: StackPrLlmJob) -> StackLlmResult {
    let index = job.input.index;
    let url = format!("{}/v1/chat/completions", job.base_url.trim_end_matches('/'));
    let prompt = format!("{}{}", job.prefix, job.suffix);
    let mut body = serde_json::json!({
        "model": job.model,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": false,
    });
    if let Some(t) = job.temperature {
        body["temperature"] = serde_json::json!(t);
    }
    if let Some(n) = job.max_tokens {
        body["max_tokens"] = serde_json::json!(n);
    }

    let raw = match transport_send(
        &url,
        job.api_key.as_deref(),
        &body,
        job.timeout,
        &job.cancel,
    ) {
        Ok(raw) => raw,
        Err(result) => return stack_from_transport_error(index, result, &job.input),
    };
    if let Some(message) = http_error_message(raw.status, &raw.body) {
        return stack_row_error(index, message, &job.input, CacheHealth::default());
    }
    let parsed: ChatResponse = match serde_json::from_str(&raw.body) {
        Ok(p) => p,
        Err(e) => {
            return stack_row_error(
                index,
                format!("invalid response shape: {e}"),
                &job.input,
                CacheHealth::default(),
            );
        }
    };
    let cache = CacheHealth::from_openai(&parsed);
    match parsed.choices.into_iter().next() {
        Some(choice) => stack_row_from_content(index, &choice.message.content, &job.input, cache),
        None => stack_row_error(index, "no choices in response".into(), &job.input, cache),
    }
}

/// Convert a transport-level [`LlmResult`] error into a row-level
/// [`StackLlmResult`].
fn stack_from_transport_error(
    index: usize,
    result: LlmResult,
    input: &StackPrInput,
) -> StackLlmResult {
    match result {
        LlmResult::Cancelled => StackLlmResult::Cancelled { index },
        LlmResult::Errored { message } => {
            stack_row_error(index, message, input, CacheHealth::default())
        }
        LlmResult::Ready(_) => unreachable!("transport errors are never Ready"),
    }
}

fn stack_row_from_content(
    index: usize,
    content: &str,
    input: &StackPrInput,
    cache: CacheHealth,
) -> StackLlmResult {
    match parse_draft(content) {
        LlmResult::Ready(draft) => StackLlmResult::Ready {
            index,
            draft: StackDraft {
                index,
                pr_type: draft.pr_type,
                branch_slug: draft.branch_slug,
                title: draft.title,
                description: draft.description,
            },
            cache,
        },
        LlmResult::Errored { message } => stack_row_error(index, message, input, cache),
        LlmResult::Cancelled => StackLlmResult::Cancelled { index },
    }
}

fn stack_row_error(
    index: usize,
    message: String,
    input: &StackPrInput,
    cache: CacheHealth,
) -> StackLlmResult {
    StackLlmResult::Errored {
        index,
        message,
        fallback: fallback_stack_draft(input),
        cache,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDraft {
    pub pr_type: String,
    pub branch_slug: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmResult {
    Ready(GeneratedDraft),
    Errored {
        message: String,
    },
    /// The user aborted the request mid-flight. Distinct from `Errored` so the
    /// app returns to idle without flagging a failure.
    Cancelled,
}

pub struct LlmGenerateJob {
    pub base_url: String,
    pub model: String,
    pub api: LlmApi,
    pub api_key: Option<String>,
    pub prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: Duration,
    /// Aborts the in-flight request when the user cancels generation. Only the
    /// `http://` (local) transport is cancellable; for `https://` this handle's
    /// socket stays empty and `cancel()` is a no-op for the request itself.
    pub cancel: CancelHandle,
}

impl Job for LlmGenerateJob {
    fn name(&self) -> &'static str {
        "domain.llm.generate"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = match self.api {
            LlmApi::Ollama => call_ollama(*self),
            LlmApi::Openai => call_openai(*self),
        };
        JobOutcome::Done(Box::new(result))
    }
}

fn call_ollama(job: LlmGenerateJob) -> LlmResult {
    let url = format!("{}/api/generate", job.base_url.trim_end_matches('/'));
    let mut options = serde_json::Map::new();
    if let Some(t) = job.temperature {
        options.insert("temperature".into(), serde_json::json!(t));
    }
    if let Some(n) = job.max_tokens {
        options.insert("num_predict".into(), serde_json::json!(n));
    }
    let body = serde_json::json!({
        "model": job.model,
        "prompt": job.prompt,
        "stream": false,
        "options": options,
    });

    let raw = match transport_send(&url, None, &body, job.timeout, &job.cancel) {
        Ok(raw) => raw,
        Err(result) => return result,
    };
    if let Some(message) = http_error_message(raw.status, &raw.body) {
        return LlmResult::Errored { message };
    }
    let parsed: GenerateResponse = match serde_json::from_str(&raw.body) {
        Ok(p) => p,
        Err(e) => {
            return LlmResult::Errored {
                message: format!("invalid response shape: {e}"),
            };
        }
    };
    parse_draft(&parsed.response)
}

/// OpenAI-compatible completion via `POST /v1/chat/completions`. Used for
/// llama.cpp's server, vLLM, and hosted OpenAI-style endpoints. The prompt
/// becomes a single user message; the reply text is fed to `parse_draft`
/// exactly like the Ollama path.
fn call_openai(job: LlmGenerateJob) -> LlmResult {
    let url = format!("{}/v1/chat/completions", job.base_url.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": job.model,
        "messages": [{ "role": "user", "content": job.prompt }],
        "stream": false,
    });
    if let Some(t) = job.temperature {
        body["temperature"] = serde_json::json!(t);
    }
    if let Some(n) = job.max_tokens {
        body["max_tokens"] = serde_json::json!(n);
    }

    let raw = match transport_send(
        &url,
        job.api_key.as_deref(),
        &body,
        job.timeout,
        &job.cancel,
    ) {
        Ok(raw) => raw,
        Err(result) => return result,
    };
    if let Some(message) = http_error_message(raw.status, &raw.body) {
        return LlmResult::Errored { message };
    }
    let parsed: ChatResponse = match serde_json::from_str(&raw.body) {
        Ok(p) => p,
        Err(e) => {
            return LlmResult::Errored {
                message: format!("invalid response shape: {e}"),
            };
        }
    };
    match parsed.choices.into_iter().next() {
        Some(choice) => parse_draft(&choice.message.content),
        None => LlmResult::Errored {
            message: "no choices in response".into(),
        },
    }
}

/// The raw HTTP response shared by both transports: the status and the body
/// text, left unparsed so each API path deserializes its own shape.
struct RawResponse {
    status: u16,
    body: String,
}

/// Send the request over whichever transport the URL's scheme dictates.
///
/// `http://` (local Ollama / llama.cpp) goes through the cancellable std
/// client so the user can abort it; `https://` (hosted) stays on ureq, where
/// the request is not cancellable. Transport failures and cancellation come
/// back as an `Err(LlmResult)` the caller returns verbatim; an `Ok` carries
/// any HTTP response, success or error status alike.
fn transport_send(
    url: &str,
    api_key: Option<&str>,
    body: &serde_json::Value,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<RawResponse, LlmResult> {
    if url.starts_with("http://") {
        send_cancellable(url, api_key, &body.to_string(), timeout, cancel)
    } else {
        send_ureq(url, api_key, body, timeout)
    }
}

fn send_cancellable(
    url: &str,
    api_key: Option<&str>,
    body: &str,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<RawResponse, LlmResult> {
    let auth = api_key.map(|key| format!("Bearer {key}"));
    let headers: Vec<(&str, &str)> = match &auth {
        Some(value) => vec![("Authorization", value.as_str())],
        None => Vec::new(),
    };
    match post_with_retry(url, &headers, body, timeout, cancel) {
        Ok(resp) => Ok(RawResponse {
            status: resp.status,
            body: resp.body,
        }),
        Err(HttpError::Cancelled) => Err(LlmResult::Cancelled),
        Err(e) => Err(LlmResult::Errored {
            message: e.to_string(),
        }),
    }
}

/// `send_with_retry`'s counterpart for the cancellable transport: retry once on
/// a transient transport failure, but never on cancellation, and skip the retry
/// entirely if the user cancelled during the backoff.
fn post_with_retry(
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Result<http::HttpResponse, HttpError> {
    match http::post_json(url, headers, body, timeout, cancel) {
        Err(HttpError::Timeout) | Err(HttpError::Io(_)) if !cancel.is_cancelled() => {
            std::thread::sleep(RETRY_BACKOFF);
            if cancel.is_cancelled() {
                return Err(HttpError::Cancelled);
            }
            http::post_json(url, headers, body, timeout, cancel)
        }
        other => other,
    }
}

fn send_ureq(
    url: &str,
    api_key: Option<&str>,
    body: &serde_json::Value,
    timeout: Duration,
) -> Result<RawResponse, LlmResult> {
    let agent = build_agent(timeout);
    let send = || {
        let mut request = agent.post(url);
        if let Some(key) = api_key {
            request = request.header("Authorization", &format!("Bearer {key}"));
        }
        request.send_json(body)
    };
    let mut response = match send_with_retry(send) {
        Ok(r) => r,
        Err(e) => {
            return Err(LlmResult::Errored {
                message: e.to_string(),
            });
        }
    };
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|e| format!("<failed to read body: {e}>"));
    Ok(RawResponse { status, body })
}

/// Pause before the single retry so a momentarily-unavailable server (e.g.
/// mid-restart) has a beat to recover. A timeout retry doesn't strictly need
/// it, but the delay is cheap on a one-shot generation.
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// Send a request, retrying exactly once on a transient transport failure.
///
/// A cold generation is legitimately slow — a large prompt's prefill plus a
/// slow decode rate can run ~50s — so the timeout itself is generous. This
/// retry is a safety net for blips (connection refused, a stray timeout), and
/// on llama.cpp it doubles as a cache-warmer: the first attempt leaves the
/// prompt in a context checkpoint, so the retry skips prefill and only decodes.
/// We do NOT retry HTTP status errors (a 400 for an over-long prompt would just
/// fail again) — those arrive as a successful `Response` here and are handled
/// by `http_error_message`.
fn send_with_retry(
    send: impl Fn() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    match send() {
        Err(e) if is_transient(&e) => {
            std::thread::sleep(RETRY_BACKOFF);
            send()
        }
        other => other,
    }
}

/// Transport-level failures worth one retry: a timeout, a refused/dropped
/// connection (`Io`), or a generic connect failure. Everything else (bad URI,
/// protocol, TLS, decode) is deterministic and would fail again.
fn is_transient(error: &ureq::Error) -> bool {
    matches!(
        error,
        ureq::Error::Timeout(_) | ureq::Error::Io(_) | ureq::Error::ConnectionFailed
    )
}

/// Build an agent that does NOT turn 4xx/5xx into a bare `http status: N`
/// error. We want to read the response body on failure: llama.cpp (and most
/// OpenAI-compatible servers) put the real reason there — e.g. a prompt that
/// exceeds the model's context window comes back as a 400 with an explanatory
/// `error.message`.
fn build_agent(timeout: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(config)
}

/// If the status is non-2xx, return a diagnostic message that includes the
/// server's response body. Returns `None` for success statuses.
fn http_error_message(status: u16, body: &str) -> Option<String> {
    if (200..300).contains(&status) {
        return None;
    }
    let detail = extract_error_message(body).unwrap_or_else(|| body.to_string());
    Some(format!("http {status}: {}", detail.trim()))
}

/// Pull `error.message` (OpenAI/llama.cpp shape) out of a JSON error body,
/// falling back to a top-level `error` string. Returns `None` if the body
/// isn't recognizable JSON, so the caller can show the raw text instead.
fn extract_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if let Some(msg) = value.get("error").and_then(|e| e.get("message")) {
        return msg.as_str().map(str::to_string);
    }
    value
        .get("error")
        .and_then(|e| e.as_str())
        .map(str::to_string)
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
    prompt_eval_count: Option<u64>,
    prompt_eval_duration: Option<u64>,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
    timings: Option<ChatTimings>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: Option<u64>,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize)]
struct PromptTokensDetails {
    cached_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ChatTimings {
    cache_n: Option<u64>,
    prompt_n: Option<u64>,
}

impl CacheHealth {
    fn from_ollama(response: &GenerateResponse) -> Self {
        Self {
            cached_tokens: None,
            prompt_tokens: None,
            prompt_eval_count: response.prompt_eval_count,
            prompt_eval_duration_ms: response.prompt_eval_duration.map(|ns| ns / 1_000_000),
        }
    }

    fn from_openai(response: &ChatResponse) -> Self {
        let usage = response.usage.as_ref();
        let timings = response.timings.as_ref();
        Self {
            cached_tokens: usage
                .and_then(|u| u.prompt_tokens_details.as_ref())
                .and_then(|d| d.cached_tokens)
                .or_else(|| timings.and_then(|t| t.cache_n)),
            prompt_tokens: usage
                .and_then(|u| u.prompt_tokens)
                .or_else(|| timings.and_then(|t| t.prompt_n)),
            prompt_eval_count: None,
            prompt_eval_duration_ms: None,
        }
    }
}

#[derive(Deserialize)]
struct DraftJson {
    #[serde(rename = "type")]
    pr_type: String,
    branch_slug: String,
    title: String,
    description: String,
}

/// One element of the batched LLM JSON array response.
#[derive(Deserialize, Default)]
struct StackDraftJson {
    change_index: Option<usize>,
    #[serde(rename = "type", default)]
    pr_type: String,
    #[serde(default)]
    branch_slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
}

/// Parse the raw LLM response. Tries to deserialize as JSON first; falls back
/// to first-line / rest split for older/plaintext model responses.
pub fn parse_draft(raw: &str) -> LlmResult {
    let trimmed = raw.trim();

    // Sometimes models wrap their JSON in ```json fences despite the instruction.
    // Strip a single leading/trailing fence pair if present.
    let stripped = strip_code_fences(trimmed);

    if let Ok(parsed) = serde_json::from_str::<DraftJson>(stripped) {
        let title = parsed.title.trim().to_string();
        let pr_type = normalize_pr_type(&parsed.pr_type);
        let branch_slug = normalize_branch_slug(&parsed.branch_slug, &title, pr_type);
        return LlmResult::Ready(GeneratedDraft {
            pr_type: pr_type.to_string(),
            branch_slug,
            title,
            description: parsed.description.trim().to_string(),
        });
    }

    // Fallback: first non-empty line as title, rest as description.
    let mut lines = trimmed.lines().peekable();
    let title = loop {
        match lines.next() {
            Some(line) if !line.trim().is_empty() => break line.trim().to_string(),
            Some(_) => continue,
            None => break String::new(),
        }
    };
    if title.is_empty() {
        return LlmResult::Errored {
            message: "empty LLM response".into(),
        };
    }
    let description = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let branch_slug = normalize_branch_slug("", &title, "chore");
    LlmResult::Ready(GeneratedDraft {
        pr_type: "chore".into(),
        branch_slug,
        title,
        description,
    })
}

fn normalize_pr_type(value: &str) -> &'static str {
    match value.trim() {
        "feat" => "feat",
        "fix" => "fix",
        "docs" => "docs",
        "refactor" => "refactor",
        "test" => "test",
        "chore" => "chore",
        _ => "chore",
    }
}

fn normalize_branch_slug(branch_slug: &str, title: &str, pr_type: &str) -> String {
    let slug = slugify(branch_slug.trim().trim_start_matches("pr/"));
    let prefix = format!("{pr_type}-");
    let slug = slug.strip_prefix(&prefix).unwrap_or(&slug).to_string();
    if slug.is_empty() {
        slugify(title)
    } else {
        slug
    }
}

/// Parse a raw batched LLM response into one [`StackDraft`] per [`StackPrInput`].
///
/// Expects the response to be a JSON array where each item has:
/// `{ "change_index", "type", "branch_slug", "title", "description" }`.
///
/// Matching is by `change_index`. Any missing or malformed row is filled from
/// local fallbacks: `type = "chore"`, `title` from `input.subject`,
/// `description = ""`, `branch_slug = slugify(subject)`. A single bad row
/// never discards valid rows; the result always has exactly `inputs.len()`
/// entries in index order.
pub fn parse_stack_drafts(raw: &str, inputs: &[StackPrInput]) -> Vec<StackDraft> {
    let stripped = strip_code_fences(raw.trim());

    // Parse as a generic JSON array first so that one malformed element does
    // not discard valid ones. Each element is then individually re-parsed into
    // `StackDraftJson`; non-objects are silently dropped.
    let raw_array: Vec<serde_json::Value> = serde_json::from_str(stripped).unwrap_or_default();

    let rows: Vec<StackDraftJson> = raw_array
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    // Build a lookup by change_index so we can fill gaps.
    let by_index: std::collections::HashMap<usize, &StackDraftJson> = rows
        .iter()
        .filter_map(|r| r.change_index.map(|i| (i, r)))
        .collect();

    inputs
        .iter()
        .map(|input| {
            if let Some(row) = by_index.get(&input.index) {
                // Attempt to use this row; fall back per field if it's empty.
                let title = row.title.trim().to_string();
                let description = row.description.trim().to_string();
                let pr_type = normalize_pr_type(&row.pr_type).to_string();
                let branch_slug = normalize_branch_slug(&row.branch_slug, &title, &pr_type);
                let (title, branch_slug) = if title.is_empty() {
                    let fallback_title = input.subject.trim().to_string();
                    let fallback_slug = normalize_branch_slug("", &fallback_title, &pr_type);
                    (fallback_title, fallback_slug)
                } else {
                    (title, branch_slug)
                };
                StackDraft {
                    index: input.index,
                    pr_type,
                    branch_slug,
                    title,
                    description,
                }
            } else {
                // Row missing or change_index absent — full fallback.
                fallback_stack_draft(input)
            }
        })
        .collect()
}

/// Build a fallback `StackDraft` from the input when the LLM row is absent or
/// unusable.
pub fn fallback_stack_draft(input: &StackPrInput) -> StackDraft {
    let title = input.subject.trim().to_string();
    let branch_slug = slugify(&title);
    StackDraft {
        index: input.index,
        pr_type: "chore".to_string(),
        branch_slug,
        title,
        description: String::new(),
    }
}

fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .unwrap_or(s);
    let s = s.trim_start_matches('\n');
    let s = s.strip_suffix("```").unwrap_or(s);
    s.trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_content_from_openai_chat_response() {
        // Shape returned by OpenAI / vLLM / llama.cpp `/v1/chat/completions`.
        let raw = r#"{
            "id": "chatcmpl-1",
            "choices": [
                { "index": 0, "message": { "role": "assistant", "content": "{\"type\":\"feat\",\"branch_slug\":\"add-foo\",\"title\":\"Add foo\",\"description\":\"Body\"}" } }
            ]
        }"#;
        let parsed: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        let content = parsed.choices.into_iter().next().unwrap().message.content;
        match parse_draft(&content) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "feat");
                assert_eq!(d.title, "Add foo");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn openai_response_without_choices_is_empty() {
        let parsed: ChatResponse = serde_json::from_str(r#"{"choices":[]}"#).expect("deserialize");
        assert!(parsed.choices.is_empty());
    }

    #[test]
    fn parses_clean_json() {
        let raw = r#"{"type":"feat","branch_slug":"add-foo","title":"Add foo","description":"Implements foo support."}"#;
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "feat");
                assert_eq!(d.branch_slug, "add-foo");
                assert_eq!(d.title, "Add foo");
                assert_eq!(d.description, "Implements foo support.");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn parses_json_in_code_fence() {
        let raw = "```json\n{\"type\":\"fix\",\"branch_slug\":\"repair-foo\",\"title\":\"Add foo\",\"description\":\"body\"}\n```";
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "fix");
                assert_eq!(d.branch_slug, "repair-foo");
                assert_eq!(d.title, "Add foo");
                assert_eq!(d.description, "body");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_first_line_then_rest() {
        let raw = "Add foo\n\nDescribes the change in plaintext.";
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "chore");
                assert_eq!(d.branch_slug, "add-foo");
                assert_eq!(d.title, "Add foo");
                assert_eq!(d.description, "Describes the change in plaintext.");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn empty_response_is_error() {
        match parse_draft("   \n  ") {
            LlmResult::Errored { message } => assert!(message.contains("empty")),
            other => panic!("expected Errored, got {other:?}"),
        }
    }

    #[test]
    fn normalizes_branch_slug_with_prefixes() {
        let raw = r#"{"type":"fix","branch_slug":"pr/fix/Add Foo!","title":"Add foo","description":"body"}"#;
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "fix");
                assert_eq!(d.branch_slug, "add-foo");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn parses_claude_response_for_rewrite_branch() {
        // Response captured from `claude -p -` with trunk()..@ prompt on 2026-06-02.
        let raw = include_str!("testdata/claude_rewrite_response.json");
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "feat");
                assert_eq!(d.branch_slug, "linux-rewrite-pr-generation");
                assert!(d.title.len() <= 72, "title too long: {}", d.title.len());
                assert!(!d.title.ends_with('.'), "title ends with period");
                assert!(d.description.contains("## Summary"));
                assert!(d.description.contains("## Why"));
                assert!(d.description.contains("## Verification"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn parses_claude_response_for_first_change_after_trunk() {
        // Response captured from `claude -p -` with trunk()..ttyqnlq prompt on 2026-06-02.
        let raw = include_str!("testdata/claude_first_change_response.json");
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "feat");
                assert!(d.title.len() <= 72, "title too long: {}", d.title.len());
                assert!(!d.title.ends_with('.'), "title ends with period");
                assert!(d.description.contains("## Summary"));
                assert!(d.description.contains("## Why"));
                assert!(d.description.contains("## Verification"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn parses_claude_response_for_last_two_non_empty_changes() {
        // Response captured from `claude -p -` with txyprrs..lruvromr prompt on 2026-06-02.
        let raw = include_str!("testdata/claude_last_two_changes_response.json");
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert!(
                    matches!(
                        d.pr_type.as_str(),
                        "feat" | "fix" | "chore" | "refactor" | "docs" | "test"
                    ),
                    "unexpected pr_type: {}",
                    d.pr_type
                );
                assert!(d.title.len() <= 72, "title too long: {}", d.title.len());
                assert!(!d.title.ends_with('.'), "title ends with period");
                assert!(d.description.contains("## Summary"));
                assert!(d.description.contains("## Why"));
                assert!(d.description.contains("## Verification"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn unknown_json_type_falls_back_to_chore() {
        let raw = r#"{"type":"bugfix","branch_slug":"repair-foo","title":"Add foo","description":"body"}"#;
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.pr_type, "chore");
                assert_eq!(d.branch_slug, "repair-foo");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn stack_row_uses_single_pr_parser_and_preserves_index() {
        let input = make_input(2, "Fallback subject");
        let cache = CacheHealth {
            cached_tokens: Some(128),
            prompt_tokens: Some(132),
            prompt_eval_count: None,
            prompt_eval_duration_ms: None,
        };
        let raw =
            r#"{"type":"feat","branch_slug":"add-cache","title":"Add cache","description":"Body"}"#;

        match stack_row_from_content(2, raw, &input, cache.clone()) {
            StackLlmResult::Ready {
                index,
                draft,
                cache: got_cache,
            } => {
                assert_eq!(index, 2);
                assert_eq!(draft.index, 2);
                assert_eq!(draft.pr_type, "feat");
                assert_eq!(draft.branch_slug, "add-cache");
                assert_eq!(draft.title, "Add cache");
                assert_eq!(got_cache, cache);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn stack_row_bad_content_returns_fallback_with_error() {
        let input = make_input(1, "Fallback title");

        match stack_row_from_content(1, "", &input, CacheHealth::default()) {
            StackLlmResult::Errored {
                index,
                message,
                fallback,
                ..
            } => {
                assert_eq!(index, 1);
                assert!(message.contains("empty"));
                assert_eq!(fallback.index, 1);
                assert_eq!(fallback.pr_type, "chore");
                assert_eq!(fallback.title, "Fallback title");
                assert_eq!(fallback.branch_slug, "fallback-title");
            }
            other => panic!("expected Errored, got {other:?}"),
        }
    }

    #[test]
    fn openai_cache_telemetry_reads_usage_details() {
        let raw = r#"{
            "choices": [],
            "usage": {
                "prompt_tokens": 2048,
                "prompt_tokens_details": { "cached_tokens": 1984 }
            }
        }"#;
        let parsed: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        let cache = CacheHealth::from_openai(&parsed);
        assert_eq!(cache.cached_tokens, Some(1984));
        assert_eq!(cache.prompt_tokens, Some(2048));
    }

    #[test]
    fn openai_cache_telemetry_falls_back_to_llama_timings() {
        let raw = r#"{
            "choices": [],
            "timings": { "cache_n": 2014, "prompt_n": 2018 }
        }"#;
        let parsed: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        let cache = CacheHealth::from_openai(&parsed);
        assert_eq!(cache.cached_tokens, Some(2014));
        assert_eq!(cache.prompt_tokens, Some(2018));
    }

    #[test]
    fn ollama_cache_telemetry_converts_duration_to_ms() {
        let parsed: GenerateResponse = serde_json::from_str(
            r#"{
                "response": "{}",
                "prompt_eval_count": 4000,
                "prompt_eval_duration": 1500000000
            }"#,
        )
        .expect("deserialize");
        let cache = CacheHealth::from_ollama(&parsed);
        assert_eq!(cache.prompt_eval_count, Some(4000));
        assert_eq!(cache.prompt_eval_duration_ms, Some(1500));
    }

    // =================== parse_stack_drafts tests ===================

    fn make_input(index: usize, subject: &str) -> StackPrInput {
        StackPrInput {
            index,
            base: "main".into(),
            head: format!("head-{index}"),
            included_change_ids: vec![format!("ch-{index}")],
            subject: subject.into(),
        }
    }

    #[test]
    fn stack_drafts_valid_full_array() {
        let inputs = vec![make_input(0, "First PR"), make_input(1, "Second PR")];
        let raw = r#"[
            {"change_index": 0, "type": "feat", "branch_slug": "first-pr", "title": "First PR", "description": "Desc 0"},
            {"change_index": 1, "type": "fix",  "branch_slug": "second-pr","title": "Second PR","description": "Desc 1"}
        ]"#;
        let drafts = parse_stack_drafts(raw, &inputs);
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].index, 0);
        assert_eq!(drafts[0].pr_type, "feat");
        assert_eq!(drafts[0].branch_slug, "first-pr");
        assert_eq!(drafts[0].title, "First PR");
        assert_eq!(drafts[0].description, "Desc 0");
        assert_eq!(drafts[1].index, 1);
        assert_eq!(drafts[1].pr_type, "fix");
        assert_eq!(drafts[1].branch_slug, "second-pr");
    }

    #[test]
    fn stack_drafts_one_malformed_row_falls_back_others_intact() {
        let inputs = vec![
            make_input(0, "Good PR"),
            make_input(1, "Bad PR"),
            make_input(2, "Also Good PR"),
        ];
        // Row 1 has malformed JSON (will be dropped); row 0 and 2 are valid.
        let raw = r#"[
            {"change_index": 0, "type": "feat", "branch_slug": "good-pr",      "title": "Good PR",      "description": "Desc 0"},
            "this is not an object",
            {"change_index": 2, "type": "docs", "branch_slug": "also-good-pr", "title": "Also Good PR", "description": "Desc 2"}
        ]"#;
        let drafts = parse_stack_drafts(raw, &inputs);
        assert_eq!(drafts.len(), 3, "must always return inputs.len() drafts");
        // Row 0 intact
        assert_eq!(drafts[0].pr_type, "feat");
        assert_eq!(drafts[0].title, "Good PR");
        // Row 1 fell back
        assert_eq!(drafts[1].pr_type, "chore");
        assert_eq!(drafts[1].title, "Bad PR");
        assert_eq!(drafts[1].branch_slug, "bad-pr");
        // Row 2 intact
        assert_eq!(drafts[2].pr_type, "docs");
        assert_eq!(drafts[2].title, "Also Good PR");
    }

    #[test]
    fn stack_drafts_missing_index_filled_by_fallback() {
        let inputs = vec![make_input(0, "PR Zero"), make_input(1, "PR One")];
        // Only change_index 0 is present; 1 is missing.
        let raw = r#"[
            {"change_index": 0, "type": "feat", "branch_slug": "pr-zero", "title": "PR Zero", "description": "D0"}
        ]"#;
        let drafts = parse_stack_drafts(raw, &inputs);
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].pr_type, "feat");
        assert_eq!(drafts[0].title, "PR Zero");
        // Missing index 1 falls back.
        assert_eq!(drafts[1].index, 1);
        assert_eq!(drafts[1].pr_type, "chore");
        assert_eq!(drafts[1].title, "PR One");
        assert_eq!(drafts[1].branch_slug, "pr-one");
    }

    #[test]
    fn stack_drafts_garbage_payload_all_fallback() {
        let inputs = vec![make_input(0, "Fallback A"), make_input(1, "Fallback B")];
        let drafts = parse_stack_drafts("not json at all", &inputs);
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].pr_type, "chore");
        assert_eq!(drafts[0].title, "Fallback A");
        assert_eq!(drafts[0].branch_slug, "fallback-a");
        assert_eq!(drafts[1].pr_type, "chore");
        assert_eq!(drafts[1].title, "Fallback B");
        assert_eq!(drafts[1].branch_slug, "fallback-b");
    }

    #[test]
    fn stack_drafts_slug_normalized_per_row() {
        let inputs = vec![make_input(0, "Add Stuff")];
        // branch_slug has uppercase and pr/ prefix; should be normalized.
        let raw = r#"[
            {"change_index": 0, "type": "feat", "branch_slug": "pr/feat/Add Stuff!", "title": "Add Stuff", "description": "D"}
        ]"#;
        let drafts = parse_stack_drafts(raw, &inputs);
        assert_eq!(drafts[0].branch_slug, "add-stuff");
    }

    #[test]
    fn stack_drafts_code_fenced_array() {
        let inputs = vec![make_input(0, "Fenced PR")];
        let raw = "```json\n[{\"change_index\":0,\"type\":\"feat\",\"branch_slug\":\"fenced-pr\",\"title\":\"Fenced PR\",\"description\":\"D\"}]\n```";
        let drafts = parse_stack_drafts(raw, &inputs);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].pr_type, "feat");
        assert_eq!(drafts[0].title, "Fenced PR");
    }

    #[test]
    fn stack_drafts_extra_and_indexless_rows_do_not_displace_valid_ones() {
        // The model returns a valid row for index 0, a structurally-valid object
        // with no `change_index` (must be dropped, not mis-assigned), and an
        // extra row for an index that has no matching input (must be ignored).
        // Index 1 has no usable row and must fall back.
        let inputs = vec![make_input(0, "Real Zero"), make_input(1, "Real One")];
        let raw = r#"[
            {"change_index": 0, "type": "feat", "branch_slug": "real-zero", "title": "Real Zero", "description": "D0"},
            {"type": "fix", "branch_slug": "ghost", "title": "Indexless Ghost", "description": "no index"},
            {"change_index": 9, "type": "docs", "branch_slug": "extra", "title": "Extra Row", "description": "unmatched"}
        ]"#;
        let drafts = parse_stack_drafts(raw, &inputs);
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].title, "Real Zero");
        assert_eq!(drafts[0].pr_type, "feat");
        // Index 1 falls back; the indexless and out-of-range rows never leak in.
        assert_eq!(drafts[1].index, 1);
        assert_eq!(drafts[1].pr_type, "chore");
        assert_eq!(drafts[1].title, "Real One");
        assert_eq!(drafts[1].branch_slug, "real-one");
    }

    #[test]
    fn stack_drafts_duplicate_index_is_deterministic_last_wins() {
        // Two rows claim change_index 0. The result must be deterministic and
        // still produce exactly inputs.len() drafts in index order.
        let inputs = vec![make_input(0, "Subject Zero")];
        let raw = r#"[
            {"change_index": 0, "type": "feat", "branch_slug": "first", "title": "First Claim", "description": "A"},
            {"change_index": 0, "type": "fix",  "branch_slug": "second","title": "Second Claim","description": "B"}
        ]"#;
        let drafts = parse_stack_drafts(raw, &inputs);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].pr_type, "fix");
        assert_eq!(drafts[0].title, "Second Claim");
        assert_eq!(drafts[0].branch_slug, "second");
    }
}
