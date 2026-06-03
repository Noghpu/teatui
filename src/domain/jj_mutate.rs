use std::process::{Command, Stdio};

use crate::runtime::{Job, JobOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JjOpKind {
    SquashWithBelow,
    MoveUp,
    MoveDown,
}

impl JjOpKind {
    pub fn label(self) -> &'static str {
        match self {
            JjOpKind::SquashWithBelow => "squash with below",
            JjOpKind::MoveUp => "move above",
            JjOpKind::MoveDown => "move below",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjOp {
    pub kind: JjOpKind,
    pub change_id: String,
    pub target_id: String,
}

impl JjOp {
    pub fn command_args(&self) -> Vec<String> {
        match self.kind {
            JjOpKind::SquashWithBelow => vec![
                "squash".into(),
                "--from".into(),
                self.change_id.clone(),
                "--into".into(),
                self.target_id.clone(),
                "--use-destination-message".into(),
            ],
            JjOpKind::MoveUp => vec![
                "rebase".into(),
                "-r".into(),
                self.change_id.clone(),
                "--insert-after".into(),
                self.target_id.clone(),
            ],
            JjOpKind::MoveDown => vec![
                "rebase".into(),
                "-r".into(),
                self.change_id.clone(),
                "--insert-before".into(),
                self.target_id.clone(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjMutateResult {
    Applied { op: JjOpKind },
    Reverted { op: JjOpKind, reason: String },
    Errored { op: JjOpKind, message: String },
}

pub struct JjMutateJob {
    pub jj_binary: String,
    pub op: JjOp,
    pub conflict_revset: String,
}

impl Job for JjMutateJob {
    fn name(&self) -> &'static str {
        "domain.jj_mutate"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        let result = run_mutate(*self);
        JobOutcome::Done(Box::new(result))
    }
}

fn run_mutate(job: JjMutateJob) -> JjMutateResult {
    if let Err(message) = ensure_no_conflicts(&job.jj_binary, &job.conflict_revset) {
        return JjMutateResult::Errored {
            op: job.op.kind,
            message,
        };
    }

    let args = job.op.command_args();
    if let Err(message) = jj(&job.jj_binary, &args) {
        return JjMutateResult::Errored {
            op: job.op.kind,
            message,
        };
    }

    if let Err(message) = ensure_no_conflicts(&job.jj_binary, &job.conflict_revset) {
        let reason = format!("{}; reverted the jj operation", message);
        return match jj(&job.jj_binary, &["undo".to_string()]) {
            Ok(_) => JjMutateResult::Reverted {
                op: job.op.kind,
                reason,
            },
            Err(undo_error) => JjMutateResult::Errored {
                op: job.op.kind,
                message: format!("{reason}, but jj undo failed: {undo_error}"),
            },
        };
    }

    JjMutateResult::Applied { op: job.op.kind }
}

fn ensure_no_conflicts(jj_binary: &str, revset: &str) -> Result<(), String> {
    let args = vec![
        "--ignore-working-copy".to_string(),
        "log".to_string(),
        "-r".to_string(),
        revset.to_string(),
        "--no-graph".to_string(),
        "-T".to_string(),
        "if(self.conflict(), \"C\", \"\")".to_string(),
    ];
    let out = jj(jj_binary, &args)?;
    if out.contains('C') {
        Err(format!("conflicts exist in {revset}"))
    } else {
        Ok(())
    }
}

fn jj(binary: &str, args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new(binary);
    cmd.arg("--no-pager");
    cmd.args(args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd
        .output()
        .map_err(|e| format!("{binary} {args:?}: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let body = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("{binary} {args:?}: {body}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squash_uses_destination_message_to_avoid_editor() {
        let op = JjOp {
            kind: JjOpKind::SquashWithBelow,
            change_id: "new".into(),
            target_id: "old".into(),
        };

        assert_eq!(
            op.command_args(),
            vec![
                "squash",
                "--from",
                "new",
                "--into",
                "old",
                "--use-destination-message",
            ]
        );
    }

    #[test]
    fn newest_first_move_up_inserts_after_visual_above_row() {
        let op = JjOp {
            kind: JjOpKind::MoveUp,
            change_id: "old".into(),
            target_id: "new".into(),
        };

        assert_eq!(
            op.command_args(),
            vec!["rebase", "-r", "old", "--insert-after", "new"]
        );
    }

    #[test]
    fn newest_first_move_down_inserts_before_visual_below_row() {
        let op = JjOp {
            kind: JjOpKind::MoveDown,
            change_id: "new".into(),
            target_id: "old".into(),
        };

        assert_eq!(
            op.command_args(),
            vec!["rebase", "-r", "new", "--insert-before", "old"]
        );
    }
}
