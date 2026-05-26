use std::path::Path;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;
use tracing::warn;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub tick_rate: Duration,
    pub llm: LlmConfig,
    pub commands: CommandConfig,
    pub pr: PrConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct RawConfig {
    #[serde(with = "humantime_serde")]
    tick_rate: Duration,
    llm: Option<LlmConfig>,
    #[serde(alias = "ollama")]
    legacy_ollama: Option<LegacyOllamaConfig>,
    commands: CommandConfig,
    pr: PrConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct LlmConfig {
    pub active: String,
    pub backends: Vec<LlmBackendConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct LlmBackendConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub backend_type: String,
    pub base_url: String,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub context_size: Option<u32>,
    pub min_p: Option<f32>,
    pub seed: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
struct LegacyOllamaConfig {
    base_url: String,
    model: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct CommandConfig {
    pub jj: String,
    pub git: String,
    pub tea: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PrConfig {
    pub default_base: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tick_rate: Duration::from_millis(250),
            llm: LlmConfig::default(),
            commands: CommandConfig::default(),
            pr: PrConfig::default(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            active: "default".into(),
            backends: vec![LlmBackendConfig::default()],
        }
    }
}

impl LlmConfig {
    pub fn active_backend(&self) -> Option<&LlmBackendConfig> {
        self.backends
            .iter()
            .find(|backend| backend.name == self.active)
    }
}

impl Default for LlmBackendConfig {
    fn default() -> Self {
        Self {
            name: "default".into(),
            backend_type: "ollama".into(),
            base_url: "http://localhost:11434".into(),
            model: "qwen2.5-coder:latest".into(),
            temperature: Some(0.1),
            max_tokens: Some(2048),
            context_size: Some(4096),
            min_p: None,
            seed: None,
        }
    }
}

impl LlmBackendConfig {
    fn from_legacy(name: impl Into<String>, legacy: LegacyOllamaConfig) -> Self {
        Self {
            name: name.into(),
            base_url: legacy.base_url,
            model: legacy.model,
            ..Self::default()
        }
    }
}

impl Default for LegacyOllamaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".into(),
            model: "qwen2.5-coder:latest".into(),
        }
    }
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

impl Default for PrConfig {
    fn default() -> Self {
        Self {
            default_base: "main".into(),
        }
    }
}

impl From<RawConfig> for Config {
    fn from(raw: RawConfig) -> Self {
        let llm = match (raw.llm, raw.legacy_ollama) {
            (Some(llm), _) => llm,
            (None, Some(legacy)) => {
                warn!(
                    "legacy [ollama] config is deprecated; migrate to [llm] and [[llm.backends]]"
                );
                LlmConfig {
                    active: "default".into(),
                    backends: vec![LlmBackendConfig::from_legacy("default", legacy)],
                }
            }
            (None, None) => LlmConfig::default(),
        };

        Self {
            tick_rate: raw.tick_rate,
            llm,
            commands: raw.commands,
            pr: raw.pr,
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut builder = config::Config::builder();

        if let Some(config_dir) = dirs::config_dir() {
            let default_path = config_dir.join("teatui").join("config.toml");
            if default_path.exists() {
                builder = builder.add_source(config::File::from(default_path));
            }
        }

        if let Some(path) = path {
            builder = builder.add_source(config::File::from(path.to_path_buf()));
        }

        builder = builder.add_source(
            config::Environment::with_prefix("TEATUI")
                .separator("_")
                .try_parsing(true),
        );

        let raw: RawConfig = builder
            .build()
            .wrap_err("Failed to build configuration")?
            .try_deserialize()
            .wrap_err("Failed to deserialize configuration")?;

        Ok(raw.into())
    }
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            tick_rate: Duration::from_millis(250),
            llm: None,
            legacy_ollama: None,
            commands: CommandConfig::default(),
            pr: PrConfig::default(),
        }
    }
}

mod humantime_serde {
    use serde::{Deserialize, Deserializer};
    use std::time::Duration;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deserialize(raw: &str) -> Config {
        let config = config::Config::builder()
            .add_source(config::File::from_str(raw, config::FileFormat::Toml))
            .build()
            .expect("config builder");
        let config: RawConfig = config.try_deserialize().expect("raw config");
        config.into()
    }

    #[test]
    fn defaults_include_a_single_default_backend() {
        let config = Config::default();

        assert_eq!(config.llm.active, "default");
        assert_eq!(config.llm.backends.len(), 1);
        let backend = &config.llm.backends[0];
        assert_eq!(backend.name, "default");
        assert_eq!(backend.backend_type, "ollama");
        assert_eq!(backend.base_url, "http://localhost:11434");
        assert_eq!(backend.model, "qwen2.5-coder:latest");
        assert_eq!(backend.temperature, Some(0.1));
        assert_eq!(backend.max_tokens, Some(2048));
        assert_eq!(backend.context_size, Some(4096));
    }

    #[test]
    fn deserializes_new_llm_backend_schema() {
        let config = deserialize(
            r#"
tick_rate = "500ms"

[llm]
active = "main"

[[llm.backends]]
name = "main"
type = "ollama"
base_url = "http://localhost:11434"
model = "qwen2.5-coder:latest"
temperature = 0.2
max_tokens = 1024
context_size = 2048
min_p = 0.05
seed = 42
"#,
        );

        assert_eq!(config.tick_rate, Duration::from_millis(500));
        assert_eq!(config.llm.active, "main");
        assert_eq!(config.llm.backends.len(), 1);
        let backend = &config.llm.backends[0];
        assert_eq!(backend.name, "main");
        assert_eq!(backend.backend_type, "ollama");
        assert_eq!(backend.temperature, Some(0.2));
        assert_eq!(backend.max_tokens, Some(1024));
        assert_eq!(backend.context_size, Some(2048));
        assert_eq!(backend.min_p, Some(0.05));
        assert_eq!(backend.seed, Some(42));
    }

    #[test]
    fn deserializes_legacy_ollama_section() {
        let config = deserialize(
            r#"
[ollama]
base_url = "http://localhost:11434"
model = "qwen2.5-coder:latest"
"#,
        );

        assert_eq!(config.llm.active, "default");
        assert_eq!(config.llm.backends.len(), 1);
        let backend = &config.llm.backends[0];
        assert_eq!(backend.name, "default");
        assert_eq!(backend.backend_type, "ollama");
        assert_eq!(backend.base_url, "http://localhost:11434");
        assert_eq!(backend.model, "qwen2.5-coder:latest");
    }
}
