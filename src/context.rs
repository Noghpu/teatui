use std::path::PathBuf;
use std::time::SystemTime;

pub use crate::command::CommandCapture;

use crate::command::{CommandError, capture};
use crate::config::Config;
use crate::generate::{PrForm, RevsetSummary};
use crate::jj::JjClient;
use crate::repo::{RemoteInfo, RepoState};

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
pub enum ContextResult {
    Ready(Box<ContextBundle>),
    Failed(Box<CommandError>),
}

impl ContextResult {
    pub fn ready(bundle: ContextBundle) -> Self {
        Self::Ready(Box::new(bundle))
    }

    pub fn failed(error: CommandError) -> Self {
        Self::Failed(Box::new(error))
    }
}

pub async fn collect(
    config: &Config,
    repo: RepoState,
    form: PrForm,
    selected_revset: RevsetSummary,
) -> Result<ContextBundle, CommandError> {
    let client = JjClient::new(config);
    let collected_at = SystemTime::now();
    let workspace_root = repo
        .workspace_root
        .clone()
        .or_else(|| std::env::current_dir().ok());
    let cwd = workspace_root.clone().unwrap_or_else(|| PathBuf::from("."));
    let label = selected_revset.label();

    let status = capture(client.status_command(&cwd)).await?;
    let revset_log = capture(client.revset_log_command(&cwd, label)).await?;
    let diff_stats = capture(client.revset_diff_stats_command(&cwd, label)).await?;
    let diff = capture(client.revset_diff_command(&cwd, label)).await?;

    let selected_descriptions = parse_selected_descriptions(&revset_log.stdout);

    Ok(ContextBundle {
        repo_identity: RepoIdentity {
            collected_at,
            workspace_root,
            remote_url: repo.remote.as_ref().map(|remote| remote.raw_url.clone()),
            base_branch: repo.base_branch.name.clone(),
            selected_revset: label.to_string(),
        },
        remote: repo.remote,
        form,
        selected_revset,
        selected_descriptions,
        status,
        revset_log,
        diff_stats,
        diff,
    })
}

fn parse_selected_descriptions(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(parse_selected_description)
        .filter(|description| !description.is_empty())
        .collect()
}

fn parse_selected_description(line: &str) -> Option<String> {
    let mut parts = line.splitn(4, '|');
    let _commit_id = parts.next()?;
    let _change_id = parts.next()?;
    let _bookmarks = parts.next()?;
    Some(parts.next()?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_selected_descriptions_from_log_output() {
        let output = "a|b||First line\na|b||Second line";

        assert_eq!(
            parse_selected_descriptions(output),
            vec!["First line", "Second line"]
        );
    }

    #[test]
    fn parses_selected_description_with_pipe_in_text() {
        let output = "a|b||First | second";

        assert_eq!(parse_selected_descriptions(output), vec!["First | second"]);
    }
}
