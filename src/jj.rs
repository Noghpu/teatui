use std::path::{Path, PathBuf};

use tokio::sync::mpsc::UnboundedSender;

use crate::command::{ExternalCommand, capture};
use crate::config::Config;
use crate::event::BackgroundEvent;
use crate::generate::RevsetSummary;
use crate::generate::StaleCheckResult;

const CANDIDATE_REVSETS: &[&str] = &["@", "@-", "heads(trunk()..)"];

const LOG_TEMPLATE: &str = "commit_id.short() ++ \"|\" ++ change_id.short() ++ \"|\" ++ bookmarks.map(|b| b.name()).join(\",\") ++ \"|\" ++ description.first_line() ++ \"\\n\"";

#[derive(Debug, Clone)]
pub struct JjClient {
    program: String,
}

impl JjClient {
    pub fn new(config: &Config) -> Self {
        Self {
            program: config.commands.jj.clone(),
        }
    }

    pub fn status_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(self.program.clone(), ["--no-pager", "status"], cwd)
    }

    pub fn revset_log_command(&self, cwd: impl Into<PathBuf>, revset: &str) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            [
                "--no-pager",
                "log",
                "--no-graph",
                "-r",
                revset,
                "-T",
                LOG_TEMPLATE,
            ],
            cwd,
        )
    }

    pub fn revset_diff_stats_command(
        &self,
        cwd: impl Into<PathBuf>,
        revset: &str,
    ) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            ["--no-pager", "diff", "-r", revset, "--stat"],
            cwd,
        )
    }

    pub fn revset_diff_command(&self, cwd: impl Into<PathBuf>, revset: &str) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            ["--no-pager", "diff", "-r", revset],
            cwd,
        )
    }

    pub fn bookmark_create_command(
        &self,
        cwd: impl Into<PathBuf>,
        bookmark: &str,
        head: &str,
    ) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            ["--no-pager", "bookmark", "create", bookmark, "-r", head],
            cwd,
        )
    }

    pub fn bookmark_move_command(
        &self,
        cwd: impl Into<PathBuf>,
        bookmark: &str,
        head: &str,
    ) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            ["--no-pager", "bookmark", "move", bookmark, "--to", head],
            cwd,
        )
    }

    pub fn git_push_bookmark_command(
        &self,
        cwd: impl Into<PathBuf>,
        bookmark: &str,
    ) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            ["--no-pager", "git", "push", "--bookmark", bookmark],
            cwd,
        )
    }

    pub async fn candidate_revsets(&self, cwd: &Path) -> Vec<RevsetSummary> {
        let mut summaries = Vec::with_capacity(CANDIDATE_REVSETS.len());
        for revset in CANDIDATE_REVSETS {
            summaries.push(self.candidate_revset_summary(cwd, revset).await);
        }
        summaries
    }

    async fn candidate_revset_summary(&self, cwd: &Path, revset: &str) -> RevsetSummary {
        let log_result = capture(self.revset_log_command(cwd, revset)).await;
        let diff_result = capture(self.revset_diff_stats_command(cwd, revset)).await;

        match (log_result, diff_result) {
            (Ok(log), Ok(diff)) => {
                parse_revset_summary(revset, log.stdout.trim(), diff.stdout.trim(), cwd)
            }
            (Err(err), _) | (_, Err(err)) => failed_revset_summary(revset, err.message),
        }
    }
}

pub fn spawn_revset_discovery(config: &Config, cwd: PathBuf, tx: UnboundedSender<BackgroundEvent>) {
    let client = JjClient::new(config);
    tokio::spawn(async move {
        let summaries = client.candidate_revsets(&cwd).await;
        let _ = tx.send(BackgroundEvent::Revsets(summaries));
    });
}

pub fn spawn_stale_context_check(
    config: &Config,
    cwd: PathBuf,
    selected_revset: String,
    expected_commit_ids: Vec<String>,
    tx: UnboundedSender<BackgroundEvent>,
) {
    let client = JjClient::new(config);
    tokio::spawn(async move {
        let result = match capture(client.revset_log_command(&cwd, &selected_revset)).await {
            Ok(log) => {
                let actual_commit_ids = parse_revset_log_commit_ids(&log.stdout);
                if commit_ids_match_order_independent(&expected_commit_ids, &actual_commit_ids) {
                    StaleCheckResult::Fresh
                } else {
                    StaleCheckResult::Stale {
                        reason: format!(
                            "repo context changed for {selected_revset}; press r to refresh revsets/context"
                        ),
                    }
                }
            }
            Err(error) => StaleCheckResult::Stale {
                reason: format!(
                    "freshness check failed for {selected_revset}: {}",
                    error.display()
                ),
            },
        };

        let _ = tx.send(BackgroundEvent::StaleCheck(result));
    });
}

pub fn parse_revset_log_commit_ids(output: &str) -> Vec<String> {
    parse_log_entries(output)
        .into_iter()
        .map(|entry| entry.commit_id)
        .collect()
}

pub fn commit_ids_match_order_independent(expected: &[String], actual: &[String]) -> bool {
    use std::collections::BTreeSet;

    let expected = expected.iter().cloned().collect::<BTreeSet<_>>();
    let actual = actual.iter().cloned().collect::<BTreeSet<_>>();
    expected == actual
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

    RevsetSummary::new(
        label,
        &description,
        bookmarks,
        diff_output,
        commit_count,
        commit_ids,
        change_ids,
        recent_log,
        warnings,
    )
}

fn failed_revset_summary(label: &str, message: String) -> RevsetSummary {
    RevsetSummary::new(
        label,
        &message,
        Vec::new(),
        "0 files changed, 0 insertions(+), 0 deletions(-)",
        0,
        Vec::new(),
        Vec::new(),
        vec![message.clone()],
        vec![format!("failed to load revset {label}: {message}")],
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
    output.lines().filter_map(parse_log_entry).collect()
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
    fn failed_revset_summary_keeps_candidate_label() {
        let summary = failed_revset_summary("@-", "revset error".into());

        assert_eq!(summary.label(), "@-");
        assert_eq!(summary.commit_count(), 0);
        assert!(summary.warnings()[0].contains("failed to load revset @-"));
    }

    #[test]
    fn builds_revset_log_command_argv() {
        let config = Config::default();
        let client = JjClient::new(&config);
        let command = client.revset_log_command("C:/repo", "@");

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
                LOG_TEMPLATE,
            ]
        );
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }

    #[test]
    fn parses_commit_ids_from_revset_log_output() {
        let output = "abc123|def456|bookmark-a|Fix the parser\nxyz789|uvw000||Second line";

        assert_eq!(
            parse_revset_log_commit_ids(output),
            vec!["abc123".to_string(), "xyz789".to_string()]
        );
    }

    #[test]
    fn builds_bookmark_and_push_commands() {
        let config = Config::default();
        let client = JjClient::new(&config);

        let create = client.bookmark_create_command("C:/repo", "feature/example", "@");
        assert_eq!(
            create.args,
            vec![
                "--no-pager",
                "bookmark",
                "create",
                "feature/example",
                "-r",
                "@"
            ]
        );

        let move_command = client.bookmark_move_command("C:/repo", "feature/example", "@");
        assert_eq!(
            move_command.args,
            vec![
                "--no-pager",
                "bookmark",
                "move",
                "feature/example",
                "--to",
                "@"
            ]
        );

        let push = client.git_push_bookmark_command("C:/repo", "feature/example");
        assert_eq!(
            push.args,
            vec!["--no-pager", "git", "push", "--bookmark", "feature/example"]
        );
    }

    #[test]
    fn compares_commit_ids_without_relying_on_order() {
        let expected = vec!["abc123".into(), "xyz789".into()];
        let actual = vec!["xyz789".into(), "abc123".into()];

        assert!(commit_ids_match_order_independent(&expected, &actual));
    }
}
