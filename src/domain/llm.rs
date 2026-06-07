use std::time::Duration;

use serde::Deserialize;

use crate::runtime::{Job, JobOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDraft {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmResult {
    Ready(GeneratedDraft),
    Errored { message: String },
}

pub struct LlmGenerateJob {
    pub base_url: String,
    pub model: String,
    pub prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: Duration,
}

impl Job for LlmGenerateJob {
    fn name(&self) -> &'static str {
        "domain.llm.generate"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = call_ollama(*self);
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

    let agent = ureq::AgentBuilder::new().timeout(job.timeout).build();
    let response = match agent.post(&url).send_json(body) {
        Ok(r) => r,
        Err(e) => {
            return LlmResult::Errored {
                message: e.to_string(),
            };
        }
    };
    let parsed: GenerateResponse = match response.into_json() {
        Ok(p) => p,
        Err(e) => {
            return LlmResult::Errored {
                message: format!("invalid response shape: {e}"),
            };
        }
    };
    parse_draft(&parsed.response)
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

#[derive(Deserialize)]
struct DraftJson {
    title: String,
    description: String,
}

/// Parse the raw LLM response. Tries to deserialize as JSON `{title, description}`
/// first; falls back to first-line / rest split.
pub fn parse_draft(raw: &str) -> LlmResult {
    let trimmed = raw.trim();

    // Sometimes models wrap their JSON in ```json fences despite the instruction.
    // Strip a single leading/trailing fence pair if present.
    let stripped = strip_code_fences(trimmed);

    if let Ok(parsed) = serde_json::from_str::<DraftJson>(stripped) {
        return LlmResult::Ready(GeneratedDraft {
            title: parsed.title.trim().to_string(),
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
    LlmResult::Ready(GeneratedDraft { title, description })
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
    fn parses_clean_json() {
        let raw = r#"{"title":"Add foo","description":"Implements foo support."}"#;
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
                assert_eq!(d.title, "Add foo");
                assert_eq!(d.description, "Implements foo support.");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn parses_json_in_code_fence() {
        let raw = "```json\n{\"title\":\"Add foo\",\"description\":\"body\"}\n```";
        match parse_draft(raw) {
            LlmResult::Ready(d) => {
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
}
