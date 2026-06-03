use std::process::{Command, Stdio};

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
    // Step 1: create or move the bookmark to the change.
    let bookmark_args = vec![
        "bookmark".to_string(),
        "set".to_string(),
        "--allow-backwards".to_string(),
        job.bookmark.clone(),
        "-r".to_string(),
        job.change_id.clone(),
    ];
    if let Err(message) = jj(&job.jj_binary, &bookmark_args) {
        return ExecuteResult::Errored {
            step: ExecuteStep::Bookmark,
            message,
        };
    }

    // Step 2: push the bookmark to the remote. A brand-new bookmark needs no
    // special flag: `jj git push --bookmark <name>` auto-creates and tracks an
    // untracked bookmark's remote counterpart.
    let push_args = vec![
        "git".to_string(),
        "push".to_string(),
        "--bookmark".to_string(),
        job.bookmark.clone(),
    ];
    if let Err(message) = jj(&job.jj_binary, &push_args) {
        return ExecuteResult::Errored {
            step: ExecuteStep::Push,
            message,
        };
    }

    // Step 3: create the PR via tea.
    let mut tea_args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--base".to_string(),
        job.base.clone(),
        "--head".to_string(),
        job.bookmark.clone(),
        "--title".to_string(),
        job.title.clone(),
        "--description".to_string(),
        job.description.clone(),
    ];
    if !job.labels.is_empty() {
        tea_args.push("--labels".to_string());
        tea_args.push(job.labels.join(","));
    }
    if !job.assignees.is_empty() {
        tea_args.push("--assignees".to_string());
        tea_args.push(job.assignees.join(","));
    }
    if !job.milestone.is_empty() {
        tea_args.push("--milestone".to_string());
        tea_args.push(job.milestone.clone());
    }
    let stdout = match tea(&job.tea_binary, &tea_args) {
        Ok(out) => out,
        Err(message) => {
            return ExecuteResult::Errored {
                step: ExecuteStep::Create,
                message,
            };
        }
    };

    let url = extract_url(&stdout).unwrap_or_else(|| stdout.trim().to_string());
    ExecuteResult::Ready { url }
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
}
