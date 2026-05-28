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

/// Returns the default config file candidate path, preferring `XDG_CONFIG_HOME` on Windows when
/// it is set and non-empty (non-whitespace). Falls back to `platform_config_dir` (from
/// `dirs::config_dir()`) when XDG is absent or blank. Returns `None` when neither is available.
///
/// On non-Windows platforms, XDG is already handled by `dirs::config_dir()` so this function
/// always uses `platform_config_dir` directly.
fn default_config_candidate(
    xdg_config_home: Option<std::ffi::OsString>,
    platform_config_dir: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        if let Some(xdg) = xdg_config_home {
            let xdg_str = xdg.to_string_lossy();
            if !xdg_str.trim().is_empty() {
                return Some(
                    std::path::PathBuf::from(xdg.as_os_str())
                        .join("teatui")
                        .join("config.toml"),
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        // On non-Windows, suppress unused-variable warning; XDG is handled by dirs.
        let _ = xdg_config_home;
    }
    platform_config_dir.map(|d| d.join("teatui").join("config.toml"))
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut builder = config::Config::builder();

        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
        let platform_config_dir = dirs::config_dir();
        if let Some(default_path) = default_config_candidate(xdg_config_home, platform_config_dir)
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

        let raw: RawConfig = builder
            .build()
            .wrap_err("Failed to build configuration")?
            .try_deserialize()
            .wrap_err("Failed to deserialize configuration")?;

        let mut config = raw.into();
        apply_legacy_ollama_env_aliases(&mut config);

        Ok(config)
    }
}

fn apply_legacy_ollama_env_aliases(config: &mut Config) {
    apply_legacy_ollama_aliases(
        config,
        std::env::var("TEATUI_OLLAMA_BASE_URL").ok(),
        std::env::var("TEATUI_OLLAMA_MODEL").ok(),
    );
}

fn apply_legacy_ollama_aliases(
    config: &mut Config,
    base_url: Option<String>,
    model: Option<String>,
) {
    let base_url = base_url.filter(|value| !value.trim().is_empty());
    let model = model.filter(|value| !value.trim().is_empty());
    if base_url.is_none() && model.is_none() {
        return;
    }

    let backend_index = config
        .llm
        .backends
        .iter()
        .position(|backend| backend.name == config.llm.active)
        .or_else(|| (!config.llm.backends.is_empty()).then_some(0));

    let backend = match backend_index {
        Some(index) => &mut config.llm.backends[index],
        None => {
            let name = if config.llm.active.trim().is_empty() {
                "default".to_string()
            } else {
                config.llm.active.clone()
            };
            if config.llm.active.trim().is_empty() {
                config.llm.active = name.clone();
            }
            config.llm.backends.push(LlmBackendConfig {
                name,
                ..LlmBackendConfig::default()
            });
            config
                .llm
                .backends
                .last_mut()
                .expect("backend was just inserted")
        }
    };

    if let Some(base_url) = base_url {
        backend.base_url = base_url;
    }
    if let Some(model) = model {
        backend.model = model;
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

    // --- default_config_candidate tests ---

    #[cfg(windows)]
    #[test]
    fn windows_xdg_set_uses_xdg_path() {
        use std::ffi::OsString;
        let xdg: Option<OsString> = Some(OsString::from(r"C:\Users\example\.config"));
        let platform: Option<std::path::PathBuf> = Some(std::path::PathBuf::from(
            r"C:\Users\example\AppData\Roaming",
        ));

        let candidate = default_config_candidate(xdg, platform).expect("should have a path");
        assert_eq!(
            candidate,
            std::path::PathBuf::from(r"C:\Users\example\.config\teatui\config.toml")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_xdg_empty_falls_back_to_platform() {
        use std::ffi::OsString;
        let xdg: Option<OsString> = Some(OsString::from("   "));
        let platform: Option<std::path::PathBuf> = Some(std::path::PathBuf::from(
            r"C:\Users\example\AppData\Roaming",
        ));

        let candidate = default_config_candidate(xdg, platform).expect("should have a path");
        assert_eq!(
            candidate,
            std::path::PathBuf::from(r"C:\Users\example\AppData\Roaming\teatui\config.toml")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_xdg_missing_falls_back_to_platform() {
        let xdg = None;
        let platform: Option<std::path::PathBuf> = Some(std::path::PathBuf::from(
            r"C:\Users\example\AppData\Roaming",
        ));

        let candidate = default_config_candidate(xdg, platform).expect("should have a path");
        assert_eq!(
            candidate,
            std::path::PathBuf::from(r"C:\Users\example\AppData\Roaming\teatui\config.toml")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_no_xdg_and_no_platform_returns_none() {
        let candidate = default_config_candidate(None, None);
        assert!(candidate.is_none());
    }

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

    #[test]
    fn legacy_ollama_env_aliases_update_the_active_backend() {
        let mut config = deserialize(
            r#"
[llm]
active = "main"

[[llm.backends]]
name = "main"
type = "ollama"
base_url = "http://localhost:11434"
model = "qwen2.5-coder:latest"

[[llm.backends]]
name = "backup"
type = "vllm"
base_url = "http://localhost:8000"
model = "qwen2"
"#,
        );

        apply_legacy_ollama_aliases(
            &mut config,
            Some("http://example.test:11434".into()),
            Some("codellama:latest".into()),
        );

        let active = config.llm.active_backend().expect("active backend");
        assert_eq!(active.name, "main");
        assert_eq!(active.base_url, "http://example.test:11434");
        assert_eq!(active.model, "codellama:latest");
        assert_eq!(config.llm.backends[1].base_url, "http://localhost:8000");
    }
}
