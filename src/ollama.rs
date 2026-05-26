use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::generate::{GeneratedDraft, validate_branch_name};
use crate::prompt::PromptBuild;
use crate::repo::OllamaStatus;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_TEMPERATURE: f32 = 0.1;

#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaClient {
    pub fn new(config: &Config) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .wrap_err("failed to build Ollama HTTP client")?;

        Ok(Self {
            base_url: config.ollama.base_url.clone(),
            model: config.ollama.model.clone(),
            client,
        })
    }

    pub async fn generate_draft(
        &self,
        prompt: &PromptBuild,
    ) -> std::result::Result<GeneratedDraft, OllamaError> {
        let request = OllamaGenerateRequest {
            model: &self.model,
            prompt: &prompt.prompt,
            stream: false,
            options: OllamaOptions {
                temperature: DEFAULT_TEMPERATURE,
            },
        };

        let url = format!("{}/api/generate", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|err| {
                OllamaError::new(format!("failed to contact Ollama endpoint: {err}"), None)
            })?;
        let status = response.status();

        let response_text = response.text().await.map_err(|err| {
            OllamaError::new(format!("failed to read Ollama response body: {err}"), None)
        })?;

        if !status.is_success() {
            return Err(OllamaError::new(
                format!("Ollama request failed with {}", status),
                Some(response_text),
            ));
        }

        let api_response: OllamaGenerateResponse =
            serde_json::from_str(&response_text).map_err(|err| {
                OllamaError::new(
                    format!("failed to parse Ollama response wrapper: {err}"),
                    Some(response_text.clone()),
                )
            })?;

        if !api_response.done {
            return Err(OllamaError::new(
                "Ollama response ended before completion",
                Some(api_response.response),
            ));
        }

        parse_generated_draft(api_response.response)
    }
}

pub async fn health_check(config: &Config) -> OllamaStatus {
    let client = match reqwest::Client::builder()
        .timeout(HEALTH_CHECK_TIMEOUT)
        .build()
        .wrap_err("failed to build Ollama health check client")
    {
        Ok(client) => client,
        Err(err) => {
            return OllamaStatus::Unreachable(err.to_string());
        }
    };

    let url = config.ollama.base_url.trim_end_matches('/').to_string();
    match client.get(url).send().await {
        Ok(_) => OllamaStatus::Reachable,
        Err(err) => OllamaStatus::Unreachable(err.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaError {
    pub message: String,
    pub raw_response: Option<String>,
}

impl OllamaError {
    pub fn new(message: impl Into<String>, raw_response: Option<String>) -> Self {
        Self {
            message: message.into(),
            raw_response,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, Deserialize, Serialize)]
struct OllamaOptions {
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
    done: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftPayload {
    branch_name: String,
    title: String,
    body: String,
    review_notes: Vec<String>,
}

fn parse_generated_draft(raw_response: String) -> std::result::Result<GeneratedDraft, OllamaError> {
    let payload: DraftPayload = serde_json::from_str(raw_response.trim()).map_err(|err| {
        OllamaError::new(
            format!("failed to parse generated draft JSON: {err}"),
            Some(raw_response.clone()),
        )
    })?;

    let branch_name = payload.branch_name.trim().to_string();
    if let Some(message) = validate_branch_name(&branch_name).into_iter().next() {
        return Err(OllamaError::new(
            format!("generated branch name is invalid: {message}"),
            Some(raw_response),
        ));
    }

    let title = payload.title.trim().to_string();
    if title.is_empty() {
        return Err(OllamaError::new(
            "generated title is required",
            Some(raw_response),
        ));
    }

    let body = payload.body.trim().to_string();
    if body.is_empty() {
        return Err(OllamaError::new(
            "generated body is required",
            Some(raw_response),
        ));
    }

    let review_notes = normalize_review_notes(payload.review_notes);

    Ok(GeneratedDraft {
        branch_name,
        title,
        body,
        review_notes,
        raw_model_response: raw_response,
    })
}

fn normalize_review_notes(notes: Vec<String>) -> Vec<String> {
    notes
        .into_iter()
        .map(|note| note.trim().to_string())
        .filter(|note| !note.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes_generated_draft_payload() {
        let draft = parse_generated_draft(
            r#"{
                "branch_name": "feature/example",
                "title": "Add Ollama generation",
                "body": "Summary\n\nTesting\n",
                "review_notes": ["  keep an eye on truncation  ", ""]
            }"#
            .into(),
        )
        .expect("draft");

        assert_eq!(draft.branch_name, "feature/example");
        assert_eq!(draft.title, "Add Ollama generation");
        assert_eq!(draft.body, "Summary\n\nTesting");
        assert_eq!(draft.review_notes, vec!["keep an eye on truncation"]);
    }

    #[test]
    fn rejects_missing_required_fields() {
        let err = parse_generated_draft(
            r#"{
                "branch_name": "feature/example",
                "body": "Summary",
                "review_notes": []
            }"#
            .into(),
        )
        .expect_err("error");

        assert!(err.message.contains("title"));
    }

    #[test]
    fn rejects_invalid_branch_names() {
        let err = parse_generated_draft(
            r#"{
                "branch_name": "Feature Example",
                "title": "Add Ollama generation",
                "body": "Summary",
                "review_notes": []
            }"#
            .into(),
        )
        .expect_err("error");

        assert!(err.message.contains("branch name"));
    }
}
