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
    pub base_url: String,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout_secs: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".into(),
            model: "qwen2.5-coder:latest".into(),
            temperature: Some(0.1),
            max_tokens: Some(2048),
            timeout_secs: 30,
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
        assert_eq!(config.llm.base_url, "http://localhost:11434");
        assert_eq!(config.llm.model, "qwen2.5-coder:latest");
        assert_eq!(config.llm.timeout_secs, 30);
    }

    #[test]
    fn llm_overrides_apply() {
        let config = deserialize(
            r#"
[llm]
base_url = "http://example.test:11434"
model = "codellama:7b"
timeout_secs = 60
"#,
        );
        assert_eq!(config.llm.base_url, "http://example.test:11434");
        assert_eq!(config.llm.model, "codellama:7b");
        assert_eq!(config.llm.timeout_secs, 60);
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
