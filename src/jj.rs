use std::path::{Path, PathBuf};
use std::process::Stdio;

use color_eyre::eyre::{Result, WrapErr};
use std::process::Command;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::Config;
use crate::generate::{RevsetSummary, RevsetUpdate};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl JjCommand {
    pub fn new<I, S>(program: impl Into<String>, args: I, cwd: impl Into<PathBuf>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: cwd.into(),
        }
    }
}

#[derive(Clone)]
pub struct JjClient {
    program: String,
}

impl JjClient {
    pub fn new(config: &Config) -> Self {
        Self {
            program: config.commands.jj.clone(),
        }
    }

    pub fn candidate_revsets_command(&self, cwd: impl Into<PathBuf>, revset: &str) -> JjCommand {
        JjCommand::new(
            self.program.clone(),
            [
                "--no-pager",
                "log",
                "--no-graph",
                "-r",
                revset,
                "-T",
                "commit_id.short() ++ \"|\" ++ change_id.short() ++ \"|\" ++ bookmarks.map(|b| b.name()).join(\",\") ++ \"|\" ++ description.first_line() ++ \"\\n\"",
            ],
            cwd,
        )
    }

    pub fn candidate_revsets_diff_command(
        &self,
        cwd: impl Into<PathBuf>,
        revset: &str,
    ) -> JjCommand {
        JjCommand::new(
            self.program.clone(),
            ["--no-pager", "diff", "-r", revset, "--stat"],
            cwd,
        )
    }

    pub fn candidate_revsets(&self, cwd: impl AsRef<Path>) -> Result<Vec<RevsetSummary>> {
        let cwd = cwd.as_ref().to_path_buf();
        let revsets = ["@", "@-", "heads(trunk()..)"];
        let mut summaries = Vec::with_capacity(revsets.len());

        for revset in revsets {
            summaries.push(self.candidate_revset_summary(&cwd, revset)?);
        }

        Ok(summaries)
    }

    pub fn candidate_revset_summary(
        &self,
        cwd: impl AsRef<Path>,
        revset: &str,
    ) -> Result<RevsetSummary> {
        let cwd = cwd.as_ref().to_path_buf();
        let log_command = self.candidate_revsets_command(&cwd, revset);
        let diff_command = self.candidate_revsets_diff_command(&cwd, revset);

        let log_output = run_command(&log_command)?;
        let diff_output = run_command(&diff_command)?;

        Ok(parse_revset_summary(
            revset,
            &log_output,
            &diff_output,
            &cwd,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLogEntry {
    commit_id: String,
    change_id: String,
    bookmarks: Vec<String>,
    description: String,
}

fn parse_revset_summary(
    label: &str,
    log_output: &str,
    diff_output: &str,
    cwd: &Path,
) -> RevsetSummary {
    let entries = parse_log_entries(log_output);
    let commit_count = entries.len();
    let commit_ids = entries
        .iter()
        .map(|entry| entry.commit_id.clone())
        .collect();
    let change_ids = entries
        .iter()
        .map(|entry| entry.change_id.clone())
        .collect();
    let bookmarks = collect_bookmarks(&entries);
    let recent_log = entries
        .iter()
        .map(|entry| {
            let bookmarks = if entry.bookmarks.is_empty() {
                String::new()
            } else {
                format!(" [{}]", entry.bookmarks.join(", "))
            };
            format!(
                "{} {} {}{}",
                entry.commit_id, entry.change_id, entry.description, bookmarks
            )
        })
        .collect::<Vec<_>>();
    let warnings = revset_warnings(label, commit_count, cwd, log_output, diff_output);
    let description = entries
        .first()
        .map(|entry| entry.description.clone())
        .unwrap_or_else(|| "No commits matched".into());
    let stats = diff_output.trim().to_string();
    let bookmark_refs = bookmarks.clone();

    RevsetSummary::new(
        label,
        &description,
        bookmark_refs,
        &stats,
        commit_count,
        commit_ids,
        change_ids,
        recent_log,
        warnings,
    )
}

fn revset_warnings(
    label: &str,
    commit_count: usize,
    cwd: &Path,
    log_output: &str,
    diff_output: &str,
) -> Vec<String> {
    let mut warnings = Vec::new();

    if commit_count == 0 {
        warnings.push(format!("revset {label} is empty"));
    }
    if commit_count > 1 {
        warnings.push(format!("revset {label} contains {commit_count} commits"));
    }
    if log_output.trim().is_empty() {
        warnings.push(format!("revset {label} did not return log output"));
    }
    if diff_output.trim().is_empty() {
        warnings.push(format!("revset {label} did not return diff stats"));
    }
    if !cwd.exists() {
        warnings.push("workspace path is unavailable".into());
    }

    warnings
}

fn collect_bookmarks(entries: &[ParsedLogEntry]) -> Vec<String> {
    let mut bookmarks = Vec::new();
    for bookmark in entries.iter().flat_map(|entry| entry.bookmarks.iter()) {
        if !bookmarks.iter().any(|existing| existing == bookmark) {
            bookmarks.push(bookmark.clone());
        }
    }
    bookmarks
}

fn parse_log_entries(output: &str) -> Vec<ParsedLogEntry> {
    output
        .lines()
        .filter_map(parse_log_entry)
        .collect::<Vec<_>>()
}

fn parse_log_entry(line: &str) -> Option<ParsedLogEntry> {
    let mut parts = line.splitn(4, '|');
    let commit_id = parts.next()?.trim();
    let change_id = parts.next()?.trim();
    let bookmarks = parts
        .next()?
        .split(',')
        .filter(|bookmark| !bookmark.trim().is_empty())
        .map(|bookmark| bookmark.trim().to_string())
        .collect::<Vec<_>>();
    let description = parts.next()?.trim();

    Some(ParsedLogEntry {
        commit_id: commit_id.to_string(),
        change_id: change_id.to_string(),
        bookmarks,
        description: description.to_string(),
    })
}

fn run_command(command: &JjCommand) -> Result<String> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .current_dir(&command.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .wrap_err("failed to run jj command")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(color_eyre::eyre::eyre!(
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ))
    }
}

#[derive(Clone)]
pub struct RevsetDiscovery {
    client: JjClient,
    cwd: PathBuf,
    tx: UnboundedSender<Box<RevsetUpdate>>,
}

impl RevsetDiscovery {
    pub fn new(
        config: &Config,
        cwd: impl Into<PathBuf>,
        tx: UnboundedSender<Box<RevsetUpdate>>,
    ) -> Self {
        Self {
            client: JjClient::new(config),
            cwd: cwd.into(),
            tx,
        }
    }

    pub fn refresh(&self) {
        let client = self.client.clone();
        let cwd = self.cwd.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let summaries = tokio::task::spawn_blocking(move || client.candidate_revsets(&cwd))
                .await
                .unwrap_or_else(|err| Err(color_eyre::eyre::eyre!(err.to_string())))
                .unwrap_or_else(|err| {
                    let message = err.to_string();
                    let description = message.clone();
                    vec![RevsetSummary::new(
                        "(revset discovery failed)",
                        &description,
                        Vec::new(),
                        "0 files changed, 0 insertions(+), 0 deletions(-)",
                        0,
                        Vec::new(),
                        Vec::new(),
                        vec![message.clone()],
                        vec![message],
                    )]
                });
            let _ = tx.send(Box::new(RevsetUpdate::new(summaries)));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_entry_line() {
        let line = "abc123|def456|bookmark-a,bookmark-b|Fix the parser";
        let entry = parse_log_entry(line).expect("entry");
        assert_eq!(entry.commit_id, "abc123");
        assert_eq!(entry.change_id, "def456");
        assert_eq!(entry.bookmarks, vec!["bookmark-a", "bookmark-b"]);
        assert_eq!(entry.description, "Fix the parser");
    }

    #[test]
    fn builds_candidate_revset_command_argv() {
        let config = Config::default();
        let client = JjClient::new(&config);
        let command = client.candidate_revsets_command("C:/repo", "@");

        assert_eq!(command.program, "jj");
        assert_eq!(
            command.args,
            vec![
                "--no-pager",
                "log",
                "--no-graph",
                "-r",
                "@",
                "-T",
                "commit_id.short() ++ \"|\" ++ change_id.short() ++ \"|\" ++ bookmarks.map(|b| b.name()).join(\",\") ++ \"|\" ++ description.first_line() ++ \"\\n\"",
            ]
        );
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }
}
