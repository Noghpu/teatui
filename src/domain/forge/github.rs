use super::{
    CreatePrInput, ForgeAuthStatus, ForgeCli, ForgeDriver, RepoOptions, StackExistingPrs,
    parse_existing_prs, parse_names,
};
use crate::domain::process;

pub(crate) struct Driver;

impl ForgeDriver for Driver {
    fn auth_status(cli: &ForgeCli) -> ForgeAuthStatus {
        let mut args = vec![
            "auth".to_string(),
            "status".to_string(),
            "--active".to_string(),
        ];
        if let Some(host) = cli.host() {
            args.push("--hostname".to_string());
            args.push(host.to_string());
        }
        match process::output(cli.binary(), &args) {
            Ok(out) if out.status.success() => ForgeAuthStatus::Configured {
                logins: vec![cli.host().unwrap_or("github.com").to_string()],
            },
            Ok(_) => ForgeAuthStatus::None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ForgeAuthStatus::Errored {
                message: format!("{} not found", cli.binary()),
            },
            Err(e) => ForgeAuthStatus::Errored {
                message: e.to_string(),
            },
        }
    }

    fn repo_options(cli: &ForgeCli, owner: &str, repo: &str) -> RepoOptions {
        let labels = gh_api_names(cli, &format!("repos/{owner}/{repo}/labels?per_page=100"));
        let assignees = gh_api_names(
            cli,
            &format!("repos/{owner}/{repo}/collaborators?affiliation=direct&per_page=100"),
        );
        let milestones = gh_api_names(
            cli,
            &format!("repos/{owner}/{repo}/milestones?state=open&per_page=100"),
        );
        RepoOptions {
            labels,
            assignees,
            milestones,
        }
    }

    fn existing_prs(cli: &ForgeCli, owner: Option<&str>, repo: Option<&str>) -> StackExistingPrs {
        let (Some(owner), Some(repo)) = (owner, repo) else {
            return Vec::new();
        };
        let endpoint = format!("repos/{owner}/{repo}/pulls?state=all&per_page=100");
        match process::capture(cli.binary(), &gh_api_args(cli, &endpoint)) {
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
            "--body".to_string(),
            input.description.to_string(),
        ];
        for label in input.labels {
            args.push("--label".to_string());
            args.push(label.clone());
        }
        for assignee in input.assignees {
            args.push("--assignee".to_string());
            args.push(assignee.clone());
        }
        if !input.milestone.is_empty() {
            args.push("--milestone".to_string());
            args.push(input.milestone.to_string());
        }
        args
    }
}

fn gh_api_names(cli: &ForgeCli, endpoint: &str) -> Vec<String> {
    let Ok(stdout) = process::capture(cli.binary(), &gh_api_args(cli, endpoint)) else {
        return Vec::new();
    };
    parse_names(&stdout)
}

fn gh_api_args(cli: &ForgeCli, endpoint: &str) -> Vec<String> {
    let mut args = vec!["api".to_string(), endpoint.to_string()];
    if let Some(host) = cli.host() {
        args.push("--hostname".to_string());
        args.push(host.to_string());
    }
    args.push("--paginate".to_string());
    args.push("--slurp".to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_create_args_use_gh_pr_flags() {
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
                "--body".to_string(),
                "Body".to_string(),
                "--label".to_string(),
                "ui".to_string(),
                "--label".to_string(),
                "rewrite".to_string(),
                "--assignee".to_string(),
                "alice".to_string(),
                "--milestone".to_string(),
                "v1".to_string(),
            ]
        );
    }

    #[test]
    fn github_api_args_include_hostname_and_pagination() {
        let cli = ForgeCli::new(
            super::super::ForgeKind::Github,
            "gh".into(),
            Some("github.example.com".into()),
        );
        assert_eq!(
            gh_api_args(&cli, "repos/o/r/labels?per_page=100"),
            vec![
                "api".to_string(),
                "repos/o/r/labels?per_page=100".to_string(),
                "--hostname".to_string(),
                "github.example.com".to_string(),
                "--paginate".to_string(),
                "--slurp".to_string(),
            ]
        );
    }
}
