use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Serialize};

use crate::config::LlmBackendConfig;
use crate::generate::{GeneratedDraft, validate_branch_name};
use crate::prompt::PromptBuild;
use crate::repo::LlmStatus;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_TEMPERATURE: f32 = 0.1;

#[derive(Debug, Clone)]
pub struct OllamaNativeBackend {
    base_url: String,
    model: String,
    temperature: f32,
    context_size: Option<u32>,
    max_tokens: Option<u32>,
    client: reqwest::Client,
}

impl OllamaNativeBackend {
    pub fn new(backend: &LlmBackendConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .wrap_err("failed to build Ollama HTTP client")?;

        Ok(Self {
            base_url: backend.base_url.clone(),
            model: backend.model.clone(),
            temperature: backend.temperature.unwrap_or(DEFAULT_TEMPERATURE),
            context_size: backend.context_size,
            max_tokens: backend.max_tokens,
            client,
        })
    }

    pub async fn generate_draft(
        &self,
        prompt: &PromptBuild,
    ) -> std::result::Result<GeneratedDraft, LlmError> {
        let request = OllamaGenerateRequest {
            model: &self.model,
            prompt: &prompt.prompt,
            stream: false,
            options: OllamaOptions {
                temperature: self.temperature,
                num_ctx: self.context_size,
                num_predict: self.max_tokens,
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
                LlmError::new(format!("failed to contact Ollama endpoint: {err}"), None)
            })?;
        let status = response.status();

        let response_text = response.text().await.map_err(|err| {
            LlmError::new(format!("failed to read Ollama response body: {err}"), None)
        })?;

        if !status.is_success() {
            return Err(LlmError::new(
                format!("Ollama request failed with {}", status),
                Some(response_text),
            ));
        }

        let api_response: OllamaGenerateResponse =
            serde_json::from_str(&response_text).map_err(|err| {
                LlmError::new(
                    format!("failed to parse Ollama response wrapper: {err}"),
                    Some(response_text.clone()),
                )
            })?;

        if !api_response.done {
            return Err(LlmError::new(
                "Ollama response ended before completion",
                Some(api_response.response),
            ));
        }

        parse_generated_draft(api_response.response)
    }

    pub async fn health_check(&self) -> LlmStatus {
        let url = self.base_url.trim_end_matches('/').to_string();
        health_check_url(url, "/").await
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatClient {
    base_url: String,
    model: String,
    temperature: f32,
    max_tokens: Option<u32>,
    min_p: Option<f32>,
    client: reqwest::Client,
}

impl OpenAiCompatClient {
    pub fn new(backend: &LlmBackendConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .wrap_err("failed to build OpenAI-compatible HTTP client")?;

        Ok(Self {
            base_url: backend.base_url.clone(),
            model: backend.model.clone(),
            temperature: backend.temperature.unwrap_or(DEFAULT_TEMPERATURE),
            max_tokens: backend.max_tokens,
            min_p: backend.min_p,
            client,
        })
    }

    pub async fn generate_draft(
        &self,
        prompt: &PromptBuild,
    ) -> std::result::Result<GeneratedDraft, LlmError> {
        let request = OpenAiChatCompletionsRequest {
            model: &self.model,
            messages: vec![OpenAiMessage {
                role: "user",
                content: &prompt.prompt,
            }],
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            min_p: self.min_p,
            stream: false,
        };

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|err| {
                LlmError::new(
                    format!("failed to contact OpenAI-compatible endpoint: {err}"),
                    None,
                )
            })?;
        let status = response.status();
        let response_text = response.text().await.map_err(|err| {
            LlmError::new(
                format!("failed to read OpenAI-compatible response body: {err}"),
                None,
            )
        })?;

        if !status.is_success() {
            return Err(LlmError::new(
                format!("OpenAI-compatible request failed with {}", status),
                Some(response_text),
            ));
        }

        let api_response: OpenAiChatCompletionsResponse = serde_json::from_str(&response_text)
            .map_err(|err| {
                LlmError::new(
                    format!("failed to parse OpenAI-compatible response wrapper: {err}"),
                    Some(response_text.clone()),
                )
            })?;

        let content = extract_openai_content(api_response, &response_text)?;

        parse_generated_draft(content)
    }

    pub async fn health_check(&self) -> LlmStatus {
        let url = self.base_url.trim_end_matches('/').to_string();
        health_check_url(url, "/v1/models").await
    }
}

#[derive(Debug, Clone)]
pub enum LlmClient {
    Ollama(OllamaNativeBackend),
    OpenAiCompat(OpenAiCompatClient),
}

impl LlmClient {
    pub fn from_config(backend: &LlmBackendConfig) -> Result<Self> {
        match backend.backend_type.as_str() {
            "ollama" => Ok(Self::Ollama(OllamaNativeBackend::new(backend)?)),
            "llama-cpp" | "vllm" => Ok(Self::OpenAiCompat(OpenAiCompatClient::new(backend)?)),
            other => bail!("unknown LLM backend type `{other}`"),
        }
    }

    pub async fn generate_draft(
        &self,
        prompt: &PromptBuild,
    ) -> std::result::Result<GeneratedDraft, LlmError> {
        match self {
            Self::Ollama(client) => client.generate_draft(prompt).await,
            Self::OpenAiCompat(client) => client.generate_draft(prompt).await,
        }
    }

    pub async fn health_check(&self) -> LlmStatus {
        match self {
            Self::Ollama(client) => client.health_check().await,
            Self::OpenAiCompat(client) => client.health_check().await,
        }
    }

    pub async fn health_check_for(backend: &LlmBackendConfig) -> LlmStatus {
        match Self::from_config(backend) {
            Ok(client) => client.health_check().await,
            Err(err) => LlmStatus::Unreachable(err.to_string()),
        }
    }
}

pub async fn health_check(base_url: &str) -> LlmStatus {
    health_check_url(base_url.trim_end_matches('/').to_string(), "/").await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmError {
    pub message: String,
    pub raw_response: Option<String>,
}

impl LlmError {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
    done: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiChatCompletionsRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_p: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatCompletionsResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    message: Option<OpenAiChoiceMessage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoiceMessage {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftPayload {
    branch_name: String,
    title: String,
    body: String,
    review_notes: Vec<String>,
}

fn parse_generated_draft(raw_response: String) -> std::result::Result<GeneratedDraft, LlmError> {
    let payload: DraftPayload = serde_json::from_str(raw_response.trim()).map_err(|err| {
        LlmError::new(
            format!("failed to parse generated draft JSON: {err}"),
            Some(raw_response.clone()),
        )
    })?;

    let branch_name = payload.branch_name.trim().to_string();
    if let Some(message) = validate_branch_name(&branch_name).into_iter().next() {
        return Err(LlmError::new(
            format!("generated branch name is invalid: {message}"),
            Some(raw_response),
        ));
    }

    let title = payload.title.trim().to_string();
    if title.is_empty() {
        return Err(LlmError::new(
            "generated title is required",
            Some(raw_response),
        ));
    }

    let body = payload.body.trim().to_string();
    if body.is_empty() {
        return Err(LlmError::new(
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

fn extract_openai_content(
    response: OpenAiChatCompletionsResponse,
    raw_response: &str,
) -> std::result::Result<String, LlmError> {
    let Some(choice) = response.choices.into_iter().next() else {
        return Err(LlmError::new(
            "OpenAI-compatible response did not include any choices",
            Some(raw_response.to_string()),
        ));
    };

    let Some(message) = choice.message else {
        return Err(LlmError::new(
            "OpenAI-compatible response did not include a message",
            Some(raw_response.to_string()),
        ));
    };

    message.content.ok_or_else(|| {
        LlmError::new(
            "OpenAI-compatible response did not include message content",
            Some(raw_response.to_string()),
        )
    })
}

async fn health_check_url(base_url: String, path: &str) -> LlmStatus {
    let client = match reqwest::Client::builder()
        .timeout(HEALTH_CHECK_TIMEOUT)
        .build()
        .wrap_err("failed to build LLM health check client")
    {
        Ok(client) => client,
        Err(err) => return LlmStatus::Unreachable(err.to_string()),
    };

    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    match client.get(url).send().await {
        Ok(response) if response.status().is_success() => LlmStatus::Reachable,
        Ok(response) => {
            LlmStatus::Unreachable(format!("health check returned {}", response.status()))
        }
        Err(err) => LlmStatus::Unreachable(err.to_string()),
    }
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

    #[test]
    fn parses_openai_compat_choices_content() {
        let response: OpenAiChatCompletionsResponse = serde_json::from_str(
            r#"{
                "choices": [
                    {"message": {"content": "{\"branch_name\":\"feature/example\",\"title\":\"Add OpenAI compat generation\",\"body\":\"Summary\",\"review_notes\":[]}"}}
                ]
            }"#,
        )
        .expect("response");

        let content = extract_openai_content(response, "raw").expect("content");

        let draft = parse_generated_draft(content).expect("draft");
        assert_eq!(draft.branch_name, "feature/example");
        assert_eq!(draft.title, "Add OpenAI compat generation");
    }

    #[test]
    fn rejects_openai_compat_envelope_without_choices() {
        let response: OpenAiChatCompletionsResponse =
            serde_json::from_str(r#"{"choices":[]}"#).expect("response");
        let err = extract_openai_content(response, "raw").expect_err("error");
        assert!(err.message.contains("any choices"));
    }

    #[test]
    fn rejects_openai_compat_envelope_without_message() {
        let response: OpenAiChatCompletionsResponse =
            serde_json::from_str(r#"{"choices":[{}]}"#).expect("response");
        let err = extract_openai_content(response, "raw").expect_err("error");
        assert!(err.message.contains("a message"));
    }

    #[test]
    fn rejects_openai_compat_envelope_without_content() {
        let response: OpenAiChatCompletionsResponse =
            serde_json::from_str(r#"{"choices":[{"message":{}}]}"#).expect("response");
        let err = extract_openai_content(response, "raw").expect_err("error");
        assert!(err.message.contains("message content"));
    }

    #[test]
    fn rejects_openai_compat_non_json_content() {
        let err = parse_generated_draft("not json".into()).expect_err("error");
        assert!(err.message.contains("parse generated draft JSON"));
    }

    #[test]
    fn constructs_native_ollama_client_for_ollama_backend() {
        let backend = LlmBackendConfig {
            backend_type: "ollama".into(),
            ..LlmBackendConfig::default()
        };

        let client = LlmClient::from_config(&backend).expect("client");

        assert!(matches!(client, LlmClient::Ollama(_)));
    }

    #[test]
    fn constructs_openai_compat_client_for_compat_backends() {
        for backend_type in ["llama-cpp", "vllm"] {
            let backend = LlmBackendConfig {
                backend_type: backend_type.into(),
                ..LlmBackendConfig::default()
            };

            let client = LlmClient::from_config(&backend).expect("client");

            assert!(matches!(client, LlmClient::OpenAiCompat(_)));
        }
    }

    #[test]
    fn rejects_unknown_backend_type() {
        let backend = LlmBackendConfig {
            backend_type: "other".into(),
            ..LlmBackendConfig::default()
        };

        let err = LlmClient::from_config(&backend).expect_err("error");

        assert!(err.to_string().contains("unknown LLM backend type"));
    }
}
