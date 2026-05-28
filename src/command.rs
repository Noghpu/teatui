use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::process::Command;
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::event::{BackgroundEvent, ExecutionOutcome, JobResult, JobStatus};
use crate::generate::ExecutionPlan;
use crate::tea::parse_pr_url;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static JOB_WAITERS: OnceLock<Mutex<HashMap<u64, oneshot::Receiver<JobResult>>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
}

impl ExternalCommand {
    pub fn new<I, S>(program: impl Into<String>, args: I, cwd: impl Into<PathBuf>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: cwd.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn display(&self) -> String {
        self.render(false)
    }

    #[allow(dead_code)] // used by the upcoming PR execution preview (design.md)
    pub fn redacted_display(&self) -> String {
        self.render(true)
    }

    fn render(&self, redact: bool) -> String {
        let mut parts = Vec::with_capacity(self.args.len() + 1);
        parts.push(self.program.clone());
        parts.extend(self.args.iter().enumerate().map(|(index, arg)| {
            if redact {
                redact_arg(index, arg, &self.args)
            } else {
                quote_arg(arg)
            }
        }));
        format!("{} (cwd: {})", parts.join(" "), self.cwd.display())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCapture {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(test)]
impl CommandCapture {
    pub fn new(command: impl Into<String>, stdout: String, stderr: String) -> Self {
        Self {
            command: command.into(),
            stdout,
            stderr,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub command: String,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

impl CommandError {
    pub fn display(&self) -> String {
        format!("{}: {}", self.command, self.message)
    }
}

pub async fn capture(command: ExternalCommand) -> Result<CommandCapture, CommandError> {
    let display = command.display();
    let mut child = Command::new(&command.program);
    child
        .kill_on_drop(true)
        .args(&command.args)
        .current_dir(&command.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = match timeout(command.timeout, child.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            return Err(CommandError {
                command: display.clone(),
                message: format!("{display}: {err}"),
                stdout: String::new(),
                stderr: String::new(),
            });
        }
        Err(_) => {
            return Err(CommandError {
                command: display.clone(),
                message: format!("{display} timed out after {:?}", command.timeout),
                stdout: String::new(),
                stderr: String::new(),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if output.status.success() {
        Ok(CommandCapture {
            command: display,
            stdout,
            stderr,
        })
    } else {
        let message = if stderr.trim().is_empty() {
            format!("{display} exited with {}", output.status)
        } else {
            stderr.trim().to_string()
        };
        Err(CommandError {
            command: display,
            message,
            stdout,
            stderr,
        })
    }
}

pub async fn spawn_job(
    command: ExternalCommand,
    name: String,
    tx: tokio::sync::mpsc::UnboundedSender<BackgroundEvent>,
) -> u64 {
    let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    let display = command.redacted_display();
    let (done_tx, done_rx) = oneshot::channel();
    job_waiters()
        .lock()
        .expect("job waiters poisoned")
        .insert(id, done_rx);
    let queued = JobResult {
        id,
        name: name.clone(),
        command: display.clone(),
        status: JobStatus::Queued,
        duration: None,
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    };
    let _ = tx.send(BackgroundEvent::Job(queued));

    tokio::spawn(async move {
        let started_at = Instant::now();
        let running = JobResult {
            id,
            name: name.clone(),
            command: display.clone(),
            status: JobStatus::Running,
            duration: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        };
        let _ = tx.send(BackgroundEvent::Job(running));

        let result = match capture(command).await {
            Ok(output) => JobResult {
                id,
                name,
                command: display,
                status: JobStatus::Succeeded,
                duration: Some(started_at.elapsed()),
                stdout: output.stdout,
                stderr: output.stderr,
                timed_out: false,
            },
            Err(error) => {
                let timed_out = error.message.to_ascii_lowercase().contains("timed out");
                JobResult {
                    id,
                    name,
                    command: display,
                    status: if timed_out {
                        JobStatus::TimedOut
                    } else {
                        JobStatus::Failed
                    },
                    duration: Some(started_at.elapsed()),
                    stdout: error.stdout,
                    stderr: error.stderr,
                    timed_out,
                }
            }
        };

        let _ = tx.send(BackgroundEvent::Job(result.clone()));
        let _ = done_tx.send(result);
    });

    id
}

pub async fn run_plan_sequentially(
    plan: ExecutionPlan,
    tx: tokio::sync::mpsc::UnboundedSender<BackgroundEvent>,
) -> ExecutionOutcome {
    let total = plan.steps.len();
    let mut pr_url = None;

    for (index, step) in plan.steps.into_iter().enumerate() {
        let _ = tx.send(BackgroundEvent::ExecutionStep { index, total });
        let job_id = spawn_job(step.command.clone(), step.label.clone(), tx.clone()).await;
        let Some(job) = await_job(job_id).await else {
            return ExecutionOutcome {
                pr_url: None,
                failed_step: Some(index),
                message: Some(format!("{} failed: job result channel closed", step.label)),
            };
        };

        if job.status != JobStatus::Succeeded {
            let message = if job.stderr.trim().is_empty() {
                format!("{} failed", step.label)
            } else {
                format!("{} failed: {}", step.label, job.stderr.trim())
            };

            return ExecutionOutcome {
                pr_url: None,
                failed_step: Some(index),
                message: Some(message),
            };
        }

        if is_pr_create_command(&step.command) {
            pr_url = parse_pr_url(&job.stdout);
        }
    }

    let message = if pr_url.is_none() {
        Some("URL not parsed; see job stdout in logs".into())
    } else {
        None
    };

    ExecutionOutcome {
        pr_url,
        failed_step: None,
        message,
    }
}

fn job_waiters() -> &'static Mutex<HashMap<u64, oneshot::Receiver<JobResult>>> {
    JOB_WAITERS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn await_job(id: u64) -> Option<JobResult> {
    let receiver = job_waiters()
        .lock()
        .expect("job waiters poisoned")
        .remove(&id)?;
    receiver.await.ok()
}

fn is_pr_create_command(command: &ExternalCommand) -> bool {
    matches!(
        command.args.as_slice(),
        [first, second, ..] if first == "pr" && second == "create"
    )
}

fn redact_arg(index: usize, arg: &str, args: &[String]) -> String {
    let sensitive_flag = matches!(
        args.get(index.saturating_sub(1))
            .map(|previous| previous.to_ascii_lowercase())
            .as_deref(),
        Some("--token" | "--password" | "--secret" | "--auth" | "--header")
    );

    if sensitive_flag {
        return "<redacted>".into();
    }

    if let Some((prefix, _)) = arg.split_once('=')
        && is_sensitive_key(prefix)
    {
        return format!("{prefix}=<redacted>");
    }

    let lower = arg.to_ascii_lowercase();
    if lower.contains("authorization:") || lower.starts_with("bearer ") {
        return "<redacted>".into();
    }

    quote_arg(arg)
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .trim_start_matches('-')
        .replace(['-', '_'], "")
        .to_ascii_lowercase();

    matches!(
        normalized.as_str(),
        "token" | "password" | "secret" | "auth" | "apikey" | "accesstoken"
    )
}

fn quote_arg(arg: &str) -> String {
    if arg.is_empty() || arg.chars().any(char::is_whitespace) {
        format!("{arg:?}")
    } else {
        arg.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_arguments_in_display() {
        let command = ExternalCommand::new(
            "tea",
            [
                "auth",
                "--token",
                "super-secret",
                "--password=also-secret",
                "--api-key=api-secret",
                "authorization: Bearer abc123",
            ],
            ".",
        );

        let display = command.redacted_display();
        assert!(display.contains("tea"));
        assert!(!display.contains("super-secret"));
        assert!(!display.contains("also-secret"));
        assert!(!display.contains("api-secret"));
        assert!(!display.contains("abc123"));
    }

    #[test]
    fn recognizes_configured_tea_pr_create_commands_by_argv() {
        let command = ExternalCommand::new(
            "C:/tools/tea.exe",
            ["pr", "create", "--title", "Title"],
            "C:/repo",
        );
        let version = ExternalCommand::new("C:/tools/tea.exe", ["--version"], "C:/repo");

        assert!(is_pr_create_command(&command));
        assert!(!is_pr_create_command(&version));
    }
}
