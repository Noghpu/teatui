use std::path::PathBuf;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

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
}
