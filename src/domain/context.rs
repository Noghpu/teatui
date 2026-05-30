use std::process::{Command, Stdio};

use crate::runtime::{Job, JobOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBundle {
    pub revset: String,
    pub status: String,
    pub log: String,
    pub diff_stats: String,
    pub diff: String,
    /// True if `diff` was truncated to the byte budget.
    pub diff_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextResult {
    Ready(ContextBundle),
    Errored { message: String },
}

pub struct ContextJob {
    pub jj_binary: String,
    pub revset: String,
    pub diff_byte_budget: usize,
}

impl Job for ContextJob {
    fn name(&self) -> &'static str {
        "domain.context"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = match collect(&self.jj_binary, &self.revset, self.diff_byte_budget) {
            Ok(bundle) => ContextResult::Ready(bundle),
            Err(message) => ContextResult::Errored { message },
        };
        JobOutcome::Done(Box::new(result))
    }
}

fn collect(jj: &str, revset: &str, budget: usize) -> Result<ContextBundle, String> {
    let status = run_jj(jj, &["status"])?;
    let log = run_jj(jj, &["log", "-r", revset, "--no-graph"])?;
    let diff_stats = run_jj(jj, &["diff", "-r", revset, "--stat"])?;
    let diff_raw = run_jj(jj, &["diff", "-r", revset])?;
    let (diff, diff_truncated) = truncate_to_byte_budget(&diff_raw, budget);
    Ok(ContextBundle {
        revset: revset.to_string(),
        status,
        log,
        diff_stats,
        diff,
        diff_truncated,
    })
}

fn run_jj(jj: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(jj);
    cmd.arg("--no-pager");
    cmd.args(args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().map_err(|e| format!("{jj} {args:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{jj} {args:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Truncate `s` to roughly `budget` bytes, preserving UTF-8 boundaries.
/// Returns the (possibly truncated) string and whether truncation occurred.
fn truncate_to_byte_budget(s: &str, budget: usize) -> (String, bool) {
    if s.len() <= budget {
        return (s.to_string(), false);
    }
    // Walk char boundaries; stop before exceeding budget.
    let mut end = 0;
    for (idx, _) in s.char_indices() {
        if idx > budget {
            break;
        }
        end = idx;
    }
    let mut out = String::with_capacity(end + 32);
    out.push_str(&s[..end]);
    out.push_str("\n\n[... truncated ...]\n");
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_below_size_truncates() {
        let s = "x".repeat(500);
        let (out, truncated) = truncate_to_byte_budget(&s, 100);
        assert!(truncated);
        assert!(out.contains("[... truncated ...]"));
        assert!(out.len() <= 100 + 64);
    }

    #[test]
    fn budget_above_size_passes_through() {
        let s = "hello";
        let (out, truncated) = truncate_to_byte_budget(s, 1024);
        assert!(!truncated);
        assert_eq!(out, "hello");
    }

    #[test]
    fn truncation_respects_utf8_boundaries() {
        let s = "α".repeat(100); // each α is 2 bytes
        let (out, truncated) = truncate_to_byte_budget(&s, 50);
        assert!(truncated);
        // The truncated body should be a valid UTF-8 string.
        assert!(out.starts_with('α'));
    }
}
