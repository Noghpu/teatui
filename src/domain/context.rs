use std::process::{Command, Stdio};

use crate::domain::stack::StackPrInput;
use crate::runtime::{Job, JobOutcome};

/// Minimum per-range diff budget. Ranges whose share of the total budget falls
/// below this floor are collected stat-only (`diff_omitted`) rather than
/// receiving a uselessly tiny diff slice.
pub const STACK_RANGE_DIFF_FLOOR: usize = 4 * 1024; // 4 KiB

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
    /// The full diff was intentionally not collected (the backend's
    /// `diff_budget_bytes` is 0). `diff` is empty and the prompt adapts to lean
    /// on commit messages and `diff_stat` instead. Distinct from an empty diff.
    pub diff_omitted: bool,
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

// ========================== Stack context collection ========================

/// Result type for [`StackContextJob`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackContextResult {
    Ready {
        bundles: Vec<ContextBundle>,
        inputs: Vec<StackPrInput>,
    },
    /// The job failed while collecting the range at `index`.
    Errored { index: usize, message: String },
}

/// A background job that collects one [`ContextBundle`] per PR range for a
/// stacked-PR generation, oldest-to-newest.
///
/// The total diff budget is divided evenly across ranges. Any range whose share
/// falls below [`STACK_RANGE_DIFF_FLOOR`] is collected stat-only
/// (`diff_omitted`) rather than a uselessly small slice.
///
/// The snapshot rule from single-PR collection is preserved: the working copy
/// is snapshotted at most once across the whole job. Stacked heads are concrete
/// change ids, so every per-range read goes through `--ignore-working-copy` and
/// needs no snapshot at all. In the degenerate case where a head is `@`,
/// `collect_range` takes that one snapshot via its `jj status` call, and only
/// one range can ever have head `@`, so the bound still holds.
pub struct StackContextJob {
    pub jj_binary: String,
    pub ranges: Vec<StackPrInput>,
    pub total_diff_byte_budget: usize,
}

impl Job for StackContextJob {
    fn name(&self) -> &'static str {
        "domain.stack_context"
    }

    fn run(self: Box<Self>) -> JobOutcome {
        let ranges = self.ranges;
        let n = ranges.len();
        if n == 0 {
            return JobOutcome::Done(Box::new(StackContextResult::Ready {
                bundles: Vec::new(),
                inputs: ranges,
            }));
        }

        // No pre-snapshot: stacked heads are concrete change ids, so each
        // `collect_range` reads with `--ignore-working-copy`. The only path that
        // snapshots is `collect_range`'s `jj status` for an `@` head, and at
        // most one range can have head `@`, so the working copy is snapshotted
        // at most once for the whole job.
        let budgets = divide_budget(self.total_diff_byte_budget, n);
        let mut bundles: Vec<ContextBundle> = Vec::with_capacity(n);

        for (i, pr) in ranges.iter().enumerate() {
            match collect_range(&self.jj_binary, &pr.base, &pr.head, budgets[i]) {
                Ok(bundle) => bundles.push(bundle),
                Err(message) => {
                    return JobOutcome::Done(Box::new(StackContextResult::Errored {
                        index: i,
                        message,
                    }));
                }
            }
        }

        JobOutcome::Done(Box::new(StackContextResult::Ready {
            bundles,
            inputs: ranges,
        }))
    }
}

/// Divide `total` bytes evenly across `n` ranges.
///
/// Each range receives `total / n` bytes. The split is intentionally even
/// rather than proportional to each range's diff size: proportional allocation
/// would need a diff-sizing pre-pass, whereas the even split lets collection
/// stay single-pass. A large range therefore truncates while a small one may
/// leave budget unused — an accepted trade-off for the locked design.
///
/// Shares below [`STACK_RANGE_DIFF_FLOOR`] are zeroed so the range falls back to
/// stat-only collection (`diff_omitted`) rather than receiving an unusably small
/// slice. With an even split this is all-or-nothing: either every range clears
/// the floor or none do.
///
/// Returns a `Vec` of length `n`. When `n == 0` returns an empty `Vec`.
pub fn divide_budget(total: usize, n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let share = total / n;
    let effective = if share < STACK_RANGE_DIFF_FLOOR {
        0
    } else {
        share
    };
    vec![effective; n]
}

// ========================= Per-range read (shared) ==========================

/// Collect a context bundle for a single `base..head` range.
///
/// This is the shared implementation used by both [`ContextJob`] (single-PR)
/// and [`StackContextJob`] (per-range stack collection). It takes the working
/// copy snapshot itself: when `head` is `@` it runs `jj status` (which
/// snapshots), so the parallel reads below can safely pass
/// `--ignore-working-copy`. For any other head the range never touches the
/// working copy and no snapshot is taken.
fn collect_range(jj: &str, base: &str, head: &str, budget: usize) -> Result<ContextBundle, String> {
    let revset = range_revset(base, head)?;
    let status = if head.trim() == "@" {
        run_jj(jj, &["status"])?
    } else {
        String::new()
    };

    let revset: &str = &revset;
    let (changes, diff_stat, diff_git) = std::thread::scope(|s| {
        let changes = s.spawn(move || collect_changes(jj, revset));
        let diff_stat = s.spawn(move || collect_diff_stat(jj, revset));
        let diff_git = (budget > 0).then(|| s.spawn(move || collect_diff_git(jj, revset)));
        (
            join_read(changes, "changes"),
            join_read(diff_stat, "diff --stat"),
            diff_git.map(|handle| join_read(handle, "diff --git")),
        )
    });

    let aggregate = match diff_git {
        Some(diff_git) => {
            let (diff, diff_truncated) = truncate_to_byte_budget(&diff_git?, budget);
            DiffContext {
                diff_stat: diff_stat?,
                diff,
                diff_truncated,
                diff_omitted: false,
            }
        }
        None => DiffContext {
            diff_stat: diff_stat?,
            diff: String::new(),
            diff_truncated: false,
            diff_omitted: true,
        },
    };
    Ok(ContextBundle {
        base: base.to_string(),
        head: head.to_string(),
        status,
        changes: changes?,
        aggregate,
    })
}

/// Single-PR entry point: delegates to `collect_range`, which handles both the
/// snapshot logic and the parallel reads.
fn collect(jj: &str, base: &str, head: &str, budget: usize) -> Result<ContextBundle, String> {
    collect_range(jj, base, head, budget)
}

/// Join a scoped read worker, mapping a thread panic to a context error so a
/// crashed jj read fails generation cleanly instead of unwinding the job.
fn join_read<T>(
    handle: std::thread::ScopedJoinHandle<'_, Result<T, String>>,
    label: &str,
) -> Result<T, String> {
    handle
        .join()
        .unwrap_or_else(|_| Err(format!("context: {label} worker panicked")))
}

fn collect_changes(jj: &str, revset: &str) -> Result<Vec<ChangeContext>, String> {
    // One `jj log --stat` call carries every change's metadata *and* its
    // per-change diff stat: jj appends the `--stat` block after each record's
    // `\x1D` marker. This replaces an N+1 fan-out (one `jj diff --stat` per
    // change) whose cost was dominated by ~0.8s of process-spawn overhead per
    // change — ~20s on a 26-deep stack. Only the per-change stat is kept here;
    // the full hunks live in the aggregate diff.
    const TEMPLATE: &str =
        r#""\x1E" ++ change_id ++ "\x1F" ++ description.lines().join("\x1F") ++ "\x1D\n""#;
    let raw = run_jj(
        jj,
        &[
            "--ignore-working-copy",
            "log",
            "-r",
            revset,
            "--no-graph",
            "--stat",
            "-T",
            TEMPLATE,
        ],
    )?;
    let mut changes = parse_change_log(&raw);
    // jj logs newest-first; reverse to the oldest-to-newest journey the prompt
    // narrates.
    changes.reverse();
    Ok(changes)
}

/// Aggregate diff stat for the whole range. `--ignore-working-copy`: the
/// snapshot was already taken by `jj status` in `collect_range` (or is unneeded
/// when head isn't `@`), so this is a pure read.
fn collect_diff_stat(jj: &str, revset: &str) -> Result<String, String> {
    run_jj(
        jj,
        &["--ignore-working-copy", "diff", "-r", revset, "--stat"],
    )
}

/// Full aggregate diff. `--git` emits standard unified-diff format: more compact
/// than jj's default line-numbered color-words output, and the format LLMs parse
/// most reliably.
fn collect_diff_git(jj: &str, revset: &str) -> Result<String, String> {
    run_jj(
        jj,
        &["--ignore-working-copy", "diff", "-r", revset, "--git"],
    )
}

/// Parse the combined `jj log --stat` output. Each record starts with `\x1E`,
/// holds `change_id`, the subject, and body lines (all `\x1F`-separated) up to
/// a `\x1D` marker, then jj's `--stat` block until the next record. The
/// change_id is consumed only to guard against empty records — it's
/// deliberately kept out of the context shape sent to the model.
fn parse_change_log(raw: &str) -> Vec<ChangeContext> {
    raw.split('\x1E')
        .skip(1)
        .filter_map(|record| {
            let (meta, stat) = record.split_once('\x1D')?;
            let mut fields = meta.split('\x1F');
            let change_id = fields.next()?.trim();
            if change_id.is_empty() {
                return None;
            }
            let subject = fields.next().unwrap_or("").trim().to_string();
            let body = fields.collect::<Vec<_>>().join("\n").trim().to_string();
            Some(ChangeContext {
                subject,
                body,
                diff_stat: stat.trim().to_string(),
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
    fn parses_change_log_with_inline_stats_and_hides_ids() {
        let raw = "\x1Eabc\x1Ffeat: add thing\x1Fbody text\x1D\n2 files changed, 5 insertions(+)\n\x1Edef\x1Ffix: repair\x1F\x1D\n1 file changed, 1 deletion(-)\n";
        let changes = parse_change_log(raw);
        assert_eq!(
            changes,
            vec![
                ChangeContext {
                    subject: "feat: add thing".into(),
                    body: "body text".into(),
                    diff_stat: "2 files changed, 5 insertions(+)".into(),
                },
                ChangeContext {
                    subject: "fix: repair".into(),
                    body: String::new(),
                    diff_stat: "1 file changed, 1 deletion(-)".into(),
                },
            ]
        );
    }

    #[test]
    fn parse_change_log_skips_records_without_change_id() {
        let raw = "\x1E\x1Ffeat: ghost\x1F\x1D\n0 files changed\n";
        assert!(parse_change_log(raw).is_empty());
    }

    // ========================== divide_budget tests ==========================

    #[test]
    fn divide_budget_zero_ranges_returns_empty() {
        assert!(divide_budget(128 * 1024, 0).is_empty());
    }

    #[test]
    fn divide_budget_single_range_receives_full_budget() {
        let budgets = divide_budget(128 * 1024, 1);
        assert_eq!(budgets.len(), 1);
        assert_eq!(budgets[0], 128 * 1024);
    }

    #[test]
    fn divide_budget_splits_evenly_above_floor() {
        // 128 KiB / 4 = 32 KiB each, well above the 4 KiB floor.
        let budgets = divide_budget(128 * 1024, 4);
        assert_eq!(budgets.len(), 4);
        assert!(budgets.iter().all(|&b| b == 32 * 1024));
    }

    #[test]
    fn divide_budget_zeroes_shares_below_floor() {
        // 4 KiB total / 2 ranges = 2 KiB each, below the 4 KiB floor.
        let budgets = divide_budget(4 * 1024, 2);
        assert_eq!(budgets.len(), 2);
        assert!(budgets.iter().all(|&b| b == 0));
    }

    #[test]
    fn divide_budget_exactly_at_floor_is_not_zeroed() {
        // Exactly STACK_RANGE_DIFF_FLOOR per range should NOT be zeroed
        // (the floor check is `< FLOOR`, so equal-to-floor is allowed).
        let budgets = divide_budget(STACK_RANGE_DIFF_FLOOR * 3, 3);
        assert_eq!(budgets.len(), 3);
        assert!(budgets.iter().all(|&b| b == STACK_RANGE_DIFF_FLOOR));
    }

    #[test]
    fn divide_budget_one_below_floor_zeroes_all() {
        // One byte below the floor across two ranges → both zeroed.
        let budgets = divide_budget((STACK_RANGE_DIFF_FLOOR - 1) * 2, 2);
        assert_eq!(budgets.len(), 2);
        assert!(budgets.iter().all(|&b| b == 0));
    }

    // ======================== StackContextResult tests =======================

    #[test]
    fn stack_context_result_ready_holds_bundles_and_inputs() {
        let result = StackContextResult::Ready {
            bundles: Vec::new(),
            inputs: Vec::new(),
        };
        assert!(matches!(
            result,
            StackContextResult::Ready {
                bundles,
                inputs,
            } if bundles.is_empty() && inputs.is_empty()
        ));
    }

    #[test]
    fn stack_context_result_errored_names_failing_index() {
        let result = StackContextResult::Errored {
            index: 2,
            message: "jj failed".into(),
        };
        match result {
            StackContextResult::Errored { index, message } => {
                assert_eq!(index, 2);
                assert_eq!(message, "jj failed");
            }
            _ => panic!("expected Errored"),
        }
    }

    #[test]
    fn stack_context_job_with_no_ranges_returns_empty_ready() {
        // The empty-ranges branch returns before any jj call, so this exercises
        // `run` directly without shelling out. A non-existent binary would only
        // be reached if the early return were missing.
        let job = Box::new(StackContextJob {
            jj_binary: "jj-does-not-exist".into(),
            ranges: Vec::new(),
            total_diff_byte_budget: 128 * 1024,
        });
        match job.run() {
            JobOutcome::Done(any) => {
                let result = any
                    .downcast::<StackContextResult>()
                    .expect("StackContextResult");
                assert_eq!(
                    *result,
                    StackContextResult::Ready {
                        bundles: Vec::new(),
                        inputs: Vec::new(),
                    }
                );
            }
            JobOutcome::Failed(msg) => panic!("expected Done, got Failed({msg})"),
        }
    }
}
