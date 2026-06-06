use super::process;
use super::stack::{PrStatus, StackPlanItem};
use super::{CreatePrInput, ForgeCli};
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
    pub forge: ForgeCli,
    pub change_id: String,
    pub bookmark: String,
    pub base: String,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
    pub remote: String,
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
        forge: &job.forge,
        change_id: &job.change_id,
        bookmark: &job.bookmark,
        base: &job.base,
        title: &job.title,
        description: &job.description,
        labels: &job.labels,
        assignees: &job.assignees,
        milestone: &job.milestone,
        remote: &job.remote,
    };
    match run_pr_steps(&args) {
        Ok(url) => ExecuteResult::Ready { url },
        Err((step, message)) => ExecuteResult::Errored { step, message },
    }
}

/// Borrowed inputs for the shared bookmark → push → create-PR sequence.
struct PrPushArgs<'a> {
    jj_binary: &'a str,
    forge: &'a ForgeCli,
    change_id: &'a str,
    bookmark: &'a str,
    base: &'a str,
    title: &'a str,
    description: &'a str,
    labels: &'a [String],
    assignees: &'a [String],
    milestone: &'a str,
    remote: &'a str,
}

/// Run the three-step bookmark → push → create-PR sequence shared by the
/// single-PR (`ExecutePrJob`) and stacked (`StackPushJob`) paths. On success
/// returns the created PR's URL; on failure returns the failing step and its
/// message. Keeping both paths on this one function is what guarantees their
/// command behavior never drifts apart.
fn run_pr_steps(args: &PrPushArgs) -> Result<String, (ExecuteStep, String)> {
    // Step 1: create or move the bookmark to the change.
    process::jj(
        args.jj_binary,
        &bookmark_args(args.change_id, args.bookmark),
    )
    .map_err(|message| (ExecuteStep::Bookmark, message))?;

    // Step 2: push the bookmark to the remote. Explicit --remote avoids jj
    // defaulting to "origin" when the repo uses a different remote name or
    // when gitoxide can't read the remote list via git safe.directory.
    process::jj(args.jj_binary, &push_args(args.bookmark, args.remote))
        .map_err(|message| (ExecuteStep::Push, message))?;

    // Step 3: create the PR via the selected forge CLI.
    let create = CreatePrInput {
        base: args.base,
        head: args.bookmark,
        title: args.title,
        description: args.description,
        labels: args.labels,
        assignees: args.assignees,
        milestone: args.milestone,
    };
    let stdout = args
        .forge
        .create_pr(&create)
        .map_err(|message| (ExecuteStep::Create, message))?;
    Ok(extract_url(&stdout).unwrap_or_else(|| stdout.trim().to_string()))
}

#[derive(Debug, Clone)]
pub struct StackPushJob {
    pub jj_binary: String,
    pub forge: ForgeCli,
    pub item: StackPlanItem,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: String,
    pub remote: String,
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

fn push_args(bookmark: &str, remote: &str) -> Vec<String> {
    vec![
        "git".to_string(),
        "push".to_string(),
        "--remote".to_string(),
        remote.to_string(),
        "--bookmark".to_string(),
        bookmark.to_string(),
    ]
}

fn run_stack_push(job: StackPushJob) -> StackPushResult {
    let index = job.item.input.index;
    let args = PrPushArgs {
        jj_binary: &job.jj_binary,
        forge: &job.forge,
        change_id: &job.item.input.head,
        bookmark: &job.item.bookmark,
        base: &job.item.input.base,
        title: &job.item.title,
        description: &job.item.description,
        labels: &job.labels,
        assignees: &job.assignees,
        milestone: &job.milestone,
        remote: &job.remote,
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
            push_args("pr/feat/add-foo", "origin"),
            vec![
                "git".to_string(),
                "push".to_string(),
                "--remote".to_string(),
                "origin".to_string(),
                "--bookmark".to_string(),
                "pr/feat/add-foo".to_string(),
            ]
        );
    }
}
