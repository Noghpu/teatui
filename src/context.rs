use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::config::Config;
use crate::generate::{PrForm, RevsetSummary};
use crate::jj::{JjClient, JjCommand};
use crate::repo::{RemoteInfo, RepoState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCapture {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub parsed_lines: Vec<String>,
}

impl CommandCapture {
    pub fn new(command: impl Into<String>, stdout: String, stderr: String) -> Self {
        let parsed_lines = stdout
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        Self {
            command: command.into(),
            stdout,
            stderr,
            parsed_lines,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentity {
    pub collected_at: SystemTime,
    pub workspace_root: Option<PathBuf>,
    pub remote_url: Option<String>,
    pub base_branch: String,
    pub selected_revset: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBundle {
    pub repo_identity: RepoIdentity,
    pub remote: Option<RemoteInfo>,
    pub form: PrForm,
    pub selected_revset: RevsetSummary,
    pub selected_descriptions: Vec<String>,
    pub status: CommandCapture,
    pub revset_log: CommandCapture,
    pub diff_stats: CommandCapture,
    pub diff: CommandCapture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextError {
    pub command: String,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

impl ContextError {
    pub fn display(&self) -> String {
        format!("{}: {}", self.command, self.message)
    }
}

#[derive(Debug, Clone)]
pub struct ContextCollector {
    client: JjClient,
    repo: RepoState,
    form: PrForm,
    selected_revset: RevsetSummary,
}

impl ContextCollector {
    pub fn new(
        config: &Config,
        repo: RepoState,
        form: PrForm,
        selected_revset: RevsetSummary,
    ) -> Self {
        Self {
            client: JjClient::new(config),
            repo,
            form,
            selected_revset,
        }
    }

    pub fn collect(self) -> std::result::Result<ContextBundle, ContextError> {
        let collected_at = SystemTime::now();
        let workspace_root = self
            .repo
            .workspace_root
            .clone()
            .or_else(|| std::env::current_dir().ok());
        let cwd = workspace_root.clone().unwrap_or_else(|| PathBuf::from("."));

        let status = run_capture(self.client.status_command(&cwd))?;
        let revset_log = run_capture(
            self.client
                .selected_revset_log_command(&cwd, self.selected_revset.label()),
        )?;
        let diff_stats = run_capture(
            self.client
                .selected_revset_diff_stats_command(&cwd, self.selected_revset.label()),
        )?;
        let diff = run_capture(
            self.client
                .selected_revset_diff_command(&cwd, self.selected_revset.label()),
        )?;

        let selected_descriptions = parse_selected_descriptions(&revset_log.stdout);

        Ok(ContextBundle {
            repo_identity: RepoIdentity {
                collected_at,
                workspace_root,
                remote_url: self
                    .repo
                    .remote
                    .as_ref()
                    .map(|remote| remote.raw_url.clone()),
                base_branch: self.repo.base_branch.name.clone(),
                selected_revset: self.selected_revset.label().to_string(),
            },
            remote: self.repo.remote.clone(),
            form: self.form,
            selected_revset: self.selected_revset,
            selected_descriptions,
            status,
            revset_log,
            diff_stats,
            diff,
        })
    }
}

fn parse_selected_descriptions(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_once('|'))
        .filter_map(|(_, tail)| {
            tail.rsplit_once('|')
                .map(|(_, description)| description.trim())
        })
        .filter(|description| !description.is_empty())
        .map(|description| description.to_string())
        .collect()
}

fn run_capture(command: JjCommand) -> std::result::Result<CommandCapture, ContextError> {
    let command_display = command.display();
    let output = match Command::new(&command.program)
        .args(&command.args)
        .current_dir(&command.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            return Err(ContextError {
                command: command_display.clone(),
                message: format!("failed to run {command_display}: {err}"),
                stdout: String::new(),
                stderr: String::new(),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let capture = CommandCapture::new(command_display.clone(), stdout.clone(), stderr.clone());

    if output.status.success() {
        Ok(capture)
    } else {
        Err(ContextError {
            command: command_display.clone(),
            message: if stderr.trim().is_empty() {
                format!("{command_display} exited with {}", output.status)
            } else {
                stderr.trim().to_string()
            },
            stdout,
            stderr,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextResult {
    Ready(Box<ContextBundle>),
    Failed(Box<ContextError>),
}

impl ContextResult {
    pub fn ready(bundle: ContextBundle) -> Self {
        Self::Ready(Box::new(bundle))
    }

    pub fn failed(error: ContextError) -> Self {
        Self::Failed(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::repo::{RemoteInfo, ToolStatus};

    #[test]
    fn parses_selected_descriptions_from_log_output() {
        let output = "a|b||First line\na|b||Second line";

        assert_eq!(
            parse_selected_descriptions(output),
            vec!["First line", "Second line"]
        );
    }

    #[test]
    fn command_capture_tracks_parsed_lines() {
        let capture =
            CommandCapture::new("jj status", "line one\nline two\n".into(), String::new());

        assert_eq!(capture.parsed_lines, vec!["line one", "line two"]);
    }

    #[test]
    fn context_error_display_includes_command_name() {
        let error = ContextError {
            command: "jj status".into(),
            message: "failed".into(),
            stdout: String::new(),
            stderr: String::new(),
        };

        assert_eq!(error.display(), "jj status: failed");
    }

    #[test]
    fn context_bundle_keeps_repo_identity_and_remote() {
        let config = Config::default();
        let repo = RepoState {
            workspace_root: Some(PathBuf::from("C:/repo")),
            inside_workspace: true,
            jj: ToolStatus::Unknown,
            git: ToolStatus::Unknown,
            tea: ToolStatus::Unknown,
            remote: Some(RemoteInfo::parse("git@github.com:owner/repo.git")),
            base_branch: crate::repo::BaseBranchInfo {
                name: config.pr.default_base,
                source: crate::repo::BaseBranchSource::Config,
            },
            ollama_base_url: config.ollama.base_url,
            ollama_model: config.ollama.model,
            blockers: Vec::new(),
        };
        let bundle = ContextBundle {
            repo_identity: RepoIdentity {
                collected_at: SystemTime::now(),
                workspace_root: repo.workspace_root.clone(),
                remote_url: repo.remote.as_ref().map(|remote| remote.raw_url.clone()),
                base_branch: repo.base_branch.name.clone(),
                selected_revset: "@".into(),
            },
            remote: repo.remote.clone(),
            form: PrForm::default(),
            selected_revset: RevsetSummary::new(
                "@",
                "description",
                Vec::new(),
                "1 file changed",
                1,
                vec!["a".into()],
                vec!["b".into()],
                vec!["line".into()],
                Vec::new(),
            ),
            selected_descriptions: vec!["description".into()],
            status: CommandCapture::new("jj status", String::new(), String::new()),
            revset_log: CommandCapture::new("jj log", String::new(), String::new()),
            diff_stats: CommandCapture::new("jj diff --stat", String::new(), String::new()),
            diff: CommandCapture::new("jj diff", String::new(), String::new()),
        };

        assert_eq!(bundle.repo_identity.base_branch, "main");
        assert!(bundle.remote.is_some());
    }
}
