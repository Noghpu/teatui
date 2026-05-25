use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::Config;

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
    pub remote: Option<RemoteInfo>,
    pub base_branch: BaseBranchInfo,
    pub ollama_base_url: String,
    pub ollama_model: String,
    pub blockers: Vec<String>,
}

impl RepoState {
    pub fn bootstrap(config: &Config) -> Self {
        Self {
            workspace_root: None,
            inside_workspace: false,
            jj: ToolStatus::Unknown,
            git: ToolStatus::Unknown,
            tea: ToolStatus::Unknown,
            remote: None,
            base_branch: BaseBranchInfo {
                name: config.pr.default_base.clone(),
                source: BaseBranchSource::Config,
            },
            ollama_base_url: config.ollama.base_url.clone(),
            ollama_model: config.ollama.model.clone(),
            blockers: Vec::new(),
        }
    }

    pub fn blocker_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        if !self.jj.is_available() {
            lines.push("jj is unavailable".into());
        }
        if !self.inside_workspace {
            lines.push("cwd is not inside a jj workspace".into());
        }
        if !self.git.is_available() {
            lines.push("git is unavailable".into());
        }
        if !self.tea.is_available() {
            lines.push("tea is unavailable".into());
        }
        if self.remote.is_none() {
            lines.push("no git remote detected".into());
        }

        lines.extend(self.blockers.iter().cloned());
        lines
    }
}

#[derive(Clone)]
pub struct RepoDiscovery {
    config: Config,
    cwd: PathBuf,
    tx: UnboundedSender<Box<RepoState>>,
}

impl RepoDiscovery {
    pub fn new(config: Config, tx: UnboundedSender<Box<RepoState>>) -> Self {
        Self {
            config,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            tx,
        }
    }

    pub fn refresh(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            let state = discover(this.config, &this.cwd).await;
            let _ = this.tx.send(Box::new(state));
        });
    }
}

pub async fn discover(config: Config, cwd: &Path) -> RepoState {
    let jj = tool_status("jj").await;
    let git = tool_status("git").await;
    let tea = tool_status("tea").await;

    let workspace_root = if jj.is_available() {
        run_output("jj", ["root"], cwd)
            .await
            .ok()
            .map(PathBuf::from)
    } else {
        None
    };

    let inside_workspace = workspace_root.is_some();

    let remote = if git.is_available() {
        match run_output("git", ["remote", "get-url", "origin"], cwd).await {
            Ok(url) => Some(RemoteInfo::parse(url)),
            Err(_) => None,
        }
    } else {
        None
    };

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
        remote,
        base_branch: BaseBranchInfo {
            name: config.pr.default_base,
            source: BaseBranchSource::Config,
        },
        ollama_base_url: config.ollama.base_url,
        ollama_model: config.ollama.model,
        blockers,
    }
}

async fn tool_status(program: &str) -> ToolStatus {
    match Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
    {
        Ok(_) => ToolStatus::Available,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => ToolStatus::Missing,
        Err(err) => ToolStatus::Error(err.to_string()),
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
        .output()
        .await?;

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
}
