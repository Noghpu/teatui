use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::command::{CommandError, ExternalCommand, capture};
use futures::future::join_all;
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::timeout;

use crate::config::Config;
use crate::event::BackgroundEvent;
use crate::llm::LlmClient;
use crate::tea::TeaClient;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    Unknown,
    Available,
    Missing,
    Error(String),
}

impl ToolStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Missing => "missing",
            Self::Error(_) => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeaAuth {
    Unknown(String),
    NotConfigured,
    Configured { host: String, user: Option<String> },
    Error(String),
}

impl TeaAuth {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Unknown(_) => "(unknown)",
            Self::NotConfigured => "(not configured)",
            Self::Configured { .. } => "configured",
            Self::Error(_) => "error",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Unknown(reason) | Self::Error(reason) if !reason.is_empty() => {
                Some(reason.as_str())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmStatus {
    Unknown(String),
    Reachable,
    Unreachable(String),
}

impl LlmStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Unknown(_) => "(unknown)",
            Self::Reachable => "reachable",
            Self::Unreachable(_) => "unreachable",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Unknown(reason) | Self::Unreachable(reason) if !reason.is_empty() => {
                Some(reason.as_str())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmBackendStatus {
    pub name: String,
    pub backend_type: String,
    pub base_url: String,
    pub model: String,
    pub status: LlmStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInfo {
    pub raw_url: String,
    pub host: String,
    pub owner: String,
    pub name: String,
    pub warning: Option<String>,
}

impl RemoteInfo {
    pub fn display_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn parse(raw_url: impl Into<String>) -> Self {
        let raw_url = raw_url.into();

        match parse_remote_url(&raw_url) {
            Some((host, owner, name)) => Self {
                raw_url,
                host,
                owner,
                name,
                warning: None,
            },
            None => Self {
                raw_url,
                host: String::new(),
                owner: String::new(),
                name: String::new(),
                warning: Some("unrecognized remote format".into()),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BaseBranchSource {
    Config,
    Discovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseBranchInfo {
    pub name: String,
    pub source: BaseBranchSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoState {
    pub workspace_root: Option<PathBuf>,
    pub inside_workspace: bool,
    pub jj: ToolStatus,
    pub git: ToolStatus,
    pub tea: ToolStatus,
    pub tea_auth: TeaAuth,
    pub remote: Option<RemoteInfo>,
    pub base_branch: BaseBranchInfo,
    pub llm_active: String,
    pub llm_backends: Vec<LlmBackendStatus>,
    pub blockers: Vec<String>,
}

impl RepoState {
    pub fn new(config: &Config) -> Self {
        Self {
            workspace_root: None,
            inside_workspace: false,
            jj: ToolStatus::Unknown,
            git: ToolStatus::Unknown,
            tea: ToolStatus::Unknown,
            tea_auth: TeaAuth::Unknown("pending discovery".into()),
            remote: None,
            base_branch: BaseBranchInfo {
                name: config.pr.default_base.clone(),
                source: BaseBranchSource::Config,
            },
            llm_active: config.llm.active.clone(),
            llm_backends: config
                .llm
                .backends
                .iter()
                .map(|backend| LlmBackendStatus {
                    name: backend.name.clone(),
                    backend_type: backend.backend_type.clone(),
                    base_url: backend.base_url.clone(),
                    model: backend.model.clone(),
                    status: LlmStatus::Unknown("pending discovery".into()),
                })
                .collect(),
            blockers: Vec::new(),
        }
    }

    pub fn bootstrap(config: &Config) -> Self {
        Self::new(config)
    }

    pub fn blocker_lines(&self) -> Vec<String> {
        self.blockers.clone()
    }
}

pub fn spawn_discovery(config: Config, cwd: PathBuf, tx: UnboundedSender<BackgroundEvent>) {
    tokio::spawn(async move {
        let state = discover(config, &cwd).await;
        let _ = tx.send(BackgroundEvent::Repo(Box::new(state)));
    });
}

pub async fn discover(config: Config, cwd: &Path) -> RepoState {
    let commands = config.commands.clone();
    let tea_client = TeaClient::new(&config);
    let backends = config.llm.backends.clone();
    let llm_checks = join_all(
        backends
            .iter()
            .map(|backend| async move { LlmClient::health_check_for(backend).await }),
    );

    let (jj, git, tea, workspace_root, remote_url, tea_login_list, llm_statuses) = tokio::join!(
        tool_status(&commands.jj),
        tool_status(&commands.git),
        tea_status(&tea_client, cwd),
        run_output(&commands.jj, ["--no-pager", "root"], cwd),
        run_output(&commands.git, ["remote", "get-url", "origin"], cwd),
        tea_login_list_output(&tea_client, cwd),
        llm_checks,
    );

    let workspace_root = workspace_root.ok().map(PathBuf::from);
    let inside_workspace = workspace_root.is_some();
    let remote = remote_url.ok().map(RemoteInfo::parse);
    let tea_auth = tea_auth_status(&tea, remote.as_ref(), tea_login_list);

    let mut blockers = Vec::new();
    if !jj.is_available() {
        blockers.push("install or configure `jj`".into());
    }
    if !inside_workspace {
        blockers.push("open teatui from inside a jj workspace".into());
    }
    if !git.is_available() {
        blockers.push("install or configure `git`".into());
    }
    if !tea.is_available() {
        blockers.push("install or configure `tea`".into());
    }
    if remote.is_none() {
        blockers.push("configure a git `origin` remote".into());
    }

    RepoState {
        workspace_root,
        inside_workspace,
        jj,
        git,
        tea,
        tea_auth,
        remote,
        base_branch: BaseBranchInfo {
            name: config.pr.default_base,
            source: BaseBranchSource::Config,
        },
        llm_active: config.llm.active,
        llm_backends: backends
            .into_iter()
            .zip(llm_statuses)
            .map(|(backend, status)| LlmBackendStatus {
                name: backend.name,
                backend_type: backend.backend_type,
                base_url: backend.base_url,
                model: backend.model,
                status,
            })
            .collect(),
        blockers,
    }
}

async fn tea_status(client: &TeaClient, cwd: &Path) -> ToolStatus {
    capture_tool_status(client.version_command(cwd)).await
}

async fn tea_login_list_output(
    client: &TeaClient,
    cwd: &Path,
) -> Result<crate::command::CommandCapture, CommandError> {
    let mut command = client.login_list_command(cwd);
    command.timeout = DISCOVERY_TIMEOUT;
    capture(command).await
}

async fn capture_tool_status(command: ExternalCommand) -> ToolStatus {
    let mut command = command;
    command.timeout = DISCOVERY_TIMEOUT;
    match capture(command).await {
        Ok(_) => ToolStatus::Available,
        Err(err) if looks_missing(&err.message) => ToolStatus::Missing,
        Err(err) => ToolStatus::Error(err.message),
    }
}

fn tea_auth_status(
    tea: &ToolStatus,
    remote: Option<&RemoteInfo>,
    tea_login_list: Result<crate::command::CommandCapture, CommandError>,
) -> TeaAuth {
    let Some(remote) = remote.filter(|remote| !remote.host.is_empty()) else {
        return TeaAuth::Unknown("remote host unavailable".into());
    };

    match (tea, tea_login_list) {
        (ToolStatus::Available, Ok(output)) => parse_tea_login_list(&output.stdout, &remote.host),
        (ToolStatus::Available, Err(err)) => TeaAuth::Error(err.message),
        (ToolStatus::Missing, _) => TeaAuth::Unknown("tea binary is missing".into()),
        (ToolStatus::Error(message), _) => TeaAuth::Error(message.clone()),
        (ToolStatus::Unknown, _) => TeaAuth::Unknown("tea status unavailable".into()),
    }
}

fn looks_missing(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("not found")
        || message.contains("cannot find the file")
        || message.contains("file not found")
}

pub fn parse_tea_login_list(stdout: &str, host: &str) -> TeaAuth {
    let host = host.trim();
    if host.is_empty() {
        return TeaAuth::Unknown("remote host unavailable".into());
    }

    let mut saw_data = false;
    let mut saw_parseable_line = false;

    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        saw_data = true;
        let tokens = line
            .split_whitespace()
            .map(normalize_token)
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();

        if tokens.is_empty() {
            continue;
        }
        if tokens.len() >= 2
            || tokens.iter().any(|token| {
                token.contains('.')
                    || token.contains('/')
                    || token.contains('@')
                    || token.contains(':')
            })
        {
            saw_parseable_line = true;
        }

        if let Some((index, matched_host)) = tokens
            .iter()
            .enumerate()
            .find(|(_, token)| token_matches_host(token, host))
        {
            let user = adjacent_user_token(&tokens, index, matched_host);
            return TeaAuth::Configured {
                host: host.to_string(),
                user,
            };
        }
    }

    if saw_data && saw_parseable_line {
        TeaAuth::NotConfigured
    } else {
        TeaAuth::Unknown("unable to parse tea login list output".into())
    }
}

fn normalize_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| {
            matches!(
                ch,
                ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\''
            )
        })
        .trim_end_matches(".git")
        .to_string()
}

fn token_matches_host(token: &str, host: &str) -> bool {
    let token = normalize_host_key(token);
    let host = normalize_host_key(host);
    if token.is_empty() || host.is_empty() {
        return false;
    }

    token == host || token.ends_with(&format!(".{host}"))
}

fn normalize_host_key(candidate: &str) -> String {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return String::new();
    }

    let candidate = candidate
        .rsplit_once('@')
        .map(|(_, value)| value)
        .unwrap_or(candidate);
    let candidate = candidate
        .split_once("://")
        .map(|(_, value)| value)
        .unwrap_or(candidate);
    let candidate = candidate
        .split_once('/')
        .map(|(value, _)| value)
        .unwrap_or(candidate);
    let candidate = candidate
        .split_once(':')
        .map(|(value, _)| value)
        .unwrap_or(candidate);

    candidate
        .trim_matches(|ch: char| matches!(ch, ',' | ';' | ')' | '('))
        .to_ascii_lowercase()
        .to_string()
}

fn adjacent_user_token(tokens: &[String], index: usize, matched_host: &str) -> Option<String> {
    let candidate_after = tokens
        .get(index + 1)
        .filter(|token| !token_matches_host(token, matched_host))
        .cloned();
    if candidate_after.is_some() {
        return candidate_after;
    }

    tokens
        .get(index.wrapping_sub(1))
        .filter(|token| !token_matches_host(token, matched_host))
        .cloned()
}

async fn tool_status(program: &str) -> ToolStatus {
    let output = Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();

    match timeout(DISCOVERY_TIMEOUT, output).await {
        Ok(Ok(output)) if output.status.success() => ToolStatus::Available,
        Ok(Ok(output)) => ToolStatus::Error(format!(
            "`{program} --version` exited with {}",
            output.status
        )),
        Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => ToolStatus::Missing,
        Ok(Err(err)) => ToolStatus::Error(err.to_string()),
        Err(_) => ToolStatus::Error(format!("`{program} --version` timed out")),
    }
}

async fn run_output<const N: usize>(
    program: &str,
    args: [&str; N],
    cwd: &Path,
) -> std::io::Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = timeout(DISCOVERY_TIMEOUT, output)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "command timed out"))??;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn parse_remote_url(raw: &str) -> Option<(String, String, String)> {
    let trimmed = raw.trim_end_matches(".git");

    if let Some((prefix, path)) = trimmed.split_once(':')
        && prefix.contains('@')
    {
        let host = prefix.rsplit_once('@').map(|(_, host)| host)?.to_string();
        return parse_owner_repo(host, path);
    }

    if let Some((scheme, rest)) = trimmed.split_once("://") {
        let host_and_path = rest.split_once('/')?;
        let authority = host_and_path.0;
        let path = host_and_path.1;
        let host = authority
            .rsplit_once('@')
            .map(|(_, host)| host)
            .unwrap_or(authority);
        let _ = scheme;
        return parse_owner_repo(host.to_string(), path);
    }

    None
}

fn parse_owner_repo(host: String, path: &str) -> Option<(String, String, String)> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let owner = segments.next()?.to_string();
    let name = segments.next()?.to_string();
    if segments.next().is_some() {
        return None;
    }
    Some((host, owner, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_remote() {
        let remote = RemoteInfo::parse("git@code.example.com:team/project.git");
        assert_eq!(remote.host, "code.example.com");
        assert_eq!(remote.owner, "team");
        assert_eq!(remote.name, "project");
        assert_eq!(remote.warning, None);
    }

    #[test]
    fn parses_https_remote() {
        let remote = RemoteInfo::parse("https://code.example.com/team/project.git");
        assert_eq!(remote.host, "code.example.com");
        assert_eq!(remote.owner, "team");
        assert_eq!(remote.name, "project");
        assert_eq!(remote.warning, None);
    }

    #[test]
    fn parses_ssh_remote_with_port() {
        let remote = RemoteInfo::parse("ssh://git@code.example.com:2222/team/project.git");
        assert_eq!(remote.host, "code.example.com:2222");
        assert_eq!(remote.owner, "team");
        assert_eq!(remote.name, "project");
        assert_eq!(remote.warning, None);
    }

    #[test]
    fn warns_on_non_owner_repo_remote_path() {
        let remote = RemoteInfo::parse("https://code.example.com/scm/team/project.git");
        assert!(remote.warning.is_some());
        assert_eq!(
            remote.raw_url,
            "https://code.example.com/scm/team/project.git"
        );
    }

    #[test]
    fn parses_tea_login_list_match_with_user() {
        let auth = parse_tea_login_list(
            r#"
host user
code.example.com alice
"#,
            "code.example.com",
        );

        assert_eq!(
            auth,
            TeaAuth::Configured {
                host: "code.example.com".into(),
                user: Some("alice".into()),
            }
        );
    }

    #[test]
    fn reports_not_configured_when_host_is_missing() {
        let auth = parse_tea_login_list(
            r#"
host user
other.example.com bob
"#,
            "code.example.com",
        );

        assert_eq!(auth, TeaAuth::NotConfigured);
    }

    #[test]
    fn parses_tea_login_list_match_from_url_with_portless_remote_host() {
        let auth = parse_tea_login_list(
            r#"
Name URL User Default
gitea https://code.example.com alice true
"#,
            "code.example.com:2222",
        );

        assert_eq!(
            auth,
            TeaAuth::Configured {
                host: "code.example.com:2222".into(),
                user: Some("alice".into()),
            }
        );
    }

    #[test]
    fn does_not_match_partial_host_suffixes() {
        let auth = parse_tea_login_list(
            r#"
host user
notcode.example.com alice
"#,
            "code.example.com",
        );

        assert_eq!(auth, TeaAuth::NotConfigured);
    }

    #[test]
    fn reports_unknown_for_unparseable_output() {
        let auth = parse_tea_login_list("!!!", "code.example.com");

        assert!(matches!(auth, TeaAuth::Unknown(reason) if reason.contains("unable to parse")));
    }
}
