use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::timeout;

use crate::config::{CommandConfig, Config};
use crate::event::{JobResult, JobStatus};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Jj,
    #[allow(dead_code)]
    Git,
    #[allow(dead_code)]
    Tea,
}

impl CommandKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jj => "jj",
            Self::Git => "git",
            Self::Tea => "tea",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCommand {
    pub kind: CommandKind,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
}

impl ExternalCommand {
    pub fn new<I, S>(
        kind: CommandKind,
        program: impl Into<String>,
        args: I,
        cwd: impl Into<PathBuf>,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            kind,
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: cwd.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[allow(dead_code)]
    pub fn display(&self) -> String {
        self.render(false)
    }

    pub fn redacted_display(&self) -> String {
        self.render(true)
    }

    fn render(&self, redact: bool) -> String {
        let mut parts = Vec::with_capacity(self.args.len() + 2);
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

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub id: u64,
    pub kind: CommandKind,
    pub display: String,
    pub status: JobStatus,
    pub duration: Duration,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl CommandResult {
    pub fn into_job_result(self) -> JobResult {
        JobResult {
            id: self.id,
            name: self.kind.as_str().to_string(),
            command: self.display,
            status: self.status,
            duration: Some(self.duration),
            stdout: self.stdout,
            stderr: self.stderr,
            timed_out: self.timed_out,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandRunner {
    command_config: CommandConfig,
    job_tx: UnboundedSender<JobResult>,
    next_job_id: std::sync::Arc<AtomicU64>,
    default_timeout: Duration,
}

impl CommandRunner {
    pub fn new(config: &Config, job_tx: UnboundedSender<JobResult>) -> Self {
        Self {
            command_config: config.commands.clone(),
            job_tx,
            next_job_id: std::sync::Arc::new(AtomicU64::new(1)),
            default_timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn spawn(&self, command: ExternalCommand) -> u64 {
        let id = self.next_job_id.fetch_add(1, Ordering::Relaxed);
        let queued = JobResult {
            id,
            name: command.kind.as_str().to_string(),
            command: command.redacted_display(),
            status: JobStatus::Queued,
            duration: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        };
        let _ = self.job_tx.send(queued);

        let job_tx = self.job_tx.clone();
        tokio::spawn(async move {
            let running = JobResult {
                id,
                name: command.kind.as_str().to_string(),
                command: command.redacted_display(),
                status: JobStatus::Running,
                duration: None,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false,
            };
            let _ = job_tx.send(running);

            let result = run_command(id, command).await.into_job_result();
            let _ = job_tx.send(result);
        });

        id
    }

    pub fn jj_status_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(
            CommandKind::Jj,
            self.command_config.jj.clone(),
            ["--no-pager", "status"],
            cwd,
        )
        .with_timeout(self.default_timeout)
    }

    #[allow(dead_code)]
    pub fn git_status_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(
            CommandKind::Git,
            self.command_config.git.clone(),
            ["status", "--short"],
            cwd,
        )
        .with_timeout(self.default_timeout)
    }

    #[allow(dead_code)]
    pub fn tea_status_command(&self, cwd: impl Into<PathBuf>) -> ExternalCommand {
        ExternalCommand::new(
            CommandKind::Tea,
            self.command_config.tea.clone(),
            ["whoami"],
            cwd,
        )
        .with_timeout(self.default_timeout)
    }
}

async fn run_command(id: u64, command: ExternalCommand) -> CommandResult {
    let started = Instant::now();
    let mut child = Command::new(&command.program);
    child
        .kill_on_drop(true)
        .args(&command.args)
        .current_dir(&command.cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = timeout(command.timeout, child.output()).await;

    match output {
        Ok(Ok(output)) => CommandResult {
            id,
            kind: command.kind,
            display: command.redacted_display(),
            status: if output.status.success() {
                JobStatus::Succeeded
            } else {
                JobStatus::Failed
            },
            duration: started.elapsed(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            timed_out: false,
        },
        Ok(Err(err)) => CommandResult {
            id,
            kind: command.kind,
            display: command.redacted_display(),
            status: JobStatus::Failed,
            duration: started.elapsed(),
            stdout: String::new(),
            stderr: err.to_string(),
            timed_out: false,
        },
        Err(_) => CommandResult {
            id,
            kind: command.kind,
            display: command.redacted_display(),
            status: JobStatus::Failed,
            duration: started.elapsed(),
            stdout: String::new(),
            stderr: format!("command timed out after {:?}", command.timeout),
            timed_out: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_arguments_in_display() {
        let command = ExternalCommand::new(
            CommandKind::Tea,
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
    fn builds_jj_status_command() {
        let config = Config::default();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let runner = CommandRunner::new(&config, tx);
        let command = runner.jj_status_command("C:/tmp");

        assert_eq!(command.program, "jj");
        assert_eq!(command.args, vec!["--no-pager", "status"]);
        assert_eq!(command.kind, CommandKind::Jj);
        assert_eq!(command.cwd, PathBuf::from("C:/tmp"));
    }
}
