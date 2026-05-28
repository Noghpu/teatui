use std::path::{Path, PathBuf};

use tokio::sync::mpsc::UnboundedSender;

use crate::command::{ExternalCommand, capture};
use crate::config::Config;
use crate::event::BackgroundEvent;
use crate::generate::RevsetSummary;
use crate::generate::StaleCheckResult;

const TRUNK_RANGE_REVSET: &str = "trunk()..@";

// The template emits one record per line. Fields are pipe-separated; the
// description field uses the full multi-line description with its newlines
// replaced by the ASCII unit-separator (\x1F) so the whole record fits on one
// line. A literal \x1E (record separator) is appended as an end-of-record
// marker so the parser can reliably detect the boundary after splitting on \n.
const LOG_TEMPLATE: &str = "commit_id.short() ++ \"|\" ++ change_id.short() ++ \"|\" ++ bookmarks.map(|b| b.name()).join(\",\") ++ \"|\" ++ description.lines().join(\"\\x1F\") ++ \"\\x1E\\n\"";

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

    pub async fn per_change_revsets(&self, cwd: &Path) -> Vec<RevsetSummary> {
        let log_result = capture(self.revset_log_command(cwd, TRUNK_RANGE_REVSET)).await;

        let entries = match log_result {
            Err(err) => {
                return vec![failed_revset_summary(
                    TRUNK_RANGE_REVSET,
                    format!("failed to enumerate changes: {}", err.message),
                )];
            }
            Ok(log) => parse_log_entries(log.stdout.trim()),
        };

        if entries.is_empty() {
            return vec![no_changes_placeholder()];
        }

        let mut summaries = Vec::with_capacity(entries.len());
        for entry in &entries {
            let revset_label = format!("trunk()..{}", entry.change_id);
            let diff_result = capture(self.revset_diff_stats_command(cwd, &revset_label)).await;
            let diff_output = match diff_result {
                Ok(diff) => diff.stdout,
                Err(_) => "0 files changed, 0 insertions(+), 0 deletions(-)".to_string(),
            };

            // Reconstruct a log line that includes the full description encoded
            // with \x1F separators so parse_revset_summary can recover the body.
            let desc_encoded = if entry.description_body.is_empty() {
                entry.description.clone()
            } else {
                format!(
                    "{}\x1F{}",
                    entry.description,
                    entry.description_body.replace('\n', "\x1F")
                )
            };
            let log_line = format!(
                "{}|{}|{}|{}\x1E",
                entry.commit_id,
                entry.change_id,
                entry.bookmarks.join(","),
                desc_encoded
            );
            let summary = parse_revset_summary(&revset_label, &log_line, diff_output.trim(), cwd);
            summaries.push(summary);
        }
        summaries
    }
}

pub fn spawn_revset_discovery(config: &Config, cwd: PathBuf, tx: UnboundedSender<BackgroundEvent>) {
    let client = JjClient::new(config);
    tokio::spawn(async move {
        let summaries = client.per_change_revsets(&cwd).await;
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
    description_body: String,
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
    let description_body = entries
        .first()
        .map(|entry| entry.description_body.clone())
        .unwrap_or_default();

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
    .with_description_body(description_body)
}

fn no_changes_placeholder() -> RevsetSummary {
    RevsetSummary::new(
        "(no changes above trunk())",
        "no changes above trunk()",
        Vec::new(),
        "0 files changed, 0 insertions(+), 0 deletions(-)",
        0,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec!["no changes above trunk()".into()],
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
    // Strip the trailing record-separator (\x1E) if present (new template format).
    let line = line.trim_end_matches('\x1E').trim_end();
    let mut parts = line.splitn(4, '|');
    let commit_id = parts.next()?.trim();
    let change_id = parts.next()?.trim();
    let bookmarks = parts
        .next()?
        .split(',')
        .filter(|bookmark| !bookmark.trim().is_empty())
        .map(|bookmark| bookmark.trim().to_string())
        .collect::<Vec<_>>();
    // The description field uses \x1F as line separator (new template) or is a
    // plain first-line string (legacy). Split on \x1F; the first segment is the
    // subject (first_line) and the rest form the body.
    let raw_desc = parts.next()?;
    let desc_parts: Vec<&str> = raw_desc.split('\x1F').collect();
    let first_line = desc_parts[0].trim();
    let body_lines = &desc_parts[1..];
    // Remove trailing empty/whitespace lines from body.
    let body = body_lines
        .iter()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();

    Some(ParsedLogEntry {
        commit_id: commit_id.to_string(),
        change_id: change_id.to_string(),
        bookmarks,
        description: first_line.to_string(),
        description_body: body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_entry_line_legacy_format() {
        // Legacy format: no \x1E terminator, plain first-line description.
        let line = "abc123|def456|bookmark-a,bookmark-b|Fix the parser";
        let entry = parse_log_entry(line).expect("entry");
        assert_eq!(entry.commit_id, "abc123");
        assert_eq!(entry.change_id, "def456");
        assert_eq!(entry.bookmarks, vec!["bookmark-a", "bookmark-b"]);
        assert_eq!(entry.description, "Fix the parser");
        assert_eq!(entry.description_body, "");
    }

    #[test]
    fn parses_log_entry_line_new_format_with_body() {
        // New template format: description lines joined with \x1F, record ends with \x1E.
        let line =
            "abc123|def456|feature/foo|Fix the parser\x1FThis is the body.\x1FMore details.\x1E";
        let entry = parse_log_entry(line).expect("entry");
        assert_eq!(entry.commit_id, "abc123");
        assert_eq!(entry.change_id, "def456");
        assert_eq!(entry.bookmarks, vec!["feature/foo"]);
        assert_eq!(entry.description, "Fix the parser");
        assert_eq!(entry.description_body, "This is the body.\nMore details.");
    }

    #[test]
    fn parses_log_entry_line_new_format_no_body() {
        // New format, single-line description (no body after the first \x1F segment).
        let line = "abc123|def456||Fix the parser\x1E";
        let entry = parse_log_entry(line).expect("entry");
        assert_eq!(entry.description, "Fix the parser");
        assert_eq!(entry.description_body, "");
    }

    #[test]
    fn failed_revset_summary_keeps_label() {
        let summary = failed_revset_summary("trunk()..abc123", "revset error".into());

        assert_eq!(summary.label(), "trunk()..abc123");
        assert_eq!(summary.commit_count(), 0);
        assert!(summary.warnings()[0].contains("failed to load revset trunk()..abc123"));
    }

    #[test]
    fn per_change_revsets_parse_produces_correct_labels() {
        // Simulate what per_change_revsets produces by calling parse_revset_summary directly
        // for each entry in a multi-entry log (new template format with \x1E terminators).
        let log_output = "abc123|def456|feature/foo|First change\x1FBody line one\x1E\nxyz789|uvw000||Second change no bookmark\x1E";
        let entries = parse_log_entries(log_output);
        assert_eq!(entries.len(), 2);

        let entry0 = &entries[0];
        assert_eq!(entry0.description, "First change");
        assert_eq!(entry0.description_body, "Body line one");
        let label0 = format!("trunk()..{}", entry0.change_id);
        let summary0 = parse_revset_summary(
            &label0,
            &format!(
                "{}|{}|{}|{}\x1F{}\x1E",
                entry0.commit_id,
                entry0.change_id,
                entry0.bookmarks.join(","),
                entry0.description,
                entry0.description_body
            ),
            "1 file changed, 2 insertions(+)",
            std::path::Path::new("C:/repo"),
        );
        assert_eq!(summary0.label(), "trunk()..def456");
        assert_eq!(summary0.bookmarks(), &["feature/foo"]);
        assert_eq!(summary0.description(), "First change");
        assert_eq!(summary0.description_body(), "Body line one");
        assert_eq!(summary0.commit_count(), 1);

        let entry1 = &entries[1];
        let label1 = format!("trunk()..{}", entry1.change_id);
        let summary1 = parse_revset_summary(
            &label1,
            &format!(
                "{}|{}|{}|{}\x1E",
                entry1.commit_id,
                entry1.change_id,
                entry1.bookmarks.join(","),
                entry1.description
            ),
            "2 files changed, 5 insertions(+)",
            std::path::Path::new("C:/repo"),
        );
        assert_eq!(summary1.label(), "trunk()..uvw000");
        assert!(summary1.bookmarks().is_empty());
        assert_eq!(summary1.description(), "Second change no bookmark");
        assert_eq!(summary1.description_body(), "");
    }

    #[test]
    fn no_changes_placeholder_is_stable() {
        let placeholder = no_changes_placeholder();
        assert_eq!(placeholder.label(), "(no changes above trunk())");
        assert_eq!(placeholder.commit_count(), 0);
        assert!(!placeholder.warnings().is_empty());
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
