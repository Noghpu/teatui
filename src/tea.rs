use std::path::PathBuf;

use crate::command::ExternalCommand;
use crate::config::Config;

#[derive(Debug, Clone)]
pub struct TeaClient {
    program: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PrCreateArgs<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub base: &'a str,
    pub head: &'a str,
    pub labels: &'a str,
    pub assignees: &'a str,
    pub milestone: &'a str,
}

impl TeaClient {
    pub fn new(config: &Config) -> Self {
        Self {
            program: config.commands.tea.clone(),
        }
    }

    pub fn version_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(self.program.clone(), ["--version"], cwd)
    }

    pub fn login_list_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(self.program.clone(), ["login", "list"], cwd)
    }

    pub fn labels_list_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            ["labels", "list", "--output", "json", "--limit", "100"],
            cwd,
        )
    }

    pub fn milestones_list_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            [
                "milestones",
                "list",
                "--state",
                "open",
                "--output",
                "json",
                "--limit",
                "100",
            ],
            cwd,
        )
    }

    pub fn collaborators_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(
            self.program.clone(),
            ["api", "/repos/{owner}/{repo}/collaborators"],
            cwd,
        )
    }

    pub fn pr_create_command(
        &self,
        cwd: impl Into<PathBuf>,
        request: PrCreateArgs<'_>,
    ) -> ExternalCommand {
        let mut args = vec![
            "pr".to_string(),
            "create".to_string(),
            "--title".to_string(),
            request.title.trim().to_string(),
            "--description".to_string(),
            request.body.trim().to_string(),
            "--base".to_string(),
            request.base.trim().to_string(),
            "--head".to_string(),
            request.head.trim().to_string(),
        ];

        for label in split_multi_values(request.labels) {
            args.push("--label".into());
            args.push(label);
        }
        for assignee in split_multi_values(request.assignees) {
            args.push("--assignee".into());
            args.push(assignee);
        }
        if let Some(milestone) = optional_single_value(request.milestone) {
            args.push("--milestone".into());
            args.push(milestone);
        }

        ExternalCommand::new(self.program.clone(), args, cwd)
    }
}

pub fn parse_pr_url(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        if let Some(url) = parse_line_for_url(line) {
            return Some(url);
        }
    }

    None
}

fn parse_line_for_url(line: &str) -> Option<String> {
    let index = match (line.find("http://"), line.find("https://")) {
        (Some(http), Some(https)) => Some(http.min(https)),
        (Some(index), None) | (None, Some(index)) => Some(index),
        (None, None) => None,
    }?;
    let remainder = &line[index..];
    let end = remainder
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace())
        .map(|(index, _)| index)
        .unwrap_or(remainder.len());
    let url = remainder[..end].trim_end_matches(&[')', ']', ',', ';', '.'][..]);
    (!url.is_empty()).then(|| url.to_string())
}

fn split_multi_values(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn optional_single_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_version_command_argv() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.version_command("C:/repo");

        assert_eq!(command.program, "tea");
        assert_eq!(command.args, vec!["--version"]);
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }

    #[test]
    fn builds_login_list_command_argv() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.login_list_command("C:/repo");

        assert_eq!(command.program, "tea");
        assert_eq!(command.args, vec!["login", "list"]);
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }

    #[test]
    fn builds_pr_create_command_argv_with_optional_fields() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.pr_create_command(
            "C:/repo",
            PrCreateArgs {
                title: "Title",
                body: "Body",
                base: "main",
                head: "feature/example",
                labels: "bug, docs",
                assignees: "alice, bob",
                milestone: "v1",
            },
        );

        assert_eq!(
            command.args,
            vec![
                "pr",
                "create",
                "--title",
                "Title",
                "--description",
                "Body",
                "--base",
                "main",
                "--head",
                "feature/example",
                "--label",
                "bug",
                "--label",
                "docs",
                "--assignee",
                "alice",
                "--assignee",
                "bob",
                "--milestone",
                "v1",
            ]
        );
    }

    #[test]
    fn builds_labels_list_command_argv() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.labels_list_command("C:/repo");

        assert_eq!(command.program, "tea");
        assert_eq!(
            command.args,
            vec!["labels", "list", "--output", "json", "--limit", "100"]
        );
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }

    #[test]
    fn builds_milestones_list_command_argv() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.milestones_list_command("C:/repo");

        assert_eq!(command.program, "tea");
        assert_eq!(
            command.args,
            vec![
                "milestones",
                "list",
                "--state",
                "open",
                "--output",
                "json",
                "--limit",
                "100",
            ]
        );
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }

    #[test]
    fn builds_collaborators_command_argv() {
        let config = Config::default();
        let client = TeaClient::new(&config);
        let command = client.collaborators_command("C:/repo");

        assert_eq!(command.program, "tea");
        assert_eq!(
            command.args,
            vec!["api", "/repos/{owner}/{repo}/collaborators"]
        );
        assert_eq!(command.cwd, PathBuf::from("C:/repo"));
    }

    #[test]
    fn parse_pr_url_extracts_first_url_and_ignores_plain_text() {
        let output =
            "creating PR\nview it at https://code.example.com/team/project/pulls/42\nthanks";
        assert_eq!(
            parse_pr_url(output),
            Some("https://code.example.com/team/project/pulls/42".into())
        );
        assert_eq!(parse_pr_url("nothing useful here"), None);
    }
}
