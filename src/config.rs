use std::path::Path;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub tick_rate: Duration,
    pub ollama: OllamaConfig,
    pub commands: CommandConfig,
    pub pr: PrConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct OllamaConfig {
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CommandConfig {
    pub jj: String,
    pub git: String,
    pub tea: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct PrConfig {
    pub default_base: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tick_rate: Duration::from_millis(250),
            ollama: OllamaConfig::default(),
            commands: CommandConfig::default(),
            pr: PrConfig::default(),
        }
    }
}

impl Default for OllamaConfig {
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

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut builder = config::Config::builder();

        // Load from default location if exists
        if let Some(config_dir) = dirs::config_dir() {
            let default_path = config_dir.join("teatui").join("config.toml");
            if default_path.exists() {
                builder = builder.add_source(config::File::from(default_path));
            }
        }

        // Load from explicit path if provided
        if let Some(path) = path {
            builder = builder.add_source(config::File::from(path.to_path_buf()));
        }

        // Environment variables with prefix
        builder = builder.add_source(
            config::Environment::with_prefix("TEATUI")
                .separator("_")
                .try_parsing(true),
        );

        let config = builder
            .build()
            .wrap_err("Failed to build configuration")?
            .try_deserialize()
            .wrap_err("Failed to deserialize configuration")?;

        Ok(config)
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
