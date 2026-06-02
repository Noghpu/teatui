use std::process::{Command, Stdio};

use crate::runtime::{Job, JobOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBundle {
    pub base: String,
    pub head: String,
    pub status: String,
    pub changes: Vec<ChangeContext>,
    pub aggregate: DiffContext,
}

/// Per-change metadata carrying the *journey*: what each step did and why.
/// Intentionally diff-free — the full code is sent once via the aggregate
/// diff, so repeating per-change hunks would just duplicate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeContext {
    pub subject: String,
    pub body: String,
    pub diff_stat: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffContext {
    pub diff_stat: String,
    pub diff: String,
    pub diff_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextResult {
    Ready(ContextBundle),
    Errored { message: String },
}

pub struct ContextJob {
    pub jj_binary: String,
    pub base: String,
    pub head: String,
    pub diff_byte_budget: usize,
}

impl Job for ContextJob {
    fn name(&self) -> &'static str {
        "domain.context"
    }
    fn run(self: Box<Self>) -> JobOutcome {
        let result = match collect(
            &self.jj_binary,
            &self.base,
            &self.head,
            self.diff_byte_budget,
        ) {
            Ok(bundle) => ContextResult::Ready(bundle),
            Err(message) => ContextResult::Errored { message },
        };
        JobOutcome::Done(Box::new(result))
    }
}

fn collect(jj: &str, base: &str, head: &str, budget: usize) -> Result<ContextBundle, String> {
    let revset = range_revset(base, head)?;
    // `jj status` describes the live working copy. It's relevant only when head is
    // the working copy (`@`); for any other head it leaks files outside base..head
    // and just adds noise the model may mistake for part of the change.
    let status = if head.trim() == "@" {
        run_jj(jj, &["status"])?
    } else {
        String::new()
    };
    let changes = collect_changes(jj, &revset)?;
    let aggregate = collect_diff_context(jj, &revset, budget)?;
    Ok(ContextBundle {
        base: base.to_string(),
        head: head.to_string(),
        status,
        changes,
        aggregate,
    })
}

fn collect_changes(jj: &str, revset: &str) -> Result<Vec<ChangeContext>, String> {
    const TEMPLATE: &str =
        r#""\x1E" ++ change_id ++ "\x1F" ++ description.lines().join("\x1F") ++ "\x1D\n""#;
    let raw = run_jj(jj, &["log", "-r", revset, "--no-graph", "-T", TEMPLATE])?;
    let entries = parse_change_log(&raw);
    let mut changes = Vec::with_capacity(entries.len());
    for entry in entries.into_iter().rev() {
        // Only the per-change stat — the full hunks live in the aggregate diff.
        let diff_stat = run_jj(jj, &["diff", "-r", &entry.change_id, "--stat"])?;
        changes.push(ChangeContext {
            subject: entry.subject,
            body: entry.body,
            diff_stat,
        });
    }
    Ok(changes)
}

fn collect_diff_context(jj: &str, revset: &str, budget: usize) -> Result<DiffContext, String> {
    let diff_stat = run_jj(jj, &["diff", "-r", revset, "--stat"])?;
    // `--git` emits standard unified-diff format: more compact than jj's default
    // line-numbered color-words output, and the format LLMs parse most reliably.
    let diff_raw = run_jj(jj, &["diff", "-r", revset, "--git"])?;
    let (diff, diff_truncated) = truncate_to_byte_budget(&diff_raw, budget);
    Ok(DiffContext {
        diff_stat,
        diff,
        diff_truncated,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedChangeLog {
    change_id: String,
    subject: String,
    body: String,
}

fn parse_change_log(raw: &str) -> Vec<ParsedChangeLog> {
    raw.split('\x1E')
        .skip(1)
        .filter_map(|record| {
            let (meta, _) = record.split_once('\x1D')?;
            let mut fields = meta.split('\x1F');
            let change_id = fields.next()?.trim().to_string();
            let subject = fields.next().unwrap_or("").trim().to_string();
            let body = fields.collect::<Vec<_>>().join("\n").trim().to_string();
            if change_id.is_empty() {
                return None;
            }
            Some(ParsedChangeLog {
                change_id,
                subject,
                body,
            })
        })
        .collect()
}

fn range_revset(base: &str, head: &str) -> Result<String, String> {
    let base = base.trim();
    let head = head.trim();
    if base.is_empty() {
        return Err("base is required".into());
    }
    if head.is_empty() {
        return Err("head is required".into());
    }
    Ok(format!("{base}..{head}"))
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

    #[test]
    fn range_revset_uses_base_and_head() {
        assert_eq!(range_revset("trunk()", "@").unwrap(), "trunk()..@");
    }

    #[test]
    fn range_revset_rejects_missing_parts() {
        assert!(range_revset("", "@").is_err());
        assert!(range_revset("trunk()", "").is_err());
    }

    #[test]
    fn parses_change_log_without_exposing_ids_in_context_shape() {
        let raw = "\x1Eabc\x1Ffeat: add thing\x1Fbody text\x1D\n\x1Edef\x1Ffix: repair\x1F\x1D\n";
        let changes = parse_change_log(raw);
        assert_eq!(
            changes,
            vec![
                ParsedChangeLog {
                    change_id: "abc".into(),
                    subject: "feat: add thing".into(),
                    body: "body text".into(),
                },
                ParsedChangeLog {
                    change_id: "def".into(),
                    subject: "fix: repair".into(),
                    body: String::new(),
                },
            ]
        );
    }
}
