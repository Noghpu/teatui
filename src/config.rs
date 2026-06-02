use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub commands: CommandConfig,
    pub llm: LlmConfig,
    pub pr: PrConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct CommandConfig {
    pub jj: String,
    pub git: String,
    pub tea: String,
}

impl Default for CommandConfig {
    fn default() -> Self {
        Self {
            jj: "jj".into(),
            git: "git".into(),
            tea: "tea".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct LlmConfig {
    pub active: String,
    pub backends: Vec<LlmBackend>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            active: "default".into(),
            backends: vec![LlmBackend::default()],
        }
    }
}

impl LlmConfig {
    pub fn active_backend(&self) -> &LlmBackend {
        self.backends
            .iter()
            .find(|b| b.name == self.active)
            .or_else(|| self.backends.first())
            .expect("llm.backends must not be empty")
    }
}

/// Which wire protocol a backend speaks.
///
/// - `Ollama` (default): native Ollama API — `GET /api/tags` for health,
///   `POST /api/generate` for completion.
/// - `Openai`: OpenAI-compatible API — `GET /v1/models` for health,
///   `POST /v1/chat/completions` for completion. Covers llama.cpp's
///   server, vLLM, and hosted OpenAI-style endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmApi {
    #[default]
    Ollama,
    Openai,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct LlmBackend {
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// Wire protocol; defaults to Ollama. Spelled `type` in TOML to match
    /// the established config convention.
    #[serde(rename = "type")]
    pub api: LlmApi,
    /// Bearer token sent as `Authorization` for `openai` backends that
    /// require auth (hosted endpoints, secured vLLM). Unused for Ollama.
    pub api_key: Option<String>,
    pub temperature: Option<f32>,
    pub context_size: Option<u32>,
    pub max_tokens: Option<u32>,
    pub timeout_secs: u64,
    pub min_p: Option<f32>,
    pub seed: Option<u64>,
}

impl Default for LlmBackend {
    fn default() -> Self {
        Self {
            name: "default".into(),
            base_url: "http://localhost:11434".into(),
            model: "qwen2.5-coder:latest".into(),
            api: LlmApi::Ollama,
            api_key: None,
            temperature: Some(0.1),
            context_size: None,
            max_tokens: Some(2048),
            timeout_secs: 120,
            min_p: None,
            seed: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PrConfig {
    pub default_base: String,
}

impl Default for PrConfig {
    fn default() -> Self {
        Self {
            default_base: "main".into(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut builder = config::Config::builder();

        if let Some(default_path) = default_config_path()
            && default_path.exists()
        {
            builder = builder.add_source(config::File::from(default_path));
        }

        if let Some(path) = path {
            builder = builder.add_source(config::File::from(path.to_path_buf()));
        }

        builder = builder.add_source(
            config::Environment::with_prefix("TEATUI")
                .separator("_")
                .try_parsing(true),
        );

        builder
            .build()
            .wrap_err("failed to build configuration")?
            .try_deserialize()
            .wrap_err("failed to deserialize configuration")
    }
}

fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("teatui").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deserialize(raw: &str) -> Config {
        let cfg = config::Config::builder()
            .add_source(config::File::from_str(raw, config::FileFormat::Toml))
            .build()
            .expect("config builder");
        cfg.try_deserialize().expect("config deserialize")
    }

    #[test]
    fn defaults_use_bare_tool_names() {
        let config = Config::default();
        assert_eq!(config.commands.jj, "jj");
        assert_eq!(config.commands.git, "git");
        assert_eq!(config.commands.tea, "tea");
    }

    #[test]
    fn overrides_tool_paths() {
        let config = deserialize(
            r#"
[commands]
jj = "/usr/local/bin/jj"
tea = "/opt/tea/bin/tea"
"#,
        );
        assert_eq!(config.commands.jj, "/usr/local/bin/jj");
        assert_eq!(config.commands.git, "git");
        assert_eq!(config.commands.tea, "/opt/tea/bin/tea");
    }

    #[test]
    fn llm_defaults_point_at_local_ollama() {
        let config = Config::default();
        let backend = config.llm.active_backend();
        assert_eq!(backend.base_url, "http://localhost:11434");
        assert_eq!(backend.model, "qwen2.5-coder:latest");
        assert_eq!(backend.timeout_secs, 120);
    }

    #[test]
    fn llm_active_backend_resolves_by_name() {
        let config = deserialize(
            r#"
[llm]
active = "fast"

[[llm.backends]]
name = "default"
base_url = "http://localhost:11434"
model = "qwen2.5-coder:latest"

[[llm.backends]]
name = "fast"
base_url = "http://example.test:11434"
model = "codellama:7b"
timeout_secs = 60
"#,
        );
        let backend = config.llm.active_backend();
        assert_eq!(backend.base_url, "http://example.test:11434");
        assert_eq!(backend.model, "codellama:7b");
        assert_eq!(backend.timeout_secs, 60);
    }

    #[test]
    fn llm_falls_back_to_first_backend_when_active_not_found() {
        let config = deserialize(
            r#"
[llm]
active = "missing"

[[llm.backends]]
name = "only"
base_url = "http://localhost:11434"
model = "qwen2.5-coder:latest"
"#,
        );
        assert_eq!(config.llm.active_backend().name, "only");
    }

    #[test]
    fn llm_backend_defaults_to_ollama_api() {
        let backend = LlmBackend::default();
        assert_eq!(backend.api, LlmApi::Ollama);
        assert!(backend.api_key.is_none());
    }

    #[test]
    fn llm_backend_parses_openai_api_and_key() {
        let config = deserialize(
            r#"
[[llm.backends]]
name = "vllm"
base_url = "http://localhost:8000"
model = "meta-llama/Llama-3.1-8B-Instruct"
type = "openai"
api_key = "sk-test"
"#,
        );
        let backend = &config.llm.backends[0];
        assert_eq!(backend.api, LlmApi::Openai);
        assert_eq!(backend.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn pr_default_base_overrides() {
        let config = deserialize(
            r#"
[pr]
default_base = "trunk"
"#,
        );
        assert_eq!(config.pr.default_base, "trunk");
    }
}
