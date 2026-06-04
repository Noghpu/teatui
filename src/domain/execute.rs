use std::process::{Command, Stdio};

use super::stack::{PrStatus, StackPlanItem};
use crate::runtime::{Job, JobOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecuteStep {
    Bookmark,
    Push,
    Create,
}

impl ExecuteStep {
    pub fn label(self) -> &'static str {
        match self {
            ExecuteStep::Bookmark => "set bookmark",
            ExecuteStep::Push => "push",
            ExecuteStep::Create => "create PR",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteResult {
    Ready { url: String },
    Errored { step: ExecuteStep, message: String },
}

pub struct ExecutePrJob {
    pub jj_binary: String,
    pub tea_binary: String,
    pub change_id: String,
    pub bookmark: String,
    pub base: String,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
}

impl Job for ExecutePrJob {
    fn name(&self) -> &'static str {
        "domain.execute"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = run_execute(*self);
        JobOutcome::Done(Box::new(result))
    }
}

fn run_execute(job: ExecutePrJob) -> ExecuteResult {
    let args = PrPushArgs {
        jj_binary: &job.jj_binary,
        tea_binary: &job.tea_binary,
        change_id: &job.change_id,
        bookmark: &job.bookmark,
        base: &job.base,
        title: &job.title,
        description: &job.description,
        labels: &job.labels,
        assignees: &job.assignees,
        milestone: &job.milestone,
    };
    match run_pr_steps(&args) {
        Ok(url) => ExecuteResult::Ready { url },
        Err((step, message)) => ExecuteResult::Errored { step, message },
    }
}

/// Borrowed inputs for the shared bookmark → push → create-PR sequence.
struct PrPushArgs<'a> {
    jj_binary: &'a str,
    tea_binary: &'a str,
    change_id: &'a str,
    bookmark: &'a str,
    base: &'a str,
    title: &'a str,
    description: &'a str,
    labels: &'a [String],
    assignees: &'a [String],
    milestone: &'a str,
}

/// Run the three-step bookmark → push → create-PR sequence shared by the
/// single-PR (`ExecutePrJob`) and stacked (`StackPushJob`) paths. On success
/// returns the created PR's URL; on failure returns the failing step and its
/// message. Keeping both paths on this one function is what guarantees their
/// command behavior never drifts apart.
fn run_pr_steps(args: &PrPushArgs) -> Result<String, (ExecuteStep, String)> {
    // Step 1: create or move the bookmark to the change.
    jj(
        args.jj_binary,
        &bookmark_args(args.change_id, args.bookmark),
    )
    .map_err(|message| (ExecuteStep::Bookmark, message))?;

    // Step 2: push the bookmark to the remote. A brand-new bookmark needs no
    // special flag: `jj git push --bookmark <name>` auto-creates and tracks an
    // untracked bookmark's remote counterpart.
    jj(args.jj_binary, &push_args(args.bookmark))
        .map_err(|message| (ExecuteStep::Push, message))?;

    // Step 3: create the PR via tea.
    let tea_args = tea_create_args(
        args.base,
        args.bookmark,
        args.title,
        args.description,
        args.labels,
        args.assignees,
        args.milestone,
    );
    let stdout =
        tea(args.tea_binary, &tea_args).map_err(|message| (ExecuteStep::Create, message))?;
    Ok(extract_url(&stdout).unwrap_or_else(|| stdout.trim().to_string()))
}

#[derive(Debug, Clone)]
pub struct StackPushJob {
    pub jj_binary: String,
    pub tea_binary: String,
    pub item: StackPlanItem,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPushResult {
    pub index: usize,
    pub status: PrStatus,
}

impl Job for StackPushJob {
    fn name(&self) -> &'static str {
        "domain.execute.stack_push"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        let result = run_stack_push(*self);
        JobOutcome::Done(Box::new(result))
    }
}

fn jj(binary: &str, args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new(binary);
    cmd.arg("--no-pager");
    cmd.args(args);
    run_capture(cmd, binary, args)
}

fn tea(binary: &str, args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new(binary);
    cmd.args(args);
    run_capture(cmd, binary, args)
}

fn bookmark_args(change_id: &str, bookmark: &str) -> Vec<String> {
    vec![
        "bookmark".to_string(),
        "set".to_string(),
        "--allow-backwards".to_string(),
        bookmark.to_string(),
        "-r".to_string(),
        change_id.to_string(),
    ]
}

fn push_args(bookmark: &str) -> Vec<String> {
    vec![
        "git".to_string(),
        "push".to_string(),
        "--bookmark".to_string(),
        bookmark.to_string(),
    ]
}

fn tea_create_args(
    base: &str,
    head: &str,
    title: &str,
    description: &str,
    labels: &[String],
    assignees: &[String],
    milestone: &str,
) -> Vec<String> {
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--base".to_string(),
        base.to_string(),
        "--head".to_string(),
        head.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--description".to_string(),
        description.to_string(),
    ];
    if !labels.is_empty() {
        args.push("--labels".to_string());
        args.push(labels.join(","));
    }
    if !assignees.is_empty() {
        args.push("--assignees".to_string());
        args.push(assignees.join(","));
    }
    if !milestone.is_empty() {
        args.push("--milestone".to_string());
        args.push(milestone.to_string());
    }
    args
}

fn run_capture(mut cmd: Command, binary: &str, args: &[String]) -> Result<String, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd
        .output()
        .map_err(|e| format!("{binary} {args:?}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let body = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("{binary} {args:?}: {body}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run_stack_push(job: StackPushJob) -> StackPushResult {
    let index = job.item.input.index;
    let args = PrPushArgs {
        jj_binary: &job.jj_binary,
        tea_binary: &job.tea_binary,
        change_id: &job.item.input.head,
        bookmark: &job.item.bookmark,
        base: &job.item.input.base,
        title: &job.item.title,
        description: &job.item.description,
        labels: &job.labels,
        assignees: &job.assignees,
        milestone: &job.milestone,
    };
    let status = match run_pr_steps(&args) {
        Ok(url) => PrStatus::Created { url },
        Err((step, message)) => PrStatus::Failed { step, message },
    };
    StackPushResult { index, status }
}

fn extract_url(text: &str) -> Option<String> {
    for tok in text.split_whitespace() {
        if tok.starts_with("http://") || tok.starts_with("https://") {
            return Some(tok.trim_end_matches(['.', ',', ';', ')']).to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_url_from_message() {
        let raw = "PR created: https://gitea.example.com/owner/repo/pulls/42\n";
        assert_eq!(
            extract_url(raw).as_deref(),
            Some("https://gitea.example.com/owner/repo/pulls/42")
        );
    }

    #[test]
    fn extracts_url_trims_trailing_punctuation() {
        let raw = "see https://example.com/p/1).";
        assert_eq!(extract_url(raw).as_deref(), Some("https://example.com/p/1"));
    }

    #[test]
    fn returns_none_when_no_url() {
        assert!(extract_url("nothing here").is_none());
    }

    #[test]
    fn bookmark_args_match_execute_path() {
        assert_eq!(
            bookmark_args("abcd", "pr/feat/add-foo"),
            vec![
                "bookmark".to_string(),
                "set".to_string(),
                "--allow-backwards".to_string(),
                "pr/feat/add-foo".to_string(),
                "-r".to_string(),
                "abcd".to_string(),
            ]
        );
    }

    #[test]
    fn push_args_match_execute_path() {
        assert_eq!(
            push_args("pr/feat/add-foo"),
            vec![
                "git".to_string(),
                "push".to_string(),
                "--bookmark".to_string(),
                "pr/feat/add-foo".to_string(),
            ]
        );
    }

    #[test]
    fn tea_create_args_include_shared_metadata() {
        let args = tea_create_args(
            "main",
            "pr/feat/add-foo",
            "Add foo",
            "Body",
            &["ui".into(), "rewrite".into()],
            &["dev".into()],
            "v1",
        );
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
                "dev".to_string(),
                "--milestone".to_string(),
                "v1".to_string(),
            ]
        );
    }
}
