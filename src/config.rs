use std::ffi::OsString;
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
    pub gh: String,
}

impl Default for CommandConfig {
    fn default() -> Self {
        Self {
            jj: "jj".into(),
            git: "git".into(),
            tea: "tea".into(),
            gh: "gh".into(),
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
    /// Byte budget for the aggregate diff sent to this backend. `Some(0)` skips
    /// the diff entirely and sends an adapted prompt (commit messages + per-file
    /// stats only). When unset, the app-wide default budget applies.
    pub diff_budget_bytes: Option<usize>,
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
            diff_budget_bytes: None,
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
    pub forge: ForgeSelection,
}

impl Default for PrConfig {
    fn default() -> Self {
        Self {
            default_base: "main".into(),
            forge: ForgeSelection::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeSelection {
    #[default]
    Auto,
    Gitea,
    Github,
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

/// Where `Config::load` looks for the config file, and where `teatui config
/// --write` writes the example. `None` when the platform has no config dir.
///
/// `XDG_CONFIG_HOME`, when set to an absolute path, wins on every platform —
/// including Windows and macOS, where `dirs::config_dir()` ignores it (it
/// returns `%APPDATA%` and `~/Library/Application Support` respectively). This
/// lets a single XDG layout drive every machine. A relative `XDG_CONFIG_HOME`
/// is ignored per the XDG Base Directory spec, falling back to the platform dir.
pub fn default_config_path() -> Option<PathBuf> {
    config_path_in(std::env::var_os("XDG_CONFIG_HOME"), dirs::config_dir())
}

/// Resolve the config path from the raw `XDG_CONFIG_HOME` value and the platform
/// config dir. Split out from the environment lookup so it can be tested without
/// mutating process-global env vars.
fn config_path_in(
    xdg_config_home: Option<OsString>,
    platform_config_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    xdg_config_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or(platform_config_dir)
        .map(|dir| dir.join("teatui").join("config.toml"))
}

/// An annotated example configuration covering every section, with two example
/// backends and the optional fields documented inline. Kept valid TOML that
/// `Config::load` accepts — `example_config_parses` guards against drift.
pub fn example_config() -> &'static str {
    EXAMPLE_CONFIG
}

const EXAMPLE_CONFIG: &str = r##"# teatui configuration.
# Generated by `teatui config`. Save it to teatui's config directory
# (`teatui config --write` does this for you), typically:
#   ~/.config/teatui/config.toml        (Linux/macOS)
#   %APPDATA%\teatui\config.toml         (Windows)
# Every value below is the built-in default unless noted; delete any line to
# fall back to the default. Environment variables (TEATUI_*) override the file.

[llm]
# Which backend (by name, from the [[llm.backends]] entries) to use. Switch
# between configured backends at runtime in-app with `b`.
active = "default"

# A roomy-context backend: keep the full diff in the prompt.
[[llm.backends]]
name = "default"
type = "ollama"                       # wire protocol: "ollama" or "openai"
base_url = "http://localhost:11434"
model = "qwen2.5-coder:latest"
temperature = 0.1
max_tokens = 2048
timeout_secs = 120
# Byte budget for the diff sent to the LLM. 0 omits the diff entirely and sends
# a commit-message-only prompt; remove the line to use the app default (128 KiB).
diff_budget_bytes = 131072
# Optional, backend-dependent:
# context_size = 131072
# api_key = "..."                     # bearer token for secured openai backends
# min_p = 0.05
# seed = 42

# A small local llama.cpp server (OpenAI-compatible). Its context window is tiny,
# so omit the diff and let the commit messages + per-file stats carry the prompt.
[[llm.backends]]
name = "llama-cpp"
type = "openai"
base_url = "http://127.0.0.1:8080"
model = "qwen3.5-4b"
temperature = 0.1
max_tokens = 2048
timeout_secs = 120
diff_budget_bytes = 0

# External tools teatui shells out to; override with absolute paths if needed.
[commands]
jj = "jj"
git = "git"
tea = "tea"
gh = "gh"

[pr]
# Default base revision/branch a pull request is opened against.
default_base = "main"
# Forge CLI selection. "auto" uses GitHub CLI for github.com remotes and Gitea
# tea everywhere else. Set "github" or "gitea" to lock the integration.
forge = "auto"
"##;

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
    fn example_config_parses_and_matches_schema() {
        // Guards against the hand-written example drifting from the live schema:
        // it must deserialize and exercise the documented fields, including the
        // two diff-budget extremes (explicit large vs. 0 to omit the diff).
        let config = deserialize(example_config());
        assert_eq!(config.llm.active, "default");
        let default = config
            .llm
            .backends
            .iter()
            .find(|b| b.name == "default")
            .expect("default backend");
        assert_eq!(default.api, LlmApi::Ollama);
        assert_eq!(default.diff_budget_bytes, Some(131072));
        let llama = config
            .llm
            .backends
            .iter()
            .find(|b| b.name == "llama-cpp")
            .expect("llama-cpp backend");
        assert_eq!(llama.api, LlmApi::Openai);
        assert_eq!(llama.diff_budget_bytes, Some(0));
        assert_eq!(config.pr.default_base, "main");
        assert_eq!(config.pr.forge, ForgeSelection::Auto);
    }

    #[test]
    fn defaults_use_bare_tool_names() {
        let config = Config::default();
        assert_eq!(config.commands.jj, "jj");
        assert_eq!(config.commands.git, "git");
        assert_eq!(config.commands.tea, "tea");
        assert_eq!(config.commands.gh, "gh");
    }

    #[test]
    fn overrides_tool_paths() {
        let config = deserialize(
            r#"
[commands]
jj = "/usr/local/bin/jj"
tea = "/opt/tea/bin/tea"
gh = "/usr/local/bin/gh"
"#,
        );
        assert_eq!(config.commands.jj, "/usr/local/bin/jj");
        assert_eq!(config.commands.git, "git");
        assert_eq!(config.commands.tea, "/opt/tea/bin/tea");
        assert_eq!(config.commands.gh, "/usr/local/bin/gh");
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
    fn llm_backend_diff_budget_defaults_unset_and_parses_zero() {
        // Unset means "use the app default"; an explicit 0 means "omit the diff".
        assert!(LlmBackend::default().diff_budget_bytes.is_none());
        let config = deserialize(
            r#"
[[llm.backends]]
name = "lean"
base_url = "http://localhost:8080"
model = "small"
diff_budget_bytes = 0
"#,
        );
        assert_eq!(config.llm.active_backend().diff_budget_bytes, Some(0));
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
    fn xdg_config_home_absolute_supersedes_platform_dir() {
        // An absolute XDG_CONFIG_HOME wins even when a platform dir exists —
        // this is the Windows case where dirs::config_dir() would point at
        // %APPDATA% but the user keeps config under XDG_CONFIG_HOME.
        let xdg = std::env::temp_dir().join("xdg-home");
        let platform = std::env::temp_dir().join("platform-home");
        let got = config_path_in(Some(xdg.clone().into_os_string()), Some(platform));
        assert_eq!(got, Some(xdg.join("teatui").join("config.toml")));
    }

    #[test]
    fn relative_xdg_config_home_is_ignored() {
        // The XDG spec says a relative XDG_CONFIG_HOME must be ignored; fall
        // back to the platform dir rather than resolving it against the cwd.
        let platform = std::env::temp_dir().join("platform-home");
        let got = config_path_in(
            Some(OsString::from("relative/config")),
            Some(platform.clone()),
        );
        assert_eq!(got, Some(platform.join("teatui").join("config.toml")));
    }

    #[test]
    fn falls_back_to_platform_dir_without_xdg() {
        let platform = std::env::temp_dir().join("platform-home");
        let got = config_path_in(None, Some(platform.clone()));
        assert_eq!(got, Some(platform.join("teatui").join("config.toml")));
    }

    #[test]
    fn no_config_path_when_nothing_resolves() {
        assert_eq!(config_path_in(None, None), None);
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

    #[test]
    fn pr_forge_selection_defaults_auto_and_overrides() {
        assert_eq!(Config::default().pr.forge, ForgeSelection::Auto);
        let config = deserialize(
            r#"
[pr]
forge = "github"
"#,
        );
        assert_eq!(config.pr.forge, ForgeSelection::Github);
    }
}
