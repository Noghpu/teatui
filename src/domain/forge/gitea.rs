use super::{
    CreatePrInput, ForgeAuthStatus, ForgeCli, ForgeDriver, RepoOptions, StackExistingPrs,
    parse_existing_prs, parse_names,
};
use crate::domain::process;

pub(crate) struct Driver;

impl ForgeDriver for Driver {
    fn auth_status(cli: &ForgeCli) -> ForgeAuthStatus {
        match process::output(cli.binary(), &["login", "list"]) {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let logins = parse_tea_logins(&stdout);
                if logins.is_empty() {
                    ForgeAuthStatus::None
                } else {
                    ForgeAuthStatus::Configured { logins }
                }
            }
            Ok(out) => ForgeAuthStatus::Errored {
                message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ForgeAuthStatus::Errored {
                message: format!("{} not found", cli.binary()),
            },
            Err(e) => ForgeAuthStatus::Errored {
                message: e.to_string(),
            },
        }
    }

    fn repo_options(cli: &ForgeCli, owner: &str, repo: &str) -> RepoOptions {
        let labels = tea_names(cli.binary(), &format!("repos/{owner}/{repo}/labels"));
        let assignees = tea_names(cli.binary(), &format!("repos/{owner}/{repo}/collaborators"));
        let milestones = tea_names(cli.binary(), &format!("repos/{owner}/{repo}/milestones"));
        RepoOptions {
            labels,
            assignees,
            milestones,
        }
    }

    fn existing_prs(cli: &ForgeCli, _owner: Option<&str>, _repo: Option<&str>) -> StackExistingPrs {
        match process::capture(cli.binary(), &["pr", "list", "--output", "json"]) {
            Ok(stdout) => parse_existing_prs(&stdout),
            Err(_) => Vec::new(),
        }
    }

    fn create_args(input: &CreatePrInput<'_>) -> Vec<String> {
        let mut args = vec![
            "pr".to_string(),
            "create".to_string(),
            "--base".to_string(),
            input.base.to_string(),
            "--head".to_string(),
            input.head.to_string(),
            "--title".to_string(),
            input.title.to_string(),
            "--description".to_string(),
            input.description.to_string(),
        ];
        if !input.labels.is_empty() {
            args.push("--labels".to_string());
            args.push(input.labels.join(","));
        }
        if !input.assignees.is_empty() {
            args.push("--assignees".to_string());
            args.push(input.assignees.join(","));
        }
        if !input.milestone.is_empty() {
            args.push("--milestone".to_string());
            args.push(input.milestone.to_string());
        }
        args
    }
}

fn parse_tea_logins(stdout: &str) -> Vec<String> {
    // Output format (whitespace-aligned table):
    //   Name      URL                                Default
    //   gitea     https://gitea.example.com          *
    // Skip the header row, take the first column.
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
    let _ = lines.next();
    lines
        .map(|line| line.split_whitespace().next().unwrap_or("").to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn tea_names(binary: &str, path: &str) -> Vec<String> {
    let Ok(stdout) = process::capture(binary, &["api", path]) else {
        return Vec::new();
    };
    parse_names(&stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tea_login_list_with_one_login() {
        let raw = "Name      URL                                Default\ngitea     https://gitea.example.com         *\n";
        let logins = parse_tea_logins(raw);
        assert_eq!(logins, vec!["gitea".to_string()]);
    }

    #[test]
    fn parses_tea_login_list_with_no_logins() {
        let raw = "Name      URL                                Default\n";
        let logins = parse_tea_logins(raw);
        assert!(logins.is_empty());
    }

    #[test]
    fn parses_tea_login_list_with_multiple() {
        let raw = "Name    URL                          Default\ngitea   https://gitea.example.com    *\nother   https://other.example.com\n";
        let logins = parse_tea_logins(raw);
        assert_eq!(logins, vec!["gitea".to_string(), "other".to_string()]);
    }

    #[test]
    fn gitea_create_args_include_shared_metadata() {
        let args = Driver::create_args(&CreatePrInput {
            base: "main",
            head: "pr/feat/add-foo",
            title: "Add foo",
            description: "Body",
            labels: &["ui".into(), "rewrite".into()],
            assignees: &["alice".into()],
            milestone: "v1",
        });
        assert_eq!(
            args,
            vec![
                "pr".to_string(),
                "create".to_string(),
                "--base".to_string(),
                "main".to_string(),
                "--head".to_string(),
                "pr/feat/add-foo".to_string(),
                "--title".to_string(),
                "Add foo".to_string(),
                "--description".to_string(),
                "Body".to_string(),
                "--labels".to_string(),
                "ui,rewrite".to_string(),
                "--assignees".to_string(),
                "alice".to_string(),
                "--milestone".to_string(),
                "v1".to_string(),
            ]
        );
    }
}
