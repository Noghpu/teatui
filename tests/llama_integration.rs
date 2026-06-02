//! Live integration test against a local OpenAI-compatible llama.cpp server.
//!
//! These tests are **excluded from the normal suite** via `#[ignore]`, so
//! `just test` / `cargo test` compile but skip them. Run them explicitly
//! against a running server:
//!
//! ```text
//! just integration                       # starts/stops the server for you
//! # or, against an already-running server:
//! cargo test --test llama_integration -- --ignored --nocapture
//! ```
//!
//! Environment overrides:
//! - `TEATUI_LLAMA_URL`   base URL (default `http://127.0.0.1:8080`)
//! - `TEATUI_LLAMA_MODEL` model name sent in the request (default `qwen3.5-4b`)
//!
//! They drive the real `openai` code paths end to end: `BackendHealthProbe`
//! hits `GET /v1/models`, `LlmGenerateJob` hits `POST /v1/chat/completions`,
//! and the reply is run through the same `parse_draft` as production.

use std::time::Duration;

use teatui::config::LlmApi;
use teatui::domain::{BackendHealth, BackendHealthProbe, LlmGenerateJob, LlmHealth, LlmResult};
use teatui::runtime::{CancelHandle, Job, JobOutcome};

fn base_url() -> String {
    std::env::var("TEATUI_LLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into())
}

fn model() -> String {
    std::env::var("TEATUI_LLAMA_MODEL").unwrap_or_else(|_| "qwen3.5-4b".into())
}

/// Run a `Job` on the calling thread and downcast its payload.
fn run_job<J: Job, T: 'static>(job: J) -> T {
    match Box::new(job).run() {
        JobOutcome::Done(any) => *any
            .downcast::<T>()
            .expect("job returned an unexpected payload type"),
        JobOutcome::Failed(msg) => panic!("job failed: {msg}"),
    }
}

#[test]
#[ignore = "requires a running llama.cpp server; use `just integration`"]
fn llama_health_reports_available() {
    let result: BackendHealth = run_job(BackendHealthProbe {
        name: "llama-cpp".into(),
        base_url: base_url(),
        api: LlmApi::Openai,
        api_key: None,
        timeout: Duration::from_secs(10),
    });
    match result.health {
        LlmHealth::Available { models } => {
            assert!(!models.is_empty(), "server reported zero models");
            eprintln!("/v1/models reported: {models:?}");
        }
        LlmHealth::Unreachable { message } => panic!("health probe unreachable: {message}"),
    }
}

#[test]
#[ignore = "requires a running llama.cpp server; use `just integration`"]
fn llama_generation_returns_draft() {
    let prompt = "You are generating a pull-request draft. Respond with ONLY compact JSON \
        and no prose, with exactly these keys: type, branch_slug, title, description. \
        `type` must be one of: feat, fix, docs, refactor, test, chore. The change adds \
        a hello-world function to the codebase.";
    let result: LlmResult = run_job(LlmGenerateJob {
        base_url: base_url(),
        model: model(),
        api: LlmApi::Openai,
        api_key: None,
        prompt: prompt.into(),
        temperature: Some(0.1),
        max_tokens: Some(512),
        timeout: Duration::from_secs(180),
        cancel: CancelHandle::new(),
    });
    match result {
        LlmResult::Ready(draft) => {
            assert!(!draft.title.trim().is_empty(), "draft title was empty");
            eprintln!(
                "draft: type={} branch_slug={} title={:?}",
                draft.pr_type, draft.branch_slug, draft.title
            );
        }
        LlmResult::Errored { message } => panic!("generation errored: {message}"),
        LlmResult::Cancelled => panic!("generation was cancelled"),
    }
}
